use super::*;

// Search and recommendation workflow tests.

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
