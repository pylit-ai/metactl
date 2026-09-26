use super::*;

pub(super) fn cmd_fleet_sync(
    cli: &Cli,
    args: &FleetSyncArgs,
) -> std::result::Result<CommandOutput, CliError> {
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
        let member_cli = match member_sync_cli(cli, project) {
            Ok(member) => member,
            Err(err) => {
                result["status"] = json!("failed");
                result["result"] = json!("invalid_config");
                result["message"] = json!(err.to_string());
                results.push(result);
                continue;
            }
        };
        let context = match load_required_context(&member_cli, &project.path) {
            Ok(context) => context,
            Err(err) => {
                result["status"] = json!("failed");
                result["result"] = json!("invalid_config");
                result["message"] = json!(err.message);
                results.push(result);
                continue;
            }
        };
        let fleet_sync_adopt = fleet_sync_adopt_from_context(&context);
        result["profile"] = json!(context.active_profile.as_ref().map(|profile| &profile.name));
        if let Some(profile) = context
            .active_profile
            .as_ref()
            .filter(|profile| profile.digest.is_none())
        {
            result["status"] = json!("failed");
            result["result"] = json!("invalid_config");
            result["message"] = json!(format!(
                "profile `{}` does not exist at {}",
                profile.name,
                profile.path.display()
            ));
            results.push(result);
            continue;
        }
        result["fleet_sync_adopt"] = json!(fleet_sync_adopt_label(fleet_sync_adopt));
        if !apply {
            match plan::preview_project(&project.path, &context, fleet_sync_adopt) {
                Ok(preview) => {
                    result["status"] = json!("planned");
                    result["result"] = json!("preview");
                    result["plan"] = preview;
                }
                Err(err) => {
                    result["status"] = json!("failed");
                    result["result"] = json!("preview_failed");
                    result["message"] = json!(err.to_string());
                }
            }
            result["planned_command"] = json!(fleet_sync_command_label(fleet_sync_adopt));
            attach_codex_skill_visibility(&mut result, &project.path);
            results.push(result);
            continue;
        }
        if git_worktree_dirty(&project.path) {
            result["worktree_dirty"] = json!(true);
            result["warnings"] = json!([DIRTY_WORKTREE_WARNING]);
        }
        match run_project_sync(
            &member_cli,
            project,
            &controller.project_root,
            fleet_sync_adopt,
        ) {
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
    let log_error = if apply {
        write_fleet_sync_log(&controller.project_root, &results)
            .err()
            .map(|err| format!("{err:#}"))
    } else {
        None
    };
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
        if let Some(targets) = item["plan"]["targets"].as_array() {
            for target in targets {
                lines.push(format!(
                    "    {}: {} planned files, {} planned skills ({})",
                    target["target"].as_str().unwrap_or("?"),
                    target["planned_output_files"]
                        .as_array()
                        .map(Vec::len)
                        .unwrap_or(0),
                    target["planned_skill_count"].as_u64().unwrap_or(0),
                    target["apply_mode"].as_str().unwrap_or("?")
                ));
            }
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
            "log_error": log_error,
        }),
    );
    if results.iter().any(|item| item["worktree_dirty"] == true) {
        json_payload["worktree_dirty"] = json!(true);
        json_payload["warnings"] = json!([DIRTY_WORKTREE_WARNING]);
    }
    if failed || log_error.is_some() {
        let mut details = fleet_sync_failure_details(&results);
        if let Some(message) = log_error {
            details.push(format!("Fleet log could not be written: {message}. Member outcomes above remain valid; do not blindly retry applied members."));
        }
        let message = if failed {
            "one or more fleet projects failed"
        } else {
            "fleet members completed, but the fleet log could not be written"
        };
        let mut err = CliError::new(EXIT_STATE, message);
        json_payload["ok"] = json!(false);
        json_payload["message"] = json!(message);
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

fn run_project_sync(
    member_cli: &Cli,
    project: &LinkedProject,
    controller_root: &Path,
    fleet_sync_adopt: FleetSyncAdoptMode,
) -> std::result::Result<Value, String> {
    // The fleet invocation already owns this exact controller lock. Invoke the
    // same sync implementation directly, retaining that guard for the duration.
    // No process identity or environment variable can exempt another writer.
    if fs::canonicalize(&project.path).map_err(|err| err.to_string())?
        == fs::canonicalize(controller_root).map_err(|err| err.to_string())?
    {
        let Commands::Sync(args) = &member_cli.command else {
            unreachable!()
        };
        let mut args = args.clone();
        args.adopt = (fleet_sync_adopt == FleetSyncAdoptMode::Patch).then_some(SyncAdoptArg::Patch);
        return cmd_sync(member_cli, &args)
            .map(|output| output.json)
            .map_err(|err| err.json.to_string());
    }
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let mut command = Command::new(exe);
    command
        .arg("--json")
        .arg("--yes")
        .arg("--no-input")
        .arg("--project")
        .arg(&project.path);
    if let Some(profile) = member_cli.profile.as_ref() {
        command.arg("--profile").arg(profile);
    } else if member_cli.no_profile {
        command.arg("--no-profile");
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

fn member_sync_cli(cli: &Cli, project: &LinkedProject) -> Result<Cli> {
    let mut args = vec![
        std::ffi::OsString::from("metactl"),
        "--json".into(),
        "--yes".into(),
        "--no-input".into(),
        "--project".into(),
        project.path.as_os_str().to_owned(),
    ];
    if let Some(profile) = project.profile.as_ref().or(cli.profile.as_ref()) {
        args.extend(["--profile".into(), profile.into()]);
    } else if cli.no_profile {
        args.push("--no-profile".into());
    }
    args.push("sync".into());
    Cli::try_parse_from(args).map_err(Into::into)
}
