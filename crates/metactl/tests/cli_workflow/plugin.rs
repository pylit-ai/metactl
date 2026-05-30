use super::*;

// Plugin export and pack import workflow tests.

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
