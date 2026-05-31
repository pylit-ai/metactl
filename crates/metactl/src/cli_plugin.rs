use super::*;

pub(super) fn cmd_plugin(
    cli: &Cli,
    args: &PluginArgs,
) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        PluginCommand::List(list_args) => cmd_plugin_list(cli, list_args),
        PluginCommand::Export(export_args) => cmd_plugin_export(cli, export_args),
        PluginCommand::Verify(verify_args) => cmd_plugin_verify(cli, verify_args),
    }
}

fn cmd_plugin_list(
    cli: &Cli,
    args: &PluginListArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let tier = args.tier.map(PluginTier::from);
    let library_root =
        resolve_plugin_library_root(cli, tier.unwrap_or(PluginTier::Public), &args.library_root)?;
    let items =
        metactl::list_plugin_packs(&library_root, &args.target, tier).map_err(state_error)?;
    let lines = if items.is_empty() {
        "No plugin-eligible packs found.".to_string()
    } else {
        items
            .iter()
            .map(|item| {
                let tiers = item
                    .eligible_tiers
                    .iter()
                    .map(|tier| tier.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                format!("- {} {} [{}]", item.pack_id, item.version, tiers)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Plugin-capable packs for {} from {}:\n{}",
                args.target,
                library_root.display(),
                lines
            ),
        ),
        json: success_json(
            "plugin",
            Some(&project_root),
            json!({
                "action": "list",
                "target": args.target,
                "tier": tier.map(|tier| tier.as_str()),
                "library_root": library_root,
                "packs": items,
            }),
        ),
    })
}

fn cmd_plugin_export(
    cli: &Cli,
    args: &PluginExportArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let tier = PluginTier::from(args.tier);
    let library_root = resolve_plugin_library_root(cli, tier, &args.library_root)?;
    let out = resolve_path_against_project(&project_root, &args.out);
    let result = metactl::export_plugin_marketplace(PluginExportOptions {
        library_root: library_root.clone(),
        target: args.target.clone(),
        tier,
        out: out.clone(),
        force: args.force,
        plugin_name: args.name.clone(),
    })
    .map_err(state_error)?;
    if tier == PluginTier::Public {
        let findings = public_boundary_findings(&out).map_err(internal_error)?;
        if !findings.is_empty() {
            return Err(CliError::new(
                EXIT_VALIDATION,
                "Generated public plugin output failed public boundary check.",
            )
            .with_details(findings));
        }
    }
    let target_label = plugin_target_display_name(&result.target);
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Exported {} plugin marketplace: {}\nBundle: {}\nPacks: {}\nNext: metactl plugin verify --target {} --tier {} --path {}",
                target_label,
                out.display(),
                result.plugin_path.display(),
                result.pack_ids.len(),
                result.target,
                result.tier.as_str(),
                out.display()
            ),
        ),
        json: success_json(
            "plugin",
            Some(&project_root),
            json!({
                "action": "export",
                "library_root": library_root,
                "result": result,
            }),
        ),
    })
}

fn cmd_plugin_verify(
    cli: &Cli,
    args: &PluginVerifyArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let path = resolve_path_against_project(&project_root, &args.path);
    let mut report = metactl::verify_plugin_marketplace(PluginVerifyOptions {
        path: path.clone(),
        target: args.target.clone(),
        tier: args.tier.map(PluginTier::from),
    })
    .map_err(state_error)?;
    if report.tier == PluginTier::Public
        || args.tier.map(PluginTier::from) == Some(PluginTier::Public)
    {
        report
            .findings
            .extend(public_boundary_findings(&path).map_err(internal_error)?);
        report.status = if report.findings.is_empty() {
            "pass".to_string()
        } else {
            "fail".to_string()
        };
    }
    if !report.findings.is_empty() {
        return Err(
            CliError::new(EXIT_VALIDATION, "plugin marketplace verification failed.")
                .with_details(report.findings),
        );
    }
    let target_label = plugin_target_display_name(&report.target);
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Verified {} plugin marketplace: {}\nBundles: {}\nPacks: {}",
                target_label, report.status, report.plugin_count, report.pack_count
            ),
        ),
        json: success_json(
            "plugin",
            Some(&project_root),
            json!({
                "action": "verify",
                "report": report,
            }),
        ),
    })
}

fn plugin_target_display_name(target: &str) -> &'static str {
    match target {
        "codex-cli" => "Codex",
        "claude-code" => "Claude Code",
        _ => "runtime",
    }
}

fn resolve_plugin_library_root(
    cli: &Cli,
    tier: PluginTier,
    provided: &Option<PathBuf>,
) -> std::result::Result<PathBuf, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    match (tier, provided) {
        (_, Some(path)) => Ok(resolve_path_against_project(&project_root, path)),
        (PluginTier::Public, None) => ensure_bundled_starter_library_root().map_err(internal_error),
        (PluginTier::Private, None) => Err(CliError::new(
            EXIT_VALIDATION,
            "Private plugin export requires --library-root.",
        )),
    }
}
