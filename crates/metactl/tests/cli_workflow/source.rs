use super::*;

// Source and private-source workflow tests.

#[test]
fn cli_help_surfaces_source_add_argument_shape() {
    let main_help = Command::new(cli_bin())
        .arg("--help")
        .output()
        .expect("main help");
    assert!(main_help.status.success(), "{}", stderr(&main_help));
    let main_text = stdout(&main_help);
    assert!(main_text.contains("metactl source add <path>"));

    let source_help = Command::new(cli_bin())
        .args(["help", "source", "add"])
        .output()
        .expect("help source add");
    assert!(source_help.status.success(), "{}", stderr(&source_help));
    let source_text = stdout(&source_help);
    assert!(source_text.contains("Usage: metactl source add"));
    assert!(source_text.contains("[NAME_OR_LOCATION]"));
    assert!(source_text.contains("[LOCATION]"));
    assert!(source_text.contains("Name for this source, or the location"));
    assert!(source_text.contains("Path or Git URL to the source root"));
}

#[test]
fn source_add_infers_name_and_source_sync_without_name_syncs_configured_sources() {
    let project = TempDir::new().expect("tempdir");
    let source = TempDir::new().expect("source");
    seed_private_source_library(source.path(), "team-pack-core-quality");
    init_project(project.path());

    let add = run_cli(
        project.path(),
        &[
            "--json",
            "source",
            "add",
            source.path().to_str().expect("source path"),
            "--private",
            "--lock-publicity",
            "private",
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));
    let add_json = json_output(&add);
    assert_eq!(add_json["name"], "team-library");
    assert_eq!(add_json["source"]["id"], "team-library");

    let sync = run_cli(project.path(), &["--json", "source", "sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    let sync_json = json_output(&sync);
    assert_json_contract(&sync_json, "source", Some(project.path()));
    assert_eq!(sync_json["action"], "sync");
    let sources = sync_json["sources"].as_array().expect("sources");
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0]["id"], "team-library");
    assert!(project
        .path()
        .join(".metactl/private/source-lock.json")
        .exists());
}

#[test]
fn audit_sources_fails_on_tracked_private_source_state() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_private_source_state(project.path());

    let audit = run_cli(project.path(), &["--json", "audit", "sources"]);
    assert_eq!(audit.status.code(), Some(13), "{}", stderr(&audit));
    let json = json_output(&audit);
    assert_eq!(json["ok"], false);
    assert!(json["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .any(|item| item["path"]
            .as_str()
            .unwrap_or("")
            .contains(".metactl/cache/sources")));
}

#[test]
fn audit_sources_fails_on_public_personal_workspace_example() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    let git_init = Command::new("git")
        .args([
            "-C",
            project.path().to_str().expect("project"),
            "init",
            "--quiet",
        ])
        .output()
        .expect("git init");
    assert!(git_init.status.success(), "{}", stderr(&git_init));
    fs::create_dir_all(project.path().join("docs/user")).expect("docs user");
    fs::write(
        project.path().join("docs/user/private-source-example.md"),
        "Use /Users/example/src/private/example-library as an example.\n",
    )
    .expect("write public doc");
    let git_add = Command::new("git")
        .args([
            "-C",
            project.path().to_str().expect("project"),
            "add",
            "docs/user/private-source-example.md",
        ])
        .output()
        .expect("git add");
    assert!(git_add.status.success(), "{}", stderr(&git_add));

    let audit = run_cli(project.path(), &["--json", "audit", "sources"]);
    assert_eq!(audit.status.code(), Some(13), "{}", stderr(&audit));
    let json = json_output(&audit);
    assert!(json["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .any(|item| item["id"] == "public-example-personal-workspace"));

    let human_audit = run_cli(project.path(), &["audit", "sources"]);
    assert_eq!(
        human_audit.status.code(),
        Some(13),
        "{}",
        stderr(&human_audit)
    );
    let human_stderr = stderr(&human_audit);
    assert!(human_stderr.contains("docs/user/private-source-example.md"));
    assert!(human_stderr.contains("Use neutral placeholders"));
}

#[test]
fn doctor_reports_source_audit_failure_for_tracked_private_source_state() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_private_source_state(project.path());

    let doctor = run_cli(project.path(), &["--json", "doctor"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let json = json_output(&doctor);
    let source_audit = json["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .find(|check| check["id"] == "source-audit")
        .expect("source-audit check");
    assert_eq!(source_audit["status"], "fail");
    assert!(source_audit["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .any(|item| item["id"] == "tracked-private-source-state"));

    let human_doctor = run_cli(project.path(), &["doctor"]);
    assert!(human_doctor.status.success(), "{}", stderr(&human_doctor));
    let human_doctor_stdout = stdout(&human_doctor);
    assert!(human_doctor_stdout.contains("next: metactl audit sources"));
    assert!(human_doctor_stdout.contains(".metactl/cache/sources"));

    let human_status = run_cli(project.path(), &["status"]);
    assert!(human_status.status.success(), "{}", stderr(&human_status));
    let human_status_stdout = stdout(&human_status);
    assert!(human_status_stdout.contains("Source state: private_source_leak_risk"));
    assert!(human_status_stdout.contains("next: metactl audit sources"));
    assert!(human_status_stdout.contains(".metactl/cache/sources"));
}

#[test]
fn validate_fails_source_audit_when_private_source_state_is_tracked() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    seed_tracked_private_source_state(project.path());

    let validate = run_cli(project.path(), &["--json", "validate"]);
    assert_eq!(validate.status.code(), Some(13), "{}", stderr(&validate));
    let json = json_output(&validate);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "validate");
    assert_eq!(json["source_audit"]["status"], "fail");

    let human_validate = run_cli(project.path(), &["validate"]);
    assert_eq!(
        human_validate.status.code(),
        Some(13),
        "{}",
        stderr(&human_validate)
    );
    let human_stderr = stderr(&human_validate);
    assert!(human_stderr.contains(".metactl/cache/sources"));
    assert!(human_stderr.contains("Remove the file from the index"));
    assert!(human_stderr.contains("metactl ignore install --scope local --include-private-sources"));
}

#[test]
fn sync_preflights_source_audit_before_apply() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_private_source_state(project.path());

    let sync = run_cli(project.path(), &["sync"]);
    assert_eq!(sync.status.code(), Some(13), "{}", stderr(&sync));
    let human_stderr = stderr(&sync);
    assert!(human_stderr.contains("Sync refused"));
    assert!(human_stderr.contains(".metactl/cache/sources"));
    assert!(human_stderr.contains("metactl ignore install --scope local --include-private-sources"));
    assert!(!project.path().join(".codex").exists());

    let json_sync = run_cli(project.path(), &["--json", "sync"]);
    assert_eq!(json_sync.status.code(), Some(13), "{}", stderr(&json_sync));
    let json = json_output(&json_sync);
    assert_eq!(json["command"], "sync");
    assert_eq!(json["source_audit"]["status"], "fail");
    assert!(json["source_audit"]["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .any(|item| item["id"] == "tracked-private-source-state"));
}

#[test]
fn cli_profile_dedupes_same_library_root_from_starter_library_and_sources() {
    let project = TempDir::new().expect("tempdir");
    let home = TempDir::new().expect("home");
    let profiles = home.path().join(".config/metactl/profiles");
    fs::create_dir_all(&profiles).expect("profiles");
    let starter = starter_library_root();
    fs::write(
        profiles.join("team-profile.yaml"),
        format!(
            "starter_library:\n  - {starter}\nsources:\n  - id: team-library\n    type: local\n    path: {starter}\n    visibility: private\n    lock_publicity: private\n"
        ),
    )
    .expect("profile");

    let init = run_cli_env(
        project.path(),
        &[
            "--json",
            "--profile",
            "team-profile",
            "init",
            "--target",
            "codex-cli",
        ],
        &[("HOME", home.path().to_str().expect("home"))],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    let status = run_cli_env(
        project.path(),
        &["--json", "status"],
        &[("HOME", home.path().to_str().expect("home"))],
    );
    assert!(
        status.status.success(),
        "duplicate same-path root should be ignored: {}",
        stdout(&status)
    );
}

#[test]
fn cli_check_strict_fails_expired_fail_policy_knowledge_source() {
    let project = TempDir::new().expect("tempdir");
    let custom = TempDir::new().expect("custom library");
    let ks_dir = custom.path().join("knowledge_sources");
    fs::create_dir_all(&ks_dir).expect("knowledge dir");
    fs::write(
        ks_dir.join("expired.json"),
        serde_json::to_string_pretty(&json!({
            "kind": "knowledge_source",
            "id": "expired-standards",
            "version": "1.0.0",
            "title": "Expired Standards",
            "source_kind": "filesystem_markdown",
            "uri_scheme": "file",
            "allowed_targets": ["codex-cli"],
            "byte_budget": {"max_search_bytes": 4096, "max_read_bytes": 4096, "max_search_results": 5},
            "trust_tier": "org_validated",
            "freshness": {
                "owner": "fixtures",
                "last_verified": "2000-01-01T00:00:00Z",
                "expires_after_days": 1,
                "source_digests": ["sha256:0123456789abcdef0123456789abcdef"],
                "freshness_policy": "fail",
                "review_status": "active"
            },
            "operations": {
                "search": {"enabled": true, "max_bytes": 4096, "max_results": 5},
                "read": {"enabled": true, "max_bytes": 4096, "max_results": 1},
                "freshness": {"enabled": true, "max_bytes": 1024, "max_results": 1},
                "propose_update": {"enabled": false, "mode": "request_only"}
            },
            "adapter": {"base_path": "docs", "allowed_uri_prefixes": ["file:docs/"]}
        }))
        .expect("knowledge json"),
    )
    .expect("write knowledge source");

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
    assert!(!check.status.success(), "strict check should fail");
    let check_json = json_output(&check);
    assert_eq!(check_json["ok"], json!(false));
    assert_eq!(check_json["freshness"][0]["id"], json!("expired-standards"));
    assert_eq!(check_json["freshness"][0]["status"], json!("fail"));
    assert_eq!(
        check_json["freshness"][0]["code"],
        json!("METACTL_KS_EXPIRED_FAIL")
    );
    assert_eq!(
        check_json["freshness"][0]["source_digests"][0],
        json!("sha256:0123456789abcdef0123456789abcdef")
    );
}

#[test]
fn pack_sources_list_includes_starter_library() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let list = run_cli(project.path(), &["--json", "source", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let json = json_output(&list);
    assert_json_contract(&json, "source", Some(project.path()));

    let sources = json["sources"].as_array().expect("sources array");
    assert!(
        sources
            .iter()
            .any(|s| s["origin"] == "starter_library" || s["origin"] == "starter-library"),
        "should include starter library as a source: {:?}",
        sources
    );
}

#[test]
fn pack_sources_add_stores_typed_source_config() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let add = run_cli(
        project.path(),
        &["--json", "source", "add", "my-packs", "/tmp/my-packs"],
    );
    assert!(add.status.success(), "{}", stderr(&add));
    let json = json_output(&add);
    assert_json_contract(&json, "source", Some(project.path()));

    // New config writes typed sources; metadata.source.* remains read-compatible only.
    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("read config");
    assert!(config.contains("sources:"), "config should contain sources");
    assert!(
        config.contains("id: my-packs"),
        "config should contain source id"
    );
    assert!(
        !config.contains("source.my-packs"),
        "new source config should not write metadata.source.*"
    );
}

#[test]
fn source_add_local_writes_typed_source_and_sync_validates_library() {
    let project = TempDir::new().expect("tempdir");
    let source = TempDir::new().expect("source");
    seed_private_source_library(source.path(), "team-pack-core-quality");
    init_project(project.path());

    let add = run_cli(
        project.path(),
        &[
            "--json",
            "source",
            "add",
            "team-library",
            source.path().to_str().expect("source path"),
            "--private",
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));
    let add_json = json_output(&add);
    assert_json_contract(&add_json, "source", Some(project.path()));
    assert_eq!(add_json["source"]["id"], "team-library");
    assert_eq!(add_json["source"]["type"], "local");
    assert_eq!(add_json["source"]["visibility"], "private");

    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("read config");
    assert!(config.contains("sources:"), "{config}");
    assert!(config.contains("id: team-library"), "{config}");
    assert!(!config.contains("source.team-library"), "{config}");

    let sync = run_cli(
        project.path(),
        &["--json", "source", "sync", "team-library"],
    );
    assert!(sync.status.success(), "{}", stderr(&sync));
    let sync_json = json_output(&sync);
    assert_eq!(sync_json["source"]["id"], "team-library");
    assert_eq!(sync_json["source"]["status"], "synced");

    let list = run_cli(project.path(), &["--json", "list", "packs"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let list_json = json_output(&list);
    assert!(list_json["items"]
        .as_array()
        .expect("items")
        .iter()
        .any(|item| item["id"] == "team-pack-core-quality"));

    let use_pack = run_cli(
        project.path(),
        &[
            "--json",
            "use",
            "team-library/team-pack-core-quality",
            "--no-sync",
        ],
    );
    assert!(use_pack.status.success(), "{}", stderr(&use_pack));
    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("read config");
    assert!(
        config.contains("team-library/team-pack-core-quality"),
        "{config}"
    );
}

#[test]
fn source_sync_missing_source_lists_recovery_commands() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let no_sources = run_cli(project.path(), &["source", "sync", "missing-source"]);
    assert_eq!(
        no_sources.status.code(),
        Some(10),
        "{}",
        stderr(&no_sources)
    );
    let no_sources_stderr = stderr(&no_sources);
    assert!(no_sources_stderr.contains("Source 'missing-source' is not configured."));
    assert!(no_sources_stderr.contains("Next: metactl source list"));
    assert!(
        no_sources_stderr.contains("Next: metactl source add <name> <path-or-git-url> --private")
    );

    let source = TempDir::new().expect("source");
    seed_private_source_library(source.path(), "team-pack-core-quality");
    let add = run_cli(
        project.path(),
        &[
            "source",
            "add",
            "team-library",
            source.path().to_str().expect("source path"),
            "--private",
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));

    let wrong_source = run_cli(project.path(), &["source", "sync", "wrong-source"]);
    assert_eq!(
        wrong_source.status.code(),
        Some(10),
        "{}",
        stderr(&wrong_source)
    );
    let wrong_source_stderr = stderr(&wrong_source);
    assert!(wrong_source_stderr.contains("Next: metactl source list"));
    assert!(wrong_source_stderr.contains("Configured sources: team-library"));
}

#[test]
fn source_add_git_sync_writes_redacted_public_and_private_locks() {
    let project = TempDir::new().expect("tempdir");
    let source = TempDir::new().expect("source");
    seed_private_source_library(source.path(), "team-pack-core-quality");
    Command::new("git")
        .args([
            "-C",
            source.path().to_str().expect("source path"),
            "init",
            "--quiet",
        ])
        .output()
        .expect("git init");
    Command::new("git")
        .args([
            "-C",
            source.path().to_str().expect("source path"),
            "add",
            ".",
        ])
        .output()
        .expect("git add");
    Command::new("git")
        .args([
            "-C",
            source.path().to_str().expect("source path"),
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "--quiet",
            "-m",
            "seed",
        ])
        .output()
        .expect("git commit");
    let rev = Command::new("git")
        .args([
            "-C",
            source.path().to_str().expect("source path"),
            "rev-parse",
            "HEAD",
        ])
        .output()
        .expect("rev-parse");
    assert!(rev.status.success(), "{}", stderr(&rev));
    let commit = stdout(&rev).trim().to_string();

    init_project(project.path());
    let add = run_cli(
        project.path(),
        &[
            "--json",
            "source",
            "add",
            "team-library",
            source.path().to_str().expect("source path"),
            "--type",
            "git",
            "--ref",
            &commit,
            "--private",
            "--lock-publicity",
            "private",
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));

    let sync = run_cli(
        project.path(),
        &["--json", "source", "sync", "team-library"],
    );
    assert!(sync.status.success(), "{}", stderr(&sync));
    let sync_json = json_output(&sync);
    assert_eq!(sync_json["source"]["resolved_commit"], commit);
    assert!(project
        .path()
        .join(".metactl/cache/sources/team-library/library.json")
        .exists());

    let public_lock: Value = serde_json::from_slice(
        &fs::read(project.path().join("metactl.lock.json")).expect("public lock"),
    )
    .expect("public lock json");
    assert_eq!(public_lock["sources"][0]["id"], "team-library");
    assert_eq!(public_lock["sources"][0]["resolved"], "redacted");
    let public_lock_text =
        fs::read_to_string(project.path().join("metactl.lock.json")).expect("lock text");
    assert!(!public_lock_text.contains(source.path().to_str().expect("source path")));
    assert!(!public_lock_text.contains(&commit));

    let private_lock: Value = serde_json::from_slice(
        &fs::read(project.path().join(".metactl/private/source-lock.json")).expect("private lock"),
    )
    .expect("private lock json");
    assert_eq!(private_lock["sources"][0]["id"], "team-library");
    assert_eq!(private_lock["sources"][0]["resolved_commit"], commit);
    assert_eq!(
        private_lock["sources"][0]["url"],
        source.path().to_str().expect("source path")
    );
}

#[test]
fn status_reports_private_source_missing_and_active_states() {
    let project = TempDir::new().expect("tempdir");
    let source = TempDir::new().expect("source");
    seed_private_source_library(source.path(), "team-pack-core-quality");
    init_project(project.path());

    let add = run_cli(
        project.path(),
        &[
            "--json",
            "source",
            "add",
            "team-library",
            source.path().to_str().expect("source path"),
            "--type",
            "git",
            "--ref",
            "HEAD",
            "--private",
            "--allow-floating-ref",
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));
    let missing = run_cli(project.path(), &["--json", "status"]);
    assert!(missing.status.success(), "{}", stderr(&missing));
    let missing_json = json_output(&missing);
    assert_eq!(
        missing_json["source_state"]["state"],
        "private_source_missing"
    );

    let active_project = TempDir::new().expect("active project");
    init_project(active_project.path());
    let add = run_cli(
        active_project.path(),
        &[
            "--json",
            "source",
            "add",
            "team-local",
            source.path().to_str().expect("source path"),
            "--private",
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));
    let active = run_cli(active_project.path(), &["--json", "status"]);
    assert!(active.status.success(), "{}", stderr(&active));
    let active_json = json_output(&active);
    assert_eq!(
        active_json["source_state"]["state"],
        "private_source_active"
    );
}

#[test]
fn sync_require_private_sources_fails_when_private_source_missing() {
    let project = TempDir::new().expect("tempdir");
    let source = TempDir::new().expect("source");
    seed_private_source_library(source.path(), "team-pack-core-quality");
    init_project(project.path());

    let add = run_cli(
        project.path(),
        &[
            "--json",
            "source",
            "add",
            "team-library",
            source.path().to_str().expect("source path"),
            "--type",
            "git",
            "--ref",
            "HEAD",
            "--private",
            "--allow-floating-ref",
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));

    let sync = run_cli(
        project.path(),
        &["--json", "sync", "--require-private-sources"],
    );
    assert_eq!(sync.status.code(), Some(10), "{}", stderr(&sync));
    let json = json_output(&sync);
    assert_eq!(json["ok"], false);
    assert_eq!(json["source_state"]["state"], "private_source_missing");
}

#[test]
fn sync_refuses_stale_git_source_cache_until_source_sync() {
    let project = TempDir::new().expect("tempdir");
    let source = TempDir::new().expect("source");
    seed_private_source_library(source.path(), "team-pack-core-quality");
    let git_init = Command::new("git")
        .args([
            "-C",
            source.path().to_str().expect("source"),
            "init",
            "--quiet",
        ])
        .output()
        .expect("git init");
    assert!(git_init.status.success(), "{}", stderr(&git_init));
    let first_commit = git_commit_all(source.path(), "seed");

    init_project(project.path());
    let add = run_cli(
        project.path(),
        &[
            "--json",
            "source",
            "add",
            "team-library",
            source.path().to_str().expect("source path"),
            "--type",
            "git",
            "--ref",
            "main",
            "--private",
            "--lock-publicity",
            "private",
            "--allow-floating-ref",
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));
    Command::new("git")
        .args([
            "-C",
            source.path().to_str().expect("source"),
            "branch",
            "-M",
            "main",
        ])
        .output()
        .expect("git branch main");
    let source_sync = run_cli(
        project.path(),
        &["--json", "source", "sync", "team-library"],
    );
    assert!(source_sync.status.success(), "{}", stderr(&source_sync));
    assert_eq!(
        json_output(&source_sync)["source"]["resolved_commit"],
        first_commit
    );

    fs::write(
        source
            .path()
            .join("vendor/team-pack-core-quality/CHANGELOG.md"),
        "new content\n",
    )
    .expect("write change");
    let second_commit = git_commit_all(source.path(), "update");
    assert_ne!(first_commit, second_commit);

    let stale_sync = run_cli(project.path(), &["--json", "sync"]);
    assert_eq!(
        stale_sync.status.code(),
        Some(10),
        "{}",
        stderr(&stale_sync)
    );
    let stale_json = json_output(&stale_sync);
    assert_eq!(stale_json["source_state"]["state"], "private_source_stale");

    let stale_human_sync = run_cli(project.path(), &["sync"]);
    assert_eq!(
        stale_human_sync.status.code(),
        Some(10),
        "{}",
        stderr(&stale_human_sync)
    );
    assert!(stderr(&stale_human_sync).contains("Next: metactl source sync team-library"));

    let refresh = run_cli(
        project.path(),
        &["--json", "source", "sync", "team-library"],
    );
    assert!(refresh.status.success(), "{}", stderr(&refresh));
    assert_eq!(
        json_output(&refresh)["source"]["resolved_commit"],
        second_commit
    );

    let final_sync = run_cli(project.path(), &["--json", "sync"]);
    assert!(final_sync.status.success(), "{}", stderr(&final_sync));
}

#[test]
fn explain_reports_private_source_context_for_namespaced_pack() {
    let project = TempDir::new().expect("tempdir");
    let source = TempDir::new().expect("source");
    seed_private_source_library(source.path(), "team-pack-core-quality");
    init_project(project.path());

    let add = run_cli(
        project.path(),
        &[
            "--json",
            "source",
            "add",
            "team-library",
            source.path().to_str().expect("source path"),
            "--private",
            "--lock-publicity",
            "private",
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));
    let use_pack = run_cli(
        project.path(),
        &[
            "--json",
            "use",
            "team-library/team-pack-core-quality",
            "--no-sync",
        ],
    );
    assert!(use_pack.status.success(), "{}", stderr(&use_pack));

    let explain = run_cli(project.path(), &["--json", "explain"]);
    assert!(explain.status.success(), "{}", stderr(&explain));
    let json = json_output(&explain);
    let source_context = json["pack_sources"]["team-pack-core-quality"].clone();
    assert_eq!(source_context["id"], "team-library");
    assert_eq!(source_context["visibility"], "private");
    assert_eq!(source_context["lock_publicity"], "private");
    assert_eq!(source_context["redacted"], true);
    assert!(source_context.get("path").is_none());
}
