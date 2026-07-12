mod support;

use std::fs;

use serde_json::Value;
use tempfile::TempDir;

use support::{json_output, run_cli, stderr};

#[test]
fn gemini_tier1_generates_extension_manifest_context_commands_and_skills() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(
        project.path(),
        &["init", "--target", "gemini-cli", "--no-input", "-y"],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    let add = run_cli(
        project.path(),
        &["add", "unit-test-loop", "--sync", "--no-input", "-y"],
    );
    assert!(add.status.success(), "{}", stderr(&add));

    let extension_root = project.path().join(".gemini/extensions/unit-test-loop");
    let manifest_path = extension_root.join("gemini-extension.json");
    assert!(
        manifest_path.exists(),
        "Gemini extension manifest should be applied at {}",
        manifest_path.display()
    );
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read gemini manifest"))
            .expect("parse gemini manifest");
    assert_eq!(manifest["name"], "unit-test-loop");
    assert_eq!(manifest["contextFileName"], "GEMINI.md");
    assert!(manifest["description"]
        .as_str()
        .unwrap_or("")
        .contains("test"));

    let context_path = extension_root.join("GEMINI.md");
    assert!(
        context_path.exists(),
        "Gemini extension context file should exist at {}",
        context_path.display()
    );
    let context = fs::read_to_string(&context_path).expect("read extension GEMINI.md");
    assert!(
        context.contains("Unit Test Loop") || context.contains("targeted test"),
        "Gemini extension context should carry pack instructions: {context}"
    );

    let command_path = extension_root.join("commands/run-targeted-tests.md");
    assert!(
        command_path.exists(),
        "Gemini command resource should exist at {}",
        command_path.display()
    );
    let command = fs::read_to_string(&command_path).expect("read gemini command");
    assert!(
        command.starts_with("---\n") && command.contains("description:"),
        "Gemini command resource must preserve markdown frontmatter: {command}"
    );

    let skill_path = extension_root.join("skills/unit-test-loop/SKILL.md");
    assert!(
        skill_path.exists(),
        "Gemini primary Agent Skill should be discoverable at {}",
        skill_path.display()
    );
    let body = fs::read_to_string(&skill_path).expect("read gemini skill");
    assert!(
        body.starts_with("---\n") && body.contains("description:"),
        "Gemini skill must preserve Agent Skill frontmatter: {body}"
    );

    let generated_skill_root = project
        .path()
        .join(".metactl/generated/gemini-cli/.gemini/extensions/unit-test-loop/skills");
    assert!(
        generated_skill_root.exists(),
        "Gemini generated skill bundle root should exist at {}",
        generated_skill_root.display()
    );

    let validate = run_cli(
        project.path(),
        &["--json", "validate", "--target", "gemini-cli"],
    );
    assert!(validate.status.success(), "{}", stderr(&validate));
    let value = json_output(&validate);
    assert_eq!(value["ok"], true);
    assert!(
        value.to_string().contains("gemini-cli") && value.to_string().contains("pass"),
        "validate JSON should report gemini-cli pass status: {value}"
    );
}
