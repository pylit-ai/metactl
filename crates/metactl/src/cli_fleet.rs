use super::*;

#[cfg(test)]
#[path = "cli_fleet_tests.rs"]
mod tests;

pub(super) fn cmd_fleet(
    cli: &Cli,
    args: &FleetArgs,
) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        Some(FleetCommand::List) => cmd_fleet_list(cli),
        Some(FleetCommand::Status(args)) => cmd_fleet_status(cli, args),
        Some(FleetCommand::Sync(args)) => cmd_fleet_sync(cli, args),
        Some(FleetCommand::Controller(args)) => cmd_fleet_controller(cli, args),
        None => cmd_fleet_status(
            cli,
            &FleetStatusArgs {
                ids: Vec::new(),
                include_disabled: false,
            },
        ),
    }
}

fn cmd_fleet_controller(
    cli: &Cli,
    args: &FleetControllerArgs,
) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        FleetControllerCommand::Init { name, path, force } => {
            validate_fleet_controller_name(name)?;
            let controller_path = resolve_fleet_controller_init_path(cli, name, path.as_deref())?;
            fs::create_dir_all(&controller_path).map_err(|err| {
                internal_error(anyhow!(
                    "create Fleet controller {}: {}",
                    controller_path.display(),
                    err
                ))
            })?;
            ensure_project_layout(&controller_path).map_err(internal_error)?;

            let config_path = project_config_path(&controller_path, cli.config.as_deref());
            if config_path.exists() && !force {
                return Err(CliError::new(
                    EXIT_STATE,
                    format!(
                        "Fleet controller config already exists: {}.\nHint: rerun with --force to replace it, or use `metactl fleet controller set {name} {}`.",
                        config_path.display(),
                        controller_path.display()
                    ),
                ));
            }
            let mut config = default_project_config();
            config.linked_projects = Vec::new();
            config
                .metadata
                .insert("fleet_controller".to_string(), "true".to_string());
            write_project_config(&config_path, &config).map_err(internal_error)?;

            let readme_path = controller_path.join("README.md");
            if !readme_path.exists() || *force {
                atomic_write(
                    &readme_path,
                    fleet_controller_readme(name, &controller_path).as_bytes(),
                )
                .map_err(internal_error)?;
            }

            let context = load_required_context_for_path(cli, &controller_path)?;
            save_fleet_controller_pointer(name, &controller_path)?;
            Ok(CommandOutput {
                human: format!(
                    "Fleet controller `{name}` initialized at {}.\nNext: edit {} and add linked_projects, then run `metactl fleet sync --preview`.\n",
                    controller_path.display(),
                    config_path.display()
                ),
                json: success_json(
                    "fleet",
                    cli.project.as_deref(),
                    json!({
                        "action": "controller-init",
                        "controller": {
                            "id": name,
                            "path": controller_path.to_string_lossy(),
                            "source": "user_default",
                            "config_path": config_path.to_string_lossy(),
                            "registry_digest": current_config_digest(&context).ok(),
                        },
                        "created_files": [
                            config_path.to_string_lossy(),
                            readme_path.to_string_lossy(),
                        ],
                    }),
                ),
            })
        }
        FleetControllerCommand::Show => {
            let settings = load_user_settings();
            let path = user_settings_path();
            let controller = resolve_fleet_controller(cli).ok();
            let human = if let Some(controller) = controller.as_ref() {
                fleet_controller_human_header(controller).join("\n")
            } else {
                format!(
                    "Fleet controller: (none)\nUser settings file: {}",
                    path.as_ref()
                        .map(|item| item.display().to_string())
                        .unwrap_or_else(
                            || "(unavailable — set HOME or XDG_CONFIG_HOME)".to_string()
                        )
                )
            };
            Ok(CommandOutput {
                human: format!("{human}\n"),
                json: success_json(
                    "fleet",
                    cli.project.as_deref(),
                    json!({
                        "action": "controller-show",
                        "settings_path": path,
                        "default_controller": settings.fleet.as_ref().and_then(|fleet| fleet.default_controller.as_deref()),
                        "controller": controller.as_ref().map(fleet_controller_json),
                    }),
                ),
            })
        }
        FleetControllerCommand::List => {
            let settings = load_user_settings();
            let path = user_settings_path();
            let fleet = settings.fleet.unwrap_or_default();
            let controllers = fleet
                .controllers
                .iter()
                .map(|(name, controller)| {
                    let resolved = resolve_user_path(&controller.path);
                    json!({
                        "name": name,
                        "path": controller.path,
                        "resolved_path": resolved.to_string_lossy(),
                        "default": fleet.default_controller.as_deref() == Some(name.as_str()),
                    })
                })
                .collect::<Vec<_>>();
            let mut lines = vec!["Fleet controllers:".to_string()];
            if controllers.is_empty() {
                lines.push("  (none)".to_string());
            }
            for item in &controllers {
                let marker = if item["default"].as_bool().unwrap_or(false) {
                    " *"
                } else {
                    ""
                };
                lines.push(format!(
                    "  {}{} — {}",
                    item["name"].as_str().unwrap_or("?"),
                    marker,
                    item["resolved_path"].as_str().unwrap_or("?")
                ));
            }
            Ok(CommandOutput {
                human: format!("{}\n", lines.join("\n")),
                json: success_json(
                    "fleet",
                    cli.project.as_deref(),
                    json!({
                        "action": "controller-list",
                        "settings_path": path,
                        "default_controller": fleet.default_controller,
                        "controllers": controllers,
                    }),
                ),
            })
        }
        FleetControllerCommand::Set { name, path } => {
            validate_fleet_controller_name(name)?;
            let mut resolved = resolve_user_path(&path.to_string_lossy());
            if !resolved.is_absolute() {
                let cwd = project_root(cli).map_err(internal_error)?;
                resolved = cwd.join(resolved);
            }
            let context = load_required_context_for_path(cli, &resolved)?;
            save_fleet_controller_pointer(name, &resolved)?;
            Ok(CommandOutput {
                human: format!("Fleet controller `{name}` set to {}.\n", resolved.display()),
                json: success_json(
                    "fleet",
                    cli.project.as_deref(),
                    json!({
                        "action": "controller-set",
                        "controller": {
                            "id": name,
                            "path": resolved.to_string_lossy(),
                            "source": "user_default",
                            "config_path": project_config_path(&resolved, cli.config.as_deref()).to_string_lossy(),
                            "registry_digest": current_config_digest(&context).ok(),
                        },
                    }),
                ),
            })
        }
        FleetControllerCommand::ClearDefault => {
            let mut settings = load_user_settings();
            if let Some(fleet) = settings.fleet.as_mut() {
                fleet.default_controller = None;
            }
            save_user_settings(&settings).map_err(internal_error)?;
            Ok(CommandOutput {
                human: "Cleared default Fleet controller.\n".to_string(),
                json: success_json(
                    "fleet",
                    cli.project.as_deref(),
                    json!({
                        "action": "controller-clear-default",
                        "default_controller": Value::Null,
                    }),
                ),
            })
        }
    }
}

fn validate_fleet_controller_name(name: &str) -> std::result::Result<(), CliError> {
    if !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Ok(());
    }
    Err(CliError::new(
        EXIT_STATE,
        format!(
            "Invalid Fleet controller name `{name}`.\nHint: use only ASCII letters, numbers, '.', '_', and '-'."
        ),
    ))
}

fn resolve_fleet_controller_init_path(
    cli: &Cli,
    name: &str,
    path: Option<&Path>,
) -> std::result::Result<PathBuf, CliError> {
    if let Some(path) = path {
        let mut resolved = resolve_user_path(&path.to_string_lossy());
        if !resolved.is_absolute() {
            let cwd = project_root(cli).map_err(internal_error)?;
            resolved = cwd.join(resolved);
        }
        return Ok(resolved);
    }
    let Some(config_dir) = metactl_user_config_dir() else {
        return Err(CliError::new(
            EXIT_STATE,
            "HOME (or XDG_CONFIG_HOME) is not set; cannot create a default Fleet controller path.",
        ));
    };
    Ok(config_dir.join("fleet").join(name))
}

fn save_fleet_controller_pointer(name: &str, path: &Path) -> std::result::Result<(), CliError> {
    let mut settings = load_user_settings();
    let fleet = settings
        .fleet
        .get_or_insert_with(UserFleetSettings::default);
    fleet.controllers.insert(
        name.to_string(),
        UserFleetController {
            path: path.to_string_lossy().to_string(),
        },
    );
    fleet.default_controller = Some(name.to_string());
    save_user_settings(&settings).map_err(internal_error)
}

fn fleet_controller_readme(name: &str, path: &Path) -> String {
    format!(
        r#"# metactl Fleet Controller: {name}

This directory is an explicit local Fleet controller.

- `metactl.yaml` owns the `linked_projects` registry.
- User-global config stores only a pointer to this directory.
- `metactl fleet sync --preview` is the default safe review command.
- `metactl --yes --no-input fleet sync --apply` applies across selected projects.

Add projects manually:

```yaml
linked_projects:
  - id: example
    path: /path/to/repo
```

Controller path: {path}
"#,
        path = path.display()
    )
}

fn cmd_fleet_list(cli: &Cli) -> std::result::Result<CommandOutput, CliError> {
    let controller = resolve_fleet_controller(cli)?;
    let projects =
        fleet_projects_for_output(&controller.project_root, &controller.context.config_file);
    let project_json = projects
        .iter()
        .map(fleet_project_list_json)
        .collect::<Vec<_>>();
    let mut lines = fleet_controller_human_header(&controller);
    lines.push("Fleet projects:".to_string());
    if project_json.is_empty() {
        lines.push("  (none configured)".to_string());
    }
    for project in &project_json {
        lines.push(format!(
            "  {:<18} {:<14} {}",
            project["id"].as_str().unwrap_or("?"),
            project["status"].as_str().unwrap_or("?"),
            project["path"].as_str().unwrap_or("?")
        ));
    }
    Ok(CommandOutput {
        human: project_human_output(&controller.project_root, lines.join("\n")),
        json: success_json(
            "fleet",
            Some(&controller.project_root),
            json!({
                "action": "list",
                "controller": fleet_controller_json(&controller),
                "projects": project_json,
            }),
        ),
    })
}

fn cmd_fleet_status(
    cli: &Cli,
    args: &FleetStatusArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let controller = resolve_fleet_controller(cli)?;
    let projects = select_fleet_projects(
        &controller.project_root,
        &controller.context.config_file,
        &args.ids,
        args.include_disabled,
    )?;
    let statuses = projects
        .iter()
        .map(fleet_project_status_json)
        .collect::<Vec<_>>();
    let mut lines = fleet_controller_human_header(&controller);
    lines.push("Fleet status:".to_string());
    for status in &statuses {
        lines.push(format!(
            "  {:<18} {:<14} {}",
            status["id"].as_str().unwrap_or("?"),
            status["status"].as_str().unwrap_or("?"),
            status["path"].as_str().unwrap_or("?")
        ));
    }
    append_fleet_codex_skill_scope_note(&mut lines);
    Ok(CommandOutput {
        human: project_human_output(&controller.project_root, lines.join("\n")),
        json: success_json(
            "fleet",
            Some(&controller.project_root),
            json!({
                "action": "status",
                "controller": fleet_controller_json(&controller),
                "projects": statuses,
                "scope_note": CODEX_FLEET_SCOPE_NOTE,
            }),
        ),
    })
}

fn cmd_fleet_sync(cli: &Cli, args: &FleetSyncArgs) -> std::result::Result<CommandOutput, CliError> {
    let controller = resolve_fleet_controller(cli)?;
    let apply = args.apply;
    if apply && !(cli.yes && cli.no_input_enabled()) {
        return Err(CliError::new(
            EXIT_STATE,
            "fleet sync --apply requires explicit --yes --no-input confirmation",
        ));
    }
    let projects = select_fleet_projects(
        &controller.project_root,
        &controller.context.config_file,
        &args.ids,
        args.include_disabled,
    )?;
    let mut results = Vec::new();
    for project in &projects {
        let mut result = linked_project_json(project);
        if project.status != LinkedProjectStatus::Ready {
            result["result"] = json!("skipped");
            results.push(result);
            continue;
        }
        let fleet_sync_adopt = match fleet_sync_adopt_for_project(project) {
            Ok(mode) => mode,
            Err(err) => {
                result["status"] = json!("failed");
                result["result"] = json!("invalid_config");
                result["message"] = json!(err.to_string());
                results.push(result);
                continue;
            }
        };
        result["fleet_sync_adopt"] = json!(fleet_sync_adopt_label(fleet_sync_adopt));
        if !apply {
            result["status"] = json!("planned");
            result["result"] = json!("preview");
            result["planned_command"] = json!(fleet_sync_command_label(fleet_sync_adopt));
            attach_codex_skill_visibility(&mut result, &project.path);
            results.push(result);
            continue;
        }
        if git_worktree_dirty(&project.path) {
            result["worktree_dirty"] = json!(true);
            result["warnings"] = json!([DIRTY_WORKTREE_WARNING]);
        }
        match run_project_sync(project, fleet_sync_adopt) {
            Ok(sync_json) => {
                result["status"] = json!("applied");
                result["result"] = json!("applied");
                result["sync"] = sync_json;
            }
            Err(message) => {
                result["status"] = json!("failed");
                result["result"] = json!("sync_failed");
                result["message"] = json!(message);
            }
        }
        attach_codex_skill_visibility(&mut result, &project.path);
        results.push(result);
    }
    if apply {
        write_fleet_sync_log(&controller.project_root, &results).map_err(internal_error)?;
    }
    let failed = results.iter().any(|item| item["status"] == "failed");
    let mut lines = fleet_controller_human_header(&controller);
    lines.push(if apply {
        "Fleet sync applied:".to_string()
    } else {
        "Fleet sync preview:".to_string()
    });
    for item in &results {
        lines.push(format!(
            "  {:<18} {:<14} {}",
            item["id"].as_str().unwrap_or("?"),
            item["status"].as_str().unwrap_or("?"),
            item["path"].as_str().unwrap_or("?")
        ));
        if item["worktree_dirty"] == true {
            lines.push(format!("    Warning: {DIRTY_WORKTREE_WARNING}"));
        }
    }
    append_fleet_codex_skill_scope_note(&mut lines);
    let mut json_payload = success_json(
        "fleet",
        Some(&controller.project_root),
        json!({
            "action": "sync",
            "controller": fleet_controller_json(&controller),
            "preview": !apply,
            "projects": results,
            "scope_note": CODEX_FLEET_SCOPE_NOTE,
        }),
    );
    if results.iter().any(|item| item["worktree_dirty"] == true) {
        json_payload["worktree_dirty"] = json!(true);
        json_payload["warnings"] = json!([DIRTY_WORKTREE_WARNING]);
    }
    if failed {
        let details = fleet_sync_failure_details(&results);
        let mut err = CliError::new(EXIT_STATE, "one or more fleet projects failed");
        json_payload["ok"] = json!(false);
        json_payload["message"] = json!("one or more fleet projects failed");
        json_payload["details"] = json!(details);
        err.json = json_payload;
        err.details = details;
        return Err(err);
    }
    Ok(CommandOutput {
        human: project_human_output(&controller.project_root, lines.join("\n")),
        json: json_payload,
    })
}

fn fleet_sync_failure_details(results: &[Value]) -> Vec<String> {
    let failures: Vec<&Value> = results
        .iter()
        .filter(|item| item["status"] == "failed")
        .collect();
    let mut details: Vec<String> = failures
        .iter()
        .take(20)
        .map(|item| {
            let id = item["id"].as_str().unwrap_or("?");
            let path = item["path"].as_str().unwrap_or("?");
            let result = item["result"].as_str().unwrap_or("failed");
            let message = item["message"]
                .as_str()
                .map(fleet_sync_failure_message_summary)
                .unwrap_or_else(|| "no failure detail returned".to_string());
            format!("{id} ({path}) {result}: {message}")
        })
        .collect();
    let omitted = failures.len().saturating_sub(20);
    if omitted > 0 {
        details.push(format!(
            "{omitted} more failed project(s); rerun with --json for the full fleet payload"
        ));
    }
    details
}

fn fleet_sync_failure_message_summary(message: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(message) {
        let mut parts = Vec::new();
        if let Some(text) = value.get("message").and_then(Value::as_str) {
            parts.push(text.to_string());
        }
        if let Some(details) = value.get("details").and_then(Value::as_array) {
            parts.extend(
                details
                    .iter()
                    .filter_map(Value::as_str)
                    .take(3)
                    .map(ToString::to_string),
            );
        }
        if let Some(findings) = value
            .pointer("/source_audit/findings")
            .and_then(Value::as_array)
        {
            for finding in findings.iter().take(3) {
                let id = finding
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("source-audit");
                let text = finding
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("source audit finding");
                match finding.get("path").and_then(Value::as_str) {
                    Some(path) => parts.push(format!("{id}: {text} ({path})")),
                    None => parts.push(format!("{id}: {text}")),
                }
            }
        }
        if !parts.is_empty() {
            return fleet_sync_single_line(&parts.join("; "), 600);
        }
    }
    fleet_sync_single_line(message, 600)
}

fn fleet_sync_single_line(input: &str, max_chars: usize) -> String {
    let single_line = input.split_whitespace().collect::<Vec<&str>>().join(" ");
    if single_line.is_empty() {
        return "no failure detail returned".to_string();
    }
    if single_line.chars().count() <= max_chars {
        return single_line;
    }
    let keep_chars = max_chars.saturating_sub(3);
    let mut truncated = single_line.chars().take(keep_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

#[derive(Debug)]
pub(super) struct FleetControllerContext {
    pub(super) id: Option<String>,
    pub(super) source: FleetControllerSource,
    pub(super) project_root: PathBuf,
    pub(super) context: metactl::project::ProjectContext,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum FleetControllerSource {
    CommandLine,
    Environment,
    CurrentProject,
    UserDefault,
}

pub(super) fn resolve_fleet_controller(
    cli: &Cli,
) -> std::result::Result<FleetControllerContext, CliError> {
    if cli.project.is_some() || cli.config.is_some() {
        let project_root = project_root(cli).map_err(internal_error)?;
        let context = load_required_context(cli, &project_root)?;
        return Ok(FleetControllerContext {
            id: None,
            source: FleetControllerSource::CommandLine,
            project_root,
            context,
        });
    }

    if let Ok(raw_path) = std::env::var("METACTL_FLEET_CONTROLLER") {
        if !raw_path.trim().is_empty() {
            let project_root = resolve_user_path(raw_path.trim());
            let context = load_required_context_for_path(cli, &project_root)?;
            return Ok(FleetControllerContext {
                id: None,
                source: FleetControllerSource::Environment,
                project_root,
                context,
            });
        }
    }

    let cwd = project_root(cli).map_err(internal_error)?;
    let cwd_config = project_config_path(&cwd, cli.config.as_deref());
    if cwd_config.exists() {
        let context = load_required_context(cli, &cwd)?;
        if !context.config_file.linked_projects.is_empty() {
            return Ok(FleetControllerContext {
                id: None,
                source: FleetControllerSource::CurrentProject,
                project_root: cwd,
                context,
            });
        }
    }

    let settings = load_user_settings();
    if let Some(fleet) = settings.fleet {
        if let Some(default_controller) = fleet.default_controller {
            if let Some(controller) = fleet.controllers.get(&default_controller) {
                let project_root = resolve_user_path(&controller.path);
                let context = load_required_context_for_path(cli, &project_root)?;
                return Ok(FleetControllerContext {
                    id: Some(default_controller),
                    source: FleetControllerSource::UserDefault,
                    project_root,
                    context,
                });
            }
            return Err(CliError::new(
                EXIT_STATE,
                format!(
                    "Fleet default controller `{default_controller}` is not configured.\nHint: run `metactl fleet controller set {default_controller} /path/to/controller`."
                ),
            ));
        }
    }

    Err(CliError::new(
        EXIT_STATE,
        "Fleet controller not found.\nHint: run from a project with linked_projects, pass `--project /path/to/controller`, set METACTL_FLEET_CONTROLLER, or run `metactl fleet controller set personal /path/to/controller`.",
    ))
}

fn fleet_controller_human_header(controller: &FleetControllerContext) -> Vec<String> {
    vec![
        format!(
            "Fleet controller: {}",
            controller.id.as_deref().unwrap_or("(explicit)")
        ),
        format!(
            "Controller source: {}",
            fleet_controller_source_label(controller.source)
        ),
        format!("Controller path: {}", controller.project_root.display()),
    ]
}

fn fleet_controller_json(controller: &FleetControllerContext) -> Value {
    json!({
        "id": controller.id.as_deref(),
        "source": fleet_controller_source_label(controller.source),
        "path": controller.project_root.to_string_lossy(),
        "config_path": project_config_path(&controller.project_root, None).to_string_lossy(),
        "registry_digest": current_config_digest(&controller.context).ok(),
    })
}

fn fleet_controller_source_label(source: FleetControllerSource) -> &'static str {
    match source {
        FleetControllerSource::CommandLine => "command_line",
        FleetControllerSource::Environment => "environment",
        FleetControllerSource::CurrentProject => "current_project",
        FleetControllerSource::UserDefault => "user_default",
    }
}

pub(super) fn fleet_projects_for_output(
    project_root: &Path,
    config: &ProjectConfigFile,
) -> Vec<LinkedProject> {
    metactl::project::discover_linked_projects(project_root, config)
}

fn select_fleet_projects(
    project_root: &Path,
    config: &ProjectConfigFile,
    ids: &[String],
    include_disabled: bool,
) -> std::result::Result<Vec<LinkedProject>, CliError> {
    let projects = fleet_projects_for_output(project_root, config);
    let selected = projects
        .into_iter()
        .filter(|project| ids.is_empty() || ids.iter().any(|id| id == &project.id))
        .filter(|project| include_disabled || project.status != LinkedProjectStatus::Disabled)
        .collect::<Vec<_>>();
    if !ids.is_empty() {
        let found = selected
            .iter()
            .map(|project| project.id.as_str())
            .collect::<BTreeSet<_>>();
        let missing = ids
            .iter()
            .filter(|id| !found.contains(id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(CliError::new(
                EXIT_STATE,
                format!("linked project id(s) not found: {}", missing.join(", ")),
            ));
        }
    }
    Ok(selected)
}

fn linked_project_json(project: &LinkedProject) -> Value {
    json!({
        "id": project.id,
        "path": project.path.to_string_lossy(),
        "config_path": project.config_path.to_string_lossy(),
        "profile": project.profile,
        "status": linked_project_status_label(project.status),
    })
}

fn fleet_project_list_json(project: &LinkedProject) -> Value {
    let mut value = linked_project_json(project);
    if project.status == LinkedProjectStatus::Ready {
        if let Err(err) =
            load_project_context(&project.path, None, project.profile.as_deref(), None)
        {
            value["status"] = json!("invalid_config");
            value["result"] = json!("invalid_config");
            value["message"] = json!(err.to_string());
            let details = error_details(&err);
            if !details.is_empty() {
                value["details"] = json!(details);
            }
        }
    }
    value
}

fn fleet_project_status_json(project: &LinkedProject) -> Value {
    let mut value = linked_project_json(project);
    if project.status == LinkedProjectStatus::Ready {
        match load_project_context(&project.path, None, project.profile.as_deref(), None) {
            Ok(context) => {
                let fleet_sync_adopt = fleet_sync_adopt_from_context(&context);
                let stale = metactl::project::lock_stale_reason(&context).ok().flatten();
                value["lock_stale"] = json!(stale.is_some());
                value["stale_reason"] = json!(stale);
                value["targets"] = json!(context.config_file.targets);
                value["packs"] = json!(context.config_file.packs);
                value["fleet_sync_adopt"] = json!(fleet_sync_adopt_label(fleet_sync_adopt));
                value["needs_sync"] =
                    json!(context.lock.targets.is_empty() || value["lock_stale"] == true);
                attach_codex_skill_visibility(&mut value, &project.path);
            }
            Err(err) => {
                value["status"] = json!("invalid_config");
                value["result"] = json!("invalid_config");
                value["message"] = json!(err.to_string());
                let details = error_details(&err);
                if !details.is_empty() {
                    value["details"] = json!(details);
                }
            }
        }
    }
    value
}

fn attach_codex_skill_visibility(value: &mut Value, project_root: &Path) {
    if let Ok(visibility) = codex_skill_visibility_json(project_root) {
        value["skill_visibility"] = visibility;
    }
}

fn append_fleet_codex_skill_scope_note(lines: &mut Vec<String>) {
    lines.push(format!("  Codex skill scope: {CODEX_FLEET_SCOPE_NOTE}"));
    lines.push("  next: metactl skills add <repo-skill-path> --scope user".to_string());
}

pub(super) fn linked_project_status_label(status: LinkedProjectStatus) -> &'static str {
    match status {
        LinkedProjectStatus::Ready => "ready",
        LinkedProjectStatus::Disabled => "disabled",
        LinkedProjectStatus::MissingPath => "missing_path",
        LinkedProjectStatus::MissingConfig => "missing_config",
    }
}

fn run_project_sync(
    project: &LinkedProject,
    fleet_sync_adopt: FleetSyncAdoptMode,
) -> std::result::Result<Value, String> {
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let mut command = Command::new(exe);
    command
        .arg("--json")
        .arg("--yes")
        .arg("--no-input")
        .arg("--project")
        .arg(&project.path);
    if let Some(profile) = project.profile.as_ref() {
        command.arg("--profile").arg(profile);
    }
    command.arg("sync");
    if fleet_sync_adopt == FleetSyncAdoptMode::Patch {
        command.arg("--adopt").arg("patch");
    }
    let output = command.output().map_err(|err| err.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Err(if stderr.is_empty() { stdout } else { stderr });
    }
    serde_json::from_slice(&output.stdout).map_err(|err| err.to_string())
}

fn fleet_sync_adopt_for_project(project: &LinkedProject) -> Result<FleetSyncAdoptMode> {
    let context = load_project_context(&project.path, None, project.profile.as_deref(), None)
        .with_context(|| format!("load linked project {}", project.id))?;
    Ok(fleet_sync_adopt_from_context(&context))
}

fn fleet_sync_adopt_from_context(context: &metactl::project::ProjectContext) -> FleetSyncAdoptMode {
    context
        .config_file
        .defaults
        .as_ref()
        .and_then(|defaults| defaults.fleet_sync_adopt)
        .unwrap_or(FleetSyncAdoptMode::Patch)
}

fn fleet_sync_adopt_label(mode: FleetSyncAdoptMode) -> &'static str {
    match mode {
        FleetSyncAdoptMode::Patch => "patch",
        FleetSyncAdoptMode::Refuse => "refuse",
    }
}

fn fleet_sync_command_label(mode: FleetSyncAdoptMode) -> &'static str {
    match mode {
        FleetSyncAdoptMode::Patch => "metactl sync --adopt patch",
        FleetSyncAdoptMode::Refuse => "metactl sync",
    }
}

fn write_fleet_sync_log(project_root: &Path, results: &[Value]) -> Result<()> {
    let log_dir = project_root.join(".metactl").join("logs");
    fs::create_dir_all(&log_dir).with_context(|| format!("create {}", log_dir.display()))?;
    let entry = json!({
        "timestamp": fleet_timestamp(),
        "metactl_version": env!("CARGO_PKG_VERSION"),
        "projects": results.iter().map(redact_fleet_log_project).collect::<Vec<_>>(),
    });
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("fleet-sync.jsonl"))
        .context("open fleet sync log")?;
    use std::io::Write as _;
    writeln!(file, "{}", entry).context("write fleet sync log")
}

fn redact_fleet_log_project(project: &Value) -> Value {
    json!({
        "id": project["id"],
        "status": project["status"],
        "result": project["result"],
        "profile": project["profile"],
    })
}

fn fleet_timestamp() -> String {
    format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or_default()
    )
}
