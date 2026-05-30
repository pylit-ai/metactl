use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};
use tempfile::TempDir;

#[path = "cli_workflow/compile.rs"]
mod compile_workflow;
#[path = "cli_workflow/explain_status.rs"]
mod explain_status_workflow;
#[path = "cli_workflow/fleet.rs"]
mod fleet_workflow;
#[path = "cli_workflow/ignore.rs"]
mod ignore_workflow;
#[path = "cli_workflow/plugin.rs"]
mod plugin_workflow;
#[path = "cli_workflow/profile.rs"]
mod profile_workflow;
#[path = "cli_workflow/search.rs"]
mod search_workflow;
#[path = "cli_workflow/source.rs"]
mod source_workflow;
#[path = "cli_workflow/sync.rs"]
mod sync_workflow;

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
fn surface_usage_stats_and_report_are_rebuildable_and_report_only() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    let usage_dir = project.path().join(".metactl/usage");
    fs::create_dir_all(&usage_dir).expect("usage dir");
    fs::write(
        usage_dir.join("events.jsonl"),
        r#"{"event_kind":"command_invoked","pack_id":"python-refactor","recorded_at":"2026-05-21T10:00:00Z"}
{"event_kind":"task_verified","pack_id":"unit-test-loop","recorded_at":"2026-05-21T10:01:00Z"}
"#,
    )
    .expect("events");

    let rebuild = run_cli(project.path(), &["--json", "stats", "rebuild"]);
    assert!(rebuild.status.success(), "{}", stderr(&rebuild));
    let rebuild_json = json_output(&rebuild);
    assert_json_contract(&rebuild_json, "stats", Some(project.path()));
    assert_eq!(rebuild_json["stats"]["event_count"], 2);
    assert!(project.path().join(".metactl/usage/stats.json").exists());

    let report = run_cli(
        project.path(),
        &["--json", "surface", "report", "--scheduled"],
    );
    assert!(report.status.success(), "{}", stderr(&report));
    let report_json = json_output(&report);
    assert_json_contract(&report_json, "surface", Some(project.path()));
    assert_eq!(report_json["report"]["rebuild_trigger"], "scheduled");
    assert_eq!(report_json["report"]["adapter_mutation_allowed"], false);
    let recommendations = report_json["report"]["recommendations"]
        .as_array()
        .expect("recommendations");
    assert!(recommendations.iter().any(|item| {
        item["pack_id"] == "python-refactor"
            && item["tier"] == "warm"
            && item["reason_code"] == "usage_observed_without_verified_outcome"
    }));
    assert!(recommendations
        .iter()
        .any(|item| item["pack_id"] == "unit-test-loop" && item["tier"] == "hot"));
    assert!(project.path().join("reports/surfaces/latest.json").exists());
    assert!(project
        .path()
        .join("docs/status/surfaces/dashboard.md")
        .exists());
}

#[test]
fn background_plan_is_report_only_and_replayable() {
    let project = TempDir::new().expect("tempdir");

    let output = run_cli(
        project.path(),
        &["--json", "background", "plan", "--scope", "project"],
    );
    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
    let value = json_output(&output);
    assert_json_contract(&value, "background", Some(project.path()));
    assert_eq!(value["action"], "plan");
    assert_eq!(value["plan"]["scope"], "project");
    assert_eq!(value["plan"]["report_only"], true);
    assert_eq!(value["plan"]["mutates_adapters"], false);
    assert!(value["plan"]["run_command"]
        .as_str()
        .expect("run command")
        .contains("background run"));
}

#[test]
fn background_install_requires_explicit_confirmation() {
    let project = TempDir::new().expect("tempdir");

    let output = run_cli(
        project.path(),
        &[
            "--json",
            "--no-input",
            "background",
            "install",
            "--scope",
            "project",
        ],
    );
    assert!(!output.status.success());
    let value = json_output(&output);
    assert_eq!(value["code"], "background_confirmation_required");
    assert_eq!(value["category"], "machine_state");
    assert_eq!(value["plan"]["report_only"], true);
}

#[test]
fn setup_plan_recommends_background_refresh_by_default() {
    let project = TempDir::new().expect("tempdir");

    let output = run_cli(
        project.path(),
        &["--json", "setup", "--plan", "--target", "codex-cli"],
    );
    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
    let value = json_output(&output);
    assert_json_contract(&value, "setup", Some(project.path()));
    assert!(value["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .any(|action| action["kind"] == "background-refresh"
            && action["report_only"] == true
            && action["mutates_adapters"] == false));
    assert!(value["next_commands"]
        .as_array()
        .expect("next commands")
        .iter()
        .any(|command| command
            .as_str()
            .unwrap_or_default()
            .contains("background install --scope project --yes")));
}

#[test]
fn setup_plan_can_opt_out_of_background_refresh_guidance() {
    let project = TempDir::new().expect("tempdir");

    let output = run_cli(
        project.path(),
        &[
            "--json",
            "setup",
            "--plan",
            "--target",
            "codex-cli",
            "--no-background",
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        stderr(&output),
        stdout(&output)
    );
    let value = json_output(&output);
    assert!(!value["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .any(|action| action["kind"] == "background-refresh"));
    assert!(!value["next_commands"]
        .as_array()
        .expect("next commands")
        .iter()
        .any(|command| command
            .as_str()
            .unwrap_or_default()
            .contains("background install")));
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
