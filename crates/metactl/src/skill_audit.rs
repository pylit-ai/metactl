use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::project::atomic_write;
use crate::API_VERSION;

const HOST_MATRIX_SOURCE: &str = "docs/internal/host-adapter-matrix.md";
const HOST_MATRIX_CHECKED_AT: &str = "2026-06-14";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillAuditScope {
    Repo,
    User,
    All,
    ExplicitRoot,
}

impl SkillAuditScope {
    pub fn as_str(self) -> &'static str {
        match self {
            SkillAuditScope::Repo => "repo",
            SkillAuditScope::User => "user",
            SkillAuditScope::All => "all",
            SkillAuditScope::ExplicitRoot => "explicit-root",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillReportFormat {
    Human,
    Markdown,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

impl Confidence {
    fn as_str(self) -> &'static str {
        match self {
            Confidence::High => "high",
            Confidence::Medium => "medium",
            Confidence::Low => "low",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinMethod {
    PackResourceId,
    SurfaceMetadata,
    Digest,
    Path,
    Inferred,
}

impl JoinMethod {
    fn as_str(self) -> &'static str {
        match self {
            JoinMethod::PackResourceId => "pack_resource_id",
            JoinMethod::SurfaceMetadata => "surface_metadata",
            JoinMethod::Digest => "digest",
            JoinMethod::Path => "path",
            JoinMethod::Inferred => "inferred",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    SameNameAs,
    SameSourceAs,
    DuplicateCandidate,
    SupersedesCandidate,
    ConflictsWithCandidate,
    ComplementsCandidate,
    VisibilityAmbiguousWith,
    GeneratedFromPack,
}

impl RelationKind {
    fn as_str(self) -> &'static str {
        match self {
            RelationKind::SameNameAs => "same_name_as",
            RelationKind::SameSourceAs => "same_source_as",
            RelationKind::DuplicateCandidate => "duplicate_candidate",
            RelationKind::SupersedesCandidate => "supersedes_candidate",
            RelationKind::ConflictsWithCandidate => "conflicts_with_candidate",
            RelationKind::ComplementsCandidate => "complements_candidate",
            RelationKind::VisibilityAmbiguousWith => "visibility_ambiguous_with",
            RelationKind::GeneratedFromPack => "generated_from_pack",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecommendationAction {
    Keep,
    KeepRepoOnly,
    MakeManualOnly,
    Watch,
    RepairMetadata,
    AddEval,
    MergeOrSplit,
    DeprecateCandidate,
    RemoveCandidate,
    QuarantineCandidate,
}

impl RecommendationAction {
    fn as_str(self) -> &'static str {
        match self {
            RecommendationAction::Keep => "keep",
            RecommendationAction::KeepRepoOnly => "keep_repo_only",
            RecommendationAction::MakeManualOnly => "make_manual_only",
            RecommendationAction::Watch => "watch",
            RecommendationAction::RepairMetadata => "repair_metadata",
            RecommendationAction::AddEval => "add_eval",
            RecommendationAction::MergeOrSplit => "merge_or_split",
            RecommendationAction::DeprecateCandidate => "deprecate_candidate",
            RecommendationAction::RemoveCandidate => "remove_candidate",
            RecommendationAction::QuarantineCandidate => "quarantine_candidate",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillAuditOptions {
    pub target_id: String,
    pub scope: SkillAuditScope,
    pub cwd: PathBuf,
    pub scan_roots: Vec<PathBuf>,
    pub include_local_paths: bool,
    pub format: SkillReportFormat,
    pub output_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostAdapterMetadata {
    pub host: String,
    pub source_url: String,
    pub source_checked_at: String,
    pub verified_by_test: bool,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationFinding {
    pub code: String,
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageJoin {
    pub join_method: String,
    pub join_confidence: String,
    pub evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_library_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<UsageCounts>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageCounts {
    #[serde(default)]
    pub command_invoked: u64,
    #[serde(default)]
    pub skill_body_read: u64,
    #[serde(default)]
    pub pack_resolved: u64,
    #[serde(default)]
    pub search_result_selected: u64,
    #[serde(default)]
    pub task_verified: u64,
    #[serde(default)]
    pub correction_or_retry: u64,
    #[serde(default)]
    pub dismissed_or_abandoned: u64,
    #[serde(default)]
    pub blocked_or_rejected: u64,
    #[serde(default)]
    pub event_count: u64,
    #[serde(default)]
    pub score: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageStatsFile {
    pub api_version: String,
    pub generated_at: String,
    pub source_path: String,
    pub event_count: u64,
    #[serde(default)]
    pub packs: Vec<PackUsageStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackUsageStats {
    pub pack_id: String,
    #[serde(flatten)]
    pub counts: UsageCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisibilityRecord {
    pub effective_visibility: String,
    pub confidence: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInventoryItem {
    pub local_id: String,
    pub name: String,
    pub target_kind: String,
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub path_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_pack_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_library_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_visibility: Option<String>,
    pub digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree_digest: Option<String>,
    pub frontmatter: Map<String, Value>,
    #[serde(default)]
    pub validation_findings: Vec<ValidationFinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    pub discovery_evidence: Vec<String>,
    pub discovery_confidence: String,
    pub host_adapter: HostAdapterMetadata,
    pub visibility: VisibilityRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_join: Option<UsageJoin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRelation {
    pub kind: String,
    pub left_id: String,
    pub right_id: String,
    pub confidence: String,
    pub reason_code: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    pub action: String,
    pub subject_ids: Vec<String>,
    pub confidence: String,
    pub reason_codes: Vec<String>,
    pub evidence: Vec<String>,
    pub next_reversible_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionPlanStep {
    pub action: String,
    pub subject_ids: Vec<String>,
    pub reason_codes: Vec<String>,
    pub confidence: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionPlan {
    pub plan_id: String,
    pub generated_at: String,
    pub target_id: String,
    pub scan_scope: String,
    pub approval_required: bool,
    pub mutation_allowed: bool,
    pub actions: Vec<ActionPlanStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillAuditSummary {
    pub total_skills: usize,
    pub host_counts: BTreeMap<String, usize>,
    pub scope_counts: BTreeMap<String, usize>,
    pub low_confidence_count: usize,
    pub duplicate_cluster_count: usize,
    pub relation_count: usize,
    pub recommendation_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillPortfolioAuditReport {
    pub api_version: String,
    pub generated_at: String,
    pub target_id: String,
    pub repo_root: String,
    pub repo_root_hash: String,
    pub cwd: String,
    pub scan_scope: String,
    pub collector_status: String,
    pub usage_window: String,
    pub project_instruction_sources: Vec<Value>,
    pub summary: SkillAuditSummary,
    pub inventory: Vec<SkillInventoryItem>,
    pub relations: Vec<SkillRelation>,
    pub recommendations: Vec<Recommendation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_plan: Option<ActionPlan>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillAuditOutput {
    pub report: SkillPortfolioAuditReport,
    pub report_json_path: PathBuf,
    pub report_markdown_path: PathBuf,
    pub inventory_path: PathBuf,
    pub relations_path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_path: Option<PathBuf>,
    pub json: Value,
    pub markdown: String,
}

pub fn run_audit(project_root: &Path, opts: SkillAuditOptions) -> Result<SkillAuditOutput> {
    let generated_at = now_string();
    let host_adapter = host_adapter_metadata(&opts.target_id);
    let project_instruction_sources = discover_project_instruction_sources(project_root, &opts.cwd)?;
    let scan_roots = resolve_scan_roots(project_root, &opts);
    let usage_stats = load_usage_stats(project_root)?;
    let collector_status = collector_status(project_root);

    let mut items = Vec::new();
    for root in &scan_roots {
        if root.exists() {
            collect_skill_items(
                project_root,
                root,
                &opts,
                &host_adapter,
                usage_stats.as_ref(),
                &mut items,
            )?;
        }
    }
    items.sort_by(|a, b| a.local_id.cmp(&b.local_id));

    let relations = build_relations(&items);
    let recommendations = build_recommendations(&items, &relations, usage_stats.as_ref());
    let action_plan = build_action_plan(&opts, &items, &recommendations, &generated_at);
    let summary = build_summary(&items, &relations, &recommendations);
    let usage_window = if usage_stats.is_some() {
        "present"
    } else {
        "none"
    }
    .to_string();

    let report = SkillPortfolioAuditReport {
        api_version: API_VERSION.to_string(),
        generated_at: generated_at.clone(),
        target_id: opts.target_id.clone(),
        repo_root: project_root.to_string_lossy().to_string(),
        repo_root_hash: sha256_string(project_root.to_string_lossy().as_bytes()),
        cwd: opts.cwd.to_string_lossy().to_string(),
        scan_scope: opts.scope.as_str().to_string(),
        collector_status,
        usage_window,
        project_instruction_sources,
        summary,
        inventory: items.clone(),
        relations: relations.clone(),
        recommendations: recommendations.clone(),
        action_plan: action_plan.clone(),
        notes: build_notes(&items, &relations, usage_stats.as_ref()),
    };

    let report_json = serde_json::to_value(&report).context("serialize skill audit report")?;
    let markdown = render_markdown(&report);

    let inventory_path = project_root.join(".metactl/skills/inventory.json");
    let relations_path = project_root.join(".metactl/skills/relations.json");
    let report_json_path = project_root.join(".metactl/reports/skills/latest.json");
    let report_markdown_path = project_root.join(".metactl/reports/skills/latest.md");
    let plan_path = report
        .action_plan
        .as_ref()
        .map(|plan| project_root.join(".metactl/reports/skills/plans").join(format!("{}.json", plan.plan_id)));

    write_json_file(&inventory_path, &serde_json::to_value(&report.inventory)?)?;
    write_json_file(&relations_path, &serde_json::to_value(&report.relations)?)?;
    write_json_file(&report_json_path, &report_json)?;
    write_text_file(&report_markdown_path, &markdown)?;
    if let Some(plan) = report.action_plan.as_ref() {
        if let Some(plan_path) = plan_path.as_ref() {
            write_json_file(plan_path, &serde_json::to_value(plan)?)?;
        }
    }
    if let Some(extra) = opts.output_path.as_ref() {
        match opts.format {
            SkillReportFormat::Json => write_json_file(extra, &report_json)?,
            SkillReportFormat::Markdown | SkillReportFormat::Human => {
                write_text_file(extra, &markdown)?
            }
        }
    }

    Ok(SkillAuditOutput {
        report,
        report_json_path,
        report_markdown_path,
        inventory_path,
        relations_path,
        plan_path,
        json: report_json,
        markdown,
    })
}

fn build_summary(
    inventory: &[SkillInventoryItem],
    relations: &[SkillRelation],
    recommendations: &[Recommendation],
) -> SkillAuditSummary {
    let mut host_counts = BTreeMap::new();
    let mut scope_counts = BTreeMap::new();
    let mut low_confidence_count = 0usize;
    for item in inventory {
        *host_counts.entry(item.target_kind.clone()).or_insert(0) += 1;
        *scope_counts.entry(item.scope.clone()).or_insert(0) += 1;
        if item.discovery_confidence == "low" || item.visibility.confidence == "low" {
            low_confidence_count += 1;
        }
    }

    let mut recommendation_counts = BTreeMap::new();
    for recommendation in recommendations {
        *recommendation_counts
            .entry(recommendation.action.clone())
            .or_insert(0) += 1;
    }

    let duplicate_cluster_count = relations
        .iter()
        .filter(|relation| relation.kind == RelationKind::DuplicateCandidate.as_str())
        .count();

    SkillAuditSummary {
        total_skills: inventory.len(),
        host_counts,
        scope_counts,
        low_confidence_count,
        duplicate_cluster_count,
        relation_count: relations.len(),
        recommendation_counts,
    }
}

fn build_notes(
    inventory: &[SkillInventoryItem],
    relations: &[SkillRelation],
    usage_stats: Option<&UsageStatsFile>,
) -> Vec<String> {
    let mut notes = Vec::new();
    if usage_stats.is_none() {
        notes.push("usage_window: none".to_string());
    }
    if inventory.is_empty() {
        notes.push("No skill-like artifacts discovered.".to_string());
    }
    if relations.is_empty() && !inventory.is_empty() {
        notes.push("No deterministic relations discovered.".to_string());
    }
    notes
}

fn build_recommendations(
    inventory: &[SkillInventoryItem],
    relations: &[SkillRelation],
    usage_stats: Option<&UsageStatsFile>,
) -> Vec<Recommendation> {
    let mut recommendations = Vec::new();
    let mut duplicate_partner_ids = BTreeSet::new();
    for relation in relations {
        if relation.kind == RelationKind::DuplicateCandidate.as_str()
            || relation.kind == RelationKind::VisibilityAmbiguousWith.as_str()
        {
            duplicate_partner_ids.insert(relation.left_id.clone());
            duplicate_partner_ids.insert(relation.right_id.clone());
        }
    }

    for item in inventory {
        let mut reason_codes = Vec::new();
        let mut evidence = Vec::new();
        if item
            .validation_findings
            .iter()
            .any(|finding| finding.severity != "info")
        {
            reason_codes.push("validation_failed".to_string());
            evidence.extend(
                item.validation_findings
                    .iter()
                    .map(|finding| format!("{}: {}", finding.code, finding.message)),
            );
        }
        if duplicate_partner_ids.contains(&item.local_id) {
            reason_codes.push("conflict_detected".to_string());
            evidence.push("duplicate or ambiguous cluster".to_string());
        }
        if item.enabled == Some(false) {
            reason_codes.push("degraded_enforcement".to_string());
        }

        let action = if !item.validation_findings.is_empty() {
            RecommendationAction::RepairMetadata
        } else if duplicate_partner_ids.contains(&item.local_id) {
            RecommendationAction::MergeOrSplit
        } else if item.enabled == Some(false) {
            RecommendationAction::Watch
        } else if usage_stats.is_none() && item.scope == "repo" {
            RecommendationAction::KeepRepoOnly
        } else if item.usage_join.is_none() {
            RecommendationAction::Watch
        } else {
            RecommendationAction::Keep
        };

        let confidence = if item.validation_findings.is_empty() {
            if item.usage_join.is_some() {
                "medium"
            } else {
                "low"
            }
        } else {
            "high"
        };

        recommendations.push(Recommendation {
            action: action.as_str().to_string(),
            subject_ids: vec![item.local_id.clone()],
            confidence: confidence.to_string(),
            reason_codes,
            evidence,
            next_reversible_action: match action {
                RecommendationAction::RepairMetadata => "repair_metadata".to_string(),
                RecommendationAction::MergeOrSplit => "merge_or_split".to_string(),
                RecommendationAction::KeepRepoOnly => "keep_repo_only".to_string(),
                RecommendationAction::Watch => "watch".to_string(),
                _ => "keep".to_string(),
            },
        });
    }

    recommendations.sort_by(|a, b| a.subject_ids.cmp(&b.subject_ids));
    recommendations
}

fn build_relations(items: &[SkillInventoryItem]) -> Vec<SkillRelation> {
    let mut relations = Vec::new();
    let mut by_name: BTreeMap<String, Vec<&SkillInventoryItem>> = BTreeMap::new();
    for item in items {
        by_name
            .entry(normalize_name(&item.name))
            .or_default()
            .push(item);
    }

    for grouped in by_name.values() {
        if grouped.len() < 2 {
            continue;
        }
        for i in 0..grouped.len() {
            for j in (i + 1)..grouped.len() {
                let left = grouped[i];
                let right = grouped[j];
                let same_source = left.source_pack_id.is_some()
                    && left.source_pack_id == right.source_pack_id
                    && left.source_library_ref == right.source_library_ref;
                let same_digest = left.digest == right.digest;
                let relation_kind = if same_digest {
                    RelationKind::DuplicateCandidate
                } else {
                    RelationKind::VisibilityAmbiguousWith
                };
                let confidence = if same_digest {
                    Confidence::High
                } else {
                    Confidence::Medium
                };
                relations.push(SkillRelation {
                    kind: RelationKind::SameNameAs.as_str().to_string(),
                    left_id: left.local_id.clone(),
                    right_id: right.local_id.clone(),
                    confidence: confidence.as_str().to_string(),
                    reason_code: "duplicate_candidate".to_string(),
                    evidence: vec![
                        format!("name: {}", left.name),
                        format!("name: {}", right.name),
                    ],
                });
                relations.push(SkillRelation {
                    kind: relation_kind.as_str().to_string(),
                    left_id: left.local_id.clone(),
                    right_id: right.local_id.clone(),
                    confidence: confidence.as_str().to_string(),
                    reason_code: if same_digest {
                        "duplicate_candidate".to_string()
                    } else {
                        "visibility_ambiguous_with".to_string()
                    },
                    evidence: vec![
                        format!("digest: {}", left.digest),
                        format!("digest: {}", right.digest),
                    ],
                });
                if same_source {
                    relations.push(SkillRelation {
                        kind: RelationKind::SameSourceAs.as_str().to_string(),
                        left_id: left.local_id.clone(),
                        right_id: right.local_id.clone(),
                        confidence: Confidence::High.as_str().to_string(),
                        reason_code: "same_source_as".to_string(),
                        evidence: vec![
                            left.source_pack_id.clone().unwrap_or_default(),
                            left.source_library_ref.clone().unwrap_or_default(),
                        ],
                    });
                }
            }
        }
    }

    for item in items {
        if let Some(pack_id) = item.source_pack_id.as_ref() {
            relations.push(SkillRelation {
                kind: RelationKind::GeneratedFromPack.as_str().to_string(),
                left_id: item.local_id.clone(),
                right_id: pack_id.clone(),
                confidence: item
                    .usage_join
                    .as_ref()
                    .map(|_| Confidence::High.as_str().to_string())
                    .unwrap_or_else(|| Confidence::Medium.as_str().to_string()),
                reason_code: "generated_from_pack".to_string(),
                evidence: vec![
                    format!("pack_id: {}", pack_id),
                    item.source_library_ref
                        .clone()
                        .unwrap_or_else(|| "generated surface".to_string()),
                ],
            });
        }
    }

    relations.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then(a.left_id.cmp(&b.left_id))
            .then(a.right_id.cmp(&b.right_id))
    });
    relations
}

fn build_action_plan(
    opts: &SkillAuditOptions,
    items: &[SkillInventoryItem],
    recommendations: &[Recommendation],
    generated_at: &str,
) -> Option<ActionPlan> {
    let mut actions = Vec::new();
    for recommendation in recommendations {
        if recommendation.action == RecommendationAction::RepairMetadata.as_str()
            || recommendation.action == RecommendationAction::MergeOrSplit.as_str()
        {
            actions.push(ActionPlanStep {
                action: recommendation.action.clone(),
                subject_ids: recommendation.subject_ids.clone(),
                reason_codes: recommendation.reason_codes.clone(),
                confidence: recommendation.confidence.clone(),
                evidence: recommendation.evidence.clone(),
            });
        }
    }
    if actions.is_empty() && items.is_empty() {
        return None;
    }
    let plan_id = sha256_string(
        format!(
            "{}|{}|{}|{}",
            opts.target_id,
            opts.scope.as_str(),
            generated_at,
            actions.len()
        )
        .as_bytes(),
    );
    Some(ActionPlan {
        plan_id,
        generated_at: generated_at.to_string(),
        target_id: opts.target_id.clone(),
        scan_scope: opts.scope.as_str().to_string(),
        approval_required: true,
        mutation_allowed: false,
        actions,
    })
}

fn discover_project_instruction_sources(project_root: &Path, cwd: &Path) -> Result<Vec<Value>> {
    let mut sources = Vec::new();
    let mut seen = BTreeSet::new();
    let canonical_project_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let canonical_cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());

    let root_agents = canonical_project_root.join("AGENTS.md");
    if root_agents.exists() && seen.insert(root_agents.clone()) {
        sources.push(json!({
            "kind": "AGENTS.md",
            "scope": "repo",
            "path": root_agents.to_string_lossy().to_string(),
            "hash": sha256_string(root_agents.to_string_lossy().as_bytes()),
        }));
    }

    let mut cursor = Some(canonical_cwd.as_path());
    while let Some(path) = cursor {
        if path.starts_with(&canonical_project_root) {
            let agents = path.join("AGENTS.md");
            if agents.exists() && seen.insert(agents.clone()) {
                sources.push(json!({
                    "kind": "AGENTS.md",
                    "scope": if path == canonical_project_root.as_path() { "repo" } else { "cwd" },
                    "path": agents.to_string_lossy().to_string(),
                    "hash": sha256_string(agents.to_string_lossy().as_bytes()),
                }));
            }
            if path == canonical_project_root.as_path() {
                break;
            }
            cursor = path.parent();
        } else {
            break;
        }
    }
    Ok(sources)
}

fn resolve_scan_roots(project_root: &Path, opts: &SkillAuditOptions) -> Vec<PathBuf> {
    if !opts.scan_roots.is_empty() {
        return opts.scan_roots.clone();
    }

    let mut roots: Vec<PathBuf> = Vec::new();

    match opts.scope {
        SkillAuditScope::ExplicitRoot => {
            push_unique_root(&mut roots, project_root.to_path_buf());
            push_unique_root(&mut roots, project_root.join(".agents/skills"));
            push_unique_root(&mut roots, project_root.join(".codex/skills"));
        }
        SkillAuditScope::Repo | SkillAuditScope::All => {
            push_unique_root(&mut roots, project_root.join(".agents/skills"));
            push_unique_root(&mut roots, project_root.join(".codex/skills"));
            push_unique_root(&mut roots, project_root.join(".claude/skills"));
            push_unique_root(&mut roots, project_root.join(".gemini/skills"));
        }
        SkillAuditScope::User => {}
    }

    if matches!(opts.scope, SkillAuditScope::User | SkillAuditScope::All) {
        if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
            push_unique_root(&mut roots, home.join(".agents/skills"));
            push_unique_root(&mut roots, home.join(".codex/skills"));
            push_unique_root(&mut roots, home.join(".claude/skills"));
            push_unique_root(&mut roots, home.join(".gemini/skills"));
        }
    }

    if roots.is_empty() {
        push_unique_root(&mut roots, project_root.to_path_buf());
    }

    roots
}

fn push_unique_root(roots: &mut Vec<PathBuf>, path: PathBuf) {
    if !roots
        .iter()
        .any(|existing| normalize_path(existing) == normalize_path(&path))
    {
        roots.push(path);
    }
}

fn collect_skill_items(
    project_root: &Path,
    root: &Path,
    opts: &SkillAuditOptions,
    host_adapter: &HostAdapterMetadata,
    usage_stats: Option<&UsageStatsFile>,
    out: &mut Vec<SkillInventoryItem>,
) -> Result<()> {
    if root.is_file() {
        if root.file_name().and_then(|name| name.to_str()) == Some("SKILL.md") {
            out.push(scan_skill_file(
                project_root,
                root,
                opts,
                host_adapter,
                usage_stats,
                root.parent().unwrap_or(root),
            )?);
        }
        return Ok(());
    }
    if !root.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(root).with_context(|| format!("read_dir {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_skill_items(project_root, &path, opts, host_adapter, usage_stats, out)?;
        } else if file_type.is_file() && path.file_name().and_then(|s| s.to_str()) == Some("SKILL.md") {
            out.push(scan_skill_file(
                project_root,
                &path,
                opts,
                host_adapter,
                usage_stats,
                path.parent().unwrap_or(root),
            )?);
        }
    }
    Ok(())
}

fn scan_skill_file(
    project_root: &Path,
    skill_path: &Path,
    opts: &SkillAuditOptions,
    host_adapter: &HostAdapterMetadata,
    usage_stats: Option<&UsageStatsFile>,
    skill_root: &Path,
) -> Result<SkillInventoryItem> {
    let raw = fs::read(skill_path).with_context(|| format!("read {}", skill_path.display()))?;
    let digest = sha256_bytes(&raw);
    let text = match String::from_utf8(raw.clone()) {
        Ok(text) => text,
        Err(_) => {
            let mut item = fallback_item(project_root, skill_path, opts, skill_root, host_adapter);
            item.validation_findings.push(ValidationFinding {
                code: "binary_or_non_utf8".to_string(),
                severity: "error".to_string(),
                message: "skill body is not valid UTF-8".to_string(),
            });
            item.digest = digest;
            return Ok(item);
        }
    };

    let (frontmatter, body, findings) = parse_frontmatter(&text);
    let mut item = fallback_item(project_root, skill_path, opts, skill_root, host_adapter);
    item.digest = digest;
    item.tree_digest = compute_tree_digest(skill_root).ok();
    item.frontmatter = frontmatter.clone();
    item.validation_findings = findings;
    item.enabled = frontmatter
        .get("enabled")
        .and_then(|value| value.as_bool());

    let name = frontmatter
        .get("name")
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            item.validation_findings.push(ValidationFinding {
                code: "missing_name".to_string(),
                severity: "error".to_string(),
                message: "frontmatter is missing `name`".to_string(),
            });
            skill_root
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("unknown-skill")
                .to_string()
        });
    item.name = name;

    match frontmatter.get("description").and_then(|value| value.as_str()) {
        Some(value) if !value.trim().is_empty() => {}
        _ => item.validation_findings.push(ValidationFinding {
            code: "missing_description".to_string(),
            severity: "error".to_string(),
            message: "frontmatter is missing `description`".to_string(),
        }),
    }

    let allowed = allowed_frontmatter_fields();
    for key in frontmatter.keys() {
        if !allowed.contains(key.as_str()) {
            item.validation_findings.push(ValidationFinding {
                code: "unsupported_field".to_string(),
                severity: "warning".to_string(),
                message: format!("unsupported frontmatter field `{key}`"),
            });
        }
    }

    if body.trim().is_empty() {
        item.validation_findings.push(ValidationFinding {
            code: "empty_body".to_string(),
            severity: "warning".to_string(),
            message: "skill body is empty".to_string(),
        });
    }

    if looks_like_secret(&text) {
        item.validation_findings.push(ValidationFinding {
            code: "secret_leakage".to_string(),
            severity: "error".to_string(),
            message: "skill body appears to contain a secret or token".to_string(),
        });
    }

    if looks_like_prompt(&text) {
        item.validation_findings.push(ValidationFinding {
            code: "prompt_leakage".to_string(),
            severity: "error".to_string(),
            message: "skill body appears to contain prompt text".to_string(),
        });
    }

    item.source_pack_id = frontmatter
        .get("source_pack_id")
        .or_else(|| frontmatter.get("metactl.source_pack_id"))
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .or_else(|| infer_pack_id(skill_path));
    item.source_library_ref = frontmatter
        .get("source_library_ref")
        .or_else(|| frontmatter.get("metactl.source_library_ref"))
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());
    item.source_visibility = frontmatter
        .get("visibility")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());

    if item.source_pack_id.is_some() {
        item.discovery_evidence
            .push("generated surface or pack-backed root".to_string());
    } else {
        item.discovery_evidence
            .push("filesystem discovery".to_string());
    }
    if skill_path.starts_with(project_root) {
        item.discovery_evidence
            .push("repo-local path".to_string());
    }
    item.discovery_confidence = if opts.scope == SkillAuditScope::ExplicitRoot {
        Confidence::High.as_str().to_string()
    } else if item.source_pack_id.is_some() {
        Confidence::Medium.as_str().to_string()
    } else {
        Confidence::Low.as_str().to_string()
    };

    item.path = if opts.include_local_paths {
        Some(skill_path.to_string_lossy().to_string())
    } else {
        None
    };
    item.path_hash = sha256_string(skill_path.to_string_lossy().as_bytes());
    item.local_id = sha256_string(
        format!(
            "{}|{}|{}",
            item.target_kind,
            item.scope,
            skill_path.to_string_lossy()
        )
        .as_bytes(),
    );
    item.visibility = visibility_for(&item, host_adapter);
    item.usage_join = join_usage(&item, skill_path, usage_stats);

    Ok(item)
}

fn fallback_item(
    project_root: &Path,
    skill_path: &Path,
    opts: &SkillAuditOptions,
    skill_root: &Path,
    host_adapter: &HostAdapterMetadata,
) -> SkillInventoryItem {
    let scope = scope_for_path(project_root, skill_path, opts.scope);
    let target_kind = target_kind_for_path(skill_path, &opts.target_id);
    SkillInventoryItem {
        local_id: sha256_string(skill_path.to_string_lossy().as_bytes()),
        name: skill_root
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown-skill")
            .to_string(),
        target_kind,
        scope,
        path: None,
        path_hash: sha256_string(skill_path.to_string_lossy().as_bytes()),
        source_pack_id: None,
        source_library_ref: None,
        source_visibility: None,
        digest: String::new(),
        tree_digest: None,
        frontmatter: Map::new(),
        validation_findings: Vec::new(),
        enabled: None,
        discovery_evidence: Vec::new(),
        discovery_confidence: Confidence::Low.as_str().to_string(),
        host_adapter: host_adapter.clone(),
        visibility: VisibilityRecord {
            effective_visibility: "unknown".to_string(),
            confidence: Confidence::Low.as_str().to_string(),
            notes: Vec::new(),
        },
        usage_join: None,
    }
}

fn target_kind_for_path(path: &Path, fallback: &str) -> String {
    let normalized = normalize_path(path);
    if normalized.contains("/.codex/skills/") {
        "metactl-generated".to_string()
    } else if normalized.contains("/.claude/skills/") {
        "claude-code".to_string()
    } else if normalized.contains("/.gemini/skills/") {
        "gemini-cli".to_string()
    } else if normalized.contains("/.agents/skills/") {
        "codex-cli".to_string()
    } else {
        fallback.to_string()
    }
}

fn scope_for_path(project_root: &Path, path: &Path, requested: SkillAuditScope) -> String {
    if matches!(requested, SkillAuditScope::ExplicitRoot) {
        return "explicit-root".to_string();
    }
    let normalized = normalize_path(path);
    let home_prefix = env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| normalize_path(&home));
    if let Some(home) = home_prefix {
        if normalized.starts_with(&home) {
            return "user".to_string();
        }
    }
    if normalized.contains("/.codex/skills/") {
        return "generated".to_string();
    }
    if normalized.contains("/.agents/skills/") {
        return "repo".to_string();
    }
    if normalized.starts_with(&normalize_path(project_root)) {
        return "repo".to_string();
    }
    if normalized.starts_with("/usr/") || normalized.starts_with("/System/") || normalized.starts_with("/etc/")
    {
        return "system".to_string();
    }
    if normalized.starts_with("/Library/") || normalized.starts_with("/opt/") {
        return "admin".to_string();
    }
    "repo".to_string()
}

fn visibility_for(item: &SkillInventoryItem, host_adapter: &HostAdapterMetadata) -> VisibilityRecord {
    let mut notes = Vec::new();
    let effective_visibility = if item.target_kind == "codex-cli" {
        if item.name.trim().is_empty() {
            notes.push("missing name".to_string());
            "ambiguous".to_string()
        } else {
            "visible".to_string()
        }
    } else if item.target_kind == "metactl-generated" {
        "generated".to_string()
    } else {
        "parse_only".to_string()
    };
    if item.validation_findings.iter().any(|finding| finding.code == "missing_name") {
        notes.push("name missing; visibility ambiguous".to_string());
    }
    VisibilityRecord {
        effective_visibility,
        confidence: host_adapter.confidence.clone(),
        notes,
    }
}

fn join_usage(
    item: &SkillInventoryItem,
    skill_path: &Path,
    usage_stats: Option<&UsageStatsFile>,
) -> Option<UsageJoin> {
    let stats = usage_stats?;
    let pack_id = item.source_pack_id.clone()?;
    let pack_stats = stats
        .packs
        .iter()
        .find(|entry| entry.pack_id == pack_id)?;
    let mut evidence = Vec::new();
    let join_method = if item.source_library_ref.is_some() {
        JoinMethod::SurfaceMetadata
    } else if item.source_pack_id.is_some() {
        JoinMethod::PackResourceId
    } else if !item.path_hash.is_empty() {
        JoinMethod::Path
    } else {
        JoinMethod::Inferred
    };
    evidence.push(format!("pack_id: {pack_id}"));
    if item.source_library_ref.is_some() {
        evidence.push("source_library_ref present".to_string());
    }
    if !skill_path.as_os_str().is_empty() {
        evidence.push(format!("path: {}", skill_path.display()));
    }
    Some(UsageJoin {
        join_method: join_method.as_str().to_string(),
        join_confidence: match join_method {
            JoinMethod::PackResourceId | JoinMethod::SurfaceMetadata => "high".to_string(),
            JoinMethod::Path => "medium".to_string(),
            JoinMethod::Digest | JoinMethod::Inferred => "low".to_string(),
        },
        evidence,
        pack_id: Some(pack_id),
        source_library_ref: item.source_library_ref.clone(),
        digest: Some(item.digest.clone()),
        path_hash: Some(item.path_hash.clone()),
        stats: Some(pack_stats.counts.clone()),
    })
}

fn collector_status(project_root: &Path) -> String {
    let usage_dir = project_root.join(".metactl/usage");
    let events = usage_dir.join("events.jsonl");
    let stats = usage_dir.join("stats.json");
    match (events.exists(), stats.exists()) {
        (false, false) => "unavailable".to_string(),
        (false, true) => "disabled".to_string(),
        (true, false) => "stale".to_string(),
        (true, true) => "disabled".to_string(),
    }
}

fn host_adapter_metadata(target_id: &str) -> HostAdapterMetadata {
    let (source_url, confidence) = match target_id {
        "codex-cli" => (format!("{HOST_MATRIX_SOURCE}#codex-cli"), "medium"),
        "metactl-generated" => (format!("{HOST_MATRIX_SOURCE}#metactl-generated"), "medium"),
        "explicit-root" => (format!("{HOST_MATRIX_SOURCE}#explicit-root"), "medium"),
        "claude-code" | "gemini-cli" | "github-cli" => {
            (format!("{HOST_MATRIX_SOURCE}#{target_id}"), "low")
        }
        _ => (format!("{HOST_MATRIX_SOURCE}#unknown"), "low"),
    };
    HostAdapterMetadata {
        host: target_id.to_string(),
        source_url,
        source_checked_at: HOST_MATRIX_CHECKED_AT.to_string(),
        verified_by_test: matches!(target_id, "codex-cli" | "metactl-generated" | "explicit-root"),
        confidence: confidence.to_string(),
    }
}

fn parse_frontmatter(text: &str) -> (Map<String, Value>, String, Vec<ValidationFinding>) {
    let mut findings = Vec::new();
    let trimmed = text.trim_start_matches('\u{feff}');
    let mut lines = trimmed.lines();
    let mut frontmatter = Map::new();
    let mut body = trimmed.to_string();

    let Some(first) = lines.next() else {
        findings.push(ValidationFinding {
            code: "empty_file".to_string(),
            severity: "error".to_string(),
            message: "skill file is empty".to_string(),
        });
        return (frontmatter, body, findings);
    };
    if first.trim() != "---" {
        findings.push(ValidationFinding {
            code: "missing_frontmatter".to_string(),
            severity: "warning".to_string(),
            message: "skill frontmatter is missing".to_string(),
        });
        return (frontmatter, body, findings);
    }

    let mut yaml_lines = Vec::new();
    let mut body_lines = Vec::new();
    let mut in_body = false;
    for line in lines {
        if !in_body {
            if line.trim() == "---" {
                in_body = true;
                continue;
            }
            yaml_lines.push(line);
        } else {
            body_lines.push(line);
        }
    }
    if !in_body {
        findings.push(ValidationFinding {
            code: "malformed_frontmatter".to_string(),
            severity: "error".to_string(),
            message: "frontmatter fence was not closed".to_string(),
        });
        return (frontmatter, body, findings);
    }
    let yaml_text = yaml_lines.join("\n");
    body = body_lines.join("\n");
    if yaml_text.trim().is_empty() {
        findings.push(ValidationFinding {
            code: "empty_frontmatter".to_string(),
            severity: "warning".to_string(),
            message: "frontmatter is empty".to_string(),
        });
        return (frontmatter, body, findings);
    }
    match serde_yaml::from_str::<serde_yaml::Value>(&yaml_text) {
        Ok(yaml) => {
            if let Some(map) = yaml.as_mapping() {
                for (key, value) in map {
                    if let Some(key) = key.as_str() {
                        frontmatter.insert(
                            key.to_string(),
                            serde_json::to_value(value).unwrap_or(Value::Null),
                        );
                    } else {
                        findings.push(ValidationFinding {
                            code: "unsupported_field".to_string(),
                            severity: "warning".to_string(),
                            message: "frontmatter contains a non-string key".to_string(),
                        });
                    }
                }
            } else {
                findings.push(ValidationFinding {
                    code: "malformed_frontmatter".to_string(),
                    severity: "error".to_string(),
                    message: "frontmatter is not a mapping".to_string(),
                });
            }
        }
        Err(err) => {
            findings.push(ValidationFinding {
                code: "malformed_frontmatter".to_string(),
                severity: "error".to_string(),
                message: format!("failed to parse frontmatter: {err}"),
            });
        }
    }

    (frontmatter, body, findings)
}

fn allowed_frontmatter_fields() -> BTreeSet<&'static str> {
    BTreeSet::from([
        "name",
        "description",
        "tags",
        "tools",
        "model",
        "enabled",
        "aliases",
        "supersedes",
        "complements",
        "source_pack_id",
        "source_library_ref",
        "visibility",
        "metactl.source_pack_id",
        "metactl.source_library_ref",
    ])
}

fn looks_like_secret(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    lowered.contains("api_key")
        || lowered.contains("secret")
        || lowered.contains("private key")
        || lowered.contains("ghp_")
}

fn looks_like_prompt(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    lowered.contains("system prompt")
        || lowered.contains("you are chatgpt")
        || lowered.contains("do not reveal")
        || lowered.contains("prompt injection")
}

fn normalize_name(name: &str) -> String {
    let mut normalized = String::new();
    for ch in name.to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch);
        } else if !normalized.ends_with('-') {
            normalized.push('-');
        }
    }
    normalized.trim_matches('-').to_string()
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn infer_pack_id(path: &Path) -> Option<String> {
    let components: Vec<String> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    for window in components.windows(4) {
        if window.get(0).map(|s| s.as_str()) == Some(".codex")
            && window.get(1).map(|s| s.as_str()) == Some("skills")
        {
            return window.get(2).cloned();
        }
    }
    None
}

fn compute_tree_digest(root: &Path) -> Result<String> {
    let mut entries = Vec::new();
    collect_tree_entries(root, root, &mut entries)?;
    entries.sort();
    let mut hasher = Sha256::new();
    for entry in entries {
        hasher.update(entry.as_bytes());
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn collect_tree_entries(root: &Path, current: &Path, entries: &mut Vec<String>) -> Result<()> {
    if current.is_file() {
        let rel = current
            .strip_prefix(root)
            .unwrap_or(current)
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = fs::read(current).with_context(|| format!("read {}", current.display()))?;
        entries.push(format!("{}:{}", rel, sha256_bytes(&bytes)));
        return Ok(());
    }
    if !current.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(current).with_context(|| format!("read_dir {}", current.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_tree_entries(root, &path, entries)?;
        } else if file_type.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
            entries.push(format!("{}:{}", rel, sha256_bytes(&bytes)));
        }
    }
    Ok(())
}

fn load_usage_stats(project_root: &Path) -> Result<Option<UsageStatsFile>> {
    let path = project_root.join(".metactl/usage/stats.json");
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("parse {}", path.display()))
        .map(Some)
}

fn write_json_file(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(value).context("serialize json")?;
    atomic_write(path, &bytes).with_context(|| format!("write {}", path.display()))
}

fn write_text_file(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    atomic_write(path, text.as_bytes()).with_context(|| format!("write {}", path.display()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn sha256_string(bytes: &[u8]) -> String {
    sha256_bytes(bytes)
}

fn now_string() -> String {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => format!("unix:{}", duration.as_secs()),
        Err(_) => "unix:0".to_string(),
    }
}

fn render_markdown(report: &SkillPortfolioAuditReport) -> String {
    let mut lines = Vec::new();
    lines.push("# Skill Portfolio Audit".to_string());
    lines.push(String::new());
    lines.push(format!("Target: `{}`", report.target_id));
    lines.push(format!("Scope: `{}`", report.scan_scope));
    lines.push(format!("Inventory: {}", report.summary.total_skills));
    lines.push(format!("Relations: {}", report.summary.relation_count));
    lines.push(format!("Collector: {}", report.collector_status));
    lines.push(format!("Usage window: {}", report.usage_window));
    lines.push(String::new());
    lines.push("## Inventory Counts".to_string());
    for (host, count) in &report.summary.host_counts {
        lines.push(format!("- {}: {}", host, count));
    }
    if report.summary.host_counts.is_empty() {
        lines.push("- (none)".to_string());
    }
    lines.push(String::new());
    lines.push("## Scope Counts".to_string());
    for (scope, count) in &report.summary.scope_counts {
        lines.push(format!("- {}: {}", scope, count));
    }
    if report.summary.scope_counts.is_empty() {
        lines.push("- (none)".to_string());
    }
    lines.push(String::new());
    lines.push("## Low Confidence".to_string());
    lines.push(format!("- {}", report.summary.low_confidence_count));
    lines.push(String::new());
    lines.push("## Top Actions".to_string());
    for recommendation in report.recommendations.iter().take(8) {
        lines.push(format!(
            "- {}: {} ({})",
            recommendation.subject_ids.join(", "),
            recommendation.action,
            recommendation.next_reversible_action
        ));
    }
    if report.recommendations.is_empty() {
        lines.push("- (none)".to_string());
    }
    if let Some(plan) = report.action_plan.as_ref() {
        lines.push(String::new());
        lines.push("## Action Plan".to_string());
        lines.push(format!("- plan_id: `{}`", plan.plan_id));
        lines.push(format!("- approval_required: {}", plan.approval_required));
        for action in &plan.actions {
            lines.push(format!(
                "- {} -> {}",
                action.action,
                action.subject_ids.join(", ")
            ));
        }
    }
    lines.join("\n")
}

impl Default for SkillAuditOptions {
    fn default() -> Self {
        Self {
            target_id: "codex-cli".to_string(),
            scope: SkillAuditScope::Repo,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            scan_roots: Vec::new(),
            include_local_paths: false,
            format: SkillReportFormat::Human,
            output_path: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_frontmatter_reports_missing_fence() {
        let (frontmatter, body, findings) = parse_frontmatter("name: bad\nbody");
        assert!(frontmatter.is_empty());
        assert!(body.contains("name: bad"));
        assert!(findings.iter().any(|finding| finding.code == "missing_frontmatter"));
    }

    #[test]
    fn parse_frontmatter_reads_known_fields() {
        let source = r#"---
name: demo
description: Demo skill
enabled: false
custom_field: ignored
---

Body
"#;
        let (frontmatter, body, findings) = parse_frontmatter(source);
        assert_eq!(frontmatter["name"], Value::String("demo".to_string()));
        assert_eq!(frontmatter["enabled"], Value::Bool(false));
        assert_eq!(body.trim(), "Body");
        assert!(findings.is_empty() || findings.iter().any(|finding| finding.code == "unsupported_field"));
    }

    #[test]
    fn build_relations_marks_duplicates() {
        let left = SkillInventoryItem {
            local_id: "left".to_string(),
            name: "Same Name".to_string(),
            target_kind: "codex-cli".to_string(),
            scope: "repo".to_string(),
            path: None,
            path_hash: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            source_pack_id: Some("pack-a".to_string()),
            source_library_ref: None,
            source_visibility: None,
            digest: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
            tree_digest: None,
            frontmatter: Map::new(),
            validation_findings: Vec::new(),
            enabled: Some(true),
            discovery_evidence: vec!["filesystem discovery".to_string()],
            discovery_confidence: "medium".to_string(),
            host_adapter: host_adapter_metadata("codex-cli"),
            visibility: VisibilityRecord {
                effective_visibility: "visible".to_string(),
                confidence: "medium".to_string(),
                notes: Vec::new(),
            },
            usage_join: None,
        };
        let right = SkillInventoryItem {
            local_id: "right".to_string(),
            name: "Same Name".to_string(),
            ..left.clone()
        };
        let relations = build_relations(&[left, right]);
        assert!(relations.iter().any(|relation| relation.kind == "same_name_as"));
        assert!(relations.iter().any(|relation| relation.kind == "duplicate_candidate"));
    }
}
