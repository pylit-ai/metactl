use super::*;
use metactl::SurfaceSelectionMode;
use std::path::Component;

pub(super) const PRIVATE_LOCAL_DESTINATIONS: &[&str] = &[
    "CLAUDE.local.md",
    "GEMINI.local.md",
    "AGENTS.local.md",
    "OPENCLAW.local.md",
    ".cursor/rules/metactl-pack-index.local.mdc",
    ".claude/settings.local.json",
    ".cursor/mcp.json",
    ".gemini/.env",
];

pub(super) fn resolve_git_exclude_path(
    project_root: &Path,
) -> std::result::Result<PathBuf, CliError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "info/exclude",
        ])
        .output()
        .map_err(|e| state_error(e.into()))?;
    if !out.status.success() {
        return Err(state_error(anyhow!("Cannot resolve Git exclude path")));
    }
    let path = PathBuf::from(
        std::str::from_utf8(&out.stdout)
            .map_err(|e| state_error(e.into()))?
            .trim_end_matches('\n'),
    );
    if !path.is_absolute() || !path.ends_with("info/exclude") {
        return Err(state_error(anyhow!("invalid Git exclude path")));
    }
    Ok(path)
}

pub(super) fn managed_ignore_file_has_broad_roots(
    path: &Path,
) -> std::result::Result<bool, CliError> {
    let (bytes, _) = read_ignore_preimage(path)?;
    let Some(bytes) = bytes else { return Ok(false) };
    let text = std::str::from_utf8(&bytes).map_err(|e| state_error(e.into()))?;
    let Some((start, end)) =
        marked_block_span(text, IGNORE_BLOCK_BEGIN, IGNORE_BLOCK_END).map_err(state_error)?
    else {
        return Ok(false);
    };
    Ok(text[start..end]
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#') && !l.starts_with('!'))
        .any(|l| {
            [".agents", ".codex", ".claude", ".cursor", ".gemini"]
                .iter()
                .any(|r| l.trim_start_matches('/').trim_end_matches('/') == *r)
        }))
}

pub(super) fn managed_ignore_has_broad_roots(root: &Path) -> bool {
    [
        root.join(".gitignore"),
        git_local_exclude_path(root).unwrap_or_else(|_| root.join(".git/info/exclude")),
    ]
    .iter()
    .any(|p| managed_ignore_file_has_broad_roots(p).unwrap_or(true))
}

pub(super) fn tracked_generated_roots_json(root: &Path, targets: &[String]) -> Result<Vec<Value>> {
    let roots: BTreeSet<&str> = targets
        .iter()
        .flat_map(|t| match t.as_str() {
            "codex-cli" => vec![".agents", ".codex"],
            "claude-code" => vec![".claude"],
            "cursor" => vec![".cursor"],
            "gemini-cli" => vec![".gemini"],
            _ => vec![],
        })
        .collect();
    if roots.is_empty() {
        return Ok(vec![]);
    }
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--"])
        .args(&roots)
        .output()?;
    if !out.status.success() {
        return Ok(vec![]);
    }
    let files = String::from_utf8(out.stdout)?;
    Ok(roots.iter().filter_map(|root| {
        let files: Vec<_> = files.split('\0').filter(|f| *f == *root || f.starts_with(&format!("{root}/"))).collect();
        (!files.is_empty()).then(|| json!({"root": root, "classification": "tracked-agent-root-ownership-unverified", "file_count": files.len(), "tracked_files": files}))
    }).collect())
}

pub(super) fn planned_proofs(
    cli: &Cli,
    root: &Path,
    overrides: &ConfigOverrides,
    surface_override: Option<SurfaceSelectionMode>,
    apply_override: Option<ApplyMode>,
) -> std::result::Result<
    (
        Vec<metactl::library_registry::ProjectionProof>,
        bool,
        Vec<String>,
    ),
    CliError,
> {
    if !project_config_path(root, cli.config.as_deref()).exists() {
        return Ok((vec![], false, vec![]));
    }
    let context = load_required_context(cli, root)?;
    let Some(registry) = context.registry.as_ref() else {
        return Ok((vec![], false, vec![]));
    };
    let kernel = kernel_from_context(&context).map_err(state_error)?;
    let mut proofs = Vec::new();
    let mut private_state = false;
    let mut retained_paths = Vec::new();
    for target in context.selected_targets(overrides).map_err(state_error)? {
        retained_paths.extend(
            registry
                .retained_private_paths(root, &target.target_ref())
                .map_err(state_error)?,
        );
        let config = context
            .effective_config(&ConfigOverrides {
                targets: vec![target.target_id.clone()],
                ..overrides.clone()
            })
            .map_err(state_error)?;
        let surface_selection_mode = surface_override.clone().or_else(|| {
            config
                .defaults
                .as_ref()
                .and_then(|d| d.surface_selection_mode.clone())
        });
        let graph = kernel
            .resolve(ResolveParams {
                config,
                overlay: context.overlay.clone(),
                available_targets: vec![target.clone()],
                provenance: None,
            })
            .map_err(state_error)?;
        private_state |= registry.graph_requires_private_state(&graph);
        proofs.extend(
            registry
                .projection_proofs(&CompileParams {
                    resolve_graph: graph,
                    apply_mode: apply_override
                        .clone()
                        .unwrap_or_else(|| preferred_apply_mode_for_target(&target, None)),
                    target_capability: target,
                    surface_selection_mode,
                    emit_policy_report: false,
                    durable_staging: false,
                    project_root: Some(root.to_string_lossy().into()),
                })
                .map_err(state_error)?,
        );
    }
    Ok((proofs, private_state, retained_paths))
}

/// Exact private paths supplement the managed rules. Authored negations remain
/// in their original order and still have to pass effective-rule verification.
pub(super) fn add_private_patterns(
    cli: &Cli,
    root: &Path,
    specs: &mut [IgnoreBlockSpec],
) -> std::result::Result<(), CliError> {
    let (proofs, _, retained) = planned_proofs(cli, root, &ConfigOverrides::default(), None, None)?;
    let paths: BTreeSet<_> = proofs
        .into_iter()
        .filter(|p| p.private)
        .map(|p| p.destination)
        .chain(retained)
        .collect();
    // Private pack identifiers must not enter the shareable .gitignore itself.
    for spec in specs
        .iter_mut()
        .filter(|s| s.begin == IGNORE_BLOCK_BEGIN && s.path.ends_with("info/exclude"))
    {
        for path in &paths {
            validate_relative_destination(path).map_err(state_error)?;
            let escaped: String = path
                .chars()
                .flat_map(|c| {
                    if "\\*?[]!# ".contains(c) {
                        vec!['\\', c]
                    } else {
                        vec![c]
                    }
                })
                .collect();
            spec.lines.push(format!("/{escaped}"));
        }
    }
    Ok(())
}

pub(super) fn ensure_private_projection_safe_for_ignore_change(
    cli: &Cli,
    root: &Path,
    prepared: &[PreparedIgnoreWrite],
) -> std::result::Result<(), CliError> {
    let records =
        metactl::materializer::installed_projection_destinations(root).map_err(state_error)?;
    let configured = project_config_path(root, cli.config.as_deref()).exists();
    let strict = configured || !records.is_empty();
    let (proofs, private_state, retained) =
        planned_proofs(cli, root, &ConfigOverrides::default(), None, None)?;
    let mut private: BTreeSet<String> = PRIVATE_LOCAL_DESTINATIONS
        .iter()
        .map(|s| (*s).into())
        .collect();
    private.extend(retained);
    private.extend([
        ".metactl/".into(),
        ".metactl/private-publication-probe".into(),
        "metactl.local.yaml".into(),
    ]);
    private.extend(
        proofs
            .iter()
            .filter(|p| p.private)
            .map(|p| p.destination.clone()),
    );
    let shared: BTreeMap<_, _> = proofs
        .iter()
        .filter(|p| !p.private)
        .map(|p| (p.destination.clone(), p))
        .collect();
    let mut verified = BTreeSet::new();
    for (destination, reference, digest) in &records {
        validate_relative_destination(destination).map_err(state_error)?;
        if let Some(expected) = shared.get(destination) {
            if digest.as_ref() == Some(&expected.digest)
                && projection_bytes_match(
                    root,
                    destination,
                    &expected.digest,
                    Some(&expected.staged_path),
                )?
            {
                verified.insert(destination.clone());
            }
        } else if reference.is_some() || private.contains(destination) {
            private.insert(destination.clone());
        }
    }
    if !ignore_recovery::git_worktree_present(root).map_err(state_error)? {
        let weakening = prepared.iter().any(|w| {
            let Some(original) = w
                .original
                .as_ref()
                .and_then(|b| std::str::from_utf8(b).ok())
            else {
                return false;
            };
            let Some((start, end)) =
                marked_block_span(original, IGNORE_BLOCK_BEGIN, IGNORE_BLOCK_END)
                    .ok()
                    .flatten()
            else {
                return false;
            };
            let updated = String::from_utf8_lossy(&w.updated);
            original[start..end]
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('!'))
                .any(|l| !updated.lines().any(|n| n.trim() == l))
        });
        if strict && weakening {
            return Err(privacy_refusal(
                "a configured project needs a Git worktree before weakening protection".into(),
            ));
        }
        if private_state || proofs.iter().any(|p| p.private) {
            return Err(privacy_refusal(
                "private projections require a Git worktree".into(),
            ));
        }
        for dir in [
            ".agents/skills",
            ".codex/skills",
            ".claude/skills",
            ".cursor/skills",
            ".gemini/skills",
        ] {
            if root.join(dir).exists() {
                return Err(privacy_refusal(format!("generated-looking content under {dir} has no trustworthy projection inventory or Git worktree evidence")));
            }
        }
        return Ok(());
    }
    let requested: Vec<_> = private
        .iter()
        .cloned()
        .chain(records.iter().map(|r| r.0.clone()))
        .collect();
    let writes = prepared
        .iter()
        .map(|w| (w.path.clone(), w.updated.clone()))
        .collect::<Vec<_>>();
    let (git_root, evidence) = metactl::git_privacy::evaluate(root, &requested, &writes)
        .map_err(|e| privacy_refusal(e.to_string()))?;
    let canonical = fs::canonicalize(root).map_err(|e| state_error(e.into()))?;
    let prefix = canonical
        .strip_prefix(&git_root)
        .map_err(|e| state_error(e.into()))?;
    for (repository_path, proof) in evidence {
        let destination = Path::new(&repository_path)
            .strip_prefix(prefix)
            .ok()
            .and_then(Path::to_str);
        let known_private =
            destination.is_some_and(|d| private.contains(d) || retained_local_destination(d));
        if known_private && (!proof.after || proof.tracked) {
            return Err(privacy_refusal(format!("private destination {repository_path} must remain effectively ignored and untracked")));
        }
        // An already tracked, non-generated authored surface is an explicit
        // publication choice. Inventory and generated paths still need byte
        // proof; private classification always takes precedence above.
        let authored_tracked = if proof.tracked {
            if let Some(d) = destination
                .filter(|d| !generated_destination(d) && !records.iter().any(|r| r.0 == *d))
            {
                let path = root.join(d);
                if path.is_file() && !path.is_symlink() {
                    let bytes = fs::read(path).map_err(|e| state_error(e.into()))?;
                    projection_bytes_match(
                        root,
                        d,
                        &format!("sha256:{}", hex::encode(Sha256::digest(&bytes))),
                        None,
                    )?
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };
        if proof.before
            && !proof.after
            && !authored_tracked
            && !destination.is_some_and(|d| verified.contains(d))
        {
            if strict || destination.is_none() || destination.is_some_and(generated_destination) {
                return Err(privacy_refusal(format!(
                    "affected path {repository_path} has no verified public projection bytes"
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn ensure_private_sync_safe(
    cli: &Cli,
    root: &Path,
    overrides: &ConfigOverrides,
    surface_override: Option<SurfaceSelectionMode>,
    apply_override: Option<ApplyMode>,
) -> std::result::Result<(), CliError> {
    let (proofs, private_state, retained) =
        planned_proofs(cli, root, overrides, surface_override, apply_override)?;
    let mut paths: Vec<_> = proofs
        .into_iter()
        .filter(|p| p.private)
        .map(|p| p.destination)
        .chain(retained)
        .collect();
    if private_state || !paths.is_empty() {
        paths.push(".metactl/".into());
    }
    metactl::git_privacy::require_private(root, &paths).map_err(state_error)
}

fn validate_relative_destination(destination: &str) -> Result<()> {
    if destination.is_empty()
        || !Path::new(destination)
            .components()
            .all(|p| matches!(p, Component::Normal(_)))
    {
        return Err(anyhow!("unsafe projection destination: {destination}"));
    }
    Ok(())
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
    .any(|r| destination == *r || destination.starts_with(&format!("{r}/")))
}

fn retained_local_destination(destination: &str) -> bool {
    destination == ".metactl"
        || destination.starts_with(".metactl/")
        || PRIVATE_LOCAL_DESTINATIONS.contains(&destination)
}

fn projection_bytes_match(
    root: &Path,
    destination: &str,
    digest: &str,
    expected_staged: Option<&str>,
) -> std::result::Result<bool, CliError> {
    let path = root.join(destination);
    let mut parent = path.parent();
    while let Some(p) = parent {
        if p == root {
            break;
        }
        if p.is_symlink() {
            return Ok(false);
        }
        parent = p.parent();
    }
    let link = if path.is_symlink() {
        let Some(expected) = expected_staged else {
            return Ok(false);
        };
        let resolved = fs::canonicalize(&path).map_err(|e| state_error(e.into()))?;
        if resolved != fs::canonicalize(root.join(expected)).map_err(|e| state_error(e.into()))? {
            return Ok(false);
        }
        let generated =
            fs::canonicalize(root.join(".metactl/generated")).map_err(|e| state_error(e.into()))?;
        if !resolved.starts_with(generated) {
            return Ok(false);
        }
        Some(fs::read_link(&path).map_err(|e| state_error(e.into()))?)
    } else {
        None
    };
    match fs::read(&path) {
        Ok(bytes) if format!("sha256:{}", hex::encode(Sha256::digest(&bytes))) != digest => {
            return Ok(false)
        }
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(state_error(e.into())),
    }
    let index = Command::new("git")
        .arg("-C")
        .arg(metactl::git_privacy::git_path_argument(root))
        .args(["ls-files", "--cached", "-z", "--", destination])
        .output()
        .map_err(|e| state_error(e.into()))?;
    if !index.status.success() {
        return Ok(false);
    }
    if !index.stdout.is_empty() {
        let out = Command::new("git")
            .arg("-C")
            .arg(metactl::git_privacy::git_path_argument(root))
            .args(["show", &format!(":./{destination}")])
            .output()
            .map_err(|e| state_error(e.into()))?;
        if !out.status.success() {
            return Ok(false);
        }
        if let Some(link) = link {
            if out.stdout != link.to_string_lossy().as_bytes() {
                return Ok(false);
            }
        } else if format!("sha256:{}", hex::encode(Sha256::digest(&out.stdout))) != digest {
            return Ok(false);
        }
    }
    Ok(true)
}

fn privacy_refusal(detail: String) -> CliError {
    let mut err = CliError::new(EXIT_STATE, format!("Refusing ignore change: {detail}. Preserve existing ignore rules and resolve private projection protection before retrying."));
    if let Some(object) = err.json.as_object_mut() {
        object.insert(
            "code".into(),
            json!("private_projection_protection_unproven"),
        );
    }
    err
}
