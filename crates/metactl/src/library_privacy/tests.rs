use super::*;
use std::process::Command;
use tempfile::TempDir;

fn git(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}

fn fixture(custom: bool) -> (TempDir, TempDir, LibraryRegistry, CompileParams) {
    let starter = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../library/starter");
    let library = TempDir::new().unwrap();
    fs::create_dir_all(library.path().join("packs/private-runtime")).unwrap();
    let mut pack: serde_json::Value =
        serde_json::from_slice(&fs::read(starter.join("packs/local-only-example.json")).unwrap())
            .unwrap();
    pack["id"] = "private-runtime".into();
    pack["compatible_targets"] = serde_json::json!(["claude-code", "private-custom"]);
    pack["resources"] = serde_json::json!([
        {"path":"packs/private-runtime/SKILL.md","kind":"instruction","required":true},
        {"path":"packs/private-runtime/command.md","kind":"command","required":true},
        {"path":"packs/private-runtime/hook.sh","kind":"hook","required":true},
        {"path":"packs/private-runtime/wiring.json","kind":"hook_wiring","required":true}
    ]);
    fs::write(
        library.path().join("packs/private-runtime.json"),
        serde_json::to_vec(&pack).unwrap(),
    )
    .unwrap();
    for (name, body) in [
        ("SKILL.md", "# Private runtime\nPRIVATE_SKILL_CANARY"),
        ("command.md", "# Command\nPRIVATE_COMMAND_CANARY"),
        (
            "hook.sh",
            "#!/bin/sh\n# PRIVATE_HOOK_CANARY - never execute\nexit 97\n",
        ),
        (
            "wiring.json",
            r#"{"event":"PostToolUse","matcher":"PRIVATE_MATCHER","command_ref":"packs/private-runtime/hook.sh"}"#,
        ),
    ] {
        fs::write(
            library.path().join("packs/private-runtime").join(name),
            body,
        )
        .unwrap();
    }
    let target_id = if custom {
        "private-custom"
    } else {
        "claude-code"
    };
    if custom {
        let mut target: serde_json::Value =
            serde_json::from_slice(&fs::read(starter.join("targets/claude-code.json")).unwrap())
                .unwrap();
        target["target_id"] = target_id.into();
        target["aliases"] = serde_json::json!([]);
        target["compile_targets"].as_array_mut().unwrap().push(
            serde_json::json!({"output_kind":"mcp_config", "path_template":".custom/mcp.json"}),
        );
        target["runtime_template"]["path"] = "targets/custom.tmpl".into();
        target["runtime_template"]["destination_path"] = ".custom/native.json".into();
        target["local_projection"]["local_surface"] = ".custom/local.md".into();
        fs::create_dir_all(library.path().join("targets")).unwrap();
        fs::write(
            library.path().join("targets/custom.json"),
            serde_json::to_vec(&target).unwrap(),
        )
        .unwrap();
        fs::write(
            library.path().join("targets/custom.tmpl"),
            "{\"hooks\": {{hooks_json}}, \"active_packs\": {{active_packs_json_array}}}",
        )
        .unwrap();
    }
    let registry =
        LibraryRegistry::load_from_roots(&[starter, library.path().to_path_buf()]).unwrap();
    let target = registry.target_by_id(target_id).unwrap();
    let config: Config = serde_json::from_value(serde_json::json!({
        "api_version": crate::types::API_VERSION,
        "role":{"kind":"role","id":"builder","version":"1.0.0"},
        "policy":{"kind":"policy","id":"brownfield-safe-builder","version":"1.0.0"},
        "targets":[target.target_ref()],
        "packs":[{"kind":"pack","id":"private-runtime","version":"1.0.0"}]
    }))
    .unwrap();
    let graph = registry
        .resolve(ResolveParams {
            config,
            overlay: None,
            available_targets: vec![target.clone()],
            provenance: None,
        })
        .unwrap();
    assert!(graph
        .activated_pack_refs
        .iter()
        .any(|p| p.id == "private-runtime"));
    let project = TempDir::new().unwrap();
    assert!(git(project.path(), &["init", "-q"]).status.success());
    let params = CompileParams {
        resolve_graph: graph,
        target_capability: target,
        apply_mode: ApplyMode::Copy,
        surface_selection_mode: None,
        emit_policy_report: false,
        durable_staging: false,
        project_root: Some(project.path().display().to_string()),
    };
    (library, project, registry, params)
}

#[test]
fn private_runtime_preserves_hooks_commands_graph_and_custom_local_surface() {
    for custom in [false, true] {
        let (_library, project, registry, params) = fixture(custom);
        // Real compile path refuses before the first private staging write.
        assert!(registry.compile(params.clone()).is_err());
        assert!(!project.path().join(".metactl").exists());
        fs::write(
            project.path().join(".git/info/exclude"),
            ".metactl/\n.claude/\n.custom/\nCLAUDE.local.md\n",
        )
        .unwrap();
        let manifest = registry.compile(params.clone()).unwrap().compile_manifest;
        let native = if custom {
            ".custom/native.json"
        } else {
            ".claude/settings.json"
        };
        let proofs = registry.projection_proofs(&params).unwrap();
        assert!(proofs.iter().any(|p| p.destination == native && p.private));
        registry
            .apply_manifest(project.path(), &manifest, &ApplyMode::Copy)
            .unwrap();
        let body = fs::read_to_string(project.path().join(native)).unwrap();
        assert!(body.contains("PRIVATE_MATCHER"));
        assert!(body.contains(".claude/hooks/private-runtime/hook.sh"));
        if custom {
            assert!(body.contains("\"private-runtime\""));
            assert!(fs::read_to_string(project.path().join(".custom/mcp.json"))
                .unwrap()
                .contains("private-runtime"));
            assert!(proofs
                .iter()
                .any(|p| p.destination == ".custom/mcp.json" && p.private));
        }
        assert!(
            fs::read_to_string(project.path().join(".claude/hooks/private-runtime/hook.sh"))
                .unwrap()
                .contains("PRIVATE_HOOK_CANARY")
        );
        assert!(fs::read_to_string(
            project
                .path()
                .join(".claude/commands/private-runtime/command.md")
        )
        .unwrap()
        .contains("PRIVATE_COMMAND_CANARY"));
        let local = if custom {
            ".custom/local.md"
        } else {
            "CLAUDE.local.md"
        };
        assert!(fs::read_to_string(project.path().join(local))
            .unwrap()
            .contains("private-runtime"));
        assert!(git(project.path(), &["check-ignore", "-q", native])
            .status
            .success());
        assert!(git(project.path(), &["add", "-A"]).status.success());
        let staged = git(project.path(), &["diff", "--cached"]);
        assert!(!String::from_utf8_lossy(&staged.stdout).contains("private-runtime"));
        assert!(git(project.path(), &["add", "-f", native]).status.success());
        assert!(registry.compile(params.clone()).is_err());
        assert!(registry
            .apply_manifest(project.path(), &manifest, &ApplyMode::Copy)
            .is_err());
        assert!(git(project.path(), &["rm", "--cached", "-f", native])
            .status
            .success());
        // Neither stripped attribution nor a forged visibility map grants sharing.
        let review = registry
            .apply_review_plan(project.path(), &manifest, &ApplyMode::Copy)
            .unwrap();
        let mut altered = manifest.clone();
        for output in &mut altered.generated_outputs {
            output.pack_ref = None;
        }
        fs::write(project.path().join(".git/info/exclude"), ".metactl/\n").unwrap();
        assert!(registry
            .apply_manifest(project.path(), &altered, &ApplyMode::Copy)
            .is_err());
        altered
            .resolve_graph
            .as_mut()
            .unwrap()
            .pack_visibility
            .insert("private-runtime".into(), VisibilityScope::Shared);
        assert!(registry
            .apply_manifest(project.path(), &altered, &ApplyMode::Copy)
            .is_err());
        assert!(registry
            .apply_manifest_bound(project.path(), &altered, &ApplyMode::Copy, &review.digest)
            .is_err());
        assert_eq!(
            fs::read_to_string(project.path().join(native)).unwrap(),
            body
        );
        assert!(registry
            .apply_review_plan(project.path(), &manifest, &ApplyMode::Copy)
            .is_ok());
        assert!(registry
            .protect_private_manifest(project.path(), &manifest, &ApplyMode::Copy)
            .is_err());
        altered.resolve_graph = None;
        assert!(registry
            .apply_manifest(project.path(), &altered, &ApplyMode::Copy)
            .is_err());
    }
}

#[test]
fn private_pack_without_native_contribution_keeps_shared_aggregate_shareable() {
    let (_library, project, mut registry, mut params) = fixture(false);
    // A target with no hook support has no native contribution from this pack.
    registry
        .targets
        .get_mut("claude-code")
        .unwrap()
        .capabilities
        .deterministic_hooks = false;
    params.target_capability = registry.target_by_id("claude-code").unwrap();
    fs::write(
        project.path().join(".git/info/exclude"),
        ".metactl/\n.claude/skills/\n.claude/hooks/\n.claude/commands/\nCLAUDE.local.md\n",
    )
    .unwrap();
    let proofs = registry.projection_proofs(&params).unwrap();
    assert!(proofs
        .iter()
        .any(|p| p.destination == ".claude/settings.json" && !p.private));
    let manifest = registry.compile(params).unwrap().compile_manifest;
    registry
        .apply_manifest(project.path(), &manifest, &ApplyMode::Copy)
        .unwrap();
    assert!(!git(
        project.path(),
        &["check-ignore", "-q", ".claude/settings.json"]
    )
    .status
    .success());
    let mut legacy = manifest.clone();
    legacy.resolve_graph = None;
    assert!(registry
        .apply_manifest(project.path(), &legacy, &ApplyMode::Copy)
        .is_err());
    let mut stale = manifest.clone();
    stale
        .generated_outputs
        .iter_mut()
        .find(|o| o.destination_path.as_deref() == Some(".claude/settings.json"))
        .unwrap()
        .digest = Some(format!("sha256:{}", "0".repeat(64)));
    assert!(registry
        .apply_manifest(project.path(), &stale, &ApplyMode::Copy)
        .is_err());
}

#[test]
fn replay_rejects_unknown_output_even_when_kind_and_attribution_are_forged() {
    let (_library, project, registry, params) = fixture(false);
    fs::write(
        project.path().join(".git/info/exclude"),
        ".metactl/\n.claude/\nCLAUDE.local.md\n",
    )
    .unwrap();
    let mut manifest = registry.compile(params).unwrap().compile_manifest;
    let mut injected = manifest
        .generated_outputs
        .iter()
        .find(|o| o.destination_path.as_deref() == Some(".claude/settings.json"))
        .unwrap()
        .clone();
    let staged_source = project.path().join(&injected.path);
    injected.destination_path = Some("ordinary-document.txt".into());
    injected.path = ".metactl/generated/claude-code/ordinary-document.txt".into();
    injected.kind = GeneratedOutputKind::Other;
    injected.pack_ref = None;
    injected.id = Some("ordinary-document".into());
    injected.ownership_token = Some("ordinary-document".into());
    fs::copy(staged_source, project.path().join(&injected.path)).unwrap();
    manifest.generated_outputs.push(injected);
    assert!(registry
        .apply_manifest(project.path(), &manifest, &ApplyMode::Copy)
        .is_err());
    assert!(!project.path().join("ordinary-document.txt").exists());
}

#[test]
fn graph_privacy_audits_all_reference_and_opaque_metadata_fields() {
    let (_library, _project, registry, mut params) = fixture(false);
    let graph = &mut params.resolve_graph;
    graph.requested_pack_refs.clear();
    graph.activated_pack_refs.clear();
    graph.suppressed_packs.clear();
    graph.capability_gaps.clear();
    graph.pack_visibility.clear();
    graph.provenance_refs.clear();
    assert!(!registry.graph_requires_private_state(graph));
    let shared_graph = graph.clone();
    let private_ref = Ref {
        kind: RefKind::Pack,
        id: "private-runtime".into(),
        version: Some("1.0.0".into()),
    };
    let unknown_ref = Ref {
        kind: RefKind::Pack,
        id: "unavailable-private".into(),
        version: None,
    };
    let mut variants = Vec::new();
    for reference in [
        private_ref.clone(),
        unknown_ref.clone(),
        Ref {
            kind: RefKind::Pack,
            id: "python-refactor".into(),
            version: Some("unknown-version".into()),
        },
    ] {
        let mut g = shared_graph.clone();
        g.requested_pack_refs.push(reference.clone());
        variants.push(g);
        let mut g = shared_graph.clone();
        g.activated_pack_refs.push(reference.clone());
        variants.push(g);
        let mut g = shared_graph.clone();
        g.suppressed_packs.push(crate::types::SuppressedRef {
            pack_ref: reference.clone(),
            reason_code: ReasonCode::NotFound,
            detail: None,
        });
        variants.push(g);
        let mut g = shared_graph.clone();
        g.capability_gaps.push(CapabilityGap {
            feature: "pack_selection".into(),
            reason_code: ReasonCode::ZeroMatch,
            affected_refs: vec![reference],
        });
        variants.push(g);
    }
    let mut g = shared_graph.clone();
    g.pack_visibility
        .insert("private-runtime".into(), VisibilityScope::Shared);
    variants.push(g);
    let mut g = shared_graph.clone();
    g.provenance_refs.push("artifact:private-source".into());
    variants.push(g);
    let mut g = shared_graph.clone();
    g.capability_gaps.push(CapabilityGap {
        feature: "opaque-private-feature".into(),
        reason_code: ReasonCode::NotFound,
        affected_refs: vec![g.role.clone()],
    });
    variants.push(g);
    let mut g = shared_graph.clone();
    g.role.id = "unknown-private-role".into();
    variants.push(g);
    let mut g = shared_graph.clone();
    g.selected_target.id = "unknown-private-target".into();
    variants.push(g);
    let mut g = shared_graph.clone();
    g.applied_policies[0].id = "unknown-private-policy".into();
    variants.push(g);
    let mut g = shared_graph.clone();
    g.source_config_digest = Some("private-id-not-a-digest".into());
    variants.push(g);
    let mut g = shared_graph.clone();
    g.role.kind = RefKind::Pack;
    variants.push(g);
    let mut g = shared_graph.clone();
    g.suppressed_packs.push(crate::types::SuppressedRef {
        pack_ref: Ref {
            kind: RefKind::Pack,
            id: "python-refactor".into(),
            version: None,
        },
        reason_code: ReasonCode::UnsupportedTarget,
        detail: Some("private-free-text".into()),
    });
    variants.push(g);
    for (index, graph) in variants.iter().enumerate() {
        assert!(
            registry.graph_requires_private_state(graph),
            "unclassified graph variant {index}"
        );
    }
    // An ordinary registry-proven shared graph still compiles without Git.
    let plain = TempDir::new().unwrap();
    params.resolve_graph = shared_graph.clone();
    params.project_root = Some(plain.path().display().to_string());
    registry.compile(params.clone()).unwrap();
    // Missing requested refs must refuse even though no output is private.
    params.resolve_graph.requested_pack_refs.push(unknown_ref);
    assert!(registry.compile(params).is_err());
}
