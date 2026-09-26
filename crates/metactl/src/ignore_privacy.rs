use super::*;
use std::path::Component;

pub(super) fn managed_ignore_file_has_broad_roots(
    path: &Path,
) -> std::result::Result<bool, CliError> {
    let (bytes, _) = read_ignore_preimage(path)?;
    let Some(bytes) = bytes else {
        return Ok(false);
    };
    let text = std::str::from_utf8(&bytes)
        .map_err(|err| state_error(anyhow!("{}: {err}", path.display())))?;
    let Some((start, end)) =
        marked_block_span(text, IGNORE_BLOCK_BEGIN, IGNORE_BLOCK_END).map_err(state_error)?
    else {
        return Ok(false);
    };
    Ok(text[start..end]
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#') && !line.starts_with('!'))
        .any(|line| {
            [".agents", ".codex", ".claude", ".cursor", ".gemini"]
                .iter()
                .any(|root| line.contains(root))
        }))
}

pub(super) fn managed_ignore_has_broad_roots(project_root: &Path) -> bool {
    [
        project_root.join(".gitignore"),
        git_local_exclude_path(project_root)
            .unwrap_or_else(|_| project_root.join(".git/info/exclude")),
    ]
    .iter()
    .any(|path| managed_ignore_file_has_broad_roots(path).unwrap_or(true))
}

pub(super) fn tracked_generated_roots_json(
    project_root: &Path,
    targets: &[String],
) -> Result<Vec<Value>> {
    let roots = generated_roots_for_targets(targets);
    if roots.is_empty() || !project_root.join(".git").exists() {
        return Ok(Vec::new());
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .arg("ls-files")
        .arg("-z")
        .arg("--")
        .args(&roots)
        .output()
        .with_context(|| format!("run git ls-files in {}", project_root.display()))?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let files = String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|item| !item.is_empty())
        .map(|item| item.replace('\\', "/"))
        .collect::<Vec<_>>();
    let mut by_root = BTreeMap::<String, Vec<String>>::new();
    for file in files {
        for root in &roots {
            if file == *root || file.starts_with(&format!("{root}/")) {
                by_root.entry(root.clone()).or_default().push(file.clone());
            }
        }
    }
    Ok(by_root
        .into_iter()
        .map(|(root, files)| {
            json!({
                "root": root,
                "classification": "tracked-agent-root-ownership-unverified",
                "file_count": files.len(),
                "tracked_files": files,
            })
        })
        .collect())
}

fn generated_roots_for_targets(targets: &[String]) -> Vec<String> {
    let mut roots = BTreeSet::new();
    for target in targets {
        match target.as_str() {
            "codex-cli" => {
                roots.insert(".codex".to_string());
                roots.insert(".agents".to_string());
            }
            "claude-code" => {
                roots.insert(".claude".to_string());
            }
            "cursor" => {
                roots.insert(".cursor".to_string());
            }
            "gemini-cli" => {
                roots.insert(".gemini".to_string());
            }
            _ => {}
        }
    }
    roots.into_iter().collect()
}

/// A read-only gate shared by plan, install, and fix. Refusal happens before
/// recovery directories or ignore files can be created.
pub(super) fn ensure_private_projection_safe_for_ignore_change(
    cli: &Cli,
    project_root: &Path,
    prepared: &[PreparedIgnoreWrite],
) -> std::result::Result<(), CliError> {
    let weakening = prepared.iter().any(|write| {
        write.original.as_deref() != Some(write.updated.as_slice())
            && managed_ignore_file_has_broad_roots(&write.path).unwrap_or(true)
    });
    if !weakening {
        return Ok(());
    }

    let config_path = project_config_path(project_root, cli.config.as_deref());
    let context = if config_path.exists() {
        Some(load_required_context(cli, project_root)?)
    } else {
        None
    };
    let registry = context.as_ref().and_then(|ctx| ctx.registry.as_ref());
    let records = metactl::materializer::installed_projection_destinations(project_root)
        .map_err(state_error)?;
    if records.is_empty() {
        refuse_uninventoried_generated_paths(project_root)?;
    }
    for (destination, pack_ref) in records {
        validate_relative_destination(&destination).map_err(state_error)?;
        // Only root instruction documents are constructed from shared packs.
        // Every other missing pack reference is unknown provenance.
        if pack_ref.is_none() && shared_root_instruction(&destination) {
            continue;
        }
        let is_shared = pack_ref
            .as_ref()
            .and_then(|reference| {
                registry?.pack_by_id(&reference.id).filter(|pack| {
                    reference.version.as_deref() == Some(pack.manifest.version.as_str())
                })
            })
            .is_some_and(|pack| pack.manifest.visibility_scope == metactl::VisibilityScope::Shared);
        if is_shared {
            continue;
        }
        if retained_local_destination(&destination) {
            checked_destination(project_root, &destination).map_err(state_error)?;
            continue;
        }
        let path = checked_destination(project_root, &destination).map_err(state_error)?;
        let present = match fs::symlink_metadata(&path) {
            Ok(_) => true,
            Err(err) if err.kind() == io::ErrorKind::NotFound => false,
            Err(err) => return Err(state_error(anyhow!("inspect {}: {err}", path.display()))),
        };
        if present || indexed_destination(project_root, &destination)? {
            return Err(privacy_refusal(format!(
                "installed private or unverified projection {} would lose Git protection",
                destination
            )));
        }
    }

    if let Some(context) = context {
        let targets = context
            .selected_targets(&ConfigOverrides::default())
            .map_err(state_error)?;
        let kernel = kernel_from_context(&context).map_err(state_error)?;
        for target in targets {
            if !target.compile_targets.iter().any(|output| {
                !matches!(
                    output.output_kind,
                    metactl::CompileTargetKind::AgentsMd
                        | metactl::CompileTargetKind::ClaudeMd
                        | metactl::CompileTargetKind::OpenclawMd
                )
            }) {
                continue;
            }
            let overrides = ConfigOverrides {
                targets: vec![target.target_id.clone()],
                ..Default::default()
            };
            let config = context.effective_config(&overrides).map_err(state_error)?;
            let graph = kernel
                .resolve(ResolveParams {
                    config,
                    overlay: context.overlay.clone(),
                    available_targets: vec![target],
                    provenance: None,
                })
                .map_err(state_error)?;
            for reference in &graph.activated_pack_refs {
                if graph.pack_visibility.get(&reference.id)
                    != Some(&metactl::VisibilityScope::Shared)
                {
                    return Err(privacy_refusal(format!(
                        "active private or unverified pack {} can reach a nonlocal target",
                        reference.id
                    )));
                }
            }
        }
    }
    Ok(())
}

fn checked_destination(project_root: &Path, destination: &str) -> Result<PathBuf> {
    validate_relative_destination(destination)?;
    let relative = Path::new(destination);
    let mut path = project_root.to_path_buf();
    for part in relative.components() {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(anyhow!("symlinked projection destination: {destination}"));
            }
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(path)
}

fn validate_relative_destination(destination: &str) -> Result<()> {
    let relative = Path::new(destination);
    if destination.is_empty()
        || !relative
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(anyhow!("unsafe projection destination: {destination}"));
    }
    Ok(())
}

fn shared_root_instruction(destination: &str) -> bool {
    matches!(
        destination,
        "AGENTS.md" | "CLAUDE.md" | "GEMINI.md" | "OpenClaw.md"
    )
}

fn refuse_uninventoried_generated_paths(project_root: &Path) -> std::result::Result<(), CliError> {
    for relative in [
        ".agents/skills",
        ".codex/skills",
        ".codex/commands",
        ".claude/skills",
        ".claude/commands",
        ".cursor/skills",
        ".gemini/skills",
        ".gemini/commands",
    ] {
        let path = checked_destination(project_root, relative).map_err(state_error)?;
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                return Err(privacy_refusal(format!(
                "generated-looking content under {relative} has no trustworthy projection inventory"
            )))
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(state_error(anyhow!("inspect {}: {err}", path.display()))),
        }
        if indexed_destination(project_root, relative)? {
            return Err(privacy_refusal(format!(
                "indexed content under {relative} has no trustworthy projection inventory"
            )));
        }
    }
    Ok(())
}

fn retained_local_destination(destination: &str) -> bool {
    destination.starts_with(".metactl/")
        || matches!(destination, "CLAUDE.local.md" | "GEMINI.local.md")
}

fn indexed_destination(
    project_root: &Path,
    destination: &str,
) -> std::result::Result<bool, CliError> {
    if !project_root.join(".git").exists() {
        return Ok(false);
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(["ls-files", "--cached", "-z", "--", destination])
        .output()
        .map_err(|err| state_error(anyhow!("inspect Git index: {err}")))?;
    if !output.status.success() {
        return Err(privacy_refusal(format!(
            "Git index probe failed for {destination}"
        )));
    }
    Ok(!output.stdout.is_empty())
}

fn privacy_refusal(detail: String) -> CliError {
    let mut error = CliError::new(EXIT_STATE, format!(
        "Refusing ignore change: {detail}. Preserve the existing ignore rules or remove private projections through a separately reviewed migration."
    ));
    if let Some(object) = error.json.as_object_mut() {
        object.insert(
            "code".into(),
            json!("private_projection_protection_unproven"),
        );
    }
    error
}
