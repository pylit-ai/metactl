use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::project::{atomic_write, atomic_write_relaxed};
use crate::types::{
    ApplyConflict, ApplyMode, ApplyReport, ApplyReviewAction, ApplyReviewPlan, BrownfieldMode,
    CapabilityGap, CompileManifest, GeneratedOutput, GeneratedOutputKind,
    InstructionProjectionMode, ReasonCode, Ref, RevertReport, SurfaceMergeStatus,
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
    pub materialize_as_regular_file: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct StageOutputsParams {
    pub inputs: Vec<StagedOutputInput>,
    pub surface_selection_mode: Option<crate::types::SurfaceSelectionMode>,
    pub surface_selection: Vec<crate::types::SurfaceSelectionDecision>,
    pub apply_modes_supported: Vec<ApplyMode>,
    pub brownfield_mode: Option<BrownfieldMode>,
    pub degradations: Vec<CapabilityGap>,
    pub durable: bool,
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
    #[serde(default)]
    source_digest: Option<String>,
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
    Noop,
    CreateFile,
    CreateSymlink,
    OverwriteManaged,
    MergeJsonManaged,
    PatchManaged,
    MergeJsonUnmanaged,
    PatchUnmanaged,
    TakeoverUnmanaged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApplyReuseDecision {
    ReuseExistingManagedOutput,
    RewriteManagedOutput,
}

impl ApplyReuseDecision {
    fn for_managed_output(
        output: &GeneratedOutput,
        apply_mode: &ApplyMode,
        existing_apply_mode: Option<&ApplyMode>,
        merge_json: bool,
        patch_marker: Option<&String>,
        state_output: &ManagedOutputState,
    ) -> Self {
        if existing_apply_mode != Some(apply_mode) {
            return Self::RewriteManagedOutput;
        }
        if !source_digest_matches(output, state_output, merge_json, patch_marker) {
            return Self::RewriteManagedOutput;
        }
        Self::ReuseExistingManagedOutput
    }

    fn can_reuse_existing_output(self) -> bool {
        matches!(self, Self::ReuseExistingManagedOutput)
    }
}

#[derive(Debug, Clone)]
struct PlannedAction {
    output: GeneratedOutput,
    kind: ActionKind,
    backup_path: Option<PathBuf>,
    patch_marker: Option<String>,
    existed_before: bool,
}

#[derive(Debug, Clone)]
enum SnapshotValue {
    Missing,
    File(Vec<u8>),
    Symlink(PathBuf),
}

#[derive(Debug, Clone)]
struct PathSnapshot {
    relative_path: String,
    value: SnapshotValue,
}

#[derive(Debug, Serialize)]
struct ApplyJournalEntry<'a> {
    schema_version: &'static str,
    plan_digest: &'a str,
    destination_path: &'a str,
    status: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    applied_digest: Option<&'a str>,
}

pub(crate) fn stage_outputs(
    project_root: &Path,
    target: &Ref,
    params: StageOutputsParams,
) -> Result<CompileManifest> {
    validate_relative_output_path("target id", &target.id)?;
    let stage_root = project_root
        .join(".metactl")
        .join("generated")
        .join(&target.id);
    ensure_contained_regular_path(
        project_root,
        Path::new(".metactl")
            .join("generated")
            .join(&target.id)
            .as_path(),
        true,
    )?;
    fs::create_dir_all(&stage_root).with_context(|| format!("create {}", stage_root.display()))?;
    let manifest_path = stage_root.join("compile.manifest.json");
    let previous_manifest = load_compile_manifest_if_present(&manifest_path)?;
    if let Some(previous) = &previous_manifest {
        if previous.target != *target {
            return Err(anyhow!(
                "previous compile manifest target {} does not match staging target {}",
                previous.target.id,
                target.id
            ));
        }
    }

    let mut outputs = Vec::new();
    let mut seen_destinations = BTreeSet::new();
    for input in params.inputs {
        validate_relative_output_path("generated destination", &input.destination_path)?;
        ensure_platform_unique_path(&mut seen_destinations, &input.destination_path)?;
        let relative_stage_path = Path::new(".metactl")
            .join("generated")
            .join(&target.id)
            .join(&input.destination_path);
        let stage_path = project_root.join(&relative_stage_path);
        ensure_contained_regular_path(project_root, &relative_stage_path, true)?;
        if let Some(parent) = stage_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        write_staged_if_changed(&stage_path, &input.contents, params.durable)
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
            materialize_as_regular_file: input.materialize_as_regular_file,
            managed: true,
        });
    }

    let expected_paths = outputs
        .iter()
        .map(|output| output.path.clone())
        .collect::<BTreeSet<_>>();
    let pruned_outputs = prune_stale_outputs(
        project_root,
        &stage_root,
        previous_manifest.as_ref(),
        &expected_paths,
    )?;

    let manifest = CompileManifest {
        api_version: crate::types::API_VERSION.to_string(),
        target: target.clone(),
        generated_outputs: outputs,
        pruned_outputs,
        surface_selection_mode: params.surface_selection_mode,
        surface_selection: params.surface_selection,
        apply_modes_supported: params.apply_modes_supported,
        brownfield_mode: params.brownfield_mode,
        degradations: params.degradations,
    };

    atomic_write(
        &manifest_path,
        &serde_json::to_vec_pretty(&manifest).context("serialize compile manifest")?,
    )
    .with_context(|| format!("write {}", manifest_path.display()))?;

    Ok(manifest)
}

fn load_compile_manifest_if_present(path: &Path) -> Result<Option<CompileManifest>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .with_context(|| format!("parse previous compile manifest {}", path.display()))
            .map(Some),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("read {}", path.display())),
    }
}

fn prune_stale_outputs(
    project_root: &Path,
    stage_root: &Path,
    previous_manifest: Option<&CompileManifest>,
    expected_paths: &BTreeSet<String>,
) -> Result<Vec<String>> {
    let Some(previous_manifest) = previous_manifest else {
        return Ok(Vec::new());
    };
    let mut pruned = Vec::new();
    for output in &previous_manifest.generated_outputs {
        if expected_paths.contains(&output.path) {
            continue;
        }
        let stale_path = safe_staged_output_path(project_root, stage_root, &output.path)?;
        match fs::symlink_metadata(&stale_path) {
            Ok(metadata) if metadata.file_type().is_dir() => {
                return Err(anyhow!(
                    "refusing to prune directory recorded as generated output: {}",
                    stale_path.display()
                ));
            }
            Ok(_) => {
                fs::remove_file(&stale_path)
                    .with_context(|| format!("prune stale output {}", stale_path.display()))?;
                pruned.push(output.path.clone());
                prune_empty_staging_parents(stale_path.parent(), stage_root)?;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("stat stale output {}", stale_path.display()));
            }
        }
    }
    pruned.sort();
    pruned.dedup();
    Ok(pruned)
}

fn safe_staged_output_path(
    project_root: &Path,
    stage_root: &Path,
    output_path: &str,
) -> Result<PathBuf> {
    let relative = Path::new(output_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(anyhow!(
            "refusing to prune unsafe generated output path '{}'",
            output_path
        ));
    }
    let absolute = project_root.join(relative);
    if absolute == stage_root || !absolute.starts_with(stage_root) {
        return Err(anyhow!(
            "refusing to prune generated output outside target staging root: {}",
            absolute.display()
        ));
    }
    Ok(absolute)
}

fn prune_empty_staging_parents(mut parent: Option<&Path>, stage_root: &Path) -> Result<()> {
    while let Some(directory) = parent {
        if directory == stage_root || !directory.starts_with(stage_root) {
            break;
        }
        let mut entries = fs::read_dir(directory)
            .with_context(|| format!("inspect staging directory {}", directory.display()))?;
        if entries.next().transpose()?.is_some() {
            break;
        }
        fs::remove_dir(directory)
            .with_context(|| format!("remove empty staging directory {}", directory.display()))?;
        parent = directory.parent();
    }
    Ok(())
}

pub(crate) fn build_apply_review_plan(
    project_root: &Path,
    manifest: &CompileManifest,
    apply_mode: &ApplyMode,
) -> Result<ApplyReviewPlan> {
    let state_path = state_path(project_root, &manifest.target);
    let state_bytes = fs::read(&state_path).ok();
    let managed = load_state(&state_path)?
        .map(|state| {
            state
                .outputs
                .into_iter()
                .map(|output| output.destination_path)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let canonical_project = project_root
        .canonicalize()
        .with_context(|| format!("canonicalize project root {}", project_root.display()))?;
    let project_identity = sha256_bytes(canonical_project.to_string_lossy().as_bytes());
    let manifest_bytes = serde_json::to_vec(manifest).context("serialize compile manifest")?;
    let mut actions = Vec::new();
    let mut equivalent_paths = BTreeSet::new();
    for output in &manifest.generated_outputs {
        let destination = output
            .destination_path
            .as_deref()
            .ok_or_else(|| anyhow!("generated output is missing destination_path"))?;
        validate_relative_output_path("generated destination", destination)?;
        ensure_platform_unique_path(&mut equivalent_paths, destination)?;
        validate_relative_output_path("staged output", &output.path)?;
        ensure_staged_path_for_target(&output.path, &manifest.target.id)?;
        ensure_contained_regular_path(project_root, Path::new(&output.path), false)?;
        ensure_contained_regular_path(project_root, Path::new(destination), true)?;

        let destination_abs = project_root.join(destination);
        let destination_is_managed = managed.contains(destination);
        let before_digest = digest_if_regular(&destination_abs, destination_is_managed)?;
        let before_path_identity = path_identity(project_root, &destination_abs)?;
        let legacy_path = legacy_codex_path(destination);
        let legacy_digest = match legacy_path.as_deref() {
            Some(path) => {
                validate_relative_output_path("legacy Codex source", path)?;
                ensure_contained_regular_path(project_root, Path::new(path), true)?;
                digest_if_regular(&project_root.join(path), false)?
            }
            None => None,
        };
        let staged_digest = digest_if_regular(&project_root.join(&output.path), false)?;
        let destination_exists = destination_abs.exists();
        let (classification, reason_code, consequence, approval_required) = classify_review_action(
            destination_is_managed,
            destination_exists,
            before_digest.as_deref(),
            legacy_digest.as_deref(),
            staged_digest.as_deref(),
            legacy_path.is_some(),
        );
        actions.push(ApplyReviewAction {
            destination_path: destination.to_string(),
            staged_path: output.path.clone(),
            classification: classification.to_string(),
            reason_code: reason_code.to_string(),
            consequence: consequence.to_string(),
            approval_required,
            desired_digest: staged_digest.unwrap_or_default(),
            before_digest,
            before_path_identity,
            legacy_path,
            legacy_digest,
        });
    }
    actions.sort_by(|a, b| a.destination_path.cmp(&b.destination_path));
    let mut plan = ApplyReviewPlan {
        schema_version: "metactl.apply-plan.v1".to_string(),
        target: manifest.target.clone(),
        target_version: manifest.target.version.clone().unwrap_or_default(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        project_identity,
        apply_mode: apply_mode.clone(),
        manifest_digest: sha256_bytes(&manifest_bytes),
        managed_state_digest: state_bytes.as_deref().map(sha256_bytes),
        actions,
        digest: String::new(),
    };
    plan.digest = review_plan_digest(&plan)?;
    Ok(plan)
}

fn review_plan_digest(plan: &ApplyReviewPlan) -> Result<String> {
    let mut semantic = plan.clone();
    semantic.digest.clear();
    Ok(sha256_bytes(
        &serde_json::to_vec(&semantic).context("serialize semantic apply plan")?,
    ))
}

fn classify_review_action<'a>(
    managed: bool,
    destination_exists: bool,
    before_digest: Option<&str>,
    legacy_digest: Option<&str>,
    staged_digest: Option<&str>,
    has_legacy_path: bool,
) -> (&'a str, &'a str, &'a str, bool) {
    if managed {
        return (
            "canonical_only",
            "managed_canonical",
            "Update or retain the metactl-managed canonical output.",
            false,
        );
    }
    if destination_exists {
        if has_legacy_path && before_digest.is_some() && before_digest == legacy_digest {
            return (
                "identical_duplicate",
                "identical_cross_root_bytes",
                "Preserve both roots; canonical bytes already exist.",
                false,
            );
        }
        return (
            "unmanaged_canonical",
            "unmanaged_destination",
            "Preserve user-owned canonical bytes; explicit adoption is required.",
            true,
        );
    }
    if has_legacy_path && legacy_digest.is_some() {
        if legacy_digest == staged_digest {
            return (
                "legacy_only",
                "legacy_compatible",
                "Create the canonical output and preserve the legacy bytes.",
                false,
            );
        }
        return (
            "content_conflict",
            "legacy_content_conflict",
            "Preserve legacy bytes and refuse a shadowing canonical write.",
            true,
        );
    }
    (
        "new",
        "canonical_create",
        "Create the canonical output.",
        false,
    )
}

fn write_staged_if_changed(path: &Path, contents: &[u8], durable: bool) -> Result<()> {
    if matches!(fs::read(path), Ok(existing) if existing == contents) {
        return Ok(());
    }
    if durable {
        atomic_write(path, contents)
    } else {
        atomic_write_relaxed(path, contents)
    }
}

fn apply_journal_path(project_root: &Path, target: &Ref, plan_digest: &str) -> PathBuf {
    let digest = plan_digest.strip_prefix("sha256:").unwrap_or(plan_digest);
    project_root
        .join(".metactl")
        .join("state")
        .join("apply-journal")
        .join(&target.id)
        .join(format!("{digest}.jsonl"))
}

fn append_apply_journal(
    journal_path: &Path,
    plan_digest: &str,
    destination_path: &str,
    status: &str,
    applied_digest: Option<&str>,
) -> Result<()> {
    if let Some(parent) = journal_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(journal_path)
        .with_context(|| format!("open {}", journal_path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .with_context(|| format!("restrict permissions on {}", journal_path.display()))?;
    }
    let entry = ApplyJournalEntry {
        schema_version: "metactl.apply-journal.v1",
        plan_digest,
        destination_path,
        status,
        applied_digest,
    };
    serde_json::to_writer(&mut file, &entry).context("serialize apply journal entry")?;
    file.write_all(b"\n")
        .with_context(|| format!("append {}", journal_path.display()))?;
    file.sync_all()
        .with_context(|| format!("sync {}", journal_path.display()))?;
    Ok(())
}

fn capture_path_snapshot(project_root: &Path, relative_path: &str) -> Result<PathSnapshot> {
    validate_relative_output_path("snapshot", relative_path)?;
    ensure_contained_regular_path(project_root, Path::new(relative_path), true)?;
    let absolute = project_root.join(relative_path);
    let value = match fs::symlink_metadata(&absolute) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let target = fs::read_link(&absolute)
                .with_context(|| format!("read link {}", absolute.display()))?;
            ensure_internal_symlink_target(project_root, &absolute, &target)?;
            SnapshotValue::Symlink(target)
        }
        Ok(metadata) if metadata.is_file() => SnapshotValue::File(
            fs::read(&absolute).with_context(|| format!("read {}", absolute.display()))?,
        ),
        Ok(_) => {
            return Err(anyhow!(
                "unsafe snapshot path '{}' is not a regular file",
                relative_path
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => SnapshotValue::Missing,
        Err(error) => return Err(error).with_context(|| format!("inspect {}", absolute.display())),
    };
    Ok(PathSnapshot {
        relative_path: relative_path.to_string(),
        value,
    })
}

fn ensure_internal_symlink_target(project_root: &Path, link: &Path, target: &Path) -> Result<()> {
    let resolved = if target.is_absolute() {
        target.to_path_buf()
    } else {
        link.parent()
            .ok_or_else(|| anyhow!("unsafe_alias: link has no parent"))?
            .join(target)
    };
    let root = fs::canonicalize(project_root)
        .with_context(|| format!("canonicalize {}", project_root.display()))?;
    let resolved = fs::canonicalize(&resolved)
        .with_context(|| format!("resolve managed link {}", link.display()))?;
    if !resolved.starts_with(&root) || !resolved.is_file() {
        return Err(anyhow!(
            "unsafe_alias: managed link target must be a regular file inside the project"
        ));
    }
    Ok(())
}

fn restore_path_snapshots(project_root: &Path, snapshots: &[PathSnapshot]) -> Vec<String> {
    let mut failed = Vec::new();
    for snapshot in snapshots.iter().rev() {
        if restore_path_snapshot(project_root, snapshot).is_err() {
            failed.push(snapshot.relative_path.clone());
        }
    }
    failed
}

fn restore_path_snapshot(project_root: &Path, snapshot: &PathSnapshot) -> Result<()> {
    validate_relative_output_path("restore", &snapshot.relative_path)?;
    ensure_contained_regular_path(project_root, Path::new(&snapshot.relative_path), true)?;
    let absolute = project_root.join(&snapshot.relative_path);
    match fs::symlink_metadata(&absolute) {
        Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
            fs::remove_file(&absolute).with_context(|| format!("remove {}", absolute.display()))?;
        }
        Ok(_) => {
            return Err(anyhow!(
                "unsafe restore path '{}' is not a regular file",
                snapshot.relative_path
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).with_context(|| format!("inspect {}", absolute.display())),
    }
    match &snapshot.value {
        SnapshotValue::Missing => {
            remove_empty_parents(&absolute, project_root);
        }
        SnapshotValue::File(bytes) => {
            if let Some(parent) = absolute.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create {}", parent.display()))?;
            }
            atomic_write(&absolute, bytes)
                .with_context(|| format!("restore {}", absolute.display()))?;
        }
        SnapshotValue::Symlink(target) => {
            if let Some(parent) = absolute.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create {}", parent.display()))?;
            }
            ensure_internal_symlink_target(project_root, &absolute, target)?;
            restore_symlink(target, &absolute)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn restore_symlink(target: &Path, destination: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, destination)
        .with_context(|| format!("restore symlink {}", destination.display()))
}

#[cfg(not(unix))]
fn restore_symlink(_target: &Path, destination: &Path) -> Result<()> {
    Err(anyhow!(
        "restoring symbolic links is unsupported on this platform: {}",
        destination.display()
    ))
}

fn managed_output_state(
    plan: &PlannedAction,
    applied_digest: String,
    project_root: &Path,
) -> ManagedOutputState {
    ManagedOutputState {
        id: plan.output.id.clone(),
        staged_path: plan.output.path.clone(),
        destination_path: plan.output.destination_path.clone().unwrap_or_default(),
        applied_digest,
        source_digest: plan.output.digest.clone(),
        backup_path: plan
            .backup_path
            .as_ref()
            .map(|path| normalize_relative(path.strip_prefix(project_root).unwrap_or(path))),
        existed_before: plan.existed_before,
        patch_marker: plan.patch_marker.clone(),
        instruction_mode: plan.output.instruction_mode.clone(),
        pack_ref: plan.output.pack_ref.clone(),
        surface_id: plan.output.surface_id.clone(),
        surface_slug: plan.output.surface_slug.clone(),
        source_resource_paths: plan.output.source_resource_paths.clone(),
        merge_status: plan.output.merge_status.clone(),
        degradation_codes: plan.output.degradation_codes.clone(),
        ownership_token: plan.output.ownership_token.clone(),
    }
}

fn failed_apply_report(
    project_root: &Path,
    target: &Ref,
    state_path: &Path,
    journal_path: &Path,
    attempted_path: &str,
    applied_count: usize,
    failed_restores: &[String],
) -> ApplyReport {
    let journal_relative = normalize_relative(
        journal_path
            .strip_prefix(project_root)
            .unwrap_or(journal_path),
    );
    let (status, detail) = if failed_restores.is_empty() {
        (
            "compensated_failure",
            format!(
                "compensated_failure: apply stopped at '{}'; restored {} prior mutation(s); journal {}",
                attempted_path, applied_count, journal_relative
            ),
        )
    } else {
        (
            "failed_partial",
            format!(
                "failed_partial: apply stopped at '{}'; compensation could not restore [{}]; journal {}",
                attempted_path,
                failed_restores.join(", "),
                journal_relative
            ),
        )
    };
    ApplyReport {
        target: target.clone(),
        applied_paths: Vec::new(),
        conflicts: vec![ApplyConflict {
            destination_path: attempted_path.to_string(),
            reason_code: ReasonCode::ConflictDetected,
            detail: format!(
                "{status}: {}",
                detail.trim_start_matches(&format!("{status}: "))
            ),
        }],
        state_path: normalize_relative(state_path.strip_prefix(project_root).unwrap_or(state_path)),
    }
}

pub(crate) fn apply_manifest(
    project_root: &Path,
    manifest: &CompileManifest,
    apply_mode: &ApplyMode,
) -> Result<ApplyReport> {
    apply_manifest_bound(project_root, manifest, apply_mode, None)
}

pub(crate) fn apply_manifest_bound(
    project_root: &Path,
    manifest: &CompileManifest,
    apply_mode: &ApplyMode,
    expected_plan_digest: Option<&str>,
) -> Result<ApplyReport> {
    let review_plan = build_apply_review_plan(project_root, manifest, apply_mode)?;
    if let Some(expected) = expected_plan_digest {
        if expected != review_plan.digest {
            return Ok(ApplyReport {
                target: manifest.target.clone(),
                applied_paths: Vec::new(),
                conflicts: vec![ApplyConflict {
                    destination_path: String::new(),
                    reason_code: ReasonCode::ConflictDetected,
                    detail: format!(
                        "stale_plan: expected plan digest {expected}, current digest {}; run preview again",
                        review_plan.digest
                    ),
                }],
                state_path: normalize_relative(
                    state_path(project_root, &manifest.target)
                        .strip_prefix(project_root)
                        .unwrap_or_else(|_| Path::new(".metactl/state")),
                ),
            });
        }
    }
    let review_conflicts = review_plan
        .actions
        .iter()
        .filter(|action| action.classification == "content_conflict")
        .map(|action| ApplyConflict {
            destination_path: action.destination_path.clone(),
            reason_code: ReasonCode::BrownfieldCollision,
            detail: format!(
                "{}: {}; legacy bytes were preserved",
                action.reason_code, action.consequence
            ),
        })
        .collect::<Vec<_>>();
    if !review_conflicts.is_empty() {
        return Ok(ApplyReport {
            target: manifest.target.clone(),
            applied_paths: Vec::new(),
            conflicts: review_conflicts,
            state_path: normalize_relative(
                state_path(project_root, &manifest.target)
                    .strip_prefix(project_root)
                    .unwrap_or_else(|_| Path::new(".metactl/state")),
            ),
        });
    }
    let state_path = state_path(project_root, &manifest.target);
    let existing_state = load_state(&state_path)?;
    let plans = plan_apply(project_root, manifest, apply_mode, existing_state.as_ref())?;

    if let Some(conflicts) = collect_conflicts(&plans) {
        return Ok(ApplyReport {
            target: manifest.target.clone(),
            applied_paths: Vec::new(),
            conflicts,
            state_path: normalize_relative(
                state_path
                    .strip_prefix(project_root)
                    .unwrap_or(state_path.as_path()),
            ),
        });
    }

    for plan in plans.iter().filter_map(|plan| plan.as_ref().ok()) {
        let destination_path = plan
            .output
            .destination_path
            .as_deref()
            .ok_or_else(|| anyhow!("generated output is missing destination_path"))?;
        validate_relative_output_path("generated destination", destination_path)?;
        validate_relative_output_path("staged output", &plan.output.path)?;
        ensure_staged_path_for_target(&plan.output.path, &manifest.target.id)?;
        ensure_contained_regular_path(project_root, Path::new(destination_path), true)?;
        ensure_contained_regular_path(project_root, Path::new(&plan.output.path), false)?;
        if let Some(backup_path) = &plan.backup_path {
            let backup_relative = backup_path.strip_prefix(project_root).map_err(|_| {
                anyhow!(
                    "backup path '{}' escapes project root",
                    backup_path.display()
                )
            })?;
            ensure_contained_regular_path(project_root, backup_relative, true)?;
        }
    }

    let journal_path = apply_journal_path(project_root, &manifest.target, &review_plan.digest);
    append_apply_journal(&journal_path, &review_plan.digest, "", "preflight_ok", None)?;

    // Capture every path before the first mutation. This makes a later failure
    // compensatable without trusting state that changed during the apply loop.
    let mut snapshots = Vec::new();
    let mut snapshotted_paths = BTreeSet::new();
    for plan in plans.iter().filter_map(|plan| plan.as_ref().ok()) {
        let destination_path = plan
            .output
            .destination_path
            .as_deref()
            .ok_or_else(|| anyhow!("generated output is missing destination_path"))?;
        if snapshotted_paths.insert(destination_path.to_string()) {
            snapshots.push(capture_path_snapshot(project_root, destination_path)?);
        }
        if let Some(backup_path) = &plan.backup_path {
            let backup_relative =
                normalize_relative(backup_path.strip_prefix(project_root).map_err(|_| {
                    anyhow!(
                        "backup path '{}' escapes project root",
                        backup_path.display()
                    )
                })?);
            if snapshotted_paths.insert(backup_relative.clone()) {
                snapshots.push(capture_path_snapshot(project_root, &backup_relative)?);
            }
        }
    }
    let state_relative = normalize_relative(
        state_path
            .strip_prefix(project_root)
            .unwrap_or(state_path.as_path()),
    );
    if snapshotted_paths.insert(state_relative.clone()) {
        snapshots.push(capture_path_snapshot(project_root, &state_relative)?);
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

        ensure_contained_regular_path(project_root, Path::new(destination_path), true)?;
        ensure_contained_regular_path(project_root, Path::new(&plan.output.path), false)?;

        if matches!(plan.kind, ActionKind::Noop) {
            let applied_digest = sha256_path(&destination_abs)?;
            append_apply_journal(
                &journal_path,
                &review_plan.digest,
                destination_path,
                "noop",
                Some(&applied_digest),
            )?;
            applied_paths.push(destination_path.clone());
            state_outputs.push(managed_output_state(&plan, applied_digest, project_root));
            continue;
        }
        let action_result = (|| -> Result<String> {
            append_apply_journal(
                &journal_path,
                &review_plan.digest,
                destination_path,
                "started",
                None,
            )?;
            if let Some(parent) = destination_abs.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create {}", parent.display()))?;
            }
            let staged_bytes =
                fs::read(&staged_abs).with_context(|| format!("read {}", staged_abs.display()))?;

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
                ActionKind::Noop => unreachable!("no-op plans are handled before writes"),
                ActionKind::CreateFile | ActionKind::TakeoverUnmanaged => {
                    // `fs::write` follows symlinks; replacing a managed symlink with a regular
                    // file requires removing the link first.
                    if destination_abs.is_symlink() {
                        fs::remove_file(&destination_abs).with_context(|| {
                            format!("remove symlink {}", destination_abs.display())
                        })?;
                    }
                    atomic_write(&destination_abs, &staged_bytes)
                        .with_context(|| format!("write {}", destination_abs.display()))?;
                }
                ActionKind::OverwriteManaged => {
                    let replace_link = !matches!(apply_mode, ApplyMode::Symlink)
                        || materialize_as_regular_file(&plan.output, destination_path);
                    if replace_link && destination_abs.is_symlink() {
                        fs::remove_file(&destination_abs).with_context(|| {
                            format!("remove symlink {}", destination_abs.display())
                        })?;
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
                    let staged = String::from_utf8(staged_bytes.clone()).map_err(|_| {
                        anyhow!("staged output {} is not utf-8", staged_abs.display())
                    })?;
                    let merged = merge_json_document(destination_path, &existing, &staged)?;
                    if destination_abs.is_symlink() {
                        fs::remove_file(&destination_abs).with_context(|| {
                            format!("remove symlink {}", destination_abs.display())
                        })?;
                    }
                    atomic_write(&destination_abs, merged.as_bytes())
                        .with_context(|| format!("write {}", destination_abs.display()))?;
                }
                ActionKind::PatchManaged | ActionKind::PatchUnmanaged => {
                    let existing = fs::read_to_string(&destination_abs)
                        .with_context(|| format!("read {}", destination_abs.display()))?;
                    let staged = String::from_utf8(staged_bytes.clone()).map_err(|_| {
                        anyhow!("staged output {} is not utf-8", staged_abs.display())
                    })?;
                    let marker = patch_marker
                        .as_deref()
                        .ok_or_else(|| anyhow!("patch apply missing marker"))?;
                    let patched = patch_document(&existing, &staged, marker)?;
                    atomic_write(&destination_abs, patched.as_bytes())
                        .with_context(|| format!("write {}", destination_abs.display()))?;
                }
            }

            let applied_digest = sha256_path(&destination_abs)?;
            append_apply_journal(
                &journal_path,
                &review_plan.digest,
                destination_path,
                "applied",
                Some(&applied_digest),
            )?;
            Ok(applied_digest)
        })();

        let applied_digest = match action_result {
            Ok(digest) => digest,
            Err(_) => {
                let failed_restores = restore_path_snapshots(project_root, &snapshots);
                let status = if failed_restores.is_empty() {
                    "failure_compensated"
                } else {
                    "failed_partial"
                };
                let _ = append_apply_journal(
                    &journal_path,
                    &review_plan.digest,
                    destination_path,
                    status,
                    None,
                );
                return Ok(failed_apply_report(
                    project_root,
                    &manifest.target,
                    &state_path,
                    &journal_path,
                    destination_path,
                    applied_paths.len(),
                    &failed_restores,
                ));
            }
        };
        applied_paths.push(destination_path.clone());
        state_outputs.push(managed_output_state(&plan, applied_digest, project_root));
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
    let write_state_result = atomic_write(
        &state_path,
        &serde_json::to_vec_pretty(&state).context("serialize managed state")?,
    )
    .with_context(|| format!("write {}", state_path.display()));
    if write_state_result.is_err() {
        let failed_restores = restore_path_snapshots(project_root, &snapshots);
        let status = if failed_restores.is_empty() {
            "failure_compensated"
        } else {
            "failed_partial"
        };
        let _ = append_apply_journal(
            &journal_path,
            &review_plan.digest,
            &state_relative,
            status,
            None,
        );
        return Ok(failed_apply_report(
            project_root,
            &manifest.target,
            &state_path,
            &journal_path,
            &state_relative,
            applied_paths.len(),
            &failed_restores,
        ));
    }
    if append_apply_journal(&journal_path, &review_plan.digest, "", "success", None).is_err() {
        let failed_restores = restore_path_snapshots(project_root, &snapshots);
        return Ok(failed_apply_report(
            project_root,
            &manifest.target,
            &state_path,
            &journal_path,
            &state_relative,
            applied_paths.len(),
            &failed_restores,
        ));
    }

    Ok(ApplyReport {
        target: manifest.target.clone(),
        applied_paths,
        conflicts: Vec::new(),
        state_path: normalize_relative(
            state_path
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
                state_path
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
            state_path
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

/// Some runtime surfaces must stay as real files even when apply mode is `Symlink`.
/// Target data marks those outputs with `materialize_as_regular_file` so the
/// materializer does not need per-target filesystem exceptions.
fn materialize_as_regular_file(output: &GeneratedOutput, destination_path: &str) -> bool {
    matches!(output.kind, GeneratedOutputKind::InstructionFile)
        || output.materialize_as_regular_file
        || structured_json_merge_output(output, destination_path)
}

fn can_skip_managed_apply(
    output: &GeneratedOutput,
    apply_mode: &ApplyMode,
    existing_apply_mode: Option<&ApplyMode>,
    merge_json: bool,
    patch_marker: Option<&String>,
    state_output: &ManagedOutputState,
) -> bool {
    ApplyReuseDecision::for_managed_output(
        output,
        apply_mode,
        existing_apply_mode,
        merge_json,
        patch_marker,
        state_output,
    )
    .can_reuse_existing_output()
}

fn source_digest_matches(
    output: &GeneratedOutput,
    state_output: &ManagedOutputState,
    merge_json: bool,
    patch_marker: Option<&String>,
) -> bool {
    match (
        state_output.source_digest.as_deref(),
        output.digest.as_deref(),
    ) {
        (Some(previous), Some(current)) => previous == current,
        (None, Some(current)) if !merge_json && patch_marker.is_none() => {
            current == state_output.applied_digest
        }
        _ => false,
    }
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
    let existing_apply_mode = existing_state.map(|state| state.apply_mode.clone());
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
                        if materialize_as_regular_file(output, destination_path) {
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

            let actual_digest = sha256_path(&destination_abs)?;
            let drift = destination_abs.exists() && actual_digest != state_output.applied_digest;

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
                    ApplyMode::Copy | ApplyMode::ImportStub => {
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
                        } else if materialize_as_regular_file(output, destination_path) {
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

            if can_skip_managed_apply(
                output,
                apply_mode,
                existing_apply_mode.as_ref(),
                merge_json,
                managed_instruction_marker.as_ref(),
                state_output,
            ) {
                plans.push(Ok(PlannedAction {
                    output: output.clone(),
                    kind: ActionKind::Noop,
                    backup_path: state_output
                        .backup_path
                        .as_ref()
                        .map(|item| project_root.join(item)),
                    patch_marker: state_output.patch_marker.clone(),
                    existed_before: state_output.existed_before,
                }));
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
                    if materialize_as_regular_file(output, destination_path) {
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(backup, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("restrict permissions on {}", backup.display()))?;
    }
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
            "{}{}{}{}\n{}\n{}",
            prefix,
            begin,
            if managed.starts_with('\n') { "" } else { "\n" },
            managed.trim_end(),
            end,
            suffix.trim_start_matches('\n')
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

fn validate_relative_output_path(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains("//")
        || !value.is_ascii()
    {
        return Err(anyhow!(
            "unsafe {label} path '{value}': paths must be non-empty relative ASCII paths"
        ));
    }
    for component in Path::new(value).components() {
        match component {
            std::path::Component::Normal(part)
                if !part.is_empty() && part != "." && part != ".." => {}
            _ => {
                return Err(anyhow!(
                    "unsafe {label} path '{value}': absolute, parent, and ambiguous components are forbidden"
                ))
            }
        }
    }
    Ok(())
}

fn ensure_platform_unique_path(seen: &mut BTreeSet<String>, value: &str) -> Result<()> {
    let key = value.to_ascii_lowercase();
    if !seen.insert(key) {
        return Err(anyhow!(
            "platform-equivalent generated destination collision for '{value}'"
        ));
    }
    Ok(())
}

fn ensure_staged_path_for_target(path: &str, target_id: &str) -> Result<()> {
    let required = format!(".metactl/generated/{target_id}/");
    if !path.starts_with(&required) || path.len() == required.len() {
        return Err(anyhow!(
            "staged output '{path}' is outside the target-owned generated root '{required}'"
        ));
    }
    Ok(())
}

fn ensure_contained_regular_path(
    project_root: &Path,
    relative: &Path,
    leaf_may_be_missing: bool,
) -> Result<()> {
    let relative_text = normalize_relative(relative);
    validate_relative_output_path("contained", &relative_text)?;
    let mut current = project_root.to_path_buf();
    let component_count = relative.components().count();
    for (index, component) in relative.components().enumerate() {
        let std::path::Component::Normal(part) = component else {
            return Err(anyhow!("unsafe contained path '{relative_text}'"));
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata)
                if metadata.file_type().is_symlink()
                    && !(index + 1 == component_count && leaf_may_be_missing) =>
            {
                return Err(anyhow!(
                    "unsafe_alias: '{}' traverses a symbolic link",
                    relative_text
                ));
            }
            Ok(metadata) if index + 1 < component_count && !metadata.is_dir() => {
                return Err(anyhow!(
                    "unsafe contained path '{}': ancestor '{}' is not a directory",
                    relative_text,
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound && leaf_may_be_missing => break,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(anyhow!("contained path '{}' is missing", relative_text));
            }
            Err(err) => return Err(err).with_context(|| format!("inspect {}", current.display())),
        }
    }
    Ok(())
}

fn digest_if_regular(path: &Path, allow_symlink: bool) -> Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() && !allow_symlink => Err(anyhow!(
            "unsafe_alias: '{}' is a symbolic link",
            path.display()
        )),
        Ok(metadata) if metadata.file_type().is_symlink() => sha256_path(path).map(Some),
        Ok(metadata) if metadata.is_file() => sha256_path(path).map(Some),
        Ok(_) => Err(anyhow!(
            "unsafe output path '{}' is not a regular file",
            path.display()
        )),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("inspect {}", path.display())),
    }
}

fn path_identity(project_root: &Path, path: &Path) -> Result<String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let target =
                fs::read_link(path).with_context(|| format!("read link {}", path.display()))?;
            ensure_internal_symlink_target(project_root, path, &target)?;
            let resolved = if target.is_absolute() {
                target
            } else {
                path.parent()
                    .ok_or_else(|| anyhow!("unsafe_alias: link has no parent"))?
                    .join(target)
            };
            let resolved = fs::canonicalize(&resolved)
                .with_context(|| format!("resolve managed link {}", path.display()))?;
            let root = fs::canonicalize(project_root)
                .with_context(|| format!("canonicalize {}", project_root.display()))?;
            let relative = resolved.strip_prefix(root).map_err(|_| {
                anyhow!("unsafe_alias: managed link target must remain inside the project")
            })?;
            Ok(format!("symlink:{}", normalize_relative(relative)))
        }
        Ok(metadata) if metadata.is_file() => Ok("file".to_string()),
        Ok(_) => Err(anyhow!(
            "unsafe output path '{}' is not a regular file",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok("missing".to_string()),
        Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
    }
}

fn legacy_codex_path(destination: &str) -> Option<String> {
    destination
        .strip_prefix(".agents/skills/")
        .map(|suffix| format!(".codex/skills/{suffix}"))
}

fn normalize_relative(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RefKind;

    fn generated_output_with_digest(digest: Option<&str>) -> GeneratedOutput {
        GeneratedOutput {
            id: Some("output".to_string()),
            path: ".metactl/generated/codex-cli/AGENTS.md".to_string(),
            destination_path: Some("AGENTS.md".to_string()),
            kind: GeneratedOutputKind::InstructionFile,
            digest: digest.map(str::to_string),
            instruction_mode: None,
            pack_ref: None,
            surface_id: None,
            surface_slug: None,
            source_resource_paths: Vec::new(),
            merge_status: None,
            degradation_codes: Vec::new(),
            ownership_token: None,
            materialize_as_regular_file: false,
            managed: true,
        }
    }

    fn managed_state_output(
        source_digest: Option<&str>,
        applied_digest: &str,
    ) -> ManagedOutputState {
        ManagedOutputState {
            id: Some("output".to_string()),
            staged_path: ".metactl/generated/codex-cli/AGENTS.md".to_string(),
            destination_path: "AGENTS.md".to_string(),
            applied_digest: applied_digest.to_string(),
            source_digest: source_digest.map(str::to_string),
            backup_path: None,
            existed_before: true,
            patch_marker: None,
            instruction_mode: None,
            pack_ref: None,
            surface_id: None,
            surface_slug: None,
            source_resource_paths: Vec::new(),
            merge_status: None,
            degradation_codes: Vec::new(),
            ownership_token: None,
        }
    }

    fn reuse_decision(
        output: &GeneratedOutput,
        state_output: &ManagedOutputState,
    ) -> ApplyReuseDecision {
        ApplyReuseDecision::for_managed_output(
            output,
            &ApplyMode::Copy,
            Some(&ApplyMode::Copy),
            false,
            None,
            state_output,
        )
    }

    #[test]
    fn apply_reuse_decision_reuses_matching_source_digest() {
        let output = generated_output_with_digest(Some("sha256:current"));
        let state_output = managed_state_output(Some("sha256:current"), "sha256:applied");

        assert_eq!(
            reuse_decision(&output, &state_output),
            ApplyReuseDecision::ReuseExistingManagedOutput
        );
    }

    #[test]
    fn apply_reuse_decision_rewrites_changed_source_digest() {
        let output = generated_output_with_digest(Some("sha256:current"));
        let state_output = managed_state_output(Some("sha256:previous"), "sha256:previous");

        assert_eq!(
            reuse_decision(&output, &state_output),
            ApplyReuseDecision::RewriteManagedOutput
        );
    }

    #[test]
    fn apply_reuse_decision_rewrites_when_apply_mode_changes() {
        let output = generated_output_with_digest(Some("sha256:current"));
        let state_output = managed_state_output(Some("sha256:current"), "sha256:applied");

        assert_eq!(
            ApplyReuseDecision::for_managed_output(
                &output,
                &ApplyMode::Copy,
                Some(&ApplyMode::Patch),
                false,
                None,
                &state_output,
            ),
            ApplyReuseDecision::RewriteManagedOutput
        );
    }

    #[test]
    fn apply_reuse_decision_accepts_legacy_digest_for_plain_outputs() {
        let output = generated_output_with_digest(Some("sha256:applied"));
        let state_output = managed_state_output(None, "sha256:applied");

        assert_eq!(
            reuse_decision(&output, &state_output),
            ApplyReuseDecision::ReuseExistingManagedOutput
        );
    }

    #[test]
    fn apply_reuse_decision_rewrites_legacy_digest_for_patch_outputs() {
        let marker = "managed-block".to_string();
        let output = generated_output_with_digest(Some("sha256:applied"));
        let state_output = managed_state_output(None, "sha256:applied");

        assert_eq!(
            ApplyReuseDecision::for_managed_output(
                &output,
                &ApplyMode::Copy,
                Some(&ApplyMode::Copy),
                false,
                Some(&marker),
                &state_output,
            ),
            ApplyReuseDecision::RewriteManagedOutput
        );
    }

    #[test]
    fn apply_reuse_decision_rewrites_legacy_digest_for_json_merge_outputs() {
        let output = generated_output_with_digest(Some("sha256:applied"));
        let state_output = managed_state_output(None, "sha256:applied");

        assert_eq!(
            ApplyReuseDecision::for_managed_output(
                &output,
                &ApplyMode::Copy,
                Some(&ApplyMode::Copy),
                true,
                None,
                &state_output,
            ),
            ApplyReuseDecision::RewriteManagedOutput
        );
    }

    #[test]
    fn apply_reuse_decision_rewrites_without_current_digest() {
        let output = generated_output_with_digest(None);
        let state_output = managed_state_output(Some("sha256:previous"), "sha256:previous");

        assert_eq!(
            reuse_decision(&output, &state_output),
            ApplyReuseDecision::RewriteManagedOutput
        );
    }

    #[test]
    fn materialize_as_regular_file_uses_output_policy() {
        let mut output = generated_output_with_digest(Some("sha256:current"));
        output.kind = GeneratedOutputKind::SkillFolder;
        output.destination_path = Some(".agents/skills/example/SKILL.md".to_string());

        assert!(!materialize_as_regular_file(
            &output,
            ".agents/skills/example/SKILL.md"
        ));

        output.materialize_as_regular_file = true;

        assert!(materialize_as_regular_file(
            &output,
            ".agents/skills/example/SKILL.md"
        ));
    }

    fn codex_skill_manifest(project_root: &Path) -> CompileManifest {
        let staged_relative = ".metactl/generated/codex-cli/.agents/skills/demo/SKILL.md";
        let staged = project_root.join(staged_relative);
        fs::create_dir_all(staged.parent().expect("staged parent")).expect("staged directory");
        fs::write(&staged, "---\nname: demo\n---\n\n# Demo\n").expect("staged skill");
        CompileManifest {
            api_version: "metactl/v2alpha1".to_string(),
            target: Ref {
                kind: RefKind::Target,
                id: "codex-cli".to_string(),
                version: Some("2026.08.12".to_string()),
            },
            generated_outputs: vec![GeneratedOutput {
                id: Some("demo".to_string()),
                path: staged_relative.to_string(),
                destination_path: Some(".agents/skills/demo/SKILL.md".to_string()),
                kind: GeneratedOutputKind::SkillFolder,
                digest: Some(sha256_bytes(b"---\nname: demo\n---\n\n# Demo\n")),
                instruction_mode: None,
                pack_ref: None,
                surface_id: None,
                surface_slug: None,
                source_resource_paths: Vec::new(),
                merge_status: None,
                degradation_codes: Vec::new(),
                ownership_token: None,
                materialize_as_regular_file: true,
                managed: true,
            }],
            pruned_outputs: Vec::new(),
            surface_selection_mode: None,
            surface_selection: Vec::new(),
            apply_modes_supported: vec![ApplyMode::Copy],
            brownfield_mode: None,
            degradations: Vec::new(),
        }
    }

    #[test]
    fn review_plan_classifies_legacy_only_and_preserves_legacy_bytes() {
        let project = tempfile::tempdir().expect("tempdir");
        let manifest = codex_skill_manifest(project.path());
        let legacy = project.path().join(".codex/skills/demo/SKILL.md");
        fs::create_dir_all(legacy.parent().expect("legacy parent")).expect("legacy directory");
        fs::write(&legacy, "---\nname: demo\n---\n\n# Demo\n").expect("legacy skill");

        let plan = build_apply_review_plan(project.path(), &manifest, &ApplyMode::Copy)
            .expect("review plan");
        assert_eq!(plan.actions[0].classification, "legacy_only");
        let legacy_before = fs::read(&legacy).expect("legacy before");
        let report = apply_manifest_bound(
            project.path(),
            &manifest,
            &ApplyMode::Copy,
            Some(&plan.digest),
        )
        .expect("bound apply");
        assert!(report.conflicts.is_empty());
        assert_eq!(fs::read(&legacy).expect("legacy after"), legacy_before);
        assert!(project.path().join(".agents/skills/demo/SKILL.md").exists());
    }

    #[test]
    fn review_plan_refuses_divergent_legacy_shadow() {
        let project = tempfile::tempdir().expect("tempdir");
        let manifest = codex_skill_manifest(project.path());
        let legacy = project.path().join(".codex/skills/demo/SKILL.md");
        fs::create_dir_all(legacy.parent().expect("legacy parent")).expect("legacy directory");
        fs::write(&legacy, "user-owned legacy bytes").expect("legacy skill");

        let plan = build_apply_review_plan(project.path(), &manifest, &ApplyMode::Copy)
            .expect("review plan");
        assert_eq!(plan.actions[0].classification, "content_conflict");
        let report = apply_manifest_bound(
            project.path(),
            &manifest,
            &ApplyMode::Copy,
            Some(&plan.digest),
        )
        .expect("bound apply");
        assert_eq!(report.conflicts.len(), 1);
        assert!(!project.path().join(".agents/skills/demo/SKILL.md").exists());
        assert_eq!(
            fs::read_to_string(legacy).expect("legacy after"),
            "user-owned legacy bytes"
        );
    }

    #[test]
    fn bound_apply_refuses_stale_or_transplanted_plan() {
        let project_a = tempfile::tempdir().expect("tempdir a");
        let project_b = tempfile::tempdir().expect("tempdir b");
        let manifest_a = codex_skill_manifest(project_a.path());
        let manifest_b = codex_skill_manifest(project_b.path());
        let plan_a = build_apply_review_plan(project_a.path(), &manifest_a, &ApplyMode::Copy)
            .expect("plan a");

        fs::write(
            project_a.path().join(&manifest_a.generated_outputs[0].path),
            "changed source",
        )
        .expect("mutate staged source");
        let stale = apply_manifest_bound(
            project_a.path(),
            &manifest_a,
            &ApplyMode::Copy,
            Some(&plan_a.digest),
        )
        .expect("stale result");
        assert_eq!(stale.conflicts.len(), 1);
        assert!(stale.conflicts[0].detail.contains("stale_plan"));

        let transplanted = apply_manifest_bound(
            project_b.path(),
            &manifest_b,
            &ApplyMode::Copy,
            Some(&plan_a.digest),
        )
        .expect("transplanted result");
        assert_eq!(transplanted.conflicts.len(), 1);
        assert!(transplanted.conflicts[0].detail.contains("stale_plan"));
    }

    #[test]
    fn containment_rejects_path_and_symlink_escapes() {
        for unsafe_path in ["../outside", "/tmp/outside", "a//b", "é/SKILL.md"] {
            assert!(validate_relative_output_path("fixture", unsafe_path).is_err());
        }
        let project = tempfile::tempdir().expect("project");
        let outside = tempfile::tempdir().expect("outside");
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), project.path().join(".agents"))
            .expect("escape symlink");
        #[cfg(unix)]
        assert!(ensure_contained_regular_path(
            project.path(),
            Path::new(".agents/skills/demo/SKILL.md"),
            true,
        )
        .is_err());
    }

    #[test]
    fn platform_equivalent_paths_collide() {
        let mut seen = BTreeSet::new();
        ensure_platform_unique_path(&mut seen, ".agents/skills/Demo/SKILL.md").expect("first path");
        assert!(ensure_platform_unique_path(&mut seen, ".agents/skills/demo/SKILL.md").is_err());
    }

    #[test]
    fn later_action_failure_compensates_prior_writes_and_keeps_journal() {
        let project = tempfile::tempdir().expect("tempdir");
        let root = project.path();
        let target = Ref {
            kind: RefKind::Target,
            id: "codex-cli".to_string(),
            version: Some("2026.08.12".to_string()),
        };
        let first_before = "first user bytes\n";
        let second_before = "<!-- metactl:begin second -->\nunterminated\n";
        fs::write(root.join("FIRST.md"), first_before).expect("first destination");
        fs::write(root.join("SECOND.md"), second_before).expect("second destination");

        let mut outputs = Vec::new();
        for (id, destination, contents) in [
            ("first", "FIRST.md", "managed first\n"),
            ("second", "SECOND.md", "managed second\n"),
        ] {
            let staged_relative = format!(".metactl/generated/codex-cli/{destination}");
            let staged = root.join(&staged_relative);
            fs::create_dir_all(staged.parent().expect("staged parent")).expect("staged directory");
            fs::write(&staged, contents).expect("staged bytes");
            outputs.push(GeneratedOutput {
                id: Some(id.to_string()),
                path: staged_relative,
                destination_path: Some(destination.to_string()),
                kind: GeneratedOutputKind::InstructionFile,
                digest: Some(sha256_bytes(contents.as_bytes())),
                instruction_mode: None,
                pack_ref: None,
                surface_id: None,
                surface_slug: None,
                source_resource_paths: Vec::new(),
                merge_status: None,
                degradation_codes: Vec::new(),
                ownership_token: None,
                materialize_as_regular_file: true,
                managed: true,
            });
        }
        let manifest = CompileManifest {
            api_version: "metactl/v2alpha1".to_string(),
            target: target.clone(),
            generated_outputs: outputs,
            pruned_outputs: Vec::new(),
            surface_selection_mode: None,
            surface_selection: Vec::new(),
            apply_modes_supported: vec![ApplyMode::Patch],
            brownfield_mode: Some(BrownfieldMode::PatchMode),
            degradations: Vec::new(),
        };
        let plan = build_apply_review_plan(root, &manifest, &ApplyMode::Patch).expect("plan");
        let report = apply_manifest_bound(root, &manifest, &ApplyMode::Patch, Some(&plan.digest))
            .expect("compensated report");

        assert_eq!(report.conflicts.len(), 1);
        assert!(report.conflicts[0]
            .detail
            .starts_with("compensated_failure:"));
        assert_eq!(
            fs::read_to_string(root.join("FIRST.md")).unwrap(),
            first_before
        );
        assert_eq!(
            fs::read_to_string(root.join("SECOND.md")).unwrap(),
            second_before
        );
        assert!(!state_path(root, &target).exists());
        let journal = fs::read_to_string(apply_journal_path(root, &target, &plan.digest))
            .expect("failure journal");
        assert!(journal.contains("\"status\":\"applied\""));
        assert!(journal.contains("\"status\":\"failure_compensated\""));
    }

    #[test]
    fn rollback_refuses_intervening_user_edit() {
        let project = tempfile::tempdir().expect("tempdir");
        let root = project.path();
        let manifest = codex_skill_manifest(root);
        let target = manifest.target.clone();
        let report = apply_manifest(root, &manifest, &ApplyMode::Copy).expect("apply");
        assert!(report.conflicts.is_empty());
        let destination = root.join(".agents/skills/demo/SKILL.md");
        fs::write(&destination, "user edit after apply\n").expect("user edit");

        let reverted = revert_target(root, &target).expect("revert report");
        assert_eq!(reverted.conflicts.len(), 1);
        assert!(reverted.reverted_paths.is_empty());
        assert_eq!(
            fs::read_to_string(destination).expect("preserved edit"),
            "user edit after apply\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn apply_rejects_destination_parent_swapped_to_external_symlink() {
        let project = tempfile::tempdir().expect("project");
        let outside = tempfile::tempdir().expect("outside");
        let sentinel = outside.path().join("sentinel");
        fs::write(&sentinel, "outside sentinel\n").expect("sentinel");
        let manifest = codex_skill_manifest(project.path());
        let plan = build_apply_review_plan(project.path(), &manifest, &ApplyMode::Copy)
            .expect("initial plan");
        std::os::unix::fs::symlink(outside.path(), project.path().join(".agents"))
            .expect("swap destination parent");

        let error = apply_manifest_bound(
            project.path(),
            &manifest,
            &ApplyMode::Copy,
            Some(&plan.digest),
        )
        .expect_err("changed link identity must fail");
        assert!(error.to_string().contains("unsafe_alias"));
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "outside sentinel\n");
    }

    #[test]
    fn stale_output_pruning_rejects_paths_outside_target_stage_root() {
        let project = tempfile::tempdir().expect("tempdir");
        let stage_root = project.path().join(".metactl/generated/codex-cli");
        std::fs::create_dir_all(&stage_root).expect("create stage root");

        for unsafe_path in ["../outside.md", ".metactl/generated/claude-code/CLAUDE.md"] {
            assert!(
                safe_staged_output_path(project.path(), &stage_root, unsafe_path).is_err(),
                "unsafe prune path was accepted: {unsafe_path}"
            );
        }
    }

    #[test]
    fn noop_apply_preserves_patch_managed_file_digest() {
        let project = tempfile::tempdir().expect("tempdir");
        let project_root = project.path();
        let staged_path = project_root.join(".metactl/generated/codex-cli/AGENTS.md");
        std::fs::create_dir_all(staged_path.parent().expect("staged parent"))
            .expect("create staged parent");

        let managed = "# Repository Builder\n";
        std::fs::write(&staged_path, managed).expect("write staged");
        std::fs::write(
            project_root.join("AGENTS.md"),
            "Private preface.\n\n<!-- metactl:begin output -->\nold\n<!-- metactl:end output -->\n",
        )
        .expect("write existing");

        let output = generated_output_with_digest(Some(&sha256_bytes(managed.as_bytes())));
        let target = Ref {
            kind: RefKind::Target,
            id: "codex-cli".to_string(),
            version: None,
        };
        let manifest = CompileManifest {
            api_version: "metactl/v2alpha1".to_string(),
            target: target.clone(),
            generated_outputs: vec![output],
            pruned_outputs: Vec::new(),
            surface_selection_mode: None,
            surface_selection: Vec::new(),
            apply_modes_supported: vec![ApplyMode::Patch],
            brownfield_mode: None,
            degradations: Vec::new(),
        };

        apply_manifest(project_root, &manifest, &ApplyMode::Patch).expect("first apply");
        assert!(drift_conflicts(project_root, &target)
            .expect("first drift check")
            .is_empty());

        apply_manifest(project_root, &manifest, &ApplyMode::Patch).expect("second apply");
        assert!(drift_conflicts(project_root, &target)
            .expect("second drift check")
            .is_empty());
    }
}
