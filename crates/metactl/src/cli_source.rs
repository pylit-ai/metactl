use super::*;

// --- Source management ---

pub(super) fn cmd_source(
    cli: &Cli,
    args: &SourceArgs,
) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        Some(SourceCommand::List) | None => cmd_source_list(cli),
        Some(SourceCommand::Add(add_args)) => cmd_source_add(cli, add_args),
        Some(SourceCommand::Sync(sync_args)) => cmd_source_sync(cli, sync_args),
        Some(SourceCommand::Remove(remove_args)) => cmd_source_remove(cli, remove_args),
    }
}

fn cmd_source_list(cli: &Cli) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_required_context(cli, &project_root)?;

    let mut sources = Vec::new();

    for source in &context.config_file.sources {
        sources.push(source_record_json(source, "config"));
    }

    // Backward-compatible sources from config metadata (metadata.source.*)
    for (key, value) in &context.config_file.metadata {
        if let Some(name) = key.strip_prefix("source.") {
            sources.push(json!({
                "id": name,
                "name": name,
                "type": "local",
                "path": value,
                "origin": "config",
            }));
        }
    }

    // Auto-discovered import roots
    let import_roots = metactl::project::discover_import_roots();
    for root in &import_roots {
        sources.push(json!({
            "name": root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "imports".to_string()),
            "path": root.to_string_lossy(),
            "origin": "auto-discovered",
        }));
    }

    // Starter library roots
    for lib_root in &context.library_roots {
        sources.push(json!({
            "name": lib_root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "library".to_string()),
            "path": lib_root.to_string_lossy(),
            "origin": "starter-library",
        }));
    }

    let mut lines = vec!["Sources:".to_string()];
    if sources.is_empty() {
        lines.push("  (none configured or discovered)".to_string());
    } else {
        for src in &sources {
            let name = src["name"].as_str().unwrap_or("?");
            let path = src["path"].as_str().unwrap_or("?");
            let origin = src["origin"].as_str().unwrap_or("?");
            lines.push(format!("  {:<20} {} ({})", name, path, origin));
        }
    }

    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "source",
            Some(&project_root),
            json!({
                "action": "list",
                "sources": sources,
            }),
        ),
    })
}

fn cmd_source_add(cli: &Cli, args: &SourceAddArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let config_path = project_config_path(&project_root, cli.config.as_deref());
    if !config_path.exists() {
        return Err(missing_config_error(cli, &project_root));
    }

    let mut raw = load_partial_project_config(&config_path).map_err(internal_error)?;
    let (source_name, location, inferred_name) = source_add_name_and_location(args)?;
    validate_source_id(&source_name)?;
    let inferred_type = args
        .source_type
        .map(Into::into)
        .unwrap_or_else(|| infer_source_type(&location));
    if inferred_type == SourceType::Git && args.ref_.is_none() && !args.allow_floating_ref {
        return Err(CliError::new(
            EXIT_STATE,
            "Git sources require --ref unless --allow-floating-ref is passed.",
        ));
    }

    let path = PathBuf::from(&location);
    if inferred_type == SourceType::Local && !path.exists() {
        if cli.no_input_enabled() {
            return Err(CliError::new(
                EXIT_STATE,
                format!("Source path does not exist: {}", location),
            ));
        }
        eprintln!("Warning: source path does not exist: {}", location);
    }

    if raw.sources.iter().any(|source| source.id == source_name) {
        return Ok(CommandOutput {
            human: project_human_output(
                &project_root,
                format!("Source '{}' already configured.", source_name),
            ),
            json: success_json(
                "source",
                Some(&project_root),
                json!({
                    "action": "add",
                    "name": source_name,
                    "source": raw.sources.iter().find(|source| source.id == source_name).map(|source| source_record_json(source, "config")),
                    "already_configured": true,
                    "inferred_name": inferred_name,
                }),
            ),
        });
    }

    let source = SourceRecord {
        id: source_name.clone(),
        source_type: inferred_type.clone(),
        path: (inferred_type == SourceType::Local).then(|| location.clone()),
        url: (inferred_type == SourceType::Git).then(|| location.clone()),
        ref_: args.ref_.clone(),
        visibility: if args.private {
            SourceVisibility::Private
        } else {
            SourceVisibility::Public
        },
        lock_publicity: args.lock_publicity.into(),
    };
    raw.sources.push(source.clone());
    write_partial_project_config(&config_path, &raw).map_err(internal_error)?;

    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!("Added source '{}' at {}.", source_name, location),
        ),
        json: success_json(
            "source",
            Some(&project_root),
            json!({
                "action": "add",
                "name": source_name,
                "source": source_record_json(&source, "config"),
                "already_configured": false,
                "inferred_name": inferred_name,
            }),
        ),
    })
}

fn cmd_source_sync(
    cli: &Cli,
    args: &SourceSyncArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_required_context(cli, &project_root)?;
    let Some(name) = args.name.as_deref() else {
        return cmd_source_sync_all(&project_root, &context, args.force);
    };
    let source = find_source_record(&context.config_file, name).ok_or_else(|| {
        let configured = context
            .config_file
            .sources
            .iter()
            .map(|source| source.id.clone())
            .collect::<Vec<_>>();
        let mut details = vec!["Next: metactl source list".to_string()];
        if configured.is_empty() {
            details.push("Next: metactl source add <name> <path-or-git-url> --private".to_string());
        } else {
            details.push(format!("Configured sources: {}", configured.join(", ")));
        }
        CliError::new(EXIT_STATE, format!("Source '{}' is not configured.", name))
            .with_details(details)
    })?;
    let synced = sync_source(&project_root, &source, args.force)?;

    let mut public_lock = context.lock.clone();
    upsert_locked_source(&mut public_lock.sources, redacted_locked_source(&synced));
    write_lock(&context.lock_path, &public_lock).map_err(internal_error)?;

    let private_lock = PrivateSourceLock {
        sources: vec![synced.clone()],
    };
    write_private_source_lock(&private_source_lock_path(&project_root), &private_lock)
        .map_err(internal_error)?;

    Ok(CommandOutput {
        human: project_human_output(&project_root, format!("Synced source '{}'.", source.id)),
        json: success_json(
            "source",
            Some(&project_root),
            json!({
                "action": "sync",
                "source": locked_source_json(&synced, false),
                "sources": [locked_source_json(&synced, false)],
            }),
        ),
    })
}

fn cmd_source_sync_all(
    project_root: &Path,
    context: &metactl::project::ProjectContext,
    force: bool,
) -> std::result::Result<CommandOutput, CliError> {
    if context.config_file.sources.is_empty() {
        return Err(
            CliError::new(EXIT_STATE, "No configured sources to sync.").with_details(vec![
                "Next: metactl source list".to_string(),
                "Next: metactl source add <location> --private".to_string(),
            ]),
        );
    }

    let mut public_lock = context.lock.clone();
    let mut private_sources = Vec::new();
    for source in &context.config_file.sources {
        let synced = sync_source(project_root, source, force)?;
        upsert_locked_source(&mut public_lock.sources, redacted_locked_source(&synced));
        private_sources.push(synced);
    }
    write_lock(&context.lock_path, &public_lock).map_err(internal_error)?;
    write_private_source_lock(
        &private_source_lock_path(project_root),
        &PrivateSourceLock {
            sources: private_sources.clone(),
        },
    )
    .map_err(internal_error)?;

    let names = private_sources
        .iter()
        .map(|source| source.id.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Ok(CommandOutput {
        human: project_human_output(
            project_root,
            format!("Synced {} source(s): {}.", private_sources.len(), names),
        ),
        json: success_json(
            "source",
            Some(project_root),
            json!({
                "action": "sync",
                "sources": private_sources
                    .iter()
                    .map(|source| locked_source_json(source, false))
                    .collect::<Vec<_>>(),
            }),
        ),
    })
}

fn cmd_source_remove(
    cli: &Cli,
    args: &SourceRemoveArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let config_path = project_config_path(&project_root, cli.config.as_deref());
    let mut raw = load_partial_project_config(&config_path).map_err(internal_error)?;
    let before = raw.sources.len();
    raw.sources.retain(|source| source.id != args.name);
    raw.metadata.remove(&format!("source.{}", args.name));
    write_partial_project_config(&config_path, &raw).map_err(internal_error)?;
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            if raw.sources.len() == before {
                format!("Source '{}' was not configured.", args.name)
            } else {
                format!("Removed source '{}'.", args.name)
            },
        ),
        json: success_json(
            "source",
            Some(&project_root),
            json!({
                "action": "remove",
                "name": args.name,
                "removed": raw.sources.len() != before,
            }),
        ),
    })
}

impl From<SourceTypeArg> for SourceType {
    fn from(value: SourceTypeArg) -> Self {
        match value {
            SourceTypeArg::Local => SourceType::Local,
            SourceTypeArg::Git => SourceType::Git,
        }
    }
}

impl From<SourceLockPublicityArg> for SourceLockPublicity {
    fn from(value: SourceLockPublicityArg) -> Self {
        match value {
            SourceLockPublicityArg::Public => SourceLockPublicity::Public,
            SourceLockPublicityArg::Private => SourceLockPublicity::Private,
        }
    }
}

fn source_add_name_and_location(
    args: &SourceAddArgs,
) -> std::result::Result<(String, String, bool), CliError> {
    match (&args.name, &args.location) {
        (Some(name), Some(location)) => Ok((name.clone(), location.clone(), false)),
        (Some(location), None) => Ok((infer_source_id(location)?, location.clone(), true)),
        _ => Err(CliError::new(
            EXIT_STATE,
            "Source add requires a location. Usage: metactl source add <location> or metactl source add <name> <location>",
        )
        .with_details(vec![
            "Next: metactl source add ./path/to/library --private".to_string(),
            "Next: metactl source add team-library ./path/to/library --private".to_string(),
        ])),
    }
}

fn infer_source_id(location: &str) -> std::result::Result<String, CliError> {
    let path = PathBuf::from(location);
    if path.exists() {
        if let Some(id) = infer_source_id_from_library_manifest(&path)? {
            validate_source_id(&id)?;
            return Ok(id);
        }
    }
    let trimmed = location.trim_end_matches(&['/', '\\'][..]);
    let basename = trimmed
        .rsplit(['/', '\\', ':'])
        .next()
        .unwrap_or(trimmed)
        .trim_end_matches(".git")
        .to_string();
    if basename.is_empty() {
        return Err(CliError::new(
            EXIT_STATE,
            format!("Could not infer a source id from location: {location}"),
        )
        .with_details(vec![
            "Next: metactl source add <name> <location>".to_string(),
            "Next: add an id field to library.json".to_string(),
        ]));
    }
    validate_source_id(&basename)?;
    Ok(basename)
}

fn infer_source_id_from_library_manifest(
    path: &Path,
) -> std::result::Result<Option<String>, CliError> {
    let manifest = path.join("library.json");
    if !manifest.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&manifest)
        .map_err(|err| internal_error(anyhow!("read {}: {}", manifest.display(), err)))?;
    let value: Value = serde_json::from_str(&raw)
        .map_err(|err| state_error(anyhow!("decode {}: {}", manifest.display(), err)))?;
    Ok(value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(|id| id.to_string()))
}

fn infer_source_type(location: &str) -> SourceType {
    if location.starts_with("git@")
        || location.starts_with("ssh://")
        || location.starts_with("https://")
        || location.starts_with("http://")
    {
        SourceType::Git
    } else {
        SourceType::Local
    }
}

pub(super) fn validate_source_id(id: &str) -> std::result::Result<(), CliError> {
    let valid = !id.is_empty()
        && !id.contains("..")
        && !id.contains('/')
        && !id.contains('\\')
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'));
    if valid {
        Ok(())
    } else {
        Err(CliError::new(
            EXIT_STATE,
            format!(
                "Invalid source id '{}'. Use a simple slug without path separators.",
                id
            ),
        ))
    }
}

fn find_source_record(config: &ProjectConfigFile, id: &str) -> Option<SourceRecord> {
    config
        .sources
        .iter()
        .find(|source| source.id == id)
        .cloned()
        .or_else(|| {
            config
                .metadata
                .get(&format!("source.{id}"))
                .map(|path| SourceRecord {
                    id: id.to_string(),
                    source_type: SourceType::Local,
                    path: Some(path.clone()),
                    url: None,
                    ref_: None,
                    visibility: SourceVisibility::Public,
                    lock_publicity: SourceLockPublicity::Public,
                })
        })
}

fn sync_source(
    project_root: &Path,
    source: &SourceRecord,
    force: bool,
) -> std::result::Result<LockedSource, CliError> {
    match source.source_type {
        SourceType::Local => sync_local_source(source),
        SourceType::Git => sync_git_source(project_root, source, force),
    }
}

fn sync_local_source(source: &SourceRecord) -> std::result::Result<LockedSource, CliError> {
    let path = source
        .path
        .as_ref()
        .ok_or_else(|| CliError::new(EXIT_STATE, "Local source is missing path."))?;
    let root = PathBuf::from(path);
    validate_library_source_root(&root)?;
    Ok(LockedSource {
        id: source.id.clone(),
        source_type: SourceType::Local,
        visibility: source.visibility.clone(),
        lock_publicity: source.lock_publicity.clone(),
        resolved: Some("synced".to_string()),
        path: Some(path.clone()),
        url: None,
        ref_: None,
        resolved_commit: None,
    })
}

fn sync_git_source(
    project_root: &Path,
    source: &SourceRecord,
    force: bool,
) -> std::result::Result<LockedSource, CliError> {
    let url = source
        .url
        .as_ref()
        .ok_or_else(|| CliError::new(EXIT_STATE, "Git source is missing url."))?;
    let requested_ref = source
        .ref_
        .as_ref()
        .ok_or_else(|| CliError::new(EXIT_STATE, "Git source is missing ref."))?;
    let cache_root = project_root
        .join(".metactl")
        .join("cache")
        .join("sources")
        .join(&source.id);
    if cache_root.exists() && force {
        fs::remove_dir_all(&cache_root)
            .map_err(|err| internal_error(anyhow!("remove {}: {}", cache_root.display(), err)))?;
    }
    if !cache_root.exists() {
        if let Some(parent) = cache_root.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| internal_error(anyhow!("create {}: {}", parent.display(), err)))?;
        }
        run_git(&[
            "clone",
            "--quiet",
            url,
            cache_root.to_string_lossy().as_ref(),
        ])?;
    } else if !git_worktree_clean(&cache_root)? {
        return Err(CliError::new(
            EXIT_STATE,
            format!(
                "Source cache {} has local changes. Re-run with --force to replace it.",
                cache_root.display()
            ),
        ));
    } else {
        run_git_in(&cache_root, &["fetch", "--quiet", "--tags", "origin"])?;
    }
    let resolved_commit = git_resolve_requested_ref(&cache_root, requested_ref)?;
    run_git_in(
        &cache_root,
        &["checkout", "--quiet", "--detach", resolved_commit.trim()],
    )?;
    validate_library_source_root(&cache_root)?;
    Ok(LockedSource {
        id: source.id.clone(),
        source_type: SourceType::Git,
        visibility: source.visibility.clone(),
        lock_publicity: source.lock_publicity.clone(),
        resolved: Some("synced".to_string()),
        path: Some(relative_to_project(project_root, &cache_root)),
        url: Some(url.clone()),
        ref_: Some(requested_ref.clone()),
        resolved_commit: Some(resolved_commit.trim().to_string()),
    })
}

fn validate_library_source_root(root: &Path) -> std::result::Result<(), CliError> {
    if !root.join("library.json").exists() {
        return Err(CliError::new(
            EXIT_STATE,
            format!("Source {} is missing library.json.", root.display()),
        ));
    }
    let packs_dir = root.join("packs");
    if !packs_dir.is_dir() {
        return Err(CliError::new(
            EXIT_STATE,
            format!("Source {} is missing packs/.", root.display()),
        ));
    }
    let registry = LibraryRegistry::load_from_roots(&[root.to_path_buf()]).map_err(state_error)?;
    if registry.list_packs().is_empty() {
        return Err(CliError::new(
            EXIT_STATE,
            format!(
                "Source {} does not contain parseable packs.",
                root.display()
            ),
        ));
    }
    Ok(())
}

fn run_git(args: &[&str]) -> std::result::Result<(), CliError> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|err| internal_error(anyhow!("run git: {}", err)))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(CliError::new(
            EXIT_STATE,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

pub(super) fn run_git_in(path: &Path, args: &[&str]) -> std::result::Result<(), CliError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .map_err(|err| internal_error(anyhow!("run git: {}", err)))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(CliError::new(
            EXIT_STATE,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

pub(super) fn git_output_in(path: &Path, args: &[&str]) -> std::result::Result<String, CliError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .map_err(|err| internal_error(anyhow!("run git: {}", err)))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(CliError::new(
            EXIT_STATE,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

fn git_output_in_optional(path: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(super) fn git_resolve_requested_ref(
    path: &Path,
    requested_ref: &str,
) -> std::result::Result<String, CliError> {
    if !requested_ref.contains('/') {
        if let Some(remote_resolved) = git_output_in_optional(
            path,
            &["rev-parse", &format!("origin/{requested_ref}^{{commit}}")],
        ) {
            return Ok(remote_resolved);
        }
    }
    git_output_in(path, &["rev-parse", &format!("{requested_ref}^{{commit}}")])
}

pub(super) fn git_worktree_clean(path: &Path) -> std::result::Result<bool, CliError> {
    git_output_in(path, &["status", "--porcelain"]).map(|output| output.trim().is_empty())
}

fn upsert_locked_source(sources: &mut Vec<LockedSource>, source: LockedSource) {
    if let Some(existing) = sources.iter_mut().find(|item| item.id == source.id) {
        *existing = source;
    } else {
        sources.push(source);
    }
}

fn redacted_locked_source(source: &LockedSource) -> LockedSource {
    if source.lock_publicity == SourceLockPublicity::Private {
        LockedSource {
            id: source.id.clone(),
            source_type: source.source_type.clone(),
            visibility: source.visibility.clone(),
            lock_publicity: source.lock_publicity.clone(),
            resolved: Some("redacted".to_string()),
            path: None,
            url: None,
            ref_: None,
            resolved_commit: None,
        }
    } else {
        source.clone()
    }
}

fn source_record_json(source: &SourceRecord, origin: &str) -> Value {
    json!({
        "id": source.id,
        "name": source.id,
        "type": source_type_label(&source.source_type),
        "path": source.path,
        "url": source.url,
        "ref": source.ref_,
        "visibility": source_visibility_label(&source.visibility),
        "lock_publicity": source_lock_publicity_label(&source.lock_publicity),
        "origin": origin,
    })
}

fn locked_source_json(source: &LockedSource, redacted: bool) -> Value {
    json!({
        "id": source.id,
        "type": source_type_label(&source.source_type),
        "visibility": source_visibility_label(&source.visibility),
        "lock_publicity": source_lock_publicity_label(&source.lock_publicity),
        "status": source.resolved.as_deref().unwrap_or("unknown"),
        "resolved": source.resolved,
        "path": if redacted { None::<String> } else { source.path.clone() },
        "url": if redacted { None::<String> } else { source.url.clone() },
        "ref": if redacted { None::<String> } else { source.ref_.clone() },
        "resolved_commit": if redacted { None::<String> } else { source.resolved_commit.clone() },
    })
}

pub(super) fn source_type_label(value: &SourceType) -> &'static str {
    match value {
        SourceType::Local => "local",
        SourceType::Git => "git",
    }
}

pub(super) fn source_visibility_label(value: &SourceVisibility) -> &'static str {
    match value {
        SourceVisibility::Public => "public",
        SourceVisibility::Private => "private",
    }
}

pub(super) fn source_lock_publicity_label(value: &SourceLockPublicity) -> &'static str {
    match value {
        SourceLockPublicity::Public => "public",
        SourceLockPublicity::Private => "private",
    }
}
