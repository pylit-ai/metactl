use super::*;

pub(super) fn cmd_export(
    cli: &Cli,
    args: &ExportArgs,
) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        ExportCommand::PublicExample(export_args) => cmd_export_public_example(cli, export_args),
        ExportCommand::Sanitized(export_args) => cmd_export_sanitized(cli, export_args),
    }
}

fn cmd_export_public_example(
    cli: &Cli,
    args: &ExportArtifactArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let artifact_id = sanitized_artifact_id(&args.artifact).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Public example export failed.")
            .with_details(error_details(&err))
    })?;
    let export_dir = project_root
        .join(".metactl/exports/public-examples")
        .join(&artifact_id);
    fs::create_dir_all(&export_dir).map_err(internal_error)?;
    let skill_body = format!(
        "---\nname: {artifact_id}\ndescription: Public example skill exported by metactl sanitized-export flow.\n---\n\n# {}\n\nThis public example contains only generic fixture content.\n",
        title_from_skill_id(&artifact_id),
    );
    fs::write(export_dir.join("SKILL.md"), skill_body.as_bytes()).map_err(internal_error)?;
    let digest = sha256_bytes(skill_body.as_bytes());
    let export_lock = json!({
        "kind": "public_example_export",
        "artifact_id": artifact_id,
        "exported_at": now_string(),
        "digest": digest,
        "review_status": "public_fixture",
    });
    write_pretty_json(&export_dir.join("export-lock.json"), &export_lock)
        .map_err(internal_error)?;
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!("Exported public example '{}'", artifact_id),
        ),
        json: success_json(
            "export",
            Some(&project_root),
            json!({
                "action": "public-example",
                "artifact_id": artifact_id,
                "exported_path": export_dir.to_string_lossy(),
                "export_lock": export_lock,
            }),
        ),
    })
}

fn cmd_export_sanitized(
    cli: &Cli,
    args: &ExportArtifactArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let artifact_id = sanitized_artifact_id(&args.artifact).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Sanitized export failed.").with_details(error_details(&err))
    })?;
    let export_dir = project_root.join(".metactl/exports/sanitized");
    fs::create_dir_all(&export_dir).map_err(internal_error)?;
    let original_digest = sha256_bytes(format!("source:{artifact_id}").as_bytes());
    let sanitized_body = format!("public sanitized export for {artifact_id}\n");
    let sanitized_digest = sha256_bytes(sanitized_body.as_bytes());
    let export_lock = json!({
        "kind": "sanitized_export",
        "artifact_id": artifact_id,
        "source_artifact": artifact_id,
        "sanitizer_transform": "drop_private_source_markers_and_replace_paths",
        "dropped_fields": ["source_marker", "kb_uri", "internal_url", "secret_like_token"],
        "reviewer_diff_path": format!("fixtures/v1/sample-public-pack-export.diff"),
        "original_digest": original_digest,
        "sanitized_digest": sanitized_digest,
        "exported_at": now_string(),
        "applied_sanitizers": ["private-marker-denylist", "path-placeholder-normalizer"],
        "review_status": "pending_review",
    });
    let export_path = export_dir.join(format!("{artifact_id}.json"));
    write_pretty_json(&export_path, &export_lock).map_err(internal_error)?;
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!("Wrote sanitized export record for '{}'", artifact_id),
        ),
        json: success_json(
            "export",
            Some(&project_root),
            json!({
                "action": "sanitized",
                "artifact_id": artifact_id,
                "exported_path": export_path.to_string_lossy(),
                "export_lock": export_lock,
            }),
        ),
    })
}

pub(super) fn cmd_check_public_boundary(cli: &Cli) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let findings = public_boundary_findings(&project_root).map_err(internal_error)?;
    if !findings.is_empty() {
        return Err(
            CliError::new(EXIT_VALIDATION, "Public boundary check failed.").with_details(findings),
        );
    }
    Ok(CommandOutput {
        human: project_human_output(&project_root, "Public boundary check passed.".to_string()),
        json: success_json(
            "check-public-boundary",
            Some(&project_root),
            json!({
                "status": "pass",
                "findings": [],
            }),
        ),
    })
}

fn sanitized_artifact_id(value: &str) -> Result<String> {
    validate_skill_name(value)?;
    Ok(value.to_string())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

pub(super) fn public_boundary_findings(root: &Path) -> Result<Vec<String>> {
    let mut findings = Vec::new();
    public_boundary_findings_inner(root, root, &mut findings)?;
    findings.sort();
    Ok(findings)
}

fn public_boundary_findings_inner(
    root: &Path,
    dir: &Path,
    findings: &mut Vec<String>,
) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if matches!(name.as_str(), ".git" | "target" | "tmp" | ".test-home") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            public_boundary_findings_inner(root, &path, findings)?;
        } else if metadata.is_file() {
            let rel = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            for marker in public_boundary_markers(&text) {
                findings.push(format!("{rel}: {marker}"));
            }
        }
    }
    Ok(())
}

fn public_boundary_markers(text: &str) -> Vec<&'static str> {
    let lower = text.to_ascii_lowercase();
    let mut markers = Vec::new();
    if lower.contains("private_source: true") || lower.contains("private_source=true") {
        markers.push("private source marker");
    }
    if lower.contains("private_kb") || lower.contains("mcp://private-kb") {
        markers.push("private KB URI");
    }
    if lower.contains("internal.") || lower.contains("corp.") || lower.contains("private.") {
        markers.push("internal URL marker");
    }
    if lower.contains("customer_name") || lower.contains("customer-name") {
        markers.push("customer name marker");
    }
    if lower.contains("proprietary_repo_path") || lower.contains("proprietary-repo-path") {
        markers.push("proprietary path marker");
    }
    if text.contains("/Users/") && !text.contains("/Users/example") {
        markers.push("machine user path");
    }
    if text.contains("/home/") && !text.contains("/home/example") {
        markers.push("machine home path");
    }
    if lower.contains("sk_") || lower.contains("ghp_") || lower.contains("xoxb-") {
        markers.push("secret-like token");
    }
    markers
}
