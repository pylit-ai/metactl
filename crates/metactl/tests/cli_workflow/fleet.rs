use super::*;

// Fleet controller and fleet sync workflow tests.

#[test]
fn fleet_list_reports_linked_project_statuses() {
    let project = TempDir::new().expect("tempdir");
    let ready = TempDir::new().expect("ready");
    init_project(ready.path());
    fs::write(
        project.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nlinked_projects:\n- id: ready\n  path: {}\n- id: disabled\n  path: {}\n  disabled: true\n- id: missing\n  path: {}\n",
            ready.path().display(),
            ready.path().display(),
            project.path().join("missing").display()
        ),
    )
    .expect("write metactl.yaml");

    let output = run_cli(project.path(), &["--json", "fleet", "list"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let json = json_output(&output);
    assert_json_contract(&json, "fleet", Some(project.path()));
    assert_eq!(json["action"], "list");
    assert_eq!(json["projects"][0]["id"], "ready");
    assert_eq!(json["projects"][0]["status"], "ready");
    assert_eq!(json["projects"][1]["status"], "disabled");
    assert_eq!(json["projects"][2]["status"], "missing_path");
}

#[test]
fn fleet_list_reports_invalid_linked_project_config() {
    let project = TempDir::new().expect("tempdir");
    let linked = TempDir::new().expect("linked");
    fs::write(
        linked.path().join("metactl.yaml"),
        "linked_projects: not-a-list\n",
    )
    .expect("write malformed linked config");
    fs::write(
        project.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nlinked_projects:\n- id: linked\n  path: {}\n",
            linked.path().display()
        ),
    )
    .expect("write metactl.yaml");

    let output = run_cli(project.path(), &["--json", "fleet", "list"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let json = json_output(&output);
    assert_json_contract(&json, "fleet", Some(project.path()));
    assert_eq!(json["projects"][0]["id"], "linked");
    assert_eq!(json["projects"][0]["status"], "invalid_config");
    assert_eq!(json["projects"][0]["result"], "invalid_config");
    assert!(json["projects"][0]["details"]
        .as_array()
        .expect("details")
        .iter()
        .any(|detail| detail
            .as_str()
            .unwrap_or_default()
            .contains("linked_projects")));
}

#[test]
fn fleet_sync_preview_does_not_mutate_linked_project() {
    let project = TempDir::new().expect("tempdir");
    let ready = TempDir::new().expect("ready");
    init_project(ready.path());
    fs::write(
        project.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nlinked_projects:\n- id: ready\n  path: {}\n",
            ready.path().display()
        ),
    )
    .expect("write metactl.yaml");

    let output = run_cli(project.path(), &["--json", "fleet", "sync", "--preview"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let json = json_output(&output);
    assert_json_contract(&json, "fleet", Some(project.path()));
    assert_eq!(json["action"], "sync");
    assert_eq!(json["preview"], true);
    assert_eq!(json["projects"][0]["id"], "ready");
    assert_eq!(json["projects"][0]["status"], "planned");
    assert_eq!(
        json["scope_note"],
        "Fleet sync updates repo-local .codex/skills in linked projects; it does not install user-global Personal skills under ~/.codex/skills."
    );
    assert_eq!(
        json["projects"][0]["skill_visibility"]["target"],
        "codex-cli"
    );
    assert!(!ready.path().join("AGENTS.md").exists());
    assert!(!ready
        .path()
        .join(".metactl/generated/codex-cli/AGENTS.md")
        .exists());

    let human = run_cli(project.path(), &["fleet", "sync", "--preview"]);
    assert!(human.status.success(), "{}", stderr(&human));
    assert!(stdout(&human).contains("Fleet sync updates repo-local .codex/skills"));
}

#[test]
fn fleet_controller_default_allows_preview_outside_controller_project() {
    let home = TempDir::new().expect("home");
    let cwd = TempDir::new().expect("cwd");
    let controller = TempDir::new().expect("controller");
    let ready = TempDir::new().expect("ready");
    init_project(ready.path());
    fs::write(
        controller.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nlinked_projects:\n- id: ready\n  path: {}\n",
            ready.path().display()
        ),
    )
    .expect("write controller config");

    let set = run_cli_cwd(
        cwd.path(),
        home.path(),
        &[
            "--json",
            "fleet",
            "controller",
            "set",
            "personal",
            controller.path().to_str().expect("controller path"),
        ],
    );
    assert!(set.status.success(), "{}", stderr(&set));
    let set_json = json_output(&set);
    assert_eq!(set_json["action"], "controller-set");

    let preview = run_cli_cwd(
        cwd.path(),
        home.path(),
        &["--json", "fleet", "sync", "--preview"],
    );
    assert!(preview.status.success(), "{}", stderr(&preview));
    let json = json_output(&preview);
    assert_json_contract(&json, "fleet", Some(controller.path()));
    assert_eq!(json["controller"]["id"], "personal");
    assert_eq!(json["controller"]["source"], "user_default");
    assert_eq!(json["projects"][0]["id"], "ready");
    assert_eq!(json["projects"][0]["status"], "planned");
    assert!(!ready.path().join("AGENTS.md").exists());
}

#[test]
fn fleet_controller_init_creates_default_xdg_controller_and_selects_it() {
    let home = TempDir::new().expect("home");
    let cwd = TempDir::new().expect("cwd");

    let init = run_cli_cwd(
        cwd.path(),
        home.path(),
        &["--json", "fleet", "controller", "init", "personal"],
    );
    assert!(init.status.success(), "{}", stderr(&init));
    let json = json_output(&init);
    let controller = home.path().join(".config/metactl/fleet/personal");
    assert_eq!(json["action"], "controller-init");
    assert_eq!(
        json["controller"]["path"],
        controller.to_string_lossy().to_string()
    );
    assert!(controller.join("metactl.yaml").exists());
    assert!(controller.join("README.md").exists());

    let list = run_cli_cwd(cwd.path(), home.path(), &["--json", "fleet", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let list_json = json_output(&list);
    assert_json_contract(&list_json, "fleet", Some(&controller));
    assert_eq!(list_json["controller"]["id"], "personal");
    assert_eq!(list_json["controller"]["source"], "user_default");
    assert!(list_json["projects"]
        .as_array()
        .expect("projects")
        .is_empty());
}

#[test]
fn fleet_controller_init_refuses_to_replace_existing_config_without_force() {
    let home = TempDir::new().expect("home");
    let cwd = TempDir::new().expect("cwd");
    let first = run_cli_cwd(
        cwd.path(),
        home.path(),
        &["--json", "fleet", "controller", "init", "personal"],
    );
    assert!(first.status.success(), "{}", stderr(&first));

    let second = run_cli_cwd(
        cwd.path(),
        home.path(),
        &["--json", "fleet", "controller", "init", "personal"],
    );
    assert_eq!(second.status.code(), Some(10), "{}", stdout(&second));
    let json = json_output(&second);
    assert_eq!(json["ok"], false);
    assert!(json["message"]
        .as_str()
        .unwrap_or_default()
        .contains("already exists"));
}

#[test]
fn fleet_controller_init_rejects_path_like_names() {
    let home = TempDir::new().expect("home");
    let cwd = TempDir::new().expect("cwd");

    let output = run_cli_cwd(
        cwd.path(),
        home.path(),
        &["--json", "fleet", "controller", "init", "../escape"],
    );
    assert_eq!(output.status.code(), Some(10), "{}", stdout(&output));
    let json = json_output(&output);
    assert_eq!(json["ok"], false);
    assert!(json["message"]
        .as_str()
        .unwrap_or_default()
        .contains("Invalid Fleet controller name"));
    assert!(!home
        .path()
        .join(".config/metactl/escape/metactl.yaml")
        .exists());
}

#[test]
fn fleet_controller_set_accepts_empty_initialized_controller() {
    let home = TempDir::new().expect("home");
    let cwd = TempDir::new().expect("cwd");
    let controller = TempDir::new().expect("controller");
    init_project(controller.path());

    let set = run_cli_cwd(
        cwd.path(),
        home.path(),
        &[
            "--json",
            "fleet",
            "controller",
            "set",
            "personal",
            controller.path().to_str().expect("controller path"),
        ],
    );
    assert!(set.status.success(), "{}", stderr(&set));

    let list = run_cli_cwd(cwd.path(), home.path(), &["--json", "fleet", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let json = json_output(&list);
    assert_json_contract(&json, "fleet", Some(controller.path()));
    assert!(json["projects"].as_array().expect("projects").is_empty());
}

#[test]
fn fleet_current_project_without_linked_projects_falls_back_to_default_controller() {
    let home = TempDir::new().expect("home");
    let cwd = TempDir::new().expect("cwd");
    let controller = TempDir::new().expect("controller");
    let ready = TempDir::new().expect("ready");
    init_project(cwd.path());
    init_project(ready.path());
    fs::write(
        controller.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nlinked_projects:\n- id: ready\n  path: {}\n",
            ready.path().display()
        ),
    )
    .expect("write controller config");
    let set = run_cli_cwd(
        cwd.path(),
        home.path(),
        &[
            "--json",
            "fleet",
            "controller",
            "set",
            "personal",
            controller.path().to_str().expect("controller path"),
        ],
    );
    assert!(set.status.success(), "{}", stderr(&set));

    let list = run_cli_cwd(cwd.path(), home.path(), &["--json", "fleet", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let json = json_output(&list);
    assert_json_contract(&json, "fleet", Some(controller.path()));
    assert_eq!(json["controller"]["source"], "user_default");
    assert_eq!(json["projects"][0]["id"], "ready");
}

#[test]
fn fleet_explicit_project_overrides_default_controller() {
    let home = TempDir::new().expect("home");
    let cwd = TempDir::new().expect("cwd");
    let default_controller = TempDir::new().expect("default-controller");
    let explicit_controller = TempDir::new().expect("explicit-controller");
    let default_ready = TempDir::new().expect("default-ready");
    let explicit_ready = TempDir::new().expect("explicit-ready");
    init_project(default_ready.path());
    init_project(explicit_ready.path());
    fs::write(
        default_controller.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nlinked_projects:\n- id: default-ready\n  path: {}\n",
            default_ready.path().display()
        ),
    )
    .expect("write default controller config");
    fs::write(
        explicit_controller.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nlinked_projects:\n- id: explicit-ready\n  path: {}\n",
            explicit_ready.path().display()
        ),
    )
    .expect("write explicit controller config");
    let set = run_cli_cwd(
        cwd.path(),
        home.path(),
        &[
            "--json",
            "fleet",
            "controller",
            "set",
            "personal",
            default_controller
                .path()
                .to_str()
                .expect("default controller path"),
        ],
    );
    assert!(set.status.success(), "{}", stderr(&set));

    let list = run_cli_cwd(
        cwd.path(),
        home.path(),
        &[
            "--json",
            "--project",
            explicit_controller
                .path()
                .to_str()
                .expect("explicit controller path"),
            "fleet",
            "list",
        ],
    );
    assert!(list.status.success(), "{}", stderr(&list));
    let json = json_output(&list);
    assert_json_contract(&json, "fleet", Some(explicit_controller.path()));
    assert_eq!(json["controller"]["source"], "command_line");
    assert_eq!(json["projects"][0]["id"], "explicit-ready");
}

#[test]
fn fleet_status_reports_ready_project_readiness() {
    let project = TempDir::new().expect("tempdir");
    let ready = TempDir::new().expect("ready");
    init_project(ready.path());
    fs::write(
        project.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nlinked_projects:\n- id: ready\n  path: {}\n",
            ready.path().display()
        ),
    )
    .expect("write metactl.yaml");

    let output = run_cli(project.path(), &["--json", "fleet", "status"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let json = json_output(&output);
    assert_json_contract(&json, "fleet", Some(project.path()));
    assert_eq!(json["action"], "status");
    assert_eq!(json["projects"][0]["id"], "ready");
    assert_eq!(json["projects"][0]["status"], "ready");
    assert_eq!(json["projects"][0]["needs_sync"], true);
}

#[test]
fn fleet_status_reports_invalid_linked_project_config() {
    let project = TempDir::new().expect("tempdir");
    let linked = TempDir::new().expect("linked");
    fs::write(linked.path().join("metactl.yaml"), "api_version: [bad\n")
        .expect("write malformed linked config");
    fs::write(
        project.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nlinked_projects:\n- id: linked\n  path: {}\n",
            linked.path().display()
        ),
    )
    .expect("write metactl.yaml");

    let output = run_cli(project.path(), &["--json", "fleet", "status"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let json = json_output(&output);
    assert_json_contract(&json, "fleet", Some(project.path()));
    assert_eq!(json["projects"][0]["id"], "linked");
    assert_eq!(json["projects"][0]["status"], "invalid_config");
    assert_eq!(json["projects"][0]["result"], "invalid_config");
    assert!(!json["projects"][0]["message"]
        .as_str()
        .unwrap_or_default()
        .is_empty());
}

#[test]
fn fleet_sync_preview_reports_invalid_linked_project_config_per_project() {
    let project = TempDir::new().expect("tempdir");
    let linked = TempDir::new().expect("linked");
    fs::write(linked.path().join("metactl.yaml"), "api_version: [bad\n")
        .expect("write malformed linked config");
    fs::write(
        project.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nlinked_projects:\n- id: linked\n  path: {}\n",
            linked.path().display()
        ),
    )
    .expect("write metactl.yaml");

    let output = run_cli(project.path(), &["--json", "fleet", "sync", "--preview"]);
    assert_eq!(output.status.code(), Some(10), "{}", stdout(&output));
    let json = json_output(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "fleet");
    assert_eq!(json["preview"], true);
    assert_eq!(json["projects"][0]["id"], "linked");
    assert_eq!(json["projects"][0]["status"], "failed");
    assert_eq!(json["projects"][0]["result"], "invalid_config");
}

#[test]
fn fleet_parent_config_decode_errors_include_details() {
    let project = TempDir::new().expect("tempdir");
    fs::write(
        project.path().join("metactl.yaml"),
        "version: 1\nlinked_projects: not-a-list\n",
    )
    .expect("write malformed parent config");

    let output = run_cli(project.path(), &["--json", "fleet", "list"]);
    assert_eq!(output.status.code(), Some(10), "{}", stdout(&output));
    let json = json_output(&output);
    assert_eq!(json["ok"], false);
    assert!(json["details"]
        .as_array()
        .expect("details")
        .iter()
        .any(|detail| detail
            .as_str()
            .unwrap_or_default()
            .contains("linked_projects")));
}

#[test]
fn fleet_status_filters_by_project_id_and_reports_missing_id() {
    let project = TempDir::new().expect("tempdir");
    let first = TempDir::new().expect("first");
    let second = TempDir::new().expect("second");
    init_project(first.path());
    init_project(second.path());
    fs::write(
        project.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nlinked_projects:\n- id: first\n  path: {}\n- id: second\n  path: {}\n",
            first.path().display(),
            second.path().display()
        ),
    )
    .expect("write metactl.yaml");

    let filtered = run_cli(
        project.path(),
        &["--json", "fleet", "status", "--id", "second"],
    );
    assert!(filtered.status.success(), "{}", stderr(&filtered));
    let json = json_output(&filtered);
    assert_eq!(json["projects"].as_array().expect("projects").len(), 1);
    assert_eq!(json["projects"][0]["id"], "second");

    let missing = run_cli(
        project.path(),
        &["--json", "fleet", "status", "--id", "missing"],
    );
    assert_eq!(missing.status.code(), Some(10), "{}", stdout(&missing));
    let missing_json = json_output(&missing);
    assert_eq!(missing_json["ok"], false);
    assert!(missing_json["message"]
        .as_str()
        .unwrap_or_default()
        .contains("linked project id(s) not found"));
}

#[test]
fn fleet_sync_apply_requires_explicit_automation_confirmation() {
    let project = TempDir::new().expect("tempdir");
    let ready = TempDir::new().expect("ready");
    init_project(ready.path());
    fs::write(
        project.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nlinked_projects:\n- id: ready\n  path: {}\n",
            ready.path().display()
        ),
    )
    .expect("write metactl.yaml");

    let refused = run_cli(project.path(), &["--json", "fleet", "sync", "--apply"]);
    assert_eq!(refused.status.code(), Some(10), "{}", stdout(&refused));
    let refused_json = json_output(&refused);
    assert_eq!(refused_json["ok"], false);

    let applied = run_cli(
        project.path(),
        &["--json", "--yes", "--no-input", "fleet", "sync", "--apply"],
    );
    assert!(applied.status.success(), "{}", stderr(&applied));
    let json = json_output(&applied);
    assert_eq!(json["preview"], false);
    assert_eq!(json["projects"][0]["status"], "applied");
    assert!(ready.path().join("AGENTS.md").exists());
    let log = fs::read_to_string(project.path().join(".metactl/logs/fleet-sync.jsonl"))
        .expect("fleet sync log");
    assert!(log.contains("\"metactl_version\""));
    assert!(log.contains("\"id\":\"ready\""));
    assert!(
        !log.contains(&ready.path().to_string_lossy().to_string()),
        "fleet log should not expose project-local paths: {log}"
    );
}

#[test]
fn fleet_sync_apply_defaults_linked_project_to_patch_adoption() {
    let project = TempDir::new().expect("tempdir");
    let linked = TempDir::new().expect("linked");
    init_project(linked.path());
    fs::write(linked.path().join("AGENTS.md"), "user-owned\n").expect("seed brownfield file");
    fs::write(
        project.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nlinked_projects:\n- id: linked\n  path: {}\n",
            linked.path().display()
        ),
    )
    .expect("write metactl.yaml");

    let output = run_cli(
        project.path(),
        &["--json", "--yes", "--no-input", "fleet", "sync", "--apply"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let json = json_output(&output);
    assert_eq!(json["projects"][0]["status"], "applied");
    assert_eq!(json["projects"][0]["fleet_sync_adopt"], "patch");
    let agents = fs::read_to_string(linked.path().join("AGENTS.md")).expect("read AGENTS");
    assert!(
        agents.contains("user-owned"),
        "patch adoption should preserve brownfield content: {agents}"
    );
}

#[test]
fn fleet_sync_apply_honors_linked_project_refuse_setting() {
    let project = TempDir::new().expect("tempdir");
    let linked = TempDir::new().expect("linked");
    init_project(linked.path());
    fs::write(
        linked.path().join("metactl.yaml"),
        "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\ndefaults:\n  fleet_sync_adopt: refuse\n",
    )
    .expect("write linked config");
    fs::write(linked.path().join("AGENTS.md"), "user-owned\n").expect("seed brownfield file");
    fs::write(
        project.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nlinked_projects:\n- id: linked\n  path: {}\n",
            linked.path().display()
        ),
    )
    .expect("write metactl.yaml");

    let output = run_cli(
        project.path(),
        &["--json", "--yes", "--no-input", "fleet", "sync", "--apply"],
    );
    assert_eq!(output.status.code(), Some(10), "{}", stdout(&output));
    let json = json_output(&output);
    assert_eq!(json["projects"][0]["status"], "failed");
    assert_eq!(json["projects"][0]["fleet_sync_adopt"], "refuse");
    assert_eq!(
        fs::read_to_string(linked.path().join("AGENTS.md")).expect("read AGENTS"),
        "user-owned\n"
    );
}

#[test]
fn fleet_sync_apply_refuses_dirty_git_project_by_default() {
    let project = TempDir::new().expect("tempdir");
    let ready = TempDir::new().expect("ready");
    init_project(ready.path());
    let git_init = Command::new("git")
        .args([
            "-C",
            ready.path().to_str().expect("ready path"),
            "init",
            "--quiet",
        ])
        .output()
        .expect("git init");
    assert!(git_init.status.success(), "{}", stderr(&git_init));
    fs::write(ready.path().join("local-edit.txt"), "dirty\n").expect("dirty file");
    fs::write(
        project.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nlinked_projects:\n- id: ready\n  path: {}\n",
            ready.path().display()
        ),
    )
    .expect("write metactl.yaml");

    let output = run_cli(
        project.path(),
        &["--json", "--yes", "--no-input", "fleet", "sync", "--apply"],
    );
    assert_eq!(output.status.code(), Some(10), "{}", stdout(&output));
    let json = json_output(&output);
    assert_eq!(json["projects"][0]["status"], "failed");
    assert_eq!(json["projects"][0]["result"], "dirty_worktree");
    assert!(json["projects"][0]["message"]
        .as_str()
        .unwrap_or_default()
        .contains("--allow-dirty"));
    assert!(!ready.path().join("AGENTS.md").exists());
}

#[test]
fn fleet_sync_apply_human_error_reports_failed_projects() {
    let project = TempDir::new().expect("tempdir");
    let ready = TempDir::new().expect("ready");
    init_project(ready.path());
    let git_init = Command::new("git")
        .args([
            "-C",
            ready.path().to_str().expect("ready path"),
            "init",
            "--quiet",
        ])
        .output()
        .expect("git init");
    assert!(git_init.status.success(), "{}", stderr(&git_init));
    fs::write(ready.path().join("local-edit.txt"), "dirty\n").expect("dirty file");
    fs::write(
        project.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nlinked_projects:\n- id: ready\n  path: {}\n",
            ready.path().display()
        ),
    )
    .expect("write metactl.yaml");

    let output = run_cli(
        project.path(),
        &["--yes", "--no-input", "fleet", "sync", "--apply"],
    );
    let err = stderr(&output);
    assert_eq!(
        output.status.code(),
        Some(10),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&output),
        err
    );
    assert!(err.contains("one or more fleet projects failed"), "{err}");
    assert!(err.contains("ready"), "{err}");
    assert!(
        err.contains(ready.path().to_str().expect("ready path")),
        "{err}"
    );
    assert!(err.contains("dirty_worktree"), "{err}");
    assert!(err.contains("--allow-dirty"), "{err}");
}

#[test]
fn fleet_sync_apply_returns_nonzero_for_mixed_project_failure() {
    let project = TempDir::new().expect("tempdir");
    let clean = TempDir::new().expect("clean");
    let dirty = TempDir::new().expect("dirty");
    init_project(clean.path());
    init_project(dirty.path());
    let git_init = Command::new("git")
        .args([
            "-C",
            dirty.path().to_str().expect("dirty path"),
            "init",
            "--quiet",
        ])
        .output()
        .expect("git init");
    assert!(git_init.status.success(), "{}", stderr(&git_init));
    fs::write(dirty.path().join("local-edit.txt"), "dirty\n").expect("dirty file");
    fs::write(
        project.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nlinked_projects:\n- id: clean\n  path: {}\n- id: dirty\n  path: {}\n",
            clean.path().display(),
            dirty.path().display()
        ),
    )
    .expect("write metactl.yaml");

    let output = run_cli(
        project.path(),
        &["--json", "--yes", "--no-input", "fleet", "sync", "--apply"],
    );
    assert_eq!(output.status.code(), Some(10), "{}", stdout(&output));
    let json = json_output(&output);
    assert_eq!(json["projects"][0]["id"], "clean");
    assert_eq!(json["projects"][0]["status"], "applied");
    assert_eq!(json["projects"][1]["id"], "dirty");
    assert_eq!(json["projects"][1]["result"], "dirty_worktree");
    assert!(clean.path().join("AGENTS.md").exists());
    assert!(!dirty.path().join("AGENTS.md").exists());
}
