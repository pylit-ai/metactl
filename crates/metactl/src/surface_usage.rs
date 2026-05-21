use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::API_VERSION;

pub const DEFAULT_LIFECYCLE_MODE: SurfaceLifecycleMode = SurfaceLifecycleMode::Recommend;
pub const DEFAULT_REBUILD_TRIGGER: SurfaceRebuildTrigger = SurfaceRebuildTrigger::Opportunistic;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceLifecycleMode {
    Observe,
    Recommend,
    Apply,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceRebuildTrigger {
    Opportunistic,
    Scheduled,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceTier {
    Hot,
    Warm,
    Cold,
    PinnedCommand,
    Blocked,
}

impl SurfaceTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            SurfaceTier::Hot => "hot",
            SurfaceTier::Warm => "warm",
            SurfaceTier::Cold => "cold",
            SurfaceTier::PinnedCommand => "pinned_command",
            SurfaceTier::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceOverrideAction {
    PinHot,
    PinCommand,
    Block,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SurfaceOverrideRecord {
    pub action: SurfaceOverrideAction,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SurfaceOverrides {
    pub api_version: String,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub overrides: BTreeMap<String, SurfaceOverrideRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PackUsageStats {
    pub pack_id: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SurfaceUsageStats {
    pub api_version: String,
    pub generated_at: String,
    pub source_path: String,
    pub event_count: u64,
    #[serde(default)]
    pub packs: Vec<PackUsageStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SurfaceRecommendation {
    pub pack_id: String,
    pub tier: SurfaceTier,
    pub score: i64,
    pub reason_code: String,
    pub next_action: String,
    pub event_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_at: Option<String>,
    #[serde(default)]
    pub counts: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SurfaceReport {
    pub api_version: String,
    pub generated_at: String,
    pub lifecycle_mode: SurfaceLifecycleMode,
    pub rebuild_trigger: SurfaceRebuildTrigger,
    pub adapter_mutation_allowed: bool,
    pub usage_event_path: String,
    pub stats_path: String,
    pub report_json_path: String,
    pub report_markdown_path: String,
    pub total_packs: usize,
    pub pending_recommendation_count: usize,
    #[serde(default)]
    pub recommendations: Vec<SurfaceRecommendation>,
    #[serde(default)]
    pub notes: Vec<String>,
}

pub fn usage_events_path(project_root: &Path) -> PathBuf {
    project_root.join(".metactl/usage/events.jsonl")
}

pub fn usage_stats_path(project_root: &Path) -> PathBuf {
    project_root.join(".metactl/usage/stats.json")
}

pub fn surface_overrides_path(project_root: &Path) -> PathBuf {
    project_root.join(".metactl/usage/surface-overrides.json")
}

pub fn surface_report_json_path(project_root: &Path) -> PathBuf {
    project_root.join("reports/surfaces/latest.json")
}

pub fn surface_report_markdown_path(project_root: &Path) -> PathBuf {
    project_root.join("docs/status/surfaces/dashboard.md")
}

pub fn rebuild_usage_stats(
    project_root: &Path,
    events_path: Option<&Path>,
    stats_path: Option<&Path>,
) -> Result<SurfaceUsageStats> {
    let event_path = events_path
        .map(PathBuf::from)
        .unwrap_or_else(|| usage_events_path(project_root));
    let output_path = stats_path
        .map(PathBuf::from)
        .unwrap_or_else(|| usage_stats_path(project_root));
    let stats = read_usage_events(&event_path)?;
    write_json_file(&output_path, &stats)?;
    Ok(stats)
}

pub fn load_usage_stats(project_root: &Path) -> Result<Option<SurfaceUsageStats>> {
    let path = usage_stats_path(project_root);
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("parse {}", path.display()))
        .map(Some)
}

pub fn load_or_rebuild_usage_stats(project_root: &Path) -> Result<SurfaceUsageStats> {
    let events = usage_events_path(project_root);
    let stats = usage_stats_path(project_root);
    if !stats.exists() || is_stale(&events, &stats)? {
        return rebuild_usage_stats(project_root, None, None);
    }
    load_usage_stats(project_root)?.ok_or_else(|| anyhow!("usage stats were not readable"))
}

pub fn load_surface_overrides(project_root: &Path) -> Result<SurfaceOverrides> {
    let path = surface_overrides_path(project_root);
    if !path.exists() {
        return Ok(SurfaceOverrides {
            api_version: API_VERSION.to_string(),
            updated_at: None,
            overrides: BTreeMap::new(),
        });
    }
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

pub fn set_surface_override(
    project_root: &Path,
    pack_id: &str,
    action: SurfaceOverrideAction,
) -> Result<SurfaceOverrides> {
    let mut overrides = load_surface_overrides(project_root)?;
    let now = now_string();
    overrides.api_version = API_VERSION.to_string();
    overrides.updated_at = Some(now.clone());
    overrides.overrides.insert(
        pack_id.to_string(),
        SurfaceOverrideRecord {
            action,
            updated_at: now,
        },
    );
    write_surface_overrides(project_root, &overrides)?;
    Ok(overrides)
}

pub fn reset_surface_override(
    project_root: &Path,
    pack_id: Option<&str>,
) -> Result<SurfaceOverrides> {
    let mut overrides = load_surface_overrides(project_root)?;
    if let Some(pack_id) = pack_id {
        overrides.overrides.remove(pack_id);
    } else {
        overrides.overrides.clear();
    }
    overrides.api_version = API_VERSION.to_string();
    overrides.updated_at = Some(now_string());
    write_surface_overrides(project_root, &overrides)?;
    Ok(overrides)
}

pub fn build_surface_report(
    project_root: &Path,
    lifecycle_mode: SurfaceLifecycleMode,
    rebuild_trigger: SurfaceRebuildTrigger,
    known_pack_ids: &[String],
    stats: &SurfaceUsageStats,
    overrides: &SurfaceOverrides,
) -> SurfaceReport {
    let mut by_pack = stats
        .packs
        .iter()
        .map(|item| (item.pack_id.clone(), item.clone()))
        .collect::<BTreeMap<_, _>>();
    for pack_id in known_pack_ids {
        by_pack
            .entry(pack_id.clone())
            .or_insert_with(|| PackUsageStats {
                pack_id: pack_id.clone(),
                ..PackUsageStats::default()
            });
    }

    let recommendations = by_pack
        .values()
        .map(|pack| recommendation_for_pack(pack, overrides))
        .collect::<Vec<_>>();
    let pending_recommendation_count = recommendations
        .iter()
        .filter(|item| !matches!(item.tier, SurfaceTier::Cold))
        .count();
    let adapter_mutation_allowed = lifecycle_mode == SurfaceLifecycleMode::Apply;
    let mut notes = vec![
        "Scheduled runs are report-only unless lifecycle_mode is apply.".to_string(),
        "Command invocation is demand evidence, not outcome evidence.".to_string(),
    ];
    if stats.event_count == 0 {
        notes.push("No usage events found; recommendations remain cold unless pinned.".to_string());
    }

    SurfaceReport {
        api_version: API_VERSION.to_string(),
        generated_at: now_string(),
        lifecycle_mode,
        rebuild_trigger,
        adapter_mutation_allowed,
        usage_event_path: relative_path(project_root, &usage_events_path(project_root)),
        stats_path: relative_path(project_root, &usage_stats_path(project_root)),
        report_json_path: relative_path(project_root, &surface_report_json_path(project_root)),
        report_markdown_path: relative_path(
            project_root,
            &surface_report_markdown_path(project_root),
        ),
        total_packs: recommendations.len(),
        pending_recommendation_count,
        recommendations,
        notes,
    }
}

pub fn write_surface_report(project_root: &Path, report: &SurfaceReport) -> Result<()> {
    write_json_file(&surface_report_json_path(project_root), report)?;
    write_text_file(
        &surface_report_markdown_path(project_root),
        &render_surface_dashboard(report),
    )?;
    Ok(())
}

pub fn render_surface_dashboard(report: &SurfaceReport) -> String {
    let mut out = String::new();
    out.push_str("# Surface Recommendations\n\n");
    out.push_str("Generated artifact. Do not hand edit.\n\n");
    out.push_str(&format!("- Generated at: {}\n", report.generated_at));
    out.push_str(&format!("- Lifecycle mode: {:?}\n", report.lifecycle_mode));
    out.push_str(&format!(
        "- Rebuild trigger: {:?}\n",
        report.rebuild_trigger
    ));
    out.push_str(&format!(
        "- Adapter mutation allowed: {}\n",
        report.adapter_mutation_allowed
    ));
    out.push_str(&format!(
        "- Pending recommendations: {}\n\n",
        report.pending_recommendation_count
    ));
    out.push_str("| Pack | Tier | Score | Reason | Next action |\n");
    out.push_str("| --- | --- | ---: | --- | --- |\n");
    for item in &report.recommendations {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            item.pack_id,
            item.tier.as_str(),
            item.score,
            item.reason_code,
            item.next_action
        ));
    }
    out.push_str("\n## Notes\n\n");
    for note in &report.notes {
        out.push_str(&format!("- {note}\n"));
    }
    out
}

pub fn surface_report_summary_json(project_root: &Path) -> Value {
    let stats_path = usage_stats_path(project_root);
    let report_path = surface_report_json_path(project_root);
    let stats_exists = stats_path.exists();
    let report_exists = report_path.exists();
    let stale = is_stale(&usage_events_path(project_root), &stats_path).unwrap_or(false);
    json!({
        "lifecycle_mode": "recommend",
        "rebuild_trigger": "opportunistic",
        "stats_path": relative_path(project_root, &stats_path),
        "report_json_path": relative_path(project_root, &report_path),
        "report_markdown_path": relative_path(project_root, &surface_report_markdown_path(project_root)),
        "stats_exists": stats_exists,
        "report_exists": report_exists,
        "stats_stale": stale,
        "scheduled_automation": "inspect_with_metactl_background_status",
        "next_reversible_action": "metactl surface report",
    })
}

fn read_usage_events(path: &Path) -> Result<SurfaceUsageStats> {
    let mut by_pack: BTreeMap<String, PackUsageStats> = BTreeMap::new();
    let mut event_count = 0u64;
    if path.exists() {
        let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(line)
                .with_context(|| format!("parse {} line {}", path.display(), index + 1))?;
            event_count += 1;
            let Some(pack_id) = event_pack_id(&value) else {
                continue;
            };
            let event_kind = string_field(&value, "event_kind")
                .or_else(|| string_field(&value, "kind"))
                .unwrap_or("unknown");
            let outcome_kind = string_field(&value, "outcome_kind")
                .or_else(|| string_field(&value, "outcome"))
                .unwrap_or("unknown");
            let recorded_at = string_field(&value, "recorded_at")
                .or_else(|| string_field(&value, "timestamp"))
                .map(str::to_string);
            let stats = by_pack
                .entry(pack_id.to_string())
                .or_insert_with(|| PackUsageStats {
                    pack_id: pack_id.to_string(),
                    ..PackUsageStats::default()
                });
            apply_event(stats, event_kind, outcome_kind, recorded_at);
        }
    }
    SurfaceUsageStats {
        api_version: API_VERSION.to_string(),
        generated_at: now_string(),
        source_path: path.to_string_lossy().to_string(),
        event_count,
        packs: by_pack.into_values().collect(),
    }
    .pipe(Ok)
}

fn apply_event(
    stats: &mut PackUsageStats,
    event_kind: &str,
    outcome_kind: &str,
    recorded_at: Option<String>,
) {
    stats.event_count += 1;
    if let Some(recorded_at) = recorded_at {
        if stats
            .last_event_at
            .as_ref()
            .map(|current| recorded_at > *current)
            .unwrap_or(true)
        {
            stats.last_event_at = Some(recorded_at);
        }
    }
    match event_kind {
        "command_invoked" => {
            stats.command_invoked += 1;
            stats.score += 10;
        }
        "skill_body_read" => {
            stats.skill_body_read += 1;
            stats.score += 8;
        }
        "pack_resolved" => {
            stats.pack_resolved += 1;
            stats.score += 6;
        }
        "search_result_selected" => {
            stats.search_result_selected += 1;
            stats.score += 4;
        }
        "task_verified" => {
            stats.task_verified += 1;
            stats.score += 12;
        }
        "correction_or_retry" => {
            stats.correction_or_retry += 1;
            stats.score += 1;
        }
        "dismissed_or_abandoned" | "dismissed" | "abandoned" => {
            stats.dismissed_or_abandoned += 1;
            stats.score -= 4;
        }
        "blocked_or_rejected" | "blocked" | "rejected" => {
            stats.blocked_or_rejected += 1;
            stats.score -= 100;
        }
        _ => {}
    }
    match outcome_kind {
        "succeeded" | "verified" => {
            stats.task_verified += 1;
            stats.score += 12;
        }
        "corrected" => {
            stats.correction_or_retry += 1;
            stats.score += 1;
        }
        "failed" | "abandoned" | "dismissed" => {
            stats.dismissed_or_abandoned += 1;
            stats.score -= 4;
        }
        _ => {}
    }
}

fn recommendation_for_pack(
    stats: &PackUsageStats,
    overrides: &SurfaceOverrides,
) -> SurfaceRecommendation {
    let override_action = overrides
        .overrides
        .get(&stats.pack_id)
        .map(|item| &item.action);
    let (tier, reason_code, next_action) = match override_action {
        Some(SurfaceOverrideAction::Block) => (
            SurfaceTier::Blocked,
            "operator_block".to_string(),
            "metactl surface reset <pack>".to_string(),
        ),
        Some(SurfaceOverrideAction::PinHot) => (
            SurfaceTier::Hot,
            "operator_pin_hot".to_string(),
            "metactl sync --surface-mode auto --preview".to_string(),
        ),
        Some(SurfaceOverrideAction::PinCommand) => (
            SurfaceTier::PinnedCommand,
            "operator_pin_command".to_string(),
            "metactl sync --surface-mode auto --preview".to_string(),
        ),
        None if stats.blocked_or_rejected > 0 && stats.score < 0 => (
            SurfaceTier::Blocked,
            "blocked_or_rejected".to_string(),
            "inspect policy or run metactl surface reset <pack>".to_string(),
        ),
        None if stats.task_verified > 0 => (
            SurfaceTier::Hot,
            "verified_outcome".to_string(),
            "metactl sync --surface-mode auto --preview".to_string(),
        ),
        None if stats.command_invoked
            + stats.skill_body_read
            + stats.pack_resolved
            + stats.search_result_selected
            > 0 =>
        {
            (
                SurfaceTier::Warm,
                "usage_observed_without_verified_outcome".to_string(),
                "collect outcome evidence or pin explicitly".to_string(),
            )
        }
        None => (
            SurfaceTier::Cold,
            "no_usage_evidence".to_string(),
            "no action".to_string(),
        ),
    };
    SurfaceRecommendation {
        pack_id: stats.pack_id.clone(),
        tier,
        score: stats.score,
        reason_code,
        next_action,
        event_count: stats.event_count,
        last_event_at: stats.last_event_at.clone(),
        counts: BTreeMap::from([
            ("command_invoked".to_string(), stats.command_invoked),
            ("skill_body_read".to_string(), stats.skill_body_read),
            ("pack_resolved".to_string(), stats.pack_resolved),
            (
                "search_result_selected".to_string(),
                stats.search_result_selected,
            ),
            ("task_verified".to_string(), stats.task_verified),
            ("correction_or_retry".to_string(), stats.correction_or_retry),
            (
                "dismissed_or_abandoned".to_string(),
                stats.dismissed_or_abandoned,
            ),
            ("blocked_or_rejected".to_string(), stats.blocked_or_rejected),
        ]),
    }
}

fn event_pack_id(value: &Value) -> Option<&str> {
    string_field(value, "pack_id")
        .or_else(|| match value.get("pack_ref") {
            Some(Value::String(item)) => Some(item.as_str()),
            Some(Value::Object(obj)) => obj.get("id").and_then(Value::as_str),
            _ => None,
        })
        .or_else(|| match value.get("resource_ref") {
            Some(Value::Object(obj)) => obj.get("pack_id").and_then(Value::as_str),
            _ => None,
        })
}

fn string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn write_surface_overrides(project_root: &Path, overrides: &SurfaceOverrides) -> Result<()> {
    write_json_file(&surface_overrides_path(project_root), overrides)
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).context("serialize JSON")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}

fn write_text_file(path: &Path, value: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, value.as_bytes()).with_context(|| format!("write {}", path.display()))
}

fn is_stale(source: &Path, projection: &Path) -> Result<bool> {
    if !source.exists() {
        return Ok(false);
    }
    if !projection.exists() {
        return Ok(true);
    }
    let source_modified = fs::metadata(source)
        .and_then(|metadata| metadata.modified())
        .with_context(|| format!("read metadata {}", source.display()))?;
    let projection_modified = fs::metadata(projection)
        .and_then(|metadata| metadata.modified())
        .with_context(|| format!("read metadata {}", projection.display()))?;
    Ok(source_modified > projection_modified)
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn now_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}

pub fn known_pack_ids_from_refs(pack_ids: impl IntoIterator<Item = String>) -> Vec<String> {
    pack_ids
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
