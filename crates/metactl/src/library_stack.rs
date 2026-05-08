use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::types::{Ref, RefKind, TrustTier, VisibilityScope, API_VERSION};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySourceRole {
    Baseline,
    Overlay,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySourceType {
    LocalPath,
    Git,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibrarySourceLocation {
    #[serde(rename = "type")]
    pub source_type: LibrarySourceType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    pub ref_: Option<String>,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactOverridePolicy {
    None,
    AllowOverlay,
    AllowBaselinePrecedence,
}

impl Default for ArtifactOverridePolicy {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibraryArtifactManifest {
    pub artifact_ref: Ref,
    pub digest: String,
    pub source_path: String,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub override_policy: ArtifactOverridePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibrarySourceManifest {
    pub kind: String,
    pub id: String,
    pub version: String,
    pub title: String,
    pub source_role: LibrarySourceRole,
    pub read_only: bool,
    pub writable: bool,
    pub pinned: bool,
    pub visibility_scope: VisibilityScope,
    pub trust_tier: TrustTier,
    pub source: LibrarySourceLocation,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<LibraryArtifactManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BaselinePrecedenceMode {
    Explicit,
    FailOnConflict,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommittedProjectionConfig {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracked_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibraryProfileManifest {
    pub kind: String,
    pub id: String,
    pub version: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub baseline_refs: Vec<String>,
    pub overlay_ref: String,
    pub baseline_precedence: BaselinePrecedenceMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub committed_projection: Option<CommittedProjectionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibraryStackManifest {
    pub api_version: String,
    pub kind: String,
    pub id: String,
    pub version: String,
    pub title: String,
    pub active_profile_ref: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<LibrarySourceManifest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<LibraryProfileManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedOverrideStatus {
    None,
    OverrodeBaseline,
    BaselinePrecedence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedLibraryArtifact {
    pub artifact_ref: Ref,
    pub source_id: String,
    pub source_role: LibrarySourceRole,
    pub source_digest: String,
    pub artifact_digest: String,
    pub locked: bool,
    pub override_status: ResolvedOverrideStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibraryStackLock {
    pub api_version: String,
    pub kind: String,
    pub stack_ref: Ref,
    pub profile_ref: Ref,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resolved_artifacts: Vec<ResolvedLibraryArtifact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct ResolutionCandidate<'a> {
    source: &'a LibrarySourceManifest,
    artifact: &'a LibraryArtifactManifest,
    override_status: ResolvedOverrideStatus,
}

pub fn resolve_library_stack(stack: &LibraryStackManifest) -> Result<LibraryStackLock> {
    if stack.api_version != API_VERSION {
        return Err(anyhow!(
            "METACTL_STACK_API_VERSION: expected {API_VERSION}, got {}",
            stack.api_version
        ));
    }
    let sources = sources_by_id(stack)?;
    let profile = active_profile(stack)?;
    let overlay = sources.get(profile.overlay_ref.as_str()).ok_or_else(|| {
        anyhow!(
            "METACTL_STACK_OVERLAY_NOT_FOUND: overlay {}",
            profile.overlay_ref
        )
    })?;
    if overlay.source_role != LibrarySourceRole::Overlay || overlay.read_only || !overlay.writable {
        return Err(anyhow!(
            "METACTL_STACK_INVALID_OVERLAY: {} must be the single writable overlay",
            overlay.id
        ));
    }

    let mut ordered_sources = Vec::new();
    for baseline_ref in &profile.baseline_refs {
        let baseline = sources
            .get(baseline_ref.as_str())
            .ok_or_else(|| anyhow!("METACTL_STACK_BASELINE_NOT_FOUND: baseline {baseline_ref}"))?;
        if baseline.source_role != LibrarySourceRole::Baseline
            || !baseline.read_only
            || baseline.writable
        {
            return Err(anyhow!(
                "METACTL_STACK_INVALID_BASELINE: {} must be read-only",
                baseline.id
            ));
        }
        if !baseline.pinned {
            return Err(anyhow!(
                "METACTL_STACK_UNPINNED_BASELINE: baseline {} must be pinned",
                baseline.id
            ));
        }
        ordered_sources.push(*baseline);
    }
    ordered_sources.push(*overlay);

    let mut resolved: BTreeMap<String, ResolutionCandidate<'_>> = BTreeMap::new();
    for source in ordered_sources {
        for artifact in &source.artifacts {
            let key = artifact_identity_key(&artifact.artifact_ref);
            match resolved.get_mut(&key) {
                None => {
                    resolved.insert(
                        key,
                        ResolutionCandidate {
                            source,
                            artifact,
                            override_status: ResolvedOverrideStatus::None,
                        },
                    );
                }
                Some(existing) => {
                    resolve_collision(profile, source, artifact, existing, &key)?;
                }
            }
        }
    }

    Ok(LibraryStackLock {
        api_version: API_VERSION.to_string(),
        kind: "library_stack_lock".to_string(),
        stack_ref: Ref {
            kind: RefKind::Artifact,
            id: stack.id.clone(),
            version: Some(stack.version.clone()),
        },
        profile_ref: Ref {
            kind: RefKind::Artifact,
            id: profile.id.clone(),
            version: Some(profile.version.clone()),
        },
        resolved_artifacts: resolved
            .values()
            .map(|candidate| ResolvedLibraryArtifact {
                artifact_ref: candidate.artifact.artifact_ref.clone(),
                source_id: candidate.source.id.clone(),
                source_role: candidate.source.source_role.clone(),
                source_digest: candidate.source.source.digest.clone(),
                artifact_digest: candidate.artifact.digest.clone(),
                locked: candidate.artifact.locked,
                override_status: candidate.override_status.clone(),
                generated_paths: Vec::new(),
            })
            .collect(),
        conflicts: Vec::new(),
        warnings: Vec::new(),
    })
}

fn artifact_identity_key(ref_: &Ref) -> String {
    format!("{:?}:{}", ref_.kind, ref_.id)
}

fn sources_by_id(stack: &LibraryStackManifest) -> Result<BTreeMap<&str, &LibrarySourceManifest>> {
    let mut sources = BTreeMap::new();
    for source in &stack.sources {
        if sources.insert(source.id.as_str(), source).is_some() {
            return Err(anyhow!(
                "METACTL_STACK_DUPLICATE_SOURCE: source {}",
                source.id
            ));
        }
    }
    Ok(sources)
}

fn active_profile(stack: &LibraryStackManifest) -> Result<&LibraryProfileManifest> {
    let matches = stack
        .profiles
        .iter()
        .filter(|profile| profile.id == stack.active_profile_ref)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [profile] => Ok(*profile),
        [] => Err(anyhow!(
            "METACTL_STACK_PROFILE_NOT_FOUND: active profile {}",
            stack.active_profile_ref
        )),
        _ => Err(anyhow!(
            "METACTL_STACK_DUPLICATE_PROFILE: active profile {}",
            stack.active_profile_ref
        )),
    }
}

fn resolve_collision<'a>(
    profile: &LibraryProfileManifest,
    source: &'a LibrarySourceManifest,
    artifact: &'a LibraryArtifactManifest,
    existing: &mut ResolutionCandidate<'a>,
    key: &str,
) -> Result<()> {
    if source.source_role == LibrarySourceRole::Overlay {
        if existing.artifact.locked {
            return Err(anyhow!(
                "METACTL_STACK_LOCKED_OVERRIDE: {key} from {} cannot be overridden by {}",
                existing.source.id,
                source.id
            ));
        }
        if existing.artifact.override_policy == ArtifactOverridePolicy::AllowOverlay {
            *existing = ResolutionCandidate {
                source,
                artifact,
                override_status: ResolvedOverrideStatus::OverrodeBaseline,
            };
            return Ok(());
        }
        return Err(anyhow!(
            "METACTL_STACK_ACCIDENTAL_COLLISION: {key} from {} conflicts with {}",
            existing.source.id,
            source.id
        ));
    }

    if existing.source.source_role == LibrarySourceRole::Baseline
        && source.source_role == LibrarySourceRole::Baseline
    {
        if profile.baseline_precedence == BaselinePrecedenceMode::Explicit
            && existing.artifact.override_policy == ArtifactOverridePolicy::AllowBaselinePrecedence
        {
            existing.override_status = ResolvedOverrideStatus::BaselinePrecedence;
            return Ok(());
        }
        return Err(anyhow!(
            "METACTL_STACK_BASELINE_CONFLICT: {key} from {} conflicts with {}",
            existing.source.id,
            source.id
        ));
    }

    Err(anyhow!("METACTL_STACK_ACCIDENTAL_COLLISION: {key}"))
}

pub fn active_stack_source_ids(stack: &LibraryStackManifest) -> Result<Vec<String>> {
    let sources = sources_by_id(stack)?;
    let profile = active_profile(stack)?;
    let mut seen = BTreeSet::new();
    let mut ids = Vec::new();
    for id in profile
        .baseline_refs
        .iter()
        .chain(std::iter::once(&profile.overlay_ref))
    {
        if !sources.contains_key(id.as_str()) {
            return Err(anyhow!("METACTL_STACK_SOURCE_NOT_FOUND: source {id}"));
        }
        if seen.insert(id.clone()) {
            ids.push(id.clone());
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::{
        resolve_library_stack, LibraryStackLock, LibraryStackManifest, ResolvedLibraryArtifact,
    };

    fn fixture_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/library_stack")
    }

    fn load_stack(case: &str) -> LibraryStackManifest {
        let path = fixture_root().join(case).join("stack.json");
        serde_json::from_slice(&std::fs::read(path).expect("stack bytes")).expect("stack")
    }

    fn load_lock(case: &str) -> LibraryStackLock {
        let path = fixture_root().join(case).join("lock.json");
        serde_json::from_slice(&std::fs::read(path).expect("lock bytes")).expect("lock")
    }

    fn artifact_map(lock: &LibraryStackLock) -> BTreeMap<String, ResolvedLibraryArtifact> {
        lock.resolved_artifacts
            .iter()
            .cloned()
            .map(|item| {
                (
                    format!("{:?}:{}", item.artifact_ref.kind, item.artifact_ref.id),
                    item,
                )
            })
            .collect()
    }

    #[test]
    fn resolves_positive_stack_fixtures_to_expected_locks() {
        for case in [
            "user-only",
            "one-baseline",
            "multi-baseline",
            "allowed-override",
        ] {
            let stack = load_stack(case);
            let expected = load_lock(case);
            let actual = resolve_library_stack(&stack).expect(case);
            assert_eq!(actual.api_version, expected.api_version, "{case}");
            assert_eq!(actual.kind, expected.kind, "{case}");
            assert_eq!(actual.stack_ref, expected.stack_ref, "{case}");
            assert_eq!(actual.profile_ref, expected.profile_ref, "{case}");
            assert_eq!(artifact_map(&actual), artifact_map(&expected), "{case}");
        }
    }

    #[test]
    fn rejects_locked_baseline_override() {
        let stack = load_stack("locked-conflict");
        let err = resolve_library_stack(&stack).expect_err("locked override should fail");
        assert!(err.to_string().contains("METACTL_STACK_LOCKED_OVERRIDE"));
    }

    #[test]
    fn rejects_accidental_collision() {
        let stack = load_stack("accidental-collision");
        let err = resolve_library_stack(&stack).expect_err("collision should fail");
        assert!(err
            .to_string()
            .contains("METACTL_STACK_ACCIDENTAL_COLLISION"));
    }
}
