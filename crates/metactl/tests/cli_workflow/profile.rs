use super::*;

// Profile binding workflow tests.

#[test]
fn committed_projection_profile_fixture_and_stale_lockfile_contract_are_explicit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let profile: Value = serde_json::from_slice(
        &fs::read(root.join("fixtures/library_stack/private-committed-projection/profile.json"))
            .expect("committed_projection profile fixture"),
    )
    .expect("profile json");
    assert_eq!(profile["committed_projection"]["enabled"], json!(true));
    assert_eq!(
        profile["committed_projection"]["public_boundary_gate"],
        json!("required")
    );
    assert!(profile["committed_projection"]["allowed_repo_classes"]
        .as_array()
        .expect("allowed repo classes")
        .iter()
        .any(|item| item == "private"));

    let committed_lockfile: Value = serde_json::from_slice(
        &fs::read(root.join("fixtures/library_stack/private-committed-projection/lock.json"))
            .expect("committed_projection lock fixture"),
    )
    .expect("committed lock json");
    let resolved_artifact = &committed_lockfile["resolved_artifacts"][0];
    assert_eq!(
        resolved_artifact["x-provenance"]["source_id"],
        json!("user-overlay")
    );
    assert_eq!(
        resolved_artifact["x-provenance"]["artifact_digest"],
        resolved_artifact["artifact_digest"]
    );
    assert_eq!(resolved_artifact["x-freshness"]["status"], json!("fresh"));
    assert_eq!(
        resolved_artifact["x-freshness"]["code"],
        json!("METACTL_KS_FRESH")
    );

    let stale_lockfile: Value = serde_json::from_slice(
        &fs::read(root.join("fixtures/library_stack/stale-lockfile/lock.json"))
            .expect("stale lockfile fixture"),
    )
    .expect("stale lockfile json");
    assert_eq!(stale_lockfile["x-stale-lockfile"]["status"], json!("stale"));
    assert_eq!(
        stale_lockfile["x-stale-lockfile"]["reason"],
        json!("source_digest_changed")
    );
    assert_eq!(
        stale_lockfile["x-stale-lockfile"]["code"],
        json!("METACTL_STACK_SOURCE_DIGEST_CHANGED")
    );
}

#[test]
fn cli_help_subcommand_shows_bind_profile_guidance() {
    let output = Command::new(cli_bin())
        .args(["help", "init"])
        .output()
        .expect("help init");
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("--bind-profile"));
    assert!(text.contains("repo should intentionally track that profile"));
}

#[test]
fn profile_list_exposes_public_builtin_templates() {
    let project = TempDir::new().expect("tempdir");
    let list = run_cli(project.path(), &["--json", "profile", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let json = json_output(&list);
    assert_json_contract(&json, "profile", Some(project.path()));
    let templates = json["templates"].as_array().expect("templates");
    for expected in [
        "neutral",
        "multi-agent",
        "agent-ci",
        "solo-codex",
        "private-overlay",
    ] {
        assert!(
            templates.iter().any(|item| item["name"] == expected),
            "missing template {expected}: {templates:?}"
        );
    }
    assert!(templates.iter().all(|item| {
        item["starter_library"]
            .as_array()
            .map(|paths| paths.is_empty())
            .unwrap_or(true)
    }));
}

#[test]
fn cli_project_defaults_do_not_clear_profile_surface_mode() {
    let project = TempDir::new().expect("tempdir");
    let home = TempDir::new().expect("home");
    let profiles = home.path().join(".config/metactl/profiles");
    fs::create_dir_all(&profiles).expect("profiles");
    let starter = starter_library_root();
    fs::write(
        profiles.join("team-profile.yaml"),
        format!(
            "starter_library:\n  - {starter}\ntargets:\n  - codex-cli\ndefaults:\n  surface_selection_mode: full\n"
        ),
    )
    .expect("profile");
    fs::write(
        project.path().join("metactl.yaml"),
        "extends_profile: team-profile\napi_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ndefaults:\n  brownfield_mode: refuse_due_to_conflict\n  discovery_mode: candidate_search\n",
    )
    .expect("config");
    fs::create_dir_all(project.path().join(".metactl/private")).expect("state dirs");

    let compile = run_cli_env(
        project.path(),
        &["--json", "compile"],
        &[("HOME", home.path().to_str().expect("home"))],
    );
    assert!(
        compile.status.success(),
        "stdout: {}\nstderr: {}",
        stdout(&compile),
        stderr(&compile)
    );
    let json = json_output(&compile);
    assert_eq!(json["targets"][0]["surface_selection_mode"], "full");
}

#[test]
fn cli_profile_binding_persists_and_detects_staleness() {
    let project = TempDir::new().expect("tempdir");
    let home = TempDir::new().expect("home");
    let profiles_dir = home.path().join(".config/metactl/profiles");
    fs::create_dir_all(&profiles_dir).expect("profiles dir");
    let profile_path = profiles_dir.join("team-profile.yaml");
    fs::write(
        &profile_path,
        format!(
            "targets:\n  - openclaw\nstarter_library:\n  - {}\npacks:\n  - python-refactor\n",
            starter_library_root()
        ),
    )
    .expect("write profile");

    let init = run_cli_env(
        project.path(),
        &["--json", "--profile", "team-profile", "init"],
        &[("HOME", home.path().to_str().expect("home path"))],
    );
    assert!(init.status.success(), "{}", stderr(&init));
    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("config");
    assert!(config.contains("extends_profile: team-profile"));
    assert!(!config.contains("targets:"));

    let sync = run_cli_env(
        project.path(),
        &["--json", "sync"],
        &[("HOME", home.path().to_str().expect("home path"))],
    );
    assert!(sync.status.success(), "{}", stderr(&sync));
    let sync_json = json_output(&sync);
    assert_eq!(sync_json["profile"]["status"], "synced");
    assert!(project.path().join("OPENCLAW.md").exists());

    fs::write(
        &profile_path,
        format!(
            "targets:\n  - codex-cli\nstarter_library:\n  - {}\npacks:\n  - python-refactor\n",
            starter_library_root()
        ),
    )
    .expect("rewrite profile");

    let doctor = run_cli_env(
        project.path(),
        &["--json", "doctor"],
        &[("HOME", home.path().to_str().expect("home path"))],
    );
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let doctor_json = json_output(&doctor);
    assert!(doctor_json["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .any(|item| item["id"] == "profile-binding" && item["status"] == "fail"));
}

#[test]
fn cli_init_uses_machine_default_profile_without_extends_profile() {
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
        &["--json", "init"],
        &[("HOME", home.path().to_str().expect("home path"))],
    );
    assert!(init.status.success(), "{}", stderr(&init));
    let init_json = json_output(&init);
    assert_eq!(
        init_json["profile_resolution"]["activation_source"],
        json!("user_default")
    );
    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("read config");
    assert!(
        !config.contains("extends_profile"),
        "machine default should not auto-bind: {config}"
    );
}

#[test]
fn cli_init_bind_profile_writes_extends_profile() {
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
        &["--json", "init", "--bind-profile"],
        &[("HOME", home.path().to_str().expect("home path"))],
    );
    assert!(init.status.success(), "{}", stderr(&init));
    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("read config");
    assert!(config.contains("extends_profile: team-profile"), "{config}");
}

#[test]
fn cli_profile_resolution_prefers_extends_over_default() {
    let project = TempDir::new().expect("tempdir");
    let home = TempDir::new().expect("home");
    let profiles = home.path().join(".config/metactl/profiles");
    fs::create_dir_all(&profiles).expect("profiles");
    let starter = starter_library_root();
    fs::write(
        profiles.join("wx-a.yaml"),
        format!("targets:\n  - codex-cli\nstarter_library:\n  - {starter}\n"),
    )
    .expect("wx-a");
    fs::write(
        profiles.join("team-profile.yaml"),
        format!("targets:\n  - openclaw\nstarter_library:\n  - {starter}\n"),
    )
    .expect("team-profile");
    let init = run_cli_env(
        project.path(),
        &["--json", "init", "--profile", "wx-a"],
        &[("HOME", home.path().to_str().expect("home"))],
    );
    assert!(init.status.success(), "{}", stderr(&init));
    fs::write(
        home.path().join(".config/metactl/config.yaml"),
        "default_profile: team-profile\n",
    )
    .expect("user settings");
    let status = run_cli_env(
        project.path(),
        &["--json", "status"],
        &[("HOME", home.path().to_str().expect("home"))],
    );
    assert!(status.status.success(), "{}", stderr(&status));
    let status_json = json_output(&status);
    let profile = &status_json["profile"];
    assert_eq!(profile["name"], "wx-a");
    assert_eq!(profile["activation_source"], json!("project_extends"));
}

#[test]
fn cli_doctor_reports_target_discoverability_failure_for_profile_bound_library() {
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

    let doctor = run_cli_env(
        project.path(),
        &["--json", "doctor"],
        &[("HOME", home.path().to_str().expect("home"))],
    );
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let json = json_output(&doctor);
    assert!(json["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .any(|item| {
            item["id"] == "target-discovery"
                && item["status"] == "fail"
                && item["message"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("made-up-target")
        }));
}

#[test]
fn cli_profile_porcelain_set_and_clear_default() {
    let home = TempDir::new().expect("home");
    let profiles = home.path().join(".config/metactl/profiles");
    fs::create_dir_all(&profiles).expect("profiles");
    let starter = starter_library_root();
    fs::write(
        profiles.join("p1.yaml"),
        format!("targets:\n  - codex-cli\nstarter_library:\n  - {starter}\n"),
    )
    .expect("p1");

    let set = Command::new(cli_bin())
        .args(["--json", "profile", "set-default", "p1"])
        .env("HOME", home.path())
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .expect("set-default");
    assert!(set.status.success(), "{}", stderr(&set));

    let show = Command::new(cli_bin())
        .args(["--json", "profile", "show"])
        .env("HOME", home.path())
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .expect("show");
    let show_json = json_output(&show);
    assert_eq!(show_json["default_profile"], "p1");

    let clr = Command::new(cli_bin())
        .args(["--json", "profile", "clear-default"])
        .env("HOME", home.path())
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .expect("clear");
    assert!(clr.status.success(), "{}", stderr(&clr));

    let show2 = Command::new(cli_bin())
        .args(["--json", "profile", "show"])
        .env("HOME", home.path())
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .expect("show2");
    let show2_json = json_output(&show2);
    assert!(
        show2_json
            .get("default_profile")
            .map_or(true, Value::is_null),
        "{show2_json}"
    );
}
