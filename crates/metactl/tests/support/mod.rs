use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

pub fn cli_bin() -> &'static str {
    env!("CARGO_BIN_EXE_metactl")
}

pub fn run_cli(project: &Path, args: &[&str]) -> Output {
    let test_home = project.join(".test-home");
    fs::create_dir_all(&test_home).expect("create test home");
    Command::new(cli_bin())
        .env_remove("METACTL_PROFILE")
        .env_remove("XDG_CONFIG_HOME")
        .env("HOME", &test_home)
        .arg("--project")
        .arg(project)
        .args(args)
        .output()
        .expect("run metactl")
}

#[allow(dead_code)]
pub fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("json stdout")
}

#[allow(dead_code)]
pub fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[allow(dead_code)]
pub fn init_project(project: &Path) {
    let out = run_cli(
        project,
        &[
            "init",
            "--target",
            "codex-cli",
            "--role",
            "builder",
            "--policy",
            "brownfield-safe-builder",
            "--no-input",
            "-y",
        ],
    );
    assert!(out.status.success(), "init failed: {}", stderr(&out));
}

#[allow(dead_code)]
pub fn init_and_sync(project: &Path, target: &str) {
    let out = run_cli(project, &["init", "--target", target, "--no-input", "-y"]);
    assert!(
        out.status.success(),
        "init --target {} failed: {}",
        target,
        stderr(&out)
    );
    let out = run_cli(project, &["sync", "--no-input", "-y"]);
    assert!(
        out.status.success(),
        "sync after init --target {} failed: {}",
        target,
        stderr(&out)
    );
}

/// Recursively walk `root`, collecting regular file paths into `out`.
/// Skips `.git/`, `tmp/`, and `target/` subtrees defensively.
#[allow(dead_code)]
pub fn walk_files(root: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(root) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if file_type.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(name, ".git" | "tmp" | "target") {
                continue;
            }
            walk_files(&path, out);
        } else if file_type.is_file() {
            out.push(path);
        } else if file_type.is_symlink() {
            // metactl's sync mode emits compile outputs as symlinks pointing
            // into `.metactl/generated/<target>/`. Follow the symlink and treat
            // it as a regular file when the target exists and resolves to one.
            if path.metadata().map(|m| m.is_file()).unwrap_or(false) {
                out.push(path);
            }
        }
    }
}
