use super::*;

// Sync, apply, and generated-output workflow tests.

#[test]
fn setup_yes_with_explicit_target_creates_config_without_sync() {
    let project = TempDir::new().expect("tempdir");
    let output = run_cli(
        project.path(),
        &["--json", "setup", "--target", "codex-cli", "--yes"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "setup", Some(project.path()));
    assert_eq!(value["ran_sync"], json!(false));
    assert_eq!(value["artifact_policy"], json!("portable-first"));
    assert!(project.path().join("metactl.yaml").exists());
    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("config");
    assert!(config.contains("agent_artifact_policy"));
    assert!(config.contains("portable-first"));
    assert!(config.contains("agentic-artifact-forge"));
    assert!(!project.path().join(".codex").exists());
}

#[test]
fn cli_preview_alias_runs_sync_preview_without_materializing_runtime_files() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let preview = run_cli(project.path(), &["--json", "preview"]);
    assert!(preview.status.success(), "{}", stderr(&preview));
    let json = json_output(&preview);
    assert_json_contract(&json, "sync", Some(project.path()));
    assert_eq!(json["preview"], true);
    assert!(
        !project.path().join("AGENTS.md").exists(),
        "preview alias must not apply runtime files"
    );
}

#[test]
fn cli_sync_greenfield_workflow() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let sync = run_cli(project.path(), &["--json", "sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    let json = json_output(&sync);
    assert_json_contract(&json, "sync", Some(project.path()));
    assert_eq!(json["preview"], false);
    assert!(matches!(
        json["targets"][0]["status"].as_str(),
        Some("ready" | "degraded")
    ));
    assert_eq!(json["apply"]["command"], "apply");
    assert_eq!(json["validate"]["command"], "validate");
    assert!(project.path().join("AGENTS.md").exists());
    assert!(project
        .path()
        .join(".codex/skills/python-refactor/python-refactor/SKILL.md")
        .exists());
}

#[test]
fn cli_sync_codex_skill_outputs_are_regular_files() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let skill_path = project
        .path()
        .join(".codex/skills/python-refactor/python-refactor/SKILL.md");
    let metadata = fs::symlink_metadata(&skill_path).expect("skill metadata");
    assert!(
        metadata.file_type().is_file(),
        "Codex skill bodies should materialize as regular files so Codex can discover them: {}",
        skill_path.display()
    );
    assert!(
        !metadata.file_type().is_symlink(),
        "Codex skill bodies should not be symlinks: {}",
        skill_path.display()
    );
}

#[test]
fn cli_sync_root_instruction_outputs_are_regular_files_under_symlink_apply() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let first_sync = run_cli(project.path(), &["sync"]);
    assert!(first_sync.status.success(), "{}", stderr(&first_sync));

    let second_sync = run_cli(project.path(), &["sync"]);
    assert!(second_sync.status.success(), "{}", stderr(&second_sync));

    let agents_path = project.path().join("AGENTS.md");
    let agents_metadata = fs::symlink_metadata(&agents_path).expect("AGENTS.md metadata");
    assert!(
        agents_metadata.file_type().is_file() && !agents_metadata.file_type().is_symlink(),
        "Codex AGENTS.md should remain a regular file under symlink apply on repeat sync: {}",
        agents_path.display()
    );

    let skill_path = project
        .path()
        .join(".codex/skills/python-refactor/python-refactor/SKILL.md");
    let skill_metadata = fs::symlink_metadata(&skill_path).expect("skill metadata");
    assert!(
        skill_metadata.file_type().is_file() && !skill_metadata.file_type().is_symlink(),
        "Codex skill bodies should remain regular files under symlink apply: {}",
        skill_path.display()
    );
}

#[test]
fn cli_repeat_sync_preserves_existing_regular_managed_outputs() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let agents_path = project.path().join("AGENTS.md");
    let metadata = fs::symlink_metadata(&agents_path).expect("AGENTS.md metadata");
    assert!(
        metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
        "initial sync should materialize AGENTS.md as a regular file: {}",
        agents_path.display()
    );

    let repeat_sync = run_cli(project.path(), &["sync"]);
    assert!(repeat_sync.status.success(), "{}", stderr(&repeat_sync));

    let metadata = fs::symlink_metadata(&agents_path).expect("AGENTS.md metadata");
    assert!(
        metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
        "repeat sync should not type-churn an existing regular managed output with matching digest: {}",
        agents_path.display()
    );
}

#[test]
fn cli_sync_preserves_restored_user_agents_when_legacy_state_has_no_patch_marker() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    fs::write(
        project.path().join("AGENTS.md"),
        "# Project Agent Guide\n\nKeep this durable repo guidance.\n",
    )
    .expect("seed AGENTS");

    let adopt = run_cli(project.path(), &["sync", "--adopt", "patch"]);
    assert!(adopt.status.success(), "{}", stderr(&adopt));

    let state_path = project.path().join(".metactl/state/codex-cli.json");
    let mut state: Value =
        serde_json::from_str(&fs::read_to_string(&state_path).expect("read state"))
            .expect("parse state");
    let agents_state = state["outputs"]
        .as_array_mut()
        .expect("outputs")
        .iter_mut()
        .find(|output| output["destination_path"] == "AGENTS.md")
        .expect("AGENTS state");
    agents_state["patch_marker"] = Value::Null;
    agents_state["backup_path"] = Value::Null;
    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&state).expect("serialize state"),
    )
    .expect("write legacy state");

    fs::write(
        project.path().join("AGENTS.md"),
        "# Project Agent Guide\n\nKeep this durable repo guidance.\n",
    )
    .expect("restore AGENTS");

    let repeat_sync = run_cli(project.path(), &["sync"]);
    assert!(repeat_sync.status.success(), "{}", stderr(&repeat_sync));

    let agents = fs::read_to_string(project.path().join("AGENTS.md")).expect("read AGENTS");
    assert!(
        agents.contains("Keep this durable repo guidance."),
        "plain sync should preserve restored repo-owned AGENTS.md content: {agents}"
    );
    assert!(
        agents.contains("metactl:begin"),
        "plain sync should add or update the managed block instead of replacing AGENTS.md: {agents}"
    );
}

#[test]
fn cli_sync_preserves_restored_user_claude_when_legacy_state_has_no_patch_marker() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(project.path(), &["init", "--target", "claude-code"]);
    assert!(init.status.success(), "{}", stderr(&init));

    fs::write(
        project.path().join("CLAUDE.md"),
        "# Claude Project Guide\n\nKeep this Claude-specific repo guidance.\n",
    )
    .expect("seed CLAUDE");

    let adopt = run_cli(project.path(), &["sync", "--adopt", "patch"]);
    assert!(adopt.status.success(), "{}", stderr(&adopt));

    let state_path = project.path().join(".metactl/state/claude-code.json");
    let mut state: Value =
        serde_json::from_str(&fs::read_to_string(&state_path).expect("read state"))
            .expect("parse state");
    let claude_state = state["outputs"]
        .as_array_mut()
        .expect("outputs")
        .iter_mut()
        .find(|output| output["destination_path"] == "CLAUDE.md")
        .expect("CLAUDE state");
    claude_state["patch_marker"] = Value::Null;
    claude_state["backup_path"] = Value::Null;
    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&state).expect("serialize state"),
    )
    .expect("write legacy state");

    fs::write(
        project.path().join("CLAUDE.md"),
        "# Claude Project Guide\n\nKeep this Claude-specific repo guidance.\n",
    )
    .expect("restore CLAUDE");

    let repeat_sync = run_cli(project.path(), &["sync"]);
    assert!(repeat_sync.status.success(), "{}", stderr(&repeat_sync));

    let claude = fs::read_to_string(project.path().join("CLAUDE.md")).expect("read CLAUDE");
    assert!(
        claude.contains("Keep this Claude-specific repo guidance."),
        "plain sync should preserve restored repo-owned CLAUDE.md content: {claude}"
    );
    assert!(
        claude.contains("metactl:begin"),
        "plain sync should add or update the managed block instead of replacing CLAUDE.md: {claude}"
    );
}

#[test]
fn cli_sync_adopt_patch_preserves_brownfield_root_docs_across_targets() {
    for (target, doc, sentinel) in [
        ("codex-cli", "AGENTS.md", "codex durable guidance sentinel"),
        (
            "claude-code",
            "CLAUDE.md",
            "claude durable guidance sentinel",
        ),
        (
            "gemini-cli",
            "GEMINI.md",
            "gemini durable guidance sentinel",
        ),
    ] {
        let project = TempDir::new().expect("tempdir");
        let init = run_cli(project.path(), &["init", "--target", target]);
        assert!(init.status.success(), "{}", stderr(&init));

        fs::write(
            project.path().join(doc),
            format!("# Root Instructions\n\n{sentinel}\n\nDo not replace this file.\n"),
        )
        .expect("seed root doc");

        let adopt = run_cli(project.path(), &["sync", "--adopt", "patch"]);
        assert!(
            adopt.status.success(),
            "adopt patch failed for {target}: {}",
            stderr(&adopt)
        );
        let repeat_sync = run_cli(project.path(), &["sync"]);
        assert!(
            repeat_sync.status.success(),
            "repeat sync failed for {target}: {}",
            stderr(&repeat_sync)
        );

        let contents = fs::read_to_string(project.path().join(doc)).expect("read root doc");
        assert!(
            contents.contains(sentinel),
            "{target} repeat sync should preserve repo-owned {doc}: {contents}"
        );
        assert_eq!(
            contents.matches("metactl:begin").count(),
            1,
            "{target} should have exactly one managed block in {doc}: {contents}"
        );
        let metadata = fs::symlink_metadata(project.path().join(doc)).expect("root doc metadata");
        assert!(
            metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
            "{target} root doc should remain a regular file"
        );
    }
}

#[test]
fn cli_sync_creates_policy_report_parent_dir() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let private_dir = project.path().join(".metactl/private");
    if private_dir.exists() {
        fs::remove_dir_all(&private_dir).expect("remove private dir");
    }

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    assert!(
        private_dir.join("codex-cli-policy-report.json").exists(),
        "sync should recreate .metactl/private before writing policy reports"
    );
}

#[test]
fn cli_sync_brownfield_preview_and_refusal() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    fs::write(project.path().join("AGENTS.md"), "user-owned").expect("seed brownfield file");

    let refused = run_cli(project.path(), &["--json", "sync"]);
    assert_eq!(refused.status.code(), Some(12), "{}", stdout(&refused));
    let refused_json = json_output(&refused);
    assert_eq!(refused_json["ok"], false);
    assert!(refused_json["next_steps"]
        .as_array()
        .expect("next steps")
        .iter()
        .any(|item| item.as_str() == Some("metactl sync --adopt preview")));
    assert_eq!(
        fs::read_to_string(project.path().join("AGENTS.md")).expect("read AGENTS"),
        "user-owned"
    );

    let preview = run_cli(project.path(), &["--json", "sync", "--adopt", "preview"]);
    assert!(preview.status.success(), "{}", stderr(&preview));
    let preview_json = json_output(&preview);
    assert_eq!(preview_json["targets"][0]["status"], "preview");
    assert_eq!(preview_json["preview"], true);
    assert_eq!(
        fs::read_to_string(project.path().join("AGENTS.md")).expect("read AGENTS"),
        "user-owned"
    );
}

#[test]
fn cli_sync_brownfield_refusal_includes_playbook() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    fs::write(project.path().join("AGENTS.md"), "user-owned").expect("seed brownfield file");

    let refused = run_cli(project.path(), &["--json", "sync"]);
    assert_eq!(refused.status.code(), Some(12), "{}", stdout(&refused));
    let refused_json = json_output(&refused);
    assert_eq!(refused_json["ok"], false);

    // Check that the error includes the playbook
    assert!(
        refused_json.get("playbook").is_some(),
        "playbook field should be in JSON output"
    );
    let playbook = refused_json["playbook"]
        .as_str()
        .expect("playbook should be a string");

    // Verify the playbook contains key elements
    assert!(
        playbook.contains("Brownfield adoption strategy"),
        "playbook should mention Brownfield adoption strategy"
    );
    assert!(
        playbook.contains("preview"),
        "playbook should mention preview step"
    );
    assert!(
        playbook.contains("patch"),
        "playbook should mention patch step"
    );
    assert!(
        playbook.contains("takeover"),
        "playbook should mention takeover option"
    );

    // Verify that JSON output doesn't contain ANSI escape codes
    // ANSI escape sequences start with \x1b[
    assert!(
        !playbook.contains("\x1b["),
        "JSON playbook output should not contain ANSI escape codes"
    );
    assert!(
        !playbook.contains("\\x1b["),
        "JSON playbook output should not contain escaped ANSI codes"
    );

    // Verify that next_steps array is still present for backward compatibility
    assert!(refused_json["next_steps"]
        .as_array()
        .expect("next steps")
        .iter()
        .any(|item| item.as_str() == Some("metactl sync --adopt preview")));
}

#[test]
fn cli_sync_patch_adopts_identical_unmanaged_skill_outputs() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let compile = run_cli(project.path(), &["compile"]);
    assert!(compile.status.success(), "{}", stderr(&compile));

    let skill_path = ".codex/skills/python-refactor/python-refactor/SKILL.md";
    let staged = project
        .path()
        .join(".metactl/generated/codex-cli")
        .join(skill_path);
    let destination = project.path().join(skill_path);
    fs::create_dir_all(destination.parent().expect("skill parent")).expect("skill dir");
    fs::copy(&staged, &destination).expect("seed identical unmanaged skill");

    let sync = run_cli(project.path(), &["sync", "--adopt", "patch"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let state = fs::read_to_string(project.path().join(".metactl/state/managed_files.json"))
        .expect("managed state");
    assert!(
        state.contains(skill_path),
        "identical skill output should be adopted into managed state: {state}"
    );
}

#[test]
fn cli_sync_patch_backs_up_conflicting_unmanaged_skill_outputs() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let skill_path = ".codex/skills/python-refactor/python-refactor/SKILL.md";
    let destination = project.path().join(skill_path);
    fs::create_dir_all(destination.parent().expect("skill parent")).expect("skill dir");
    fs::write(&destination, "local unmanaged skill body").expect("seed conflicting skill");
    fs::write(
        project.path().join("AGENTS.md"),
        "# Agents\n\nUNMANAGED_FOLDER_SENTINEL\n",
    )
    .expect("seed AGENTS");

    let sync = run_cli(project.path(), &["sync", "--adopt", "patch"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let agents = fs::read_to_string(project.path().join("AGENTS.md")).expect("read AGENTS");
    assert!(
        agents.contains("UNMANAGED_FOLDER_SENTINEL"),
        "patch adoption should preserve root doc guidance: {agents}"
    );
    assert!(
        agents.contains("metactl:begin"),
        "patch adoption should add a managed block to AGENTS.md: {agents}"
    );

    let state =
        fs::read_to_string(project.path().join(".metactl/state/codex-cli.json")).expect("state");
    assert!(
        state.contains(skill_path),
        "conflicting skill output should be adopted into target state: {state}"
    );
    assert!(
        state.contains(".metactl/state/backups/codex-cli/"),
        "conflicting skill adoption should record a backup path: {state}"
    );
    assert!(
        !fs::read_to_string(&destination)
            .expect("read adopted skill")
            .contains("local unmanaged skill body"),
        "destination should be replaced by generated skill body after backup"
    );
}

#[test]
fn cli_sync_takeover_unsupported_target_shows_alternative() {
    let project = TempDir::new().expect("tempdir");
    // Init with claude-code target (reference-based, doesn't support takeover)
    let output = run_cli(project.path(), &["init", "--target", "claude-code"]);
    assert!(output.status.success(), "{}", stderr(&output));

    // Create unmanaged AGENTS.md to trigger brownfield mode
    fs::write(project.path().join("AGENTS.md"), "user-owned").expect("seed brownfield file");

    // Try sync --adopt takeover
    let refused = run_cli(project.path(), &["--json", "sync", "--adopt", "takeover"]);
    // Should fail with exit code 10 (state error) because takeover is not supported for claude-code
    assert_eq!(
        refused.status.code(),
        Some(10),
        "exit code for unsupported takeover should be 10 (state error): {}",
        stdout(&refused)
    );
    let refused_json = json_output(&refused);
    assert_eq!(refused_json["ok"], false);

    // Check that the error message contains helpful information
    let error_str = stdout(&refused);
    assert!(
        error_str.contains("does not support takeover") || error_str.contains("reference-based"),
        "error should mention takeover not being supported: {}",
        error_str
    );
    assert!(
        error_str.contains("patch")
            || error_str.contains("apply")
            || error_str.contains("Brownfield adoption strategy"),
        "error should suggest patch mode or show playbook: {}",
        error_str
    );
}

#[test]
fn cli_v1_user_private_library_project_link_sync_and_check_flow() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(
        project.path(),
        &["--json", "library", "init", "--user", "--profile", "solo"],
    );
    assert!(init.status.success(), "{}", stderr(&init));
    let init_json = json_output(&init);
    assert_json_contract(&init_json, "library", Some(project.path()));
    let library_root = PathBuf::from(init_json["library_root"].as_str().expect("library root"));
    assert!(library_root.join("packs").is_dir());
    assert!(PathBuf::from(init_json["profile_path"].as_str().expect("profile path")).exists());

    let link = run_cli(
        project.path(),
        &["--json", "project", "link", "--profile", "solo"],
    );
    assert!(link.status.success(), "{}", stderr(&link));
    let link_json = json_output(&link);
    assert_json_contract(&link_json, "project", Some(project.path()));
    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("config");
    assert!(config.contains("extends_profile: solo"), "{config}");

    let preview = run_cli(
        project.path(),
        &["--json", "sync", "--target", "codex,claude", "--preview"],
    );
    assert!(preview.status.success(), "{}", stderr(&preview));
    let preview_json = json_output(&preview);
    assert_json_contract(&preview_json, "sync", Some(project.path()));
    assert_eq!(preview_json["preview"], json!(true));
    assert!(!project.path().join("AGENTS.md").exists());

    let apply = run_cli(
        project.path(),
        &["--json", "sync", "--target", "codex,claude", "--apply"],
    );
    assert!(apply.status.success(), "{}", stderr(&apply));
    assert!(project.path().join("AGENTS.md").exists());
    assert!(project.path().join("CLAUDE.md").exists());

    let check = run_cli(project.path(), &["--json", "check", "--strict"]);
    assert!(check.status.success(), "{}", stderr(&check));
    let check_json = json_output(&check);
    assert_json_contract(&check_json, "validate", Some(project.path()));
    assert_eq!(check_json["strict"], json!(true));
}

#[test]
fn cli_status_reports_discoverability_blockers_before_sync() {
    let project = TempDir::new().expect("tempdir");
    let home = TempDir::new().expect("home");
    let custom_library = TempDir::new().expect("custom library");

    let profiles = home.path().join(".config/metactl/profiles");
    fs::create_dir_all(&profiles).expect("profiles");
    fs::write(
        profiles.join("team-profile.yaml"),
        format!(
            "starter_library:\n  - {}\n",
            custom_library.path().display()
        ),
    )
    .expect("write team-profile profile");
    fs::write(
        project.path().join("metactl.yaml"),
        "extends_profile: team-profile\napi_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- made-up-target\n",
    )
    .expect("write metactl.yaml");

    let status = run_cli_env(
        project.path(),
        &["--json", "status"],
        &[("HOME", home.path().to_str().expect("home"))],
    );
    assert!(status.status.success(), "{}", stderr(&status));
    let json = json_output(&status);
    assert_eq!(json["execution_readiness"], "blocked");
    assert!(json["blocking_checks"]
        .as_array()
        .expect("blocking_checks")
        .iter()
        .any(|item| {
            item["id"] == "target-discovery"
                && item["missing_targets"]
                    .as_array()
                    .map(|targets| targets.iter().any(|target| target == "made-up-target"))
                    .unwrap_or(false)
        }));

    let status_human = run_cli_env(
        project.path(),
        &["status"],
        &[("HOME", home.path().to_str().expect("home"))],
    );
    assert!(status_human.status.success(), "{}", stderr(&status_human));
    let text = stdout(&status_human);
    assert!(text.contains("Execution readiness: blocked"), "{text}");
    assert!(text.contains("configured target made-up-target"), "{text}");
    assert!(text.contains("Next: metactl doctor"), "{text}");
    assert!(!text.contains("Next: metactl sync"), "{text}");
}

#[test]
fn cli_sync_failure_reports_effective_library_roots_and_fix_hint() {
    let project = TempDir::new().expect("tempdir");
    let home = TempDir::new().expect("home");
    let custom_library = TempDir::new().expect("custom library");

    let profiles = home.path().join(".config/metactl/profiles");
    fs::create_dir_all(&profiles).expect("profiles");
    fs::write(
        profiles.join("team-profile.yaml"),
        format!(
            "starter_library:\n  - {}\n",
            custom_library.path().display()
        ),
    )
    .expect("write team-profile profile");
    fs::write(
        project.path().join("metactl.yaml"),
        "extends_profile: team-profile\napi_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- made-up-target\n",
    )
    .expect("write metactl.yaml");

    let sync = run_cli_env(
        project.path(),
        &["--json", "sync"],
        &[("HOME", home.path().to_str().expect("home"))],
    );
    assert_eq!(sync.status.code(), Some(10), "{}", stdout(&sync));
    let json = json_output(&sync);
    assert_eq!(json["reason_code"], "target_discovery_blocked");
    assert!(json["effective_library_roots"]
        .as_array()
        .expect("effective_library_roots")
        .iter()
        .any(|item| item.as_str() == Some(custom_library.path().to_str().expect("custom root"))));
    assert!(json["suggested_actions"]
        .as_array()
        .expect("suggested_actions")
        .iter()
        .any(|item| {
            item.as_str()
                .unwrap_or_default()
                .contains("add a library root that contains targets")
        }));

    let text = json["message"].as_str().unwrap_or_default();
    assert!(text.contains("effective library roots"), "{text}");
    assert!(text.contains("made-up-target"), "{text}");
    assert!(text.contains("metactl doctor"), "{text}");
}

#[test]
fn cli_add_with_sync_compiles_and_applies() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let add = run_cli(
        project.path(),
        &["--json", "add", "unit-test-loop", "--sync"],
    );
    assert!(add.status.success(), "{}", stderr(&add));
    let json = json_output(&add);
    assert_json_contract(&json, "add", Some(project.path()));
    assert!(json.get("sync").is_some());
    assert_eq!(json["sync"]["ok"], true);

    // Verify the agent artifacts were actually applied
    assert!(project.path().join("AGENTS.md").exists());
}

#[test]
fn cli_add_already_configured_with_sync_still_runs_sync() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let first = run_cli(
        project.path(),
        &["--json", "add", "unit-test-loop", "--sync"],
    );
    assert!(first.status.success(), "{}", stderr(&first));

    let second = run_cli(
        project.path(),
        &["--json", "add", "unit-test-loop", "--sync"],
    );
    assert!(second.status.success(), "{}", stderr(&second));
    let json = json_output(&second);
    assert_json_contract(&json, "add", Some(project.path()));
    assert!(json["added"].as_array().expect("added").is_empty());
    assert!(json["already_configured"]
        .as_array()
        .expect("already_configured")
        .iter()
        .any(|item| item == "unit-test-loop"));
    assert!(json.get("sync").is_some());
    assert_eq!(json["sync"]["ok"], true);
    assert!(project.path().join("AGENTS.md").exists());
}

#[test]
fn porcelain_use_resolves_and_syncs() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    // Use a known pack
    let use_out = run_cli(project.path(), &["--json", "use", "unit-test-loop"]);
    assert!(use_out.status.success(), "{}", stderr(&use_out));
    let json = json_output(&use_out);
    assert_json_contract(&json, "use", Some(project.path()));
    assert_eq!(json["resolved_pack"], "unit-test-loop");

    // Verify pack was added to config
    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("read config");
    assert!(config.contains("unit-test-loop"));

    // Verify sync ran (artifacts exist)
    assert!(project.path().join("AGENTS.md").exists());
}

#[test]
fn cli_sync_multi_target_shared_agents_md_uses_single_owner() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(
        project.path(),
        &["init", "--target", "codex-cli", "--target", "cursor"],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    assert!(
        project.path().join("AGENTS.md").exists(),
        "codex-cli should still own the root AGENTS.md"
    );
    assert!(
        project
            .path()
            .join(".cursor/rules/metactl-pack-index.mdc")
            .exists(),
        "cursor should still emit its target-local rule index"
    );
    assert!(
        !project
            .path()
            .join(".metactl/generated/cursor/AGENTS.md")
            .exists(),
        "cursor should not stage a duplicate root AGENTS.md when codex-cli is enabled"
    );
}

#[test]
fn cli_sync_multi_target_root_instruction_outputs_are_regular_files() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(
        project.path(),
        &[
            "init",
            "--target",
            "codex-cli",
            "--target",
            "claude-code",
            "--target",
            "gemini-cli",
        ],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    for file in ["AGENTS.md", "CLAUDE.md", "GEMINI.md"] {
        let path = project.path().join(file);
        let metadata = fs::symlink_metadata(&path).unwrap_or_else(|err| {
            panic!("read metadata for {}: {}", path.display(), err);
        });
        assert!(
            metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
            "{file} should be a regular file under default symlink-capable sync"
        );
    }
}

#[test]
fn cli_sync_claude_settings_is_regular_file_not_symlink() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(project.path(), &["init", "--target", "claude-code"]);
    assert!(init.status.success(), "{}", stderr(&init));

    let sync = run_cli(project.path(), &["sync", "--yes"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let settings_path = project.path().join(".claude/settings.json");
    assert!(
        settings_path.exists(),
        "Claude settings should exist after sync"
    );
    assert!(
        !settings_path.is_symlink(),
        "shared Claude settings should be materialized as a regular file, not a symlink"
    );
}

#[test]
fn cli_sync_recreates_missing_managed_claude_settings() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(project.path(), &["init", "--target", "claude-code"]);
    assert!(init.status.success(), "{}", stderr(&init));

    let first_sync = run_cli(project.path(), &["sync", "--yes"]);
    assert!(first_sync.status.success(), "{}", stderr(&first_sync));

    let settings_path = project.path().join(".claude/settings.json");
    fs::remove_file(&settings_path).expect("remove managed settings.json");

    let second_sync = run_cli(project.path(), &["sync", "--yes"]);
    assert!(second_sync.status.success(), "{}", stderr(&second_sync));
    assert!(
        settings_path.exists(),
        "missing managed Claude settings should be recreated"
    );
    assert!(
        !settings_path.is_symlink(),
        "recreated Claude settings should remain a regular file"
    );
}

#[test]
fn cli_sync_claude_settings_patch_preserves_user_hooks_and_settings() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(project.path(), &["init", "--target", "claude-code"]);
    assert!(init.status.success(), "{}", stderr(&init));

    let add = run_cli(project.path(), &["add", "migration-guard"]);
    assert!(add.status.success(), "{}", stderr(&add));

    let claude_dir = project.path().join(".claude");
    fs::create_dir_all(&claude_dir).expect("create .claude dir");
    fs::write(
        claude_dir.join("settings.json"),
        r#"{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "command": "env OPENDREAM_WORKSPACE=\"$CLAUDE_PROJECT_DIR\" sh \"$CLAUDE_PROJECT_DIR\"/.opendream/hooks/claude-post-task.sh",
            "type": "command"
          }
        ]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "command": "env OPENDREAM_WORKSPACE=\"$CLAUDE_PROJECT_DIR\" sh \"$CLAUDE_PROJECT_DIR\"/.opendream/hooks/claude-pre-task.sh",
            "type": "command"
          }
        ]
      }
    ]
  },
  "customSetting": "keep-me"
}"#,
    )
    .expect("seed claude settings");

    let sync = run_cli(project.path(), &["sync", "--adopt", "patch", "--yes"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let settings_path = claude_dir.join("settings.json");
    let merged: Value = serde_json::from_str(
        &fs::read_to_string(&settings_path).expect("read merged settings.json"),
    )
    .expect("parse merged settings.json");

    let has_command = |value: &Value, event: &str, command: &str| -> bool {
        value["hooks"][event]
            .as_array()
            .map(|entries| {
                entries.iter().any(|entry| {
                    entry["hooks"].as_array().is_some_and(|hooks| {
                        hooks
                            .iter()
                            .any(|hook| hook["command"].as_str() == Some(command))
                    })
                })
            })
            .unwrap_or(false)
    };

    assert!(
        has_command(
            &merged,
            "Stop",
            "env OPENDREAM_WORKSPACE=\"$CLAUDE_PROJECT_DIR\" sh \"$CLAUDE_PROJECT_DIR\"/.opendream/hooks/claude-post-task.sh"
        ),
        "existing Stop hook should be preserved: {merged:#}"
    );
    assert!(
        has_command(
            &merged,
            "UserPromptSubmit",
            "env OPENDREAM_WORKSPACE=\"$CLAUDE_PROJECT_DIR\" sh \"$CLAUDE_PROJECT_DIR\"/.opendream/hooks/claude-pre-task.sh"
        ),
        "existing UserPromptSubmit hook should be preserved: {merged:#}"
    );
    assert!(
        has_command(
            &merged,
            "PostToolUse",
            ".claude/hooks/migration-guard/hook.sh"
        ),
        "metactl-managed PostToolUse hook should be merged in: {merged:#}"
    );
    assert_eq!(merged["customSetting"], json!("keep-me"));
    assert!(
        merged.get("permissions").is_none(),
        "patch sync should not inject permissions into an existing settings.json: {merged:#}"
    );

    let mut user_edited = merged.clone();
    user_edited["permissions"] = json!({
        "allow": ["Read", "Glob", "Grep", "WebFetch"],
        "ask": ["Write"],
        "deny": ["Bash(rm -rf:*)"]
    });
    user_edited["hooks"]["SessionStart"] = json!([
        {
            "hooks": [
                {
                    "type": "command",
                    "command": "echo session-start"
                }
            ]
        }
    ]);
    fs::write(
        &settings_path,
        serde_json::to_vec_pretty(&user_edited).expect("serialize edited settings"),
    )
    .expect("write edited settings");

    let second_sync = run_cli(project.path(), &["sync", "--yes"]);
    assert!(second_sync.status.success(), "{}", stderr(&second_sync));

    let resynced: Value = serde_json::from_str(
        &fs::read_to_string(&settings_path).expect("read resynced settings.json"),
    )
    .expect("parse resynced settings.json");
    assert!(
        has_command(
            &resynced,
            "Stop",
            "env OPENDREAM_WORKSPACE=\"$CLAUDE_PROJECT_DIR\" sh \"$CLAUDE_PROJECT_DIR\"/.opendream/hooks/claude-post-task.sh"
        ),
        "existing Stop hook should survive re-sync: {resynced:#}"
    );
    assert!(
        has_command(&resynced, "SessionStart", "echo session-start"),
        "user-added SessionStart hook should survive re-sync: {resynced:#}"
    );
    assert!(
        has_command(
            &resynced,
            "PostToolUse",
            ".claude/hooks/migration-guard/hook.sh"
        ),
        "metactl-managed PostToolUse hook should survive re-sync: {resynced:#}"
    );
    assert_eq!(
        resynced["permissions"],
        json!({
            "allow": ["Read", "Glob", "Grep", "WebFetch"],
            "ask": ["Write"],
            "deny": ["Bash(rm -rf:*)"]
        })
    );
}

// ── Visibility scope tests ──────────────────────────────────────────────

#[test]
fn cli_sync_all_targets_no_double_path_segments() {
    for target in [
        "claude-code",
        "cursor",
        "codex-cli",
        "gemini-cli",
        "openclaw",
    ] {
        let project = TempDir::new().expect("tempdir");
        let init = run_cli(
            project.path(),
            &["--no-input", "-y", "init", "--target", target],
        );
        assert!(
            init.status.success(),
            "init --target {} failed: {}",
            target,
            stderr(&init)
        );
        let sync = run_cli(project.path(), &["--no-input", "-y", "sync"]);
        assert!(
            sync.status.success(),
            "sync for {} failed: {}",
            target,
            stderr(&sync)
        );
        let add = run_cli(
            project.path(),
            &["--no-input", "-y", "add", "unit-test-loop", "--sync"],
        );
        assert!(
            add.status.success(),
            "add --sync for {} failed: {}",
            target,
            stderr(&add)
        );

        let mut files = Vec::new();
        walk_project_files(project.path(), &mut files);
        for path in &files {
            let p = path.to_string_lossy().to_string();
            for seg in ["commands", "rules", "scripts", "plugins", "hooks", "skills"] {
                let needle = format!("/{seg}/");
                if let Some(first) = p.find(&needle) {
                    let rest = &p[first + needle.len()..];
                    assert!(
                        !rest.contains(&needle),
                        "target={target} doubled {seg} segment in {p}"
                    );
                }
            }
        }
    }
}

#[test]
fn cli_sync_gemini_produces_extension_bundle() {
    let project = TempDir::new().expect("tempdir");
    let init = run_cli(
        project.path(),
        &["--no-input", "-y", "init", "--target", "gemini-cli"],
    );
    assert!(init.status.success(), "init failed: {}", stderr(&init));
    let sync = run_cli(project.path(), &["--no-input", "-y", "sync"]);
    assert!(sync.status.success(), "sync failed: {}", stderr(&sync));

    let manifest_path = project
        .path()
        .join(".gemini/extensions/python-refactor/gemini-extension.json");
    assert!(
        manifest_path.exists(),
        "extension manifest missing at {}",
        manifest_path.display()
    );
    let parsed: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest"))
            .expect("parse manifest");
    assert_eq!(parsed["name"], "python-refactor");
    assert_eq!(parsed["contextFileName"], "GEMINI.md");

    let context_path = project
        .path()
        .join(".gemini/extensions/python-refactor/GEMINI.md");
    assert!(
        context_path.exists(),
        "GEMINI.md missing at {}",
        context_path.display()
    );

    // Iterate the skills dir and assert at least one SKILL.md exists.
    let skills_root = project
        .path()
        .join(".gemini/extensions/python-refactor/skills");
    assert!(
        skills_root.exists(),
        "skills dir missing at {}",
        skills_root.display()
    );
    let mut skill_files = Vec::new();
    walk_project_files(&skills_root, &mut skill_files);
    let found_skill = skill_files
        .iter()
        .any(|p| p.file_name().and_then(|n| n.to_str()) == Some("SKILL.md"));
    assert!(
        found_skill,
        "no SKILL.md found under {}; files: {:?}",
        skills_root.display(),
        skill_files
    );
}
