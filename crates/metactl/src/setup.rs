use super::*;

pub(super) fn cmd_setup(
    cli: &Cli,
    args: &SetupArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let config_path = project_config_path(&project_root, cli.config.as_deref());
    if args.import_from.is_some() || args.browse_projects {
        return cmd_setup_import(cli, args);
    }
    let detected_surfaces = detect_existing_surfaces(&project_root);
    let existing_config = if config_path.exists() {
        Some(load_partial_project_config(&config_path).map_err(state_error)?)
    } else {
        None
    };
    let mut targets = setup_targets(&project_root, args, existing_config.as_ref())?;
    if targets.is_empty() && !cli.no_input_enabled() && io::stdin().is_terminal() {
        targets.push("codex-cli".to_string());
    }
    let artifact_policy = setup_artifact_policy(args, existing_config.is_some());
    let next_commands = setup_next_commands(args, &targets);
    let actions = setup_actions(
        args,
        &config_path,
        &targets,
        existing_config.is_some(),
        artifact_policy,
    );

    if args.plan {
        let mut lines = vec!["Setup plan:".to_string()];
        for action in &actions {
            let kind = action["kind"].as_str().unwrap_or("action");
            let summary = action["summary"].as_str().unwrap_or("");
            lines.push(format!("  - {kind}: {summary}"));
        }
        lines.push("Equivalent commands:".to_string());
        for command in &next_commands {
            lines.push(format!("  {command}"));
        }
        return Ok(CommandOutput {
            human: project_human_output(&project_root, lines.join("\n")),
            json: success_json(
                "setup",
                Some(&project_root),
                json!({
                    "plan": true,
                    "ready": !targets.is_empty() || existing_config.is_some(),
                    "config_path": config_path,
                    "detected_surfaces": detected_surfaces.iter().map(|(target, surface)| json!({
                        "target": target,
                        "surface": surface,
                    })).collect::<Vec<_>>(),
                    "targets": targets,
                    "profile_template": args.profile_template.clone(),
                    "artifact_policy": artifact_policy.as_str(),
                    "actions": actions,
                    "next_commands": next_commands,
                }),
            ),
        });
    }

    if let Some(existing) = existing_config {
        let existing_targets = existing.targets.clone();
        if args.artifact_policy.is_some() || args.install_background {
            if (cli.no_input_enabled() || !io::stdin().is_terminal()) && !args.yes {
                let mut err = CliError::new(
                    EXIT_STATE,
                    "Non-interactive setup requires --yes before updating machine or project state.",
                )
                .with_details(next_commands.clone());
                if let Some(obj) = err.json.as_object_mut() {
                    obj.insert("code".to_string(), json!("setup_confirmation_required"));
                    obj.insert("category".to_string(), json!("project_state"));
                    obj.insert("next_commands".to_string(), json!(next_commands));
                }
                return Err(err);
            }
            let artifact_report = if args.artifact_policy.is_some() {
                apply_setup_artifact_policy(&project_root, &config_path, artifact_policy)?
            } else {
                json!({
                    "policy": existing.metadata.get(AGENT_ARTIFACT_POLICY_METADATA_KEY).cloned().unwrap_or_else(|| "not-configured".to_string()),
                    "skipped": true,
                })
            };
            let background_install = if args.install_background {
                Some(cmd_background_install(
                    cli,
                    &BackgroundInstallArgs {
                        scope: BackgroundScopeArg::Project,
                        controller: None,
                        interval_minutes: 60,
                        log_dir: None,
                        label: None,
                        yes: args.yes,
                    },
                )?)
            } else {
                None
            };
            return Ok(CommandOutput {
                human: project_human_output(
                    &project_root,
                    format!(
                        "Setup updated.\nArtifact policy: {}\nBackground refresh: {}\nNext: metactl sync --preview",
                        artifact_policy.as_str(),
                        if background_install.is_some() { "installed" } else { "not installed" }
                    ),
                ),
                json: success_json(
                    "setup",
                    Some(&project_root),
                    json!({
                        "applied": true,
                        "already_configured": true,
                        "config_path": config_path,
                        "targets": existing_targets,
                        "artifact_policy": artifact_policy.as_str(),
                        "artifact_policy_update": artifact_report,
                        "background_install": background_install.map(|output| output.json),
                        "next_commands": ["metactl sync --preview", "metactl status"],
                    }),
                ),
            });
        }
        return Ok(CommandOutput {
            human: project_human_output(
                &project_root,
                format!(
                    "metactl is already configured at {}.\nExisting targets: {}\nNext: metactl status",
                    config_path.display(),
                    if existing_targets.is_empty() {
                        "(none)".to_string()
                    } else {
                        existing_targets.join(", ")
                    }
                ),
            ),
            json: success_json(
                "setup",
                Some(&project_root),
                json!({
                    "applied": false,
                    "already_configured": true,
                    "config_path": config_path,
                    "targets": existing_targets,
                    "artifact_policy": existing.metadata.get(AGENT_ARTIFACT_POLICY_METADATA_KEY).cloned().unwrap_or_else(|| "not-configured".to_string()),
                    "next_commands": ["metactl status", "metactl setup --plan"],
                }),
            ),
        });
    }

    if targets.is_empty() {
        let mut err = CliError::new(
            EXIT_STATE,
            "Setup needs an explicit target before it can write project state.",
        )
        .with_details(next_commands.clone());
        if let Some(obj) = err.json.as_object_mut() {
            obj.insert("code".to_string(), json!("setup_needs_target"));
            obj.insert("category".to_string(), json!("project_state"));
            obj.insert("next_commands".to_string(), json!(next_commands));
        }
        return Err(err);
    }

    if (cli.no_input_enabled() || !io::stdin().is_terminal()) && !args.yes {
        let mut err = CliError::new(
            EXIT_STATE,
            "Non-interactive setup requires --yes with explicit target choices.",
        )
        .with_details(next_commands.clone());
        if let Some(obj) = err.json.as_object_mut() {
            obj.insert("code".to_string(), json!("setup_confirmation_required"));
            obj.insert("category".to_string(), json!("project_state"));
            obj.insert("next_commands".to_string(), json!(next_commands));
        }
        return Err(err);
    }

    let init_args = InitArgs {
        target: targets.clone(),
        role: None,
        policy: None,
        starter_library: args.source.clone(),
        mode: InitMode::BrownfieldAutoDetect,
        detect: false,
        bind_profile: args.bind_profile,
    };
    let init_output = cmd_init(cli, &init_args)?;
    let artifact_report =
        apply_setup_artifact_policy(&project_root, &config_path, artifact_policy)?;
    let background_install = if args.install_background {
        Some(cmd_background_install(
            cli,
            &BackgroundInstallArgs {
                scope: BackgroundScopeArg::Project,
                controller: None,
                interval_minutes: 60,
                log_dir: None,
                label: None,
                yes: args.yes,
            },
        )?)
    } else {
        None
    };
    let mut lines = vec!["Setup applied.".to_string(), init_output.human];
    if artifact_policy != ArtifactPolicyArg::Off {
        lines.push(format!(
            "Portable agent artifacts: {}.",
            artifact_policy.as_str()
        ));
    }
    if background_install.is_some() {
        lines.push("Background refresh installed.".to_string());
    }
    lines.push("Next: metactl ignore fix --plan".to_string());

    Ok(CommandOutput {
        human: lines.join("\n\n"),
        json: success_json(
            "setup",
            Some(&project_root),
            json!({
                "applied": true,
                "config_path": config_path,
                "targets": targets,
                "ran_sync": false,
                "init": init_output.json,
                "artifact_policy": artifact_policy.as_str(),
                "artifact_policy_update": artifact_report,
                "background_install": background_install.map(|output| output.json),
                "next_commands": next_commands,
            }),
        ),
    })
}

fn cmd_setup_import(cli: &Cli, args: &SetupArgs) -> std::result::Result<CommandOutput, CliError> {
    if args.import_from.is_some() && args.browse_projects {
        return Err(project_import_error(
            "conflicting_setup_import_selectors",
            "`--import-from` and `--browse-projects` cannot be used together.",
            vec!["Choose a direct source or the interactive browser.".to_string()],
        ));
    }
    if let Some(source) = args.import_from.as_deref() {
        let options = ProjectImportPlanOptions {
            source,
            mode: args.import_mode,
            fields: args.import_fields.as_deref(),
            include_public_sources: args.include_public_sources,
            include_private_sources: args.include_private_sources,
            search_roots: &[],
            include_unready: false,
        };
        if args.plan {
            cmd_project_import_plan(cli, &options, "setup")
        } else {
            cmd_project_import_apply(
                cli,
                &options,
                ProjectImportApplyMode::Create,
                args.yes,
                "setup",
            )
        }
    } else {
        let browse_args = ProjectImportBrowseArgs {
            mode: args.import_mode,
            fields: args.import_fields.clone(),
            include_public_sources: args.include_public_sources,
            include_private_sources: args.include_private_sources,
            apply: !args.plan,
            merge: false,
            replace: false,
            search_root: Vec::new(),
            yes: args.yes,
        };
        cmd_project_import_browse(cli, &browse_args, "setup")
    }
}

fn setup_targets(
    project_root: &Path,
    args: &SetupArgs,
    existing_config: Option<&metactl::project::PartialProjectConfig>,
) -> std::result::Result<Vec<String>, CliError> {
    if !args.target.is_empty() {
        return expand_setup_targets(&args.target).map_err(state_error);
    }
    if let Some(config) = existing_config {
        if !config.targets.is_empty() {
            return Ok(config.targets.clone());
        }
    }
    let detected = detect_existing_surfaces(project_root);
    if !detected.is_empty() {
        return Ok(unique_strings(
            detected
                .into_iter()
                .map(|(target, _)| target)
                .collect::<Vec<_>>(),
        ));
    }
    Ok(Vec::new())
}

fn expand_setup_targets(requested: &[String]) -> Result<Vec<String>> {
    let mut targets = Vec::new();
    for target in requested {
        if target == "all" {
            targets.extend(IGNORE_TARGETS.iter().map(|item| item.to_string()));
        } else {
            targets.push(target.clone());
        }
    }
    Ok(unique_strings(targets))
}

fn setup_next_commands(args: &SetupArgs, targets: &[String]) -> Vec<String> {
    let default_targets;
    let target_arg = if targets.is_empty() {
        default_targets = vec!["codex-cli".to_string()];
        repeated_target_args(&default_targets)
    } else {
        repeated_target_args(targets)
    };
    let mut ignore_fix = format!(
        "metactl ignore fix --plan --scope {}",
        ignore_scope_label(args.ignore_scope)
    );
    if args.include_private_sources {
        ignore_fix.push_str(" --include-private-sources");
    }
    if args.include_lock {
        ignore_fix.push_str(" --include-lock");
    }
    let artifact_arg = args
        .artifact_policy
        .map(|policy| format!(" --artifact-policy {}", policy.as_str()))
        .unwrap_or_default();
    let mut commands = vec![
        format!("metactl setup{target_arg}{artifact_arg} --yes"),
        format!("metactl setup{target_arg}{artifact_arg} --plan"),
        ignore_fix,
    ];
    if !args.no_background {
        commands.push("metactl background plan --scope project".to_string());
        commands.push("metactl background install --scope project --yes".to_string());
    }
    commands.push("metactl sync --preview".to_string());
    commands
}

fn setup_artifact_policy(args: &SetupArgs, already_configured: bool) -> ArtifactPolicyArg {
    args.artifact_policy.unwrap_or(if already_configured {
        ArtifactPolicyArg::Off
    } else {
        ArtifactPolicyArg::PortableFirst
    })
}

fn setup_actions(
    args: &SetupArgs,
    config_path: &Path,
    targets: &[String],
    already_configured: bool,
    artifact_policy: ArtifactPolicyArg,
) -> Vec<Value> {
    let mut actions = vec![
        json!({
            "kind": "config",
            "path": config_path,
            "summary": if already_configured {
                "preserve existing metactl.yaml"
            } else {
                "create metactl.yaml and metactl.lock.json"
            },
            "targets": targets,
        }),
        json!({
            "kind": "ignore-repair",
            "summary": "recommend plan-first generated-surface ignore repair",
            "scope": ignore_scope_label(args.ignore_scope),
            "include_private_sources": args.include_private_sources,
            "include_lock": args.include_lock,
        }),
    ];
    actions.push(json!({
        "kind": "agent-artifacts",
        "summary": artifact_policy.summary(),
        "policy": artifact_policy.as_str(),
        "pack": if artifact_policy == ArtifactPolicyArg::PortableFirst {
            Value::String(AGENT_ARTIFACT_STEWARDSHIP_PACK.to_string())
        } else {
            Value::Null
        },
    }));
    if !args.no_background {
        actions.push(json!({
            "kind": "background-refresh",
            "summary": "offer report-only usage-ranked surface refresh through the OS scheduler",
            "default": "recommended",
            "install_command": "metactl background install --scope project --yes",
            "opt_out": "metactl setup --no-background",
            "report_only": true,
            "mutates_adapters": false,
        }));
    }
    actions
}

fn apply_setup_artifact_policy(
    project_root: &Path,
    config_path: &Path,
    artifact_policy: ArtifactPolicyArg,
) -> std::result::Result<Value, CliError> {
    let mut config = load_partial_project_config(config_path).map_err(state_error)?;
    let mut packs_changed = false;

    config.metadata.insert(
        AGENT_ARTIFACT_POLICY_METADATA_KEY.to_string(),
        artifact_policy.as_str().to_string(),
    );

    if artifact_policy == ArtifactPolicyArg::PortableFirst
        && !config
            .packs
            .iter()
            .any(|pack| pack == AGENT_ARTIFACT_STEWARDSHIP_PACK)
    {
        config
            .packs
            .push(AGENT_ARTIFACT_STEWARDSHIP_PACK.to_string());
        packs_changed = true;
    }

    write_partial_project_config(config_path, &config).map_err(internal_error)?;
    let lock_path = project_lock_path(project_root);
    if lock_path.exists() {
        let mut lock = load_lock(&lock_path).map_err(internal_error)?;
        lock.config_digest = Some(digest_path(config_path).map_err(internal_error)?);
        lock.updated_at = Some(now_string());
        write_lock(&lock_path, &lock).map_err(internal_error)?;
    }

    Ok(json!({
        "policy": artifact_policy.as_str(),
        "metadata_key": AGENT_ARTIFACT_POLICY_METADATA_KEY,
        "added_pack": if packs_changed {
            Value::String(AGENT_ARTIFACT_STEWARDSHIP_PACK.to_string())
        } else {
            Value::Null
        },
    }))
}
