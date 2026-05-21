use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use metactl::{
    ApplyMode, Config, ConfigDefaults, EntryPoint, InvocationOverlay, LibraryRegistry,
    MetactlKernel, ReasonCode, Ref, RefKind, ReferenceKernel, ResolveParams, ValidationStatus,
};
use pretty_assertions::assert_eq;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn starter_root() -> PathBuf {
    repo_root().join("library/starter")
}

fn unique_project_root(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = repo_root()
        .join("tmp")
        .join("tests")
        .join(format!("{name}-{stamp}"));
    fs::create_dir_all(&root).expect("create temp project");
    root
}

fn target_manifest(id: &str) -> metactl::TargetCapabilityMatrix {
    let path = starter_root().join("targets").join(format!("{id}.json"));
    serde_json::from_slice(&fs::read(path).expect("target bytes")).expect("target manifest")
}

fn config(role: &str, policy: &str, target: &str) -> Config {
    Config {
        api_version: metactl::API_VERSION.to_string(),
        role: Ref {
            kind: RefKind::Role,
            id: role.to_string(),
            version: Some("1.0.0".to_string()),
        },
        packs: Vec::new(),
        policy: Ref {
            kind: RefKind::Policy,
            id: policy.to_string(),
            version: Some("1.0.0".to_string()),
        },
        targets: vec![Ref {
            kind: RefKind::Target,
            id: target.to_string(),
            version: Some("2026.03.26".to_string()),
        }],
        defaults: Some(ConfigDefaults {
            brownfield_mode: None,
            discovery_mode: None,
            surface_selection_mode: None,
        }),
        metadata: Default::default(),
    }
}

fn overlay(entrypoint: EntryPoint, target: &str) -> InvocationOverlay {
    InvocationOverlay {
        entrypoint,
        task: None,
        selected_project: None,
        attached_artifacts: Vec::new(),
        privacy_mode: None,
        cost_budget_usd: None,
        selected_target_override: Some(Ref {
            kind: RefKind::Target,
            id: target.to_string(),
            version: Some("2026.03.26".to_string()),
        }),
        temporary_approvals: Vec::new(),
        candidate_pack_hints: Vec::new(),
    }
}

fn kernel() -> ReferenceKernel {
    ReferenceKernel::load_from_library_roots(vec![starter_root()]).expect("starter kernel")
}

#[test]
fn sandbox_greenfield_apply_revert() {
    let project_root = unique_project_root("greenfield");
    let kernel = kernel();
    let target = target_manifest("claude-code");

    let resolve = kernel
        .resolve(ResolveParams {
            config: config("reviewer", "safe-review", "claude-code"),
            overlay: Some(overlay(EntryPoint::MagicwormholeHotkey, "claude-code")),
            available_targets: vec![target.clone()],
            provenance: None,
        })
        .expect("resolve");

    let compile = kernel
        .compile(metactl::CompileParams {
            resolve_graph: resolve.clone(),
            target_capability: target.clone(),
            apply_mode: ApplyMode::Copy,
            surface_selection_mode: None,
            emit_policy_report: true,
            durable_staging: true,
            project_root: Some(project_root.to_string_lossy().to_string()),
        })
        .expect("compile");

    assert!(compile
        .compile_manifest
        .generated_outputs
        .iter()
        .all(|item| item.path.starts_with(".metactl/generated/claude-code/")));

    let apply = kernel
        .apply_compiled_outputs(&project_root, &compile.compile_manifest, &ApplyMode::Copy)
        .expect("apply");
    assert!(apply.conflicts.is_empty());
    assert!(project_root.join("CLAUDE.md").exists());
    assert!(project_root.join(".claude/settings.json").exists());

    let validate = kernel
        .validate(metactl::ValidateParams {
            subject_ref: target.target_ref(),
            resolve_graph: Some(resolve),
            compile_manifest: Some(compile.compile_manifest.clone()),
            policy_enforcement_report: compile.policy_enforcement_report.clone(),
            project_root: Some(project_root.to_string_lossy().to_string()),
        })
        .expect("validate");
    assert_eq!(validate.status, ValidationStatus::Pass);

    let revert = kernel
        .revert_target(&project_root, &target.target_ref())
        .expect("revert");
    assert!(revert.conflicts.is_empty());
    assert!(!project_root.join("CLAUDE.md").exists());
}

#[test]
fn sandbox_brownfield_patch_and_refusal() {
    let kernel = kernel();
    let target = target_manifest("codex-cli");

    let patch_root = unique_project_root("brownfield-patch");
    fs::write(patch_root.join("AGENTS.md"), "Existing house rules\n").expect("seed AGENTS");
    let resolve = kernel
        .resolve(ResolveParams {
            config: config("builder", "brownfield-safe-builder", "codex-cli"),
            overlay: Some(overlay(EntryPoint::MagicwormholeDrop, "codex-cli")),
            available_targets: vec![target.clone()],
            provenance: None,
        })
        .expect("resolve");
    let compile = kernel
        .compile(metactl::CompileParams {
            resolve_graph: resolve,
            target_capability: target.clone(),
            apply_mode: ApplyMode::Patch,
            surface_selection_mode: None,
            emit_policy_report: true,
            durable_staging: true,
            project_root: Some(patch_root.to_string_lossy().to_string()),
        })
        .expect("compile");
    let patch_apply = kernel
        .apply_compiled_outputs(&patch_root, &compile.compile_manifest, &ApplyMode::Patch)
        .expect("patch apply");
    assert!(patch_apply.conflicts.is_empty());
    let patched = fs::read_to_string(patch_root.join("AGENTS.md")).expect("patched agents");
    assert!(patched.contains("Existing house rules"));
    assert!(patched.contains("metactl:begin"));

    let refusal_root = unique_project_root("brownfield-refusal");
    // Spec 019: codex-cli no longer emits .codex/config.toml; seed AGENTS.md
    // (a real managed surface) to exercise brownfield collision.
    fs::write(refusal_root.join("AGENTS.md"), "user-owned house rules\n").expect("seed agents");
    let resolve = kernel
        .resolve(ResolveParams {
            config: config("builder", "brownfield-safe-builder", "codex-cli"),
            overlay: Some(overlay(EntryPoint::MagicwormholeDrop, "codex-cli")),
            available_targets: vec![target.clone()],
            provenance: None,
        })
        .expect("resolve refusal");
    let compile = kernel
        .compile(metactl::CompileParams {
            resolve_graph: resolve,
            target_capability: target,
            apply_mode: ApplyMode::Copy,
            surface_selection_mode: None,
            emit_policy_report: true,
            durable_staging: true,
            project_root: Some(refusal_root.to_string_lossy().to_string()),
        })
        .expect("compile refusal");
    let refusal = kernel
        .apply_compiled_outputs(&refusal_root, &compile.compile_manifest, &ApplyMode::Copy)
        .expect("refusal apply");
    assert_eq!(refusal.conflicts.len(), 1);
    assert_eq!(
        refusal.conflicts[0].reason_code,
        ReasonCode::BrownfieldCollision
    );
    assert_eq!(
        fs::read_to_string(refusal_root.join("AGENTS.md")).expect("agents"),
        "user-owned house rules\n"
    );
}

#[test]
fn drift_detection() {
    let project_root = unique_project_root("drift");
    let kernel = kernel();
    let target = target_manifest("codex-cli");
    let resolve = kernel
        .resolve(ResolveParams {
            config: config("builder", "brownfield-safe-builder", "codex-cli"),
            overlay: Some(overlay(EntryPoint::MagicwormholeDrop, "codex-cli")),
            available_targets: vec![target.clone()],
            provenance: None,
        })
        .expect("resolve");
    let compile = kernel
        .compile(metactl::CompileParams {
            resolve_graph: resolve,
            target_capability: target.clone(),
            apply_mode: ApplyMode::Copy,
            surface_selection_mode: None,
            emit_policy_report: true,
            durable_staging: true,
            project_root: Some(project_root.to_string_lossy().to_string()),
        })
        .expect("compile");
    kernel
        .apply_compiled_outputs(&project_root, &compile.compile_manifest, &ApplyMode::Copy)
        .expect("apply");
    fs::write(project_root.join("AGENTS.md"), "manual drift\n").expect("introduce drift");

    let drift = kernel
        .detect_drift(&project_root, &target.target_ref())
        .expect("drift report");
    assert_eq!(drift.status, ValidationStatus::Fail);
    assert!(drift
        .checks
        .iter()
        .any(|check| check.id == "applied-output-drift"));
}

#[test]
fn managed_drift_apply_patch_repairs_codex_agents() {
    let project_root = unique_project_root("drift-repair");
    let kernel = kernel();
    let target = target_manifest("codex-cli");
    let resolve = kernel
        .resolve(ResolveParams {
            config: config("builder", "brownfield-safe-builder", "codex-cli"),
            overlay: Some(overlay(EntryPoint::MagicwormholeDrop, "codex-cli")),
            available_targets: vec![target.clone()],
            provenance: None,
        })
        .expect("resolve");
    let compile = kernel
        .compile(metactl::CompileParams {
            resolve_graph: resolve.clone(),
            target_capability: target.clone(),
            apply_mode: ApplyMode::Patch,
            surface_selection_mode: None,
            emit_policy_report: true,
            durable_staging: true,
            project_root: Some(project_root.to_string_lossy().to_string()),
        })
        .expect("compile");
    kernel
        .apply_compiled_outputs(&project_root, &compile.compile_manifest, &ApplyMode::Patch)
        .expect("first apply");
    fs::write(project_root.join("AGENTS.md"), "manual drift\n").expect("introduce drift");

    let drift = kernel
        .detect_drift(&project_root, &target.target_ref())
        .expect("drift report");
    assert_eq!(drift.status, ValidationStatus::Fail);

    let repair = kernel
        .apply_compiled_outputs(&project_root, &compile.compile_manifest, &ApplyMode::Patch)
        .expect("repair apply");
    assert!(repair.conflicts.is_empty());

    let after = kernel
        .detect_drift(&project_root, &target.target_ref())
        .expect("drift after repair");
    assert_eq!(after.status, ValidationStatus::Pass);
}

#[test]
fn magicwormhole_overlay_entrypoints() {
    let kernel = kernel();
    let cases = [
        (
            "reviewer",
            "safe-review",
            "claude-code",
            EntryPoint::MagicwormholeHotkey,
        ),
        (
            "reviewer",
            "safe-review",
            "claude-code",
            EntryPoint::MagicwormholeTray,
        ),
        (
            "release-manager",
            "release-policy",
            "openclaw",
            EntryPoint::MagicwormholeNotch,
        ),
        (
            "builder",
            "brownfield-safe-builder",
            "codex-cli",
            EntryPoint::MagicwormholeDrop,
        ),
    ];

    for (role, policy, target_id, entrypoint) in cases {
        let target = target_manifest(target_id);
        let project_root = unique_project_root(target_id);
        let resolve = kernel
            .resolve(ResolveParams {
                config: config(role, policy, target_id),
                overlay: Some(overlay(entrypoint, target_id)),
                available_targets: vec![target.clone()],
                provenance: None,
            })
            .expect("resolve");
        let explain = kernel
            .explain(metactl::ExplainParams {
                resolve_graph: resolve.clone(),
            })
            .expect("explain");
        assert!(explain.summary.contains(target_id));
        let compile = kernel
            .compile(metactl::CompileParams {
                resolve_graph: resolve.clone(),
                target_capability: target.clone(),
                apply_mode: ApplyMode::Copy,
                surface_selection_mode: None,
                emit_policy_report: true,
                durable_staging: true,
                project_root: Some(project_root.to_string_lossy().to_string()),
            })
            .expect("compile");
        let validate = kernel
            .validate(metactl::ValidateParams {
                subject_ref: target.target_ref(),
                resolve_graph: Some(resolve),
                compile_manifest: Some(compile.compile_manifest),
                policy_enforcement_report: compile.policy_enforcement_report,
                project_root: Some(project_root.to_string_lossy().to_string()),
            })
            .expect("validate");
        assert!(matches!(
            validate.status,
            ValidationStatus::Pass | ValidationStatus::Warn
        ));
    }
}

#[test]
fn openclaw_target() {
    let project_root = unique_project_root("openclaw");
    let kernel = kernel();
    let target = target_manifest("openclaw");
    let resolve = kernel
        .resolve(ResolveParams {
            config: config("reviewer", "safe-review", "openclaw"),
            overlay: Some(overlay(EntryPoint::MagicwormholeNotch, "openclaw")),
            available_targets: vec![target.clone()],
            provenance: None,
        })
        .expect("resolve");
    let compile = kernel
        .compile(metactl::CompileParams {
            resolve_graph: resolve,
            target_capability: target.clone(),
            apply_mode: ApplyMode::Copy,
            surface_selection_mode: None,
            emit_policy_report: true,
            durable_staging: true,
            project_root: Some(project_root.to_string_lossy().to_string()),
        })
        .expect("compile");

    let destinations = compile
        .compile_manifest
        .generated_outputs
        .iter()
        .map(|item| item.destination_path.clone().unwrap_or_default())
        .collect::<Vec<_>>();
    assert!(destinations.contains(&"OPENCLAW.md".to_string()));
    assert!(destinations.contains(&".openclaw/config.json".to_string()));
    assert!(compile
        .compile_manifest
        .degradations
        .iter()
        .any(|gap| gap.feature == "local_scripts"));
}

#[test]
fn codex_multi_surface_emission_uses_pack_scoped_surface_paths() {
    let project_root = unique_project_root("codex-multi-surface");
    let kernel = kernel();
    let target = target_manifest("codex-cli");
    let resolve = kernel
        .resolve(ResolveParams {
            config: config("builder", "brownfield-safe-builder", "codex-cli"),
            overlay: Some(overlay(EntryPoint::MagicwormholeDrop, "codex-cli")),
            available_targets: vec![target.clone()],
            provenance: None,
        })
        .expect("resolve");
    let compile = kernel
        .compile(metactl::CompileParams {
            resolve_graph: resolve,
            target_capability: target,
            apply_mode: ApplyMode::Copy,
            surface_selection_mode: Some(metactl::SurfaceSelectionMode::Full),
            emit_policy_report: true,
            durable_staging: true,
            project_root: Some(project_root.to_string_lossy().to_string()),
        })
        .expect("compile");

    let destinations = compile
        .compile_manifest
        .generated_outputs
        .iter()
        .filter_map(|item| item.destination_path.clone())
        .collect::<Vec<_>>();
    assert!(destinations
        .contains(&".codex/skills/python-refactor/python-refactor/SKILL.md".to_string()));
    assert!(destinations.contains(&".codex/skills/python-refactor/contracts/SKILL.md".to_string()));
    assert!(destinations.contains(&".codex/skills/python-refactor/tests/SKILL.md".to_string()));
    assert!(compile
        .compile_manifest
        .generated_outputs
        .iter()
        .any(|item| {
            item.surface_id.as_deref() == Some("python-refactor:contracts")
                && item.merge_status == Some(metactl::SurfaceMergeStatus::Separate)
        }));
}

#[test]
fn merge_degradation_is_explicit_when_target_cannot_emit_separate_surfaces() {
    let project_root = unique_project_root("codex-merged-surface");
    let kernel = kernel();
    let mut target = target_manifest("codex-cli");
    let codex_target = target
        .compile_targets
        .iter_mut()
        .find(|item| item.output_kind == metactl::CompileTargetKind::CodexSkill)
        .expect("codex skill target");
    codex_target.path_template = "skills/{pack_id}/SKILL.md".to_string();
    codex_target.supports_multi_surface_pack = false;
    codex_target.surface_merge_strategy = Some(metactl::SurfaceMergeStrategy::Required);

    let resolve = kernel
        .resolve(ResolveParams {
            config: config("builder", "brownfield-safe-builder", "codex-cli"),
            overlay: Some(overlay(EntryPoint::MagicwormholeDrop, "codex-cli")),
            available_targets: vec![target.clone()],
            provenance: None,
        })
        .expect("resolve");
    let compile = kernel
        .compile(metactl::CompileParams {
            resolve_graph: resolve,
            target_capability: target,
            apply_mode: ApplyMode::Copy,
            surface_selection_mode: Some(metactl::SurfaceSelectionMode::Full),
            emit_policy_report: true,
            durable_staging: true,
            project_root: Some(project_root.to_string_lossy().to_string()),
        })
        .expect("compile");

    assert!(compile
        .compile_manifest
        .generated_outputs
        .iter()
        .any(
            |item| item.destination_path.as_deref() == Some("skills/python-refactor/SKILL.md")
                && item.merge_status == Some(metactl::SurfaceMergeStatus::Merged)
                && item
                    .degradation_codes
                    .iter()
                    .any(|code| code == "merged_surface_pack")
        ));
    assert!(compile
        .compile_manifest
        .degradations
        .iter()
        .any(|gap| gap.feature == "surface_merge:python-refactor"));
}

#[test]
fn target_native_pack_resources() {
    let kernel = kernel();
    let cases = [
        (
            "reviewer",
            "safe-review",
            "claude-code",
            vec![
                "CLAUDE.md",
                ".claude/settings.json",
                ".claude/commands/unit-test-loop/run-targeted-tests.md",
                ".claude/skills/unit-test-loop/unit-test-loop/SKILL.md",
            ],
        ),
        (
            "release-manager",
            "release-policy",
            "openclaw",
            // Spec 019: openclaw only emits OPENCLAW.md, .openclaw/config.json,
            // and instruction pack_resource bodies under .openclaw/packs/...
            vec![
                "OPENCLAW.md",
                ".openclaw/config.json",
                ".openclaw/packs/unit-test-loop/SKILL.md",
            ],
        ),
        (
            "release-manager",
            "release-policy",
            "codex-cli",
            // Spec 019 plus Codex command support.
            vec![
                "AGENTS.md",
                ".codex/skills/unit-test-loop/unit-test-loop/SKILL.md",
                ".codex/commands/run-targeted-tests.md",
            ],
        ),
        (
            "builder",
            "brownfield-safe-builder",
            "codex-cli",
            vec!["AGENTS.md"],
        ),
    ];

    for (role, policy, target_id, expected_paths) in cases {
        let project_root = unique_project_root(&format!("native-pack-resource-{target_id}"));
        let target = target_manifest(target_id);
        let resolve = kernel
            .resolve(ResolveParams {
                config: config(role, policy, target_id),
                overlay: Some(overlay(EntryPoint::MagicwormholeDrop, target_id)),
                available_targets: vec![target.clone()],
                provenance: None,
            })
            .expect("resolve");
        let compile = kernel
            .compile(metactl::CompileParams {
                resolve_graph: resolve,
                target_capability: target.clone(),
                apply_mode: ApplyMode::Copy,
                surface_selection_mode: None,
                emit_policy_report: true,
                durable_staging: true,
                project_root: Some(project_root.to_string_lossy().to_string()),
            })
            .expect("compile");

        let destinations = compile
            .compile_manifest
            .generated_outputs
            .iter()
            .filter_map(|item| item.destination_path.clone())
            .collect::<Vec<_>>();
        for expected in &expected_paths {
            assert!(
                destinations.contains(&expected.to_string()),
                "missing staged output {expected} for {target_id}: {destinations:?}"
            );
        }

        let apply = kernel
            .apply_compiled_outputs(&project_root, &compile.compile_manifest, &ApplyMode::Copy)
            .expect("apply");
        assert!(apply.conflicts.is_empty(), "{:?}", apply.conflicts);
        for expected in &expected_paths {
            assert!(
                project_root.join(expected).exists(),
                "missing applied output {} for {}",
                expected,
                target_id
            );
        }
    }
}

#[test]
fn reference_based_instruction_indexes() {
    let kernel = kernel();
    let cases = [
        (
            "reviewer",
            "safe-review",
            "claude-code",
            "CLAUDE.md",
            ".claude/skills/unit-test-loop/",
            "Run the narrowest relevant test loop before closing work.",
        ),
        (
            "release-manager",
            "release-policy",
            "openclaw",
            "OPENCLAW.md",
            ".openclaw/packs/unit-test-loop/",
            "Run the narrowest relevant test loop before closing work.",
        ),
        (
            "release-manager",
            "release-policy",
            "codex-cli",
            "AGENTS.md",
            ".codex/skills/unit-test-loop/",
            "Run the narrowest relevant test loop before closing work.",
        ),
    ];

    for (role, policy, target_id, entry_path, referenced_body, inline_phrase) in cases {
        let project_root = unique_project_root(&format!("instruction-index-{target_id}"));
        let target = target_manifest(target_id);
        let resolve = kernel
            .resolve(ResolveParams {
                config: config(role, policy, target_id),
                overlay: Some(overlay(EntryPoint::MagicwormholeDrop, target_id)),
                available_targets: vec![target.clone()],
                provenance: None,
            })
            .expect("resolve");
        let compile = kernel
            .compile(metactl::CompileParams {
                resolve_graph: resolve,
                target_capability: target.clone(),
                apply_mode: ApplyMode::Copy,
                surface_selection_mode: None,
                emit_policy_report: true,
                durable_staging: true,
                project_root: Some(project_root.to_string_lossy().to_string()),
            })
            .expect("compile");

        let entry = compile
            .compile_manifest
            .generated_outputs
            .iter()
            .find(|item| item.destination_path.as_deref() == Some(entry_path))
            .expect("entry document");
        assert_eq!(
            entry.instruction_mode,
            Some(metactl::InstructionProjectionMode::ReferenceIndex)
        );
        assert!(compile
            .compile_manifest
            .generated_outputs
            .iter()
            .any(|item| {
                item.destination_path
                    .as_deref()
                    .is_some_and(|path| path.starts_with(referenced_body))
            }));

        let staged_entry =
            fs::read_to_string(project_root.join(&entry.path)).expect("staged entry");
        assert!(staged_entry.contains("Prefer retrieval-led reasoning"));
        assert!(staged_entry.contains(referenced_body));
        assert!(!staged_entry.contains(inline_phrase));
    }
}

#[test]
fn starter_library_inventory_and_metadata_checks() {
    let registry = LibraryRegistry::load_from_roots(&[starter_root()]).expect("starter registry");
    let (roles, policies, packs) = registry
        .starter_library_inventory_report()
        .expect("starter inventory");
    assert_eq!(roles, 3);
    assert_eq!(policies, 3);
    assert_eq!(packs, 13);

    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(starter_root().join("library.json")).expect("starter metadata"),
    )
    .expect("library metadata json");
    assert_eq!(manifest["minimum_roles"], 3);
    assert_eq!(manifest["minimum_packs"], 13);
}
