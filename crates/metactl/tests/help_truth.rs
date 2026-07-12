use std::process::Command;

use tempfile::TempDir;

fn cli_bin() -> &'static str {
    env!("CARGO_BIN_EXE_metactl")
}

#[test]
fn help_all_lists_hidden_and_porcelain_commands() {
    let output = Command::new(cli_bin())
        .args(["help", "--all"])
        .output()
        .expect("run help --all");
    assert!(output.status.success(), "help --all failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("source"),
        "hidden source command missing: {stdout}"
    );
    assert!(
        stdout.contains("sync"),
        "porcelain sync command missing: {stdout}"
    );
}

#[test]
fn default_help_stays_short_and_points_to_help_all() {
    let output = Command::new(cli_bin())
        .arg("--help")
        .output()
        .expect("run --help");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Run 'metactl help --all' to list advanced commands."));
    assert!(!stdout.contains("\n  source "));
}

#[test]
fn verify_has_the_same_exit_code_as_validate() {
    let project = TempDir::new().expect("create project");
    let init = Command::new(cli_bin())
        .current_dir(project.path())
        .args(["init", "--target", "codex-cli", "--no-input", "--yes"])
        .output()
        .expect("init project");
    assert!(init.status.success(), "init failed: {init:?}");

    let validate = Command::new(cli_bin())
        .current_dir(project.path())
        .arg("validate")
        .output()
        .expect("run validate");
    let verify = Command::new(cli_bin())
        .current_dir(project.path())
        .arg("verify")
        .output()
        .expect("run verify");
    assert_eq!(verify.status.code(), validate.status.code());
}
