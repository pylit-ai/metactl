mod support;

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use metactl::skill_card::validate_skill_card;
use serde_json::{json, Value};
use support::{json_output, run_cli, stderr, stdout};
use tempfile::TempDir;

const SOURCE_KINDS: [&str; 8] = [
    "first_party",
    "vendored",
    "imported",
    "local",
    "first_party_adaptation",
    "first_party_reviewed_adaptation",
    "third_party_adaptation",
    "user_provided_research_adaptation",
];

fn card(source_kind: &str) -> Value {
    json!({
        "schema_version": "2alpha1",
        "name": "development-help",
        "version": "1.0.0",
        "summary": "Fix application bugs and review code changes.",
        "aliases": [],
        "intents": {"positive": ["fix an application bug", "add a regression test", "review code changes"], "negative": []},
        "facets": {},
        "reviewed_relations": [],
        "host_compatibility": {"targets": ["codex-cli"]},
        "provenance": {"source_kind": source_kind, "reviewed_by": "fixture-owner", "reviewed_at": "2026-09-25"}
    })
}

#[test]
fn source_kinds_match_schema_and_retain_distinct_card_identity() {
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/schemas/metactl/skill_card.schema.json");
    let schema: Value =
        serde_json::from_slice(&fs::read(schema_path).expect("schema bytes")).expect("schema JSON");
    let schema_kinds = schema["oneOf"][1]["properties"]["provenance"]["properties"]["source_kind"]
        ["enum"]
        .as_array()
        .expect("source-kind enum")
        .iter()
        .map(|value| value.as_str().expect("string source kind"))
        .collect::<BTreeSet<_>>();
    assert_eq!(schema_kinds, BTreeSet::from(SOURCE_KINDS));

    let mut hashes = BTreeSet::new();
    for source_kind in SOURCE_KINDS {
        let card = card(source_kind);
        let original = serde_json::to_vec(&card).expect("original card bytes");
        let result = validate_skill_card(&card).expect(source_kind);
        assert_eq!(result.decision, "accept", "{source_kind}");
        assert!(result.degradations.is_empty(), "{source_kind}");
        assert_eq!(card["provenance"]["source_kind"], source_kind);
        assert_eq!(serde_json::to_vec(&card).unwrap(), original);
        assert!(
            hashes.insert(result.canonical_hash),
            "{source_kind} was collapsed"
        );
    }
    let error = validate_skill_card(&card("unknown_source_kind")).unwrap_err();
    assert_eq!(error.to_string(), "invalid provenance.source_kind");
}

#[test]
fn cli_routes_development_requests_with_each_supported_source_kind() {
    let project = TempDir::new().expect("project");
    let library = TempDir::new().expect("library");
    let starter = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../library/starter");
    let skill_dir = library.path().join("packs/development-help");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: development-help\ndescription: Fix application bugs and review code changes.\n---\nAdd a focused regression test.\n",
    )
    .unwrap();
    let mut pack: Value =
        serde_json::from_slice(&fs::read(starter.join("packs/python-refactor.json")).unwrap())
            .unwrap();
    pack["id"] = json!("development-help");
    pack["resources"] = json!([
        {"path": "packs/development-help/SKILL.md", "kind": "instruction", "required": true},
        {"path": "packs/development-help/skill-card.json", "kind": "example", "required": true}
    ]);
    fs::write(
        library.path().join("packs/development-help.json"),
        serde_json::to_vec(&pack).unwrap(),
    )
    .unwrap();
    let config_path = project.path().join("metactl.yaml");
    let config = format!(
        "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets: [codex-cli]\nstarter_library:\n- {}\n- {}\n",
        starter.display(), library.path().display()
    );
    fs::write(&config_path, &config).unwrap();
    let card_path = skill_dir.join("skill-card.json");
    let args = [
        "--agent",
        "skills",
        "route",
        "Fix a small application bug, add a focused regression test, and review the code changes.",
    ];
    for source_kind in SOURCE_KINDS {
        let bytes = serde_json::to_vec(&card(source_kind)).unwrap();
        fs::write(&card_path, &bytes).unwrap();
        let output = run_cli(project.path(), &args);
        assert!(
            output.status.success(),
            "{source_kind}: {} {}",
            stdout(&output),
            stderr(&output)
        );
        let result = json_output(&output);
        assert_eq!(result["ok"], true);
        assert!(result["result"]["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|candidate| candidate["skill_name"] == "development-help"));
        assert_eq!(fs::read(&card_path).unwrap(), bytes);
        assert_eq!(fs::read_to_string(&config_path).unwrap(), config);
        assert!(!project.path().join("metactl.lock.json").exists());
    }
    fs::write(
        &card_path,
        serde_json::to_vec(&card("unknown_source_kind")).unwrap(),
    )
    .unwrap();
    let output = run_cli(project.path(), &args);
    assert!(!output.status.success());
    let result = json_output(&output);
    assert_eq!(result["ok"], false);
    assert_eq!(
        result["message"],
        "validate declared skill card packs/development-help/skill-card.json"
    );
}
