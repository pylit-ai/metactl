use super::*;

pub(super) fn cmd_pack(cli: &Cli, args: &PackArgs) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        PackCommand::Use(use_args) => cmd_use(cli, use_args),
        PackCommand::Add(add_args) => cmd_add(cli, add_args),
        PackCommand::Remove(remove_args) => cmd_remove(cli, remove_args),
        PackCommand::ImportSkill(import_args) => cmd_pack_import_skill(cli, import_args),
        PackCommand::ExportSkill(export_args) => cmd_pack_export_skill(cli, export_args),
        PackCommand::VerifySkill(verify_args) => cmd_pack_verify_skill(cli, verify_args),
    }
}

#[derive(Debug, Clone)]
pub(super) struct SkillFrontmatter {
    pub(super) name: String,
    pub(super) description: String,
}

#[derive(Debug, Clone)]
pub(super) struct SkillFileEntry {
    pub(super) relative_path: String,
    pub(super) source_path: PathBuf,
    pub(super) executable: bool,
    pub(super) is_script: bool,
    pub(super) byte_len: u64,
}

#[derive(Debug, Clone)]
pub(super) struct CodexSkillEntry {
    pub(super) name: String,
    pub(super) dir: PathBuf,
    pub(super) skill_md: PathBuf,
}

fn cmd_pack_import_skill(
    cli: &Cli,
    args: &PackImportSkillArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let skill_dir = canonical_skill_dir(&args.path).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Agent Skill import failed.")
            .with_details(error_details(&err))
    })?;
    let skill_md = skill_dir.join("SKILL.md");
    let frontmatter = read_skill_frontmatter(&skill_md).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Agent Skill frontmatter is invalid.")
            .with_details(error_details(&err))
    })?;
    let files = collect_skill_files(&skill_dir).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Agent Skill import safety check failed.")
            .with_details(error_details(&err))
    })?;
    let safety_findings = skill_import_safety_findings(&files, args.allow_executable_scripts);
    if !safety_findings.is_empty() {
        return Err(
            CliError::new(EXIT_VALIDATION, "Agent Skill import was refused.")
                .with_details(safety_findings),
        );
    }

    let imported_at = now_string();
    let digest = skill_tree_digest(&files).map_err(internal_error)?;
    let target_dir = imported_skill_dir(&project_root, &frontmatter.name);
    if target_dir.exists() {
        fs::remove_dir_all(&target_dir).map_err(internal_error)?;
    }
    let skill_target_dir = target_dir.join("skill");
    copy_skill_files(&files, &skill_target_dir).map_err(internal_error)?;

    let script_classification = script_classification_json(&files);
    let manifest = json!({
        "kind": "pack",
        "id": frontmatter.name,
        "version": "0.1.0-imported",
        "title": title_from_skill_id(&frontmatter.name),
        "description": frontmatter.description,
        "activation_class": "instruction",
        "side_effect_class": "none",
        "trust_tier": "candidate_quarantined",
        "requires_confirmation": false,
        "compatible_roles": [],
        "compatible_targets": [],
        "resources": skill_resources_json(&files),
        "imports": [{
            "ecosystem": "skill_md",
            "origin": skill_dir.to_string_lossy(),
            "digest": digest,
            "imported_at": imported_at,
        }],
        "visibility_scope": "private",
        "metadata": {
            "agent_skill": "true",
            "script_execution_granted": "false"
        }
    });
    let provenance = json!({
        "source_path": skill_dir.to_string_lossy(),
        "digest": digest,
        "imported_at": imported_at,
        "script_execution_granted": false,
    });
    write_pretty_json(&target_dir.join("pack.json"), &manifest).map_err(internal_error)?;
    write_pretty_json(&target_dir.join("provenance.json"), &provenance).map_err(internal_error)?;

    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Imported Agent Skill '{}' as a local candidate pack.",
                frontmatter.name
            ),
        ),
        json: success_json(
            "pack",
            Some(&project_root),
            json!({
                "action": "import-skill",
                "pack_id": frontmatter.name,
                "imported_path": target_dir.to_string_lossy(),
                "provenance": provenance,
                "script_classification": script_classification,
            }),
        ),
    })
}

fn cmd_pack_export_skill(
    cli: &Cli,
    args: &PackExportSkillArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let source_dir = imported_skill_dir(&project_root, &args.pack_id).join("skill");
    if !source_dir.join("SKILL.md").exists() {
        return Err(
            CliError::new(EXIT_STATE, "Imported Agent Skill was not found.").with_details(vec![
                format!("missing {}", source_dir.join("SKILL.md").display()),
            ]),
        );
    }
    let export_dir = project_root
        .join(".metactl/exported-skills")
        .join(&args.target)
        .join(&args.pack_id);
    if export_dir.exists() {
        fs::remove_dir_all(&export_dir).map_err(internal_error)?;
    }
    let files = collect_skill_files(&source_dir).map_err(internal_error)?;
    copy_skill_files(&files, &export_dir).map_err(internal_error)?;
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Exported Agent Skill '{}' for {}.",
                args.pack_id, args.target
            ),
        ),
        json: success_json(
            "pack",
            Some(&project_root),
            json!({
                "action": "export-skill",
                "pack_id": args.pack_id,
                "target": args.target,
                "exported_path": export_dir.to_string_lossy(),
                "script_execution_granted": false,
            }),
        ),
    })
}

fn cmd_pack_verify_skill(
    cli: &Cli,
    args: &PackVerifySkillArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let source_dir = imported_skill_dir(&project_root, &args.pack_id).join("skill");
    let skill_md = source_dir.join("SKILL.md");
    let frontmatter = read_skill_frontmatter(&skill_md).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Agent Skill verification failed.")
            .with_details(error_details(&err))
    })?;
    let files = collect_skill_files(&source_dir).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Agent Skill verification failed.")
            .with_details(error_details(&err))
    })?;
    let findings = skill_import_safety_findings(&files, false);
    if !findings.is_empty() {
        return Err(
            CliError::new(EXIT_VALIDATION, "Agent Skill verification failed.")
                .with_details(findings),
        );
    }
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Verified Agent Skill '{}' with profile {}.",
                frontmatter.name, args.profile
            ),
        ),
        json: success_json(
            "pack",
            Some(&project_root),
            json!({
                "action": "verify-skill",
                "pack_id": frontmatter.name,
                "profile": args.profile,
                "status": "pass",
                "script_classification": script_classification_json(&files),
            }),
        ),
    })
}

fn canonical_skill_dir(path: &Path) -> Result<PathBuf> {
    let dir = fs::canonicalize(path).with_context(|| format!("canonicalize {}", path.display()))?;
    if !dir.is_dir() {
        return Err(anyhow!("{} is not a directory", dir.display()));
    }
    if !dir.join("SKILL.md").is_file() {
        return Err(anyhow!("{} must contain SKILL.md", dir.display()));
    }
    Ok(dir)
}

fn canonical_skill_source_dir(path: &Path) -> Result<PathBuf> {
    if path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md") {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow!("SKILL.md path has no parent directory"))?;
        canonical_skill_dir(parent)
    } else {
        canonical_skill_dir(path)
    }
}

pub(super) fn resolve_skill_source_dir(project_root: &Path, input: &Path) -> Result<PathBuf> {
    let mut candidates = vec![input.to_path_buf()];
    if input.is_relative() {
        candidates.push(project_root.join(input));
    }
    for candidate in candidates {
        if candidate.exists() {
            return canonical_skill_source_dir(&candidate);
        }
    }

    let name = input.to_string_lossy();
    validate_skill_name(&name)?;
    let repo_root = project_root.join(".codex").join("skills");
    let matches = discover_codex_skill_entries(&repo_root)?
        .into_iter()
        .filter(|entry| entry.name == name)
        .collect::<Vec<_>>();
    match matches.len() {
        0 => Err(anyhow!(
            "repo-local Codex skill '{name}' was not found under {}",
            repo_root.display()
        )),
        1 => Ok(matches[0].dir.clone()),
        _ => Err(anyhow!(
            "repo-local Codex skill '{name}' is ambiguous; pass a concrete skill path"
        )),
    }
}

pub(super) fn ensure_codex_skill_target(target: &str) -> std::result::Result<(), CliError> {
    if target == "codex-cli" {
        Ok(())
    } else {
        Err(CliError::new(
            EXIT_STATE,
            format!(
                "skills currently manages only codex-cli skill roots; unsupported target: {target}"
            ),
        ))
    }
}

pub(super) fn codex_user_skill_root_for_command() -> std::result::Result<PathBuf, CliError> {
    codex_user_skill_root().ok_or_else(|| {
        CliError::new(
            EXIT_STATE,
            "HOME is not set; cannot resolve user-global Codex skill root ~/.codex/skills",
        )
    })
}

pub(super) fn codex_user_skill_root() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(".codex").join("skills"))
}

pub(super) fn replace_existing_user_skill_dir(
    skill_dir: &Path,
    force: bool,
) -> std::result::Result<(), CliError> {
    let Ok(metadata) = fs::symlink_metadata(skill_dir) else {
        return Ok(());
    };
    if metadata.file_type().is_symlink() {
        return Err(CliError::new(
            EXIT_CONFLICT,
            format!(
                "Refusing to replace symlinked user-global skill path: {}",
                skill_dir.display()
            ),
        ));
    }
    if !metadata.is_dir() {
        return Err(CliError::new(
            EXIT_CONFLICT,
            format!(
                "User-global skill path exists but is not a directory: {}",
                skill_dir.display()
            ),
        ));
    }
    if !force {
        return Err(CliError::new(
            EXIT_CONFLICT,
            format!(
                "User-global skill already exists at {}. Rerun with --force to replace it.",
                skill_dir.display()
            ),
        ));
    }
    fs::remove_dir_all(skill_dir).map_err(internal_error)
}

pub(super) fn ensure_removable_user_skill_dir(
    skill_dir: &Path,
) -> std::result::Result<(), CliError> {
    let metadata = fs::symlink_metadata(skill_dir).map_err(|_| {
        CliError::new(
            EXIT_STATE,
            format!("User-global skill does not exist: {}", skill_dir.display()),
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(CliError::new(
            EXIT_CONFLICT,
            format!(
                "Refusing to remove symlinked user-global skill path: {}",
                skill_dir.display()
            ),
        ));
    }
    if !metadata.is_dir() || !skill_dir.join("SKILL.md").is_file() {
        return Err(CliError::new(
            EXIT_CONFLICT,
            format!(
                "Refusing to remove non-skill directory: {}",
                skill_dir.display()
            ),
        ));
    }
    Ok(())
}
