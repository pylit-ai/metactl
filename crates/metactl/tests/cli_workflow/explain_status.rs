use super::*;

// Explain, status, and doctor workflow tests.

#[test]
fn surface_overrides_and_auto_explain_are_machine_readable() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    let usage_dir = project.path().join(".metactl/usage");
    fs::create_dir_all(&usage_dir).expect("usage dir");
    fs::write(
        usage_dir.join("events.jsonl"),
        r#"{"event_kind":"command_invoked","pack_id":"python-refactor","recorded_at":"2026-05-21T10:00:00Z"}
"#,
    )
    .expect("events");

    let pin = run_cli(
        project.path(),
        &["--json", "surface", "pin", "python-refactor"],
    );
    assert!(pin.status.success(), "{}", stderr(&pin));
    let pin_json = json_output(&pin);
    assert_eq!(
        pin_json["overrides"]["overrides"]["python-refactor"]["action"],
        "pin_hot"
    );

    let report = run_cli(project.path(), &["--json", "surface", "report"]);
    assert!(report.status.success(), "{}", stderr(&report));
    let report_json = json_output(&report);
    let recommendations = report_json["report"]["recommendations"]
        .as_array()
        .expect("recommendations");
    assert!(recommendations.iter().any(|item| {
        item["pack_id"] == "python-refactor"
            && item["tier"] == "hot"
            && item["reason_code"] == "operator_pin_hot"
    }));

    let explain = run_cli(
        project.path(),
        &["--json", "explain", "--surface-mode", "auto"],
    );
    assert!(explain.status.success(), "{}", stderr(&explain));
    let explain_json = json_output(&explain);
    assert_eq!(
        explain_json["target_projection"]["surface_selection_mode"],
        "auto"
    );
    assert_eq!(
        explain_json["surface_usage"]["next_reversible_action"],
        "metactl surface report"
    );
}

#[test]
fn doctor_agent_json_has_recoverable_next_commands() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let output = run_cli(project.path(), &["--agent", "doctor"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "doctor", Some(project.path()));
    let checks = value["checks"].as_array().expect("checks");
    assert!(checks
        .iter()
        .any(|item| item["id"] == "ignore-repair" && item["next_commands"].as_array().is_some()));
}

#[test]
fn status_reports_codex_skill_visibility_scopes() {
    let project = TempDir::new().expect("tempdir");
    let setup = run_cli(
        project.path(),
        &["--json", "setup", "--target", "codex-cli", "--yes"],
    );
    assert!(setup.status.success(), "{}", stderr(&setup));
    let repo_skill_root = project
        .path()
        .join(".codex/skills/team-pack/release-manager");
    fs::create_dir_all(&repo_skill_root).expect("repo skill dir");
    fs::write(
        repo_skill_root.join("SKILL.md"),
        r#"---
name: release-manager
description: Portable release manager skill for verification handoffs.
---

# Release Manager
"#,
    )
    .expect("write repo skill");

    let status = run_cli(project.path(), &["--json", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json = json_output(&status);
    assert_json_contract(&json, "status", Some(project.path()));
    assert_eq!(json["skill_visibility"]["target"], json!("codex-cli"));
    assert_eq!(
        json["agent_artifact_policy"]["policy"],
        json!("portable-first")
    );
    assert_eq!(
        json["agent_artifact_policy"]["stewardship_pack_configured"],
        json!(true)
    );
    assert_eq!(json["skill_visibility"]["repo_local_count"], json!(1));
    assert_eq!(
        json["skill_visibility"]["missing_user_global_count"],
        json!(1)
    );
    assert_eq!(
        json["skill_visibility"]["repo_local_skills"][0]["user_global_installed"],
        json!(false)
    );

    let human = run_cli(project.path(), &["status"]);
    assert!(human.status.success(), "{}", stderr(&human));
    let text = stdout(&human);
    assert!(text.contains("Codex skill visibility:"));
    assert!(text.contains("Agent artifact stewardship:"));
    assert!(text.contains("policy: portable-first"));
    assert!(text.contains("repo-local: 1 skill(s)"));
    assert!(text.contains("user-global: 0 skill(s)"));
    assert!(text.contains("metactl skills add <repo-skill-path> --scope user"));
}

#[test]
fn cli_lock_and_doctor_detects_stale_lock() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    let compile = run_cli(project.path(), &["compile"]);
    assert!(compile.status.success(), "{}", stderr(&compile));

    let config_path = project.path().join("metactl.yaml");
    let updated = fs::read_to_string(&config_path)
        .expect("read config")
        .replace("builder", "reviewer");
    fs::write(&config_path, updated).expect("write config");

    let compile_stale = run_cli(project.path(), &["compile"]);
    assert_eq!(
        compile_stale.status.code(),
        Some(11),
        "{}",
        stdout(&compile_stale)
    );
    assert!(stderr(&compile_stale).contains("stale"));

    let doctor = run_cli(project.path(), &["--json", "doctor"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let json = json_output(&doctor);
    let checks = json["checks"].as_array().expect("checks array");
    assert!(checks
        .iter()
        .any(|item| item["id"] == "lock" && item["status"] == "fail"));
}

#[test]
fn cli_lock_and_doctor_weak_corpus() {
    let project = TempDir::new().expect("tempdir");
    let empty_library = TempDir::new().expect("empty library");
    let init = run_cli(
        project.path(),
        &[
            "init",
            "--target",
            "codex-cli",
            "--starter-library",
            empty_library.path().to_str().expect("library path"),
        ],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    let search = run_cli(project.path(), &["--json", "search", "python refactor"]);
    assert!(search.status.success(), "{}", stderr(&search));
    let json = json_output(&search);
    assert_eq!(json["classification"], "matches");
    assert!(json["result_count"].as_u64().unwrap_or_default() > 0);

    let doctor = run_cli(project.path(), &["--json", "doctor"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let checks = json_output(&doctor)["checks"]
        .as_array()
        .expect("doctor checks")
        .clone();
    assert!(checks
        .iter()
        .any(|item| item["id"] == "starter-library" && item["status"] == "pass"));
}

#[test]
fn cli_init_human_output_explains_machine_default_binding_choice() {
    let project = TempDir::new().expect("tempdir");
    let home = TempDir::new().expect("home");
    seed_user_default_profile(
        home.path(),
        "team-profile",
        &format!(
            "targets:\n  - codex-cli\nstarter_library:\n  - {}\npacks:\n  - python-refactor\n",
            starter_library_root()
        ),
    );
    let init = run_cli_env(
        project.path(),
        &["init"],
        &[("HOME", home.path().to_str().expect("home path"))],
    );
    assert!(init.status.success(), "{}", stderr(&init));
    let text = stdout(&init);
    assert!(text.contains("Applied machine default profile from user settings locally"));
    assert!(text.contains("Leave it this way for a portable repo"));
    assert!(text.contains("metactl init --bind-profile"));
}

#[test]
fn cli_status_human_output_explains_machine_default_binding_choice() {
    let project = TempDir::new().expect("tempdir");
    let home = TempDir::new().expect("home");
    seed_user_default_profile(
        home.path(),
        "team-profile",
        &format!(
            "targets:\n  - codex-cli\nstarter_library:\n  - {}\npacks:\n  - python-refactor\n",
            starter_library_root()
        ),
    );
    let init = run_cli_env(
        project.path(),
        &["init"],
        &[("HOME", home.path().to_str().expect("home path"))],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    let status = run_cli_env(
        project.path(),
        &["status"],
        &[("HOME", home.path().to_str().expect("home path"))],
    );
    assert!(status.status.success(), "{}", stderr(&status));
    let text = stdout(&status);
    assert!(text.contains("Machine default profile team-profile is active locally"));
    assert!(text.contains("metactl init --bind-profile"));
}

#[test]
fn cli_explain_verbose_shows_surface_detail() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let explain = run_cli(project.path(), &["--json", "--verbose", "explain"]);
    assert!(explain.status.success(), "{}", stderr(&explain));
    let json = json_output(&explain);
    let surface_details = json["surface_details"]
        .as_array()
        .expect("surface_details array");
    assert!(surface_details
        .iter()
        .any(|item| item["pack_ref"]["id"] == "python-refactor"));
    assert!(surface_details.iter().any(|item| {
        item["pack_ref"]["id"] == "python-refactor"
            && item["surfaces"]
                .as_array()
                .map(|surfaces| {
                    surfaces.iter().any(|surface| {
                        surface["surface_slug"] == "contracts"
                            && surface["emitted"] == false
                            && surface["reason_code"] == "suppressed_by_mode"
                    })
                })
                .unwrap_or(false)
    }));
}

#[test]
fn cli_status_flags_transient_surface_mode_mismatch() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let sync = run_cli(project.path(), &["sync", "--surface-mode", "full"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let status = run_cli(project.path(), &["--json", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_json = json_output(&status);
    assert_eq!(status_json["lock_stale"], false);
    assert_eq!(status_json["needs_sync"], true);
    assert_eq!(
        status_json["applied_targets"][0]["surface_selection_mode"],
        "full"
    );
    assert_eq!(
        status_json["applied_targets"][0]["configured_surface_selection_mode"],
        "minimal"
    );
    assert_eq!(
        status_json["applied_targets"][0]["surface_selection_mode_matches_config"],
        false
    );
    assert_eq!(
        status_json["surface_mode_mismatches"][0]["target"],
        "codex-cli"
    );

    let human = run_cli(project.path(), &["status"]);
    assert!(human.status.success(), "{}", stderr(&human));
    let text = stdout(&human);
    assert!(
        text.contains("surface: full, next sync: minimal"),
        "status output should explain the applied/configured surface-mode mismatch:\n{text}"
    );
}

#[test]
fn cli_explain_reports_emitted_and_suppressed_surfaces() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let explain = run_cli(project.path(), &["--json", "explain"]);
    assert!(explain.status.success(), "{}", stderr(&explain));
    let json = json_output(&explain);

    let python_refactor = json["surface_details"]
        .as_array()
        .expect("surface_details array")
        .iter()
        .find(|item| item["pack_ref"]["id"] == "python-refactor")
        .expect("python-refactor surface detail");
    let surfaces = python_refactor["surfaces"].as_array().expect("surfaces");

    assert!(surfaces.iter().any(|surface| {
        surface["surface_slug"] == "python-refactor" && surface["emitted"] == true
    }));
    assert!(surfaces.iter().any(|surface| {
        surface["surface_slug"] == "contracts"
            && surface["emitted"] == false
            && surface["reason_code"] == "suppressed_by_mode"
    }));

    let human = run_cli(project.path(), &["explain"]);
    assert!(human.status.success(), "{}", stderr(&human));
    let text = stdout(&human);
    assert!(
        text.contains("Surface selection mode: minimal"),
        "explain output should show effective surface mode:\n{text}"
    );
    assert!(
        text.contains("pack python-refactor: 1 emitted, 2 suppressed"),
        "explain output should distinguish emitted from suppressed surfaces:\n{text}"
    );
}

#[test]
fn cli_explain_reports_target_projection() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let explain = run_cli(project.path(), &["--json", "explain"]);
    assert!(explain.status.success(), "{}", stderr(&explain));
    let json = json_output(&explain);
    assert_eq!(json["target_projection"]["target_id"], "codex-cli");
    assert!(json["target_projection"]["summary"]
        .as_str()
        .unwrap_or_default()
        .contains("AGENTS.md"));
    assert!(json["target_projection"]["summary"]
        .as_str()
        .unwrap_or_default()
        .contains("skills/{pack_id}/{surface_slug}/SKILL.md"));
    assert_eq!(
        json["target_projection"]["outputs"][0]["instruction_mode"],
        "reference_index"
    );
    assert!(json["target_projection"]["instruction_behavior"]
        .as_str()
        .unwrap_or_default()
        .contains("references emitted pack bodies"));
    assert!(json["target_projection"]["instruction_budget"]
        .as_str()
        .unwrap_or_default()
        .contains("8192"));
    assert!(json["target_projection"]["surface_behavior"]
        .as_str()
        .unwrap_or_default()
        .contains("separate"));

    let human = run_cli(project.path(), &["explain"]);
    assert!(human.status.success(), "{}", stderr(&human));
    assert!(stdout(&human).contains("Projection:"));
    assert!(stdout(&human).contains("references emitted pack bodies"));
}

#[test]
fn cli_explain_reports_reference_instruction_projection() {
    let project = TempDir::new().expect("tempdir");
    let init = run_cli(
        project.path(),
        &[
            "init",
            "--role",
            "reviewer",
            "--policy",
            "safe-review",
            "--target",
            "claude-code",
        ],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    let explain = run_cli(project.path(), &["--json", "explain"]);
    assert!(explain.status.success(), "{}", stderr(&explain));
    let json = json_output(&explain);
    assert_eq!(json["target_projection"]["target_id"], "claude-code");
    assert_eq!(
        json["target_projection"]["outputs"][0]["instruction_mode"],
        "reference_index"
    );
    assert!(json["target_projection"]["instruction_behavior"]
        .as_str()
        .unwrap_or_default()
        .contains("entry document concise"));

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    let claude_md = fs::read_to_string(project.path().join("CLAUDE.md")).expect("CLAUDE.md");
    assert!(claude_md.contains(".claude/skills/unit-test-loop/"));
    assert!(claude_md.contains("Prefer retrieval-led reasoning"));
    assert!(!claude_md.contains("Run the narrowest relevant test loop before closing work."));
}

#[test]
fn cli_explain_reports_instruction_budget_behavior() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let explain = run_cli(project.path(), &["--json", "explain"]);
    assert!(explain.status.success(), "{}", stderr(&explain));
    let json = json_output(&explain);
    assert!(json["target_projection"]["instruction_budget"]
        .as_str()
        .unwrap_or_default()
        .contains("8192"));

    let human = run_cli(project.path(), &["explain"]);
    assert!(human.status.success(), "{}", stderr(&human));
    assert!(stdout(&human).contains("32768"));
}

#[test]
fn cli_status_shows_project_state() {
    let project = TempDir::new().expect("tempdir");

    // Status before init
    let status_before = run_cli(project.path(), &["--json", "status"]);
    assert!(status_before.status.success(), "{}", stderr(&status_before));
    let json_before = json_output(&status_before);
    assert_json_contract(&json_before, "status", Some(project.path()));
    assert_eq!(json_before["initialized"], false);

    // Status after init
    init_project(project.path());
    let status_after = run_cli(project.path(), &["--json", "status"]);
    assert!(status_after.status.success(), "{}", stderr(&status_after));
    let json_after = json_output(&status_after);
    assert_eq!(json_after["initialized"], true);
    assert!(json_after["role"].is_string());
    assert!(json_after["targets"].is_array());
    assert_eq!(json_after["needs_sync"], true);

    // Status after sync
    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    let status_synced = run_cli(project.path(), &["--json", "status"]);
    assert!(status_synced.status.success(), "{}", stderr(&status_synced));
    let json_synced = json_output(&status_synced);
    assert_eq!(json_synced["lock_stale"], false);
    assert!(!json_synced["applied_targets"]
        .as_array()
        .expect("applied_targets")
        .is_empty());
    assert_eq!(
        json_synced["applied_targets"][0]["surface_selection_mode"],
        "minimal"
    );
    assert!(
        json_synced["applied_targets"][0]["generated_outputs"]
            .as_u64()
            .expect("generated_outputs")
            > 0
    );
}

#[test]
fn status_shows_provenance_layers_and_stale_reason() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    // Sync so we have locked targets
    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    // Status should include layers in JSON
    let status = run_cli(project.path(), &["--json", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json = json_output(&status);
    assert_eq!(json["initialized"], true);

    // Must have layers array with at least the shared layer
    let layers = json["layers"].as_array().expect("layers array");
    assert!(
        layers.iter().any(|l| l["layer"] == "shared"),
        "should have shared layer: {:?}",
        layers
    );

    // Stale reason should be null when not stale
    assert!(
        json["stale_reason"].is_null(),
        "stale_reason should be null when lock is fresh"
    );

    // Human output should include Layers section
    let status_human = run_cli(project.path(), &["status"]);
    assert!(status_human.status.success(), "{}", stderr(&status_human));
    let text = stdout(&status_human);
    assert!(text.contains("Layers:"), "human output should show layers");

    // Now make it stale by editing config
    let config = std::fs::read_to_string(project.path().join("metactl.yaml")).expect("read config");
    std::fs::write(
        project.path().join("metactl.yaml"),
        format!("{}\n# modified\n", config),
    )
    .expect("modify config");

    let status2 = run_cli(project.path(), &["--json", "status"]);
    assert!(status2.status.success(), "{}", stderr(&status2));
    let json2 = json_output(&status2);
    assert_eq!(json2["lock_stale"], true);
    assert_eq!(json2["stale_reason"], "config changed");
}

#[test]
fn status_shows_local_config_layer() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    // Add local config
    fs::write(
        project.path().join("metactl.local.yaml"),
        "packs:\n  - unit-test-loop\n",
    )
    .expect("write local config");

    let status = run_cli(project.path(), &["--json", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json = json_output(&status);
    let layers = json["layers"].as_array().expect("layers array");
    assert!(
        layers.iter().any(|l| l["layer"] == "local"),
        "should have local layer when metactl.local.yaml exists: {:?}",
        layers
    );
}

#[test]
fn provenance_ledger_in_status_output() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let status = run_cli(project.path(), &["--json", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json = json_output(&status);

    // Should reflect initialized project with targets and packs
    assert_eq!(json["initialized"], true, "status should show initialized");
    assert!(
        json["targets"].as_array().is_some(),
        "status JSON should contain targets array"
    );
}

#[test]
fn hook_status_reports_installed_hooks() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let git_init = std::process::Command::new("git")
        .args(["init"])
        .current_dir(project.path())
        .output()
        .expect("git init");
    assert!(git_init.status.success());

    // Before install
    let status_before = run_cli(project.path(), &["--json", "hook", "status"]);
    assert!(status_before.status.success(), "{}", stderr(&status_before));
    let json_before = json_output(&status_before);
    let hooks_before = json_before["hooks"].as_array().expect("hooks array");
    assert!(hooks_before
        .iter()
        .all(|h| h["has_metactl"] == false || h["exists"] == false));

    // Install
    let install = run_cli(project.path(), &["hook", "install"]);
    assert!(install.status.success(), "{}", stderr(&install));

    // After install
    let status_after = run_cli(project.path(), &["--json", "hook", "status"]);
    assert!(status_after.status.success(), "{}", stderr(&status_after));
    let json_after = json_output(&status_after);
    let hooks_after = json_after["hooks"].as_array().expect("hooks array");
    assert!(hooks_after
        .iter()
        .any(|h| h["hook"] == "post-checkout" && h["has_metactl"] == true));
}

#[test]
fn doctor_reports_local_config_and_projection_checks() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let doctor = run_cli(project.path(), &["--json", "doctor"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let json = json_output(&doctor);
    let checks = json["checks"].as_array().expect("checks array");

    // Should have local-config check
    assert!(
        checks.iter().any(|c| c["id"] == "local-config"),
        "doctor should include local-config check: {:?}",
        checks.iter().map(|c| &c["id"]).collect::<Vec<_>>()
    );

    // Should have input-provenance check
    assert!(
        checks.iter().any(|c| c["id"] == "input-provenance"),
        "doctor should include input-provenance check"
    );
}

#[test]
fn explain_includes_certificates() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let explain = run_cli(project.path(), &["--json", "explain"]);
    assert!(explain.status.success(), "{}", stderr(&explain));
    let json = json_output(&explain);

    let certs = json["certificates"].as_array();
    assert!(
        certs.is_some(),
        "explain JSON should contain certificates array"
    );
    let certs = certs.unwrap();
    assert!(
        !certs.is_empty(),
        "should have at least one explanation certificate"
    );
    // Verify certificate structure
    let cert = &certs[0];
    assert!(cert.get("subject").is_some(), "certificate needs subject");
    assert!(cert.get("premises").is_some(), "certificate needs premises");
    assert!(cert.get("evidence").is_some(), "certificate needs evidence");
    assert!(
        cert.get("conclusion").is_some(),
        "certificate needs conclusion"
    );
}

#[test]
fn cursor_target_projection_in_status() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(project.path(), &["init", "--target", "cursor"]);
    assert!(init.status.success(), "{}", stderr(&init));

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let status = run_cli(project.path(), &["--json", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json = json_output(&status);

    let applied = json["applied_targets"].as_array().expect("applied_targets");
    let cursor_target = applied
        .iter()
        .find(|t| t["target"] == "cursor")
        .expect("should have cursor in applied targets");

    assert_eq!(
        cursor_target["projection"], "exact",
        "cursor should report exact projection in status"
    );
}

#[test]
fn cli_status_reports_shared_agents_md_owner() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(
        project.path(),
        &["init", "--target", "codex-cli", "--target", "cursor"],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    let status = run_cli(project.path(), &["--json", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json = json_output(&status);
    let rules = json["shared_surface_rules"]
        .as_array()
        .expect("shared_surface_rules");
    let agents_rule = rules
        .iter()
        .find(|rule| rule["path"] == "AGENTS.md")
        .expect("AGENTS.md shared-surface rule");

    assert_eq!(agents_rule["owner"], "codex-cli");
    assert!(
        agents_rule["suppressed_targets"]
            .as_array()
            .expect("suppressed_targets")
            .iter()
            .any(|target| target == "cursor"),
        "cursor should be listed as a suppressed secondary target: {:?}",
        agents_rule
    );
}

#[test]
fn cli_doctor_detects_brownfield_and_suggests_preview() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    // Create unmanaged AGENTS.md file to simulate brownfield state
    let agents_path = project.path().join("AGENTS.md");
    fs::write(&agents_path, "# Unmanaged AGENTS.md\n").expect("write AGENTS.md");

    // Run doctor with unmanaged files present
    let doctor = run_cli(project.path(), &["--json", "doctor"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let json = json_output(&doctor);
    let checks = json["checks"].as_array().expect("checks array");

    // Verify brownfield detection check is present
    let brownfield_check = checks.iter().find(|c| c["id"] == "brownfield-detection");
    assert!(
        brownfield_check.is_some(),
        "doctor should include brownfield-detection check"
    );

    let brownfield = brownfield_check.unwrap();
    assert_eq!(
        brownfield["status"], "warn",
        "brownfield should be a warning"
    );

    // Verify the message mentions the detected files
    let message = brownfield["message"].as_str().expect("message");
    assert!(
        message.contains("AGENTS.md"),
        "message should mention AGENTS.md: {}",
        message
    );

    // Verify the message suggests preview mode
    assert!(
        message.contains("preview"),
        "message should mention 'preview': {}",
        message
    );

    // Verify exit code is still 0 (warning, not error)
    assert_eq!(
        doctor.status.code(),
        Some(0),
        "doctor should exit with code 0"
    );

    // Verify human output also contains the hint
    let doctor_human = run_cli(project.path(), &["doctor"]);
    let human_text = stdout(&doctor_human);
    assert!(
        human_text.contains("brownfield-detection") || human_text.contains("AGENTS.md"),
        "human output should mention brownfield: {}",
        human_text
    );
}
