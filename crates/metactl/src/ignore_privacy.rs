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
        let Some(original) = write.original.as_deref() else {
            return false;
        };
        if original == write.updated.as_slice() {
            return false;
        }
        let Ok(original) = std::str::from_utf8(original) else {
            return true;
        };
        let Ok(updated) = std::str::from_utf8(&write.updated) else {
            return true;
        };
        let Ok(Some((start, end))) =
            marked_block_span(original, IGNORE_BLOCK_BEGIN, IGNORE_BLOCK_END)
        else {
            return false;
        };
        original[start..end]
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('!'))
            .any(|line| {
                !updated
                    .lines()
                    .any(|new| new.trim().trim_start_matches('/') == line.trim_start_matches('/'))
            })
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
    let strict_inventory = context.is_some() || !records.is_empty();
    let mut verified = BTreeSet::new();
    for (destination, pack_ref, digest) in records {
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
        if is_shared && projection_bytes_match(project_root, &destination, digest.as_deref())? {
            verified.insert(destination);
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
    refuse_uninventoried_generated_paths(project_root, prepared, &verified, strict_inventory)?;

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

fn generated_destination(destination: &str) -> bool {
    [
        ".agents/skills",
        ".codex/skills",
        ".codex/commands",
        ".claude/skills",
        ".claude/commands",
        ".cursor/skills",
        ".gemini/skills",
        ".gemini/commands",
    ]
    .iter()
    .any(|root| destination == *root || destination.starts_with(&format!("{root}/")))
}

/// Inspect both working files (including ignored files) and index-only entries.
/// Git identifies the original rule responsible for protection, so custom
/// destinations and targets absent from the current selection are covered too.
fn refuse_uninventoried_generated_paths(
    project_root: &Path,
    prepared: &[PreparedIgnoreWrite],
    verified: &BTreeSet<String>,
    strict_inventory: bool,
) -> std::result::Result<(), CliError> {
    if !ignore_recovery::git_worktree_present(project_root).map_err(state_error)? {
        if strict_inventory {
            return Err(privacy_refusal(
                "a configured project needs a Git worktree to verify every affected path before weakening existing protection; initialize Git and retry".into(),
            ));
        }
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
            match fs::symlink_metadata(path) {
                Ok(_) => return Err(privacy_refusal(format!(
                    "generated-looking content under {relative} has no trustworthy projection inventory or Git worktree evidence"
                ))),
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(state_error(anyhow!("inspect {relative}: {err}"))),
            }
        }
        return Ok(());
    }
    let files = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(["ls-files", "--cached", "--others", "--full-name", "-z"])
        .output()
        .map_err(|err| state_error(anyhow!("inspect affected paths: {err}")))?;
    if !files.status.success() {
        return Err(privacy_refusal("affected-path enumeration failed".into()));
    }
    for raw in files
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let destination = std::str::from_utf8(raw)
            .map_err(|_| privacy_refusal("non-UTF8 affected path cannot be verified".into()))?;
        if retained_local_destination(destination) || verified.contains(destination) {
            continue;
        }
        // Preserve legacy configless authored surfaces. Generated-looking paths
        // still need proof even when no MetaCTL project has been configured.
        if !strict_inventory && !generated_destination(destination) {
            continue;
        }
        let mut probe = Command::new("git")
            .arg("-C")
            .arg(project_root)
            .args(["check-ignore", "--no-index", "--verbose", "-z", "--stdin"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|err| state_error(anyhow!("inspect ignore rule: {err}")))?;
        {
            let mut input = probe
                .stdin
                .take()
                .ok_or_else(|| state_error(anyhow!("missing ignore probe input")))?;
            input
                .write_all(raw)
                .and_then(|_| input.write_all(&[0]))
                .map_err(|err| state_error(anyhow!("write ignore probe: {err}")))?;
        }
        let ignored = probe
            .wait_with_output()
            .map_err(|err| state_error(anyhow!("inspect ignore rule: {err}")))?;
        if ignored.status.code() == Some(1) {
            continue;
        }
        if !ignored.status.success() {
            return Err(privacy_refusal("affected-path ignore probe failed".into()));
        }
        let fields = ignored.stdout.split(|byte| *byte == 0).collect::<Vec<_>>();
        if fields.len() < 4 {
            return Err(privacy_refusal(
                "invalid affected-path ignore evidence".into(),
            ));
        }
        let source = PathBuf::from(String::from_utf8_lossy(fields[0]).as_ref());
        let source = if source.is_absolute() {
            source
        } else {
            project_root.join(source)
        };
        let line = std::str::from_utf8(fields[1])
            .ok()
            .and_then(|line| line.parse::<usize>().ok());
        for write in prepared {
            if source != write.path || write.original.as_deref() == Some(write.updated.as_slice()) {
                continue;
            }
            let original = std::str::from_utf8(write.original.as_deref().unwrap_or_default())
                .map_err(|err| state_error(anyhow!("invalid ignore preimage: {err}")))?;
            let mut in_block = false;
            for (index, text) in original.lines().enumerate() {
                if text.trim() == IGNORE_BLOCK_BEGIN {
                    in_block = true;
                }
                if text.trim() == IGNORE_BLOCK_END {
                    in_block = false;
                }
                if in_block && line == Some(index + 1) && !fields[2].starts_with(b"!") {
                    return Err(privacy_refusal(format!(
                        "affected path {destination} has no verified public projection bytes"
                    )));
                }
            }
        }
    }
    Ok(())
}

fn projection_bytes_match(
    project_root: &Path,
    destination: &str,
    digest: Option<&str>,
) -> std::result::Result<bool, CliError> {
    let Some(digest) = digest else {
        return Ok(false);
    };
    let path = checked_destination(project_root, destination).map_err(state_error)?;
    match fs::read(&path) {
        Ok(bytes) if format!("sha256:{}", hex::encode(Sha256::digest(&bytes))) != digest => {
            return Ok(false)
        }
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(state_error(anyhow!("inspect projection bytes: {err}"))),
    }
    if indexed_destination(project_root, destination)? {
        let output = Command::new("git")
            .arg("-C")
            .arg(project_root)
            .args(["show", &format!(":{destination}")])
            .output()
            .map_err(|err| state_error(anyhow!("inspect indexed bytes: {err}")))?;
        if !output.status.success()
            || format!("sha256:{}", hex::encode(Sha256::digest(&output.stdout))) != digest
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn retained_local_destination(destination: &str) -> bool {
    destination.starts_with(".metactl/")
        || matches!(destination, "CLAUDE.local.md" | "GEMINI.local.md")
}

fn indexed_destination(
    project_root: &Path,
    destination: &str,
) -> std::result::Result<bool, CliError> {
    if !ignore_recovery::git_worktree_present(project_root).map_err(state_error)? {
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
