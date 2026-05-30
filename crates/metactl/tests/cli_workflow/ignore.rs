use super::*;

// Ignore and repository hygiene workflow tests.

#[test]
fn ignore_status_reports_tracked_generated_roots() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_generated_root(project.path(), ".codex/skills/example/SKILL.md");

    let output = run_cli(
        project.path(),
        &["--json", "ignore", "status", "--target", "codex-cli"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "ignore", Some(project.path()));
    assert_eq!(value["tracked_generated_roots"][0]["root"], ".codex");
    assert!(value["next_commands"][0]
        .as_str()
        .expect("next command")
        .contains("metactl ignore fix --plan"));
}

#[test]
fn ignore_status_agent_json_has_next_commands() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let output = run_cli(
        project.path(),
        &["--agent", "ignore", "status", "--target", "codex-cli"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "ignore", Some(project.path()));
    assert!(
        value["next_commands"]
            .as_array()
            .expect("next commands")
            .len()
            >= 1
    );
}

#[test]
fn ignore_target_resolution_prefers_configured_then_detected() {
    let project = TempDir::new().expect("tempdir");
    let init = run_cli(project.path(), &["init", "--target", "gemini-cli"]);
    assert!(init.status.success(), "{}", stderr(&init));
    fs::create_dir_all(project.path().join(".codex/skills/example")).expect("codex dir");

    let output = run_cli(project.path(), &["--json", "ignore", "status"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_eq!(value["target_source"], "config");
    assert_eq!(value["targets"], json!(["gemini-cli"]));
}

#[test]
fn ignore_fix_plan_reports_actions_without_writes() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_generated_root(project.path(), ".codex/skills/example/SKILL.md");
    let before_files = project_file_snapshot(project.path());
    let before_index = git_ls_files(project.path());

    let output = run_cli(
        project.path(),
        &["--json", "ignore", "fix", "--plan", "--target", "codex-cli"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "ignore", Some(project.path()));
    assert_eq!(value["plan"], json!(true));
    assert!(value["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .any(|item| item["kind"] == "untrack-generated"));
    assert_eq!(project_file_snapshot(project.path()), before_files);
    assert_eq!(git_ls_files(project.path()), before_index);
}

#[test]
fn ignore_fix_yes_untracks_generated_roots_without_deleting_files() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_generated_root(project.path(), ".codex/skills/example/SKILL.md");

    let output = run_cli(
        project.path(),
        &[
            "--json",
            "ignore",
            "fix",
            "--scope",
            "both",
            "--target",
            "codex-cli",
            "--untrack-generated",
            "--yes",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_eq!(value["untracked_generated_roots"][0]["path"], ".codex");
    assert!(project
        .path()
        .join(".codex/skills/example/SKILL.md")
        .exists());
    assert!(!git_ls_files(project.path()).contains(".codex/skills/example/SKILL.md"));
}

#[test]
fn ignore_fix_no_input_requires_untrack_generated() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_generated_root(project.path(), ".codex/skills/example/SKILL.md");

    let output = run_cli(
        project.path(),
        &[
            "--no-input",
            "--json",
            "ignore",
            "fix",
            "--target",
            "codex-cli",
            "--yes",
        ],
    );
    assert!(
        !output.status.success(),
        "ignore fix should require explicit untrack"
    );
    let value = json_output(&output);
    assert_eq!(value["code"], "untrack_generated_required");
    assert!(git_ls_files(project.path()).contains(".codex/skills/example/SKILL.md"));
}

#[test]
fn ignore_fix_agent_plan_json_is_parseable() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let output = run_cli(project.path(), &["--agent", "ignore", "fix", "--plan"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "ignore", Some(project.path()));
    assert_eq!(value["action"], "fix");
    assert_eq!(value["plan"], json!(true));
}

#[test]
fn doctor_reports_ignore_repair_checks() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_generated_root(project.path(), ".codex/skills/example/SKILL.md");

    let output = run_cli(project.path(), &["--json", "doctor"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "doctor", Some(project.path()));
    let checks = value["checks"].as_array().expect("checks");
    let ignore = checks
        .iter()
        .find(|item| item["id"] == "ignore-repair")
        .expect("ignore check");
    assert_eq!(ignore["status"], "warn");
    assert_eq!(ignore["fix_plan_ref"], "metactl ignore fix --plan");
}

#[test]
fn doctor_does_not_mutate_ignore_or_git_index() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_generated_root(project.path(), ".codex/skills/example/SKILL.md");
    let before_files = project_file_snapshot(project.path());
    let before_index = git_ls_files(project.path());

    let output = run_cli(project.path(), &["--json", "doctor"]);
    assert!(output.status.success(), "{}", stderr(&output));

    assert_eq!(project_file_snapshot(project.path()), before_files);
    assert_eq!(git_ls_files(project.path()), before_index);
}

#[test]
fn cli_ignore_install_local_writes_git_exclude_only() {
    let project = TempDir::new().expect("tempdir");
    fs::create_dir_all(project.path().join(".git/info")).expect("create git info");

    let output = run_cli(
        project.path(),
        &[
            "ignore",
            "install",
            "--scope",
            "local",
            "--target",
            "codex-cli",
            "--target",
            "cursor",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));

    let exclude =
        fs::read_to_string(project.path().join(".git/info/exclude")).expect("read exclude");
    assert!(exclude.contains("# metactl:begin generated-agent-surfaces"));
    assert!(exclude.contains(".codex/"));
    assert!(exclude.contains(".cursor/"));
    assert!(exclude.contains("metactl.local.yaml"));
    assert!(!exclude.contains("/.agents/"));
    assert!(!exclude.contains("/.codex/"));
    assert!(!exclude.contains("/.cursor/"));
    assert!(!exclude.contains("/metactl.local.yaml"));
    assert!(!project.path().join(".cursorignore").exists());
    assert!(!project.path().join(".geminiignore").exists());

    let second = run_cli(
        project.path(),
        &[
            "ignore",
            "install",
            "--scope",
            "local",
            "--target",
            "codex-cli",
        ],
    );
    assert!(second.status.success(), "{}", stderr(&second));
    let updated =
        fs::read_to_string(project.path().join(".git/info/exclude")).expect("read exclude");
    assert_eq!(
        updated
            .matches("# metactl:begin generated-agent-surfaces")
            .count(),
        1,
        "managed ignore block should be replaced idempotently"
    );
    assert!(updated.contains(".codex/"));
    assert!(!updated.contains(".cursor/"));

    let status = run_cli(project.path(), &["ignore", "status", "--target", "cursor"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_stdout = stdout(&status);
    assert!(
        !status_stdout.contains("repo-scoped Git ignores can hide Cursor skills"),
        "local exclude posture should not warn about repo-scoped agent allowlists:\n{}",
        status_stdout
    );
}

#[test]
fn cli_ignore_install_can_include_private_source_paths() {
    let project = TempDir::new().expect("tempdir");
    fs::create_dir_all(project.path().join(".git/info")).expect("create git info");

    let output = run_cli(
        project.path(),
        &[
            "ignore",
            "install",
            "--scope",
            "local",
            "--include-private-sources",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));

    let exclude =
        fs::read_to_string(project.path().join(".git/info/exclude")).expect("read exclude");
    assert!(exclude.contains(".metactl/cache/sources/"));
    assert!(exclude.contains(".metactl/private/source-lock.json"));
}

#[test]
fn cli_ignore_status_reports_private_source_protection() {
    let project = TempDir::new().expect("tempdir");
    fs::create_dir_all(project.path().join(".git/info")).expect("create git info");

    let before = run_cli(project.path(), &["--json", "ignore", "status"]);
    assert!(before.status.success(), "{}", stderr(&before));
    let before_json = json_output(&before);
    assert_eq!(before_json["private_sources"]["protected"], false);
    let before_human = run_cli(project.path(), &["ignore", "status"]);
    assert!(before_human.status.success(), "{}", stderr(&before_human));
    assert!(stdout(&before_human)
        .contains("next: metactl ignore install --scope local --include-private-sources"));

    let install = run_cli(
        project.path(),
        &[
            "ignore",
            "install",
            "--scope",
            "local",
            "--include-private-sources",
        ],
    );
    assert!(install.status.success(), "{}", stderr(&install));

    let after = run_cli(project.path(), &["--json", "ignore", "status"]);
    assert!(after.status.success(), "{}", stderr(&after));
    let after_json = json_output(&after);
    assert_eq!(after_json["private_sources"]["protected"], true);
    assert_eq!(after_json["private_sources"]["cache_protected"], true);
    assert_eq!(
        after_json["private_sources"]["private_lock_protected"],
        true
    );
}

#[test]
fn cli_ignore_install_repo_writes_gitignore_and_agent_allowlists() {
    let project = TempDir::new().expect("tempdir");

    let output = run_cli(
        project.path(),
        &["ignore", "install", "--scope", "repo", "--target", "all"],
    );
    assert!(output.status.success(), "{}", stderr(&output));

    let gitignore = fs::read_to_string(project.path().join(".gitignore")).expect("read gitignore");
    assert!(gitignore.contains(".metactl/"));
    assert!(gitignore.contains(".codex/"));
    assert!(gitignore.contains(".cursor/"));
    assert!(gitignore.contains(".claude/"));
    assert!(gitignore.contains(".gemini/"));
    assert!(gitignore.contains("CLAUDE.local.md"));
    assert!(gitignore.contains("GEMINI.local.md"));
    assert!(!gitignore.contains("/.agents/"));
    assert!(!gitignore.contains("/.metactl/"));
    assert!(!gitignore.contains("/.codex/"));
    assert!(!gitignore.contains("/.cursor/"));
    assert!(!gitignore.contains("/.claude/"));
    assert!(!gitignore.contains("/.gemini/"));
    assert!(!gitignore.contains("/CLAUDE.local.md"));
    assert!(!gitignore.contains("/GEMINI.local.md"));
    assert!(!gitignore.contains("/metactl.lock.json"));

    let cursorignore =
        fs::read_to_string(project.path().join(".cursorignore")).expect("read cursorignore");
    assert!(cursorignore.contains("# metactl:begin agent-surface-allowlist"));
    assert!(cursorignore.contains("!/.cursor/rules/**"));
    assert!(cursorignore.contains("!/.cursor/skills/**"));
    assert!(cursorignore.contains("!/.codex/skills/**"));

    let geminiignore =
        fs::read_to_string(project.path().join(".geminiignore")).expect("read geminiignore");
    assert!(geminiignore.contains("# metactl:begin agent-surface-allowlist"));
    assert!(geminiignore.contains("!/.gemini/extensions/**"));

    let second = run_cli(
        project.path(),
        &["ignore", "install", "--scope", "repo", "--target", "all"],
    );
    assert!(second.status.success(), "{}", stderr(&second));
    let updated = fs::read_to_string(project.path().join(".cursorignore"))
        .expect("read updated cursorignore");
    assert_eq!(
        updated
            .matches("# metactl:begin agent-surface-allowlist")
            .count(),
        1,
        "managed agent allowlist block should be replaced idempotently"
    );
}

#[test]
fn cli_ignore_status_warns_when_repo_gitignore_can_hide_cursor_surfaces() {
    let project = TempDir::new().expect("tempdir");
    fs::write(project.path().join(".gitignore"), "/.cursor/\n").expect("write gitignore");

    let output = run_cli(project.path(), &["ignore", "status", "--target", "cursor"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let output_stdout = stdout(&output);
    assert!(
        output_stdout.contains("repo-scoped Git ignores can hide Cursor skills"),
        "status should warn when repo gitignore can hide Cursor surfaces:\n{}",
        output_stdout
    );
}

#[test]
fn cli_check_strict_reports_warn_ignore_and_superseded_knowledge_sources() {
    let project = TempDir::new().expect("tempdir");
    let custom = TempDir::new().expect("custom library");
    let ks_dir = custom.path().join("knowledge_sources");
    fs::create_dir_all(&ks_dir).expect("knowledge dir");

    let write_source = |file_name: &str, manifest: Value| {
        fs::write(
            ks_dir.join(file_name),
            serde_json::to_string_pretty(&manifest).expect("knowledge json"),
        )
        .expect("write knowledge source");
    };
    let source = |id: &str,
                  freshness_policy: &str,
                  review_status: &str,
                  superseded_by: Vec<&str>| {
        json!({
            "kind": "knowledge_source",
            "id": id,
            "version": "1.0.0",
            "title": id,
            "source_kind": "filesystem_markdown",
            "uri_scheme": "file",
            "allowed_targets": ["codex-cli"],
            "byte_budget": {"max_search_bytes": 4096, "max_read_bytes": 4096, "max_search_results": 5},
            "trust_tier": "org_validated",
            "freshness": {
                "owner": "fixtures",
                "last_verified": "2000-01-01T00:00:00Z",
                "expires_after_days": 1,
                "source_digests": ["sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],
                "freshness_policy": freshness_policy,
                "review_status": review_status,
                "superseded_by": superseded_by
            },
            "operations": {
                "search": {"enabled": true, "max_bytes": 4096, "max_results": 5},
                "read": {"enabled": true, "max_bytes": 4096, "max_results": 1},
                "freshness": {"enabled": true, "max_bytes": 1024, "max_results": 1},
                "propose_update": {"enabled": false, "mode": "request_only"}
            },
            "adapter": {"base_path": "docs", "allowed_uri_prefixes": ["file:docs/"]}
        })
    };
    write_source(
        "expired-warn.json",
        source("expired-warn", "warn", "active", Vec::new()),
    );
    write_source(
        "expired-ignore.json",
        source("expired-ignore", "ignore", "active", Vec::new()),
    );
    let mut superseded_source = source(
        "superseded-source",
        "warn",
        "superseded",
        vec!["knowledge_source:current-source"],
    );
    superseded_source["freshness"]["last_verified"] = json!("2999-01-01T00:00:00Z");
    write_source("superseded-source.json", superseded_source);

    fs::write(
        project.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1
role: builder
policy: brownfield-safe-builder
targets:
- codex-cli
starter_library:
- {}
- {}
",
            starter_library_root(),
            custom.path().display()
        ),
    )
    .expect("write config");

    let sync = run_cli(
        project.path(),
        &["--json", "sync", "--target", "codex", "--apply"],
    );
    assert!(sync.status.success(), "{}", stderr(&sync));

    let check = run_cli(project.path(), &["--json", "check", "--strict"]);
    assert!(check.status.success(), "{}", stderr(&check));
    let check_json = json_output(&check);
    let freshness = check_json["freshness"]
        .as_array()
        .expect("freshness findings");
    let by_id = |id: &str| {
        freshness
            .iter()
            .find(|item| item["id"] == id)
            .unwrap_or_else(|| panic!("missing freshness finding for {id}: {freshness:?}"))
    };

    let warn = by_id("expired-warn");
    assert_eq!(warn["status"], json!("warn"));
    assert_eq!(warn["code"], json!("METACTL_KS_EXPIRED_WARN"));
    assert_eq!(
        warn["source_digests"][0],
        json!("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
    assert_eq!(warn["trust_tier"], json!("org_validated"));

    let ignored = by_id("expired-ignore");
    assert_eq!(ignored["status"], json!("ignored"));
    assert_eq!(ignored["code"], json!("METACTL_KS_EXPIRED_IGNORE"));
    assert_eq!(ignored["freshness_policy"], json!("ignore"));

    let superseded = by_id("superseded-source");
    assert_eq!(superseded["status"], json!("warn"));
    assert_eq!(superseded["code"], json!("METACTL_KS_SUPERSEDED"));
    assert_eq!(
        superseded["superseded_by"][0],
        json!("knowledge_source:current-source")
    );
}

#[test]
fn local_config_layer_gitignored() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let gitignore = fs::read_to_string(project.path().join(".gitignore")).expect("read .gitignore");
    assert!(
        gitignore.contains("metactl.local.yaml"),
        "gitignore should contain metactl.local.yaml entry"
    );
}

#[test]
fn cli_doctor_ignores_brownfield_files_once_managed() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let sync = run_cli(project.path(), &["sync", "--adopt", "patch", "--yes"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let doctor = run_cli(project.path(), &["--json", "doctor"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let json = json_output(&doctor);
    let checks = json["checks"].as_array().expect("checks array");

    assert!(
        checks.iter().all(|c| c["id"] != "brownfield-detection"),
        "doctor should not report brownfield-detection after managed sync: {:?}",
        checks
    );
}
