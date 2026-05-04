use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::project::atomic_write;
use crate::types::{
    ApplyConflict, ApplyMode, ApplyReport, BrownfieldMode, CapabilityGap, CompileManifest,
    GeneratedOutput, GeneratedOutputKind, InstructionProjectionMode, ReasonCode, Ref, RevertReport,
    SurfaceMergeStatus,
};

#[derive(Debug, Clone)]
pub(crate) struct StagedOutputInput {
    pub id: Option<String>,
    pub destination_path: String,
    pub kind: GeneratedOutputKind,
    pub contents: Vec<u8>,
    pub instruction_mode: Option<InstructionProjectionMode>,
    pub pack_ref: Option<Ref>,
    pub surface_id: Option<String>,
    pub surface_slug: Option<String>,
    pub source_resource_paths: Vec<String>,
    pub merge_status: Option<SurfaceMergeStatus>,
    pub degradation_codes: Vec<String>,
    pub ownership_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedState {
    api_version: String,
    target: Ref,
    apply_mode: ApplyMode,
    outputs: Vec<ManagedOutputState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedOutputState {
    id: Option<String>,
    staged_path: String,
    destination_path: String,
    applied_digest: String,
    backup_path: Option<String>,
    existed_before: bool,
    patch_marker: Option<String>,
    #[serde(default)]
    instruction_mode: Option<InstructionProjectionMode>,
    #[serde(default)]
    pack_ref: Option<Ref>,
    #[serde(default)]
    surface_id: Option<String>,
    #[serde(default)]
    surface_slug: Option<String>,
    #[serde(default)]
    source_resource_paths: Vec<String>,
    #[serde(default)]
    merge_status: Option<SurfaceMergeStatus>,
    #[serde(default)]
    degradation_codes: Vec<String>,
    #[serde(default)]
    ownership_token: Option<String>,
}

#[derive(Debug, Clone)]
enum ActionKind {
    CreateFile,
    CreateSymlink,
    OverwriteManaged,
    MergeJsonManaged,
    PatchManaged,
    MergeJsonUnmanaged,
    PatchUnmanaged,
    TakeoverUnmanaged,
}

#[derive(Debug, Clone)]
struct PlannedAction {
    output: GeneratedOutput,
    kind: ActionKind,
    backup_path: Option<PathBuf>,
    patch_marker: Option<String>,
    existed_before: bool,
}

pub(crate) fn stage_outputs(
    project_root: &Path,
    target: &Ref,
    inputs: Vec<StagedOutputInput>,
    surface_selection_mode: Option<crate::types::SurfaceSelectionMode>,
    surface_selection: Vec<crate::types::SurfaceSelectionDecision>,
    apply_modes_supported: Vec<ApplyMode>,
    brownfield_mode: Option<BrownfieldMode>,
    degradations: Vec<CapabilityGap>,
) -> Result<CompileManifest> {
    let stage_root = project_root
        .join(".metactl")
        .join("generated")
        .join(&target.id);
    fs::create_dir_all(&stage_root).with_context(|| format!("create {}", stage_root.display()))?;

    let mut outputs = Vec::new();
    let mut seen_destinations = BTreeSet::new();
    for input in inputs {
        if !seen_destinations.insert(input.destination_path.clone()) {
            return Err(anyhow!(
                "duplicate generated destination path '{}' for target {}",
                input.destination_path,
                target.id
            ));
        }
        let relative_stage_path = Path::new(".metactl")
            .join("generated")
            .join(&target.id)
            .join(&input.destination_path);
        let stage_path = project_root.join(&relative_stage_path);
        if let Some(parent) = stage_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        atomic_write(&stage_path, &input.contents)
            .with_context(|| format!("write {}", stage_path.display()))?;
        outputs.push(GeneratedOutput {
            id: input.id,
            path: normalize_relative(&relative_stage_path),
            destination_path: Some(input.destination_path),
            kind: input.kind,
            digest: Some(sha256_bytes(&input.contents)),
            instruction_mode: input.instruction_mode,
            pack_ref: input.pack_ref,
            surface_id: input.surface_id,
            surface_slug: input.surface_slug,
            source_resource_paths: input.source_resource_paths,
            merge_status: input.merge_status,
            degradation_codes: input.degradation_codes,
            ownership_token: input.ownership_token,
            managed: true,
        });
    }

    let manifest = CompileManifest {
        api_version: crate::types::API_VERSION.to_string(),
        target: target.clone(),
        generated_outputs: outputs,
        surface_selection_mode,
        surface_selection,
        apply_modes_supported,
        brownfield_mode,
        degradations,
    };

    let manifest_path = stage_root.join("compile.manifest.json");
    atomic_write(
        &manifest_path,
        &serde_json::to_vec_pretty(&manifest).context("serialize compile manifest")?,
    )
    .with_context(|| format!("write {}", manifest_path.display()))?;

    Ok(manifest)
}

pub(crate) fn apply_manifest(
    project_root: &Path,
    manifest: &CompileManifest,
    apply_mode: &ApplyMode,
) -> Result<ApplyReport> {
    let state_path = state_path(project_root, &manifest.target);
    let existing_state = load_state(&state_path)?;
    let plans = plan_apply(project_root, manifest, apply_mode, existing_state.as_ref())?;

    if let Some(conflicts) = collect_conflicts(&plans) {
        return Ok(ApplyReport {
            target: manifest.target.clone(),
            applied_paths: Vec::new(),
            conflicts,
            state_path: normalize_relative(
                &state_path
                    .strip_prefix(project_root)
                    .unwrap_or(state_path.as_path()),
            ),
        });
    }

    let mut applied_paths = Vec::new();
    let mut state_outputs = Vec::new();
    for plan in plans {
        let plan = plan?;
        let destination_path = plan
            .output
            .destination_path
            .as_ref()
            .ok_or_else(|| anyhow!("generated output is missing destination_path"))?;
        let destination_abs = project_root.join(destination_path);
        let staged_abs = project_root.join(&plan.output.path);
        let staged_bytes =
            fs::read(&staged_abs).with_context(|| format!("read {}", staged_abs.display()))?;

        if let Some(parent) = destination_abs.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }

        if plan.existed_before
            && matches!(
                plan.kind,
                ActionKind::MergeJsonUnmanaged
                    | ActionKind::PatchUnmanaged
                    | ActionKind::TakeoverUnmanaged
            )
        {
            if let Some(backup_path) = &plan.backup_path {
                backup_existing(&destination_abs, backup_path)?;
            }
        }

        let patch_marker = plan.patch_marker.clone();
        match plan.kind {
            ActionKind::CreateFile | ActionKind::TakeoverUnmanaged => {
                // `fs::write` follows symlinks; replacing a managed symlink with a regular file
                // requires removing the link first (otherwise bytes land in `.metactl/generated/`).
                if destination_abs.is_symlink() {
                    fs::remove_file(&destination_abs)
                        .with_context(|| format!("remove symlink {}", destination_abs.display()))?;
                }
                atomic_write(&destination_abs, &staged_bytes)
                    .with_context(|| format!("write {}", destination_abs.display()))?;
            }
            ActionKind::OverwriteManaged => {
                let replace_link = !matches!(apply_mode, ApplyMode::Symlink)
                    || materialize_as_regular_file(
                        &manifest.target,
                        &plan.output,
                        destination_path,
                    );
                if replace_link && destination_abs.is_symlink() {
                    fs::remove_file(&destination_abs)
                        .with_context(|| format!("remove symlink {}", destination_abs.display()))?;
                }
                atomic_write(&destination_abs, &staged_bytes)
                    .with_context(|| format!("write {}", destination_abs.display()))?;
            }
            ActionKind::CreateSymlink => {
                recreate_symlink(&staged_abs, &destination_abs)?;
            }
            ActionKind::MergeJsonManaged | ActionKind::MergeJsonUnmanaged => {
                let existing = fs::read_to_string(&destination_abs)
                    .with_context(|| format!("read {}", destination_abs.display()))?;
                let staged = String::from_utf8(staged_bytes.clone())
                    .map_err(|_| anyhow!("staged output {} is not utf-8", staged_abs.display()))?;
                let merged = merge_json_document(destination_path, &existing, &staged)?;
                if destination_abs.is_symlink() {
                    fs::remove_file(&destination_abs)
                        .with_context(|| format!("remove symlink {}", destination_abs.display()))?;
                }
                atomic_write(&destination_abs, merged.as_bytes())
                    .with_context(|| format!("write {}", destination_abs.display()))?;
            }
            ActionKind::PatchManaged | ActionKind::PatchUnmanaged => {
                let existing = fs::read_to_string(&destination_abs)
                    .with_context(|| format!("read {}", destination_abs.display()))?;
                let staged = String::from_utf8(staged_bytes.clone())
                    .map_err(|_| anyhow!("staged output {} is not utf-8", staged_abs.display()))?;
                let marker = patch_marker
                    .as_deref()
                    .ok_or_else(|| anyhow!("patch apply missing marker"))?;
                let patched = patch_document(&existing, &staged, marker)?;
                atomic_write(&destination_abs, patched.as_bytes())
                    .with_context(|| format!("write {}", destination_abs.display()))?;
            }
        }

        let applied_digest = sha256_path(&destination_abs)?;
        applied_paths.push(destination_path.clone());
        state_outputs.push(ManagedOutputState {
            id: plan.output.id.clone(),
            staged_path: plan.output.path.clone(),
            destination_path: destination_path.clone(),
            applied_digest,
            backup_path: plan.backup_path.as_ref().map(|path: &PathBuf| {
                normalize_relative(path.strip_prefix(project_root).unwrap_or(path))
            }),
            existed_before: plan.existed_before,
            patch_marker: plan.patch_marker,
            instruction_mode: plan.output.instruction_mode.clone(),
            pack_ref: plan.output.pack_ref.clone(),
            surface_id: plan.output.surface_id.clone(),
            surface_slug: plan.output.surface_slug.clone(),
            source_resource_paths: plan.output.source_resource_paths.clone(),
            merge_status: plan.output.merge_status.clone(),
            degradation_codes: plan.output.degradation_codes.clone(),
            ownership_token: plan.output.ownership_token.clone(),
        });
    }

    if let Some(parent) = state_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let state = ManagedState {
        api_version: crate::types::API_VERSION.to_string(),
        target: manifest.target.clone(),
        apply_mode: apply_mode.clone(),
        outputs: state_outputs,
    };
    atomic_write(
        &state_path,
        &serde_json::to_vec_pretty(&state).context("serialize managed state")?,
    )
    .with_context(|| format!("write {}", state_path.display()))?;

    Ok(ApplyReport {
        target: manifest.target.clone(),
        applied_paths,
        conflicts: Vec::new(),
        state_path: normalize_relative(
            &state_path
                .strip_prefix(project_root)
                .unwrap_or(state_path.as_path()),
        ),
    })
}

pub(crate) fn revert_target(project_root: &Path, target: &Ref) -> Result<RevertReport> {
    let state_path = state_path(project_root, target);
    let Some(state) = load_state(&state_path)? else {
        return Ok(RevertReport {
            target: target.clone(),
            reverted_paths: Vec::new(),
            conflicts: vec![ApplyConflict {
                destination_path: String::new(),
                reason_code: ReasonCode::NotFound,
                detail: format!("No managed state found for target {}.", target.id),
            }],
            state_path: None,
        });
    };

    let mut conflicts = Vec::new();
    for output in &state.outputs {
        let destination_abs = project_root.join(&output.destination_path);
        if !destination_abs.exists() {
            conflicts.push(ApplyConflict {
                destination_path: output.destination_path.clone(),
                reason_code: ReasonCode::ConflictDetected,
                detail: "Managed output is missing and cannot be reverted cleanly.".to_string(),
            });
            continue;
        }
        let actual = sha256_path(&destination_abs)?;
        if actual != output.applied_digest {
            conflicts.push(ApplyConflict {
                destination_path: output.destination_path.clone(),
                reason_code: ReasonCode::ConflictDetected,
                detail: "Managed output has drifted since apply.".to_string(),
            });
        }
    }

    if !conflicts.is_empty() {
        return Ok(RevertReport {
            target: target.clone(),
            reverted_paths: Vec::new(),
            conflicts,
            state_path: Some(normalize_relative(
                &state_path
                    .strip_prefix(project_root)
                    .unwrap_or(state_path.as_path()),
            )),
        });
    }

    let mut reverted_paths = Vec::new();
    for output in &state.outputs {
        let destination_abs = project_root.join(&output.destination_path);
        if let Some(backup_path) = &output.backup_path {
            let backup_abs = project_root.join(backup_path);
            let backup_bytes =
                fs::read(&backup_abs).with_context(|| format!("read {}", backup_abs.display()))?;
            atomic_write(&destination_abs, &backup_bytes)
                .with_context(|| format!("write {}", destination_abs.display()))?;
            let _ = fs::remove_file(&backup_abs);
        } else {
            let _ = fs::remove_file(&destination_abs);
        }
        remove_empty_parents(&destination_abs, project_root);
        reverted_paths.push(output.destination_path.clone());
    }

    let _ = fs::remove_file(&state_path);
    let backup_dir = backup_root(project_root, target);
    if backup_dir.exists() {
        let _ = fs::remove_dir_all(&backup_dir);
    }

    Ok(RevertReport {
        target: target.clone(),
        reverted_paths,
        conflicts: Vec::new(),
        state_path: Some(normalize_relative(
            &state_path
                .strip_prefix(project_root)
                .unwrap_or(state_path.as_path()),
        )),
    })
}

pub(crate) fn drift_conflicts(project_root: &Path, target: &Ref) -> Result<Vec<ApplyConflict>> {
    let state_path = state_path(project_root, target);
    let Some(state) = load_state(&state_path)? else {
        return Ok(vec![ApplyConflict {
            destination_path: String::new(),
            reason_code: ReasonCode::NotFound,
            detail: format!("No managed state found for target {}.", target.id),
        }]);
    };

    let mut conflicts = Vec::new();
    for output in state.outputs {
        let destination_abs = project_root.join(&output.destination_path);
        if !destination_abs.exists() {
            conflicts.push(ApplyConflict {
                destination_path: output.destination_path,
                reason_code: ReasonCode::ConflictDetected,
                detail: "Managed output is missing from the repository.".to_string(),
            });
            continue;
        }
        let actual = sha256_path(&destination_abs)?;
        if actual != output.applied_digest {
            conflicts.push(ApplyConflict {
                destination_path: output.destination_path,
                reason_code: ReasonCode::ConflictDetected,
                detail: "Managed output digest diverged from recorded state.".to_string(),
            });
        }
    }
    Ok(conflicts)
}

/// Cursor discovers project rules from `.cursor/rules/`. Some editors and tooling resolve
/// symlinked `.mdc` entries poorly compared to regular files. When apply mode is `Symlink`,
/// still materialize the pack **index** as a real file; other cursor outputs may remain
/// symlinks into `.metactl/generated/`.
const CURSOR_PACK_INDEX_DEST: &str = ".cursor/rules/metactl-pack-index.mdc";

fn cursor_pack_index_materialize_as_file(
    target: &crate::types::Ref,
    destination_path: &str,
) -> bool {
    target.id == "cursor" && normalize_relative_dest(destination_path) == CURSOR_PACK_INDEX_DEST
}

fn codex_skill_materialize_as_file(
    target: &crate::types::Ref,
    output: &GeneratedOutput,
    destination_path: &str,
) -> bool {
    target.id == "codex-cli"
        && matches!(output.kind, GeneratedOutputKind::SkillFolder)
        && normalize_relative_dest(destination_path).starts_with(".codex/skills/")
}

fn materialize_as_regular_file(
    target: &crate::types::Ref,
    output: &GeneratedOutput,
    destination_path: &str,
) -> bool {
    matches!(output.kind, GeneratedOutputKind::InstructionFile)
        || cursor_pack_index_materialize_as_file(target, destination_path)
        || codex_skill_materialize_as_file(target, output, destination_path)
        || structured_json_merge_output(output, destination_path)
}

fn structured_json_merge_output(output: &GeneratedOutput, destination_path: &str) -> bool {
    matches!(
        output.kind,
        GeneratedOutputKind::HookConfig
            | GeneratedOutputKind::McpConfig
            | GeneratedOutputKind::RuntimeJson
    ) && normalize_relative_dest(destination_path).ends_with(".json")
}

fn normalize_relative_dest(destination_path: &str) -> String {
    destination_path.replace('\\', "/")
}

fn managed_instruction_patch_marker(
    output: &GeneratedOutput,
    destination_path: &str,
    state_output: &ManagedOutputState,
) -> Option<String> {
    if output.kind == GeneratedOutputKind::InstructionFile && state_output.existed_before {
        Some(
            state_output
                .patch_marker
                .clone()
                .unwrap_or_else(|| patch_marker_for(output, destination_path)),
        )
    } else {
        state_output.patch_marker.clone()
    }
}

fn merge_json_document(destination_path: &str, existing: &str, managed: &str) -> Result<String> {
    let existing_json: serde_json::Value = serde_json::from_str(existing)
        .with_context(|| format!("parse existing {}", destination_path))?;
    let managed_json: serde_json::Value = serde_json::from_str(managed)
        .with_context(|| format!("parse staged {}", destination_path))?;
    let merged = match normalize_relative_dest(destination_path).as_str() {
        ".claude/settings.json" => merge_claude_settings(existing_json, managed_json),
        _ => merge_json_preserving_existing(existing_json, managed_json),
    };
    serde_json::to_string_pretty(&merged)
        .with_context(|| format!("serialize merged {}", destination_path))
}

fn merge_claude_settings(
    existing: serde_json::Value,
    managed: serde_json::Value,
) -> serde_json::Value {
    let (mut existing_map, managed_map) = match (existing, managed) {
        (serde_json::Value::Object(existing_map), serde_json::Value::Object(managed_map)) => {
            (existing_map, managed_map)
        }
        (existing, _) => return existing,
    };

    if let Some(managed_hooks) = managed_map.get("hooks").cloned() {
        let merged_hooks = match existing_map.remove("hooks") {
            Some(existing_hooks) => merge_json_preserving_existing(existing_hooks, managed_hooks),
            None => managed_hooks,
        };
        existing_map.insert("hooks".to_string(), merged_hooks);
    }

    for (key, managed_value) in managed_map {
        if key == "hooks" {
            continue;
        }
        if key == "permissions" || key == "policy" {
            continue;
        }
        match existing_map.remove(&key) {
            Some(existing_value) => {
                existing_map.insert(
                    key,
                    merge_json_preserving_existing(existing_value, managed_value),
                );
            }
            None => {
                existing_map.insert(key, managed_value);
            }
        }
    }

    serde_json::Value::Object(existing_map)
}

fn merge_json_preserving_existing(
    existing: serde_json::Value,
    managed: serde_json::Value,
) -> serde_json::Value {
    match (existing, managed) {
        (serde_json::Value::Object(mut existing_map), serde_json::Value::Object(managed_map)) => {
            for (key, managed_value) in managed_map {
                match existing_map.remove(&key) {
                    Some(existing_value) => {
                        existing_map.insert(
                            key,
                            merge_json_preserving_existing(existing_value, managed_value),
                        );
                    }
                    None => {
                        existing_map.insert(key, managed_value);
                    }
                }
            }
            serde_json::Value::Object(existing_map)
        }
        (serde_json::Value::Array(mut existing_items), serde_json::Value::Array(managed_items)) => {
            for managed_item in managed_items {
                if !existing_items
                    .iter()
                    .any(|existing_item| existing_item == &managed_item)
                {
                    existing_items.push(managed_item);
                }
            }
            serde_json::Value::Array(existing_items)
        }
        (existing_scalar, _) => existing_scalar,
    }
}

fn collect_conflicts(plans: &[Result<PlannedAction>]) -> Option<Vec<ApplyConflict>> {
    let conflicts = plans
        .iter()
        .filter_map(|plan| match plan {
            Ok(_) => None,
            Err(err) => serde_json::from_str::<ApplyConflict>(&err.to_string()).ok(),
        })
        .collect::<Vec<_>>();
    if conflicts.is_empty() {
        None
    } else {
        Some(conflicts)
    }
}

fn plan_apply(
    project_root: &Path,
    manifest: &CompileManifest,
    apply_mode: &ApplyMode,
    existing_state: Option<&ManagedState>,
) -> Result<Vec<Result<PlannedAction>>> {
    let managed = existing_state
        .map(|state| {
            state
                .outputs
                .iter()
                .map(|item| (item.destination_path.clone(), item.clone()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    let mut plans = Vec::new();
    for output in &manifest.generated_outputs {
        let Some(destination_path) = &output.destination_path else {
            plans.push(Err(conflict_json(
                destination_path_fallback(output),
                ReasonCode::MissingMetadata,
                "Generated output is missing destination_path.",
            )));
            continue;
        };
        let destination_abs = project_root.join(destination_path);
        let staged_abs = project_root.join(&output.path);
        let merge_json = structured_json_merge_output(output, destination_path);
        if !staged_abs.exists() {
            plans.push(Err(conflict_json(
                destination_path,
                ReasonCode::NotFound,
                "Staged output is missing from .metactl/generated.",
            )));
            continue;
        }

        if let Some(state_output) = managed.get(destination_path) {
            let managed_instruction_marker =
                managed_instruction_patch_marker(output, destination_path, state_output);
            if !destination_abs.exists() {
                let kind = match apply_mode {
                    ApplyMode::Symlink => {
                        if materialize_as_regular_file(&manifest.target, output, destination_path) {
                            ActionKind::CreateFile
                        } else {
                            ActionKind::CreateSymlink
                        }
                    }
                    _ => ActionKind::CreateFile,
                };
                plans.push(Ok(PlannedAction {
                    output: output.clone(),
                    kind,
                    backup_path: state_output
                        .backup_path
                        .as_ref()
                        .map(|item| project_root.join(item)),
                    patch_marker: None,
                    existed_before: false,
                }));
                continue;
            }

            let drift = destination_abs.exists()
                && sha256_path(&destination_abs)? != state_output.applied_digest;

            if drift {
                // Reconcile on-disk edits (or another target overwriting a shared path) using the
                // requested apply mode instead of refusing until the user deletes managed state.
                match apply_mode {
                    ApplyMode::Takeover => {
                        let backup_path = backup_path(project_root, &manifest.target, output);
                        plans.push(Ok(PlannedAction {
                            output: output.clone(),
                            kind: ActionKind::TakeoverUnmanaged,
                            backup_path: Some(backup_path),
                            patch_marker: None,
                            existed_before: true,
                        }));
                    }
                    ApplyMode::Patch => {
                        if output.kind == GeneratedOutputKind::InstructionFile {
                            if let Some(marker) = managed_instruction_marker.clone() {
                                plans.push(Ok(PlannedAction {
                                    output: output.clone(),
                                    kind: ActionKind::PatchManaged,
                                    backup_path: state_output
                                        .backup_path
                                        .as_ref()
                                        .map(|item| project_root.join(item)),
                                    patch_marker: Some(marker),
                                    existed_before: state_output.existed_before,
                                }));
                            } else {
                                plans.push(Ok(PlannedAction {
                                    output: output.clone(),
                                    kind: ActionKind::OverwriteManaged,
                                    backup_path: state_output
                                        .backup_path
                                        .as_ref()
                                        .map(|item| project_root.join(item)),
                                    patch_marker: None,
                                    existed_before: state_output.existed_before,
                                }));
                            }
                        } else if merge_json {
                            plans.push(Ok(PlannedAction {
                                output: output.clone(),
                                kind: ActionKind::MergeJsonManaged,
                                backup_path: state_output
                                    .backup_path
                                    .as_ref()
                                    .map(|item| project_root.join(item)),
                                patch_marker: None,
                                existed_before: state_output.existed_before,
                            }));
                        } else {
                            plans.push(Ok(PlannedAction {
                                output: output.clone(),
                                kind: ActionKind::OverwriteManaged,
                                backup_path: state_output
                                    .backup_path
                                    .as_ref()
                                    .map(|item| project_root.join(item)),
                                patch_marker: None,
                                existed_before: state_output.existed_before,
                            }));
                        }
                    }
                    ApplyMode::Copy => {
                        let kind = if managed_instruction_marker.is_some() {
                            ActionKind::PatchManaged
                        } else if merge_json {
                            ActionKind::MergeJsonManaged
                        } else {
                            ActionKind::OverwriteManaged
                        };
                        plans.push(Ok(PlannedAction {
                            output: output.clone(),
                            kind,
                            backup_path: state_output
                                .backup_path
                                .as_ref()
                                .map(|item| project_root.join(item)),
                            patch_marker: managed_instruction_marker.clone(),
                            existed_before: state_output.existed_before,
                        }));
                    }
                    ApplyMode::Symlink => {
                        let kind = if managed_instruction_marker.is_some() {
                            ActionKind::PatchManaged
                        } else if materialize_as_regular_file(
                            &manifest.target,
                            output,
                            destination_path,
                        ) {
                            if merge_json {
                                ActionKind::MergeJsonManaged
                            } else {
                                ActionKind::OverwriteManaged
                            }
                        } else {
                            ActionKind::CreateSymlink
                        };
                        plans.push(Ok(PlannedAction {
                            output: output.clone(),
                            kind,
                            backup_path: None,
                            patch_marker: managed_instruction_marker.clone(),
                            existed_before: state_output.existed_before,
                        }));
                    }
                }
                continue;
            }

            let action = if merge_json {
                ActionKind::MergeJsonManaged
            } else if managed_instruction_marker.is_some() {
                ActionKind::PatchManaged
            } else {
                ActionKind::OverwriteManaged
            };
            plans.push(Ok(PlannedAction {
                output: output.clone(),
                kind: action,
                backup_path: state_output
                    .backup_path
                    .as_ref()
                    .map(|item| project_root.join(item)),
                patch_marker: managed_instruction_marker
                    .or_else(|| state_output.patch_marker.clone()),
                existed_before: state_output.existed_before,
            }));
            continue;
        }

        if !destination_abs.exists() {
            let kind = match apply_mode {
                ApplyMode::Symlink => {
                    if materialize_as_regular_file(&manifest.target, output, destination_path) {
                        ActionKind::CreateFile
                    } else {
                        ActionKind::CreateSymlink
                    }
                }
                _ => ActionKind::CreateFile,
            };
            plans.push(Ok(PlannedAction {
                output: output.clone(),
                kind,
                backup_path: None,
                patch_marker: None,
                existed_before: false,
            }));
            continue;
        }

        if matches!(apply_mode, ApplyMode::Patch)
            && sha256_path(&destination_abs)? == sha256_path(&staged_abs)?
        {
            plans.push(Ok(PlannedAction {
                output: output.clone(),
                kind: ActionKind::OverwriteManaged,
                backup_path: None,
                patch_marker: None,
                existed_before: true,
            }));
            continue;
        }

        match apply_mode {
            ApplyMode::Patch if output.kind == GeneratedOutputKind::InstructionFile => {
                let marker = patch_marker_for(output, destination_path);
                let backup_path = backup_path(project_root, &manifest.target, output);
                plans.push(Ok(PlannedAction {
                    output: output.clone(),
                    kind: ActionKind::PatchUnmanaged,
                    backup_path: Some(backup_path),
                    patch_marker: Some(marker),
                    existed_before: true,
                }));
            }
            ApplyMode::Patch if merge_json => {
                let backup_path = backup_path(project_root, &manifest.target, output);
                plans.push(Ok(PlannedAction {
                    output: output.clone(),
                    kind: ActionKind::MergeJsonUnmanaged,
                    backup_path: Some(backup_path),
                    patch_marker: None,
                    existed_before: true,
                }));
            }
            ApplyMode::Patch => {
                let backup_path = backup_path(project_root, &manifest.target, output);
                plans.push(Ok(PlannedAction {
                    output: output.clone(),
                    kind: ActionKind::TakeoverUnmanaged,
                    backup_path: Some(backup_path),
                    patch_marker: None,
                    existed_before: true,
                }));
            }
            ApplyMode::Takeover => {
                let backup_path = backup_path(project_root, &manifest.target, output);
                plans.push(Ok(PlannedAction {
                    output: output.clone(),
                    kind: ActionKind::TakeoverUnmanaged,
                    backup_path: Some(backup_path),
                    patch_marker: None,
                    existed_before: true,
                }));
            }
            _ => {
                let detail = match manifest
                    .brownfield_mode
                    .clone()
                    .unwrap_or(BrownfieldMode::RefuseDueToConflict)
                {
                    BrownfieldMode::PatchMode => {
                        "Unmanaged destination exists and cannot be patched safely."
                    }
                    BrownfieldMode::TakeoverMode => {
                        "Unmanaged destination exists and takeover was not explicitly requested."
                    }
                    _ => "Unmanaged destination exists and metactl refused silent takeover.",
                };
                plans.push(Err(conflict_json(
                    destination_path,
                    ReasonCode::BrownfieldCollision,
                    detail,
                )));
            }
        }
    }
    Ok(plans)
}

fn conflict_json(
    destination_path: impl AsRef<str>,
    reason_code: ReasonCode,
    detail: &str,
) -> anyhow::Error {
    anyhow!(
        "{}",
        serde_json::to_string(&ApplyConflict {
            destination_path: destination_path.as_ref().to_string(),
            reason_code,
            detail: detail.to_string(),
        })
        .unwrap_or_else(|_| "{\"destination_path\":\"\",\"reason_code\":\"validation_failed\",\"detail\":\"unable to encode conflict\"}".to_string())
    )
}

fn destination_path_fallback(output: &GeneratedOutput) -> &str {
    output.destination_path.as_deref().unwrap_or("")
}

fn load_state(path: &Path) -> Result<Option<ManagedState>> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let state =
        serde_json::from_slice(&bytes).with_context(|| format!("decode {}", path.display()))?;
    Ok(Some(state))
}

fn state_path(project_root: &Path, target: &Ref) -> PathBuf {
    project_root
        .join(".metactl")
        .join("state")
        .join(format!("{}.json", target.id))
}

fn backup_root(project_root: &Path, target: &Ref) -> PathBuf {
    project_root
        .join(".metactl")
        .join("state")
        .join("backups")
        .join(&target.id)
}

fn backup_path(project_root: &Path, target: &Ref, output: &GeneratedOutput) -> PathBuf {
    backup_root(project_root, target).join(
        output
            .id
            .clone()
            .unwrap_or_else(|| output.path.replace('/', "_")),
    )
}

fn backup_existing(source: &Path, backup: &Path) -> Result<()> {
    if let Some(parent) = backup.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let bytes = fs::read(source).with_context(|| format!("read {}", source.display()))?;
    atomic_write(backup, &bytes).with_context(|| format!("write {}", backup.display()))?;
    Ok(())
}

fn patch_marker_for(output: &GeneratedOutput, destination_path: &str) -> String {
    output.id.clone().unwrap_or_else(|| {
        destination_path
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
            .collect()
    })
}

fn patch_document(existing: &str, managed: &str, marker: &str) -> Result<String> {
    let begin = format!("<!-- metactl:begin {} -->", marker);
    let end = format!("<!-- metactl:end {} -->", marker);
    if let Some(start) = existing.find(&begin) {
        let tail = &existing[start + begin.len()..];
        let end_offset = tail
            .find(&end)
            .ok_or_else(|| anyhow!("unterminated metactl managed block for {}", marker))?;
        let prefix = &existing[..start];
        let suffix = &tail[end_offset + end.len()..];
        return Ok(format!(
            "{}{}{}{}{}",
            prefix,
            begin,
            if managed.starts_with('\n') { "" } else { "\n" },
            managed.trim_end(),
            format!("\n{}\n{}", end, suffix.trim_start_matches('\n'))
        ));
    }

    let mut patched = existing.trim_end().to_string();
    if !patched.is_empty() {
        patched.push_str("\n\n");
    }
    patched.push_str(&begin);
    patched.push('\n');
    patched.push_str(managed.trim_end());
    patched.push('\n');
    patched.push_str(&end);
    patched.push('\n');
    Ok(patched)
}

fn remove_empty_parents(path: &Path, project_root: &Path) {
    let mut current = path.parent();
    while let Some(dir) = current {
        if dir == project_root {
            break;
        }
        match fs::remove_dir(dir) {
            Ok(_) => current = dir.parent(),
            Err(_) => break,
        }
    }
}

#[cfg(unix)]
fn recreate_symlink(source: &Path, dest: &Path) -> Result<()> {
    let _ = fs::remove_file(dest);
    std::os::unix::fs::symlink(source, dest)
        .with_context(|| format!("symlink {} -> {}", dest.display(), source.display()))
}

#[cfg(not(unix))]
fn recreate_symlink(source: &Path, dest: &Path) -> Result<()> {
    let bytes = fs::read(source).with_context(|| format!("read {}", source.display()))?;
    atomic_write(dest, &bytes).with_context(|| format!("write {}", dest.display()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn sha256_path(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(sha256_bytes(&bytes))
}

fn normalize_relative(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
