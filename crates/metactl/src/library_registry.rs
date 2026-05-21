use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use anyhow::{anyhow, Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::materializer::{self, StagedOutputInput};
use crate::suite_registry::selected_target_from_config;
use crate::types::{
    ActivationClass, ApplyMode, ApplyReport, CapabilityGap, CompileManifest, CompileParams,
    CompileResult, CompileTargetKind, Config, DiscoveryMode, EnforcementStatus, ExplainParams,
    ExplainResult, GeneratedOutputKind, ImportEcosystem, InstructionProjectionMode,
    InvocationOverlay, KnowledgeSourceManifest, LocalProjectionSupport, PackImport, PackManifest,
    PackResource, PolicyEnforcementReport, PolicyManifest, PolicyOperator, PolicyRuleReport,
    PolicySelectors, PolicySubject, PromotionStatus, ProvenanceEnvelope, ProvenanceReview,
    RealizedEnforcementClass, ReasonCode, Ref, RefKind, RequestedEnforcementClass, ResolveGraph,
    ResolveParams, ResourceKind, RevertReport, RoleManifest, RuntimeTemplateRef, SearchMatch,
    SearchMatchEvidence, SearchParams, SearchResult, SideEffectClass, SuppressedRef,
    SuppressedSubject, SurfaceMergeStatus, SurfaceMergeStrategy, SurfaceRelevanceTier,
    SurfaceSelectionDecision, SurfaceSelectionMode, TargetCapabilityMatrix, TrustTier,
    ValidateParams, ValidationCheck, ValidationReport, ValidationStatus, VisibilityScope,
};

const CANDIDATE_VERSION: &str = "0.0.0-candidate";
const INSTRUCTION_INDEX_WARN_BYTES: usize = 8 * 1024;
const INSTRUCTION_INDEX_MAX_BYTES: usize = 32 * 1024;
const INSTRUCTION_INDEX_POINTER: &str = "open the referenced pack root for full detail";

#[derive(Debug, Clone)]
struct CachedResource {
    modified: Option<SystemTime>,
    len: u64,
    bytes: Vec<u8>,
}

static RESOURCE_READ_CACHE: OnceLock<Mutex<BTreeMap<PathBuf, CachedResource>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct LibraryRegistry {
    roots: Vec<PathBuf>,
    roles: BTreeMap<String, RoleManifest>,
    policies: BTreeMap<String, PolicyManifest>,
    targets: BTreeMap<String, TargetCapabilityMatrix>,
    knowledge_sources: BTreeMap<String, KnowledgeSourceManifest>,
    packs: BTreeMap<String, DiscoveredPack>,
}

#[derive(Debug, Clone)]
pub struct ListedPack {
    pub manifest: PackManifest,
    pub promotion_status: PromotionStatus,
    pub source_path: PathBuf,
    pub library_root: PathBuf,
    pub provenance_ref: Option<Ref>,
}

#[derive(Debug, Clone)]
struct DiscoveredPack {
    manifest: PackManifest,
    provenance: Option<ProvenanceEnvelope>,
    provenance_ref: Option<Ref>,
    source_path: PathBuf,
    library_root: PathBuf,
    promotion_status: PromotionStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct SurfaceSummary {
    pub surface_id: String,
    pub surface_slug: String,
    pub title: String,
    pub emitted: bool,
    pub relevance_tier: SurfaceRelevanceTier,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<ReasonCode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub instruction_resource_paths: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attached_script_paths: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attached_reference_paths: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attached_asset_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackSurfaceSummary {
    pub pack_ref: Ref,
    pub selection_mode: SurfaceSelectionMode,
    pub emission_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_strategy: Option<SurfaceMergeStrategy>,
    pub surfaces: Vec<SurfaceSummary>,
}

#[derive(Debug, Clone)]
struct DerivedSkillSurface {
    surface_id: String,
    surface_slug: String,
    title: String,
    instruction_resource_paths: Vec<String>,
    attached_script_paths: Vec<String>,
    attached_reference_paths: Vec<String>,
    attached_asset_paths: Vec<String>,
    contents: Vec<u8>,
}

#[derive(Debug, Clone)]
struct InstructionReference {
    path: String,
    locator: String,
    source_resource_paths: Vec<String>,
}

#[derive(Debug, Clone)]
struct PlannedInstructionPack {
    pack_ref: Ref,
    title: String,
    description: Option<String>,
    when_to_open: Vec<String>,
    references: Vec<InstructionReference>,
    inline_snippet: Option<String>,
}

#[derive(Debug, Clone)]
struct InstructionDocumentPlan {
    mode: InstructionProjectionMode,
    packs: Vec<PlannedInstructionPack>,
    source_resource_paths: Vec<String>,
    degradation_codes: Vec<String>,
}

#[derive(Debug, Clone)]
struct BudgetedInstructionDocument {
    content: String,
    truncated: bool,
}

impl LibraryRegistry {
    pub fn load_from_roots(roots: &[PathBuf]) -> Result<Self> {
        if roots.is_empty() {
            return Err(anyhow!("at least one library root is required"));
        }

        let mut registry = Self {
            roots: roots.to_vec(),
            roles: BTreeMap::new(),
            policies: BTreeMap::new(),
            targets: BTreeMap::new(),
            knowledge_sources: BTreeMap::new(),
            packs: BTreeMap::new(),
        };
        let mut provenance_by_subject = BTreeMap::new();

        for root in roots {
            registry.load_root(root, &mut provenance_by_subject)?;
        }

        for pack in registry.packs.values_mut() {
            if let Some(provenance) = provenance_by_subject.remove(&pack.manifest.pack_ref().key())
            {
                pack.provenance_ref = Some(provenance_ref_for(&pack.manifest));
                pack.provenance = Some(provenance);
            }
        }

        Ok(registry)
    }

    pub fn root_names(&self) -> Vec<String> {
        self.roots
            .iter()
            .map(|root| {
                root.file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_string()
            })
            .collect()
    }

    pub fn list_roles(&self) -> Vec<RoleManifest> {
        self.roles.values().cloned().collect()
    }

    pub fn list_policies(&self) -> Vec<PolicyManifest> {
        self.policies.values().cloned().collect()
    }

    pub fn list_targets(&self) -> Vec<TargetCapabilityMatrix> {
        self.targets.values().cloned().collect()
    }

    pub fn list_knowledge_sources(&self) -> Vec<KnowledgeSourceManifest> {
        self.knowledge_sources.values().cloned().collect()
    }

    pub fn list_packs(&self) -> Vec<ListedPack> {
        self.packs
            .values()
            .map(|pack| ListedPack {
                manifest: pack.manifest.clone(),
                promotion_status: pack.promotion_status.clone(),
                source_path: pack.source_path.clone(),
                library_root: pack.library_root.clone(),
                provenance_ref: pack.provenance_ref.clone(),
            })
            .collect()
    }

    pub fn role_by_id(&self, id: &str) -> Option<RoleManifest> {
        self.roles.get(id).cloned()
    }

    pub fn policy_by_id(&self, id: &str) -> Option<PolicyManifest> {
        self.policies.get(id).cloned()
    }

    pub fn target_by_id(&self, id: &str) -> Option<TargetCapabilityMatrix> {
        self.targets.get(id).cloned()
    }

    pub fn knowledge_source_by_id(&self, id: &str) -> Option<KnowledgeSourceManifest> {
        self.knowledge_sources.get(id).cloned()
    }

    pub fn pack_by_id(&self, id: &str) -> Option<ListedPack> {
        self.packs.get(id).map(|pack| ListedPack {
            manifest: pack.manifest.clone(),
            promotion_status: pack.promotion_status.clone(),
            source_path: pack.source_path.clone(),
            library_root: pack.library_root.clone(),
            provenance_ref: pack.provenance_ref.clone(),
        })
    }

    pub fn surface_summaries_for_target(
        &self,
        pack_refs: &[Ref],
        target: &TargetCapabilityMatrix,
        surface_selection_mode: SurfaceSelectionMode,
    ) -> Result<Vec<PackSurfaceSummary>> {
        let compile_target = skill_compile_target_for(target);
        pack_refs
            .iter()
            .filter_map(|pack_ref| self.find_pack(pack_ref))
            .map(|pack| {
                let surfaces = derive_skill_surfaces(pack)?;
                let decisions =
                    surface_selection_decisions(pack, &surfaces, surface_selection_mode.clone());
                let emitted_surfaces = surfaces
                    .iter()
                    .filter(|surface| {
                        decisions.iter().any(|decision| {
                            decision.surface_id == surface.surface_id && decision.emitted
                        })
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                let emission_mode = if emitted_surfaces.is_empty() {
                    "suppressed".to_string()
                } else if let Some(compile_target) = compile_target {
                    if should_emit_separate_surfaces(target, compile_target, &emitted_surfaces)? {
                        "separate".to_string()
                    } else if emitted_surfaces.len() > 1 {
                        "merged".to_string()
                    } else {
                        "single".to_string()
                    }
                } else {
                    "not_emitted".to_string()
                };
                Ok(PackSurfaceSummary {
                    pack_ref: pack.manifest.pack_ref(),
                    selection_mode: surface_selection_mode.clone(),
                    emission_mode,
                    merge_strategy: compile_target
                        .and_then(|item| item.surface_merge_strategy.clone()),
                    surfaces: surfaces
                        .into_iter()
                        .map(|surface| {
                            let decision = decisions
                                .iter()
                                .find(|item| item.surface_id == surface.surface_id)
                                .expect("surface decision");
                            SurfaceSummary {
                                surface_id: surface.surface_id,
                                surface_slug: surface.surface_slug,
                                title: surface.title,
                                emitted: decision.emitted,
                                relevance_tier: decision.relevance_tier.clone(),
                                reason_code: decision.reason_code.clone(),
                                detail: decision.detail.clone(),
                                instruction_resource_paths: surface.instruction_resource_paths,
                                attached_script_paths: surface.attached_script_paths,
                                attached_reference_paths: surface.attached_reference_paths,
                                attached_asset_paths: surface.attached_asset_paths,
                            }
                        })
                        .collect(),
                })
            })
            .collect()
    }

    pub fn search(&self, params: SearchParams) -> Result<SearchResult> {
        let role = self.find_role(&params.config.role)?;
        let policy = self.find_policy(&params.config.policy)?;
        let selected_target = self.selected_target(&params.config, params.overlay.as_ref())?;
        let discovery_mode = effective_discovery_mode(&params.config, policy);
        let allowed_pack_refs = params
            .candidate_packs
            .iter()
            .map(|pack| pack.pack_ref().key())
            .collect::<BTreeSet<_>>();
        let query_terms = query_terms(&params.query);

        let mut matches = Vec::new();
        let mut suppressed = Vec::new();
        let mut notes = Vec::new();

        for pack in self.packs.values() {
            if !allowed_pack_refs.is_empty()
                && !allowed_pack_refs.contains(&pack.manifest.pack_ref().key())
            {
                continue;
            }
            if let Some(item) =
                self.suppression_reason(pack, role, policy, &selected_target, &discovery_mode)
            {
                if pack.is_candidate() {
                    notes.push(format!(
                        "Quarantined candidate {} was withheld from active matches.",
                        pack.manifest.id
                    ));
                }
                suppressed.push(item);
                continue;
            }
            let evidence = search_match_evidence(pack, &query_terms)?;
            let score = relevance_score(pack, &query_terms, role, &selected_target, &evidence);
            if score <= 0.0 {
                continue;
            }
            matches.push(SearchMatch {
                pack_ref: pack.manifest.pack_ref(),
                score,
                why: why_string(pack, &evidence),
                trust_tier: pack.manifest.trust_tier.clone(),
                requires_confirmation: pack.manifest.requires_confirmation,
                provenance_ref: pack.provenance_ref.clone(),
                match_evidence: Some(evidence),
                lifecycle: pack.manifest.lifecycle.clone(),
            });
        }

        matches.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.pack_ref.key().cmp(&right.pack_ref.key()))
        });
        suppressed.sort_by(|left, right| left.pack_ref.key().cmp(&right.pack_ref.key()));
        notes.sort();
        notes.dedup();

        if let Some(limit) = params.limit {
            matches.truncate(limit as usize);
        }

        Ok(SearchResult {
            api_version: crate::types::API_VERSION.to_string(),
            query: params.query,
            discovery_mode,
            matches,
            suppressed,
            notes,
        })
    }

    pub fn resolve(&self, params: ResolveParams) -> Result<ResolveGraph> {
        let role = self.find_role(&params.config.role)?;
        let policy = self.find_policy(&params.config.policy)?;
        let selected_target = self.selected_target(&params.config, params.overlay.as_ref())?;
        let discovery_mode = effective_discovery_mode(&params.config, policy);

        if !params
            .available_targets
            .iter()
            .any(|target| target.target_ref() == selected_target)
        {
            return Err(anyhow!(
                "selected target {} is not available",
                selected_target.id
            ));
        }

        let requested_pack_refs = if params.config.packs.is_empty() {
            role.default_pack_refs.clone()
        } else {
            params.config.packs.clone()
        };

        let mut activated_pack_refs = Vec::new();
        let mut suppressed_packs = Vec::new();
        let mut provenance_refs = Vec::new();
        let mut pack_visibility = BTreeMap::new();

        for pack_ref in &requested_pack_refs {
            match self.find_pack(pack_ref) {
                Some(pack) => {
                    if let Some(item) = self.suppression_reason(
                        pack,
                        role,
                        policy,
                        &selected_target,
                        &discovery_mode,
                    ) {
                        suppressed_packs.push(item);
                        continue;
                    }
                    activated_pack_refs.push(pack.manifest.pack_ref());
                    pack_visibility.insert(
                        pack.manifest.id.clone(),
                        pack.manifest.visibility_scope.clone(),
                    );
                    if let Some(provenance_ref) = &pack.provenance_ref {
                        provenance_refs.push(format!("artifact:{}", provenance_ref.id));
                    }
                }
                None => suppressed_packs.push(SuppressedRef {
                    pack_ref: pack_ref.clone(),
                    reason_code: ReasonCode::NotFound,
                    detail: Some(
                        "Pack was not discovered in the configured library roots.".to_string(),
                    ),
                }),
            }
        }

        let mut capability_gaps = Vec::new();
        if activated_pack_refs.is_empty() {
            capability_gaps.push(CapabilityGap {
                feature: "pack_selection".to_string(),
                reason_code: ReasonCode::ZeroMatch,
                affected_refs: vec![role.role_ref(), policy.policy_ref()],
            });
        }

        Ok(ResolveGraph {
            api_version: crate::types::API_VERSION.to_string(),
            source_config_digest: Some(digest_json(&params.config)?),
            overlay_digest: params.overlay.as_ref().map(digest_json).transpose()?,
            role: role.role_ref(),
            selected_target,
            requested_pack_refs,
            activated_pack_refs,
            suppressed_packs,
            applied_policies: vec![policy.policy_ref()],
            capability_gaps,
            provenance_refs,
            brownfield_mode: params
                .config
                .defaults
                .as_ref()
                .and_then(|defaults| defaults.brownfield_mode.clone()),
            pack_visibility,
        })
    }

    pub fn explain(&self, params: ExplainParams) -> ExplainResult {
        let resolve_graph = params.resolve_graph;
        let active_pack_count = resolve_graph.activated_pack_refs.len();
        let summary = if resolve_graph
            .capability_gaps
            .iter()
            .any(|gap| gap.reason_code == ReasonCode::ZeroMatch)
        {
            format!(
                "{} resolved to {} with role and policy only because no usable packs were available.",
                resolve_graph.role.id, resolve_graph.selected_target.id
            )
        } else {
            format!(
                "{} resolved to {} with {} active pack(s) and {} suppressed pack(s).",
                resolve_graph.role.id,
                resolve_graph.selected_target.id,
                active_pack_count,
                resolve_graph.suppressed_packs.len()
            )
        };

        let mut what_is_active = vec![
            format!("role {}", resolve_graph.role.id),
            format!("target {}", resolve_graph.selected_target.id),
        ];
        what_is_active.extend(resolve_graph.activated_pack_refs.iter().map(|pack| {
            let vis = resolve_graph
                .pack_visibility
                .get(&pack.id)
                .cloned()
                .unwrap_or_default();
            match vis {
                VisibilityScope::Private => format!("pack {} (private)", pack.id),
                VisibilityScope::Shared => format!("pack {}", pack.id),
            }
        }));

        let mut why_it_is_active = Vec::new();
        why_it_is_active.push(crate::types::ExplanationReason {
            subject_ref: resolve_graph.role.clone(),
            reason: "Requested durable role remained active.".to_string(),
        });
        why_it_is_active.push(crate::types::ExplanationReason {
            subject_ref: resolve_graph.selected_target.clone(),
            reason: "Selected target was available at resolve time.".to_string(),
        });
        why_it_is_active.extend(resolve_graph.activated_pack_refs.iter().cloned().map(
            |pack_ref| crate::types::ExplanationReason {
                subject_ref: pack_ref,
                reason: "Pack satisfied role, target, and policy constraints.".to_string(),
            },
        ));

        let what_was_suppressed = resolve_graph
            .suppressed_packs
            .iter()
            .map(|item| SuppressedSubject {
                subject_ref: item.pack_ref.clone(),
                reason_code: item.reason_code.clone(),
                detail: item.detail.clone(),
            })
            .collect();

        let unknown_or_unsupported = resolve_graph
            .capability_gaps
            .iter()
            .map(|gap| format!("{}: {:?}", gap.feature, gap.reason_code))
            .collect();

        ExplainResult {
            api_version: crate::types::API_VERSION.to_string(),
            summary,
            what_is_active,
            why_it_is_active,
            what_was_suppressed,
            unknown_or_unsupported,
            resolve_graph,
        }
    }

    pub fn compile(&self, params: CompileParams) -> Result<CompileResult> {
        if params.resolve_graph.selected_target != params.target_capability.target_ref() {
            return Err(anyhow!(
                "compile target mismatch: graph={} request={}",
                params.resolve_graph.selected_target.id,
                params.target_capability.target_id
            ));
        }

        let project_root = compile_project_root(params.project_root.as_deref())?;
        let role = self.find_role(&params.resolve_graph.role)?;
        let policy_ref = params
            .resolve_graph
            .applied_policies
            .first()
            .ok_or_else(|| anyhow!("resolve graph is missing an applied policy"))?;
        let policy = self.find_policy(policy_ref)?;
        let active_packs = params
            .resolve_graph
            .activated_pack_refs
            .iter()
            .map(|pack_ref| {
                self.find_pack(pack_ref).ok_or_else(|| {
                    anyhow!(
                        "pack {} was not discovered in configured library roots",
                        pack_ref.id
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let policy_report =
            build_policy_report(policy, &params.target_capability, &params.resolve_graph);
        let mut degradations = params.resolve_graph.capability_gaps.clone();
        degradations.extend(pack_degradations(&active_packs, &params.target_capability));
        degradations.extend(policy_degradations(&policy_report));
        dedupe_degradations(&mut degradations);

        let effective_surface_selection_mode = skill_compile_target_for(&params.target_capability)
            .map(|compile_target| {
                effective_surface_selection_mode(
                    compile_target,
                    params.surface_selection_mode.clone(),
                )
            });

        let (outputs, surface_selection, surface_degradations) = synthesize_outputs(
            role,
            policy,
            &active_packs,
            &params.resolve_graph,
            &params.target_capability,
            &self.roots,
            effective_surface_selection_mode.clone(),
        )?;
        degradations.extend(surface_degradations);
        dedupe_degradations(&mut degradations);
        let manifest = materializer::stage_outputs(
            &project_root,
            &params.target_capability.target_ref(),
            outputs,
            effective_surface_selection_mode,
            surface_selection,
            supported_apply_modes(&params.target_capability),
            params.resolve_graph.brownfield_mode.clone(),
            degradations,
            params.durable_staging,
        )?;

        Ok(CompileResult {
            compile_manifest: manifest,
            policy_enforcement_report: params.emit_policy_report.then_some(policy_report),
        })
    }

    pub fn validate(&self, params: ValidateParams) -> Result<ValidationReport> {
        let subject_ref = params.subject_ref.clone();
        let mut report = ValidationReport {
            api_version: crate::types::API_VERSION.to_string(),
            subject_ref: subject_ref.clone(),
            status: ValidationStatus::Pass,
            checks: Vec::new(),
            generated_at: None,
        };

        if let Some(compile_manifest) = params.compile_manifest.as_ref() {
            let staged_conflicts = validate_staged_outputs(
                compile_project_root(params.project_root.as_deref())?,
                compile_manifest,
            )?;
            if staged_conflicts.is_empty() {
                report.checks.push(ValidationCheck {
                    id: "staged-output-digests".to_string(),
                    status: ValidationStatus::Pass,
                    message: "Staged outputs match recorded digests.".to_string(),
                    artifact_ref: Some(compile_manifest.target.clone()),
                });
            } else {
                report.status = ValidationStatus::Fail;
                report.checks.push(ValidationCheck {
                    id: "staged-output-digests".to_string(),
                    status: ValidationStatus::Fail,
                    message: staged_conflicts.join(" "),
                    artifact_ref: Some(compile_manifest.target.clone()),
                });
            }
        }

        if let Some(project_root) = params.project_root.as_deref() {
            let drift = materializer::drift_conflicts(
                &compile_project_root(Some(project_root))?,
                &subject_ref,
            )?;
            if drift.len() == 1 && drift[0].reason_code == ReasonCode::NotFound {
                report.checks.push(ValidationCheck {
                    id: "managed-state".to_string(),
                    status: ValidationStatus::Warn,
                    message: drift[0].detail.clone(),
                    artifact_ref: Some(subject_ref.clone()),
                });
                if report.status == ValidationStatus::Pass {
                    report.status = ValidationStatus::Warn;
                }
            } else if drift.is_empty() {
                report.checks.push(ValidationCheck {
                    id: "applied-output-drift".to_string(),
                    status: ValidationStatus::Pass,
                    message: "Applied outputs match recorded managed state.".to_string(),
                    artifact_ref: Some(subject_ref.clone()),
                });
            } else {
                report.status = ValidationStatus::Fail;
                report.checks.push(ValidationCheck {
                    id: "applied-output-drift".to_string(),
                    status: ValidationStatus::Fail,
                    message: drift
                        .into_iter()
                        .map(|item| format!("{}: {}", item.destination_path, item.detail))
                        .collect::<Vec<_>>()
                        .join(" "),
                    artifact_ref: Some(subject_ref.clone()),
                });
            }
        }

        if let Some(policy_report) = params.policy_enforcement_report.as_ref() {
            let degraded = policy_report
                .rules
                .iter()
                .any(|rule| rule.status == EnforcementStatus::Degraded);
            report.checks.push(ValidationCheck {
                id: "policy-enforcement".to_string(),
                status: if degraded {
                    ValidationStatus::Warn
                } else {
                    ValidationStatus::Pass
                },
                message: if degraded {
                    "One or more policy rules were degraded on the selected target.".to_string()
                } else {
                    "Policy enforcement remained within requested guarantees.".to_string()
                },
                artifact_ref: Some(subject_ref.clone()),
            });
            if degraded && report.status == ValidationStatus::Pass {
                report.status = ValidationStatus::Warn;
            }
        }

        if report.checks.is_empty() {
            report.checks.push(ValidationCheck {
                id: "validation-inputs".to_string(),
                status: ValidationStatus::Warn,
                message: "No compile manifest, policy report, or project root was provided."
                    .to_string(),
                artifact_ref: Some(subject_ref),
            });
            report.status = ValidationStatus::Warn;
        }

        Ok(report)
    }

    pub fn apply_manifest(
        &self,
        project_root: &Path,
        manifest: &CompileManifest,
        apply_mode: &ApplyMode,
    ) -> Result<ApplyReport> {
        materializer::apply_manifest(project_root, manifest, apply_mode)
    }

    pub fn revert_target(&self, project_root: &Path, target: &Ref) -> Result<RevertReport> {
        materializer::revert_target(project_root, target)
    }

    pub fn detect_drift(&self, project_root: &Path, target: &Ref) -> Result<ValidationReport> {
        self.validate(ValidateParams {
            subject_ref: target.clone(),
            resolve_graph: None,
            compile_manifest: None,
            policy_enforcement_report: None,
            project_root: Some(project_root.to_string_lossy().to_string()),
        })
    }

    pub fn starter_library_inventory_report(&self) -> Result<(usize, usize, usize)> {
        let promoted = self
            .packs
            .values()
            .filter(|pack| pack.promotion_status == PromotionStatus::Promoted)
            .count();
        if promoted == 0 {
            return Err(anyhow!(
                "starter library did not include any promoted packs"
            ));
        }
        Ok((self.roles.len(), self.policies.len(), self.packs.len()))
    }

    fn load_root(
        &mut self,
        root: &Path,
        provenance_by_subject: &mut BTreeMap<String, ProvenanceEnvelope>,
    ) -> Result<()> {
        for path in sorted_glob_json(&root.join("roles"))? {
            let manifest: RoleManifest = load_json(path.clone())?;
            self.register_role(path, manifest)?;
        }
        for path in sorted_glob_json(&root.join("policies"))? {
            let manifest: PolicyManifest = load_json(path.clone())?;
            self.register_policy(path, manifest)?;
        }
        for path in sorted_glob_json(&root.join("targets"))? {
            let manifest: TargetCapabilityMatrix = load_json(path.clone())?;
            self.register_target(path, manifest)?;
        }
        for path in sorted_glob_json(&root.join("knowledge_sources"))? {
            let manifest: KnowledgeSourceManifest = load_json(path.clone())?;
            self.register_knowledge_source(path, manifest)?;
        }
        for path in sorted_glob_json(&root.join("packs"))? {
            let manifest: PackManifest = load_json(path.clone())?;
            self.register_pack(root, path, manifest, None)?;
        }
        for path in sorted_glob_json(&root.join("provenance"))? {
            let envelope: ProvenanceEnvelope = load_json(path)?;
            register_unique_manifest(
                provenance_by_subject,
                envelope.subject_ref.key(),
                envelope.clone(),
                "provenance",
                &envelope.origin,
                |existing: &ProvenanceEnvelope| existing.subject_ref.version.clone(),
                |current: &ProvenanceEnvelope| current.subject_ref.version.clone(),
            )?;
        }
        for path in sorted_imports(&root.join("imports"))? {
            let (manifest, provenance) = normalize_candidate(root, &path)?;
            self.register_pack(root, path, manifest, Some(provenance))?;
        }
        Ok(())
    }

    fn register_role(&mut self, path: PathBuf, manifest: RoleManifest) -> Result<()> {
        register_unique_manifest(
            &mut self.roles,
            manifest.id.clone(),
            manifest,
            "role",
            path.display().to_string().as_str(),
            |existing: &RoleManifest| Some(existing.version.clone()),
            |current: &RoleManifest| Some(current.version.clone()),
        )
    }

    fn register_policy(&mut self, path: PathBuf, manifest: PolicyManifest) -> Result<()> {
        register_unique_manifest(
            &mut self.policies,
            manifest.id.clone(),
            manifest,
            "policy",
            path.display().to_string().as_str(),
            |existing: &PolicyManifest| Some(existing.version.clone()),
            |current: &PolicyManifest| Some(current.version.clone()),
        )
    }

    fn register_target(&mut self, path: PathBuf, manifest: TargetCapabilityMatrix) -> Result<()> {
        register_unique_manifest(
            &mut self.targets,
            manifest.target_id.clone(),
            manifest,
            "target",
            path.display().to_string().as_str(),
            |existing: &TargetCapabilityMatrix| Some(existing.version.clone()),
            |current: &TargetCapabilityMatrix| Some(current.version.clone()),
        )
    }

    fn register_knowledge_source(
        &mut self,
        path: PathBuf,
        manifest: KnowledgeSourceManifest,
    ) -> Result<()> {
        register_unique_manifest(
            &mut self.knowledge_sources,
            manifest.id.clone(),
            manifest,
            "knowledge_source",
            path.display().to_string().as_str(),
            |existing: &KnowledgeSourceManifest| Some(existing.version.clone()),
            |current: &KnowledgeSourceManifest| Some(current.version.clone()),
        )
    }

    fn register_pack(
        &mut self,
        root: &Path,
        path: PathBuf,
        manifest: PackManifest,
        provenance: Option<ProvenanceEnvelope>,
    ) -> Result<()> {
        let key = manifest.id.clone();
        let candidate = provenance
            .as_ref()
            .and_then(|item| item.review.as_ref())
            .and_then(|review| review.promotion_status.clone())
            .unwrap_or(PromotionStatus::Promoted);
        match self.packs.entry(key.clone()) {
            Entry::Vacant(entry) => {
                let provenance_ref = provenance.as_ref().map(|_| provenance_ref_for(&manifest));
                entry.insert(DiscoveredPack {
                    manifest,
                    provenance,
                    provenance_ref,
                    source_path: path,
                    library_root: root.to_path_buf(),
                    promotion_status: candidate,
                });
                Ok(())
            }
            Entry::Occupied(entry) => {
                let existing = entry.get();
                if existing.manifest.version == manifest.version {
                    Err(anyhow!(
                        "duplicate pack {}@{} discovered at {} and {}",
                        manifest.id,
                        manifest.version,
                        existing.source_path.display(),
                        path.display()
                    ))
                } else {
                    Err(anyhow!(
                        "conflicting pack {} discovered with versions {} and {} at {} and {}",
                        manifest.id,
                        existing.manifest.version,
                        manifest.version,
                        existing.source_path.display(),
                        path.display()
                    ))
                }
            }
        }
    }

    fn find_role(&self, role_ref: &Ref) -> Result<&RoleManifest> {
        self.roles
            .get(&role_ref.id)
            .filter(|manifest| ref_matches_version(role_ref, &manifest.version))
            .ok_or_else(|| {
                anyhow!(
                    "role {} was not discovered in configured library roots",
                    role_ref.id
                )
            })
    }

    fn find_policy(&self, policy_ref: &Ref) -> Result<&PolicyManifest> {
        self.policies
            .get(&policy_ref.id)
            .filter(|manifest| ref_matches_version(policy_ref, &manifest.version))
            .ok_or_else(|| {
                anyhow!(
                    "policy {} was not discovered in configured library roots",
                    policy_ref.id
                )
            })
    }

    fn selected_target(&self, config: &Config, overlay: Option<&InvocationOverlay>) -> Result<Ref> {
        let target_ref = selected_target_from_config(config, overlay)
            .ok_or_else(|| anyhow!("request does not include a selected target"))?;
        self.targets
            .get(&target_ref.id)
            .filter(|manifest| ref_matches_version(&target_ref, &manifest.version))
            .map(|manifest| manifest.target_ref())
            .ok_or_else(|| {
                anyhow!(
                    "target {} was not discovered in configured library roots",
                    target_ref.id
                )
            })
    }

    fn find_pack(&self, pack_ref: &Ref) -> Option<&DiscoveredPack> {
        self.packs
            .get(&pack_ref.id)
            .filter(|pack| ref_matches_version(pack_ref, &pack.manifest.version))
    }

    fn suppression_reason(
        &self,
        pack: &DiscoveredPack,
        role: &RoleManifest,
        policy: &PolicyManifest,
        selected_target: &Ref,
        discovery_mode: &DiscoveryMode,
    ) -> Option<SuppressedRef> {
        if !pack.manifest.compatible_roles.is_empty()
            && !pack
                .manifest
                .compatible_roles
                .iter()
                .any(|item| item == &role.id)
        {
            return Some(SuppressedRef {
                pack_ref: pack.manifest.pack_ref(),
                reason_code: ReasonCode::IncompatibleRole,
                detail: Some(format!("Pack does not support role {}.", role.id)),
            });
        }
        if !pack.manifest.compatible_targets.is_empty()
            && !pack
                .manifest
                .compatible_targets
                .iter()
                .any(|item| item == &selected_target.id)
        {
            return Some(SuppressedRef {
                pack_ref: pack.manifest.pack_ref(),
                reason_code: ReasonCode::UnsupportedTarget,
                detail: Some(format!(
                    "Pack does not support target {}.",
                    selected_target.id
                )),
            });
        }
        if pack.is_candidate()
            && !matches!(
                discovery_mode,
                &DiscoveryMode::CandidateSearch | &DiscoveryMode::Exploratory
            )
        {
            return Some(SuppressedRef {
                pack_ref: pack.manifest.pack_ref(),
                reason_code: ReasonCode::SuppressedByMode,
                detail: Some("Discovery mode does not permit quarantined candidates.".to_string()),
            });
        }
        policy_rule_suppression(pack, policy)
    }
}

fn compile_project_root(project_root: Option<&str>) -> Result<PathBuf> {
    match project_root {
        Some(path) => Ok(PathBuf::from(path)),
        None => std::env::current_dir().context("determine current directory for compile"),
    }
}

fn validate_skill_frontmatter_text(text: &str) -> Vec<String> {
    let mut failures = Vec::new();
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        failures.push("SKILL.md must start with YAML frontmatter.".to_string());
        return failures;
    }

    let mut yaml_lines = Vec::new();
    let mut closed = false;
    for line in lines {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        yaml_lines.push(line);
    }
    if !closed {
        failures.push("SKILL.md frontmatter is not closed.".to_string());
        return failures;
    }

    let yaml = yaml_lines.join("\n");
    let value: serde_yaml::Value = match serde_yaml::from_str(&yaml) {
        Ok(value) => value,
        Err(err) => {
            failures.push(format!("SKILL.md frontmatter invalid YAML: {err}."));
            return failures;
        }
    };

    for field in ["name", "description"] {
        match value.get(field).and_then(|item| item.as_str()) {
            Some(value) if !value.trim().is_empty() => {}
            _ => failures.push(format!("SKILL.md frontmatter.{field} is required.")),
        }
    }

    failures
}

fn validate_generated_skill_frontmatter(path: &Path) -> Result<Vec<String>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("read generated skill {}", path.display()))?;
    Ok(validate_skill_frontmatter_text(&text))
}

fn validate_staged_outputs(
    project_root: PathBuf,
    manifest: &CompileManifest,
) -> Result<Vec<String>> {
    let mut failures = Vec::new();
    for output in &manifest.generated_outputs {
        let staged_abs = project_root.join(&output.path);
        if !staged_abs.exists() {
            failures.push(format!("{} is missing from the staging area.", output.path));
            continue;
        }
        if output.kind == GeneratedOutputKind::SkillFolder {
            for failure in validate_generated_skill_frontmatter(&staged_abs)? {
                failures.push(format!("{}: {}", output.path, failure));
            }
        }
        if let Some(expected) = output.digest.as_deref() {
            let actual = sha256_digest(&staged_abs)?;
            if actual != expected {
                failures.push(format!(
                    "{} digest mismatch: expected {} got {}.",
                    output.path, expected, actual
                ));
            }
        }
    }
    Ok(failures)
}

fn render_frontmatter(frontmatter: &BTreeMap<String, String>) -> Result<String> {
    let typed_frontmatter = frontmatter
        .iter()
        .map(|(key, value)| (key.clone(), yaml_frontmatter_value(value)))
        .collect::<BTreeMap<_, _>>();
    let yaml = serde_yaml::to_string(&typed_frontmatter).context("serialize YAML frontmatter")?;
    let mut out = String::from("---\n");
    out.push_str(&yaml);
    if !yaml.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("---\n");
    Ok(out)
}

fn yaml_frontmatter_value(value: &str) -> serde_yaml::Value {
    match value {
        "true" => serde_yaml::Value::Bool(true),
        "false" => serde_yaml::Value::Bool(false),
        _ => serde_yaml::Value::String(value.to_string()),
    }
}

fn wrap_with_frontmatter(body: &[u8], frontmatter: &BTreeMap<String, String>) -> Result<Vec<u8>> {
    if frontmatter.is_empty() {
        return Ok(body.to_vec());
    }
    let mut out = render_frontmatter(frontmatter)?;
    out.push_str(std::str::from_utf8(body).unwrap_or(""));
    Ok(out.into_bytes())
}

fn synthesize_outputs(
    role: &RoleManifest,
    policy: &PolicyManifest,
    packs: &[&DiscoveredPack],
    resolve_graph: &ResolveGraph,
    target: &TargetCapabilityMatrix,
    library_roots: &[PathBuf],
    surface_selection_override: Option<SurfaceSelectionMode>,
) -> Result<(
    Vec<StagedOutputInput>,
    Vec<SurfaceSelectionDecision>,
    Vec<CapabilityGap>,
)> {
    let mut outputs = Vec::new();
    let mut surface_selection = Vec::new();
    let mut degradations = Vec::new();
    let mut emitted_local_instruction_document = false;
    for compile_target in &target.compile_targets {
        match compile_target.output_kind {
            CompileTargetKind::ClaudeMd
            | CompileTargetKind::AgentsMd
            | CompileTargetKind::OpenclawMd => {
                let destination = compile_target.path_template.clone();

                // Split packs by visibility scope
                let shared_packs: Vec<&DiscoveredPack> = packs
                    .iter()
                    .filter(|p| p.manifest.visibility_scope == VisibilityScope::Shared)
                    .copied()
                    .collect();
                let private_packs: Vec<&DiscoveredPack> = packs
                    .iter()
                    .filter(|p| p.manifest.visibility_scope == VisibilityScope::Private)
                    .copied()
                    .collect();

                // Primary instruction document: shared packs only
                let plan = instruction_document_plan(
                    role,
                    policy,
                    &shared_packs,
                    resolve_graph,
                    target,
                    compile_target,
                )?;
                let document = instruction_document(role, policy, &plan, resolve_graph, target)?;
                let mut degradation_codes = plan.degradation_codes;
                if document.truncated {
                    degradation_codes.push("instruction_index_truncated".to_string());
                }
                outputs.push(StagedOutputInput {
                    id: Some(document_id_for_target(target)),
                    destination_path: destination,
                    kind: GeneratedOutputKind::InstructionFile,
                    contents: wrap_with_frontmatter(
                        document.content.as_bytes(),
                        &compile_target.instruction_frontmatter,
                    )?,
                    instruction_mode: Some(plan.mode),
                    pack_ref: None,
                    surface_id: None,
                    surface_slug: None,
                    source_resource_paths: plan.source_resource_paths,
                    merge_status: None,
                    degradation_codes,
                    ownership_token: Some(format!(
                        "{}::{}",
                        target.target_id,
                        document_id_for_target(target)
                    )),
                });

                // Local instruction document: private packs only
                if !private_packs.is_empty() && !emitted_local_instruction_document {
                    let has_exact_local = target
                        .local_projection
                        .as_ref()
                        .map(|lp| {
                            lp.support == LocalProjectionSupport::Exact
                                && lp.local_surface.is_some()
                        })
                        .unwrap_or(false);

                    if has_exact_local {
                        let local_surface = target
                            .local_projection
                            .as_ref()
                            .and_then(|lp| lp.local_surface.clone())
                            .unwrap();
                        let local_plan = instruction_document_plan(
                            role,
                            policy,
                            &private_packs,
                            resolve_graph,
                            target,
                            compile_target,
                        )?;
                        let local_document =
                            instruction_document(role, policy, &local_plan, resolve_graph, target)?;
                        let local_id = format!("{}-local", document_id_for_target(target));
                        let mut local_degradation_codes = local_plan.degradation_codes;
                        if local_document.truncated {
                            local_degradation_codes.push("instruction_index_truncated".to_string());
                        }
                        outputs.push(StagedOutputInput {
                            id: Some(local_id.clone()),
                            destination_path: local_surface,
                            kind: GeneratedOutputKind::InstructionFile,
                            contents: wrap_with_frontmatter(
                                local_document.content.as_bytes(),
                                &compile_target.instruction_frontmatter,
                            )?,
                            instruction_mode: Some(local_plan.mode),
                            pack_ref: None,
                            surface_id: None,
                            surface_slug: None,
                            source_resource_paths: local_plan.source_resource_paths,
                            merge_status: None,
                            degradation_codes: local_degradation_codes,
                            ownership_token: Some(format!("{}::{}", target.target_id, local_id)),
                        });
                        emitted_local_instruction_document = true;
                    } else {
                        // Target lacks local surface — record degradation
                        degradations.push(CapabilityGap {
                            feature: format!(
                                "private_packs_no_local_surface:{}",
                                private_packs.len()
                            ),
                            reason_code: ReasonCode::CapabilityGap,
                            affected_refs: private_packs
                                .iter()
                                .map(|p| p.manifest.pack_ref())
                                .collect(),
                        });
                        emitted_local_instruction_document = true;
                    }
                }
            }
            CompileTargetKind::CodexSkill => {
                let selection_mode = effective_surface_selection_mode(
                    compile_target,
                    surface_selection_override.clone(),
                );
                for pack in packs {
                    let surfaces = derive_skill_surfaces(pack)?;
                    let decisions =
                        surface_selection_decisions(pack, &surfaces, selection_mode.clone());
                    surface_selection.extend(decisions.clone());
                    let emitted_surfaces = surfaces
                        .iter()
                        .filter(|surface| {
                            decisions.iter().any(|decision| {
                                decision.surface_id == surface.surface_id && decision.emitted
                            })
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    if emitted_surfaces.is_empty() {
                        continue;
                    }
                    if should_emit_separate_surfaces(target, compile_target, &emitted_surfaces)? {
                        for surface in &emitted_surfaces {
                            let destination = expand_skill_path(
                                &compile_target.path_template,
                                &pack.manifest.id,
                                Some(&surface.surface_slug),
                            )?;
                            outputs.push(StagedOutputInput {
                                id: Some(format!(
                                    "skill-{}-{}",
                                    pack.manifest.id, surface.surface_slug
                                )),
                                destination_path: destination,
                                kind: GeneratedOutputKind::SkillFolder,
                                contents: skill_surface_document(
                                    pack,
                                    surface,
                                    compile_target.supports_surface_frontmatter,
                                )?,
                                instruction_mode: None,
                                pack_ref: Some(pack.manifest.pack_ref()),
                                surface_id: Some(surface.surface_id.clone()),
                                surface_slug: Some(surface.surface_slug.clone()),
                                source_resource_paths: surface.instruction_resource_paths.clone(),
                                merge_status: Some(SurfaceMergeStatus::Separate),
                                degradation_codes: Vec::new(),
                                ownership_token: Some(format!(
                                    "{}::{}",
                                    pack.manifest.id, surface.surface_id
                                )),
                            });
                        }
                    } else {
                        let destination = expand_skill_path(
                            &compile_target.path_template,
                            &pack.manifest.id,
                            None,
                        )?;
                        let merged_surfaces = emitted_surfaces
                            .iter()
                            .map(|surface| surface.surface_slug.clone())
                            .collect::<Vec<_>>();
                        let mut degradation_codes = Vec::new();
                        let merge_status = if emitted_surfaces.len() > 1 {
                            degradation_codes.push("merged_surface_pack".to_string());
                            degradations.push(CapabilityGap {
                                feature: format!("surface_merge:{}", pack.manifest.id),
                                reason_code: ReasonCode::CapabilityGap,
                                affected_refs: vec![pack.manifest.pack_ref()],
                            });
                            Some(SurfaceMergeStatus::Merged)
                        } else {
                            Some(SurfaceMergeStatus::Separate)
                        };
                        outputs.push(StagedOutputInput {
                            id: Some(format!("skill-{}", pack.manifest.id)),
                            destination_path: destination,
                            kind: GeneratedOutputKind::SkillFolder,
                            contents: merged_skill_document(pack)?,
                            instruction_mode: None,
                            pack_ref: Some(pack.manifest.pack_ref()),
                            surface_id: if emitted_surfaces.len() == 1 {
                                emitted_surfaces
                                    .first()
                                    .map(|surface| surface.surface_id.clone())
                            } else {
                                None
                            },
                            surface_slug: if emitted_surfaces.len() == 1 {
                                emitted_surfaces
                                    .first()
                                    .map(|surface| surface.surface_slug.clone())
                            } else {
                                None
                            },
                            source_resource_paths: emitted_surfaces
                                .iter()
                                .flat_map(|surface| surface.instruction_resource_paths.clone())
                                .collect(),
                            merge_status,
                            degradation_codes,
                            ownership_token: Some(format!(
                                "{}::{}",
                                pack.manifest.id,
                                if merged_surfaces.is_empty() {
                                    "skill".to_string()
                                } else {
                                    merged_surfaces.join("+")
                                }
                            )),
                        });
                    }
                }
            }
            CompileTargetKind::PackResource => {
                outputs.extend(emit_pack_resource_outputs(compile_target, packs)?);
            }
            CompileTargetKind::PackExtensionManifest => {
                outputs.extend(emit_pack_extension_manifests(compile_target, packs)?);
            }
            CompileTargetKind::HookConfig
            | CompileTargetKind::RuntimeJson
            | CompileTargetKind::Other => {
                // Runtime config bodies are now emitted via target.runtime_template
                // (see expand_runtime_template). Inline CompileTarget entries with
                // these kinds are legacy and ignored so target JSONs can be
                // rewritten incrementally; Phase 3 of spec 019 removes them.
            }
            CompileTargetKind::McpConfig => {
                let destination = compile_target.path_template.clone();
                let contents = serde_json::to_vec_pretty(&serde_json::json!({
                    "target": target.target_id,
                    "policies": resolve_graph.applied_policies.iter().map(|item| &item.id).collect::<Vec<_>>(),
                    "active_packs": resolve_graph.activated_pack_refs.iter().map(|item| &item.id).collect::<Vec<_>>(),
                }))?;
                outputs.push(StagedOutputInput {
                    id: Some("mcp-config".to_string()),
                    destination_path: destination,
                    kind: GeneratedOutputKind::McpConfig,
                    contents,
                    instruction_mode: None,
                    pack_ref: None,
                    surface_id: None,
                    surface_slug: None,
                    source_resource_paths: Vec::new(),
                    merge_status: None,
                    degradation_codes: Vec::new(),
                    ownership_token: Some("mcp-config".to_string()),
                });
            }
        }
    }

    if let Some(template_ref) = &target.runtime_template {
        let (kind, contents) = expand_runtime_template(
            library_roots,
            template_ref,
            target,
            policy,
            resolve_graph,
            packs,
        )?;
        let destination = template_ref.destination_path.clone();
        let ownership = format!("{}::runtime-template", target.target_id);
        outputs.push(StagedOutputInput {
            id: Some(format!("{}-runtime-config", target.target_id)),
            destination_path: destination,
            kind,
            contents,
            instruction_mode: None,
            pack_ref: None,
            surface_id: None,
            surface_slug: None,
            source_resource_paths: vec![template_ref.path.clone()],
            merge_status: None,
            degradation_codes: Vec::new(),
            ownership_token: Some(ownership),
        });
    }

    Ok((outputs, surface_selection, degradations))
}

fn build_policy_report(
    policy: &PolicyManifest,
    target: &TargetCapabilityMatrix,
    resolve_graph: &ResolveGraph,
) -> PolicyEnforcementReport {
    let rules = policy
        .rules
        .iter()
        .map(|rule| {
            let affected_refs = affected_refs_for_rule(rule.subject.clone(), resolve_graph);
            match rule.operator {
                PolicyOperator::Deny | PolicyOperator::QuarantineOnly => PolicyRuleReport {
                    rule_id: rule.id.clone(),
                    requested_enforcement_class: rule.requested_enforcement_class.clone(),
                    realized_enforcement_class: RealizedEnforcementClass::EnforceableLocal,
                    status: EnforcementStatus::Enforced,
                    enforcement_surface: Some("resolver suppression".to_string()),
                    rationale: Some("The resolver withheld disallowed packs before compilation.".to_string()),
                    affected_refs,
                },
                PolicyOperator::RequireApproval => {
                    let (realized, status, surface, rationale) =
                        require_approval_realization(rule.subject.clone(), target);
                    PolicyRuleReport {
                        rule_id: rule.id.clone(),
                        requested_enforcement_class: rule.requested_enforcement_class.clone(),
                        realized_enforcement_class: realized,
                        status,
                        enforcement_surface: Some(surface),
                        rationale: Some(rationale),
                        affected_refs,
                    }
                }
                PolicyOperator::BudgetCap => PolicyRuleReport {
                    rule_id: rule.id.clone(),
                    requested_enforcement_class: RequestedEnforcementClass::ExplainOnlyUnverifiable,
                    realized_enforcement_class: RealizedEnforcementClass::ExplainOnlyUnverifiable,
                    status: EnforcementStatus::Enforced,
                    enforcement_surface: Some("explain + shell budget UI".to_string()),
                    rationale: Some("Budget posture remains machine-readable for the shell even when the target cannot enforce it directly.".to_string()),
                    affected_refs,
                },
                _ => PolicyRuleReport {
                    rule_id: rule.id.clone(),
                    requested_enforcement_class: rule.requested_enforcement_class.clone(),
                    realized_enforcement_class: RealizedEnforcementClass::Advisory,
                    status: EnforcementStatus::Degraded,
                    enforcement_surface: Some("instruction surface".to_string()),
                    rationale: Some("The current target adapter only preserved this rule as advisory guidance.".to_string()),
                    affected_refs,
                },
            }
        })
        .collect();

    PolicyEnforcementReport {
        api_version: crate::types::API_VERSION.to_string(),
        target: target.target_ref(),
        rules,
    }
}

fn require_approval_realization(
    subject: PolicySubject,
    target: &TargetCapabilityMatrix,
) -> (RealizedEnforcementClass, EnforcementStatus, String, String) {
    match subject {
        PolicySubject::Filesystem if target.capabilities.deterministic_hooks => (
            RealizedEnforcementClass::EnforceableLocal,
            EnforcementStatus::Enforced,
            target_filesystem_surface(target),
            "Path-sensitive write controls remain enforceable on this target.".to_string(),
        ),
        PolicySubject::Network | PolicySubject::Artifact | PolicySubject::Filesystem
            if target.capabilities.approval_policies =>
        {
            (
                RealizedEnforcementClass::EnforceableLocal,
                EnforcementStatus::Enforced,
                approval_surface(target),
                "The target exposes approval controls for the requested action class.".to_string(),
            )
        }
        _ => (
            RealizedEnforcementClass::Advisory,
            EnforcementStatus::Degraded,
            advisory_surface(target),
            "The target lacks a deterministic hard surface for this rule, so metactl emitted advisory guidance instead.".to_string(),
        ),
    }
}

fn affected_refs_for_rule(subject: PolicySubject, resolve_graph: &ResolveGraph) -> Vec<Ref> {
    match subject {
        PolicySubject::Pack => resolve_graph
            .suppressed_packs
            .iter()
            .map(|item| item.pack_ref.clone())
            .collect(),
        PolicySubject::Filesystem | PolicySubject::Artifact => {
            resolve_graph.activated_pack_refs.clone()
        }
        _ => vec![Ref {
            kind: RefKind::Rule,
            id: format!("{:?}", subject).to_ascii_lowercase(),
            version: None,
        }],
    }
}

fn pack_degradations(
    packs: &[&DiscoveredPack],
    target: &TargetCapabilityMatrix,
) -> Vec<CapabilityGap> {
    let mut degradations = Vec::new();
    for pack in packs {
        match pack.manifest.activation_class {
            ActivationClass::Hook if !target.capabilities.deterministic_hooks => {
                degradations.push(CapabilityGap {
                    feature: "deterministic_hooks".to_string(),
                    reason_code: ReasonCode::UnsupportedTarget,
                    affected_refs: vec![pack.manifest.pack_ref()],
                });
            }
            ActivationClass::Script if !target.capabilities.local_scripts => {
                degradations.push(CapabilityGap {
                    feature: "local_scripts".to_string(),
                    reason_code: ReasonCode::UnsupportedTarget,
                    affected_refs: vec![pack.manifest.pack_ref()],
                });
            }
            ActivationClass::Service if !target.capabilities.mcp_servers => {
                degradations.push(CapabilityGap {
                    feature: "mcp_servers".to_string(),
                    reason_code: ReasonCode::UnsupportedTarget,
                    affected_refs: vec![pack.manifest.pack_ref()],
                });
            }
            _ => {}
        }
    }
    degradations
}

fn policy_degradations(report: &PolicyEnforcementReport) -> Vec<CapabilityGap> {
    report
        .rules
        .iter()
        .filter(|rule| rule.status == EnforcementStatus::Degraded)
        .map(|rule| CapabilityGap {
            feature: rule.rule_id.clone(),
            reason_code: ReasonCode::DegradedEnforcement,
            affected_refs: rule.affected_refs.clone(),
        })
        .collect()
}

fn dedupe_degradations(degradations: &mut Vec<CapabilityGap>) {
    let mut seen = BTreeSet::new();
    degradations.retain(|gap| {
        let key = format!(
            "{}::{:?}::{:?}",
            gap.feature, gap.reason_code, gap.affected_refs
        );
        seen.insert(key)
    });
}

fn supported_apply_modes(target: &TargetCapabilityMatrix) -> Vec<ApplyMode> {
    if !target.apply_modes.is_empty() {
        return target.apply_modes.clone();
    }
    let mut modes = vec![ApplyMode::Copy];
    if target.capabilities.layered_instructions {
        modes.push(ApplyMode::Patch);
    }
    if target.capabilities.local_scripts {
        modes.push(ApplyMode::Symlink);
    }
    modes
}

fn instruction_document_plan(
    _role: &RoleManifest,
    _policy: &PolicyManifest,
    packs: &[&DiscoveredPack],
    resolve_graph: &ResolveGraph,
    target: &TargetCapabilityMatrix,
    compile_target: &crate::types::CompileTarget,
) -> Result<InstructionDocumentPlan> {
    let desired_mode = compile_target
        .instruction_mode
        .clone()
        .unwrap_or(InstructionProjectionMode::Inline);
    if desired_mode == InstructionProjectionMode::Inline {
        return Ok(InstructionDocumentPlan {
            mode: InstructionProjectionMode::Inline,
            packs: packs
                .iter()
                .map(|pack| {
                    Ok(PlannedInstructionPack {
                        pack_ref: pack.manifest.pack_ref(),
                        title: pack.manifest.title.clone(),
                        description: pack.manifest.description.clone(),
                        when_to_open: when_to_open_for_pack(pack),
                        references: Vec::new(),
                        inline_snippet: Some(primary_instruction_snippet(pack)?),
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            source_resource_paths: packs
                .iter()
                .flat_map(|pack| {
                    pack.manifest
                        .resources
                        .iter()
                        .filter(|resource| resource.kind == ResourceKind::Instruction)
                        .map(|resource| resource.path.clone())
                        .collect::<Vec<_>>()
                })
                .collect(),
            degradation_codes: Vec::new(),
        });
    }

    let mut planned_packs = Vec::new();
    let mut source_resource_paths = Vec::new();
    let mut degradation_codes = Vec::new();
    let mut actual_mode = InstructionProjectionMode::ReferenceIndex;

    for pack in packs {
        let references = instruction_references_for_pack(pack, target)?;
        let mut inline_snippet = None;
        if references.is_empty() {
            let snippet = primary_instruction_snippet(pack)?;
            if !snippet.trim().is_empty() {
                inline_snippet = Some(snippet);
                actual_mode = InstructionProjectionMode::Inline;
                degradation_codes.push(format!("instruction_inline_fallback:{}", pack.manifest.id));
            }
        }
        source_resource_paths.extend(
            references
                .iter()
                .flat_map(|reference| reference.source_resource_paths.clone()),
        );
        if references.is_empty() {
            source_resource_paths.extend(
                pack.manifest
                    .resources
                    .iter()
                    .filter(|resource| resource.kind == ResourceKind::Instruction)
                    .map(|resource| resource.path.clone()),
            );
        }
        planned_packs.push(PlannedInstructionPack {
            pack_ref: pack.manifest.pack_ref(),
            title: pack.manifest.title.clone(),
            description: pack.manifest.description.clone(),
            when_to_open: when_to_open_for_pack(pack),
            references,
            inline_snippet,
        });
    }

    if !resolve_graph.capability_gaps.is_empty() && planned_packs.is_empty() {
        actual_mode = desired_mode;
    }

    source_resource_paths.sort();
    source_resource_paths.dedup();
    degradation_codes.sort();
    degradation_codes.dedup();

    Ok(InstructionDocumentPlan {
        mode: actual_mode,
        packs: planned_packs,
        source_resource_paths,
        degradation_codes,
    })
}

fn instruction_document(
    role: &RoleManifest,
    policy: &PolicyManifest,
    plan: &InstructionDocumentPlan,
    resolve_graph: &ResolveGraph,
    target: &TargetCapabilityMatrix,
) -> Result<BudgetedInstructionDocument> {
    let mut lines = vec![
        format!("# {}", role.title),
        String::new(),
        format!(
            "[metactl Instruction Index]|target:{}|policy:{}|mode:{}",
            target.target_id,
            policy.id,
            instruction_mode_label(&plan.mode)
        ),
        "|IMPORTANT: Prefer retrieval-led reasoning over pre-training-led reasoning.".to_string(),
        format!(
            "|budget:warn={}B|max={}B",
            INSTRUCTION_INDEX_WARN_BYTES, INSTRUCTION_INDEX_MAX_BYTES
        ),
    ];
    if plan.packs.is_empty() {
        lines.push("|packs:none".to_string());
    }
    for pack in &plan.packs {
        if !pack.references.is_empty() {
            let root = common_reference_root(&pack.references);
            let surfaces = pack
                .references
                .iter()
                .map(|reference| reference.locator.clone())
                .collect::<Vec<_>>()
                .join(",");
            let mut line = format!(
                "|pack:{}|title:{}|open:{}|surfaces:{}",
                pack.pack_ref.id, pack.title, root, surfaces
            );
            if !pack.when_to_open.is_empty() {
                line.push_str(&format!("|when:{}", pack.when_to_open.join(",")));
            }
            if let Some(description) = &pack.description {
                line.push_str(&format!("|summary:{}", description.trim()));
            }
            lines.push(line);
        } else if let Some(snippet) = &pack.inline_snippet {
            lines.push(format!(
                "|inline:{}|summary:{}",
                pack.pack_ref.id,
                summarize_inline_snippet(snippet)
            ));
        } else {
            lines.push(format!(
                "|pack:{}|summary:no-emitted-body",
                pack.pack_ref.id
            ));
        }
    }
    for gap in &resolve_graph.capability_gaps {
        lines.push(format!("|gap:{}={:?}", gap.feature, gap.reason_code));
    }

    budget_instruction_document(lines.join("\n"))
}

fn instruction_references_for_pack(
    pack: &DiscoveredPack,
    target: &TargetCapabilityMatrix,
) -> Result<Vec<InstructionReference>> {
    if let Some(compile_target) = skill_compile_target_for(target) {
        let surfaces = derive_skill_surfaces(pack)?;
        if should_emit_separate_surfaces(target, compile_target, &surfaces)? {
            return surfaces
                .into_iter()
                .map(|surface| {
                    Ok(InstructionReference {
                        path: expand_skill_path(
                            &compile_target.path_template,
                            &pack.manifest.id,
                            Some(&surface.surface_slug),
                        )?,
                        locator: surface.surface_slug,
                        source_resource_paths: surface.instruction_resource_paths,
                    })
                })
                .collect();
        }

        return Ok(vec![InstructionReference {
            path: expand_skill_path(&compile_target.path_template, &pack.manifest.id, None)?,
            locator: "bundle".to_string(),
            source_resource_paths: surfaces
                .into_iter()
                .flat_map(|surface| surface.instruction_resource_paths)
                .collect(),
        }]);
    }

    let Some(compile_target) = instruction_resource_compile_target_for(target) else {
        return Ok(Vec::new());
    };

    pack.manifest
        .resources
        .iter()
        .filter(|resource| resource.kind == ResourceKind::Instruction)
        .map(|resource| {
            let path = expand_pack_resource_path(&compile_target.path_template, pack, resource)?;
            Ok(InstructionReference {
                locator: compact_reference_locator(&path),
                path,
                source_resource_paths: vec![resource.path.clone()],
            })
        })
        .collect()
}

fn merged_skill_document(pack: &DiscoveredPack) -> Result<Vec<u8>> {
    if let Some(bytes) = primary_instruction_bytes(pack)? {
        return Ok(bytes);
    }
    let mut frontmatter = BTreeMap::new();
    frontmatter.insert("name".to_string(), pack.manifest.id.clone());
    frontmatter.insert(
        "description".to_string(),
        pack.manifest
            .description
            .clone()
            .unwrap_or_else(|| pack.manifest.title.clone()),
    );
    Ok(render_frontmatter(&frontmatter)?.into_bytes())
}

fn derive_skill_surfaces(pack: &DiscoveredPack) -> Result<Vec<DerivedSkillSurface>> {
    let instruction_resources = pack
        .manifest
        .resources
        .iter()
        .filter(|item| item.kind == ResourceKind::Instruction)
        .collect::<Vec<_>>();
    if instruction_resources.is_empty() {
        return Ok(vec![DerivedSkillSurface {
            surface_id: format!("{}:skill", pack.manifest.id),
            surface_slug: "skill".to_string(),
            title: pack.manifest.title.clone(),
            instruction_resource_paths: Vec::new(),
            attached_script_paths: pack_resource_paths(pack, ResourceKind::Script),
            attached_reference_paths: pack_resource_paths(pack, ResourceKind::Example),
            attached_asset_paths: pack_resource_paths(pack, ResourceKind::Asset),
            contents: default_skill_surface_bytes(pack)?,
        }]);
    }

    let mut seen_slugs = BTreeSet::new();
    let primary_surface_index = instruction_resources
        .iter()
        .position(|resource| resource_file_name(resource) == Some("SKILL.md"))
        .unwrap_or(0);
    let mut surfaces = Vec::new();
    for (index, resource) in instruction_resources.iter().enumerate() {
        let contents = read_pack_resource(pack, resource)?;
        let base_candidate = resource_surface_slug(resource, &contents)
            .unwrap_or_else(|| format!("surface-{}", index + 1));
        let surface_slug = dedupe_surface_slug(&mut seen_slugs, &base_candidate);
        surfaces.push(DerivedSkillSurface {
            surface_id: format!("{}:{}", pack.manifest.id, surface_slug),
            surface_slug,
            title: surface_title(pack, resource, &contents),
            instruction_resource_paths: vec![resource.path.clone()],
            attached_script_paths: if index == primary_surface_index {
                pack_resource_paths(pack, ResourceKind::Script)
            } else {
                Vec::new()
            },
            attached_reference_paths: if index == primary_surface_index {
                pack_resource_paths(pack, ResourceKind::Example)
            } else {
                Vec::new()
            },
            attached_asset_paths: if index == primary_surface_index {
                pack_resource_paths(pack, ResourceKind::Asset)
            } else {
                Vec::new()
            },
            contents,
        });
    }
    Ok(surfaces)
}

fn should_emit_separate_surfaces(
    target: &TargetCapabilityMatrix,
    compile_target: &crate::types::CompileTarget,
    surfaces: &[DerivedSkillSurface],
) -> Result<bool> {
    if surfaces.len() <= 1 {
        return Ok(compile_target.path_template.contains("{surface_slug}"));
    }
    if !target.capabilities.skill_folders {
        return Ok(false);
    }
    if !compile_target.supports_multi_surface_pack {
        return Ok(false);
    }
    if !compile_target.path_template.contains("{surface_slug}") {
        match compile_target
            .surface_merge_strategy
            .clone()
            .unwrap_or(SurfaceMergeStrategy::Optional)
        {
            SurfaceMergeStrategy::None => {
                return Err(anyhow!(
                "target {} requires separate surfaces but its path template omits {{surface_slug}}",
                target.target_id
            ))
            }
            SurfaceMergeStrategy::Optional | SurfaceMergeStrategy::Required => return Ok(false),
        }
    }
    Ok(true)
}

fn effective_surface_selection_mode(
    compile_target: &crate::types::CompileTarget,
    override_mode: Option<SurfaceSelectionMode>,
) -> SurfaceSelectionMode {
    override_mode
        .or_else(|| compile_target.surface_selection_mode.clone())
        .unwrap_or(SurfaceSelectionMode::Full)
}

fn primary_instruction_path(pack: &DiscoveredPack) -> Option<&str> {
    pack.manifest
        .resources
        .iter()
        .find(|resource| resource.kind == ResourceKind::Instruction)
        .map(|resource| resource.path.as_str())
}

fn surface_relevance_tier(
    pack: &DiscoveredPack,
    surface: &DerivedSkillSurface,
) -> SurfaceRelevanceTier {
    let explicit = surface
        .instruction_resource_paths
        .first()
        .and_then(|resource_path| {
            pack.manifest
                .resources
                .iter()
                .find(|resource| resource.path == *resource_path)
                .and_then(|resource| resource.surface_relevance.clone())
        });
    explicit.unwrap_or_else(|| {
        if surface
            .instruction_resource_paths
            .first()
            .and_then(|resource_path| {
                primary_instruction_path(pack).map(|primary| resource_path == primary)
            })
            .unwrap_or(false)
        {
            SurfaceRelevanceTier::AlwaysOn
        } else {
            SurfaceRelevanceTier::Suppressible
        }
    })
}

fn surface_selection_decisions(
    pack: &DiscoveredPack,
    surfaces: &[DerivedSkillSurface],
    mode: SurfaceSelectionMode,
) -> Vec<SurfaceSelectionDecision> {
    surfaces
        .iter()
        .map(|surface| {
            let relevance_tier = surface_relevance_tier(pack, surface);
            let emitted = match mode {
                SurfaceSelectionMode::Full => true,
                SurfaceSelectionMode::Minimal | SurfaceSelectionMode::Auto => {
                    relevance_tier == SurfaceRelevanceTier::AlwaysOn
                }
            };
            let reason_code = if emitted {
                None
            } else {
                Some(ReasonCode::SuppressedByMode)
            };
            let detail = if emitted {
                None
            } else {
                Some("Surface is suppressible and omitted in minimal surface mode.".to_string())
            };
            SurfaceSelectionDecision {
                pack_ref: pack.manifest.pack_ref(),
                surface_id: surface.surface_id.clone(),
                surface_slug: surface.surface_slug.clone(),
                relevance_tier,
                emitted,
                reason_code,
                detail,
                source_resource_paths: surface.instruction_resource_paths.clone(),
            }
        })
        .collect()
}

fn expand_skill_path(template: &str, pack_id: &str, surface_slug: Option<&str>) -> Result<String> {
    let mut path = template.replace("{pack_id}", pack_id);
    if let Some(surface_slug) = surface_slug {
        path = path.replace("{surface_slug}", surface_slug);
    } else if path.contains("{surface_slug}") {
        return Err(anyhow!(
            "path template {} requires {{surface_slug}} for separate surface output",
            template
        ));
    }
    Ok(path)
}

fn expand_pack_resource_path(
    template: &str,
    pack: &DiscoveredPack,
    resource: &PackResource,
) -> Result<String> {
    // When the template has no per-resource token, callers must guarantee a
    // single resource per pack (e.g. `primary_instruction_only`). Otherwise
    // multiple resources would collide on the same destination.
    let has_resource_token = template.contains("{resource_path}")
        || template.contains("{resource_name}")
        || template.contains("{resource_slug}");
    if !has_resource_token && !template.contains("{pack_id}") {
        return Err(anyhow!(
            "path template {} requires one of {{resource_path}}, {{resource_name}}, or {{resource_slug}} for pack_resource output (or {{pack_id}} for primary-only emission)",
            template
        ));
    }

    let resource_path = pack_resource_relative_path(pack, resource);
    let resource_name = resource_file_name(resource)
        .ok_or_else(|| anyhow!("resource {} is missing a file name", resource.path))?;
    let resource_contents = read_pack_resource(pack, resource)?;
    let resource_slug = resource_surface_slug(resource, &resource_contents)
        .or_else(|| slugify_surface_candidate(resource_name))
        .ok_or_else(|| anyhow!("resource {} could not derive a slug", resource.path))?;

    Ok(template
        .replace("{pack_id}", &pack.manifest.id)
        .replace("{resource_path}", &resource_path)
        .replace("{resource_name}", resource_name)
        .replace("{resource_slug}", &resource_slug))
}

fn emit_pack_extension_manifests(
    compile_target: &crate::types::CompileTarget,
    packs: &[&DiscoveredPack],
) -> Result<Vec<StagedOutputInput>> {
    let mut outputs = Vec::new();
    for pack in packs {
        let destination = compile_target
            .path_template
            .replace("{pack_id}", &pack.manifest.id);
        let description = pack
            .manifest
            .description
            .clone()
            .unwrap_or_else(|| pack.manifest.title.clone());
        let manifest = serde_json::json!({
            "name": pack.manifest.id,
            "description": description,
            "version": pack.manifest.version,
            "contextFileName": "GEMINI.md",
        });
        let contents = serde_json::to_vec_pretty(&manifest)?;
        outputs.push(StagedOutputInput {
            id: Some(format!("pack-extension-manifest-{}", pack.manifest.id)),
            destination_path: destination,
            kind: GeneratedOutputKind::PackExtensionManifest,
            contents,
            instruction_mode: None,
            pack_ref: Some(pack.manifest.pack_ref()),
            surface_id: None,
            surface_slug: None,
            source_resource_paths: Vec::new(),
            merge_status: None,
            degradation_codes: Vec::new(),
            ownership_token: Some(format!("{}::extension-manifest", pack.manifest.id)),
        });
    }
    Ok(outputs)
}

fn emit_pack_resource_outputs(
    compile_target: &crate::types::CompileTarget,
    packs: &[&DiscoveredPack],
) -> Result<Vec<StagedOutputInput>> {
    if compile_target.resource_kinds.is_empty() {
        return Err(anyhow!(
            "pack_resource compile target {} must declare resource_kinds",
            compile_target.path_template
        ));
    }

    // Pre-compute the pack's primary instruction path (first declared
    // `Instruction` resource) so `primary_instruction_only` can filter
    // siblings without re-scanning the manifest per resource.
    let mut outputs = Vec::new();
    for pack in packs {
        let primary_instruction_path: Option<String> = pack
            .manifest
            .resources
            .iter()
            .find(|r| r.kind == ResourceKind::Instruction)
            .map(|r| r.path.clone());
        for resource in pack
            .manifest
            .resources
            .iter()
            .filter(|resource| compile_target.resource_kinds.contains(&resource.kind))
        {
            if compile_target.primary_instruction_only && resource.kind == ResourceKind::Instruction
            {
                let is_primary = primary_instruction_path
                    .as_deref()
                    .map(|p| p == resource.path.as_str())
                    .unwrap_or(false);
                if !is_primary {
                    continue;
                }
            }
            let destination =
                expand_pack_resource_path(&compile_target.path_template, pack, resource)?;
            let relative_path = pack_resource_relative_path(pack, resource);
            let raw_contents = read_pack_resource(pack, resource)?;
            let contents = if resource.kind == ResourceKind::Command {
                apply_command_adapter(&raw_contents, compile_target.command_adapter.as_ref(), pack)
            } else {
                raw_contents
            };
            outputs.push(StagedOutputInput {
                id: Some(pack_resource_output_id(&pack.manifest.id, &relative_path)),
                destination_path: destination,
                kind: GeneratedOutputKind::ResourceFile,
                contents,
                instruction_mode: None,
                pack_ref: Some(pack.manifest.pack_ref()),
                surface_id: None,
                surface_slug: None,
                source_resource_paths: vec![resource.path.clone()],
                merge_status: None,
                degradation_codes: Vec::new(),
                ownership_token: Some(format!("{}::resource:{}", pack.manifest.id, relative_path)),
            });
        }
    }
    Ok(outputs)
}

fn skill_surface_document(
    pack: &DiscoveredPack,
    surface: &DerivedSkillSurface,
    supports_frontmatter: bool,
) -> Result<Vec<u8>> {
    if !supports_frontmatter {
        return Ok(surface.contents.clone());
    }
    if markdown_has_frontmatter(&surface.contents) {
        let contents = String::from_utf8_lossy(&surface.contents);
        let failures = validate_skill_frontmatter_text(&contents);
        if !failures.is_empty() {
            return Err(anyhow!(
                "instruction surface {} has invalid skill frontmatter: {}",
                surface.surface_id,
                failures.join(" ")
            ));
        }
        return Ok(surface.contents.clone());
    }
    let mut frontmatter = BTreeMap::new();
    frontmatter.insert("name".to_string(), surface.title.clone());
    frontmatter.insert(
        "description".to_string(),
        pack.manifest
            .description
            .clone()
            .unwrap_or_else(|| pack.manifest.title.clone()),
    );
    frontmatter.insert("pack_id".to_string(), pack.manifest.id.clone());
    frontmatter.insert("surface_slug".to_string(), surface.surface_slug.clone());
    let mut document = render_frontmatter(&frontmatter)?.into_bytes();
    document.push(b'\n');
    document.extend_from_slice(&surface.contents);
    Ok(document)
}

/// Expand a target's runtime-config template into emitted bytes + kind.
///
/// The kernel knows nothing about which target has which config shape — it
/// reads `target.runtime_template.path` from the library root, seeds a
/// context map from policy + target capabilities + resolve graph, and runs
/// `substitute_tokens`. Adding or changing a target's runtime config is now
/// a data-only edit (target JSON + template file).
fn expand_runtime_template(
    library_roots: &[PathBuf],
    template_ref: &RuntimeTemplateRef,
    target: &TargetCapabilityMatrix,
    policy: &PolicyManifest,
    resolve_graph: &ResolveGraph,
    packs: &[&DiscoveredPack],
) -> Result<(GeneratedOutputKind, Vec<u8>)> {
    let (tmpl_path, raw) = library_roots
        .iter()
        .map(|root| root.join(&template_ref.path))
        .find_map(|candidate| {
            std::fs::read_to_string(&candidate)
                .ok()
                .map(|raw| (candidate, raw))
        })
        .ok_or_else(|| {
            anyhow!(
                "runtime template not found in any library root: {}",
                template_ref.path
            )
        })?;
    let _ = tmpl_path;

    let mut ctx: BTreeMap<String, String> = BTreeMap::new();
    ctx.insert("policy_id".into(), policy.id.clone());
    ctx.insert("target_id".into(), target.target_id.clone());
    let approval = if target.capabilities.approval_policies {
        "on-request"
    } else {
        "advisory"
    };
    ctx.insert("approval_policy".into(), approval.into());
    ctx.insert("approval_mode".into(), approval.into());
    ctx.insert("sandbox_mode".into(), "workspace-write".into());
    ctx.insert(
        "readonly_hints".into(),
        target.capabilities.readonly_hints.to_string(),
    );
    ctx.insert(
        "active_packs_json_array".into(),
        serde_json::to_string(
            &resolve_graph
                .activated_pack_refs
                .iter()
                .map(|item| &item.id)
                .collect::<Vec<_>>(),
        )
        .unwrap_or_else(|_| "[]".into()),
    );
    let hooks_value = aggregate_hook_wirings_for_target(target, packs)?;
    ctx.insert(
        "hooks_json".into(),
        serde_json::to_string(&hooks_value).unwrap_or_else(|_| "{}".into()),
    );

    let expanded = substitute_tokens(&raw, &ctx);
    let kind = match template_ref.output_kind.clone() {
        Some(CompileTargetKind::HookConfig) => GeneratedOutputKind::HookConfig,
        Some(CompileTargetKind::McpConfig) => GeneratedOutputKind::McpConfig,
        _ => GeneratedOutputKind::RuntimeJson,
    };
    Ok((kind, expanded.into_bytes()))
}

/// Build the `hooks` map for a runtime template by aggregating every
/// `HookWiring` resource from the activated packs whose `compatible_targets`
/// includes the requested target. Returns an empty object when the target
/// does not advertise `deterministic_hooks`.
///
/// The kernel does not invent matchers, events, or commands — packs declare
/// them. The materialized command path mirrors the `.claude/hooks/{pack_id}/
/// {resource_path}` template that `emit_pack_resource_outputs` produces for
/// the sibling `Hook` script (see `library/starter/targets/claude-code.json`).
fn aggregate_hook_wirings_for_target(
    target: &TargetCapabilityMatrix,
    packs: &[&DiscoveredPack],
) -> Result<serde_json::Value> {
    if !target.capabilities.deterministic_hooks {
        return Ok(serde_json::json!({}));
    }
    let mut events: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    for pack in packs {
        for resource in &pack.manifest.resources {
            if resource.kind != ResourceKind::HookWiring {
                continue;
            }
            let bytes = read_pack_resource(pack, resource)?;
            let wiring: serde_json::Value = serde_json::from_slice(&bytes)
                .with_context(|| format!("malformed hook wiring: {}", resource.path))?;
            if let Some(compat) = wiring.get("compatible_targets").and_then(|v| v.as_array()) {
                if !compat
                    .iter()
                    .any(|v| v.as_str() == Some(target.target_id.as_str()))
                {
                    continue;
                }
            }
            let event = wiring
                .get("event")
                .and_then(|v| v.as_str())
                .unwrap_or("PostToolUse")
                .to_string();
            let matcher = wiring
                .get("matcher")
                .and_then(|v| v.as_str())
                .unwrap_or("*")
                .to_string();
            let command_ref = wiring
                .get("command_ref")
                .and_then(|v| v.as_str())
                .with_context(|| format!("hook wiring {} missing command_ref", resource.path))?;
            // Resolve command_ref against the pack's Hook resources so the
            // emitted `command` matches the path that emit_pack_resource_outputs
            // will write into the project. Falls back to a basename derived from
            // command_ref if no matching Hook resource is declared.
            let hook_resource = pack
                .manifest
                .resources
                .iter()
                .find(|r| r.kind == ResourceKind::Hook && r.path == command_ref);
            let materialized = match hook_resource {
                Some(hook) => format!(
                    ".claude/hooks/{}/{}",
                    pack.manifest.id,
                    pack_resource_relative_path(pack, hook)
                ),
                None => {
                    let basename = command_ref
                        .rsplit('/')
                        .next()
                        .unwrap_or(command_ref)
                        .to_string();
                    format!(".claude/hooks/{}/{}", pack.manifest.id, basename)
                }
            };
            events.entry(event).or_default().push(serde_json::json!({
                "matcher": matcher,
                "hooks": [{ "type": "command", "command": materialized }],
            }));
        }
    }
    Ok(serde_json::to_value(events).unwrap_or_else(|_| serde_json::json!({})))
}

fn primary_instruction_snippet(pack: &DiscoveredPack) -> Result<String> {
    let Some(bytes) = primary_instruction_bytes(pack)? else {
        return Ok(String::new());
    };
    Ok(String::from_utf8(bytes).unwrap_or_default())
}

fn primary_instruction_bytes(pack: &DiscoveredPack) -> Result<Option<Vec<u8>>> {
    let instructions: Vec<&PackResource> = pack
        .manifest
        .resources
        .iter()
        .filter(|item| item.kind == ResourceKind::Instruction)
        .collect();
    if instructions.is_empty() {
        return Ok(None);
    }

    let mut combined = Vec::<u8>::new();
    for (index, resource) in instructions.iter().enumerate() {
        let bytes = read_pack_resource(pack, resource)?;
        if index == 0 {
            combined = bytes;
            continue;
        }
        let label = instruction_resource_heading(resource);
        combined.extend_from_slice(b"\n\n---\n\n## ");
        combined.extend_from_slice(label.as_bytes());
        combined.extend_from_slice(b"\n\n");
        combined.extend_from_slice(&bytes);
    }
    Ok(Some(combined))
}

fn default_skill_surface_bytes(pack: &DiscoveredPack) -> Result<Vec<u8>> {
    let description = pack
        .manifest
        .description
        .clone()
        .unwrap_or_else(|| "No bundled instruction resource was available.".to_string());
    let mut frontmatter = BTreeMap::new();
    frontmatter.insert("name".to_string(), pack.manifest.title.clone());
    frontmatter.insert("description".to_string(), description.clone());
    let mut document = render_frontmatter(&frontmatter)?.into_bytes();
    document
        .extend_from_slice(format!("\n# {}\n\n{}\n", pack.manifest.title, description).as_bytes());
    Ok(document)
}

fn pack_resource_paths(pack: &DiscoveredPack, kind: ResourceKind) -> Vec<String> {
    pack.manifest
        .resources
        .iter()
        .filter(|item| item.kind == kind)
        .map(|item| item.path.clone())
        .collect()
}

fn resource_surface_slug(resource: &PackResource, contents: &[u8]) -> Option<String> {
    frontmatter_name(contents)
        .and_then(|value| slugify_surface_candidate(&value))
        .or_else(|| semantic_carrier_parent_slug(resource))
        .or_else(|| {
            Path::new(&resource.path)
                .file_stem()
                .and_then(|value| value.to_str())
                .and_then(slugify_surface_candidate)
        })
        .or_else(|| first_heading_slug(contents))
}

fn resource_file_name(resource: &PackResource) -> Option<&str> {
    Path::new(&resource.path)
        .file_name()
        .and_then(|value| value.to_str())
}

fn first_heading_slug(contents: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(contents);
    text.lines()
        .find_map(|line| line.strip_prefix("# ").and_then(slugify_surface_candidate))
}

fn surface_title(pack: &DiscoveredPack, resource: &PackResource, contents: &[u8]) -> String {
    if let Some(name) = frontmatter_name(contents) {
        return name;
    }
    let text = String::from_utf8_lossy(contents);
    if let Some(title) = text.lines().find_map(|line| {
        line.strip_prefix("# ")
            .map(|value| value.trim().to_string())
    }) {
        return title;
    }
    instruction_resource_heading(resource)
        .split_whitespace()
        .map(capitalize_surface_word)
        .collect::<Vec<_>>()
        .join(" ")
        .if_empty_then(pack.manifest.title.clone())
}

fn semantic_carrier_parent_slug(resource: &PackResource) -> Option<String> {
    let file_name = resource_file_name(resource)?;
    if !matches!(file_name, "SKILL.md" | "README.md" | "INDEX.md") {
        return None;
    }
    Path::new(&resource.path)
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|value| value.to_str())
        .and_then(slugify_surface_candidate)
}

fn frontmatter_name(contents: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(contents);
    let mut lines = text.lines();
    if lines.next()? != "---" {
        return None;
    }
    for line in lines {
        if line == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix("name:") {
            let cleaned = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
    }
    None
}

fn dedupe_surface_slug(seen: &mut BTreeSet<String>, base: &str) -> String {
    if seen.insert(base.to_string()) {
        return base.to_string();
    }
    for index in 2.. {
        let candidate = format!("{}-{}", base, index);
        if seen.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!("surface slug dedupe must terminate")
}

fn slugify_surface_candidate(candidate: &str) -> Option<String> {
    let slug = candidate
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        None
    } else {
        Some(
            slug.split('-')
                .filter(|segment| !segment.is_empty())
                .collect::<Vec<_>>()
                .join("-"),
        )
    }
}

fn markdown_has_frontmatter(contents: &[u8]) -> bool {
    String::from_utf8_lossy(contents).starts_with("---\n")
}

/// Wrap a `Command` resource body in a per-target envelope (Markdown
/// frontmatter or TOML prompt object) when the compile target declares a
/// `command_adapter`. Returning the body unchanged when no adapter is set
/// preserves byte-for-byte parity with the pre-spec-019 behaviour.
///
/// Description fallback chain: `pack.manifest.description` →
/// `pack.manifest.title` → empty string. `PackResource` itself does not
/// carry a per-resource description today, so the pack-level description
/// is the most specific source available.
fn apply_command_adapter(
    body: &[u8],
    adapter: Option<&crate::types::CommandAdapter>,
    pack: &DiscoveredPack,
) -> Vec<u8> {
    let Some(adapter) = adapter else {
        return body.to_vec();
    };
    let body_str = String::from_utf8_lossy(body).into_owned();
    let description = pack
        .manifest
        .description
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| pack.manifest.title.clone());
    let escaped = description.replace('\\', "\\\\").replace('"', "\\\"");
    match adapter.format {
        crate::types::CommandAdapterFormat::Markdown => {
            if !adapter.inject_description {
                return body.to_vec();
            }
            if markdown_has_frontmatter(body) {
                return body.to_vec();
            }
            let mut out = String::from("---\n");
            out.push_str(&format!("description: \"{}\"\n", escaped));
            out.push_str("---\n\n");
            out.push_str(&body_str);
            out.into_bytes()
        }
        crate::types::CommandAdapterFormat::Toml => {
            let mut out = String::new();
            out.push_str(&format!("description = \"{}\"\n", escaped));
            out.push_str("prompt = \"\"\"\n");
            out.push_str(&body_str);
            if !body_str.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("\"\"\"\n");
            out.into_bytes()
        }
    }
}

fn skill_compile_target_for(
    target: &TargetCapabilityMatrix,
) -> Option<&crate::types::CompileTarget> {
    target
        .compile_targets
        .iter()
        .find(|item| item.output_kind == CompileTargetKind::CodexSkill)
}

fn instruction_resource_compile_target_for(
    target: &TargetCapabilityMatrix,
) -> Option<&crate::types::CompileTarget> {
    target.compile_targets.iter().find(|item| {
        item.output_kind == CompileTargetKind::PackResource
            && item.resource_kinds.contains(&ResourceKind::Instruction)
    })
}

fn instruction_mode_label(mode: &InstructionProjectionMode) -> &'static str {
    match mode {
        InstructionProjectionMode::Inline => "inline",
        InstructionProjectionMode::ReferenceIndex => "reference_index",
    }
}

fn common_reference_root(references: &[InstructionReference]) -> String {
    let mut segments = references
        .first()
        .map(|reference| {
            reference
                .path
                .split('/')
                .map(|segment| segment.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if segments.is_empty() {
        return String::new();
    }
    for reference in references.iter().skip(1) {
        let other = reference.path.split('/').collect::<Vec<_>>();
        let mut prefix_len = 0usize;
        while prefix_len < segments.len()
            && prefix_len < other.len()
            && segments[prefix_len] == other[prefix_len]
        {
            prefix_len += 1;
        }
        segments.truncate(prefix_len);
    }
    if let Some(last) = segments.last() {
        if last.contains('.') {
            segments.pop();
        }
    }
    let mut root = segments.join("/");
    if !root.is_empty() {
        root.push('/');
    }
    root
}

fn compact_reference_locator(path: &str) -> String {
    let trimmed = path
        .trim_end_matches("/SKILL.md")
        .trim_end_matches("/README.md")
        .trim_end_matches(".md");
    trimmed
        .rsplit_once('/')
        .map(|(_, tail)| tail.to_string())
        .unwrap_or_else(|| trimmed.to_string())
}

fn when_to_open_for_pack(pack: &DiscoveredPack) -> Vec<String> {
    if !pack.manifest.task_tags.is_empty() {
        return pack.manifest.task_tags.clone();
    }
    vec![match pack.manifest.activation_class {
        ActivationClass::Instruction => "general guidance",
        ActivationClass::Script => "scripted workflows",
        ActivationClass::Hook => "approval or write boundaries",
        ActivationClass::Service => "service or MCP setup",
    }
    .to_string()]
}

fn summarize_inline_snippet(snippet: &str) -> String {
    let compact = snippet
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("---"))
        .collect::<Vec<_>>()
        .join(" ");
    if compact.len() <= 200 {
        compact
    } else {
        format!("{}...", &compact[..197])
    }
}

fn budget_instruction_document(content: String) -> Result<BudgetedInstructionDocument> {
    let mut budgeted = content;
    let mut truncated = false;
    if budgeted.len() > INSTRUCTION_INDEX_WARN_BYTES {
        budgeted = truncate_instruction_document(&budgeted, INSTRUCTION_INDEX_WARN_BYTES);
        truncated = true;
    }
    if budgeted.len() > INSTRUCTION_INDEX_WARN_BYTES
        && budgeted.len() <= INSTRUCTION_INDEX_MAX_BYTES
    {
        return Err(anyhow!(
            "instruction index could not fit within {} bytes using structured truncation",
            INSTRUCTION_INDEX_WARN_BYTES
        ));
    }
    if budgeted.len() > INSTRUCTION_INDEX_MAX_BYTES {
        return Err(anyhow!(
            "instruction index exceeds {} bytes after truncation; reduce active pack routing detail",
            INSTRUCTION_INDEX_MAX_BYTES
        ));
    }
    Ok(BudgetedInstructionDocument {
        content: budgeted,
        truncated,
    })
}

fn truncate_instruction_document(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.to_string();
    }

    let mut lines = content
        .lines()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    let mut total_bytes = lines.join("\n").len();
    let mut candidates = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.starts_with("|pack:") || line.starts_with("|inline:"))
        .map(|(index, line)| (index, line.len()))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.1.cmp(&left.1));

    for (index, _) in candidates {
        if total_bytes <= max_bytes {
            break;
        }
        let original = &lines[index];
        let truncated_line = truncate_instruction_line(original);
        if truncated_line.len() < original.len() {
            total_bytes = total_bytes - original.len() + truncated_line.len();
            lines[index] = truncated_line;
        }
    }

    if total_bytes > max_bytes {
        while total_bytes > max_bytes {
            let Some(index) = lines
                .iter()
                .rposition(|line| line.starts_with("|gap:") || line.starts_with("|pack:"))
            else {
                break;
            };
            total_bytes -= lines[index].len() + 1;
            lines.remove(index);
        }
    }

    if !lines
        .iter()
        .any(|line| line.contains(INSTRUCTION_INDEX_POINTER))
    {
        lines.push(format!("|truncated:{}", INSTRUCTION_INDEX_POINTER));
        while lines.join("\n").len() > max_bytes {
            let Some(index) = lines
                .iter()
                .rposition(|line| line.starts_with("|pack:") || line.starts_with("|gap:"))
            else {
                break;
            };
            lines.remove(index);
        }
    }

    lines.join("\n")
}

fn truncate_instruction_line(line: &str) -> String {
    if line.len() <= 180 {
        return line.to_string();
    }
    let prefix: String = line.chars().take(140).collect();
    format!("{prefix}…|truncated:{INSTRUCTION_INDEX_POINTER}")
}

fn capitalize_surface_word(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

trait IfEmptyThen {
    fn if_empty_then(self, fallback: String) -> String;
}

impl IfEmptyThen for String {
    fn if_empty_then(self, fallback: String) -> String {
        if self.trim().is_empty() {
            fallback
        } else {
            self
        }
    }
}

fn instruction_resource_heading(resource: &PackResource) -> String {
    resource
        .path
        .rsplit('/')
        .next()
        .unwrap_or(resource.path.as_str())
        .trim_end_matches(".md")
        .replace(['-', '_'], " ")
}

fn pack_resource_relative_path(pack: &DiscoveredPack, resource: &PackResource) -> String {
    let prefix = format!("packs/{}/", pack.manifest.id);
    let stripped = resource
        .path
        .strip_prefix(&prefix)
        .unwrap_or(resource.path.as_str());
    // Also strip a leading kind-directory segment (e.g. "commands/") so target
    // templates of the form "{kind}/{pack_id}/{resource_path}" do not double-nest
    // the kind segment (bug: spec 019 task 1.1).
    let kind_prefix = format!("{}/", resource.kind.as_directory_segment());
    stripped
        .strip_prefix(&kind_prefix)
        .unwrap_or(stripped)
        .to_string()
}

fn pack_resource_output_id(pack_id: &str, relative_path: &str) -> String {
    let slug = relative_path
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    format!("resource-{}-{}", pack_id, slug)
}

fn read_pack_resource(pack: &DiscoveredPack, resource: &PackResource) -> Result<Vec<u8>> {
    let path = pack.library_root.join(&resource.path);
    if path.exists() {
        return read_cached_pack_resource(&path);
    }
    Ok(format!(
        "# {}\n\n{}\n",
        pack.manifest.title,
        pack.manifest
            .description
            .clone()
            .unwrap_or_else(|| "No bundled instruction resource was available.".to_string())
    )
    .into_bytes())
}

fn read_cached_pack_resource(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    let modified = metadata.modified().ok();
    let len = metadata.len();
    let cache = RESOURCE_READ_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Ok(cache) = cache.lock() {
        if let Some(entry) = cache.get(path) {
            if entry.modified == modified && entry.len == len {
                return Ok(entry.bytes.clone());
            }
        }
    }

    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if let Ok(mut cache) = cache.lock() {
        cache.insert(
            path.to_path_buf(),
            CachedResource {
                modified,
                len,
                bytes: bytes.clone(),
            },
        );
    }
    Ok(bytes)
}

fn document_id_for_target(target: &TargetCapabilityMatrix) -> String {
    // The instruction document id is derived from the target's metadata
    // override first (for targets whose document id doesn't match any
    // output_kind enum), then from the first instruction-document
    // compile_target. Target JSONs declare exactly one such entry; ordering
    // is stable.
    if let Some(explicit) = target.metadata.get("instruction_document_id") {
        return explicit.clone();
    }
    for ct in &target.compile_targets {
        match ct.output_kind {
            CompileTargetKind::ClaudeMd => return "claude-md".to_string(),
            CompileTargetKind::OpenclawMd => return "openclaw-md".to_string(),
            CompileTargetKind::AgentsMd => return "agents-md".to_string(),
            _ => continue,
        }
    }
    "agents-md".to_string()
}

fn approval_surface(target: &TargetCapabilityMatrix) -> String {
    target
        .metadata
        .get("approval_surface")
        .cloned()
        .unwrap_or_else(|| "policy-declared approval surface".to_string())
}

fn target_filesystem_surface(target: &TargetCapabilityMatrix) -> String {
    target
        .metadata
        .get("filesystem_surface")
        .cloned()
        .unwrap_or_else(|| approval_surface(target))
}

fn advisory_surface(target: &TargetCapabilityMatrix) -> String {
    target
        .metadata
        .get("advisory_surface")
        .cloned()
        .unwrap_or_else(|| "instruction document advisory guidance".to_string())
}

impl DiscoveredPack {
    fn is_candidate(&self) -> bool {
        self.promotion_status == PromotionStatus::Candidate
    }
}

fn policy_rule_suppression(
    pack: &DiscoveredPack,
    policy: &PolicyManifest,
) -> Option<SuppressedRef> {
    for rule in &policy.rules {
        if rule.subject != PolicySubject::Pack {
            continue;
        }
        if !selectors_match(rule.selectors.as_ref(), &pack.manifest) {
            continue;
        }
        match rule.operator {
            PolicyOperator::Deny => {
                return Some(SuppressedRef {
                    pack_ref: pack.manifest.pack_ref(),
                    reason_code: ReasonCode::SuppressedByPolicy,
                    detail: Some(format!("Policy rule {} denied this pack.", rule.id)),
                })
            }
            PolicyOperator::QuarantineOnly if pack.is_candidate() => {
                return Some(SuppressedRef {
                    pack_ref: pack.manifest.pack_ref(),
                    reason_code: ReasonCode::UntrustedPack,
                    detail: Some(
                        "Candidate packs remain quarantined until explicitly promoted.".to_string(),
                    ),
                })
            }
            _ => {}
        }
    }
    None
}

fn selectors_match(selectors: Option<&PolicySelectors>, pack: &PackManifest) -> bool {
    let Some(selectors) = selectors else {
        return true;
    };
    let id_match = selectors.ids.is_empty() || selectors.ids.iter().any(|item| item == &pack.id);
    let tag_match = selectors.tags.is_empty()
        || selectors
            .tags
            .iter()
            .any(|item| pack.task_tags.iter().any(|tag| tag == item));
    let trust_match = selectors.trust_tiers.is_empty()
        || selectors
            .trust_tiers
            .iter()
            .any(|tier| tier == &pack.trust_tier);
    id_match && tag_match && trust_match
}

fn effective_discovery_mode(config: &Config, policy: &PolicyManifest) -> DiscoveryMode {
    config
        .defaults
        .as_ref()
        .and_then(|defaults| defaults.discovery_mode.clone())
        .or_else(|| policy.discovery_mode.clone())
        .unwrap_or(DiscoveryMode::CuratedOnly)
}

fn ref_matches_version(reference: &Ref, version: &str) -> bool {
    reference
        .version
        .as_deref()
        .map(|item| item == version)
        .unwrap_or(true)
}

fn normalize_candidate(root: &Path, path: &Path) -> Result<(PackManifest, ProvenanceEnvelope)> {
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

fn provenance_ref_for(manifest: &PackManifest) -> Ref {
    Ref {
        kind: RefKind::Artifact,
        id: format!("{}-provenance", manifest.id),
        version: None,
    }
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(|item| item.trim().to_ascii_lowercase())
        .filter(|item| item.len() >= 3)
        .collect()
}

fn search_match_evidence(
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

fn relevance_score(
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

fn why_string(pack: &DiscoveredPack, evidence: &SearchMatchEvidence) -> String {
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

fn digest_json<T: Serialize>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value)?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}

fn sha256_digest(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}

fn load_json<T: DeserializeOwned>(path: PathBuf) -> Result<T> {
    let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("decode {}", path.display()))
}

fn sorted_glob_json(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries = dir
        .read_dir()
        .with_context(|| format!("read {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|item| item.path()))
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    entries.sort();
    Ok(entries)
}

fn sorted_imports(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = sorted_files(dir)?
        .into_iter()
        .filter(|path| {
            matches!(
                path.file_name().and_then(|value| value.to_str()),
                Some("AGENTS.md") | Some("SKILL.md")
            )
        })
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

fn sorted_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut entries = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let mut children = current
            .read_dir()
            .with_context(|| format!("read {}", current.display()))?
            .filter_map(|entry| entry.ok().map(|item| item.path()))
            .collect::<Vec<_>>();
        children.sort();
        for child in children.into_iter().rev() {
            if child.is_dir() {
                pending.push(child);
            } else {
                entries.push(child);
            }
        }
    }
    entries.sort();
    Ok(entries)
}

fn relative_path(root: &Path, path: &Path) -> Result<String> {
    path.strip_prefix(root)
        .map(|item| item.to_string_lossy().to_string())
        .map_err(|_| anyhow!("{} is not under {}", path.display(), root.display()))
}

fn register_unique_manifest<T, F, G>(
    map: &mut BTreeMap<String, T>,
    key: String,
    value: T,
    label: &str,
    source: &str,
    existing_version: F,
    current_version: G,
) -> Result<()>
where
    T: Clone,
    F: Fn(&T) -> Option<String>,
    G: Fn(&T) -> Option<String>,
{
    match map.entry(key.clone()) {
        Entry::Vacant(entry) => {
            entry.insert(value);
            Ok(())
        }
        Entry::Occupied(entry) => {
            let existing = entry.get();
            let left = existing_version(existing).unwrap_or_default();
            let right = current_version(&value).unwrap_or_default();
            if left == right {
                Err(anyhow!(
                    "duplicate {label} {key}@{left} discovered while loading {source}"
                ))
            } else {
                Err(anyhow!("conflicting {label} {key} discovered with versions {left} and {right} while loading {source}"))
            }
        }
    }
}

/// Replace `{{token}}` placeholders in `src` using values from `ctx`.
///
/// Unknown tokens are left untouched (`{{foo}}` stays in the output) so target
/// template authors can see them in the emitted file and diagnose typos.
/// A trailing unmatched `{{` is left verbatim.
fn substitute_tokens(src: &str, ctx: &std::collections::BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(src.len());
    let mut cursor = 0;
    while let Some(start) = src[cursor..].find("{{") {
        let abs = cursor + start;
        out.push_str(&src[cursor..abs]);
        if let Some(end_rel) = src[abs + 2..].find("}}") {
            let key = &src[abs + 2..abs + 2 + end_rel];
            match ctx.get(key.trim()) {
                Some(value) => out.push_str(value),
                None => out.push_str(&src[abs..abs + 2 + end_rel + 2]),
            }
            cursor = abs + 2 + end_rel + 2;
        } else {
            out.push_str(&src[abs..]);
            return out;
        }
    }
    out.push_str(&src[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    use super::{
        budget_instruction_document, derive_skill_surfaces, read_cached_pack_resource,
        resource_surface_slug, skill_surface_document, validate_generated_skill_frontmatter,
        DerivedSkillSurface, DiscoveredPack, LibraryRegistry, INSTRUCTION_INDEX_MAX_BYTES,
        INSTRUCTION_INDEX_POINTER, INSTRUCTION_INDEX_WARN_BYTES,
    };
    use crate::kernel::MetactlKernel;
    use crate::reference_kernel::ReferenceKernel;
    use crate::types::{
        ActivationClass, ApplyMode, CompileParams, Config, ConfigDefaults, DiscoveryMode,
        EntryPoint, ImportEcosystem, InvocationOverlay, LifecycleStatus, PackImport, PackLifecycle,
        PackManifest, PackResource, Ref, RefKind, ResolveParams, ResourceKind, SearchParams,
        SideEffectClass, SurfaceSelectionMode, TargetCapabilityMatrix, TrustTier, VisibilityScope,
    };

    fn fixtures_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/library/manifest-discovery")
    }

    fn load_target(root: &str) -> TargetCapabilityMatrix {
        let path = fixtures_root().join(root).join("targets/codex-cli.json");
        serde_json::from_slice(&std::fs::read(path).expect("target bytes"))
            .expect("target manifest")
    }

    fn library_config(role_id: &str, policy_id: &str) -> Config {
        Config {
            api_version: crate::types::API_VERSION.to_string(),
            role: Ref {
                kind: RefKind::Role,
                id: role_id.to_string(),
                version: Some("1.0.0".to_string()),
            },
            packs: Vec::new(),
            policy: Ref {
                kind: RefKind::Policy,
                id: policy_id.to_string(),
                version: Some("1.0.0".to_string()),
            },
            targets: vec![Ref {
                kind: RefKind::Target,
                id: "codex-cli".to_string(),
                version: Some("2026.03.25".to_string()),
            }],
            defaults: Some(ConfigDefaults {
                brownfield_mode: None,
                discovery_mode: Some(DiscoveryMode::CandidateSearch),
                surface_selection_mode: None,
            }),
            metadata: Default::default(),
        }
    }

    fn starter_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../library/starter")
    }

    fn starter_target(target_id: &str) -> TargetCapabilityMatrix {
        let path = starter_root()
            .join("targets")
            .join(format!("{target_id}.json"));
        serde_json::from_slice(&fs::read(path).expect("target bytes")).expect("target manifest")
    }

    fn starter_config(role_id: &str, policy_id: &str, target_id: &str) -> Config {
        Config {
            api_version: crate::types::API_VERSION.to_string(),
            role: Ref {
                kind: RefKind::Role,
                id: role_id.to_string(),
                version: Some("1.0.0".to_string()),
            },
            packs: Vec::new(),
            policy: Ref {
                kind: RefKind::Policy,
                id: policy_id.to_string(),
                version: Some("1.0.0".to_string()),
            },
            targets: vec![Ref {
                kind: RefKind::Target,
                id: target_id.to_string(),
                version: None,
            }],
            defaults: Some(ConfigDefaults {
                brownfield_mode: None,
                discovery_mode: Some(DiscoveryMode::CandidateSearch),
                surface_selection_mode: None,
            }),
            metadata: Default::default(),
        }
    }

    fn seed_search_lifecycle_library(root: &PathBuf) {
        fs::create_dir_all(root.join("packs")).expect("packs dir");
        fs::create_dir_all(root.join("vendor/legacy-python-audit")).expect("skill dir");
        fs::write(
            root.join("vendor/legacy-python-audit/SKILL.md"),
            "# Legacy Python Audit\n\nDetect temporal coupling in old Python service modules before refactors land.\n",
        )
        .expect("write skill");
        fs::write(
            root.join("packs/legacy-python-audit.json"),
            r#"{
  "kind": "pack",
  "id": "legacy-python-audit",
  "version": "1.0.0",
  "title": "Legacy Python Audit",
  "description": "Audit legacy Python modules before modernization work.",
  "activation_class": "instruction",
  "side_effect_class": "none",
  "trust_tier": "first_party_validated",
  "requires_confirmation": false,
  "compatible_roles": ["builder"],
  "compatible_targets": ["codex-cli"],
  "resources": [
    {
      "path": "vendor/legacy-python-audit/SKILL.md",
      "kind": "instruction",
      "required": true
    }
  ],
  "lifecycle": {
    "status": "deprecated",
    "replacement_pack_ref": {
      "kind": "pack",
      "id": "python-refactor",
      "version": "2.0.0"
    },
    "verified_targets": ["codex-cli"],
    "last_verified_at": "2026-04-22T12:00:00Z",
    "evidence_refs": ["evals/search/legacy-python-audit.json"]
  }
}
"#,
        )
        .expect("write pack manifest");
    }

    #[test]
    fn manifest_library_discovery_loads_curated_and_candidate_roots_in_deterministic_order() {
        let registry = LibraryRegistry::load_from_roots(&[
            fixtures_root().join("curated-root"),
            fixtures_root().join("candidate-root"),
        ])
        .expect("registry");

        let pack_ids = registry
            .packs
            .values()
            .map(|pack| pack.manifest.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            pack_ids,
            vec![
                "migration-guard".to_string(),
                "python-refactor".to_string(),
                "release-guard".to_string(),
                "repo-policy".to_string(),
            ]
        );
    }

    #[test]
    fn manifest_library_discovery_rejects_duplicate_and_conflict_packs() {
        let duplicate = LibraryRegistry::load_from_roots(&[
            fixtures_root().join("curated-root"),
            fixtures_root().join("duplicate-root"),
        ])
        .expect_err("duplicate failure");
        assert!(duplicate
            .to_string()
            .contains("duplicate pack python-refactor@2.0.0"));

        let conflict = LibraryRegistry::load_from_roots(&[
            fixtures_root().join("curated-root"),
            fixtures_root().join("conflict-root"),
        ])
        .expect_err("conflict failure");
        assert!(conflict
            .to_string()
            .contains("conflicting pack python-refactor"));
    }

    #[test]
    fn candidate_normalization_imports_agents_and_skill_into_quarantine() {
        let registry = LibraryRegistry::load_from_roots(&[fixtures_root().join("candidate-root")])
            .expect("registry");

        let agents = registry.packs.get("repo-policy").expect("agents pack");
        assert_eq!(
            agents.manifest.trust_tier,
            crate::types::TrustTier::CandidateQuarantined
        );
        assert_eq!(
            agents.promotion_status,
            crate::types::PromotionStatus::Candidate
        );
        assert_eq!(
            agents.manifest.imports[0].ecosystem,
            crate::types::ImportEcosystem::AgentsMd
        );
        assert_eq!(
            agents.manifest.resources[0].path,
            "imports/repo-policy/AGENTS.md"
        );

        let skill = registry.packs.get("release-guard").expect("skill pack");
        assert_eq!(
            skill.manifest.imports[0].ecosystem,
            crate::types::ImportEcosystem::SkillMd
        );
        assert_eq!(skill.manifest.resources.len(), 3);
        assert_eq!(
            skill
                .provenance
                .as_ref()
                .and_then(|item| item.review.as_ref())
                .and_then(|review| review.promotion_status.clone()),
            Some(crate::types::PromotionStatus::Candidate)
        );
    }

    #[test]
    fn zero_match_degrades_to_role_policy_only() {
        let kernel =
            ReferenceKernel::load_from_library_roots(vec![fixtures_root().join("zero-match-root")])
                .expect("library kernel");
        let config = library_config("release-manager", "release-policy");
        let resolve = kernel
            .resolve(ResolveParams {
                config,
                overlay: Some(InvocationOverlay {
                    entrypoint: EntryPoint::Cli,
                    task: None,
                    selected_project: None,
                    attached_artifacts: Vec::new(),
                    privacy_mode: None,
                    cost_budget_usd: None,
                    selected_target_override: None,
                    temporary_approvals: Vec::new(),
                    candidate_pack_hints: Vec::new(),
                }),
                available_targets: vec![load_target("zero-match-root")],
                provenance: None,
            })
            .expect("resolve graph");

        assert!(resolve.activated_pack_refs.is_empty());
        assert_eq!(resolve.applied_policies.len(), 1);
        assert_eq!(resolve.capability_gaps.len(), 1);
        assert_eq!(
            resolve.capability_gaps[0].reason_code,
            crate::types::ReasonCode::ZeroMatch
        );
        assert_eq!(resolve.suppressed_packs.len(), 1);
    }

    #[test]
    fn candidate_search_keeps_quarantined_matches_suppressed_under_policy() {
        let kernel = ReferenceKernel::load_from_library_roots(vec![
            fixtures_root().join("curated-root"),
            fixtures_root().join("candidate-root"),
        ])
        .expect("library kernel");

        let result = kernel
            .search(SearchParams {
                query: "release guard and python refactor".to_string(),
                config: library_config("builder", "builder-policy"),
                overlay: None,
                candidate_packs: Vec::new(),
                limit: None,
            })
            .expect("search");

        assert_eq!(result.discovery_mode, DiscoveryMode::CandidateSearch);
        assert_eq!(
            result
                .matches
                .iter()
                .map(|item| item.pack_ref.id.clone())
                .collect::<Vec<_>>(),
            vec!["python-refactor".to_string(), "migration-guard".to_string()]
        );
        assert_eq!(result.suppressed.len(), 2);
    }

    #[test]
    fn search_full_text_matches_instruction_body_terms() {
        let custom_root = TempDir::new().expect("custom root");
        seed_search_lifecycle_library(&custom_root.path().to_path_buf());
        let kernel = ReferenceKernel::load_from_library_roots(vec![
            starter_root(),
            custom_root.path().to_path_buf(),
        ])
        .expect("library kernel");

        let result = kernel
            .search(SearchParams {
                query: "temporal coupling".to_string(),
                config: starter_config("builder", "brownfield-safe-builder", "codex-cli"),
                overlay: None,
                candidate_packs: Vec::new(),
                limit: None,
            })
            .expect("search");

        assert_eq!(
            result.matches.first().expect("first match").pack_ref.id,
            "legacy-python-audit"
        );
    }

    #[test]
    fn search_results_include_match_evidence_and_lifecycle_hints() {
        let custom_root = TempDir::new().expect("custom root");
        seed_search_lifecycle_library(&custom_root.path().to_path_buf());
        let kernel = ReferenceKernel::load_from_library_roots(vec![
            starter_root(),
            custom_root.path().to_path_buf(),
        ])
        .expect("library kernel");

        let result = kernel
            .search(SearchParams {
                query: "temporal coupling".to_string(),
                config: starter_config("builder", "brownfield-safe-builder", "codex-cli"),
                overlay: None,
                candidate_packs: Vec::new(),
                limit: None,
            })
            .expect("search");

        let legacy = result
            .matches
            .iter()
            .find(|item| item.pack_ref.id == "legacy-python-audit")
            .expect("legacy match");
        assert_eq!(
            legacy.lifecycle.as_ref().expect("lifecycle").status,
            LifecycleStatus::Deprecated
        );
        assert_eq!(
            legacy
                .match_evidence
                .as_ref()
                .expect("match_evidence")
                .matched_resource_paths,
            vec!["vendor/legacy-python-audit/SKILL.md".to_string()]
        );
    }

    #[test]
    fn search_finds_library_steward_for_metactl_operations_queries() {
        let kernel =
            ReferenceKernel::load_from_library_roots(vec![starter_root()]).expect("library kernel");

        let result = kernel
            .search(SearchParams {
                query: "bind profile local overrides".to_string(),
                config: starter_config("builder", "brownfield-safe-builder", "codex-cli"),
                overlay: None,
                candidate_packs: Vec::new(),
                limit: None,
            })
            .expect("search");

        let first = result.matches.first().expect("first match");
        assert_eq!(first.pack_ref.id, "library-organization-guide");

        let evidence = first.match_evidence.as_ref().expect("match_evidence");
        assert!(evidence
            .matched_resource_paths
            .iter()
            .any(|path| path == "packs/library-organization-guide/OPERATIONS.md"));
        assert!(evidence.matched_terms.iter().any(|term| term == "profile"));
    }

    #[test]
    fn search_finds_project_onboarding_for_brownfield_install_queries() {
        let kernel =
            ReferenceKernel::load_from_library_roots(vec![starter_root()]).expect("library kernel");

        let result = kernel
            .search(SearchParams {
                query: "install metactl in a brownfield repo choose profile packs and sync"
                    .to_string(),
                config: starter_config("builder", "brownfield-safe-builder", "codex-cli"),
                overlay: None,
                candidate_packs: Vec::new(),
                limit: None,
            })
            .expect("search");

        let first = result.matches.first().expect("first match");
        assert_eq!(first.pack_ref.id, "metactl-project-onboarding");

        let evidence = first.match_evidence.as_ref().expect("match_evidence");
        assert!(evidence
            .matched_resource_paths
            .iter()
            .any(|path| path == "packs/metactl-project-onboarding/SKILL.md"));
        assert!(evidence
            .matched_terms
            .iter()
            .any(|term| term == "brownfield"));
    }

    #[test]
    fn search_finds_agent_candidate_library_installer_for_candidate_source_queries() {
        let kernel =
            ReferenceKernel::load_from_library_roots(vec![starter_root()]).expect("library kernel");

        let result = kernel
            .search(SearchParams {
                query:
                    "install private agent candidate library pre-commit hook source registration"
                        .to_string(),
                config: starter_config("builder", "brownfield-safe-builder", "codex-cli"),
                overlay: None,
                candidate_packs: Vec::new(),
                limit: None,
            })
            .expect("search");

        let first = result.matches.first().expect("first match");
        assert_eq!(first.pack_ref.id, "agent-candidate-library-installer");

        let evidence = first.match_evidence.as_ref().expect("match_evidence");
        assert!(evidence.matched_resource_paths.iter().any(|path| {
            path == "packs/agent-candidate-library-installer/SKILL.md"
                || path
                    == "packs/agent-candidate-library-installer/references/install-agent-candidate-library.md"
        }));
        assert!(evidence
            .matched_terms
            .iter()
            .any(|term| term == "candidate"));
    }

    #[test]
    fn pack_manifest_lifecycle_metadata_round_trip() {
        let manifest = PackManifest {
            kind: "pack".to_string(),
            id: "roundtrip-pack".to_string(),
            version: "1.0.0".to_string(),
            title: "Roundtrip Pack".to_string(),
            description: Some("Roundtrip lifecycle metadata".to_string()),
            activation_class: ActivationClass::Instruction,
            side_effect_class: SideEffectClass::None,
            trust_tier: TrustTier::FirstPartyValidated,
            requires_confirmation: false,
            task_tags: Vec::new(),
            compatible_roles: Vec::new(),
            compatible_targets: vec!["codex-cli".to_string()],
            knowledge_refs: Vec::new(),
            resources: vec![PackResource {
                path: "packs/roundtrip/SKILL.md".to_string(),
                kind: ResourceKind::Instruction,
                required: true,
                surface_relevance: None,
            }],
            imports: Vec::new(),
            visibility_scope: VisibilityScope::default(),
            lifecycle: Some(PackLifecycle {
                status: LifecycleStatus::Deprecated,
                replacement_pack_ref: Some(Ref {
                    kind: RefKind::Pack,
                    id: "python-refactor".to_string(),
                    version: Some("2.0.0".to_string()),
                }),
                verified_targets: vec!["codex-cli".to_string()],
                last_verified_at: Some("2026-04-22T12:00:00Z".to_string()),
                evidence_refs: vec!["evals/search/roundtrip-pack.json".to_string()],
            }),
            metadata: BTreeMap::new(),
        };

        let encoded = serde_json::to_value(&manifest).expect("encode manifest");
        let decoded: PackManifest = serde_json::from_value(encoded).expect("decode manifest");

        assert_eq!(
            decoded.lifecycle.expect("lifecycle").status,
            LifecycleStatus::Deprecated
        );
    }

    #[test]
    fn search_ranking_remains_pack_first_and_deterministic() {
        let custom_root = TempDir::new().expect("custom root");
        seed_search_lifecycle_library(&custom_root.path().to_path_buf());
        let kernel = ReferenceKernel::load_from_library_roots(vec![
            starter_root(),
            custom_root.path().to_path_buf(),
        ])
        .expect("library kernel");

        let params = SearchParams {
            query: "temporal coupling python".to_string(),
            config: starter_config("builder", "brownfield-safe-builder", "codex-cli"),
            overlay: None,
            candidate_packs: Vec::new(),
            limit: None,
        };
        let first = kernel.search(params.clone()).expect("first search");
        let second = kernel.search(params).expect("second search");

        assert_eq!(
            first
                .matches
                .iter()
                .map(|item| item.pack_ref.id.clone())
                .collect::<Vec<_>>(),
            second
                .matches
                .iter()
                .map(|item| item.pack_ref.id.clone())
                .collect::<Vec<_>>()
        );
        assert!(first
            .matches
            .iter()
            .all(|item| item.pack_ref.kind == RefKind::Pack));
    }

    #[test]
    fn relevance_selector_minimal_suppresses_cold_surfaces() {
        let project = TempDir::new().expect("tempdir");
        let kernel =
            ReferenceKernel::load_from_library_roots(vec![starter_root()]).expect("library kernel");
        let target = starter_target("codex-cli");
        let resolve = kernel
            .resolve(ResolveParams {
                config: starter_config("builder", "brownfield-safe-builder", "codex-cli"),
                overlay: None,
                available_targets: vec![target.clone()],
                provenance: None,
            })
            .expect("resolve");

        let compile = kernel
            .compile(CompileParams {
                resolve_graph: resolve,
                target_capability: target,
                apply_mode: ApplyMode::Copy,
                emit_policy_report: true,
                durable_staging: true,
                project_root: Some(project.path().to_string_lossy().into_owned()),
                surface_selection_mode: Some(SurfaceSelectionMode::Minimal),
            })
            .expect("compile");

        assert_eq!(
            compile.compile_manifest.surface_selection_mode,
            Some(SurfaceSelectionMode::Minimal)
        );
        assert!(compile
            .compile_manifest
            .surface_selection
            .iter()
            .any(|item| {
                item.pack_ref.id == "python-refactor"
                    && item.surface_slug == "contracts"
                    && !item.emitted
                    && item.reason_code == Some(crate::types::ReasonCode::SuppressedByMode)
            }));
    }

    #[test]
    fn relevance_selector_full_emits_all_eligible_surfaces() {
        let project = TempDir::new().expect("tempdir");
        let kernel =
            ReferenceKernel::load_from_library_roots(vec![starter_root()]).expect("library kernel");
        let target = starter_target("codex-cli");
        let resolve = kernel
            .resolve(ResolveParams {
                config: starter_config("builder", "brownfield-safe-builder", "codex-cli"),
                overlay: None,
                available_targets: vec![target.clone()],
                provenance: None,
            })
            .expect("resolve");

        let compile = kernel
            .compile(CompileParams {
                resolve_graph: resolve,
                target_capability: target,
                apply_mode: ApplyMode::Copy,
                emit_policy_report: true,
                durable_staging: true,
                project_root: Some(project.path().to_string_lossy().into_owned()),
                surface_selection_mode: Some(SurfaceSelectionMode::Full),
            })
            .expect("compile");

        assert_eq!(
            compile.compile_manifest.surface_selection_mode,
            Some(SurfaceSelectionMode::Full)
        );
        assert!(compile
            .compile_manifest
            .surface_selection
            .iter()
            .any(|item| {
                item.pack_ref.id == "python-refactor"
                    && item.surface_slug == "contracts"
                    && item.emitted
            }));
    }

    #[test]
    fn resource_read_cache_invalidates_changed_files() {
        let root = TempDir::new().expect("tempdir");
        let path = root.path().join("packs").join("demo").join("SKILL.md");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(&path, b"first").expect("first write");

        assert_eq!(
            read_cached_pack_resource(&path).expect("first read"),
            b"first"
        );

        std::fs::write(&path, b"second-version").expect("second write");
        assert_eq!(
            read_cached_pack_resource(&path).expect("second read"),
            b"second-version"
        );
    }

    #[test]
    fn surface_derivation_is_stable_and_dedupes_slugs() {
        let root = TempDir::new().expect("tempdir");
        let pack_dir = root.path().join("packs").join("demo-pack");
        std::fs::create_dir_all(&pack_dir).expect("pack dir");
        std::fs::write(pack_dir.join("+++!!.md"), "# Guided Review\n").expect("unsafe stem");
        std::fs::write(pack_dir.join("guided-review.md"), "# Guided Review\n").expect("dupe");

        let pack = DiscoveredPack {
            manifest: PackManifest {
                kind: "pack".to_string(),
                id: "demo-pack".to_string(),
                version: "1.0.0".to_string(),
                title: "Demo Pack".to_string(),
                description: Some("Derivation test".to_string()),
                activation_class: ActivationClass::Instruction,
                side_effect_class: SideEffectClass::None,
                trust_tier: TrustTier::FirstPartyValidated,
                requires_confirmation: false,
                task_tags: Vec::new(),
                compatible_roles: Vec::new(),
                compatible_targets: vec!["codex-cli".to_string()],
                knowledge_refs: Vec::new(),
                resources: vec![
                    PackResource {
                        path: "packs/demo-pack/+++!!.md".to_string(),
                        kind: ResourceKind::Instruction,
                        required: true,
                        surface_relevance: None,
                    },
                    PackResource {
                        path: "packs/demo-pack/guided-review.md".to_string(),
                        kind: ResourceKind::Instruction,
                        required: true,
                        surface_relevance: None,
                    },
                ],
                imports: vec![PackImport {
                    ecosystem: ImportEcosystem::FirstParty,
                    origin: "test".to_string(),
                    digest: None,
                }],
                visibility_scope: VisibilityScope::default(),
                lifecycle: None,
                metadata: BTreeMap::new(),
            },
            provenance: None,
            provenance_ref: None,
            source_path: root.path().join("packs/demo-pack.json"),
            library_root: root.path().to_path_buf(),
            promotion_status: crate::types::PromotionStatus::Promoted,
        };

        let surfaces = derive_skill_surfaces(&pack).expect("surfaces");
        assert_eq!(
            surfaces
                .iter()
                .map(|surface| surface.surface_slug.clone())
                .collect::<Vec<_>>(),
            vec!["guided-review".to_string(), "guided-review-2".to_string()]
        );
        assert_eq!(
            surfaces
                .iter()
                .map(|surface| surface.surface_id.clone())
                .collect::<Vec<_>>(),
            vec![
                "demo-pack:guided-review".to_string(),
                "demo-pack:guided-review-2".to_string(),
            ]
        );
    }

    #[test]
    fn skill_surface_frontmatter_serializes_yaml_scalars_with_colons() {
        let root = TempDir::new().expect("tempdir");
        let pack = DiscoveredPack {
            manifest: PackManifest {
                kind: "pack".to_string(),
                id: "demo-pack".to_string(),
                version: "1.0.0".to_string(),
                title: "Demo Pack".to_string(),
                description: Some(
                    "Copy/paste prompt bodies for repository artifact normalization: primary prompt, rubric, and index."
                        .to_string(),
                ),
                activation_class: ActivationClass::Instruction,
                side_effect_class: SideEffectClass::None,
                trust_tier: TrustTier::FirstPartyValidated,
                requires_confirmation: false,
                task_tags: Vec::new(),
                compatible_roles: Vec::new(),
                compatible_targets: vec!["codex-cli".to_string()],
                knowledge_refs: Vec::new(),
                resources: Vec::new(),
                imports: vec![PackImport {
                    ecosystem: ImportEcosystem::FirstParty,
                    origin: "test".to_string(),
                    digest: None,
                }],
                visibility_scope: VisibilityScope::default(),
                lifecycle: None,
                metadata: BTreeMap::new(),
            },
            provenance: None,
            provenance_ref: None,
            source_path: root.path().join("packs/demo-pack.json"),
            library_root: root.path().to_path_buf(),
            promotion_status: crate::types::PromotionStatus::Promoted,
        };
        let surface = DerivedSkillSurface {
            surface_id: "demo-pack:rubric".to_string(),
            surface_slug: "rubric".to_string(),
            title: "Scoring rubric: repository artifact normalization prompts".to_string(),
            instruction_resource_paths: vec!["prompts/rubric.md".to_string()],
            attached_script_paths: Vec::new(),
            attached_reference_paths: Vec::new(),
            attached_asset_paths: Vec::new(),
            contents: b"# Scoring rubric\n".to_vec(),
        };

        let document =
            skill_surface_document(&pack, &surface, true).expect("skill surface document");
        let skill_path = root.path().join("SKILL.md");
        fs::write(&skill_path, document).expect("skill document");

        assert_eq!(
            validate_generated_skill_frontmatter(&skill_path).expect("validation"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn generated_skill_frontmatter_validation_reports_yaml_and_required_fields() {
        let root = TempDir::new().expect("tempdir");
        let skill_path = root.path().join("SKILL.md");

        fs::write(
            &skill_path,
            "---\nname: Scoring rubric: repository artifact normalization prompts\ndescription: ok\n---\n",
        )
        .expect("invalid yaml skill");
        assert!(validate_generated_skill_frontmatter(&skill_path)
            .expect("validation")
            .iter()
            .any(|failure| failure.contains("invalid YAML")));

        fs::write(&skill_path, "---\nid: prompt\n---\n").expect("missing fields skill");
        let failures = validate_generated_skill_frontmatter(&skill_path).expect("validation");
        assert!(failures
            .iter()
            .any(|failure| failure.contains("frontmatter.name is required")));
        assert!(failures
            .iter()
            .any(|failure| failure.contains("frontmatter.description is required")));
    }

    #[test]
    fn semantic_skill_surface_slugs_prefer_semantic_identity() {
        let root = TempDir::new().expect("tempdir");
        let pack_dir = root
            .path()
            .join("packs")
            .join("demo-pack")
            .join("cli-audit");
        std::fs::create_dir_all(&pack_dir).expect("pack dir");
        std::fs::write(
            pack_dir.join("SKILL.md"),
            "---\nname: CLI Audit\n---\n\n# CLI Audit\n",
        )
        .expect("skill");

        let pack = DiscoveredPack {
            manifest: PackManifest {
                kind: "pack".to_string(),
                id: "demo-pack".to_string(),
                version: "1.0.0".to_string(),
                title: "Demo Pack".to_string(),
                description: Some("Derivation test".to_string()),
                activation_class: ActivationClass::Instruction,
                side_effect_class: SideEffectClass::None,
                trust_tier: TrustTier::FirstPartyValidated,
                requires_confirmation: false,
                task_tags: Vec::new(),
                compatible_roles: Vec::new(),
                compatible_targets: vec!["codex-cli".to_string()],
                knowledge_refs: Vec::new(),
                resources: vec![PackResource {
                    path: "packs/demo-pack/cli-audit/SKILL.md".to_string(),
                    kind: ResourceKind::Instruction,
                    required: true,
                    surface_relevance: None,
                }],
                imports: vec![PackImport {
                    ecosystem: ImportEcosystem::FirstParty,
                    origin: "test".to_string(),
                    digest: None,
                }],
                visibility_scope: VisibilityScope::default(),
                lifecycle: None,
                metadata: BTreeMap::new(),
            },
            provenance: None,
            provenance_ref: None,
            source_path: root.path().join("packs/demo-pack.json"),
            library_root: root.path().to_path_buf(),
            promotion_status: crate::types::PromotionStatus::Promoted,
        };

        let surfaces = derive_skill_surfaces(&pack).expect("surfaces");
        assert_eq!(surfaces.len(), 1);
        assert_eq!(surfaces[0].surface_slug, "cli-audit");
        assert_eq!(surfaces[0].title, "CLI Audit");
    }

    #[test]
    fn semantic_surface_slugs_prefer_explicit_file_name_over_heading() {
        let resource = PackResource {
            path: "packs/demo-pack/CONTRACTS.md".to_string(),
            kind: ResourceKind::Instruction,
            required: true,
            surface_relevance: None,
        };

        let slug = resource_surface_slug(&resource, b"# Public API and boundaries\n")
            .expect("surface slug");

        assert_eq!(slug, "contracts");
    }

    #[test]
    fn instruction_index_budget_warns_and_truncates() {
        let mut content = String::from("# Demo\n\n[metactl Instruction Index]|target:codex-cli|policy:test|mode:reference_index\n");
        while content.len() <= INSTRUCTION_INDEX_WARN_BYTES + 256 {
            content.push_str("|pack:demo-pack|title:Demo Pack|open:skills/demo-pack/|surfaces:cli-audit,cli-dogfooding-audit,cli-testing-audit,ux-devex-testing|summary:");
            content.push_str(&"x".repeat(180));
            content.push('\n');
        }

        let budgeted = budget_instruction_document(content).expect("budgeted");
        assert!(budgeted.truncated);
        assert!(budgeted.content.len() <= INSTRUCTION_INDEX_WARN_BYTES);
        assert!(budgeted.content.contains(INSTRUCTION_INDEX_POINTER));
    }

    #[test]
    fn instruction_index_budget_fails_above_max() {
        let content = "x".repeat(INSTRUCTION_INDEX_MAX_BYTES + 1);
        let err = budget_instruction_document(content).expect_err("over budget");
        assert!(err.to_string().contains("instruction index exceeds"));
    }

    #[test]
    fn substitute_tokens_replaces_known_keys_and_leaves_unknown_intact() {
        let mut ctx = BTreeMap::new();
        ctx.insert(
            "policy_id".to_string(),
            "brownfield-safe-builder".to_string(),
        );
        ctx.insert("pack_id".to_string(), "python-refactor".to_string());
        let input = r#"{"policy":"{{policy_id}}","unknown":"{{never_set}}","id":"{{pack_id}}"}"#;
        let got = super::substitute_tokens(input, &ctx);
        assert_eq!(
            got,
            r#"{"policy":"brownfield-safe-builder","unknown":"{{never_set}}","id":"python-refactor"}"#
        );
    }

    #[test]
    fn substitute_tokens_handles_trailing_unmatched_open() {
        let ctx = BTreeMap::new();
        let got = super::substitute_tokens("trailing {{ open", &ctx);
        assert_eq!(got, "trailing {{ open");
    }

    #[test]
    fn substitute_tokens_trims_whitespace_around_key() {
        let mut ctx = BTreeMap::new();
        ctx.insert("name".to_string(), "metactl".to_string());
        let got = super::substitute_tokens("hello {{ name }}", &ctx);
        assert_eq!(got, "hello metactl");
    }
}
