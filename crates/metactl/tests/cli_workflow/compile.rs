use super::*;

// Compile workflow tests.

#[test]
fn cli_init_compile_apply_revert_greenfield() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let compile = run_cli(project.path(), &["compile"]);
    assert!(
        compile.status.success(),
        "stdout: {}\nstderr: {}",
        stdout(&compile),
        stderr(&compile)
    );
    assert!(project
        .path()
        .join(".metactl/generated/codex-cli/AGENTS.md")
        .exists());

    let apply = run_cli(project.path(), &["apply", "--mode", "symlink"]);
    assert!(apply.status.success(), "{}", stderr(&apply));
    assert!(project.path().join("AGENTS.md").exists());
    assert!(project
        .path()
        .join(".codex/skills/python-refactor/python-refactor/SKILL.md")
        .exists());
    assert!(
        !project
            .path()
            .join(".codex/skills/python-refactor/contracts/SKILL.md")
            .exists(),
        "contracts surface should stay suppressed in default minimal mode"
    );
    assert!(project
        .path()
        .join(".metactl/state/managed_files.json")
        .exists());

    let revert = run_cli(project.path(), &["revert"]);
    assert!(revert.status.success(), "{}", stderr(&revert));
    assert!(!project.path().join("AGENTS.md").exists());
    assert!(!project.path().join(".codex/skills").exists());
}

#[test]
fn cli_compile_apply_chained_greenfield() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let out = run_cli(project.path(), &["--json", "compile", "--apply"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let value = json_output(&out);
    assert_json_contract(&value, "compile", Some(project.path()));
    assert!(value.get("apply").is_some());
    assert_eq!(value["apply"]["ok"], true);
    assert_eq!(value["apply"]["command"], "apply");

    assert!(project.path().join("AGENTS.md").exists());
    assert!(project
        .path()
        .join(".codex/skills/python-refactor/python-refactor/SKILL.md")
        .exists());

    let validate = run_cli(project.path(), &["validate"]);
    assert!(validate.status.success(), "{}", stderr(&validate));
}

#[test]
fn cli_compile_surface_mode_controls_emitted_skill_surfaces() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let minimal = run_cli(project.path(), &["--json", "compile"]);
    assert!(minimal.status.success(), "{}", stderr(&minimal));
    let minimal_manifest: Value = serde_json::from_slice(
        &fs::read(
            project
                .path()
                .join(".metactl/generated/codex-cli/compile.manifest.json"),
        )
        .expect("read minimal compile manifest"),
    )
    .expect("decode minimal compile manifest");

    assert_eq!(minimal_manifest["surface_selection_mode"], "minimal");
    assert!(minimal_manifest["surface_selection"]
        .as_array()
        .expect("surface_selection")
        .iter()
        .any(|item| {
            item["pack_ref"]["id"] == "python-refactor"
                && item["surface_slug"] == "contracts"
                && item["emitted"] == false
                && item["reason_code"] == "suppressed_by_mode"
        }));
    assert!(
        !project
            .path()
            .join(".metactl/generated/codex-cli/.codex/skills/python-refactor/contracts/SKILL.md")
            .exists(),
        "contracts surface should be suppressed in minimal mode"
    );

    let full = run_cli(
        project.path(),
        &["--json", "compile", "--surface-mode", "full"],
    );
    assert!(full.status.success(), "{}", stderr(&full));
    let full_manifest: Value = serde_json::from_slice(
        &fs::read(
            project
                .path()
                .join(".metactl/generated/codex-cli/compile.manifest.json"),
        )
        .expect("read full compile manifest"),
    )
    .expect("decode full compile manifest");

    assert_eq!(full_manifest["surface_selection_mode"], "full");
    assert!(full_manifest["surface_selection"]
        .as_array()
        .expect("surface_selection")
        .iter()
        .any(|item| {
            item["pack_ref"]["id"] == "python-refactor"
                && item["surface_slug"] == "contracts"
                && item["emitted"] == true
        }));
    assert!(
        project
            .path()
            .join(".metactl/generated/codex-cli/.codex/skills/python-refactor/contracts/SKILL.md")
            .exists(),
        "contracts surface should emit in full mode"
    );
}

#[test]
fn cli_config_default_surface_mode_controls_compile_explain_and_status() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let config_path = project.path().join("metactl.yaml");
    let config = fs::read_to_string(&config_path).expect("read config");
    fs::write(
        &config_path,
        config.replace(
            "  discovery_mode: candidate_search\n",
            "  discovery_mode: candidate_search\n  surface_selection_mode: full\n",
        ),
    )
    .expect("write config");

    let explain = run_cli(project.path(), &["--json", "explain"]);
    assert!(explain.status.success(), "{}", stderr(&explain));
    let explain_json = json_output(&explain);
    assert_eq!(
        explain_json["target_projection"]["surface_selection_mode"],
        "full"
    );

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    let sync_text = stdout(&sync);
    assert!(
        sync_text.contains("surface: full"),
        "sync output should expose surface mode:\n{sync_text}"
    );

    let status = run_cli(project.path(), &["--json", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_json = json_output(&status);
    assert_eq!(
        status_json["applied_targets"][0]["surface_selection_mode"],
        "full"
    );
    assert_eq!(
        status_json["applied_targets"][0]["configured_surface_selection_mode"],
        "full"
    );
    assert_eq!(
        status_json["applied_targets"][0]["surface_selection_mode_matches_config"],
        true
    );
    assert_eq!(
        status_json["surface_mode_mismatches"]
            .as_array()
            .expect("surface_mode_mismatches")
            .len(),
        0
    );
    assert_eq!(status_json["needs_sync"], false);

    let manifest: Value = serde_json::from_slice(
        &fs::read(
            project
                .path()
                .join(".metactl/generated/codex-cli/compile.manifest.json"),
        )
        .expect("read compile manifest"),
    )
    .expect("decode compile manifest");
    assert_eq!(manifest["surface_selection_mode"], "full");
}

#[test]
fn cursor_compile_produces_expected_output_paths() {
    let project = TempDir::new().expect("tempdir");

    // Init with both cursor and codex-cli targets (packs are codex-cli compatible)
    let init = run_cli(
        project.path(),
        &["init", "--target", "cursor", "--target", "codex-cli"],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    // Add a pack directly (use would resolve against target compatibility)
    let add_out = run_cli(project.path(), &["add", "python-refactor"]);
    assert!(add_out.status.success(), "{}", stderr(&add_out));

    // Compile should produce cursor-specific outputs
    let compile = run_cli(project.path(), &["--json", "compile"]);
    assert!(compile.status.success(), "{}", stderr(&compile));

    // Verify the cursor generated directory exists
    let generated_dir = project.path().join(".metactl/generated/cursor");
    assert!(
        generated_dir.exists(),
        "cursor generated directory should exist"
    );

    // The cursor target uses .cursor/rules/metactl-pack-index.mdc as the index path
    let index_path = generated_dir.join(".cursor/rules/metactl-pack-index.mdc");
    assert!(
        index_path.exists(),
        "cursor pack index should exist at .cursor/rules/metactl-pack-index.mdc, found: {:?}",
        list_tree(&generated_dir)
    );
}

#[test]
fn cli_compile_with_all_flag() {
    let project = TempDir::new().expect("tempdir");

    // Initialize with multiple targets
    let init = run_cli(
        project.path(),
        &["init", "--target", "claude-code", "--target", "cursor"],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    // Compile using --all flag
    let compile = run_cli(project.path(), &["--json", "compile", "--all"]);
    assert!(compile.status.success(), "{}", stderr(&compile));
    let compile_json = json_output(&compile);
    assert_json_contract(&compile_json, "compile", Some(project.path()));

    // Verify both targets were compiled
    let compiled = compile_json["targets"]
        .as_array()
        .expect("targets should be an array");
    assert!(
        compiled.len() >= 2,
        "should have at least 2 compiled targets, got: {:?}",
        compiled
    );

    let target_names: Vec<String> = compiled
        .iter()
        .map(|t| {
            t["target"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_default()
        })
        .collect();
    assert!(
        target_names.contains(&"claude-code".to_string()),
        "claude-code should be in compiled targets: {:?}",
        target_names
    );
    assert!(
        target_names.contains(&"cursor".to_string()),
        "cursor should be in compiled targets: {:?}",
        target_names
    );
}

#[test]
fn cli_compile_bad_target_shows_available() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    // Try to compile with unknown target
    let output = run_cli(project.path(), &["compile", "--target", "unknown-target"]);
    assert!(
        !output.status.success(),
        "compile with bad target should fail"
    );

    let error_text = stderr(&output);
    assert!(
        error_text.contains("Available targets"),
        "error should mention available targets: {}",
        error_text
    );
    assert!(
        error_text.contains("codex-cli"),
        "error should list codex-cli: {}",
        error_text
    );
}

#[test]
fn cli_compile_accepts_target_aliases() {
    let project = TempDir::new().expect("tempdir");
    // Initialize project with claude-code target
    let init = run_cli(project.path(), &["init", "--target", "claude-code"]);
    assert!(init.status.success(), "{}", stderr(&init));

    // Compile with "claude" alias instead of "claude-code"
    let output = run_cli(project.path(), &["compile", "--target", "claude"]);
    assert!(output.status.success(), "{}", stderr(&output));

    // Verify compiled file exists for the canonical target name
    assert!(project
        .path()
        .join(".metactl/generated/claude-code/CLAUDE.md")
        .exists());

    // Verify stderr contains the alias resolution note
    let err_text = stderr(&output);
    assert!(
        err_text.contains("resolved target alias"),
        "stderr should mention alias resolution: {}",
        err_text
    );
    assert!(
        err_text.contains("claude"),
        "stderr should mention 'claude' alias: {}",
        err_text
    );
    assert!(
        err_text.contains("claude-code"),
        "stderr should mention 'claude-code' canonical name: {}",
        err_text
    );
}
