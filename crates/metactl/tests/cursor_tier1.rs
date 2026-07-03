mod support;

use std::fs;

use tempfile::TempDir;

use support::{json_output, run_cli, stderr};

#[test]
fn cursor_tier1_generates_project_rule_index_and_skill_bundle() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(
        project.path(),
        &["init", "--target", "cursor", "--no-input", "-y"],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    let add = run_cli(
        project.path(),
        &["add", "unit-test-loop", "--sync", "--no-input", "-y"],
    );
    assert!(add.status.success(), "{}", stderr(&add));

    let rule_path = project.path().join(".cursor/rules/metactl-pack-index.mdc");
    assert!(
        rule_path.exists(),
        "Cursor project rule index should be applied at {}",
        rule_path.display()
    );
    let rule = fs::read_to_string(&rule_path).expect("read cursor rule");
    assert!(
        rule.starts_with("---\n"),
        "Cursor .mdc rules must start with YAML frontmatter: {rule}"
    );
    assert!(
        rule.contains("description:") && rule.contains("alwaysApply: true"),
        "Cursor .mdc frontmatter must include description and alwaysApply: {rule}"
    );

    let skill_path = project
        .path()
        .join(".cursor/skills/unit-test-loop/unit-test-loop/SKILL.md");
    assert!(
        skill_path.exists(),
        "Cursor primary skill surface should be discoverable at {}",
        skill_path.display()
    );
    let body = fs::read_to_string(&skill_path).expect("read cursor skill");
    assert!(
        body.starts_with("---\n") && body.contains("description:"),
        "Cursor skill must preserve Agent Skill frontmatter: {body}"
    );

    let generated_skill_root = project
        .path()
        .join(".metactl/generated/cursor/.cursor/skills/unit-test-loop");
    assert!(
        generated_skill_root.exists(),
        "Cursor generated skill bundle root should exist at {}",
        generated_skill_root.display()
    );

    let validate = run_cli(
        project.path(),
        &["--json", "validate", "--target", "cursor"],
    );
    assert!(validate.status.success(), "{}", stderr(&validate));
    let value = json_output(&validate);
    assert_eq!(value["ok"], true);
    assert!(
        value.to_string().contains("cursor") && value.to_string().contains("pass"),
        "validate JSON should report cursor pass status: {value}"
    );
}
