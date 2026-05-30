mod support;

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use support::{init_project, json_output, run_cli, stderr};

fn starter_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../library/starter")
}

#[test]
fn target_aliases_are_declared_in_starter_data() {
    let aliases = [
        ("claude-code", "claude"),
        ("codex-cli", "codex"),
        ("gemini-cli", "gemini"),
    ];

    for (target_id, alias) in aliases {
        let path = starter_root()
            .join("targets")
            .join(format!("{target_id}.json"));
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(path).expect("target bytes")).expect("target json");
        assert!(
            json["aliases"]
                .as_array()
                .expect("aliases")
                .iter()
                .any(|item| item == alias),
            "{target_id} should declare alias {alias}"
        );
    }
}

#[test]
fn target_list_reports_alias_metadata() {
    let project = TempDir::new().expect("tempdir");
    let output = run_cli(project.path(), &["--json", "target", "list"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let json = json_output(&output);
    let items = json["items"].as_array().expect("items");
    let claude = items
        .iter()
        .find(|item| item["id"] == "claude-code")
        .expect("claude-code target");
    assert!(
        claude["aliases"]
            .as_array()
            .expect("aliases")
            .iter()
            .any(|item| item == "claude"),
        "target list should expose aliases: {claude}"
    );
}

#[test]
fn target_add_accepts_metadata_aliases() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let output = run_cli(project.path(), &["target", "add", "claude"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("config");
    assert!(
        config.contains("- claude-code"),
        "target add should store canonical target id: {config}"
    );
}

#[test]
fn compile_accepts_metadata_aliases() {
    let project = TempDir::new().expect("tempdir");
    let init = run_cli(project.path(), &["init", "--target", "claude-code"]);
    assert!(init.status.success(), "{}", stderr(&init));

    let output = run_cli(project.path(), &["compile", "--target", "claude"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(project
        .path()
        .join(".metactl/generated/claude-code/CLAUDE.md")
        .exists());
    assert!(
        stderr(&output).contains("resolved target alias 'claude' to 'claude-code'"),
        "compile should report metadata alias resolution: {}",
        stderr(&output)
    );
}
