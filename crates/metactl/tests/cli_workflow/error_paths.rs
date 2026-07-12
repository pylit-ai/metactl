use super::*;

fn assert_agent_error(value: &Value, error_code: &str) {
    assert_eq!(value["ok"], false);
    assert_eq!(value["error_code"], error_code);
    assert!(
        value["next_commands"]
            .as_array()
            .is_some_and(|commands| !commands.is_empty()),
        "agent error must include recovery commands: {value:#?}"
    );
}

#[test]
fn agent_unknown_subcommand_is_json_usage_error() {
    let project = TempDir::new().expect("tempdir");
    let output = run_cli(project.path(), &["--agent", "unknown-subcommand"]);

    assert_eq!(output.status.code(), Some(10), "{}", stderr(&output));
    assert_agent_error(&json_output(&output), "usage");
}

#[test]
fn agent_invalid_flag_is_json_usage_error() {
    let project = TempDir::new().expect("tempdir");
    let output = run_cli(project.path(), &["--agent", "sync", "--bogus-flag"]);

    assert_eq!(output.status.code(), Some(10), "{}", stderr(&output));
    assert_agent_error(&json_output(&output), "usage");
}

#[test]
fn agent_missing_explicit_project_is_project_not_found_error() {
    let cwd = TempDir::new().expect("cwd");
    let home = TempDir::new().expect("home");
    let missing = cwd.path().join("does-not-exist");
    let output = run_cli_cwd(
        cwd.path(),
        home.path(),
        &[
            "--project",
            missing.to_str().expect("missing path"),
            "--agent",
            "status",
        ],
    );

    assert!(!output.status.success(), "{}", stdout(&output));
    assert_agent_error(&json_output(&output), "project_not_found");
}

#[test]
fn agent_init_allows_a_new_explicit_project_directory() {
    let cwd = TempDir::new().expect("cwd");
    let home = TempDir::new().expect("home");
    let new_project = cwd.path().join("new-project");
    let output = run_cli_cwd(
        cwd.path(),
        home.path(),
        &[
            "--project",
            new_project.to_str().expect("new project path"),
            "--agent",
            "init",
            "--target",
            "codex-cli",
        ],
    );

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(new_project.join("metactl.yaml").exists());
}

#[test]
fn agent_drifted_validate_includes_recovery_command() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let agents = project.path().join("AGENTS.md");
    fs::write(
        &agents,
        format!(
            "{}\nlocal drift\n",
            fs::read_to_string(&agents).expect("agents")
        ),
    )
    .expect("drift agents");
    let output = run_cli(project.path(), &["--agent", "validate"]);

    assert_eq!(output.status.code(), Some(13), "{}", stderr(&output));
    assert_agent_error(&json_output(&output), "validation");
}
