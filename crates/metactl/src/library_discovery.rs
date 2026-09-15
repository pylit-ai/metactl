use super::{
    read_pack_resource, relative_path, sha256_digest, sorted_files, DiscoveredPack,
    CANDIDATE_VERSION,
};
use crate::types::{
    ActivationClass, ImportEcosystem, PackImport, PackManifest, PackResource, PromotionStatus,
    ProvenanceEnvelope, ProvenanceReview, Ref, RefKind, ResourceKind, RoleManifest,
    SearchMatchEvidence, SideEffectClass, TrustTier, VisibilityScope,
};
use anyhow::{anyhow, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

pub(super) fn normalize_candidate(
    root: &Path,
    path: &Path,
) -> Result<(PackManifest, ProvenanceEnvelope)> {
    match path.file_name().and_then(|value| value.to_str()) {
        Some("AGENTS.md") => normalize_agents_candidate(root, path),
        Some("SKILL.md") => normalize_skill_candidate(root, path),
        _ => Err(anyhow!("unsupported import candidate {}", path.display())),
    }
}

fn normalize_agents_candidate(
    root: &Path,
    path: &Path,
) -> Result<(PackManifest, ProvenanceEnvelope)> {
    let contents = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let pack_id = path
        .parent()
        .and_then(|item| item.file_name())
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("unable to infer candidate id from {}", path.display()))?
        .to_string();
    let digest = sha256_digest(path)?;
    let title = first_heading(&contents).unwrap_or_else(|| format!("Imported {}", pack_id));
    let description = first_body_line(&contents);
    let manifest = PackManifest {
        kind: "pack".to_string(),
        id: pack_id.clone(),
        version: CANDIDATE_VERSION.to_string(),
        title,
        description,
        activation_class: ActivationClass::Instruction,
        side_effect_class: SideEffectClass::None,
        trust_tier: TrustTier::CandidateQuarantined,
        requires_confirmation: false,
        task_tags: infer_tags(&contents, &pack_id),
        compatible_roles: Vec::new(),
        compatible_targets: Vec::new(),
        knowledge_refs: Vec::new(),
        resources: vec![PackResource {
            path: relative_path(root, path)?,
            kind: ResourceKind::Instruction,
            required: true,
            surface_relevance: None,
        }],
        imports: vec![PackImport {
            ecosystem: ImportEcosystem::AgentsMd,
            origin: path.display().to_string(),
            digest: Some(digest.clone()),
        }],
        visibility_scope: VisibilityScope::default(),
        lifecycle: None,
        metadata: BTreeMap::from([("normalized_from".to_string(), "AGENTS.md".to_string())]),
    };
    let provenance = candidate_provenance(&manifest, digest, path, ImportEcosystem::AgentsMd);
    Ok((manifest, provenance))
}

fn normalize_skill_candidate(
    root: &Path,
    path: &Path,
) -> Result<(PackManifest, ProvenanceEnvelope)> {
    let contents = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let pack_id = path
        .parent()
        .and_then(|item| item.file_name())
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("unable to infer candidate id from {}", path.display()))?
        .to_string();
    let digest = sha256_digest(path)?;
    let title = first_heading(&contents).unwrap_or_else(|| format!("Imported {}", pack_id));
    let description = first_body_line(&contents);
    let resources = sorted_files(path.parent().expect("skill parent"))?
        .into_iter()
        .filter(|item| item.is_file())
        .map(|item| {
            Ok(PackResource {
                path: relative_path(root, &item)?,
                kind: infer_resource_kind(path.parent().expect("skill parent"), &item),
                required: item.file_name().and_then(|value| value.to_str()) == Some("SKILL.md"),
                surface_relevance: None,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let manifest = PackManifest {
        kind: "pack".to_string(),
        id: pack_id.clone(),
        version: CANDIDATE_VERSION.to_string(),
        title,
        description,
        activation_class: ActivationClass::Instruction,
        side_effect_class: SideEffectClass::None,
        trust_tier: TrustTier::CandidateQuarantined,
        requires_confirmation: false,
        task_tags: infer_tags(&contents, &pack_id),
        compatible_roles: Vec::new(),
        compatible_targets: Vec::new(),
        knowledge_refs: Vec::new(),
        resources,
        imports: vec![PackImport {
            ecosystem: ImportEcosystem::SkillMd,
            origin: path.display().to_string(),
            digest: Some(digest.clone()),
        }],
        visibility_scope: VisibilityScope::default(),
        lifecycle: None,
        metadata: BTreeMap::from([("normalized_from".to_string(), "SKILL.md".to_string())]),
    };
    let provenance = candidate_provenance(&manifest, digest, path, ImportEcosystem::SkillMd);
    Ok((manifest, provenance))
}

fn candidate_provenance(
    manifest: &PackManifest,
    digest: String,
    path: &Path,
    ecosystem: ImportEcosystem,
) -> ProvenanceEnvelope {
    ProvenanceEnvelope {
        api_version: crate::types::API_VERSION.to_string(),
        subject_ref: manifest.pack_ref(),
        digest,
        origin: path.display().to_string(),
        imported_from_ecosystem: ecosystem,
        imported_at: None,
        review: Some(ProvenanceReview {
            reviewed_by: None,
            reviewed_at: None,
            promotion_status: Some(PromotionStatus::Candidate),
        }),
        attestation_refs: Vec::new(),
        validation_refs: Vec::new(),
    }
}

pub(super) fn provenance_ref_for(manifest: &PackManifest) -> Ref {
    Ref {
        kind: RefKind::Artifact,
        id: format!("{}-provenance", manifest.id),
        version: None,
    }
}

pub(super) fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(|item| item.trim().to_ascii_lowercase())
        .filter(|item| item.len() >= 3)
        .collect()
}

pub(super) fn search_match_evidence(
    pack: &DiscoveredPack,
    query_terms: &[String],
) -> Result<SearchMatchEvidence> {
    let mut matched_fields = Vec::new();
    let mut matched_resource_paths = Vec::new();
    let mut matched_terms = BTreeSet::new();

    let metadata_fields = [
        ("id", pack.manifest.id.to_ascii_lowercase()),
        ("title", pack.manifest.title.to_ascii_lowercase()),
        (
            "description",
            pack.manifest
                .description
                .clone()
                .unwrap_or_default()
                .to_ascii_lowercase(),
        ),
        (
            "task_tags",
            pack.manifest.task_tags.join(" ").to_ascii_lowercase(),
        ),
    ];
    for term in query_terms {
        for (field, value) in &metadata_fields {
            if value.contains(term) {
                matched_fields.push((*field).to_string());
                matched_terms.insert(term.clone());
            }
        }
    }

    for resource in pack.manifest.resources.iter().filter(|resource| {
        matches!(
            resource.kind,
            ResourceKind::Instruction | ResourceKind::Example
        )
    }) {
        let contents =
            String::from_utf8_lossy(&read_pack_resource(pack, resource)?).to_ascii_lowercase();
        if query_terms.iter().any(|term| contents.contains(term)) {
            matched_resource_paths.push(resource.path.clone());
            for term in query_terms {
                if contents.contains(term) {
                    matched_terms.insert(term.clone());
                }
            }
        }
    }

    matched_fields.sort();
    matched_fields.dedup();
    matched_resource_paths.sort();
    matched_resource_paths.dedup();

    Ok(SearchMatchEvidence {
        matched_fields,
        matched_resource_paths,
        matched_terms: matched_terms.into_iter().collect(),
    })
}

pub(super) fn relevance_score(
    pack: &DiscoveredPack,
    query_terms: &[String],
    role: &RoleManifest,
    target: &Ref,
    evidence: &SearchMatchEvidence,
) -> f64 {
    let mut score = 0.0_f64;
    let haystack = format!(
        "{} {} {} {}",
        pack.manifest.id,
        pack.manifest.title,
        pack.manifest.description.clone().unwrap_or_default(),
        pack.manifest.task_tags.join(" ")
    )
    .to_ascii_lowercase();

    for term in query_terms {
        if haystack.contains(term) {
            score += 0.18;
        }
    }
    score += evidence.matched_resource_paths.len() as f64 * 0.18;
    score += evidence.matched_fields.len() as f64 * 0.04;
    if pack.manifest.compatible_roles.is_empty()
        || pack
            .manifest
            .compatible_roles
            .iter()
            .any(|item| item == &role.id)
    {
        score += 0.15;
    }
    if pack.manifest.compatible_targets.is_empty()
        || pack
            .manifest
            .compatible_targets
            .iter()
            .any(|item| item == &target.id)
    {
        score += 0.1;
    }
    if !pack.is_candidate() {
        score += 0.1;
    }
    (score * 100.0).round() / 100.0
}

pub(super) fn why_string(pack: &DiscoveredPack, evidence: &SearchMatchEvidence) -> String {
    if evidence.matched_fields.iter().any(|field| field == "id") {
        return "Query matched the pack identifier directly.".to_string();
    }
    if evidence
        .matched_fields
        .iter()
        .any(|field| field == "task_tags")
    {
        return "Query aligned with normalized pack task tags.".to_string();
    }
    if !evidence.matched_resource_paths.is_empty() {
        return "Query matched instruction or reference content in the pack body.".to_string();
    }
    if pack.is_candidate() {
        return "Candidate remained discoverable because search mode and policy allowed it."
            .to_string();
    }
    "Pack satisfied current role, target, and policy constraints.".to_string()
}

fn infer_tags(contents: &str, pack_id: &str) -> Vec<String> {
    let mut tags = BTreeSet::new();
    for token in pack_id
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .chain(contents.split(|ch: char| !ch.is_ascii_alphanumeric()))
    {
        let token = token.trim().to_ascii_lowercase();
        if token.len() >= 4 {
            tags.insert(token);
        }
        if tags.len() == 6 {
            break;
        }
    }
    tags.into_iter().collect()
}

fn infer_resource_kind(root: &Path, path: &Path) -> ResourceKind {
    if path.file_name().and_then(|value| value.to_str()) == Some("SKILL.md") {
        return ResourceKind::Instruction;
    }
    let relative = path.strip_prefix(root).unwrap_or(path);
    let rel_str = relative.to_string_lossy();
    if rel_str.starts_with("references/")
        || path.extension().and_then(|value| value.to_str()) == Some("md")
    {
        ResourceKind::Example
    } else if rel_str.starts_with("scripts/") {
        ResourceKind::Script
    } else {
        ResourceKind::Asset
    }
}

fn first_heading(contents: &str) -> Option<String> {
    contents
        .lines()
        .find_map(|line| line.strip_prefix('#').map(str::trim))
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
}

fn first_body_line(contents: &str) -> Option<String> {
    contents
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_string())
}
