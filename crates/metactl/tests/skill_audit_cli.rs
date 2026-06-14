use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

fn cli_bin() -> &'static str {
    env!("CARGO_BIN_EXE_metactl")
}

fn run_cli(project: &Path, args: &[&str]) -> Output {
    let test_home = project.join(".test-home");
    fs::create_dir_all(&test_home).expect("create test home");
    Command::new(cli_bin())
        .env_remove("METACTL_PROFILE")
        .env_remove("XDG_CONFIG_HOME")
        .env("HOME", &test_home)
        .current_dir(project)
        .arg("--project")
        .arg(project)
        .args(args)
        .output()
        .expect("run metactl")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("utf8 stdout")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("utf8 stderr")
}

fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("json stdout")
}

fn write_skill(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, content).expect("write skill");
}

#[test]
fn skills_audit_writes_reports_and_reads_usage_stats() {
    let project = TempDir::new().expect("tempdir");
    fs::write(
        project.path().join("AGENTS.md"),
        "# Project Instructions\n\nRepo instructions.\n",
    )
    .expect("write agents");
    write_skill(
        &project.path().join(".agents/skills/repo-local/SKILL.md"),
        r#"---
name: repo-local-skill
description: Repo-local skill fixture.
---

Repo local guidance.
"#,
    );
    write_skill(
        &project.path().join(".codex/skills/demo-pack/review/SKILL.md"),
        r#"---
name: generated-review-skill
description: Generated skill fixture.
source_pack_id: demo-pack
source_library_ref: surface:review
visibility: private
enabled: true
---

Generated review guidance.
"#,
    );
    fs::create_dir_all(project.path().join(".metactl/usage")).expect("usage dir");
    fs::write(
        project.path().join(".metactl/usage/stats.json"),
        r#"{
  "api_version": "0.1.0",
  "generated_at": "2026-06-14T00:00:00Z",
  "source_path": ".metactl/usage/events.jsonl",
  "event_count": 1,
  "packs": [
    {
      "pack_id": "demo-pack",
      "command_invoked": 4,
      "skill_body_read": 2,
      "pack_resolved": 3,
      "search_result_selected": 1,
      "task_verified": 2,
      "correction_or_retry": 0,
      "dismissed_or_abandoned": 0,
      "blocked_or_rejected": 0,
      "event_count": 12,
      "score": 7,
      "last_event_at": "2026-06-14T00:00:00Z"
    }
  ]
}
"#,
    )
    .expect("write stats");

    let output = run_cli(project.path(), &["--json", "skills", "audit"]);
    assert!(output.status.success(), "stdout:\n{}\nstderr:\n{}", stdout(&output), stderr(&output));
    let value = json_output(&output);
    assert_eq!(value["ok"], true);
    assert_eq!(value["command"], "skills");
    assert_eq!(value["action"], "audit");
    assert_eq!(
        value["report"]["target_id"],
        Value::String("codex-cli".to_string())
    );
    assert_eq!(value["report"]["scan_scope"], "repo");
    assert!(
        value["report"]["inventory"].as_array().expect("inventory").len() >= 2
    );
    assert!(
        !value["report"]["relations"].as_array().expect("relations").is_empty()
    );
    let instruction_sources = value["report"]["project_instruction_sources"]
        .as_array()
        .expect("instruction sources");
    assert!(
        instruction_sources.iter().any(|item| item["kind"] == "AGENTS.md"),
        "instruction sources: {}",
        serde_json::to_string_pretty(&value["report"]["project_instruction_sources"])
            .expect("pretty instruction sources")
    );

    let report_json = project.path().join(".metactl/reports/skills/latest.json");
    let report_md = project.path().join(".metactl/reports/skills/latest.md");
    let inventory_path = project.path().join(".metactl/skills/inventory.json");
    let relations_path = project.path().join(".metactl/skills/relations.json");
    assert!(report_json.exists(), "missing {}", report_json.display());
    assert!(report_md.exists(), "missing {}", report_md.display());
    assert!(inventory_path.exists(), "missing {}", inventory_path.display());
    assert!(relations_path.exists(), "missing {}", relations_path.display());

    let markdown = run_cli(project.path(), &["skills", "audit", "--format", "markdown"]);
    assert!(
        markdown.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&markdown),
        stderr(&markdown)
    );
    let text = stdout(&markdown);
    assert!(text.contains("# Skill Portfolio Audit"));
    assert!(text.contains("Inventory:"));
    assert!(text.contains("Project:"));
}
