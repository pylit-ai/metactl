use super::*;

fn write_source_project(project: &Path, include_sources: bool) {
    let sources = if include_sources {
        format!(
            r#"
sources:
- id: public-lib
  type: local
  path: {}
  visibility: public
  lock_publicity: public
- id: private-lib
  type: local
  path: {}
  visibility: private
  lock_publicity: private
"#,
            project.join("public-lib").display(),
            project.join("private-lib").display()
        )
    } else {
        String::new()
    };
    fs::write(
        project.join("metactl.yaml"),
        format!(
            r#"api_version: metactl/v2alpha1
role: reviewer
policy: brownfield-safe-builder
packs:
- wxb-pack-core-quality
targets:
- codex-cli
defaults:
  discovery_mode: candidate_search
metadata:
  agent_artifact_policy: portable-first
{sources}"#
        ),
    )
    .expect("write source metactl.yaml");
}

#[test]
fn project_import_plan_accepts_direct_path_and_omits_sources_by_default() {
    let source = TempDir::new().expect("source");
    let target = TempDir::new().expect("target");
    write_source_project(source.path(), true);

    let output = run_cli(
        target.path(),
        &[
            "--json",
            "project",
            "import",
            "plan",
            source.path().to_str().expect("source path"),
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let json = json_output(&output);
    assert_json_contract(&json, "project import", Some(target.path()));
    assert_eq!(json["action"], "plan");
    assert_eq!(json["source"]["source"], "direct_path");
    assert_eq!(json["mode"], "explicit");
    assert_eq!(json["equivalence"], "source_omitted");
    assert_eq!(json["projected_config"]["role"], "reviewer");
    assert!(json["projected_config"].get("sources").is_none());
    assert!(json["next_commands"][0]
        .as_str()
        .unwrap()
        .contains(source.path().to_str().unwrap()));
    assert!(json["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| { warning["code"] == "source_omitted" }));
}

#[test]
fn project_import_apply_creates_config_and_lock_without_private_sources() {
    let source = TempDir::new().expect("source");
    let target = TempDir::new().expect("target");
    write_source_project(source.path(), true);

    let output = run_cli(
        target.path(),
        &[
            "--json",
            "project",
            "import",
            "apply",
            source.path().to_str().expect("source path"),
            "--yes",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let json = json_output(&output);
    assert_json_contract(&json, "project import", Some(target.path()));
    assert_eq!(json["action"], "apply");
    assert_eq!(json["applied"], true);
    assert_eq!(json["apply_mode"], "create");
    assert_eq!(json["equivalence"], "source_omitted");

    let config = fs::read_to_string(target.path().join("metactl.yaml")).expect("target config");
    assert!(config.contains("role: reviewer"));
    assert!(config.contains("wxb-pack-core-quality"));
    assert!(config.contains("agent_artifact_policy: portable-first"));
    assert!(!config.contains("private-lib"));
    assert!(!config.contains("public-lib"));

    let lock = fs::read_to_string(target.path().join("metactl.lock.json")).expect("lock");
    assert!(lock.contains("config_digest"));
}

#[test]
fn project_import_apply_copies_sources_only_with_explicit_opt_in() {
    let source = TempDir::new().expect("source");
    let target = TempDir::new().expect("target");
    write_source_project(source.path(), true);

    let output = run_cli(
        target.path(),
        &[
            "--json",
            "project",
            "import",
            "apply",
            source.path().to_str().expect("source path"),
            "--include-private-sources",
            "--yes",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let json = json_output(&output);
    assert_json_contract(&json, "project import", Some(target.path()));
    assert_eq!(json["equivalence"], "source_omitted");
    assert!(json["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| warning["code"] == "source_omitted"));

    let config = fs::read_to_string(target.path().join("metactl.yaml")).expect("target config");
    assert!(config.contains("private-lib"));
    assert!(!config.contains("public-lib"));
}

#[test]
fn project_import_apply_copies_public_and_private_sources_when_both_flags_are_set() {
    let source = TempDir::new().expect("source");
    let target = TempDir::new().expect("target");
    write_source_project(source.path(), true);

    let output = run_cli(
        target.path(),
        &[
            "--json",
            "project",
            "import",
            "apply",
            source.path().to_str().expect("source path"),
            "--include-public-sources",
            "--include-private-sources",
            "--yes",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let json = json_output(&output);
    assert_json_contract(&json, "project import", Some(target.path()));
    assert_eq!(json["equivalence"], "equivalent");
    assert!(json["warnings"].as_array().unwrap().is_empty());

    let config = fs::read_to_string(target.path().join("metactl.yaml")).expect("target config");
    assert!(config.contains("public-lib"));
    assert!(config.contains("private-lib"));
}

#[test]
fn project_import_apply_refuses_existing_config_without_merge_or_replace() {
    let source = TempDir::new().expect("source");
    let target = TempDir::new().expect("target");
    write_source_project(source.path(), false);
    fs::write(
        target.path().join("metactl.yaml"),
        "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\n",
    )
    .expect("write existing target config");

    let output = run_cli(
        target.path(),
        &[
            "--json",
            "project",
            "import",
            "apply",
            source.path().to_str().expect("source path"),
            "--yes",
        ],
    );
    assert_eq!(output.status.code(), Some(10), "{}", stdout(&output));
    let json = json_output(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["code"], "existing_config_requires_mode");

    let merge = run_cli(
        target.path(),
        &[
            "--json",
            "project",
            "import",
            "apply",
            source.path().to_str().expect("source path"),
            "--merge",
            "--yes",
        ],
    );
    assert!(merge.status.success(), "{}", stderr(&merge));
    let merge_json = json_output(&merge);
    assert_eq!(merge_json["apply_mode"], "merge");
}

#[test]
fn project_import_apply_replace_overwrites_selected_fields() {
    let source = TempDir::new().expect("source");
    let target = TempDir::new().expect("target");
    write_source_project(source.path(), false);
    fs::write(
        target.path().join("metactl.yaml"),
        "api_version: metactl/v2alpha1\nrole: builder\npolicy: release-policy\npacks:\n- old-pack\ntargets:\n- claude-code\n",
    )
    .expect("write existing target config");

    let output = run_cli(
        target.path(),
        &[
            "--json",
            "project",
            "import",
            "apply",
            source.path().to_str().expect("source path"),
            "--replace",
            "--yes",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let json = json_output(&output);
    assert_json_contract(&json, "project import", Some(target.path()));
    assert_eq!(json["apply_mode"], "replace");

    let config = fs::read_to_string(target.path().join("metactl.yaml")).expect("target config");
    assert!(config.contains("role: reviewer"));
    assert!(config.contains("policy: brownfield-safe-builder"));
    assert!(config.contains("wxb-pack-core-quality"));
    assert!(config.contains("codex-cli"));
    assert!(!config.contains("role: builder"));
    assert!(!config.contains("release-policy"));
    assert!(!config.contains("old-pack"));
    assert!(!config.contains("claude-code"));
}

#[test]
fn setup_import_from_alias_creates_project_config() {
    let source = TempDir::new().expect("source");
    let target = TempDir::new().expect("target");
    write_source_project(source.path(), false);

    let output = run_cli(
        target.path(),
        &[
            "--json",
            "setup",
            "--import-from",
            source.path().to_str().expect("source path"),
            "--yes",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let json = json_output(&output);
    assert_json_contract(&json, "setup", Some(target.path()));
    assert_eq!(json["action"], "apply");

    let config = fs::read_to_string(target.path().join("metactl.yaml")).expect("target config");
    assert!(config.contains("role: reviewer"));
    assert!(config.contains("codex-cli"));
}

#[test]
fn project_import_list_discovers_projects_from_search_root() {
    let root = TempDir::new().expect("root");
    let source = root.path().join("source-app");
    let target = TempDir::new().expect("target");
    fs::create_dir_all(&source).expect("source dir");
    write_source_project(&source, false);

    let output = run_cli(
        target.path(),
        &[
            "--json",
            "project",
            "import",
            "list",
            "--search-root",
            root.path().to_str().expect("root path"),
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let json = json_output(&output);
    assert_json_contract(&json, "project import", Some(target.path()));
    assert_eq!(json["action"], "list");
    assert!(json["projects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|project| { project["name"] == "source-app" && project["source"] == "search_root" }));
}

#[test]
fn project_import_inspect_reports_source_summary() {
    let source = TempDir::new().expect("source");
    let target = TempDir::new().expect("target");
    write_source_project(source.path(), true);

    let output = run_cli(
        target.path(),
        &[
            "--json",
            "project",
            "import",
            "inspect",
            source.path().to_str().expect("source path"),
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let json = json_output(&output);
    assert_json_contract(&json, "project import", Some(target.path()));
    assert_eq!(json["action"], "inspect");
    assert_eq!(json["raw_config"]["role"], "reviewer");
    assert_eq!(json["raw_config"]["sources_count"], 2);
    assert_eq!(json["raw_config"]["private_sources_count"], 1);
}

#[test]
fn project_import_browse_is_rejected_in_agent_safe_mode() {
    let target = TempDir::new().expect("target");

    let output = run_cli(
        target.path(),
        &["--json", "--no-input", "project", "import", "browse"],
    );
    assert_eq!(output.status.code(), Some(10), "{}", stdout(&output));
    let json = json_output(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["code"], "browse_requires_tty");
}

#[test]
fn setup_browse_projects_is_rejected_in_agent_safe_mode() {
    let target = TempDir::new().expect("target");

    let output = run_cli(
        target.path(),
        &["--json", "--no-input", "setup", "--browse-projects"],
    );
    assert_eq!(output.status.code(), Some(10), "{}", stdout(&output));
    let json = json_output(&output);
    assert_eq!(json["ok"], false);
    assert_eq!(json["code"], "browse_requires_tty");
}
