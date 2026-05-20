use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};
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
        .arg("--project")
        .arg(project)
        .args(args)
        .output()
        .expect("run metactl")
}

fn run_cli_env(project: &Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
    let test_home = project.join(".test-home");
    fs::create_dir_all(&test_home).expect("create test home");
    let mut command = Command::new(cli_bin());
    command
        .env_remove("METACTL_PROFILE")
        .env_remove("XDG_CONFIG_HOME")
        .env("HOME", &test_home)
        .arg("--project")
        .arg(project)
        .args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("run metactl with env")
}

fn run_cli_cwd(cwd: &Path, home: &Path, args: &[&str]) -> Output {
    fs::create_dir_all(home).expect("create test home");
    Command::new(cli_bin())
        .env_remove("METACTL_PROFILE")
        .env_remove("XDG_CONFIG_HOME")
        .env("HOME", home)
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run metactl in cwd")
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

fn project_file_snapshot(project: &Path) -> Vec<String> {
    fn walk(root: &Path, path: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(path).expect("read snapshot dir") {
            let entry = entry.expect("snapshot entry");
            let entry_path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if matches!(name.as_ref(), ".git" | ".test-home" | "target") {
                continue;
            }
            let rel = entry_path
                .strip_prefix(root)
                .expect("relative snapshot path")
                .to_string_lossy()
                .replace('\\', "/");
            out.push(rel);
            if entry_path.is_dir() {
                walk(root, &entry_path, out);
            }
        }
    }

    let mut out = Vec::new();
    walk(project, project, &mut out);
    out.sort();
    out
}

fn assert_json_contract(value: &Value, command: &str, project: Option<&Path>) {
    assert_eq!(value["ok"], true);
    assert_eq!(value["command"], command);
    assert_eq!(value["api_version"], metactl::API_VERSION);
    match project {
        Some(project) => assert_eq!(
            value["project_root"],
            Value::String(project.to_string_lossy().to_string())
        ),
        None => assert!(value.get("project_root").is_none()),
    }
}

fn starter_library_root() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../library/starter")
        .to_string_lossy()
        .to_string()
}

fn seed_custom_library_with_third_party_pack(root: &Path) {
    let packs_dir = root.join("packs");
    let skill_dir = root.join("vendor/third-party");
    fs::create_dir_all(&packs_dir).expect("packs dir");
    fs::create_dir_all(&skill_dir).expect("skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        r#"---
name: team-pack-third-party
description: Private team pack for plugin export tests.
---

# Third Party Pack

Use this private pack only inside the owning team workspace.
"#,
    )
    .expect("write third-party skill");
    fs::write(
        packs_dir.join("team-pack-third-party.json"),
        r#"{
  "kind": "pack",
  "id": "team-pack-third-party",
  "version": "1.0.0",
  "title": "Third Party Pack",
  "description": "Regression fixture for third_party import ecosystems.",
  "activation_class": "instruction",
  "side_effect_class": "none",
  "trust_tier": "external_unreviewed",
  "requires_confirmation": false,
  "compatible_roles": ["builder"],
  "compatible_targets": ["codex-cli", "claude-code"],
  "resources": [
    {
      "path": "vendor/third-party/SKILL.md",
      "kind": "instruction",
      "required": true
    }
  ],
  "imports": [
    {
      "ecosystem": "third_party",
      "origin": "https://example.com/third-party-pack"
    }
  ],
  "visibility_scope": "private"
}
"#,
    )
    .expect("write third-party pack manifest");
}

fn seed_custom_library_with_search_lifecycle_pack(root: &Path) {
    let packs_dir = root.join("packs");
    let skill_dir = root.join("vendor/legacy-python-audit");
    fs::create_dir_all(&packs_dir).expect("packs dir");
    fs::create_dir_all(&skill_dir).expect("skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        r#"# Legacy Python Audit

Detect temporal coupling in old Python service modules before refactors land.
"#,
    )
    .expect("write skill");
    fs::write(
        packs_dir.join("legacy-python-audit.json"),
        r#"{
  "kind": "pack",
  "id": "legacy-python-audit",
  "version": "1.0.0",
  "title": "Legacy Python Audit",
  "description": "Audit legacy Python modules before modernization work.",
  "activation_class": "instruction",
  "side_effect_class": "none",
  "trust_tier": "first_party_validated",
  "requires_confirmation": false,
  "compatible_roles": ["builder"],
  "compatible_targets": ["codex-cli"],
  "resources": [
    {
      "path": "vendor/legacy-python-audit/SKILL.md",
      "kind": "instruction",
      "required": true
    }
  ],
  "lifecycle": {
    "status": "deprecated",
    "replacement_pack_ref": {
      "kind": "pack",
      "id": "python-refactor",
      "version": "2.0.0"
    },
    "verified_targets": ["codex-cli"],
    "last_verified_at": "2026-04-22T12:00:00Z",
    "evidence_refs": ["evals/search/legacy-python-audit.json"]
  }
}
"#,
    )
    .expect("write lifecycle pack manifest");
}

fn seed_private_source_library(root: &Path, pack_id: &str) {
    let packs_dir = root.join("packs");
    let skill_dir = root.join("vendor").join(pack_id);
    fs::create_dir_all(&packs_dir).expect("packs dir");
    fs::create_dir_all(&skill_dir).expect("skill dir");
    fs::write(
        root.join("library.json"),
        r#"{"kind":"library","id":"team-library","version":"1.0.0"}"#,
    )
    .expect("write library");
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("# Team Pack\n\nVerifier workflow for {pack_id}.\n"),
    )
    .expect("write skill");
    fs::write(
        packs_dir.join(format!("{pack_id}.json")),
        format!(
            r#"{{
  "kind": "pack",
  "id": "{pack_id}",
  "version": "1.0.0",
  "title": "Team Core Quality",
  "description": "Private verifier workflow fixture.",
  "activation_class": "instruction",
  "side_effect_class": "none",
  "trust_tier": "org_validated",
  "requires_confirmation": false,
  "compatible_roles": ["builder"],
  "compatible_targets": ["codex-cli"],
  "resources": [
    {{
      "path": "vendor/{pack_id}/SKILL.md",
      "kind": "instruction",
      "required": true
    }}
  ],
  "visibility_scope": "private"
}}
"#
        ),
    )
    .expect("write pack");
}

fn seed_user_default_profile(home: &Path, name: &str, profile_yaml: &str) {
    let profiles_dir = home.join(".config/metactl/profiles");
    fs::create_dir_all(&profiles_dir).expect("profiles dir");
    fs::write(profiles_dir.join(format!("{name}.yaml")), profile_yaml).expect("write profile");
    let cfg_dir = home.join(".config/metactl");
    fs::create_dir_all(&cfg_dir).expect("config dir");
    fs::write(
        cfg_dir.join("config.yaml"),
        format!("default_profile: {name}\n"),
    )
    .expect("write user settings");
}

fn seed_tracked_private_source_state(project: &Path) {
    let git_init = Command::new("git")
        .args(["-C", project.to_str().expect("project"), "init", "--quiet"])
        .output()
        .expect("git init");
    assert!(git_init.status.success(), "{}", stderr(&git_init));
    let private_cache = project.join(".metactl/cache/sources/team-library/packs");
    fs::create_dir_all(&private_cache).expect("private cache");
    fs::write(private_cache.join("team-pack-core-quality.json"), "{}").expect("private body");
    fs::create_dir_all(project.join(".metactl/private")).expect("private dir");
    fs::write(
        project.join(".metactl/private/source-lock.json"),
        r#"{"sources":[{"id":"team-library","url":"git@example.com:org/private.git"}]}"#,
    )
    .expect("private lock");
    let git_add = Command::new("git")
        .args([
            "-C",
            project.to_str().expect("project"),
            "add",
            "-f",
            ".metactl/cache/sources",
            ".metactl/private/source-lock.json",
        ])
        .output()
        .expect("git add");
    assert!(git_add.status.success(), "{}", stderr(&git_add));
}

fn git_init_project(project: &Path) {
    let git_init = Command::new("git")
        .args(["-C", project.to_str().expect("project"), "init", "--quiet"])
        .output()
        .expect("git init");
    assert!(git_init.status.success(), "{}", stderr(&git_init));
}

fn git_add_forced(project: &Path, paths: &[&str]) {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(project)
        .arg("add")
        .arg("-f")
        .args(paths);
    let output = command.output().expect("git add");
    assert!(output.status.success(), "{}", stderr(&output));
}

fn git_ls_files(project: &Path) -> String {
    let output = Command::new("git")
        .args(["-C", project.to_str().expect("project"), "ls-files"])
        .output()
        .expect("git ls-files");
    assert!(output.status.success(), "{}", stderr(&output));
    stdout(&output)
}

fn seed_tracked_generated_root(project: &Path, file: &str) {
    git_init_project(project);
    let path = project.join(file);
    fs::create_dir_all(path.parent().expect("generated parent")).expect("generated parent");
    fs::write(&path, "generated\n").expect("generated file");
    git_add_forced(project, &[file]);
}

fn git_commit_all(repo: &Path, message: &str) -> String {
    let add = Command::new("git")
        .args(["-C", repo.to_str().expect("repo"), "add", "."])
        .output()
        .expect("git add");
    assert!(add.status.success(), "{}", stderr(&add));
    let commit = Command::new("git")
        .args([
            "-C",
            repo.to_str().expect("repo"),
            "-c",
            "user.email=test@example.com",
            "-c",
            "user.name=Test User",
            "commit",
            "--quiet",
            "-m",
            message,
        ])
        .output()
        .expect("git commit");
    assert!(commit.status.success(), "{}", stderr(&commit));
    let rev = Command::new("git")
        .args(["-C", repo.to_str().expect("repo"), "rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse");
    assert!(rev.status.success(), "{}", stderr(&rev));
    stdout(&rev).trim().to_string()
}

fn init_project(project: &Path) {
    let output = run_cli(project, &["init", "--target", "codex-cli"]);
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn ignore_status_reports_tracked_generated_roots() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_generated_root(project.path(), ".codex/skills/example/SKILL.md");

    let output = run_cli(
        project.path(),
        &["--json", "ignore", "status", "--target", "codex-cli"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "ignore", Some(project.path()));
    assert_eq!(value["tracked_generated_roots"][0]["root"], ".codex");
    assert!(value["next_commands"][0]
        .as_str()
        .expect("next command")
        .contains("metactl ignore fix --plan"));
}

#[test]
fn ignore_status_agent_json_has_next_commands() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let output = run_cli(
        project.path(),
        &["--agent", "ignore", "status", "--target", "codex-cli"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "ignore", Some(project.path()));
    assert!(
        value["next_commands"]
            .as_array()
            .expect("next commands")
            .len()
            >= 1
    );
}

#[test]
fn ignore_target_resolution_prefers_configured_then_detected() {
    let project = TempDir::new().expect("tempdir");
    let init = run_cli(project.path(), &["init", "--target", "gemini-cli"]);
    assert!(init.status.success(), "{}", stderr(&init));
    fs::create_dir_all(project.path().join(".codex/skills/example")).expect("codex dir");

    let output = run_cli(project.path(), &["--json", "ignore", "status"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_eq!(value["target_source"], "config");
    assert_eq!(value["targets"], json!(["gemini-cli"]));
}

#[test]
fn ignore_fix_plan_reports_actions_without_writes() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_generated_root(project.path(), ".codex/skills/example/SKILL.md");
    let before_files = project_file_snapshot(project.path());
    let before_index = git_ls_files(project.path());

    let output = run_cli(
        project.path(),
        &["--json", "ignore", "fix", "--plan", "--target", "codex-cli"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "ignore", Some(project.path()));
    assert_eq!(value["plan"], json!(true));
    assert!(value["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .any(|item| item["kind"] == "untrack-generated"));
    assert_eq!(project_file_snapshot(project.path()), before_files);
    assert_eq!(git_ls_files(project.path()), before_index);
}

#[test]
fn ignore_fix_yes_untracks_generated_roots_without_deleting_files() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_generated_root(project.path(), ".codex/skills/example/SKILL.md");

    let output = run_cli(
        project.path(),
        &[
            "--json",
            "ignore",
            "fix",
            "--scope",
            "both",
            "--target",
            "codex-cli",
            "--untrack-generated",
            "--yes",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_eq!(value["untracked_generated_roots"][0]["path"], ".codex");
    assert!(project
        .path()
        .join(".codex/skills/example/SKILL.md")
        .exists());
    assert!(!git_ls_files(project.path()).contains(".codex/skills/example/SKILL.md"));
}

#[test]
fn ignore_fix_no_input_requires_untrack_generated() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_generated_root(project.path(), ".codex/skills/example/SKILL.md");

    let output = run_cli(
        project.path(),
        &[
            "--no-input",
            "--json",
            "ignore",
            "fix",
            "--target",
            "codex-cli",
            "--yes",
        ],
    );
    assert!(
        !output.status.success(),
        "ignore fix should require explicit untrack"
    );
    let value = json_output(&output);
    assert_eq!(value["code"], "untrack_generated_required");
    assert!(git_ls_files(project.path()).contains(".codex/skills/example/SKILL.md"));
}

#[test]
fn ignore_fix_agent_plan_json_is_parseable() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let output = run_cli(project.path(), &["--agent", "ignore", "fix", "--plan"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "ignore", Some(project.path()));
    assert_eq!(value["action"], "fix");
    assert_eq!(value["plan"], json!(true));
}

#[test]
fn setup_plan_json_has_replayable_commands() {
    let project = TempDir::new().expect("tempdir");
    let output = run_cli(
        project.path(),
        &["--json", "setup", "--plan", "--target", "codex-cli"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "setup", Some(project.path()));
    assert_eq!(value["plan"], json!(true));
    assert_eq!(value["artifact_policy"], json!("portable-first"));
    assert!(value["next_commands"]
        .as_array()
        .expect("next commands")
        .iter()
        .any(|item| item
            .as_str()
            .unwrap_or("")
            .contains("metactl setup --target codex-cli --yes")));
    assert!(value["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .any(|item| item["kind"] == "agent-artifacts"
            && item["policy"] == "portable-first"
            && item["pack"] == "agentic-artifact-forge"));
    assert!(!project.path().join("metactl.yaml").exists());
}

#[test]
fn setup_agent_mode_never_prompts() {
    let project = TempDir::new().expect("tempdir");
    let output = run_cli(project.path(), &["--agent", "setup", "--plan"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "setup", Some(project.path()));
    assert_eq!(value["plan"], json!(true));
    assert!(value["next_commands"].is_array());
}

#[test]
fn setup_yes_with_explicit_target_creates_config_without_sync() {
    let project = TempDir::new().expect("tempdir");
    let output = run_cli(
        project.path(),
        &["--json", "setup", "--target", "codex-cli", "--yes"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "setup", Some(project.path()));
    assert_eq!(value["ran_sync"], json!(false));
    assert_eq!(value["artifact_policy"], json!("portable-first"));
    assert!(project.path().join("metactl.yaml").exists());
    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("config");
    assert!(config.contains("agent_artifact_policy"));
    assert!(config.contains("portable-first"));
    assert!(config.contains("agentic-artifact-forge"));
    assert!(!project.path().join(".codex").exists());
}

#[test]
fn setup_existing_config_preserves_targets() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let output = run_cli(
        project.path(),
        &["--json", "setup", "--target", "gemini-cli", "--yes"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_eq!(value["already_configured"], json!(true));
    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("config");
    assert!(config.contains("codex-cli"));
    assert!(!config.contains("gemini-cli"));
}

#[test]
fn setup_existing_config_explicit_artifact_policy_updates_config() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let output = run_cli(
        project.path(),
        &[
            "--json",
            "setup",
            "--artifact-policy",
            "portable-first",
            "--yes",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_eq!(value["already_configured"], json!(true));
    assert_eq!(value["artifact_policy"], json!("portable-first"));
    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("config");
    assert!(config.contains("agent_artifact_policy"));
    assert!(config.contains("agentic-artifact-forge"));
}

#[test]
fn doctor_reports_ignore_repair_checks() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_generated_root(project.path(), ".codex/skills/example/SKILL.md");

    let output = run_cli(project.path(), &["--json", "doctor"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "doctor", Some(project.path()));
    let checks = value["checks"].as_array().expect("checks");
    let ignore = checks
        .iter()
        .find(|item| item["id"] == "ignore-repair")
        .expect("ignore check");
    assert_eq!(ignore["status"], "warn");
    assert_eq!(ignore["fix_plan_ref"], "metactl ignore fix --plan");
}

#[test]
fn doctor_agent_json_has_recoverable_next_commands() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let output = run_cli(project.path(), &["--agent", "doctor"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "doctor", Some(project.path()));
    let checks = value["checks"].as_array().expect("checks");
    assert!(checks
        .iter()
        .any(|item| item["id"] == "ignore-repair" && item["next_commands"].as_array().is_some()));
}

#[test]
fn doctor_does_not_mutate_ignore_or_git_index() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_generated_root(project.path(), ".codex/skills/example/SKILL.md");
    let before_files = project_file_snapshot(project.path());
    let before_index = git_ls_files(project.path());

    let output = run_cli(project.path(), &["--json", "doctor"]);
    assert!(output.status.success(), "{}", stderr(&output));

    assert_eq!(project_file_snapshot(project.path()), before_files);
    assert_eq!(git_ls_files(project.path()), before_index);
}

#[test]
fn cli_pack_import_export_verify_skill_roundtrip_records_provenance() {
    let project = TempDir::new().expect("tempdir");
    let skill_root = project.path().join("release-manager");
    let scripts_dir = skill_root.join("scripts");
    fs::create_dir_all(&scripts_dir).expect("scripts dir");
    fs::write(
        skill_root.join("SKILL.md"),
        r#"---
name: release-manager
description: Portable release manager skill for verification handoffs.
---

# Release Manager

Run release verification and produce a handoff.
"#,
    )
    .expect("write skill");
    fs::write(
        scripts_dir.join("check.sh"),
        "#!/usr/bin/env bash\necho check\n",
    )
    .expect("write script");

    let import = run_cli(
        project.path(),
        &[
            "--json",
            "pack",
            "import-skill",
            skill_root.to_str().expect("skill path"),
        ],
    );
    assert!(import.status.success(), "{}", stderr(&import));
    let import_json = json_output(&import);
    assert_json_contract(&import_json, "pack", Some(project.path()));
    assert_eq!(import_json["action"], json!("import-skill"));
    assert_eq!(import_json["pack_id"], json!("release-manager"));
    assert_eq!(
        import_json["script_classification"][0]["path"],
        json!("scripts/check.sh")
    );
    assert_eq!(
        import_json["script_classification"][0]["executable"],
        json!(false)
    );
    assert!(import_json["provenance"]["digest"].as_str().is_some());
    assert!(project
        .path()
        .join(".metactl/imported-packs/release-manager/pack.json")
        .exists());

    let export = run_cli(
        project.path(),
        &[
            "--json",
            "pack",
            "export-skill",
            "release-manager",
            "--target",
            "codex-cli",
        ],
    );
    assert!(export.status.success(), "{}", stderr(&export));
    let export_json = json_output(&export);
    assert_json_contract(&export_json, "pack", Some(project.path()));
    assert_eq!(export_json["action"], json!("export-skill"));
    assert!(project
        .path()
        .join(".metactl/exported-skills/codex-cli/release-manager/SKILL.md")
        .exists());

    let verify = run_cli(
        project.path(),
        &[
            "--json",
            "pack",
            "verify-skill",
            "release-manager",
            "--profile",
            "portable",
        ],
    );
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json = json_output(&verify);
    assert_json_contract(&verify_json, "pack", Some(project.path()));
    assert_eq!(verify_json["action"], json!("verify-skill"));
    assert_eq!(verify_json["profile"], json!("portable"));
    assert_eq!(verify_json["status"], json!("pass"));
}

#[test]
fn cli_skills_add_list_remove_user_global_codex_skill() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    let skill_root = project.path().join("release-manager");
    fs::create_dir_all(&skill_root).expect("skill dir");
    fs::write(
        skill_root.join("SKILL.md"),
        r#"---
name: release-manager
description: Portable release manager skill for verification handoffs.
---

# Release Manager

Run release verification and produce a handoff.
"#,
    )
    .expect("write skill");

    let add = run_cli(
        project.path(),
        &[
            "--json",
            "skills",
            "add",
            skill_root.to_str().expect("skill path"),
            "--scope",
            "user",
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));
    let add_json = json_output(&add);
    assert_json_contract(&add_json, "skills", Some(project.path()));
    assert_eq!(add_json["action"], json!("add"));
    assert_eq!(add_json["scope"], json!("user"));
    assert_eq!(add_json["skill"]["name"], json!("release-manager"));
    assert!(project
        .path()
        .join(".test-home/.codex/skills/release-manager/SKILL.md")
        .exists());

    let list = run_cli(
        project.path(),
        &["--json", "skills", "list", "--scope", "user"],
    );
    assert!(list.status.success(), "{}", stderr(&list));
    let list_json = json_output(&list);
    assert_json_contract(&list_json, "skills", Some(project.path()));
    assert_eq!(list_json["action"], json!("list"));
    assert_eq!(list_json["count"], json!(1));
    assert_eq!(list_json["skills"][0]["name"], json!("release-manager"));

    let remove = run_cli(
        project.path(),
        &[
            "--json",
            "skills",
            "remove",
            "release-manager",
            "--scope",
            "user",
        ],
    );
    assert!(remove.status.success(), "{}", stderr(&remove));
    let remove_json = json_output(&remove);
    assert_json_contract(&remove_json, "skills", Some(project.path()));
    assert_eq!(remove_json["action"], json!("remove"));
    assert!(!project
        .path()
        .join(".test-home/.codex/skills/release-manager")
        .exists());
}

#[test]
fn status_reports_codex_skill_visibility_scopes() {
    let project = TempDir::new().expect("tempdir");
    let setup = run_cli(
        project.path(),
        &["--json", "setup", "--target", "codex-cli", "--yes"],
    );
    assert!(setup.status.success(), "{}", stderr(&setup));
    let repo_skill_root = project
        .path()
        .join(".codex/skills/team-pack/release-manager");
    fs::create_dir_all(&repo_skill_root).expect("repo skill dir");
    fs::write(
        repo_skill_root.join("SKILL.md"),
        r#"---
name: release-manager
description: Portable release manager skill for verification handoffs.
---

# Release Manager
"#,
    )
    .expect("write repo skill");

    let status = run_cli(project.path(), &["--json", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json = json_output(&status);
    assert_json_contract(&json, "status", Some(project.path()));
    assert_eq!(json["skill_visibility"]["target"], json!("codex-cli"));
    assert_eq!(
        json["agent_artifact_policy"]["policy"],
        json!("portable-first")
    );
    assert_eq!(
        json["agent_artifact_policy"]["stewardship_pack_configured"],
        json!(true)
    );
    assert_eq!(json["skill_visibility"]["repo_local_count"], json!(1));
    assert_eq!(
        json["skill_visibility"]["missing_user_global_count"],
        json!(1)
    );
    assert_eq!(
        json["skill_visibility"]["repo_local_skills"][0]["user_global_installed"],
        json!(false)
    );

    let human = run_cli(project.path(), &["status"]);
    assert!(human.status.success(), "{}", stderr(&human));
    let text = stdout(&human);
    assert!(text.contains("Codex skill visibility:"));
    assert!(text.contains("Agent artifact stewardship:"));
    assert!(text.contains("policy: portable-first"));
    assert!(text.contains("repo-local: 1 skill(s)"));
    assert!(text.contains("user-global: 0 skill(s)"));
    assert!(text.contains("metactl skills add <repo-skill-path> --scope user"));
}

#[test]
fn cli_pack_import_skill_rejects_unsafe_agent_skill_fixtures() {
    let project = TempDir::new().expect("tempdir");

    let executable_skill = project.path().join("executable-skill");
    let executable_scripts = executable_skill.join("scripts");
    fs::create_dir_all(&executable_scripts).expect("scripts dir");
    fs::write(
        executable_skill.join("SKILL.md"),
        r#"---
name: executable-skill
description: Skill with an executable script fixture.
---

# Executable Skill
"#,
    )
    .expect("write skill");
    let executable_script = executable_scripts.join("run.sh");
    fs::write(&executable_script, "#!/usr/bin/env bash\necho unsafe\n").expect("write script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&executable_script)
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable_script, permissions).expect("chmod script");
    }

    let executable_import = run_cli(
        project.path(),
        &[
            "pack",
            "import-skill",
            executable_skill.to_str().expect("skill path"),
        ],
    );
    assert!(!executable_import.status.success(), "import should fail");
    assert!(
        stderr(&executable_import)
            .contains("executable script requires --allow-executable-scripts"),
        "stderr: {}",
        stderr(&executable_import)
    );

    let secret_skill = project.path().join("secret-skill");
    fs::create_dir_all(&secret_skill).expect("secret dir");
    fs::write(
        secret_skill.join("SKILL.md"),
        r#"---
name: secret-skill
description: Skill with a hidden secret fixture.
---

# Secret Skill
"#,
    )
    .expect("write skill");
    fs::write(secret_skill.join(".env.secret"), "TOKEN=secret\n").expect("write secret");
    let secret_import = run_cli(
        project.path(),
        &[
            "pack",
            "import-skill",
            secret_skill.to_str().expect("skill path"),
        ],
    );
    assert!(!secret_import.status.success(), "secret import should fail");
    assert!(
        stderr(&secret_import).contains("hidden secret-like file"),
        "stderr: {}",
        stderr(&secret_import)
    );
}

#[test]
fn cli_export_sanitized_and_check_public_boundary_commands_gate_private_markers() {
    let project = TempDir::new().expect("tempdir");

    let public_example = run_cli(
        project.path(),
        &["--json", "export", "public-example", "release-manager"],
    );
    assert!(
        public_example.status.success(),
        "{}",
        stderr(&public_example)
    );
    let public_json = json_output(&public_example);
    assert_json_contract(&public_json, "export", Some(project.path()));
    assert_eq!(public_json["action"], json!("public-example"));
    assert!(project
        .path()
        .join(".metactl/exports/public-examples/release-manager/SKILL.md")
        .exists());

    let sanitized = run_cli(
        project.path(),
        &["--json", "export", "sanitized", "release-manager"],
    );
    assert!(sanitized.status.success(), "{}", stderr(&sanitized));
    let sanitized_json = json_output(&sanitized);
    assert_json_contract(&sanitized_json, "export", Some(project.path()));
    assert_eq!(sanitized_json["action"], json!("sanitized"));
    assert!(sanitized_json["export_lock"]["original_digest"]
        .as_str()
        .is_some());
    assert!(sanitized_json["export_lock"]["sanitized_digest"]
        .as_str()
        .is_some());

    let clean_check = run_cli(project.path(), &["--json", "check-public-boundary"]);
    assert!(clean_check.status.success(), "{}", stderr(&clean_check));
    let clean_json = json_output(&clean_check);
    assert_json_contract(&clean_json, "check-public-boundary", Some(project.path()));
    assert_eq!(clean_json["status"], json!("pass"));

    fs::write(
        project.path().join("unsafe-export.md"),
        "private_source: true\nprivate_kb: mcp://private-kb/release-policy\n",
    )
    .expect("write unsafe marker");
    let unsafe_check = run_cli(project.path(), &["--json", "check-public-boundary"]);
    assert!(!unsafe_check.status.success(), "unsafe check should fail");
    let unsafe_json = json_output(&unsafe_check);
    assert_eq!(unsafe_json["ok"], json!(false));
    assert!(unsafe_json["details"][0]
        .as_str()
        .unwrap_or_default()
        .contains("unsafe-export.md"));
}

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
fn cli_help_and_parsing_main_help() {
    let output = Command::new(cli_bin())
        .arg("--help")
        .output()
        .expect("help");
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("metactl init"));
    assert!(text.contains("metactl init --bind-profile"));
    assert!(text.contains("metactl sync"));
    assert!(text.contains("metactl apply"));
    assert!(text.contains("Common workflow"));
}

#[test]
fn cli_list_packs_supports_third_party_import_ecosystem_in_custom_library() {
    let project = TempDir::new().expect("tempdir");
    let custom_library = TempDir::new().expect("custom library");
    seed_custom_library_with_third_party_pack(custom_library.path());

    fs::write(
        project.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nstarter_library:\n- {}\n- {}\ndefaults:\n  brownfield_mode: refuse_due_to_conflict\n  discovery_mode: candidate_search\n",
            starter_library_root(),
            custom_library.path().display()
        ),
    )
    .expect("write metactl.yaml");

    let output = run_cli(project.path(), &["list", "packs"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains("team-pack-third-party"),
        "custom library pack should be listed: {}",
        text
    );
}

#[test]
fn cli_plugin_exports_private_library_to_local_marketplace() {
    let project = TempDir::new().expect("tempdir");
    let custom_library = TempDir::new().expect("custom library");
    seed_custom_library_with_third_party_pack(custom_library.path());
    let marketplace = project.path().join("private-plugin-marketplace");

    let list = run_cli(
        project.path(),
        &[
            "--json",
            "plugin",
            "list",
            "--tier",
            "private",
            "--library-root",
            custom_library.path().to_str().expect("library path"),
            "--target",
            "codex-cli",
        ],
    );
    assert!(list.status.success(), "{}", stderr(&list));
    let list_json = json_output(&list);
    assert_json_contract(&list_json, "plugin", Some(project.path()));
    assert_eq!(list_json["packs"][0]["pack_id"], "team-pack-third-party");

    let output = run_cli(
        project.path(),
        &[
            "--json",
            "plugin",
            "export",
            "--tier",
            "private",
            "--library-root",
            custom_library.path().to_str().expect("library path"),
            "--target",
            "codex-cli",
            "--out",
            marketplace.to_str().expect("marketplace path"),
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "plugin", Some(project.path()));
    assert_eq!(value["action"], "export");
    assert_eq!(value["result"]["tier"], "private");
    assert_eq!(value["result"]["pack_ids"][0], "team-pack-third-party");

    let plugin_path = PathBuf::from(
        value["result"]["plugin_path"]
            .as_str()
            .expect("plugin path"),
    );
    let marketplace_manifest = marketplace.join(".agents/plugins/marketplace.json");
    assert!(marketplace_manifest.exists());
    let marketplace_json: Value =
        serde_json::from_slice(&fs::read(&marketplace_manifest).expect("marketplace bytes"))
            .expect("marketplace json");
    assert_eq!(
        marketplace_json["plugins"][0]["source"]["path"],
        format!(
            "./plugins/{}",
            value["result"]["plugin_name"]
                .as_str()
                .expect("plugin name")
        )
    );
    assert!(plugin_path.join(".codex-plugin/plugin.json").exists());
    assert!(plugin_path
        .join(".codex-plugin/metactl-projection.json")
        .exists());
    assert!(plugin_path
        .join("skills/team-pack-third-party/SKILL.md")
        .exists());

    let projection: Value = serde_json::from_slice(
        &fs::read(plugin_path.join(".codex-plugin/metactl-projection.json"))
            .expect("projection bytes"),
    )
    .expect("projection json");
    assert_eq!(projection["output_tier"], "private");
    assert_eq!(projection["target_runtime"], "codex-cli");
    assert!(projection["source_library"]
        .as_str()
        .expect("source library")
        .contains(custom_library.path().to_str().expect("library path")));

    let verify = run_cli(
        project.path(),
        &[
            "--json",
            "plugin",
            "verify",
            "--target",
            "codex-cli",
            "--tier",
            "private",
            "--path",
            marketplace.to_str().expect("marketplace path"),
        ],
    );
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json = json_output(&verify);
    assert_json_contract(&verify_json, "plugin", Some(project.path()));
    assert_eq!(verify_json["report"]["status"], "pass");
    assert_eq!(verify_json["report"]["pack_count"], 1);
}

#[test]
fn cli_plugin_exports_public_starter_without_private_projection_paths() {
    let project = TempDir::new().expect("tempdir");
    let marketplace = project.path().join("public-plugin-marketplace");

    let output = run_cli(
        project.path(),
        &[
            "--json",
            "plugin",
            "export",
            "--tier",
            "public",
            "--target",
            "codex-cli",
            "--out",
            marketplace.to_str().expect("marketplace path"),
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "plugin", Some(project.path()));
    assert_eq!(value["action"], "export");
    assert_eq!(value["result"]["tier"], "public");

    let plugin_path = PathBuf::from(
        value["result"]["plugin_path"]
            .as_str()
            .expect("plugin path"),
    );
    let marketplace_manifest = marketplace.join(".agents/plugins/marketplace.json");
    assert!(marketplace_manifest.exists());
    let marketplace_json: Value =
        serde_json::from_slice(&fs::read(&marketplace_manifest).expect("marketplace bytes"))
            .expect("marketplace json");
    assert_eq!(
        marketplace_json["plugins"][0]["source"]["path"],
        format!(
            "./plugins/{}",
            value["result"]["plugin_name"]
                .as_str()
                .expect("plugin name")
        )
    );
    let projection_path = plugin_path.join(".codex-plugin/metactl-projection.json");
    let projection_text = fs::read_to_string(&projection_path).expect("projection text");
    assert!(projection_text.contains("\"source_library\": \"library/starter\""));
    assert!(
        !projection_text.contains("/Users/"),
        "public projection should not include machine paths: {}",
        projection_text
    );
    assert!(
        !projection_text.contains("source_manifest_path"),
        "public projection should not include source manifest paths: {}",
        projection_text
    );

    let verify = run_cli(
        project.path(),
        &[
            "--json",
            "plugin",
            "verify",
            "--target",
            "codex-cli",
            "--tier",
            "public",
            "--path",
            marketplace.to_str().expect("marketplace path"),
        ],
    );
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json = json_output(&verify);
    assert_eq!(verify_json["report"]["status"], "pass");
    assert!(
        verify_json["report"]["pack_count"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
}

#[test]
fn cli_plugin_exports_private_library_to_claude_marketplace() {
    let project = TempDir::new().expect("tempdir");
    let custom_library = TempDir::new().expect("custom library");
    seed_custom_library_with_third_party_pack(custom_library.path());
    let marketplace = project.path().join("private-claude-plugin-marketplace");

    let output = run_cli(
        project.path(),
        &[
            "--json",
            "plugin",
            "export",
            "--tier",
            "private",
            "--library-root",
            custom_library.path().to_str().expect("library path"),
            "--target",
            "claude-code",
            "--out",
            marketplace.to_str().expect("marketplace path"),
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "plugin", Some(project.path()));
    assert_eq!(value["result"]["target"], "claude-code");
    assert_eq!(value["result"]["pack_ids"][0], "team-pack-third-party");

    let plugin_path = PathBuf::from(
        value["result"]["plugin_path"]
            .as_str()
            .expect("plugin path"),
    );
    let marketplace_manifest = marketplace.join(".claude-plugin/marketplace.json");
    assert!(marketplace_manifest.exists());
    let marketplace_json: Value =
        serde_json::from_slice(&fs::read(&marketplace_manifest).expect("marketplace bytes"))
            .expect("marketplace json");
    assert_eq!(
        marketplace_json["plugins"][0]["source"],
        format!(
            "./plugins/{}",
            value["result"]["plugin_name"]
                .as_str()
                .expect("plugin name")
        )
    );
    assert!(plugin_path.join(".claude-plugin/plugin.json").exists());
    assert!(plugin_path.join(".metactl/plugin-projection.json").exists());
    assert!(plugin_path
        .join("skills/team-pack-third-party/SKILL.md")
        .exists());

    let projection: Value = serde_json::from_slice(
        &fs::read(plugin_path.join(".metactl/plugin-projection.json")).expect("projection bytes"),
    )
    .expect("projection json");
    assert_eq!(projection["output_tier"], "private");
    assert_eq!(projection["target_runtime"], "claude-code");
    assert!(projection["source_library"]
        .as_str()
        .expect("source library")
        .contains(custom_library.path().to_str().expect("library path")));

    let verify = run_cli(
        project.path(),
        &[
            "--json",
            "plugin",
            "verify",
            "--target",
            "claude-code",
            "--tier",
            "private",
            "--path",
            marketplace.to_str().expect("marketplace path"),
        ],
    );
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json = json_output(&verify);
    assert_json_contract(&verify_json, "plugin", Some(project.path()));
    assert_eq!(verify_json["report"]["status"], "pass");
    assert_eq!(verify_json["report"]["pack_count"], 1);
}

#[test]
fn cli_plugin_exports_public_claude_starter_without_private_projection_paths() {
    let project = TempDir::new().expect("tempdir");
    let marketplace = project.path().join("public-claude-plugin-marketplace");

    let output = run_cli(
        project.path(),
        &[
            "--json",
            "plugin",
            "export",
            "--tier",
            "public",
            "--target",
            "claude-code",
            "--out",
            marketplace.to_str().expect("marketplace path"),
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "plugin", Some(project.path()));
    assert_eq!(value["result"]["tier"], "public");
    assert_eq!(value["result"]["target"], "claude-code");

    let plugin_path = PathBuf::from(
        value["result"]["plugin_path"]
            .as_str()
            .expect("plugin path"),
    );
    let marketplace_manifest = marketplace.join(".claude-plugin/marketplace.json");
    assert!(marketplace_manifest.exists());
    let marketplace_json: Value =
        serde_json::from_slice(&fs::read(&marketplace_manifest).expect("marketplace bytes"))
            .expect("marketplace json");
    assert_eq!(
        marketplace_json["plugins"][0]["source"],
        format!(
            "./plugins/{}",
            value["result"]["plugin_name"]
                .as_str()
                .expect("plugin name")
        )
    );
    assert!(plugin_path.join(".claude-plugin/plugin.json").exists());
    let projection_path = plugin_path.join(".metactl/plugin-projection.json");
    let projection_text = fs::read_to_string(&projection_path).expect("projection text");
    assert!(projection_text.contains("\"source_library\": \"library/starter\""));
    assert!(
        !projection_text.contains("/Users/"),
        "public projection should not include machine paths: {}",
        projection_text
    );
    assert!(
        !projection_text.contains("source_manifest_path"),
        "public projection should not include source manifest paths: {}",
        projection_text
    );

    let verify = run_cli(
        project.path(),
        &[
            "--json",
            "plugin",
            "verify",
            "--target",
            "claude-code",
            "--tier",
            "public",
            "--path",
            marketplace.to_str().expect("marketplace path"),
        ],
    );
    assert!(verify.status.success(), "{}", stderr(&verify));
    let verify_json = json_output(&verify);
    assert_eq!(verify_json["report"]["status"], "pass");
    assert!(
        verify_json["report"]["pack_count"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
}

#[test]
fn cli_help_subcommand_shows_add_usage() {
    let output = Command::new(cli_bin())
        .args(["help", "add"])
        .output()
        .expect("help add");
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("add"));
    assert!(text.contains("--sync"));
    assert!(text.contains("PACK_IDS"));
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
fn cli_help_and_parsing_list_subcommand() {
    let output = Command::new(cli_bin())
        .args(["list", "roles", "--json"])
        .output()
        .expect("list roles");
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "list", Some(&std::env::current_dir().expect("cwd")));
    assert_eq!(value["subject"], "roles");
    assert!(value["items"].is_array());
}

#[test]
fn cli_json_output_public_commands() {
    let project = TempDir::new().expect("tempdir");
    let init = run_cli(project.path(), &["--json", "init", "--target", "codex-cli"]);
    assert!(init.status.success(), "{}", stderr(&init));
    let init_json = json_output(&init);
    assert_json_contract(&init_json, "init", Some(project.path()));

    let search = run_cli(project.path(), &["--json", "search", "python refactor"]);
    assert!(
        search.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&search),
        stderr(&search)
    );
    let search_json = json_output(&search);
    assert_json_contract(&search_json, "search", Some(project.path()));

    let explain = run_cli(project.path(), &["--json", "explain"]);
    assert!(explain.status.success(), "{}", stderr(&explain));
    let explain_json = json_output(&explain);
    assert_json_contract(&explain_json, "explain", Some(project.path()));

    let sync = run_cli(project.path(), &["--json", "sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    let sync_json = json_output(&sync);
    assert_json_contract(&sync_json, "sync", Some(project.path()));

    let compile = run_cli(project.path(), &["--json", "compile"]);
    assert!(
        compile.status.success(),
        "stdout: {}\nstderr: {}",
        stdout(&compile),
        stderr(&compile)
    );
    let compile_json = json_output(&compile);
    assert_json_contract(&compile_json, "compile", Some(project.path()));

    let apply = run_cli(project.path(), &["--json", "apply", "--mode", "symlink"]);
    assert!(apply.status.success(), "{}", stderr(&apply));
    let apply_json = json_output(&apply);
    assert_json_contract(&apply_json, "apply", Some(project.path()));

    let validate = run_cli(project.path(), &["--json", "validate"]);
    assert!(validate.status.success(), "{}", stderr(&validate));
    let validate_json = json_output(&validate);
    assert_json_contract(&validate_json, "validate", Some(project.path()));

    let doctor = run_cli(project.path(), &["--json", "doctor"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let doctor_json = json_output(&doctor);
    assert_json_contract(&doctor_json, "doctor", Some(project.path()));

    let target = run_cli(project.path(), &["--json", "target", "list"]);
    assert!(target.status.success(), "{}", stderr(&target));
    let target_json = json_output(&target);
    assert_json_contract(&target_json, "target", Some(project.path()));

    let revert = run_cli(project.path(), &["--json", "revert"]);
    assert!(revert.status.success(), "{}", stderr(&revert));
    let revert_json = json_output(&revert);
    assert_json_contract(&revert_json, "revert", Some(project.path()));

    let version = Command::new(cli_bin())
        .args(["--json", "version"])
        .output()
        .expect("version");
    assert!(version.status.success(), "{}", stderr(&version));
    let version_json = json_output(&version);
    assert_json_contract(&version_json, "version", None);
}

#[test]
fn cli_bare_group_defaults_are_read_only_and_json_compatible() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let commands: &[(&[&str], &str, &str)] = &[
        (&["target"], "target", "list"),
        (&["source"], "source", "list"),
        (&["profile"], "profile", "show"),
        (&["ignore"], "ignore", "status"),
        (&["audit"], "audit", "sources"),
        (&["fleet"], "fleet", "status"),
        (&["demo"], "demo list", "list"),
    ];

    for (args, command, action) in commands {
        let before = project_file_snapshot(project.path());
        let mut cli_args = vec!["--json"];
        cli_args.extend_from_slice(args);
        let output = run_cli(project.path(), &cli_args);
        assert!(
            output.status.success(),
            "{:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            stdout(&output),
            stderr(&output)
        );
        let json = json_output(&output);
        assert_json_contract(&json, command, Some(project.path()));
        assert_eq!(json["action"], *action, "{args:?} json: {json}");
        assert_eq!(
            project_file_snapshot(project.path()),
            before,
            "{args:?} wrote files"
        );
    }
}

#[test]
fn cli_preview_alias_runs_sync_preview_without_materializing_runtime_files() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let preview = run_cli(project.path(), &["--json", "preview"]);
    assert!(preview.status.success(), "{}", stderr(&preview));
    let json = json_output(&preview);
    assert_json_contract(&json, "sync", Some(project.path()));
    assert_eq!(json["preview"], true);
    assert!(
        !project.path().join("AGENTS.md").exists(),
        "preview alias must not apply runtime files"
    );
}

#[test]
fn cli_agent_mode_implies_json_no_input_and_error_contract() {
    let project = TempDir::new().expect("tempdir");
    let output = run_cli(project.path(), &["--agent", "init"]);
    assert_eq!(output.status.code(), Some(10), "{}", stderr(&output));
    assert!(
        stderr(&output).is_empty(),
        "--agent should not emit human stderr: {}",
        stderr(&output)
    );
    let json = json_output(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["command"], "init");
    assert_eq!(json["error_code"], "state");
    assert_eq!(json["requires_operator"], false);
    assert!(json["next_commands"]
        .as_array()
        .expect("next_commands")
        .iter()
        .any(|item| item
            .as_str()
            .unwrap_or_default()
            .contains("metactl init --target")));
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
fn pack_activation_object_aliases_match_top_level_verbs() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let add = run_cli(project.path(), &["--json", "pack", "add", "unit-test-loop"]);
    assert!(add.status.success(), "{}", stderr(&add));
    let add_json = json_output(&add);
    assert_json_contract(&add_json, "add", Some(project.path()));
    assert_eq!(add_json["added"][0], "unit-test-loop");

    let remove = run_cli(
        project.path(),
        &["--json", "pack", "remove", "unit-test-loop"],
    );
    assert!(remove.status.success(), "{}", stderr(&remove));
    let remove_json = json_output(&remove);
    assert_json_contract(&remove_json, "remove", Some(project.path()));
    assert_eq!(remove_json["removed"][0], "unit-test-loop");
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
fn cli_search_json_contract_locks_minimum_fields_and_tolerates_additions() {
    let project = TempDir::new().expect("tempdir");
    let init = run_cli(project.path(), &["--json", "init", "--target", "codex-cli"]);
    assert!(init.status.success(), "{}", stderr(&init));

    let search = run_cli(project.path(), &["--json", "search", "tests"]);
    assert!(search.status.success(), "{}", stderr(&search));
    let search_json = json_output(&search);
    assert_json_contract(&search_json, "search", Some(project.path()));
    assert_eq!(search_json["classification"], "matches");

    let matches = search_json["matches"].as_array().expect("matches array");
    assert!(!matches.is_empty(), "expected at least one search match");

    let first = &matches[0];
    assert!(
        first["pack_ref"]["id"].as_str().is_some(),
        "match should include pack_ref.id: {first}"
    );
    assert!(
        first["score"].as_f64().is_some(),
        "match should include score: {first}"
    );
    assert!(
        first["why"].as_str().is_some(),
        "match should include why: {first}"
    );
    assert!(
        first.as_object().is_some_and(|obj| obj.len() >= 3),
        "match should tolerate additive fields beyond the documented minimum: {first}"
    );
}

#[test]
fn cli_search_json_reports_match_evidence_and_lifecycle_hints() {
    let project = TempDir::new().expect("tempdir");
    let custom_library = TempDir::new().expect("custom library");
    seed_custom_library_with_search_lifecycle_pack(custom_library.path());

    fs::write(
        project.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nstarter_library:\n- {}\n- {}\ndefaults:\n  brownfield_mode: refuse_due_to_conflict\n",
            starter_library_root(),
            custom_library.path().display()
        ),
    )
    .expect("write metactl.yaml");

    let search = run_cli(project.path(), &["--json", "search", "temporal coupling"]);
    assert!(
        search.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&search),
        stderr(&search)
    );
    let search_json = json_output(&search);
    assert_json_contract(&search_json, "search", Some(project.path()));

    let legacy = search_json["matches"]
        .as_array()
        .expect("matches")
        .iter()
        .find(|item| item["pack_ref"]["id"] == "legacy-python-audit")
        .expect("legacy-python-audit match");

    assert_eq!(legacy["lifecycle"]["status"], "deprecated");
    assert_eq!(
        legacy["lifecycle"]["replacement_pack_ref"]["id"],
        "python-refactor"
    );
    assert!(legacy["match_evidence"]["matched_resource_paths"]
        .as_array()
        .expect("matched_resource_paths")
        .iter()
        .any(|item| item == "vendor/legacy-python-audit/SKILL.md"));
    assert!(legacy["match_evidence"]["matched_terms"]
        .as_array()
        .expect("matched_terms")
        .iter()
        .any(|item| item == "temporal"));
}

#[test]
fn search_eval_harness_emits_local_artifact() {
    let project = TempDir::new().expect("tempdir");
    let output_path = project.path().join("starter-search-eval.json");
    let script_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/evaluate_search.py");

    let out = Command::new("python3")
        .arg(script_path)
        .arg("--metactl-bin")
        .arg(cli_bin())
        .arg("--output")
        .arg(&output_path)
        .output()
        .expect("run search eval harness");
    assert!(out.status.success(), "{}", stderr(&out));

    let artifact: Value =
        serde_json::from_slice(&fs::read(&output_path).expect("read eval artifact"))
            .expect("decode eval artifact");
    assert_eq!(artifact["api_version"], metactl::API_VERSION);
    assert!(artifact["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .any(|case| case["query"] == "python refactor"));
    assert!(artifact["freshness"]
        .as_array()
        .expect("freshness")
        .iter()
        .any(|entry| entry["pack_id"] == "python-refactor"));
}

#[test]
fn cli_init_compile_apply_revert_greenfield() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let compile = run_cli(project.path(), &["compile"]);
    assert!(
        compile.status.success(),
        "stdout: {}\nstderr: {}",
        stdout(&compile),
        stderr(&compile)
    );
    assert!(project
        .path()
        .join(".metactl/generated/codex-cli/AGENTS.md")
        .exists());

    let apply = run_cli(project.path(), &["apply", "--mode", "symlink"]);
    assert!(apply.status.success(), "{}", stderr(&apply));
    assert!(project.path().join("AGENTS.md").exists());
    assert!(project
        .path()
        .join(".codex/skills/python-refactor/python-refactor/SKILL.md")
        .exists());
    assert!(
        !project
            .path()
            .join(".codex/skills/python-refactor/contracts/SKILL.md")
            .exists(),
        "contracts surface should stay suppressed in default minimal mode"
    );
    assert!(project
        .path()
        .join(".metactl/state/managed_files.json")
        .exists());

    let revert = run_cli(project.path(), &["revert"]);
    assert!(revert.status.success(), "{}", stderr(&revert));
    assert!(!project.path().join("AGENTS.md").exists());
    assert!(!project.path().join(".codex/skills").exists());
}

#[test]
fn cli_compile_apply_chained_greenfield() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let out = run_cli(project.path(), &["--json", "compile", "--apply"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let value = json_output(&out);
    assert_json_contract(&value, "compile", Some(project.path()));
    assert!(value.get("apply").is_some());
    assert_eq!(value["apply"]["ok"], true);
    assert_eq!(value["apply"]["command"], "apply");

    assert!(project.path().join("AGENTS.md").exists());
    assert!(project
        .path()
        .join(".codex/skills/python-refactor/python-refactor/SKILL.md")
        .exists());

    let validate = run_cli(project.path(), &["validate"]);
    assert!(validate.status.success(), "{}", stderr(&validate));
}

#[test]
fn cli_lock_and_doctor_detects_stale_lock() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    let compile = run_cli(project.path(), &["compile"]);
    assert!(compile.status.success(), "{}", stderr(&compile));

    let config_path = project.path().join("metactl.yaml");
    let updated = fs::read_to_string(&config_path)
        .expect("read config")
        .replace("builder", "reviewer");
    fs::write(&config_path, updated).expect("write config");

    let compile_stale = run_cli(project.path(), &["compile"]);
    assert_eq!(
        compile_stale.status.code(),
        Some(11),
        "{}",
        stdout(&compile_stale)
    );
    assert!(stderr(&compile_stale).contains("stale"));

    let doctor = run_cli(project.path(), &["--json", "doctor"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let json = json_output(&doctor);
    let checks = json["checks"].as_array().expect("checks array");
    assert!(checks
        .iter()
        .any(|item| item["id"] == "lock" && item["status"] == "fail"));
}

#[test]
fn cli_lock_and_doctor_weak_corpus() {
    let project = TempDir::new().expect("tempdir");
    let empty_library = TempDir::new().expect("empty library");
    let init = run_cli(
        project.path(),
        &[
            "init",
            "--target",
            "codex-cli",
            "--starter-library",
            empty_library.path().to_str().expect("library path"),
        ],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    let search = run_cli(project.path(), &["--json", "search", "python refactor"]);
    assert!(search.status.success(), "{}", stderr(&search));
    let json = json_output(&search);
    assert_eq!(json["classification"], "matches");
    assert!(json["result_count"].as_u64().unwrap_or_default() > 0);

    let doctor = run_cli(project.path(), &["--json", "doctor"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let checks = json_output(&doctor)["checks"]
        .as_array()
        .expect("doctor checks")
        .clone();
    assert!(checks
        .iter()
        .any(|item| item["id"] == "starter-library" && item["status"] == "pass"));
}

#[test]
fn cli_greenfield_workflow_end_to_end() {
    let project = TempDir::new().expect("tempdir");
    let commands = [
        vec!["init", "--target", "codex-cli"],
        vec!["search", "python refactor"],
        vec!["explain"],
        vec!["sync"],
        vec!["revert"],
    ];

    for command in commands {
        let output = run_cli(project.path(), &command);
        assert!(
            output.status.success(),
            "{}\n{}",
            stdout(&output),
            stderr(&output)
        );
    }

    assert!(project.path().join(".gitignore").exists());
    assert!(project.path().join(".metactl/history").exists());
    assert!(!project.path().join("AGENTS.md").exists());
}

#[test]
fn cli_ignore_install_local_writes_git_exclude_only() {
    let project = TempDir::new().expect("tempdir");
    fs::create_dir_all(project.path().join(".git/info")).expect("create git info");

    let output = run_cli(
        project.path(),
        &[
            "ignore",
            "install",
            "--scope",
            "local",
            "--target",
            "codex-cli",
            "--target",
            "cursor",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));

    let exclude =
        fs::read_to_string(project.path().join(".git/info/exclude")).expect("read exclude");
    assert!(exclude.contains("# metactl:begin generated-agent-surfaces"));
    assert!(exclude.contains(".codex/"));
    assert!(exclude.contains(".cursor/"));
    assert!(exclude.contains("metactl.local.yaml"));
    assert!(!exclude.contains("/.agents/"));
    assert!(!exclude.contains("/.codex/"));
    assert!(!exclude.contains("/.cursor/"));
    assert!(!exclude.contains("/metactl.local.yaml"));
    assert!(!project.path().join(".cursorignore").exists());
    assert!(!project.path().join(".geminiignore").exists());

    let second = run_cli(
        project.path(),
        &[
            "ignore",
            "install",
            "--scope",
            "local",
            "--target",
            "codex-cli",
        ],
    );
    assert!(second.status.success(), "{}", stderr(&second));
    let updated =
        fs::read_to_string(project.path().join(".git/info/exclude")).expect("read exclude");
    assert_eq!(
        updated
            .matches("# metactl:begin generated-agent-surfaces")
            .count(),
        1,
        "managed ignore block should be replaced idempotently"
    );
    assert!(updated.contains(".codex/"));
    assert!(!updated.contains(".cursor/"));

    let status = run_cli(project.path(), &["ignore", "status", "--target", "cursor"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_stdout = stdout(&status);
    assert!(
        !status_stdout.contains("repo-scoped Git ignores can hide Cursor skills"),
        "local exclude posture should not warn about repo-scoped agent allowlists:\n{}",
        status_stdout
    );
}

#[test]
fn cli_ignore_install_can_include_private_source_paths() {
    let project = TempDir::new().expect("tempdir");
    fs::create_dir_all(project.path().join(".git/info")).expect("create git info");

    let output = run_cli(
        project.path(),
        &[
            "ignore",
            "install",
            "--scope",
            "local",
            "--include-private-sources",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));

    let exclude =
        fs::read_to_string(project.path().join(".git/info/exclude")).expect("read exclude");
    assert!(exclude.contains(".metactl/cache/sources/"));
    assert!(exclude.contains(".metactl/private/source-lock.json"));
}

#[test]
fn cli_ignore_status_reports_private_source_protection() {
    let project = TempDir::new().expect("tempdir");
    fs::create_dir_all(project.path().join(".git/info")).expect("create git info");

    let before = run_cli(project.path(), &["--json", "ignore", "status"]);
    assert!(before.status.success(), "{}", stderr(&before));
    let before_json = json_output(&before);
    assert_eq!(before_json["private_sources"]["protected"], false);
    let before_human = run_cli(project.path(), &["ignore", "status"]);
    assert!(before_human.status.success(), "{}", stderr(&before_human));
    assert!(stdout(&before_human)
        .contains("next: metactl ignore install --scope local --include-private-sources"));

    let install = run_cli(
        project.path(),
        &[
            "ignore",
            "install",
            "--scope",
            "local",
            "--include-private-sources",
        ],
    );
    assert!(install.status.success(), "{}", stderr(&install));

    let after = run_cli(project.path(), &["--json", "ignore", "status"]);
    assert!(after.status.success(), "{}", stderr(&after));
    let after_json = json_output(&after);
    assert_eq!(after_json["private_sources"]["protected"], true);
    assert_eq!(after_json["private_sources"]["cache_protected"], true);
    assert_eq!(
        after_json["private_sources"]["private_lock_protected"],
        true
    );
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
fn missing_config_errors_include_init_hints() {
    let project = TempDir::new().expect("tempdir");

    let sync = run_cli(project.path(), &["sync", "--preview"]);
    assert_eq!(sync.status.code(), Some(10), "{}", stderr(&sync));
    let sync_stderr = stderr(&sync);
    assert!(sync_stderr.contains("Project config"));
    assert!(sync_stderr.contains("metactl init --detect"));
    assert!(sync_stderr.contains("metactl init -t codex-cli"));
    assert!(sync_stderr.contains("--config PATH"));

    let validate = run_cli(project.path(), &["validate"]);
    assert_eq!(validate.status.code(), Some(10), "{}", stderr(&validate));
    assert!(stderr(&validate).contains("metactl init --detect"));
}

#[test]
fn cli_ignore_install_repo_writes_gitignore_and_agent_allowlists() {
    let project = TempDir::new().expect("tempdir");

    let output = run_cli(
        project.path(),
        &["ignore", "install", "--scope", "repo", "--target", "all"],
    );
    assert!(output.status.success(), "{}", stderr(&output));

    let gitignore = fs::read_to_string(project.path().join(".gitignore")).expect("read gitignore");
    assert!(gitignore.contains(".metactl/"));
    assert!(gitignore.contains(".codex/"));
    assert!(gitignore.contains(".cursor/"));
    assert!(gitignore.contains(".claude/"));
    assert!(gitignore.contains(".gemini/"));
    assert!(gitignore.contains("CLAUDE.local.md"));
    assert!(gitignore.contains("GEMINI.local.md"));
    assert!(!gitignore.contains("/.agents/"));
    assert!(!gitignore.contains("/.metactl/"));
    assert!(!gitignore.contains("/.codex/"));
    assert!(!gitignore.contains("/.cursor/"));
    assert!(!gitignore.contains("/.claude/"));
    assert!(!gitignore.contains("/.gemini/"));
    assert!(!gitignore.contains("/CLAUDE.local.md"));
    assert!(!gitignore.contains("/GEMINI.local.md"));
    assert!(!gitignore.contains("/metactl.lock.json"));

    let cursorignore =
        fs::read_to_string(project.path().join(".cursorignore")).expect("read cursorignore");
    assert!(cursorignore.contains("# metactl:begin agent-surface-allowlist"));
    assert!(cursorignore.contains("!/.cursor/rules/**"));
    assert!(cursorignore.contains("!/.cursor/skills/**"));
    assert!(cursorignore.contains("!/.codex/skills/**"));

    let geminiignore =
        fs::read_to_string(project.path().join(".geminiignore")).expect("read geminiignore");
    assert!(geminiignore.contains("# metactl:begin agent-surface-allowlist"));
    assert!(geminiignore.contains("!/.gemini/extensions/**"));

    let second = run_cli(
        project.path(),
        &["ignore", "install", "--scope", "repo", "--target", "all"],
    );
    assert!(second.status.success(), "{}", stderr(&second));
    let updated = fs::read_to_string(project.path().join(".cursorignore"))
        .expect("read updated cursorignore");
    assert_eq!(
        updated
            .matches("# metactl:begin agent-surface-allowlist")
            .count(),
        1,
        "managed agent allowlist block should be replaced idempotently"
    );
}

#[test]
fn cli_ignore_status_warns_when_repo_gitignore_can_hide_cursor_surfaces() {
    let project = TempDir::new().expect("tempdir");
    fs::write(project.path().join(".gitignore"), "/.cursor/\n").expect("write gitignore");

    let output = run_cli(project.path(), &["ignore", "status", "--target", "cursor"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let output_stdout = stdout(&output);
    assert!(
        output_stdout.contains("repo-scoped Git ignores can hide Cursor skills"),
        "status should warn when repo gitignore can hide Cursor surfaces:\n{}",
        output_stdout
    );
}

#[test]
fn cli_sync_greenfield_workflow() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let sync = run_cli(project.path(), &["--json", "sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    let json = json_output(&sync);
    assert_json_contract(&json, "sync", Some(project.path()));
    assert_eq!(json["preview"], false);
    assert!(matches!(
        json["targets"][0]["status"].as_str(),
        Some("ready" | "degraded")
    ));
    assert_eq!(json["apply"]["command"], "apply");
    assert_eq!(json["validate"]["command"], "validate");
    assert!(project.path().join("AGENTS.md").exists());
    assert!(project
        .path()
        .join(".codex/skills/python-refactor/python-refactor/SKILL.md")
        .exists());
}

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

#[test]
fn cli_sync_codex_skill_outputs_are_regular_files() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let skill_path = project
        .path()
        .join(".codex/skills/python-refactor/python-refactor/SKILL.md");
    let metadata = fs::symlink_metadata(&skill_path).expect("skill metadata");
    assert!(
        metadata.file_type().is_file(),
        "Codex skill bodies should materialize as regular files so Codex can discover them: {}",
        skill_path.display()
    );
    assert!(
        !metadata.file_type().is_symlink(),
        "Codex skill bodies should not be symlinks: {}",
        skill_path.display()
    );
}

#[test]
fn cli_sync_root_instruction_outputs_are_regular_files_under_symlink_apply() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let first_sync = run_cli(project.path(), &["sync"]);
    assert!(first_sync.status.success(), "{}", stderr(&first_sync));

    let second_sync = run_cli(project.path(), &["sync"]);
    assert!(second_sync.status.success(), "{}", stderr(&second_sync));

    let agents_path = project.path().join("AGENTS.md");
    let agents_metadata = fs::symlink_metadata(&agents_path).expect("AGENTS.md metadata");
    assert!(
        agents_metadata.file_type().is_file() && !agents_metadata.file_type().is_symlink(),
        "Codex AGENTS.md should remain a regular file under symlink apply on repeat sync: {}",
        agents_path.display()
    );

    let skill_path = project
        .path()
        .join(".codex/skills/python-refactor/python-refactor/SKILL.md");
    let skill_metadata = fs::symlink_metadata(&skill_path).expect("skill metadata");
    assert!(
        skill_metadata.file_type().is_file() && !skill_metadata.file_type().is_symlink(),
        "Codex skill bodies should remain regular files under symlink apply: {}",
        skill_path.display()
    );
}

#[test]
fn cli_repeat_sync_preserves_existing_regular_managed_outputs() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let agents_path = project.path().join("AGENTS.md");
    let metadata = fs::symlink_metadata(&agents_path).expect("AGENTS.md metadata");
    assert!(
        metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
        "initial sync should materialize AGENTS.md as a regular file: {}",
        agents_path.display()
    );

    let repeat_sync = run_cli(project.path(), &["sync"]);
    assert!(repeat_sync.status.success(), "{}", stderr(&repeat_sync));

    let metadata = fs::symlink_metadata(&agents_path).expect("AGENTS.md metadata");
    assert!(
        metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
        "repeat sync should not type-churn an existing regular managed output with matching digest: {}",
        agents_path.display()
    );
}

#[test]
fn cli_sync_preserves_restored_user_agents_when_legacy_state_has_no_patch_marker() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    fs::write(
        project.path().join("AGENTS.md"),
        "# Project Agent Guide\n\nKeep this durable repo guidance.\n",
    )
    .expect("seed AGENTS");

    let adopt = run_cli(project.path(), &["sync", "--adopt", "patch"]);
    assert!(adopt.status.success(), "{}", stderr(&adopt));

    let state_path = project.path().join(".metactl/state/codex-cli.json");
    let mut state: Value =
        serde_json::from_str(&fs::read_to_string(&state_path).expect("read state"))
            .expect("parse state");
    let agents_state = state["outputs"]
        .as_array_mut()
        .expect("outputs")
        .iter_mut()
        .find(|output| output["destination_path"] == "AGENTS.md")
        .expect("AGENTS state");
    agents_state["patch_marker"] = Value::Null;
    agents_state["backup_path"] = Value::Null;
    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&state).expect("serialize state"),
    )
    .expect("write legacy state");

    fs::write(
        project.path().join("AGENTS.md"),
        "# Project Agent Guide\n\nKeep this durable repo guidance.\n",
    )
    .expect("restore AGENTS");

    let repeat_sync = run_cli(project.path(), &["sync"]);
    assert!(repeat_sync.status.success(), "{}", stderr(&repeat_sync));

    let agents = fs::read_to_string(project.path().join("AGENTS.md")).expect("read AGENTS");
    assert!(
        agents.contains("Keep this durable repo guidance."),
        "plain sync should preserve restored repo-owned AGENTS.md content: {agents}"
    );
    assert!(
        agents.contains("metactl:begin"),
        "plain sync should add or update the managed block instead of replacing AGENTS.md: {agents}"
    );
}

#[test]
fn cli_sync_preserves_restored_user_claude_when_legacy_state_has_no_patch_marker() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(project.path(), &["init", "--target", "claude-code"]);
    assert!(init.status.success(), "{}", stderr(&init));

    fs::write(
        project.path().join("CLAUDE.md"),
        "# Claude Project Guide\n\nKeep this Claude-specific repo guidance.\n",
    )
    .expect("seed CLAUDE");

    let adopt = run_cli(project.path(), &["sync", "--adopt", "patch"]);
    assert!(adopt.status.success(), "{}", stderr(&adopt));

    let state_path = project.path().join(".metactl/state/claude-code.json");
    let mut state: Value =
        serde_json::from_str(&fs::read_to_string(&state_path).expect("read state"))
            .expect("parse state");
    let claude_state = state["outputs"]
        .as_array_mut()
        .expect("outputs")
        .iter_mut()
        .find(|output| output["destination_path"] == "CLAUDE.md")
        .expect("CLAUDE state");
    claude_state["patch_marker"] = Value::Null;
    claude_state["backup_path"] = Value::Null;
    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&state).expect("serialize state"),
    )
    .expect("write legacy state");

    fs::write(
        project.path().join("CLAUDE.md"),
        "# Claude Project Guide\n\nKeep this Claude-specific repo guidance.\n",
    )
    .expect("restore CLAUDE");

    let repeat_sync = run_cli(project.path(), &["sync"]);
    assert!(repeat_sync.status.success(), "{}", stderr(&repeat_sync));

    let claude = fs::read_to_string(project.path().join("CLAUDE.md")).expect("read CLAUDE");
    assert!(
        claude.contains("Keep this Claude-specific repo guidance."),
        "plain sync should preserve restored repo-owned CLAUDE.md content: {claude}"
    );
    assert!(
        claude.contains("metactl:begin"),
        "plain sync should add or update the managed block instead of replacing CLAUDE.md: {claude}"
    );
}

#[test]
fn cli_sync_adopt_patch_preserves_brownfield_root_docs_across_targets() {
    for (target, doc, sentinel) in [
        ("codex-cli", "AGENTS.md", "codex durable guidance sentinel"),
        (
            "claude-code",
            "CLAUDE.md",
            "claude durable guidance sentinel",
        ),
        (
            "gemini-cli",
            "GEMINI.md",
            "gemini durable guidance sentinel",
        ),
    ] {
        let project = TempDir::new().expect("tempdir");
        let init = run_cli(project.path(), &["init", "--target", target]);
        assert!(init.status.success(), "{}", stderr(&init));

        fs::write(
            project.path().join(doc),
            format!("# Root Instructions\n\n{sentinel}\n\nDo not replace this file.\n"),
        )
        .expect("seed root doc");

        let adopt = run_cli(project.path(), &["sync", "--adopt", "patch"]);
        assert!(
            adopt.status.success(),
            "adopt patch failed for {target}: {}",
            stderr(&adopt)
        );
        let repeat_sync = run_cli(project.path(), &["sync"]);
        assert!(
            repeat_sync.status.success(),
            "repeat sync failed for {target}: {}",
            stderr(&repeat_sync)
        );

        let contents = fs::read_to_string(project.path().join(doc)).expect("read root doc");
        assert!(
            contents.contains(sentinel),
            "{target} repeat sync should preserve repo-owned {doc}: {contents}"
        );
        assert_eq!(
            contents.matches("metactl:begin").count(),
            1,
            "{target} should have exactly one managed block in {doc}: {contents}"
        );
        let metadata = fs::symlink_metadata(project.path().join(doc)).expect("root doc metadata");
        assert!(
            metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
            "{target} root doc should remain a regular file"
        );
    }
}

#[test]
fn cli_sync_creates_policy_report_parent_dir() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let private_dir = project.path().join(".metactl/private");
    if private_dir.exists() {
        fs::remove_dir_all(&private_dir).expect("remove private dir");
    }

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    assert!(
        private_dir.join("codex-cli-policy-report.json").exists(),
        "sync should recreate .metactl/private before writing policy reports"
    );
}

#[test]
fn cli_sync_brownfield_preview_and_refusal() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    fs::write(project.path().join("AGENTS.md"), "user-owned").expect("seed brownfield file");

    let refused = run_cli(project.path(), &["--json", "sync"]);
    assert_eq!(refused.status.code(), Some(12), "{}", stdout(&refused));
    let refused_json = json_output(&refused);
    assert_eq!(refused_json["ok"], false);
    assert!(refused_json["next_steps"]
        .as_array()
        .expect("next steps")
        .iter()
        .any(|item| item.as_str() == Some("metactl sync --adopt preview")));
    assert_eq!(
        fs::read_to_string(project.path().join("AGENTS.md")).expect("read AGENTS"),
        "user-owned"
    );

    let preview = run_cli(project.path(), &["--json", "sync", "--adopt", "preview"]);
    assert!(preview.status.success(), "{}", stderr(&preview));
    let preview_json = json_output(&preview);
    assert_eq!(preview_json["targets"][0]["status"], "preview");
    assert_eq!(preview_json["preview"], true);
    assert_eq!(
        fs::read_to_string(project.path().join("AGENTS.md")).expect("read AGENTS"),
        "user-owned"
    );
}

#[test]
fn cli_sync_brownfield_refusal_includes_playbook() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    fs::write(project.path().join("AGENTS.md"), "user-owned").expect("seed brownfield file");

    let refused = run_cli(project.path(), &["--json", "sync"]);
    assert_eq!(refused.status.code(), Some(12), "{}", stdout(&refused));
    let refused_json = json_output(&refused);
    assert_eq!(refused_json["ok"], false);

    // Check that the error includes the playbook
    assert!(
        refused_json.get("playbook").is_some(),
        "playbook field should be in JSON output"
    );
    let playbook = refused_json["playbook"]
        .as_str()
        .expect("playbook should be a string");

    // Verify the playbook contains key elements
    assert!(
        playbook.contains("Brownfield adoption strategy"),
        "playbook should mention Brownfield adoption strategy"
    );
    assert!(
        playbook.contains("preview"),
        "playbook should mention preview step"
    );
    assert!(
        playbook.contains("patch"),
        "playbook should mention patch step"
    );
    assert!(
        playbook.contains("takeover"),
        "playbook should mention takeover option"
    );

    // Verify that JSON output doesn't contain ANSI escape codes
    // ANSI escape sequences start with \x1b[
    assert!(
        !playbook.contains("\x1b["),
        "JSON playbook output should not contain ANSI escape codes"
    );
    assert!(
        !playbook.contains("\\x1b["),
        "JSON playbook output should not contain escaped ANSI codes"
    );

    // Verify that next_steps array is still present for backward compatibility
    assert!(refused_json["next_steps"]
        .as_array()
        .expect("next steps")
        .iter()
        .any(|item| item.as_str() == Some("metactl sync --adopt preview")));
}

#[test]
fn cli_sync_patch_adopts_identical_unmanaged_skill_outputs() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let compile = run_cli(project.path(), &["compile"]);
    assert!(compile.status.success(), "{}", stderr(&compile));

    let skill_path = ".codex/skills/python-refactor/python-refactor/SKILL.md";
    let staged = project
        .path()
        .join(".metactl/generated/codex-cli")
        .join(skill_path);
    let destination = project.path().join(skill_path);
    fs::create_dir_all(destination.parent().expect("skill parent")).expect("skill dir");
    fs::copy(&staged, &destination).expect("seed identical unmanaged skill");

    let sync = run_cli(project.path(), &["sync", "--adopt", "patch"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let state = fs::read_to_string(project.path().join(".metactl/state/managed_files.json"))
        .expect("managed state");
    assert!(
        state.contains(skill_path),
        "identical skill output should be adopted into managed state: {state}"
    );
}

#[test]
fn cli_sync_patch_backs_up_conflicting_unmanaged_skill_outputs() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let skill_path = ".codex/skills/python-refactor/python-refactor/SKILL.md";
    let destination = project.path().join(skill_path);
    fs::create_dir_all(destination.parent().expect("skill parent")).expect("skill dir");
    fs::write(&destination, "local unmanaged skill body").expect("seed conflicting skill");
    fs::write(
        project.path().join("AGENTS.md"),
        "# Agents\n\nUNMANAGED_FOLDER_SENTINEL\n",
    )
    .expect("seed AGENTS");

    let sync = run_cli(project.path(), &["sync", "--adopt", "patch"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let agents = fs::read_to_string(project.path().join("AGENTS.md")).expect("read AGENTS");
    assert!(
        agents.contains("UNMANAGED_FOLDER_SENTINEL"),
        "patch adoption should preserve root doc guidance: {agents}"
    );
    assert!(
        agents.contains("metactl:begin"),
        "patch adoption should add a managed block to AGENTS.md: {agents}"
    );

    let state =
        fs::read_to_string(project.path().join(".metactl/state/codex-cli.json")).expect("state");
    assert!(
        state.contains(skill_path),
        "conflicting skill output should be adopted into target state: {state}"
    );
    assert!(
        state.contains(".metactl/state/backups/codex-cli/"),
        "conflicting skill adoption should record a backup path: {state}"
    );
    assert!(
        !fs::read_to_string(&destination)
            .expect("read adopted skill")
            .contains("local unmanaged skill body"),
        "destination should be replaced by generated skill body after backup"
    );
}

#[test]
fn cli_sync_takeover_unsupported_target_shows_alternative() {
    let project = TempDir::new().expect("tempdir");
    // Init with claude-code target (reference-based, doesn't support takeover)
    let output = run_cli(project.path(), &["init", "--target", "claude-code"]);
    assert!(output.status.success(), "{}", stderr(&output));

    // Create unmanaged AGENTS.md to trigger brownfield mode
    fs::write(project.path().join("AGENTS.md"), "user-owned").expect("seed brownfield file");

    // Try sync --adopt takeover
    let refused = run_cli(project.path(), &["--json", "sync", "--adopt", "takeover"]);
    // Should fail with exit code 10 (state error) because takeover is not supported for claude-code
    assert_eq!(
        refused.status.code(),
        Some(10),
        "exit code for unsupported takeover should be 10 (state error): {}",
        stdout(&refused)
    );
    let refused_json = json_output(&refused);
    assert_eq!(refused_json["ok"], false);

    // Check that the error message contains helpful information
    let error_str = stdout(&refused);
    assert!(
        error_str.contains("does not support takeover") || error_str.contains("reference-based"),
        "error should mention takeover not being supported: {}",
        error_str
    );
    assert!(
        error_str.contains("patch")
            || error_str.contains("apply")
            || error_str.contains("Brownfield adoption strategy"),
        "error should suggest patch mode or show playbook: {}",
        error_str
    );
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
fn cli_init_all_targets_preset_expands_supported_targets() {
    let project = TempDir::new().expect("tempdir");
    let init = run_cli(project.path(), &["--json", "init", "--target", "all"]);
    assert!(init.status.success(), "{}", stderr(&init));
    let json = json_output(&init);
    let targets = json["targets"].as_array().expect("targets");
    assert!(targets.iter().any(|item| item == "claude-code"));
    assert!(targets.iter().any(|item| item == "codex-cli"));
    assert!(targets.iter().any(|item| item == "cursor"));
    assert!(targets.iter().any(|item| item == "gemini-cli"));
    assert!(targets.iter().any(|item| item == "openclaw"));

    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("read config");
    assert!(config.contains("- claude-code"));
    assert!(config.contains("- codex-cli"));
    assert!(config.contains("- cursor"));
    assert!(config.contains("- gemini-cli"));
    assert!(config.contains("- openclaw"));
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
fn cli_init_human_output_explains_machine_default_binding_choice() {
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
        &["init"],
        &[("HOME", home.path().to_str().expect("home path"))],
    );
    assert!(init.status.success(), "{}", stderr(&init));
    let text = stdout(&init);
    assert!(text.contains("Applied machine default profile from user settings locally"));
    assert!(text.contains("Leave it this way for a portable repo"));
    assert!(text.contains("metactl init --bind-profile"));
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
fn cli_check_strict_reports_warn_ignore_and_superseded_knowledge_sources() {
    let project = TempDir::new().expect("tempdir");
    let custom = TempDir::new().expect("custom library");
    let ks_dir = custom.path().join("knowledge_sources");
    fs::create_dir_all(&ks_dir).expect("knowledge dir");

    let write_source = |file_name: &str, manifest: Value| {
        fs::write(
            ks_dir.join(file_name),
            serde_json::to_string_pretty(&manifest).expect("knowledge json"),
        )
        .expect("write knowledge source");
    };
    let source = |id: &str,
                  freshness_policy: &str,
                  review_status: &str,
                  superseded_by: Vec<&str>| {
        json!({
            "kind": "knowledge_source",
            "id": id,
            "version": "1.0.0",
            "title": id,
            "source_kind": "filesystem_markdown",
            "uri_scheme": "file",
            "allowed_targets": ["codex-cli"],
            "byte_budget": {"max_search_bytes": 4096, "max_read_bytes": 4096, "max_search_results": 5},
            "trust_tier": "org_validated",
            "freshness": {
                "owner": "fixtures",
                "last_verified": "2000-01-01T00:00:00Z",
                "expires_after_days": 1,
                "source_digests": ["sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],
                "freshness_policy": freshness_policy,
                "review_status": review_status,
                "superseded_by": superseded_by
            },
            "operations": {
                "search": {"enabled": true, "max_bytes": 4096, "max_results": 5},
                "read": {"enabled": true, "max_bytes": 4096, "max_results": 1},
                "freshness": {"enabled": true, "max_bytes": 1024, "max_results": 1},
                "propose_update": {"enabled": false, "mode": "request_only"}
            },
            "adapter": {"base_path": "docs", "allowed_uri_prefixes": ["file:docs/"]}
        })
    };
    write_source(
        "expired-warn.json",
        source("expired-warn", "warn", "active", Vec::new()),
    );
    write_source(
        "expired-ignore.json",
        source("expired-ignore", "ignore", "active", Vec::new()),
    );
    let mut superseded_source = source(
        "superseded-source",
        "warn",
        "superseded",
        vec!["knowledge_source:current-source"],
    );
    superseded_source["freshness"]["last_verified"] = json!("2999-01-01T00:00:00Z");
    write_source("superseded-source.json", superseded_source);

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
    assert!(check.status.success(), "{}", stderr(&check));
    let check_json = json_output(&check);
    let freshness = check_json["freshness"]
        .as_array()
        .expect("freshness findings");
    let by_id = |id: &str| {
        freshness
            .iter()
            .find(|item| item["id"] == id)
            .unwrap_or_else(|| panic!("missing freshness finding for {id}: {freshness:?}"))
    };

    let warn = by_id("expired-warn");
    assert_eq!(warn["status"], json!("warn"));
    assert_eq!(warn["code"], json!("METACTL_KS_EXPIRED_WARN"));
    assert_eq!(
        warn["source_digests"][0],
        json!("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
    assert_eq!(warn["trust_tier"], json!("org_validated"));

    let ignored = by_id("expired-ignore");
    assert_eq!(ignored["status"], json!("ignored"));
    assert_eq!(ignored["code"], json!("METACTL_KS_EXPIRED_IGNORE"));
    assert_eq!(ignored["freshness_policy"], json!("ignore"));

    let superseded = by_id("superseded-source");
    assert_eq!(superseded["status"], json!("warn"));
    assert_eq!(superseded["code"], json!("METACTL_KS_SUPERSEDED"));
    assert_eq!(
        superseded["superseded_by"][0],
        json!("knowledge_source:current-source")
    );
}

#[test]
fn cli_v1_user_private_library_project_link_sync_and_check_flow() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(
        project.path(),
        &["--json", "library", "init", "--user", "--profile", "solo"],
    );
    assert!(init.status.success(), "{}", stderr(&init));
    let init_json = json_output(&init);
    assert_json_contract(&init_json, "library", Some(project.path()));
    let library_root = PathBuf::from(init_json["library_root"].as_str().expect("library root"));
    assert!(library_root.join("packs").is_dir());
    assert!(PathBuf::from(init_json["profile_path"].as_str().expect("profile path")).exists());

    let link = run_cli(
        project.path(),
        &["--json", "project", "link", "--profile", "solo"],
    );
    assert!(link.status.success(), "{}", stderr(&link));
    let link_json = json_output(&link);
    assert_json_contract(&link_json, "project", Some(project.path()));
    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("config");
    assert!(config.contains("extends_profile: solo"), "{config}");

    let preview = run_cli(
        project.path(),
        &["--json", "sync", "--target", "codex,claude", "--preview"],
    );
    assert!(preview.status.success(), "{}", stderr(&preview));
    let preview_json = json_output(&preview);
    assert_json_contract(&preview_json, "sync", Some(project.path()));
    assert_eq!(preview_json["preview"], json!(true));
    assert!(!project.path().join("AGENTS.md").exists());

    let apply = run_cli(
        project.path(),
        &["--json", "sync", "--target", "codex,claude", "--apply"],
    );
    assert!(apply.status.success(), "{}", stderr(&apply));
    assert!(project.path().join("AGENTS.md").exists());
    assert!(project.path().join("CLAUDE.md").exists());

    let check = run_cli(project.path(), &["--json", "check", "--strict"]);
    assert!(check.status.success(), "{}", stderr(&check));
    let check_json = json_output(&check);
    assert_json_contract(&check_json, "validate", Some(project.path()));
    assert_eq!(check_json["strict"], json!(true));
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
fn cli_status_human_output_explains_machine_default_binding_choice() {
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
        &["init"],
        &[("HOME", home.path().to_str().expect("home path"))],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    let status = run_cli_env(
        project.path(),
        &["status"],
        &[("HOME", home.path().to_str().expect("home path"))],
    );
    assert!(status.status.success(), "{}", stderr(&status));
    let text = stdout(&status);
    assert!(text.contains("Machine default profile team-profile is active locally"));
    assert!(text.contains("metactl init --bind-profile"));
}

#[test]
fn cli_status_reports_discoverability_blockers_before_sync() {
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

    let status = run_cli_env(
        project.path(),
        &["--json", "status"],
        &[("HOME", home.path().to_str().expect("home"))],
    );
    assert!(status.status.success(), "{}", stderr(&status));
    let json = json_output(&status);
    assert_eq!(json["execution_readiness"], "blocked");
    assert!(json["blocking_checks"]
        .as_array()
        .expect("blocking_checks")
        .iter()
        .any(|item| {
            item["id"] == "target-discovery"
                && item["missing_targets"]
                    .as_array()
                    .map(|targets| targets.iter().any(|target| target == "made-up-target"))
                    .unwrap_or(false)
        }));

    let status_human = run_cli_env(
        project.path(),
        &["status"],
        &[("HOME", home.path().to_str().expect("home"))],
    );
    assert!(status_human.status.success(), "{}", stderr(&status_human));
    let text = stdout(&status_human);
    assert!(text.contains("Execution readiness: blocked"), "{text}");
    assert!(text.contains("configured target made-up-target"), "{text}");
    assert!(text.contains("Next: metactl doctor"), "{text}");
    assert!(!text.contains("Next: metactl sync"), "{text}");
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
fn cli_sync_failure_reports_effective_library_roots_and_fix_hint() {
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

    let sync = run_cli_env(
        project.path(),
        &["--json", "sync"],
        &[("HOME", home.path().to_str().expect("home"))],
    );
    assert_eq!(sync.status.code(), Some(10), "{}", stdout(&sync));
    let json = json_output(&sync);
    assert_eq!(json["reason_code"], "target_discovery_blocked");
    assert!(json["effective_library_roots"]
        .as_array()
        .expect("effective_library_roots")
        .iter()
        .any(|item| item.as_str() == Some(custom_library.path().to_str().expect("custom root"))));
    assert!(json["suggested_actions"]
        .as_array()
        .expect("suggested_actions")
        .iter()
        .any(|item| {
            item.as_str()
                .unwrap_or_default()
                .contains("add a library root that contains targets")
        }));

    let text = json["message"].as_str().unwrap_or_default();
    assert!(text.contains("effective library roots"), "{text}");
    assert!(text.contains("made-up-target"), "{text}");
    assert!(text.contains("metactl doctor"), "{text}");
}

#[test]
fn cli_user_settings_respect_xdg_config_home() {
    let project = TempDir::new().expect("tempdir");
    let xdg = TempDir::new().expect("xdg");
    let metactl_root = xdg.path().join("metactl");
    let profiles = metactl_root.join("profiles");
    fs::create_dir_all(&profiles).expect("profiles");
    let starter = starter_library_root();
    fs::write(
        profiles.join("xdg-p.yaml"),
        format!("targets:\n  - codex-cli\nstarter_library:\n  - {starter}\n"),
    )
    .expect("profile");
    fs::write(metactl_root.join("config.yaml"), "default_profile: xdg-p\n").expect("settings");
    let home = TempDir::new().expect("home");
    let init = run_cli_env(
        project.path(),
        &["--json", "init"],
        &[
            ("XDG_CONFIG_HOME", xdg.path().to_str().expect("xdg")),
            ("HOME", home.path().to_str().expect("home")),
        ],
    );
    assert!(init.status.success(), "{}", stderr(&init));
    let init_json = json_output(&init);
    assert_eq!(
        init_json["profile_resolution"]["activation_source"],
        json!("user_default")
    );
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

#[test]
fn cli_stateful_human_output_reports_project_root() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let compile = run_cli(project.path(), &["compile"]);
    assert!(compile.status.success(), "{}", stderr(&compile));
    assert!(stdout(&compile).starts_with(&format!("Project: {}", project.path().display())));

    let apply = run_cli(project.path(), &["apply", "--mode", "copy"]);
    assert!(apply.status.success(), "{}", stderr(&apply));
    assert!(stdout(&apply).starts_with(&format!("Project: {}", project.path().display())));

    let validate = run_cli(project.path(), &["validate"]);
    assert!(validate.status.success(), "{}", stderr(&validate));
    assert!(stdout(&validate).starts_with(&format!("Project: {}", project.path().display())));

    let doctor = run_cli(project.path(), &["doctor"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    assert!(stdout(&doctor).starts_with(&format!("Project: {}", project.path().display())));

    let revert = run_cli(project.path(), &["revert", "--all"]);
    assert!(revert.status.success(), "{}", stderr(&revert));
    assert!(stdout(&revert).starts_with(&format!("Project: {}", project.path().display())));
}

#[test]
fn cli_target_list_add_and_remove_updates_config() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let list = run_cli(project.path(), &["--json", "target", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let list_json = json_output(&list);
    assert!(list_json["items"]
        .as_array()
        .expect("items")
        .iter()
        .any(|item| item["id"] == "codex-cli" && item["configured"] == true));

    let add = run_cli(
        project.path(),
        &["--json", "target", "add", "openclaw", "claude-code"],
    );
    assert!(add.status.success(), "{}", stderr(&add));
    let add_json = json_output(&add);
    assert!(add_json["added"]
        .as_array()
        .expect("added")
        .iter()
        .any(|item| item == "openclaw"));
    assert!(add_json["added"]
        .as_array()
        .expect("added")
        .iter()
        .any(|item| item == "claude-code"));

    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("read config");
    assert!(config.contains("openclaw"));
    assert!(config.contains("claude-code"));

    let remove = run_cli(
        project.path(),
        &["--json", "target", "remove", "claude-code"],
    );
    assert!(remove.status.success(), "{}", stderr(&remove));
    let remove_json = json_output(&remove);
    assert!(remove_json["removed"]
        .as_array()
        .expect("removed")
        .iter()
        .any(|item| item == "claude-code"));

    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("read config");
    assert!(!config.contains("claude-code"));
    assert!(config.contains("openclaw"));
}

#[test]
fn cli_brownfield_safety_refuses_unmanaged_collisions() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    fs::write(project.path().join("AGENTS.md"), "user-owned").expect("seed brownfield file");

    let compile = run_cli(project.path(), &["compile"]);
    assert!(compile.status.success(), "{}", stderr(&compile));

    let apply = run_cli(
        project.path(),
        &["--json", "apply", "--mode", "copy", "--no-input"],
    );
    assert_eq!(apply.status.code(), Some(12), "{}", stdout(&apply));
    let json = json_output(&apply);
    assert_eq!(json["ok"], false);
    assert!(json["details"]
        .as_array()
        .expect("details")
        .iter()
        .any(|item| item
            .as_str()
            .unwrap_or_default()
            .contains("Unmanaged destination exists")));
    assert_eq!(
        fs::read_to_string(project.path().join("AGENTS.md")).expect("read AGENTS"),
        "user-owned"
    );
}

#[test]
fn cli_non_interactive_json_and_no_input_are_machine_safe() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    fs::write(project.path().join("AGENTS.md"), "user-owned").expect("seed AGENTS");
    let compile = run_cli(project.path(), &["compile"]);
    assert!(compile.status.success(), "{}", stderr(&compile));

    let apply = run_cli(project.path(), &["--json", "--no-input", "apply"]);
    assert_eq!(apply.status.code(), Some(12));
    let json = json_output(&apply);
    assert_eq!(json["ok"], false);
    assert!(stdout(&apply).contains("\"message\""));
}

#[test]
fn cli_explain_verbose_shows_surface_detail() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let explain = run_cli(project.path(), &["--json", "--verbose", "explain"]);
    assert!(explain.status.success(), "{}", stderr(&explain));
    let json = json_output(&explain);
    let surface_details = json["surface_details"]
        .as_array()
        .expect("surface_details array");
    assert!(surface_details
        .iter()
        .any(|item| item["pack_ref"]["id"] == "python-refactor"));
    assert!(surface_details.iter().any(|item| {
        item["pack_ref"]["id"] == "python-refactor"
            && item["surfaces"]
                .as_array()
                .map(|surfaces| {
                    surfaces.iter().any(|surface| {
                        surface["surface_slug"] == "contracts"
                            && surface["emitted"] == false
                            && surface["reason_code"] == "suppressed_by_mode"
                    })
                })
                .unwrap_or(false)
    }));
}

#[test]
fn cli_compile_surface_mode_controls_emitted_skill_surfaces() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let minimal = run_cli(project.path(), &["--json", "compile"]);
    assert!(minimal.status.success(), "{}", stderr(&minimal));
    let minimal_manifest: Value = serde_json::from_slice(
        &fs::read(
            project
                .path()
                .join(".metactl/generated/codex-cli/compile.manifest.json"),
        )
        .expect("read minimal compile manifest"),
    )
    .expect("decode minimal compile manifest");

    assert_eq!(minimal_manifest["surface_selection_mode"], "minimal");
    assert!(minimal_manifest["surface_selection"]
        .as_array()
        .expect("surface_selection")
        .iter()
        .any(|item| {
            item["pack_ref"]["id"] == "python-refactor"
                && item["surface_slug"] == "contracts"
                && item["emitted"] == false
                && item["reason_code"] == "suppressed_by_mode"
        }));
    assert!(
        !project
            .path()
            .join(".metactl/generated/codex-cli/.codex/skills/python-refactor/contracts/SKILL.md")
            .exists(),
        "contracts surface should be suppressed in minimal mode"
    );

    let full = run_cli(
        project.path(),
        &["--json", "compile", "--surface-mode", "full"],
    );
    assert!(full.status.success(), "{}", stderr(&full));
    let full_manifest: Value = serde_json::from_slice(
        &fs::read(
            project
                .path()
                .join(".metactl/generated/codex-cli/compile.manifest.json"),
        )
        .expect("read full compile manifest"),
    )
    .expect("decode full compile manifest");

    assert_eq!(full_manifest["surface_selection_mode"], "full");
    assert!(full_manifest["surface_selection"]
        .as_array()
        .expect("surface_selection")
        .iter()
        .any(|item| {
            item["pack_ref"]["id"] == "python-refactor"
                && item["surface_slug"] == "contracts"
                && item["emitted"] == true
        }));
    assert!(
        project
            .path()
            .join(".metactl/generated/codex-cli/.codex/skills/python-refactor/contracts/SKILL.md")
            .exists(),
        "contracts surface should emit in full mode"
    );
}

#[test]
fn cli_config_default_surface_mode_controls_compile_explain_and_status() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let config_path = project.path().join("metactl.yaml");
    let config = fs::read_to_string(&config_path).expect("read config");
    fs::write(
        &config_path,
        config.replace(
            "  discovery_mode: candidate_search\n",
            "  discovery_mode: candidate_search\n  surface_selection_mode: full\n",
        ),
    )
    .expect("write config");

    let explain = run_cli(project.path(), &["--json", "explain"]);
    assert!(explain.status.success(), "{}", stderr(&explain));
    let explain_json = json_output(&explain);
    assert_eq!(
        explain_json["target_projection"]["surface_selection_mode"],
        "full"
    );

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    let sync_text = stdout(&sync);
    assert!(
        sync_text.contains("surface: full"),
        "sync output should expose surface mode:\n{sync_text}"
    );

    let status = run_cli(project.path(), &["--json", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_json = json_output(&status);
    assert_eq!(
        status_json["applied_targets"][0]["surface_selection_mode"],
        "full"
    );
    assert_eq!(
        status_json["applied_targets"][0]["configured_surface_selection_mode"],
        "full"
    );
    assert_eq!(
        status_json["applied_targets"][0]["surface_selection_mode_matches_config"],
        true
    );
    assert_eq!(
        status_json["surface_mode_mismatches"]
            .as_array()
            .expect("surface_mode_mismatches")
            .len(),
        0
    );
    assert_eq!(status_json["needs_sync"], false);

    let manifest: Value = serde_json::from_slice(
        &fs::read(
            project
                .path()
                .join(".metactl/generated/codex-cli/compile.manifest.json"),
        )
        .expect("read compile manifest"),
    )
    .expect("decode compile manifest");
    assert_eq!(manifest["surface_selection_mode"], "full");
}

#[test]
fn cli_status_flags_transient_surface_mode_mismatch() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let sync = run_cli(project.path(), &["sync", "--surface-mode", "full"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let status = run_cli(project.path(), &["--json", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_json = json_output(&status);
    assert_eq!(status_json["lock_stale"], false);
    assert_eq!(status_json["needs_sync"], true);
    assert_eq!(
        status_json["applied_targets"][0]["surface_selection_mode"],
        "full"
    );
    assert_eq!(
        status_json["applied_targets"][0]["configured_surface_selection_mode"],
        "minimal"
    );
    assert_eq!(
        status_json["applied_targets"][0]["surface_selection_mode_matches_config"],
        false
    );
    assert_eq!(
        status_json["surface_mode_mismatches"][0]["target"],
        "codex-cli"
    );

    let human = run_cli(project.path(), &["status"]);
    assert!(human.status.success(), "{}", stderr(&human));
    let text = stdout(&human);
    assert!(
        text.contains("surface: full, next sync: minimal"),
        "status output should explain the applied/configured surface-mode mismatch:\n{text}"
    );
}

#[test]
fn cli_explain_reports_emitted_and_suppressed_surfaces() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let explain = run_cli(project.path(), &["--json", "explain"]);
    assert!(explain.status.success(), "{}", stderr(&explain));
    let json = json_output(&explain);

    let python_refactor = json["surface_details"]
        .as_array()
        .expect("surface_details array")
        .iter()
        .find(|item| item["pack_ref"]["id"] == "python-refactor")
        .expect("python-refactor surface detail");
    let surfaces = python_refactor["surfaces"].as_array().expect("surfaces");

    assert!(surfaces.iter().any(|surface| {
        surface["surface_slug"] == "python-refactor" && surface["emitted"] == true
    }));
    assert!(surfaces.iter().any(|surface| {
        surface["surface_slug"] == "contracts"
            && surface["emitted"] == false
            && surface["reason_code"] == "suppressed_by_mode"
    }));

    let human = run_cli(project.path(), &["explain"]);
    assert!(human.status.success(), "{}", stderr(&human));
    let text = stdout(&human);
    assert!(
        text.contains("Surface selection mode: minimal"),
        "explain output should show effective surface mode:\n{text}"
    );
    assert!(
        text.contains("pack python-refactor: 1 emitted, 2 suppressed"),
        "explain output should distinguish emitted from suppressed surfaces:\n{text}"
    );
}

#[test]
fn cli_explain_reports_target_projection() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let explain = run_cli(project.path(), &["--json", "explain"]);
    assert!(explain.status.success(), "{}", stderr(&explain));
    let json = json_output(&explain);
    assert_eq!(json["target_projection"]["target_id"], "codex-cli");
    assert!(json["target_projection"]["summary"]
        .as_str()
        .unwrap_or_default()
        .contains("AGENTS.md"));
    assert!(json["target_projection"]["summary"]
        .as_str()
        .unwrap_or_default()
        .contains("skills/{pack_id}/{surface_slug}/SKILL.md"));
    assert_eq!(
        json["target_projection"]["outputs"][0]["instruction_mode"],
        "reference_index"
    );
    assert!(json["target_projection"]["instruction_behavior"]
        .as_str()
        .unwrap_or_default()
        .contains("references emitted pack bodies"));
    assert!(json["target_projection"]["instruction_budget"]
        .as_str()
        .unwrap_or_default()
        .contains("8192"));
    assert!(json["target_projection"]["surface_behavior"]
        .as_str()
        .unwrap_or_default()
        .contains("separate"));

    let human = run_cli(project.path(), &["explain"]);
    assert!(human.status.success(), "{}", stderr(&human));
    assert!(stdout(&human).contains("Projection:"));
    assert!(stdout(&human).contains("references emitted pack bodies"));
}

#[test]
fn cli_explain_reports_reference_instruction_projection() {
    let project = TempDir::new().expect("tempdir");
    let init = run_cli(
        project.path(),
        &[
            "init",
            "--role",
            "reviewer",
            "--policy",
            "safe-review",
            "--target",
            "claude-code",
        ],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    let explain = run_cli(project.path(), &["--json", "explain"]);
    assert!(explain.status.success(), "{}", stderr(&explain));
    let json = json_output(&explain);
    assert_eq!(json["target_projection"]["target_id"], "claude-code");
    assert_eq!(
        json["target_projection"]["outputs"][0]["instruction_mode"],
        "reference_index"
    );
    assert!(json["target_projection"]["instruction_behavior"]
        .as_str()
        .unwrap_or_default()
        .contains("entry document concise"));

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    let claude_md = fs::read_to_string(project.path().join("CLAUDE.md")).expect("CLAUDE.md");
    assert!(claude_md.contains(".claude/skills/unit-test-loop/"));
    assert!(claude_md.contains("Prefer retrieval-led reasoning"));
    assert!(!claude_md.contains("Run the narrowest relevant test loop before closing work."));
}

#[test]
fn cli_explain_reports_instruction_budget_behavior() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let explain = run_cli(project.path(), &["--json", "explain"]);
    assert!(explain.status.success(), "{}", stderr(&explain));
    let json = json_output(&explain);
    assert!(json["target_projection"]["instruction_budget"]
        .as_str()
        .unwrap_or_default()
        .contains("8192"));

    let human = run_cli(project.path(), &["explain"]);
    assert!(human.status.success(), "{}", stderr(&human));
    assert!(stdout(&human).contains("32768"));
}

#[test]
fn cli_target_native_harness_outputs() {
    let project = TempDir::new().expect("tempdir");
    let init = run_cli(
        project.path(),
        &[
            "init",
            "--role",
            "release-manager",
            "--policy",
            "release-policy",
            "--target",
            "codex-cli",
        ],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    // Spec 019 plus Codex command support: codex-cli emits AGENTS.md,
    // .codex/skills/..., and project slash commands under .codex/commands.
    // Other .codex/* paths (rules, plugins, scripts, hooks, config.toml)
    // are not real Codex project surfaces and remain removed.
    assert!(project.path().join("AGENTS.md").exists());
    assert!(project
        .path()
        .join(".codex/skills/unit-test-loop/unit-test-loop/SKILL.md")
        .exists());
    assert!(project
        .path()
        .join(".codex/commands/run-targeted-tests.md")
        .exists());
    assert!(!project.path().join(".codex/config.toml").exists());
    assert!(!project.path().join(".codex/rules").exists());
    assert!(!project.path().join(".codex/plugins").exists());
    assert!(!project.path().join(".codex/scripts").exists());
}

#[test]
fn cli_two_target_workflow_release_gate() {
    let project = TempDir::new().expect("tempdir");
    let init = run_cli(
        project.path(),
        &[
            "init",
            "--role",
            "release-manager",
            "--policy",
            "release-policy",
            "--target",
            "codex-cli",
            "--target",
            "openclaw",
        ],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    let compile = run_cli(project.path(), &["compile"]);
    assert!(compile.status.success(), "{}", stderr(&compile));
    assert!(project
        .path()
        .join(".metactl/generated/codex-cli/AGENTS.md")
        .exists());
    assert!(project
        .path()
        .join(".metactl/generated/openclaw/OPENCLAW.md")
        .exists());

    let apply = run_cli(project.path(), &["apply", "--mode", "copy"]);
    assert!(apply.status.success(), "{}", stderr(&apply));
    assert!(project.path().join("AGENTS.md").exists());
    assert!(project.path().join("OPENCLAW.md").exists());

    let validate = run_cli(project.path(), &["validate"]);
    assert!(validate.status.success(), "{}", stderr(&validate));

    let revert = run_cli(project.path(), &["revert", "--all"]);
    assert!(revert.status.success(), "{}", stderr(&revert));
    assert!(!project.path().join("AGENTS.md").exists());
    assert!(!project.path().join("OPENCLAW.md").exists());
}

#[test]
fn cli_symlink_fallback_copy_mode() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    let compile = run_cli(project.path(), &["compile"]);
    assert!(compile.status.success(), "{}", stderr(&compile));

    let apply = run_cli_env(
        project.path(),
        &["--json", "apply", "--mode", "symlink"],
        &[("METACTL_FORCE_NO_SYMLINK", "1")],
    );
    assert!(apply.status.success(), "{}", stderr(&apply));
    let json = json_output(&apply);
    assert_eq!(json["targets"][0]["apply_mode"], "copy");
    assert!(json["notes"][0]
        .as_str()
        .unwrap_or_default()
        .contains("fell back to copy"));
}

#[test]
fn cli_add_pack_updates_config_and_validates() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    // Add a pack
    let add = run_cli(project.path(), &["--json", "add", "unit-test-loop"]);
    assert!(add.status.success(), "{}", stderr(&add));
    let json = json_output(&add);
    assert_json_contract(&json, "add", Some(project.path()));
    assert!(json["added"]
        .as_array()
        .expect("added array")
        .iter()
        .any(|item| item == "unit-test-loop"));

    // Verify config was updated
    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("read config");
    assert!(config.contains("unit-test-loop"));

    // Add same pack again — should report already configured
    let add_again = run_cli(project.path(), &["--json", "add", "unit-test-loop"]);
    assert!(add_again.status.success(), "{}", stderr(&add_again));
    let json_again = json_output(&add_again);
    assert!(json_again["already_configured"]
        .as_array()
        .expect("already_configured")
        .iter()
        .any(|item| item == "unit-test-loop"));
    assert!(json_again["added"].as_array().expect("added").is_empty());
}

#[test]
fn cli_add_nonexistent_pack_fails() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let add = run_cli(
        project.path(),
        &["--json", "add", "this-pack-does-not-exist"],
    );
    assert_eq!(add.status.code(), Some(10), "{}", stdout(&add));
    let json = json_output(&add);
    assert_eq!(json["ok"], false);
    assert!(json["message"]
        .as_str()
        .unwrap_or_default()
        .contains("not found"));
}

#[test]
fn cli_remove_pack_updates_config() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    // Add then remove
    let add = run_cli(project.path(), &["add", "unit-test-loop"]);
    assert!(add.status.success(), "{}", stderr(&add));

    let remove = run_cli(project.path(), &["--json", "remove", "unit-test-loop"]);
    assert!(remove.status.success(), "{}", stderr(&remove));
    let json = json_output(&remove);
    assert_json_contract(&json, "remove", Some(project.path()));
    assert!(json["removed"]
        .as_array()
        .expect("removed")
        .iter()
        .any(|item| item == "unit-test-loop"));

    // Verify config was updated
    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("read config");
    assert!(!config.contains("unit-test-loop"));
}

#[test]
fn cli_status_shows_project_state() {
    let project = TempDir::new().expect("tempdir");

    // Status before init
    let status_before = run_cli(project.path(), &["--json", "status"]);
    assert!(status_before.status.success(), "{}", stderr(&status_before));
    let json_before = json_output(&status_before);
    assert_json_contract(&json_before, "status", Some(project.path()));
    assert_eq!(json_before["initialized"], false);

    // Status after init
    init_project(project.path());
    let status_after = run_cli(project.path(), &["--json", "status"]);
    assert!(status_after.status.success(), "{}", stderr(&status_after));
    let json_after = json_output(&status_after);
    assert_eq!(json_after["initialized"], true);
    assert!(json_after["role"].is_string());
    assert!(json_after["targets"].is_array());
    assert_eq!(json_after["needs_sync"], true);

    // Status after sync
    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    let status_synced = run_cli(project.path(), &["--json", "status"]);
    assert!(status_synced.status.success(), "{}", stderr(&status_synced));
    let json_synced = json_output(&status_synced);
    assert_eq!(json_synced["lock_stale"], false);
    assert!(!json_synced["applied_targets"]
        .as_array()
        .expect("applied_targets")
        .is_empty());
    assert_eq!(
        json_synced["applied_targets"][0]["surface_selection_mode"],
        "minimal"
    );
    assert!(
        json_synced["applied_targets"][0]["generated_outputs"]
            .as_u64()
            .expect("generated_outputs")
            > 0
    );
}

#[test]
fn cli_add_with_sync_compiles_and_applies() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let add = run_cli(
        project.path(),
        &["--json", "add", "unit-test-loop", "--sync"],
    );
    assert!(add.status.success(), "{}", stderr(&add));
    let json = json_output(&add);
    assert_json_contract(&json, "add", Some(project.path()));
    assert!(json.get("sync").is_some());
    assert_eq!(json["sync"]["ok"], true);

    // Verify the agent artifacts were actually applied
    assert!(project.path().join("AGENTS.md").exists());
}

#[test]
fn cli_add_already_configured_with_sync_still_runs_sync() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let first = run_cli(
        project.path(),
        &["--json", "add", "unit-test-loop", "--sync"],
    );
    assert!(first.status.success(), "{}", stderr(&first));

    let second = run_cli(
        project.path(),
        &["--json", "add", "unit-test-loop", "--sync"],
    );
    assert!(second.status.success(), "{}", stderr(&second));
    let json = json_output(&second);
    assert_json_contract(&json, "add", Some(project.path()));
    assert!(json["added"].as_array().expect("added").is_empty());
    assert!(json["already_configured"]
        .as_array()
        .expect("already_configured")
        .iter()
        .any(|item| item == "unit-test-loop"));
    assert!(json.get("sync").is_some());
    assert_eq!(json["sync"]["ok"], true);
    assert!(project.path().join("AGENTS.md").exists());
}

#[test]
fn cli_init_shows_next_steps() {
    let project = TempDir::new().expect("tempdir");
    let output = run_cli(project.path(), &["init", "-t", "claude-code"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("Next steps:"));
    assert!(text.contains("metactl use"));
    assert!(text.contains("metactl sync"));
    assert!(text.contains("Role:"));
    assert!(text.contains("Targets:"));
}

#[test]
fn cli_init_warns_when_replacing_existing_config() {
    let project = TempDir::new().expect("tempdir");
    assert!(run_cli(project.path(), &["init", "--target", "codex-cli"])
        .status
        .success());

    let again_human = run_cli(project.path(), &["init", "--target", "codex-cli"]);
    assert!(again_human.status.success(), "{}", stderr(&again_human));
    let text = stdout(&again_human);
    assert!(
        text.contains("Warning:") && text.contains("already existed") && text.contains("replaced"),
        "expected re-init warning in stdout: {text}"
    );

    let again_json = run_cli(project.path(), &["--json", "init", "--target", "codex-cli"]);
    assert!(again_json.status.success(), "{}", stderr(&again_json));
    let json = json_output(&again_json);
    assert_eq!(json.get("reinitialized"), Some(&Value::Bool(true)));
}

#[test]
fn init_refuses_without_target_when_no_surfaces_detected() {
    let project = TempDir::new().expect("tempdir");
    // Empty directory: no existing surfaces, no --target, no profile
    let output = run_cli(project.path(), &["init"]);
    assert!(
        !output.status.success(),
        "init should fail without target in empty dir"
    );
    let text = stderr(&output);
    assert!(
        text.contains("No target specified") || text.contains("Available targets"),
        "expected diagnostic about missing target: {text}"
    );
}

#[test]
fn init_refuses_without_target_json_output() {
    let project = TempDir::new().expect("tempdir");
    let output = run_cli(project.path(), &["--json", "init"]);
    assert!(!output.status.success());
    let json = json_output(&output);
    assert_eq!(json["ok"], false);
}

// ========================================================================
// 018 — Open standard local layers and provenance
// ========================================================================

#[test]
fn local_config_layer_additive_packs_and_staleness() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    // Write a local config that adds a pack
    fs::write(
        project.path().join("metactl.local.yaml"),
        "packs:\n  - unit-test-loop\n",
    )
    .expect("write local config");

    // Status should show the project is initialized
    let status = run_cli(project.path(), &["--json", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json = json_output(&status);
    assert_eq!(json["initialized"], true);

    // Sync should succeed with the local pack included
    let sync = run_cli(project.path(), &["--json", "sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    // After sync, the lock should include the local config digest
    let lock_raw = fs::read_to_string(project.path().join("metactl.lock.json")).expect("read lock");
    let lock_json: Value = serde_json::from_str(&lock_raw).expect("parse lock");
    assert!(
        lock_json.get("local_config_digest").is_some(),
        "lock should contain local_config_digest"
    );

    // Modify the local config → lock should become stale
    fs::write(
        project.path().join("metactl.local.yaml"),
        "packs:\n  - unit-test-loop\nrole: reviewer\n",
    )
    .expect("rewrite local config");

    let compile_stale = run_cli(project.path(), &["compile"]);
    assert_eq!(
        compile_stale.status.code(),
        Some(11),
        "expected stale lock after local config change: {}",
        stdout(&compile_stale)
    );
}

#[test]
fn local_config_layer_gitignored() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let gitignore = fs::read_to_string(project.path().join(".gitignore")).expect("read .gitignore");
    assert!(
        gitignore.contains("metactl.local.yaml"),
        "gitignore should contain metactl.local.yaml entry"
    );
}

#[test]
fn status_shows_provenance_layers_and_stale_reason() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    // Sync so we have locked targets
    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    // Status should include layers in JSON
    let status = run_cli(project.path(), &["--json", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json = json_output(&status);
    assert_eq!(json["initialized"], true);

    // Must have layers array with at least the shared layer
    let layers = json["layers"].as_array().expect("layers array");
    assert!(
        layers.iter().any(|l| l["layer"] == "shared"),
        "should have shared layer: {:?}",
        layers
    );

    // Stale reason should be null when not stale
    assert!(
        json["stale_reason"].is_null(),
        "stale_reason should be null when lock is fresh"
    );

    // Human output should include Layers section
    let status_human = run_cli(project.path(), &["status"]);
    assert!(status_human.status.success(), "{}", stderr(&status_human));
    let text = stdout(&status_human);
    assert!(text.contains("Layers:"), "human output should show layers");

    // Now make it stale by editing config
    let config = std::fs::read_to_string(project.path().join("metactl.yaml")).expect("read config");
    std::fs::write(
        project.path().join("metactl.yaml"),
        format!("{}\n# modified\n", config),
    )
    .expect("modify config");

    let status2 = run_cli(project.path(), &["--json", "status"]);
    assert!(status2.status.success(), "{}", stderr(&status2));
    let json2 = json_output(&status2);
    assert_eq!(json2["lock_stale"], true);
    assert_eq!(json2["stale_reason"], "config changed");
}

#[test]
fn status_shows_local_config_layer() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    // Add local config
    fs::write(
        project.path().join("metactl.local.yaml"),
        "packs:\n  - unit-test-loop\n",
    )
    .expect("write local config");

    let status = run_cli(project.path(), &["--json", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json = json_output(&status);
    let layers = json["layers"].as_array().expect("layers array");
    assert!(
        layers.iter().any(|l| l["layer"] == "local"),
        "should have local layer when metactl.local.yaml exists: {:?}",
        layers
    );
}

#[test]
fn porcelain_use_resolves_and_syncs() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    // Use a known pack
    let use_out = run_cli(project.path(), &["--json", "use", "unit-test-loop"]);
    assert!(use_out.status.success(), "{}", stderr(&use_out));
    let json = json_output(&use_out);
    assert_json_contract(&json, "use", Some(project.path()));
    assert_eq!(json["resolved_pack"], "unit-test-loop");

    // Verify pack was added to config
    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("read config");
    assert!(config.contains("unit-test-loop"));

    // Verify sync ran (artifacts exist)
    assert!(project.path().join("AGENTS.md").exists());
}

#[test]
fn porcelain_use_local_adds_to_local_config() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let use_out = run_cli(
        project.path(),
        &["--json", "use", "unit-test-loop", "--local"],
    );
    assert!(use_out.status.success(), "{}", stderr(&use_out));
    let json = json_output(&use_out);
    assert_eq!(json["local"], true);

    // Verify pack was added to local config, not shared
    let config = fs::read_to_string(project.path().join("metactl.yaml")).expect("read config");
    assert!(
        !config.contains("unit-test-loop"),
        "shared config should not contain the local pack"
    );

    let local_config =
        fs::read_to_string(project.path().join("metactl.local.yaml")).expect("read local config");
    assert!(
        local_config.contains("unit-test-loop"),
        "local config should contain the pack"
    );
}

#[test]
fn porcelain_use_no_match_fails() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let use_out = run_cli(
        project.path(),
        &["--json", "use", "nonexistent-pack-xyz-999"],
    );
    assert!(
        !use_out.status.success(),
        "expected failure for nonexistent pack"
    );
    let json = json_output(&use_out);
    assert_eq!(json["ok"], false);
}

#[test]
fn provenance_ledger_in_status_output() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let status = run_cli(project.path(), &["--json", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json = json_output(&status);

    // Should reflect initialized project with targets and packs
    assert_eq!(json["initialized"], true, "status should show initialized");
    assert!(
        json["targets"].as_array().is_some(),
        "status JSON should contain targets array"
    );
}

#[test]
fn provenance_ledger_with_local_config() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    // Add local config
    fs::write(
        project.path().join("metactl.local.yaml"),
        "packs:\n  - unit-test-loop\n",
    )
    .expect("write local config");

    let status = run_cli(project.path(), &["--json", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json = json_output(&status);
    assert_eq!(json["initialized"], true, "status should show initialized");
    // The project should still report packs from the merged config
    assert!(
        json["packs"].as_array().is_some(),
        "status JSON should contain packs array"
    );
}

#[test]
fn hook_install_creates_git_hooks() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    // Initialize a git repo
    let git_init = std::process::Command::new("git")
        .args(["init"])
        .current_dir(project.path())
        .output()
        .expect("git init");
    assert!(git_init.status.success(), "git init failed");

    let install = run_cli(project.path(), &["--json", "hook", "install"]);
    assert!(install.status.success(), "{}", stderr(&install));
    let json = json_output(&install);
    assert_json_contract(&json, "hook", Some(project.path()));

    // Verify hooks were created
    assert!(
        project.path().join(".git/hooks/post-checkout").exists(),
        "post-checkout hook should exist"
    );
    assert!(
        project.path().join(".git/hooks/post-merge").exists(),
        "post-merge hook should exist"
    );

    // Verify hooks contain metactl references and hardened behavior
    let post_checkout = fs::read_to_string(project.path().join(".git/hooks/post-checkout"))
        .expect("read post-checkout");
    assert!(post_checkout.contains("metactl"));
    // Verify detached HEAD guard
    assert!(
        post_checkout.contains("symbolic-ref"),
        "hook should check for detached HEAD"
    );
    // Verify HEAD@{1} guard
    assert!(
        post_checkout.contains("rev-parse --verify HEAD@{1}"),
        "hook should verify HEAD@{{1}} exists before diffing"
    );
    // Verify metactl-on-PATH check
    assert!(
        post_checkout.contains("not found on PATH"),
        "hook should warn when metactl not on PATH"
    );
    // Verify lock file is watched
    assert!(
        post_checkout.contains("metactl.lock.json"),
        "hook should watch metactl.lock.json"
    );
}

#[test]
fn hook_status_reports_installed_hooks() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let git_init = std::process::Command::new("git")
        .args(["init"])
        .current_dir(project.path())
        .output()
        .expect("git init");
    assert!(git_init.status.success());

    // Before install
    let status_before = run_cli(project.path(), &["--json", "hook", "status"]);
    assert!(status_before.status.success(), "{}", stderr(&status_before));
    let json_before = json_output(&status_before);
    let hooks_before = json_before["hooks"].as_array().expect("hooks array");
    assert!(hooks_before
        .iter()
        .all(|h| h["has_metactl"] == false || h["exists"] == false));

    // Install
    let install = run_cli(project.path(), &["hook", "install"]);
    assert!(install.status.success(), "{}", stderr(&install));

    // After install
    let status_after = run_cli(project.path(), &["--json", "hook", "status"]);
    assert!(status_after.status.success(), "{}", stderr(&status_after));
    let json_after = json_output(&status_after);
    let hooks_after = json_after["hooks"].as_array().expect("hooks array");
    assert!(hooks_after
        .iter()
        .any(|h| h["hook"] == "post-checkout" && h["has_metactl"] == true));
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
fn add_missing_pack_suggests_search_and_nearest_matches() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let human = run_cli(project.path(), &["add", "python-refctor"]);
    assert_eq!(human.status.code(), Some(10), "{}", stderr(&human));
    let human_stderr = stderr(&human);
    assert!(human_stderr.contains("Did you mean:"));
    assert!(human_stderr.contains("python-refactor"));
    assert!(human_stderr.contains("Next: metactl list packs"));
    assert!(human_stderr.contains("Next: metactl search python-refctor"));
    assert!(human_stderr.contains("Available pack count:"));
    assert!(!human_stderr.contains("agent-candidate-library-installer"));

    let json = run_cli(project.path(), &["--json", "add", "python-refctor"]);
    assert_eq!(json.status.code(), Some(10), "{}", stderr(&json));
    let payload = json_output(&json);
    assert_eq!(payload["not_found"][0], "python-refctor");
    assert!(payload["suggestions"]
        .as_array()
        .expect("suggestions")
        .iter()
        .any(|item| item == "python-refactor"));
    assert!(payload["available_packs"]
        .as_array()
        .expect("available_packs")
        .iter()
        .any(|item| item == "agent-candidate-library-installer"));
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

#[test]
fn doctor_reports_local_config_and_projection_checks() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let doctor = run_cli(project.path(), &["--json", "doctor"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let json = json_output(&doctor);
    let checks = json["checks"].as_array().expect("checks array");

    // Should have local-config check
    assert!(
        checks.iter().any(|c| c["id"] == "local-config"),
        "doctor should include local-config check: {:?}",
        checks.iter().map(|c| &c["id"]).collect::<Vec<_>>()
    );

    // Should have input-provenance check
    assert!(
        checks.iter().any(|c| c["id"] == "input-provenance"),
        "doctor should include input-provenance check"
    );
}

#[test]
fn explain_includes_certificates() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let explain = run_cli(project.path(), &["--json", "explain"]);
    assert!(explain.status.success(), "{}", stderr(&explain));
    let json = json_output(&explain);

    let certs = json["certificates"].as_array();
    assert!(
        certs.is_some(),
        "explain JSON should contain certificates array"
    );
    let certs = certs.unwrap();
    assert!(
        !certs.is_empty(),
        "should have at least one explanation certificate"
    );
    // Verify certificate structure
    let cert = &certs[0];
    assert!(cert.get("subject").is_some(), "certificate needs subject");
    assert!(cert.get("premises").is_some(), "certificate needs premises");
    assert!(cert.get("evidence").is_some(), "certificate needs evidence");
    assert!(
        cert.get("conclusion").is_some(),
        "certificate needs conclusion"
    );
}

#[test]
fn init_detect_existing_surfaces() {
    let project = TempDir::new().expect("tempdir");

    // Seed existing agent surfaces
    fs::write(project.path().join("CLAUDE.md"), "# Claude instructions").expect("seed CLAUDE.md");
    fs::create_dir_all(project.path().join(".cursor/rules")).expect("create .cursor/rules");

    // Init with --detect should pick up both surfaces
    let init = run_cli(project.path(), &["--json", "init", "--detect"]);
    assert!(init.status.success(), "{}", stderr(&init));
    let json = json_output(&init);
    let targets = json["targets"].as_array().expect("targets");

    assert!(
        targets.iter().any(|t| t == "claude-code"),
        "should detect claude-code from CLAUDE.md"
    );
    assert!(
        targets.iter().any(|t| t == "cursor"),
        "should detect cursor from .cursor/"
    );
}

#[test]
fn init_auto_detects_surfaces_when_no_target_specified() {
    let project = TempDir::new().expect("tempdir");
    fs::write(project.path().join("CLAUDE.md"), "# Claude").expect("seed");

    let init = run_cli(project.path(), &["--json", "init"]);
    assert!(init.status.success(), "{}", stderr(&init));
    let json = json_output(&init);
    let targets = json["targets"].as_array().expect("targets");

    // Should detect claude-code from the existing CLAUDE.md
    assert!(
        targets.iter().any(|t| t == "claude-code"),
        "should auto-detect claude-code: {:?}",
        targets
    );
}

#[test]
fn target_local_projection_metadata_in_library() {
    // Verify that target JSON files include local_projection metadata
    let starter_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../library/starter");

    let claude_raw = fs::read(starter_root.join("targets/claude-code.json")).expect("read claude");
    let claude: Value = serde_json::from_slice(&claude_raw).expect("parse claude");
    assert_eq!(
        claude["local_projection"]["support"], "exact",
        "claude-code should have exact local projection support"
    );
    assert!(
        claude["local_projection"]["local_surface"]
            .as_str()
            .unwrap_or_default()
            .contains("CLAUDE.local.md"),
        "claude-code local surface should be CLAUDE.local.md"
    );

    let codex_raw = fs::read(starter_root.join("targets/codex-cli.json")).expect("read codex");
    let codex: Value = serde_json::from_slice(&codex_raw).expect("parse codex");
    assert_eq!(
        codex["local_projection"]["support"], "degraded",
        "codex-cli should have degraded local projection support"
    );

    let cursor_raw = fs::read(starter_root.join("targets/cursor.json")).expect("read cursor");
    let cursor: Value = serde_json::from_slice(&cursor_raw).expect("parse cursor");
    assert_eq!(
        cursor["local_projection"]["support"], "exact",
        "cursor should have exact local projection support"
    );
}

// ========================================================================
// Dogfood readiness — Cursor local projection validation
// ========================================================================

#[test]
fn cursor_compile_produces_expected_output_paths() {
    let project = TempDir::new().expect("tempdir");

    // Init with both cursor and codex-cli targets (packs are codex-cli compatible)
    let init = run_cli(
        project.path(),
        &["init", "--target", "cursor", "--target", "codex-cli"],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    // Add a pack directly (use would resolve against target compatibility)
    let add_out = run_cli(project.path(), &["add", "python-refactor"]);
    assert!(add_out.status.success(), "{}", stderr(&add_out));

    // Compile should produce cursor-specific outputs
    let compile = run_cli(project.path(), &["--json", "compile"]);
    assert!(compile.status.success(), "{}", stderr(&compile));

    // Verify the cursor generated directory exists
    let generated_dir = project.path().join(".metactl/generated/cursor");
    assert!(
        generated_dir.exists(),
        "cursor generated directory should exist"
    );

    // The cursor target uses .cursor/rules/metactl-pack-index.mdc as the index path
    let index_path = generated_dir.join(".cursor/rules/metactl-pack-index.mdc");
    assert!(
        index_path.exists(),
        "cursor pack index should exist at .cursor/rules/metactl-pack-index.mdc, found: {:?}",
        list_tree(&generated_dir)
    );
}

#[test]
fn cursor_pack_index_is_regular_file_when_default_symlink_apply() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(project.path(), &["init", "--target", "cursor"]);
    assert!(init.status.success(), "{}", stderr(&init));

    let sync = run_cli(project.path(), &["sync", "--no-input", "-y"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let index_path = project.path().join(".cursor/rules/metactl-pack-index.mdc");
    assert!(
        index_path.exists(),
        "cursor pack index should exist at repo root .cursor/rules/"
    );
    assert!(
        !index_path.is_symlink(),
        "cursor pack index must be a regular file (not a symlink) for reliable Cursor rule loading"
    );
}

fn list_tree(dir: &Path) -> Vec<String> {
    let mut paths = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                for sub in list_tree(&path) {
                    paths.push(sub);
                }
            } else {
                paths.push(path.to_string_lossy().to_string());
            }
        }
    }
    paths
}

#[test]
fn cursor_target_projection_in_status() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(project.path(), &["init", "--target", "cursor"]);
    assert!(init.status.success(), "{}", stderr(&init));

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let status = run_cli(project.path(), &["--json", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json = json_output(&status);

    let applied = json["applied_targets"].as_array().expect("applied_targets");
    let cursor_target = applied
        .iter()
        .find(|t| t["target"] == "cursor")
        .expect("should have cursor in applied targets");

    assert_eq!(
        cursor_target["projection"], "exact",
        "cursor should report exact projection in status"
    );
}

#[test]
fn cli_status_reports_shared_agents_md_owner() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(
        project.path(),
        &["init", "--target", "codex-cli", "--target", "cursor"],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    let status = run_cli(project.path(), &["--json", "status"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let json = json_output(&status);
    let rules = json["shared_surface_rules"]
        .as_array()
        .expect("shared_surface_rules");
    let agents_rule = rules
        .iter()
        .find(|rule| rule["path"] == "AGENTS.md")
        .expect("AGENTS.md shared-surface rule");

    assert_eq!(agents_rule["owner"], "codex-cli");
    assert!(
        agents_rule["suppressed_targets"]
            .as_array()
            .expect("suppressed_targets")
            .iter()
            .any(|target| target == "cursor"),
        "cursor should be listed as a suppressed secondary target: {:?}",
        agents_rule
    );
}

#[test]
fn cli_sync_multi_target_shared_agents_md_uses_single_owner() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(
        project.path(),
        &["init", "--target", "codex-cli", "--target", "cursor"],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    assert!(
        project.path().join("AGENTS.md").exists(),
        "codex-cli should still own the root AGENTS.md"
    );
    assert!(
        project
            .path()
            .join(".cursor/rules/metactl-pack-index.mdc")
            .exists(),
        "cursor should still emit its target-local rule index"
    );
    assert!(
        !project
            .path()
            .join(".metactl/generated/cursor/AGENTS.md")
            .exists(),
        "cursor should not stage a duplicate root AGENTS.md when codex-cli is enabled"
    );
}

#[test]
fn cli_sync_multi_target_root_instruction_outputs_are_regular_files() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(
        project.path(),
        &[
            "init",
            "--target",
            "codex-cli",
            "--target",
            "claude-code",
            "--target",
            "gemini-cli",
        ],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    let sync = run_cli(project.path(), &["sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    for file in ["AGENTS.md", "CLAUDE.md", "GEMINI.md"] {
        let path = project.path().join(file);
        let metadata = fs::symlink_metadata(&path).unwrap_or_else(|err| {
            panic!("read metadata for {}: {}", path.display(), err);
        });
        assert!(
            metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
            "{file} should be a regular file under default symlink-capable sync"
        );
    }
}

#[test]
fn cli_compile_with_all_flag() {
    let project = TempDir::new().expect("tempdir");

    // Initialize with multiple targets
    let init = run_cli(
        project.path(),
        &["init", "--target", "claude-code", "--target", "cursor"],
    );
    assert!(init.status.success(), "{}", stderr(&init));

    // Compile using --all flag
    let compile = run_cli(project.path(), &["--json", "compile", "--all"]);
    assert!(compile.status.success(), "{}", stderr(&compile));
    let compile_json = json_output(&compile);
    assert_json_contract(&compile_json, "compile", Some(project.path()));

    // Verify both targets were compiled
    let compiled = compile_json["targets"]
        .as_array()
        .expect("targets should be an array");
    assert!(
        compiled.len() >= 2,
        "should have at least 2 compiled targets, got: {:?}",
        compiled
    );

    let target_names: Vec<String> = compiled
        .iter()
        .map(|t| {
            t["target"]
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_default()
        })
        .collect();
    assert!(
        target_names.contains(&"claude-code".to_string()),
        "claude-code should be in compiled targets: {:?}",
        target_names
    );
    assert!(
        target_names.contains(&"cursor".to_string()),
        "cursor should be in compiled targets: {:?}",
        target_names
    );
}

#[test]
fn cli_compile_bad_target_shows_available() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    // Try to compile with unknown target
    let output = run_cli(project.path(), &["compile", "--target", "unknown-target"]);
    assert!(
        !output.status.success(),
        "compile with bad target should fail"
    );

    let error_text = stderr(&output);
    assert!(
        error_text.contains("Available targets"),
        "error should mention available targets: {}",
        error_text
    );
    assert!(
        error_text.contains("codex-cli"),
        "error should list codex-cli: {}",
        error_text
    );
}

#[test]
fn cli_compile_accepts_target_aliases() {
    let project = TempDir::new().expect("tempdir");
    // Initialize project with claude-code target
    let init = run_cli(project.path(), &["init", "--target", "claude-code"]);
    assert!(init.status.success(), "{}", stderr(&init));

    // Compile with "claude" alias instead of "claude-code"
    let output = run_cli(project.path(), &["compile", "--target", "claude"]);
    assert!(output.status.success(), "{}", stderr(&output));

    // Verify compiled file exists for the canonical target name
    assert!(project
        .path()
        .join(".metactl/generated/claude-code/CLAUDE.md")
        .exists());

    // Verify stderr contains the alias resolution note
    let err_text = stderr(&output);
    assert!(
        err_text.contains("resolved target alias"),
        "stderr should mention alias resolution: {}",
        err_text
    );
    assert!(
        err_text.contains("claude"),
        "stderr should mention 'claude' alias: {}",
        err_text
    );
    assert!(
        err_text.contains("claude-code"),
        "stderr should mention 'claude-code' canonical name: {}",
        err_text
    );
}

#[test]
fn cli_doctor_detects_brownfield_and_suggests_preview() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    // Create unmanaged AGENTS.md file to simulate brownfield state
    let agents_path = project.path().join("AGENTS.md");
    fs::write(&agents_path, "# Unmanaged AGENTS.md\n").expect("write AGENTS.md");

    // Run doctor with unmanaged files present
    let doctor = run_cli(project.path(), &["--json", "doctor"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let json = json_output(&doctor);
    let checks = json["checks"].as_array().expect("checks array");

    // Verify brownfield detection check is present
    let brownfield_check = checks.iter().find(|c| c["id"] == "brownfield-detection");
    assert!(
        brownfield_check.is_some(),
        "doctor should include brownfield-detection check"
    );

    let brownfield = brownfield_check.unwrap();
    assert_eq!(
        brownfield["status"], "warn",
        "brownfield should be a warning"
    );

    // Verify the message mentions the detected files
    let message = brownfield["message"].as_str().expect("message");
    assert!(
        message.contains("AGENTS.md"),
        "message should mention AGENTS.md: {}",
        message
    );

    // Verify the message suggests preview mode
    assert!(
        message.contains("preview"),
        "message should mention 'preview': {}",
        message
    );

    // Verify exit code is still 0 (warning, not error)
    assert_eq!(
        doctor.status.code(),
        Some(0),
        "doctor should exit with code 0"
    );

    // Verify human output also contains the hint
    let doctor_human = run_cli(project.path(), &["doctor"]);
    let human_text = stdout(&doctor_human);
    assert!(
        human_text.contains("brownfield-detection") || human_text.contains("AGENTS.md"),
        "human output should mention brownfield: {}",
        human_text
    );
}

#[test]
fn cli_doctor_ignores_brownfield_files_once_managed() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let sync = run_cli(project.path(), &["sync", "--adopt", "patch", "--yes"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let doctor = run_cli(project.path(), &["--json", "doctor"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let json = json_output(&doctor);
    let checks = json["checks"].as_array().expect("checks array");

    assert!(
        checks.iter().all(|c| c["id"] != "brownfield-detection"),
        "doctor should not report brownfield-detection after managed sync: {:?}",
        checks
    );
}

#[test]
fn cli_sync_claude_settings_is_regular_file_not_symlink() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(project.path(), &["init", "--target", "claude-code"]);
    assert!(init.status.success(), "{}", stderr(&init));

    let sync = run_cli(project.path(), &["sync", "--yes"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let settings_path = project.path().join(".claude/settings.json");
    assert!(
        settings_path.exists(),
        "Claude settings should exist after sync"
    );
    assert!(
        !settings_path.is_symlink(),
        "shared Claude settings should be materialized as a regular file, not a symlink"
    );
}

#[test]
fn cli_sync_recreates_missing_managed_claude_settings() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(project.path(), &["init", "--target", "claude-code"]);
    assert!(init.status.success(), "{}", stderr(&init));

    let first_sync = run_cli(project.path(), &["sync", "--yes"]);
    assert!(first_sync.status.success(), "{}", stderr(&first_sync));

    let settings_path = project.path().join(".claude/settings.json");
    fs::remove_file(&settings_path).expect("remove managed settings.json");

    let second_sync = run_cli(project.path(), &["sync", "--yes"]);
    assert!(second_sync.status.success(), "{}", stderr(&second_sync));
    assert!(
        settings_path.exists(),
        "missing managed Claude settings should be recreated"
    );
    assert!(
        !settings_path.is_symlink(),
        "recreated Claude settings should remain a regular file"
    );
}

#[test]
fn cli_sync_claude_settings_patch_preserves_user_hooks_and_settings() {
    let project = TempDir::new().expect("tempdir");

    let init = run_cli(project.path(), &["init", "--target", "claude-code"]);
    assert!(init.status.success(), "{}", stderr(&init));

    let add = run_cli(project.path(), &["add", "migration-guard"]);
    assert!(add.status.success(), "{}", stderr(&add));

    let claude_dir = project.path().join(".claude");
    fs::create_dir_all(&claude_dir).expect("create .claude dir");
    fs::write(
        claude_dir.join("settings.json"),
        r#"{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "command": "env OPENDREAM_WORKSPACE=\"$CLAUDE_PROJECT_DIR\" sh \"$CLAUDE_PROJECT_DIR\"/.opendream/hooks/claude-post-task.sh",
            "type": "command"
          }
        ]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "command": "env OPENDREAM_WORKSPACE=\"$CLAUDE_PROJECT_DIR\" sh \"$CLAUDE_PROJECT_DIR\"/.opendream/hooks/claude-pre-task.sh",
            "type": "command"
          }
        ]
      }
    ]
  },
  "customSetting": "keep-me"
}"#,
    )
    .expect("seed claude settings");

    let sync = run_cli(project.path(), &["sync", "--adopt", "patch", "--yes"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let settings_path = claude_dir.join("settings.json");
    let merged: Value = serde_json::from_str(
        &fs::read_to_string(&settings_path).expect("read merged settings.json"),
    )
    .expect("parse merged settings.json");

    let has_command = |value: &Value, event: &str, command: &str| -> bool {
        value["hooks"][event]
            .as_array()
            .map(|entries| {
                entries.iter().any(|entry| {
                    entry["hooks"].as_array().is_some_and(|hooks| {
                        hooks
                            .iter()
                            .any(|hook| hook["command"].as_str() == Some(command))
                    })
                })
            })
            .unwrap_or(false)
    };

    assert!(
        has_command(
            &merged,
            "Stop",
            "env OPENDREAM_WORKSPACE=\"$CLAUDE_PROJECT_DIR\" sh \"$CLAUDE_PROJECT_DIR\"/.opendream/hooks/claude-post-task.sh"
        ),
        "existing Stop hook should be preserved: {merged:#}"
    );
    assert!(
        has_command(
            &merged,
            "UserPromptSubmit",
            "env OPENDREAM_WORKSPACE=\"$CLAUDE_PROJECT_DIR\" sh \"$CLAUDE_PROJECT_DIR\"/.opendream/hooks/claude-pre-task.sh"
        ),
        "existing UserPromptSubmit hook should be preserved: {merged:#}"
    );
    assert!(
        has_command(
            &merged,
            "PostToolUse",
            ".claude/hooks/migration-guard/hook.sh"
        ),
        "metactl-managed PostToolUse hook should be merged in: {merged:#}"
    );
    assert_eq!(merged["customSetting"], json!("keep-me"));
    assert!(
        merged.get("permissions").is_none(),
        "patch sync should not inject permissions into an existing settings.json: {merged:#}"
    );

    let mut user_edited = merged.clone();
    user_edited["permissions"] = json!({
        "allow": ["Read", "Glob", "Grep", "WebFetch"],
        "ask": ["Write"],
        "deny": ["Bash(rm -rf:*)"]
    });
    user_edited["hooks"]["SessionStart"] = json!([
        {
            "hooks": [
                {
                    "type": "command",
                    "command": "echo session-start"
                }
            ]
        }
    ]);
    fs::write(
        &settings_path,
        serde_json::to_vec_pretty(&user_edited).expect("serialize edited settings"),
    )
    .expect("write edited settings");

    let second_sync = run_cli(project.path(), &["sync", "--yes"]);
    assert!(second_sync.status.success(), "{}", stderr(&second_sync));

    let resynced: Value = serde_json::from_str(
        &fs::read_to_string(&settings_path).expect("read resynced settings.json"),
    )
    .expect("parse resynced settings.json");
    assert!(
        has_command(
            &resynced,
            "Stop",
            "env OPENDREAM_WORKSPACE=\"$CLAUDE_PROJECT_DIR\" sh \"$CLAUDE_PROJECT_DIR\"/.opendream/hooks/claude-post-task.sh"
        ),
        "existing Stop hook should survive re-sync: {resynced:#}"
    );
    assert!(
        has_command(&resynced, "SessionStart", "echo session-start"),
        "user-added SessionStart hook should survive re-sync: {resynced:#}"
    );
    assert!(
        has_command(
            &resynced,
            "PostToolUse",
            ".claude/hooks/migration-guard/hook.sh"
        ),
        "metactl-managed PostToolUse hook should survive re-sync: {resynced:#}"
    );
    assert_eq!(
        resynced["permissions"],
        json!({
            "allow": ["Read", "Glob", "Grep", "WebFetch"],
            "ask": ["Write"],
            "deny": ["Bash(rm -rf:*)"]
        })
    );
}

// ── Visibility scope tests ──────────────────────────────────────────────

fn init_claude_code_project(project: &Path) {
    let output = run_cli(project, &["init", "--target", "claude-code"]);
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn private_pack_excluded_from_committed_claude_md() {
    let project = TempDir::new().expect("tempdir");
    init_claude_code_project(project.path());

    // Add both a shared pack and the private pack
    let add_shared = run_cli(project.path(), &["add", "migration-guard"]);
    assert!(add_shared.status.success(), "{}", stderr(&add_shared));
    let add_private = run_cli(project.path(), &["add", "local-only-example"]);
    assert!(add_private.status.success(), "{}", stderr(&add_private));

    // Compile
    let compile = run_cli(project.path(), &["--json", "compile"]);
    assert!(compile.status.success(), "{}", stderr(&compile));

    // Read the generated CLAUDE.md (committed surface)
    let claude_md = fs::read_to_string(
        project
            .path()
            .join(".metactl/generated/claude-code/CLAUDE.md"),
    )
    .expect("read generated CLAUDE.md");

    // Shared pack should be in the committed index
    assert!(
        claude_md.contains("|pack:migration-guard|"),
        "shared pack should appear in CLAUDE.md: {}",
        claude_md
    );

    // Private pack should NOT be in the committed index
    assert!(
        !claude_md.contains("|pack:local-only-example|"),
        "private pack should NOT appear in CLAUDE.md: {}",
        claude_md
    );
}

#[test]
fn private_pack_emitted_to_local_surface() {
    let project = TempDir::new().expect("tempdir");
    init_claude_code_project(project.path());

    let add = run_cli(project.path(), &["add", "local-only-example"]);
    assert!(add.status.success(), "{}", stderr(&add));

    let compile = run_cli(project.path(), &["--json", "compile"]);
    assert!(compile.status.success(), "{}", stderr(&compile));

    // Local surface (CLAUDE.local.md) should be generated
    let local_md_path = project
        .path()
        .join(".metactl/generated/claude-code/CLAUDE.local.md");
    assert!(
        local_md_path.exists(),
        "CLAUDE.local.md should be generated for private packs, tree: {:?}",
        list_tree(&project.path().join(".metactl/generated/claude-code"))
    );

    let local_md = fs::read_to_string(&local_md_path).expect("read CLAUDE.local.md");
    assert!(
        local_md.contains("|pack:local-only-example|") || local_md.contains("local-only-example"),
        "private pack should appear in CLAUDE.local.md: {}",
        local_md
    );
}

#[test]
fn shared_pack_backward_compat_appears_in_committed_index() {
    let project = TempDir::new().expect("tempdir");
    init_claude_code_project(project.path());

    // migration-guard has no visibility_scope field (defaults to shared)
    let add = run_cli(project.path(), &["add", "migration-guard"]);
    assert!(add.status.success(), "{}", stderr(&add));

    let compile = run_cli(project.path(), &["--json", "compile"]);
    assert!(compile.status.success(), "{}", stderr(&compile));

    let claude_md = fs::read_to_string(
        project
            .path()
            .join(".metactl/generated/claude-code/CLAUDE.md"),
    )
    .expect("read generated CLAUDE.md");

    assert!(
        claude_md.contains("|pack:migration-guard|"),
        "pack without visibility_scope should appear in committed index (backward compat): {}",
        claude_md
    );
}

#[test]
fn private_pack_degradation_when_target_lacks_local_surface() {
    let project = TempDir::new().expect("tempdir");

    // codex-cli has degraded local_projection support
    init_project(project.path());

    let add = run_cli(project.path(), &["add", "local-only-example"]);
    assert!(add.status.success(), "{}", stderr(&add));

    let compile = run_cli(project.path(), &["--json", "compile"]);
    assert!(compile.status.success(), "{}", stderr(&compile));

    let json = json_output(&compile);

    // Check that AGENTS.md does not contain the private pack
    let agents_md = fs::read_to_string(
        project
            .path()
            .join(".metactl/generated/codex-cli/AGENTS.md"),
    )
    .expect("read generated AGENTS.md");
    assert!(
        !agents_md.contains("|pack:local-only-example|"),
        "private pack should NOT appear in codex-cli AGENTS.md: {}",
        agents_md
    );

    // Check for degradation in compile output targets
    let targets = json["targets"].as_array().expect("targets array");
    let codex_target = targets
        .iter()
        .find(|t| t["target"] == "codex-cli")
        .expect("should have codex-cli target");
    let degradations = codex_target["degradations"]
        .as_array()
        .expect("degradations array");
    let has_private_pack_degradation = degradations.iter().any(|d| {
        d["feature"]
            .as_str()
            .map(|f| f.starts_with("private_packs_no_local_surface"))
            .unwrap_or(false)
    });

    assert!(
        has_private_pack_degradation,
        "should have degradation code for private packs on target without local surface.\nDegradations: {:?}",
        degradations
    );
}

// ---------------------------------------------------------------------------
// Spec 019 Task 6.2 — live projection smoke tests
// ---------------------------------------------------------------------------

/// Recursively walk `root`, collecting regular file paths into `out`.
/// Skips `.git/`, `tmp/`, and `target/` subtrees. Follows symlinks that
/// resolve to regular files (metactl sync emits compile outputs as symlinks
/// into `.metactl/generated/<target>/`).
fn walk_project_files(root: &Path, out: &mut Vec<PathBuf>) {
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
            walk_project_files(&path, out);
        } else if file_type.is_file() {
            out.push(path);
        } else if file_type.is_symlink() {
            if path.metadata().map(|m| m.is_file()).unwrap_or(false) {
                out.push(path);
            }
        }
    }
}

#[test]
fn cli_sync_all_targets_no_double_path_segments() {
    for target in [
        "claude-code",
        "cursor",
        "codex-cli",
        "gemini-cli",
        "openclaw",
    ] {
        let project = TempDir::new().expect("tempdir");
        let init = run_cli(
            project.path(),
            &["--no-input", "-y", "init", "--target", target],
        );
        assert!(
            init.status.success(),
            "init --target {} failed: {}",
            target,
            stderr(&init)
        );
        let sync = run_cli(project.path(), &["--no-input", "-y", "sync"]);
        assert!(
            sync.status.success(),
            "sync for {} failed: {}",
            target,
            stderr(&sync)
        );
        let add = run_cli(
            project.path(),
            &["--no-input", "-y", "add", "unit-test-loop", "--sync"],
        );
        assert!(
            add.status.success(),
            "add --sync for {} failed: {}",
            target,
            stderr(&add)
        );

        let mut files = Vec::new();
        walk_project_files(project.path(), &mut files);
        for path in &files {
            let p = path.to_string_lossy().to_string();
            for seg in ["commands", "rules", "scripts", "plugins", "hooks", "skills"] {
                let needle = format!("/{seg}/");
                if let Some(first) = p.find(&needle) {
                    let rest = &p[first + needle.len()..];
                    assert!(
                        !rest.contains(&needle),
                        "target={target} doubled {seg} segment in {p}"
                    );
                }
            }
        }
    }
}

#[test]
fn cli_sync_gemini_produces_extension_bundle() {
    let project = TempDir::new().expect("tempdir");
    let init = run_cli(
        project.path(),
        &["--no-input", "-y", "init", "--target", "gemini-cli"],
    );
    assert!(init.status.success(), "init failed: {}", stderr(&init));
    let sync = run_cli(project.path(), &["--no-input", "-y", "sync"]);
    assert!(sync.status.success(), "sync failed: {}", stderr(&sync));

    let manifest_path = project
        .path()
        .join(".gemini/extensions/python-refactor/gemini-extension.json");
    assert!(
        manifest_path.exists(),
        "extension manifest missing at {}",
        manifest_path.display()
    );
    let parsed: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest"))
            .expect("parse manifest");
    assert_eq!(parsed["name"], "python-refactor");
    assert_eq!(parsed["contextFileName"], "GEMINI.md");

    let context_path = project
        .path()
        .join(".gemini/extensions/python-refactor/GEMINI.md");
    assert!(
        context_path.exists(),
        "GEMINI.md missing at {}",
        context_path.display()
    );

    // Iterate the skills dir and assert at least one SKILL.md exists.
    let skills_root = project
        .path()
        .join(".gemini/extensions/python-refactor/skills");
    assert!(
        skills_root.exists(),
        "skills dir missing at {}",
        skills_root.display()
    );
    let mut skill_files = Vec::new();
    walk_project_files(&skills_root, &mut skill_files);
    let found_skill = skill_files
        .iter()
        .any(|p| p.file_name().and_then(|n| n.to_str()) == Some("SKILL.md"));
    assert!(
        found_skill,
        "no SKILL.md found under {}; files: {:?}",
        skills_root.display(),
        skill_files
    );
}

#[test]
fn cli_demo_create_list_path_and_destroy_lifecycle() {
    let project = TempDir::new().expect("tempdir");
    let demo_home = project.path().join("demo-home");
    let demo_home_str = demo_home.to_string_lossy().to_string();
    let envs = [("METACTL_DEMO_HOME", demo_home_str.as_str())];

    let create = run_cli_env(
        project.path(),
        &[
            "--json",
            "demo",
            "create",
            "--name",
            "alpha",
            "--target",
            "codex-cli",
            "--sync",
        ],
        &envs,
    );
    assert!(
        create.status.success(),
        "create failed: {}",
        stderr(&create)
    );
    let create_json = json_output(&create);
    assert_eq!(create_json["ok"], true);
    assert_eq!(create_json["command"], "demo create");
    let demo_path = PathBuf::from(create_json["path"].as_str().expect("demo path"));
    assert!(demo_path.join(".metactl-demo/manifest.json").exists());
    assert!(demo_path.join("AGENTS.md").exists());
    assert!(demo_path.join("metactl.yaml").exists());
    assert_eq!(create_json["sync_preview"], true);
    assert!(create_json["next_commands"]
        .as_array()
        .expect("next commands")
        .iter()
        .any(|item| item.as_str() == Some("metactl validate")));

    let list = run_cli_env(project.path(), &["--json", "demo", "list"], &envs);
    assert!(list.status.success(), "list failed: {}", stderr(&list));
    let list_json = json_output(&list);
    assert_eq!(list_json["command"], "demo list");
    assert_eq!(list_json["demos"].as_array().expect("demos").len(), 1);
    assert_eq!(list_json["demos"][0]["name"], "alpha");

    let path = run_cli_env(
        project.path(),
        &["--json", "demo", "path", "--name", "alpha"],
        &envs,
    );
    assert!(path.status.success(), "path failed: {}", stderr(&path));
    assert_eq!(
        json_output(&path)["path"],
        Value::String(demo_path.to_string_lossy().to_string())
    );

    let refused = run_cli_env(
        project.path(),
        &["demo", "destroy", "--name", "alpha"],
        &envs,
    );
    assert_eq!(refused.status.code(), Some(12));
    assert!(demo_path.exists());

    let destroy = run_cli_env(
        project.path(),
        &["--json", "--yes", "demo", "destroy", "--name", "alpha"],
        &envs,
    );
    assert!(
        destroy.status.success(),
        "destroy failed: {}",
        stderr(&destroy)
    );
    assert!(!demo_path.exists());
    assert_eq!(json_output(&destroy)["removed"], true);
}

#[test]
fn cli_demo_destroy_refuses_unmanaged_path() {
    let project = TempDir::new().expect("tempdir");
    let unmanaged = project.path().join("not-a-demo");
    fs::create_dir_all(&unmanaged).expect("unmanaged dir");
    fs::write(unmanaged.join("important.txt"), "keep\n").expect("write unmanaged file");
    let unmanaged_str = unmanaged.to_string_lossy().to_string();

    let destroy = run_cli(
        project.path(),
        &["--yes", "demo", "destroy", "--path", unmanaged_str.as_str()],
    );
    assert_eq!(destroy.status.code(), Some(12));
    assert!(unmanaged.join("important.txt").exists());
}
