use super::*;

pub(super) fn cmd_skills(
    cli: &Cli,
    args: &SkillsArgs,
) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        SkillsCommand::Add(add_args) => cmd_skills_add(cli, add_args),
        SkillsCommand::List(list_args) => cmd_skills_list(cli, list_args),
        SkillsCommand::Remove(remove_args) => cmd_skills_remove(cli, remove_args),
        SkillsCommand::Audit(audit_args) => cmd_skills_audit(cli, audit_args),
        SkillsCommand::Route(route_args) => cmd_skills_route(cli, route_args),
        SkillsCommand::Select(select_args) => cmd_skills_select(cli, select_args),
    }
}

fn cmd_skills_route(
    cli: &Cli,
    args: &SkillsRouteArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_required_context(cli, &project_root)?;
    let registry = context.registry.as_ref().ok_or_else(|| {
        CliError::new(
            EXIT_STATE,
            "No configured skill library is available for routing.",
        )
    })?;
    let result = registry
        .route_skills(&args.query, args.target.as_deref(), Some(args.limit))
        .map_err(internal_error)?;
    let mut lines = vec![format!("Skill route for \"{}\":", args.query)];
    if result.candidates.is_empty() {
        lines.push("  (no declared skill resources matched)".to_string());
    } else {
        for candidate in &result.candidates {
            let suppression = if candidate.suppressed_reasons.is_empty() {
                String::new()
            } else {
                format!(" [suppressed: {}]", candidate.suppressed_reasons.join(", "))
            };
            lines.push(format!(
                "  {:<32} score {:>3} [{}]{}",
                candidate.skill_name,
                candidate.score,
                candidate.matched_fields.join(", "),
                suppression,
            ));
            if !candidate.surface_ids.is_empty() {
                lines.push(format!(
                    "    surface IDs: {}",
                    candidate.surface_ids.join(", ")
                ));
            }
        }
    }
    lines.push("Read-only: routing does not activate skills or bypass policy checks.".to_string());
    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "skills",
            Some(&project_root),
            json!({"action": "route", "result": result}),
        ),
    })
}

fn cmd_skills_select(
    cli: &Cli,
    args: &SkillsSelectArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_required_context(cli, &project_root)?;
    let registry = context.registry.as_ref().ok_or_else(|| {
        CliError::new(
            EXIT_STATE,
            "No configured skill library is available for surface selection.",
        )
    })?;
    if !registry
        .known_surface_ids()
        .map_err(internal_error)?
        .contains(&args.surface_id)
    {
        return Err(CliError::new(
            EXIT_VALIDATION,
            format!("Unknown skill surface id '{}'.", args.surface_id),
        )
        .with_details(vec![
            "Use `metactl skills route <task> --json` to obtain declared surface ids.".to_string(),
            "Selections are saved only for surfaces present in the configured library.".to_string(),
        ]));
    }

    let local_path = metactl::project::local_config_path(&project_root);
    let mut local = metactl::project::load_local_config(&project_root)
        .map_err(internal_error)?
        .unwrap_or_default();
    let defaults = local.defaults.get_or_insert_with(Default::default);
    defaults.surface_selection_mode = Some(metactl::SurfaceSelectionMode::Auto);
    let selection = defaults
        .auto_surface_selection
        .get_or_insert_with(Default::default);
    let prior = selection.clone();
    match args.mode {
        SkillSelectionModeArg::Select => {
            selection.blocked_surface_ids.remove(&args.surface_id);
            selection.pinned_surface_ids.remove(&args.surface_id);
            selection
                .selected_surface_ids
                .insert(args.surface_id.clone());
        }
        SkillSelectionModeArg::Pin => {
            selection.blocked_surface_ids.remove(&args.surface_id);
            selection.selected_surface_ids.remove(&args.surface_id);
            selection.pinned_surface_ids.insert(args.surface_id.clone());
        }
        SkillSelectionModeArg::Block => {
            selection.selected_surface_ids.remove(&args.surface_id);
            selection.pinned_surface_ids.remove(&args.surface_id);
            selection
                .blocked_surface_ids
                .insert(args.surface_id.clone());
        }
        SkillSelectionModeArg::Clear => {
            selection.selected_surface_ids.remove(&args.surface_id);
            selection.pinned_surface_ids.remove(&args.surface_id);
            selection.blocked_surface_ids.remove(&args.surface_id);
        }
    }
    let resulting = selection.clone();
    ensure_gitignore_entries(&project_root).map_err(internal_error)?;
    write_partial_project_config(&local_path, &local).map_err(internal_error)?;

    let mode = match args.mode {
        SkillSelectionModeArg::Select => "selected",
        SkillSelectionModeArg::Pin => "pinned",
        SkillSelectionModeArg::Block => "blocked",
        SkillSelectionModeArg::Clear => "cleared",
    };
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Surface {} {} in metactl.local.yaml.\nAuto mode is now active for this machine-local project setting.\nNext: metactl explain",
                args.surface_id, mode,
            ),
        ),
        json: success_json(
            "skills",
            Some(&project_root),
            json!({
                "action": "select",
                "surface_id": args.surface_id,
                "decision": mode,
                "config_path": local_path.to_string_lossy(),
                "previous_selection": prior,
                "selection": resulting,
                "next_steps": ["metactl explain", "metactl sync --preview"],
            }),
        ),
    })
}

fn cmd_skills_add(cli: &Cli, args: &SkillsAddArgs) -> std::result::Result<CommandOutput, CliError> {
    ensure_codex_skill_target(&args.target)?;
    if args.scope != SkillScopeArg::User {
        return Err(CliError::new(
            EXIT_STATE,
            "skills add --scope repo is not supported; repo-local skills are generated by metactl sync",
        ));
    }
    let project_root = project_root(cli).map_err(internal_error)?;
    let user_root = codex_user_skill_root_for_command()?;
    let skill_dir = resolve_skill_source_dir(&project_root, &args.path).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Codex skill source was not found.")
            .with_details(error_details(&err))
    })?;
    let skill_md = skill_dir.join("SKILL.md");
    let frontmatter = read_skill_frontmatter(&skill_md).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Agent Skill frontmatter is invalid.")
            .with_details(error_details(&err))
    })?;
    let files = collect_skill_files(&skill_dir).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Agent Skill install safety check failed.")
            .with_details(error_details(&err))
    })?;
    let safety_findings = skill_import_safety_findings(&files, args.allow_executable_scripts);
    if !safety_findings.is_empty() {
        return Err(
            CliError::new(EXIT_VALIDATION, "Agent Skill install was refused.")
                .with_details(safety_findings),
        );
    }

    let install_dir = user_root.join(&frontmatter.name);
    replace_existing_user_skill_dir(&install_dir, args.force)?;
    copy_skill_files(&files, &install_dir).map_err(internal_error)?;
    let digest = skill_tree_digest(&files).map_err(internal_error)?;
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Installed Codex skill '{}' to {}.\nScope: user-global Personal skill root.\nSource: {}",
                frontmatter.name,
                install_dir.display(),
                skill_dir.display()
            ),
        ),
        json: success_json(
            "skills",
            Some(&project_root),
            json!({
                "action": "add",
                "target": args.target,
                "scope": "user",
                "skill": {
                    "name": frontmatter.name,
                    "description": frontmatter.description,
                    "source_path": skill_dir.to_string_lossy(),
                    "installed_path": install_dir.to_string_lossy(),
                    "digest": digest,
                },
                "scope_note": CODEX_SKILL_SCOPE_NOTE,
            }),
        ),
    })
}

fn cmd_skills_list(
    cli: &Cli,
    args: &SkillsListArgs,
) -> std::result::Result<CommandOutput, CliError> {
    ensure_codex_skill_target(&args.target)?;
    let project_root = project_root(cli).map_err(internal_error)?;
    let (scope, root) = match args.scope {
        SkillScopeArg::Repo => ("repo", project_root.join(".agents").join("skills")),
        SkillScopeArg::User => ("user", codex_user_skill_root_for_command()?),
    };
    let skills = discover_codex_skill_entries(&root).map_err(internal_error)?;
    let mut lines = vec![format!("Codex skills ({scope} scope): {}", root.display())];
    if skills.is_empty() {
        lines.push("  (none)".to_string());
    } else {
        for skill in &skills {
            lines.push(format!("  {:<24} {}", skill.name, skill.dir.display()));
        }
    }
    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "skills",
            Some(&project_root),
            json!({
                "action": "list",
                "target": args.target,
                "scope": scope,
                "root": root.to_string_lossy(),
                "count": skills.len(),
                "skills": skills.iter().map(codex_skill_entry_json).collect::<Vec<_>>(),
                "scope_note": CODEX_SKILL_SCOPE_NOTE,
            }),
        ),
    })
}

fn cmd_skills_remove(
    cli: &Cli,
    args: &SkillsRemoveArgs,
) -> std::result::Result<CommandOutput, CliError> {
    ensure_codex_skill_target(&args.target)?;
    if args.scope != SkillScopeArg::User {
        return Err(CliError::new(
            EXIT_STATE,
            "skills remove --scope repo is not supported; remove repo-local generated skills with metactl revert or metactl sync",
        ));
    }
    validate_skill_name(&args.name).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Codex skill name is invalid.")
            .with_details(error_details(&err))
    })?;
    let project_root = project_root(cli).map_err(internal_error)?;
    let user_root = codex_user_skill_root_for_command()?;
    let skill_dir = user_root.join(&args.name);
    ensure_removable_user_skill_dir(&skill_dir)?;
    fs::remove_dir_all(&skill_dir).map_err(internal_error)?;
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Removed Codex skill '{}' from {}.",
                args.name,
                skill_dir.display()
            ),
        ),
        json: success_json(
            "skills",
            Some(&project_root),
            json!({
                "action": "remove",
                "target": args.target,
                "scope": "user",
                "name": args.name,
                "removed_path": skill_dir.to_string_lossy(),
            }),
        ),
    })
}
