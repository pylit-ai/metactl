use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::types::{PackManifest, ResourceKind, VisibilityScope, API_VERSION};
use crate::LibraryRegistry;

const CODEX_PLUGIN_MANIFEST: &str = ".codex-plugin/plugin.json";
const CODEX_MARKETPLACE_MANIFEST: &str = ".agents/plugins/marketplace.json";
const CODEX_PROJECTION_MANIFEST: &str = ".codex-plugin/metactl-projection.json";
const CLAUDE_PLUGIN_MANIFEST: &str = ".claude-plugin/plugin.json";
const CLAUDE_MARKETPLACE_MANIFEST: &str = ".claude-plugin/marketplace.json";
const CLAUDE_PROJECTION_MANIFEST: &str = ".metactl/plugin-projection.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginTarget {
    CodexCli,
    ClaudeCode,
}

impl PluginTarget {
    fn parse(target: &str) -> Result<Self> {
        match target {
            "codex-cli" => Ok(Self::CodexCli),
            "claude-code" => Ok(Self::ClaudeCode),
            _ => Err(anyhow!(
                "unsupported plugin target: {} (supported: codex-cli, claude-code)",
                target
            )),
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::CodexCli => "Codex",
            Self::ClaudeCode => "Claude Code",
        }
    }

    fn keyword(self) -> &'static str {
        match self {
            Self::CodexCli => "codex-cli",
            Self::ClaudeCode => "claude-code",
        }
    }

    fn plugin_manifest_path(self) -> &'static str {
        match self {
            Self::CodexCli => CODEX_PLUGIN_MANIFEST,
            Self::ClaudeCode => CLAUDE_PLUGIN_MANIFEST,
        }
    }

    fn marketplace_manifest_path(self) -> &'static str {
        match self {
            Self::CodexCli => CODEX_MARKETPLACE_MANIFEST,
            Self::ClaudeCode => CLAUDE_MARKETPLACE_MANIFEST,
        }
    }

    fn projection_manifest_path(self) -> &'static str {
        match self {
            Self::CodexCli => CODEX_PROJECTION_MANIFEST,
            Self::ClaudeCode => CLAUDE_PROJECTION_MANIFEST,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginTier {
    Public,
    Private,
}

impl PluginTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
        }
    }

    fn includes_visibility(self, visibility: &VisibilityScope) -> bool {
        matches!(
            (self, visibility),
            (Self::Public, VisibilityScope::Shared) | (Self::Private, VisibilityScope::Private)
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginListItem {
    pub pack_id: String,
    pub version: String,
    pub title: String,
    pub visibility_scope: VisibilityScope,
    pub compatible_target: bool,
    pub eligible_tiers: Vec<PluginTier>,
}

#[derive(Debug, Clone)]
pub struct PluginExportOptions {
    pub library_root: PathBuf,
    pub target: String,
    pub tier: PluginTier,
    pub out: PathBuf,
    pub force: bool,
    pub plugin_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginExportResult {
    pub plugin_name: String,
    pub plugin_version: String,
    pub plugin_path: PathBuf,
    pub projection_path: PathBuf,
    pub target: String,
    pub tier: PluginTier,
    pub pack_ids: Vec<String>,
    pub source_digest: String,
    pub degraded_surfaces: Vec<PluginSurfaceDegradation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSurfaceDegradation {
    pub pack_id: String,
    pub surface: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct PluginVerifyOptions {
    pub path: PathBuf,
    pub target: String,
    pub tier: Option<PluginTier>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginVerifyReport {
    pub path: PathBuf,
    pub target: String,
    pub tier: PluginTier,
    pub plugin_count: usize,
    pub pack_count: usize,
    pub status: String,
    pub findings: Vec<String>,
}

#[derive(Debug, Clone)]
struct SelectedPack {
    manifest: PackManifest,
    source_path: PathBuf,
    library_root: PathBuf,
}

pub fn list_plugin_packs(
    library_root: &Path,
    target: &str,
    tier: Option<PluginTier>,
) -> Result<Vec<PluginListItem>> {
    PluginTarget::parse(target)?;
    let registry = LibraryRegistry::load_from_roots(&[library_root.to_path_buf()])?;
    let mut items = registry
        .list_packs()
        .into_iter()
        .map(|pack| {
            let compatible_target = target_compatible(&pack.manifest, target);
            let eligible_tiers = eligible_tiers(&pack.manifest.visibility_scope);
            PluginListItem {
                pack_id: pack.manifest.id,
                version: pack.manifest.version,
                title: pack.manifest.title,
                visibility_scope: pack.manifest.visibility_scope,
                compatible_target,
                eligible_tiers,
            }
        })
        .filter(|item| item.compatible_target)
        .filter(|item| {
            tier.map(|tier| item.eligible_tiers.contains(&tier))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| left.pack_id.cmp(&right.pack_id));
    Ok(items)
}

pub fn export_plugin_marketplace(options: PluginExportOptions) -> Result<PluginExportResult> {
    let target = PluginTarget::parse(&options.target)?;
    let selected = selected_packs(&options.library_root, &options.target, options.tier)?;
    if selected.is_empty() {
        return Err(anyhow!(
            "no {} packs compatible with {} were found in {}",
            options.tier.as_str(),
            options.target,
            options.library_root.display()
        ));
    }

    let source_digest = source_digest(&selected)?;
    let digest_suffix = digest_suffix(&source_digest);
    let source_label = source_label(&options.library_root, options.tier);
    let plugin_name = options.plugin_name.clone().unwrap_or_else(|| {
        format!(
            "metactl-{}-{}",
            options.tier.as_str(),
            slugify(&source_label)
        )
    });
    let plugin_version = format!("0.1.0+{}", digest_suffix);
    let plugin_path = options.out.join("plugins").join(&plugin_name);
    if plugin_path.exists() {
        if options.force {
            fs::remove_dir_all(&plugin_path)
                .with_context(|| format!("remove {}", plugin_path.display()))?;
        } else {
            return Err(anyhow!(
                "plugin output already exists: {} (pass --force to replace)",
                plugin_path.display()
            ));
        }
    }

    fs::create_dir_all(plugin_path.join("skills"))
        .with_context(|| format!("create {}", plugin_path.join("skills").display()))?;
    create_parent_dir(&plugin_path.join(target.plugin_manifest_path()))?;
    create_parent_dir(&plugin_path.join(target.projection_manifest_path()))?;
    create_parent_dir(&options.out.join(target.marketplace_manifest_path()))?;

    let mut pack_records = Vec::new();
    let mut pack_ids = Vec::new();
    let mut degraded_surfaces = Vec::new();

    for pack in &selected {
        pack_ids.push(pack.manifest.id.clone());
        let skill_dir = plugin_path.join("skills").join(slugify(&pack.manifest.id));
        let pack_degraded = copy_pack_skill(pack, &skill_dir)?;
        degraded_surfaces.extend(pack_degraded);
        pack_records.push(pack_record(pack, options.tier));
    }

    let projection = json!({
        "kind": "metactl_plugin_projection",
        "api_version": API_VERSION,
        "target_runtime": options.target,
        "output_tier": options.tier,
        "source_library": projection_source_library(&options.library_root, options.tier),
        "source_ref": source_ref(&options.library_root),
        "source_digest": source_digest,
        "generated_at": now_string(),
        "visibility_filter": {
            "included": options.tier.as_str(),
            "shared_private": "unsupported_deferred"
        },
        "pack_ids": pack_ids,
        "packs": pack_records,
        "degraded_surfaces": degraded_surfaces,
        "install": install_instructions(options.tier),
    });
    let projection_path = plugin_path.join(target.projection_manifest_path());
    write_json(&projection_path, &projection)?;

    let projected_pack_ids = pack_ids_for_projection(&projection);
    let plugin_manifest = plugin_manifest(
        target,
        &plugin_name,
        &plugin_version,
        options.tier,
        &source_label,
        &projected_pack_ids,
    );
    write_json(
        &plugin_path.join(target.plugin_manifest_path()),
        &plugin_manifest,
    )?;
    write_readme(
        &plugin_path.join("README.md"),
        target,
        options.tier,
        &plugin_name,
    )?;
    let marketplace_manifest = marketplace_manifest(
        target,
        &plugin_name,
        &plugin_version,
        options.tier,
        &source_label,
    );
    write_json(
        &options.out.join(target.marketplace_manifest_path()),
        &marketplace_manifest,
    )?;

    Ok(PluginExportResult {
        plugin_name,
        plugin_version,
        plugin_path,
        projection_path,
        target: options.target,
        tier: options.tier,
        pack_ids: projected_pack_ids,
        source_digest: projection["source_digest"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        degraded_surfaces: serde_json::from_value(projection["degraded_surfaces"].clone())
            .unwrap_or_default(),
    })
}

pub fn verify_plugin_marketplace(options: PluginVerifyOptions) -> Result<PluginVerifyReport> {
    let target = PluginTarget::parse(&options.target)?;
    let marketplace_manifest = options.path.join(target.marketplace_manifest_path());
    let is_direct_bundle = options.path.join(target.plugin_manifest_path()).exists();
    if !is_direct_bundle && !marketplace_manifest.exists() {
        return Err(anyhow!(
            "{} marketplace root is missing {}",
            target.display_name(),
            marketplace_manifest.display()
        ));
    }
    if marketplace_manifest.exists() {
        validate_marketplace_manifest(target, &options.path, &marketplace_manifest)?;
    }
    let bundles = plugin_bundles(target, &options.path)?;
    if bundles.is_empty() {
        return Err(anyhow!(
            "no {} plugin bundle found at {}",
            target.display_name(),
            options.path.display()
        ));
    }

    let mut findings = Vec::new();
    let mut pack_count = 0usize;
    let mut report_tier = options.tier.unwrap_or(PluginTier::Private);

    for bundle in &bundles {
        let plugin_manifest = bundle.join(target.plugin_manifest_path());
        if !plugin_manifest.exists() {
            findings.push(format!("missing {}", plugin_manifest.display()));
            continue;
        }
        let projection_path = bundle.join(target.projection_manifest_path());
        if !projection_path.exists() {
            findings.push(format!("missing {}", projection_path.display()));
            continue;
        }
        let projection: Value = serde_json::from_slice(
            &fs::read(&projection_path)
                .with_context(|| format!("read {}", projection_path.display()))?,
        )
        .with_context(|| format!("decode {}", projection_path.display()))?;
        let target = projection
            .get("target_runtime")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if target != options.target {
            findings.push(format!(
                "{} target mismatch: expected {}, got {}",
                projection_path.display(),
                options.target,
                target
            ));
        }
        let tier = projection
            .get("output_tier")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or(PluginTier::Private);
        report_tier = tier;
        if let Some(expected) = options.tier {
            if tier != expected {
                findings.push(format!(
                    "{} tier mismatch: expected {}, got {}",
                    projection_path.display(),
                    expected.as_str(),
                    tier.as_str()
                ));
            }
        }
        if tier == PluginTier::Public {
            public_projection_findings(&projection, &mut findings);
        }
        let pack_ids = projection
            .get("pack_ids")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        pack_count += pack_ids.len();
        for pack_id in pack_ids.iter().filter_map(Value::as_str) {
            let skill_md = bundle
                .join("skills")
                .join(slugify(pack_id))
                .join("SKILL.md");
            if !skill_md.exists() {
                findings.push(format!("missing {}", skill_md.display()));
            }
        }
    }

    let status = if findings.is_empty() { "pass" } else { "fail" }.to_string();
    Ok(PluginVerifyReport {
        path: options.path,
        target: options.target,
        tier: report_tier,
        plugin_count: bundles.len(),
        pack_count,
        status,
        findings,
    })
}

fn selected_packs(
    library_root: &Path,
    target: &str,
    tier: PluginTier,
) -> Result<Vec<SelectedPack>> {
    let registry = LibraryRegistry::load_from_roots(&[library_root.to_path_buf()])?;
    let mut packs = registry
        .list_packs()
        .into_iter()
        .filter(|pack| target_compatible(&pack.manifest, target))
        .filter(|pack| tier.includes_visibility(&pack.manifest.visibility_scope))
        .map(|pack| SelectedPack {
            manifest: pack.manifest,
            source_path: pack.source_path,
            library_root: pack.library_root,
        })
        .collect::<Vec<_>>();
    packs.sort_by(|left, right| left.manifest.id.cmp(&right.manifest.id));
    Ok(packs)
}

fn target_compatible(pack: &PackManifest, target: &str) -> bool {
    pack.compatible_targets.is_empty() || pack.compatible_targets.iter().any(|item| item == target)
}

fn eligible_tiers(visibility: &VisibilityScope) -> Vec<PluginTier> {
    match visibility {
        VisibilityScope::Shared => vec![PluginTier::Public],
        VisibilityScope::Private => vec![PluginTier::Private],
    }
}

fn copy_pack_skill(pack: &SelectedPack, skill_dir: &Path) -> Result<Vec<PluginSurfaceDegradation>> {
    let mut degraded = Vec::new();
    fs::create_dir_all(skill_dir).with_context(|| format!("create {}", skill_dir.display()))?;
    let Some(instruction) = pack
        .manifest
        .resources
        .iter()
        .find(|resource| resource.kind == ResourceKind::Instruction)
    else {
        degraded.push(PluginSurfaceDegradation {
            pack_id: pack.manifest.id.clone(),
            surface: "skill".to_string(),
            reason: "missing instruction resource".to_string(),
        });
        return Ok(degraded);
    };

    let instruction_path = pack.library_root.join(&instruction.path);
    if !instruction_path.is_file() {
        degraded.push(PluginSurfaceDegradation {
            pack_id: pack.manifest.id.clone(),
            surface: instruction.path.clone(),
            reason: "instruction resource file missing".to_string(),
        });
        return Ok(degraded);
    }
    let base_dir = instruction_path
        .parent()
        .filter(|parent| *parent != pack.library_root)
        .unwrap_or(&pack.library_root);
    let mut copied = BTreeSet::new();
    for resource in &pack.manifest.resources {
        let source = pack.library_root.join(&resource.path);
        if !source.is_file() {
            if resource.required {
                degraded.push(PluginSurfaceDegradation {
                    pack_id: pack.manifest.id.clone(),
                    surface: resource.path.clone(),
                    reason: "required resource file missing".to_string(),
                });
            }
            continue;
        }
        let relative = if source == instruction_path {
            PathBuf::from("SKILL.md")
        } else if let Ok(relative) = source.strip_prefix(base_dir) {
            sanitize_relative_path(relative)
        } else {
            resource_fallback_path(resource.kind.clone(), &source)
        };
        if copied.insert(relative.clone()) {
            copy_file(&source, &skill_dir.join(relative))?;
        }
    }
    if !skill_dir.join("SKILL.md").exists() {
        degraded.push(PluginSurfaceDegradation {
            pack_id: pack.manifest.id.clone(),
            surface: "SKILL.md".to_string(),
            reason: "instruction resource did not produce SKILL.md".to_string(),
        });
    }
    Ok(degraded)
}

fn sanitize_relative_path(path: &Path) -> PathBuf {
    let mut sanitized = PathBuf::new();
    for component in path.components() {
        if let std::path::Component::Normal(value) = component {
            sanitized.push(value);
        }
    }
    if sanitized.as_os_str().is_empty() {
        PathBuf::from("resource")
    } else {
        sanitized
    }
}

fn resource_fallback_path(kind: ResourceKind, source: &Path) -> PathBuf {
    let file_name = source
        .file_name()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("resource"));
    match kind {
        ResourceKind::Script => PathBuf::from("scripts").join(file_name),
        ResourceKind::Example | ResourceKind::KnowledgeSource => {
            PathBuf::from("references").join(file_name)
        }
        ResourceKind::Asset => PathBuf::from("assets").join(file_name),
        _ => file_name,
    }
}

fn copy_file(source: &Path, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::copy(source, destination)
        .with_context(|| format!("copy {} to {}", source.display(), destination.display()))?;
    Ok(())
}

fn source_digest(packs: &[SelectedPack]) -> Result<String> {
    let mut hasher = Sha256::new();
    for pack in packs {
        hasher.update(pack.manifest.id.as_bytes());
        hasher.update([0]);
        hasher.update(
            fs::read(&pack.source_path)
                .with_context(|| format!("read pack manifest {}", pack.source_path.display()))?,
        );
        hasher.update([0]);
        for resource in &pack.manifest.resources {
            let path = pack.library_root.join(&resource.path);
            if path.is_file() {
                hasher.update(resource.path.as_bytes());
                hasher.update([0]);
                hasher.update(
                    fs::read(&path).with_context(|| format!("read resource {}", path.display()))?,
                );
                hasher.update([0]);
            }
        }
    }
    Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
}

fn pack_record(pack: &SelectedPack, tier: PluginTier) -> Value {
    let mut record = json!({
        "id": pack.manifest.id,
        "version": pack.manifest.version,
        "title": pack.manifest.title,
        "visibility_scope": pack.manifest.visibility_scope,
        "compatible_targets": pack.manifest.compatible_targets,
        "resources": pack.manifest.resources,
    });
    if tier == PluginTier::Private {
        record["source_manifest_path"] = json!(pack.source_path.to_string_lossy().to_string());
    }
    record
}

fn create_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    Ok(())
}

fn plugin_description(tier: PluginTier, source_label: &str) -> String {
    format!(
        "metactl {} plugin projection for {}. Packs remain canonical in the source library.",
        tier.as_str(),
        source_label
    )
}

fn plugin_manifest(
    target: PluginTarget,
    plugin_name: &str,
    plugin_version: &str,
    tier: PluginTier,
    source_label: &str,
    pack_ids: &[String],
) -> Value {
    let display_tier = match tier {
        PluginTier::Public => "Public",
        PluginTier::Private => "Private",
    };
    let description = plugin_description(tier, source_label);
    match target {
        PluginTarget::CodexCli => json!({
            "name": plugin_name,
            "version": plugin_version,
            "description": description,
            "author": { "name": "metactl" },
            "license": if tier == PluginTier::Public { "Apache-2.0" } else { "UNLICENSED" },
            "keywords": ["metactl", "packs", target.keyword(), tier.as_str()],
            "skills": "./skills/",
            "interface": {
                "displayName": format!("metactl {} packs", display_tier),
                "shortDescription": format!("{} metactl pack projection", display_tier),
                "longDescription": description,
                "developerName": "metactl",
                "category": "Engineering",
                "capabilities": ["Read", "Write"],
                "defaultPrompt": [
                    format!("Use one of these metactl packs: {}", pack_ids.join(", "))
                ],
                "screenshots": []
            }
        }),
        PluginTarget::ClaudeCode => json!({
            "name": plugin_name,
            "version": plugin_version,
            "description": description,
            "author": { "name": "metactl" },
            "license": if tier == PluginTier::Public { "Apache-2.0" } else { "UNLICENSED" },
            "keywords": ["metactl", "packs", target.keyword(), tier.as_str()],
            "skills": "./skills/"
        }),
    }
}

fn marketplace_manifest(
    target: PluginTarget,
    plugin_name: &str,
    plugin_version: &str,
    tier: PluginTier,
    source_label: &str,
) -> Value {
    let description = plugin_description(tier, source_label);
    match target {
        PluginTarget::CodexCli => json!({
            "name": "metactl-local",
            "plugins": [
                {
                    "name": plugin_name,
                    "source": {
                        "source": "local",
                        "path": format!("./plugins/{plugin_name}")
                    },
                    "policy": {
                        "installation": "AVAILABLE",
                        "authentication": "ON_INSTALL"
                    },
                    "category": "Engineering"
                }
            ]
        }),
        PluginTarget::ClaudeCode => json!({
            "name": "metactl-local",
            "owner": { "name": "metactl" },
            "metadata": {
                "description": "Local metactl pack projections."
            },
            "plugins": [
                {
                    "name": plugin_name,
                    "source": format!("./plugins/{plugin_name}"),
                    "description": description,
                    "version": plugin_version,
                    "author": { "name": "metactl" },
                    "category": "productivity",
                    "keywords": ["metactl", "packs", target.keyword(), tier.as_str()]
                }
            ]
        }),
    }
}

fn validate_marketplace_manifest(
    target: PluginTarget,
    root: &Path,
    manifest_path: &Path,
) -> Result<()> {
    let manifest: Value = serde_json::from_slice(
        &fs::read(manifest_path).with_context(|| format!("read {}", manifest_path.display()))?,
    )
    .with_context(|| format!("decode {}", manifest_path.display()))?;
    let plugins = manifest
        .get("plugins")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("{} missing plugins array", manifest_path.display()))?;
    if plugins.is_empty() {
        return Err(anyhow!("{} contains no plugins", manifest_path.display()));
    }
    for plugin in plugins {
        let source_path = match target {
            PluginTarget::CodexCli => {
                let source = plugin
                    .get("source")
                    .and_then(Value::as_object)
                    .ok_or_else(|| anyhow!("{} plugin missing source", manifest_path.display()))?;
                let source_kind = source
                    .get("source")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if source_kind != "local" {
                    return Err(anyhow!(
                        "{} only supports local generated plugin sources, got {}",
                        manifest_path.display(),
                        source_kind
                    ));
                }
                source.get("path").and_then(Value::as_str).ok_or_else(|| {
                    anyhow!("{} plugin source missing path", manifest_path.display())
                })?
            }
            PluginTarget::ClaudeCode => plugin
                .get("source")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("{} plugin source missing path", manifest_path.display()))?,
        };
        let bundle_path = root.join(source_path);
        if !bundle_path.join(target.plugin_manifest_path()).exists() {
            return Err(anyhow!(
                "{} source path {} does not contain {}",
                manifest_path.display(),
                source_path,
                target.plugin_manifest_path()
            ));
        }
    }
    Ok(())
}

fn write_readme(
    path: &Path,
    target: PluginTarget,
    tier: PluginTier,
    plugin_name: &str,
) -> Result<()> {
    let target_name = target.display_name();
    let target_arg = target.keyword();
    let body = match tier {
        PluginTier::Public => format!(
            "# {plugin_name}\n\nGenerated {target_name} plugin projection from public metactl packs.\n\nPacks remain canonical in the source metactl library. This directory is an installable {target_name} plugin bundle.\n\nVerify:\n\n```bash\nmetactl plugin verify --target {target_arg} --tier public --path /path/to/plugin-marketplace\n```\n\nExpected output:\n\n```text\nVerified {target_name} plugin marketplace: pass\n```\n"
        ),
        PluginTier::Private => format!(
            "# {plugin_name}\n\nGenerated {target_name} plugin projection from private metactl packs.\n\nKeep this bundle in a local or private Git marketplace. Do not publish it unless every included pack has been reviewed for public release.\n\nVerify:\n\n```bash\nmetactl plugin verify --target {target_arg} --tier private --path /path/to/private-plugin-marketplace\n```\n\nExpected output:\n\n```text\nVerified {target_name} plugin marketplace: pass\n```\n"
        ),
    };
    fs::write(path, body).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn source_label(library_root: &Path, tier: PluginTier) -> String {
    if tier == PluginTier::Public {
        "starter".to_string()
    } else {
        library_root
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("library")
            .to_string()
    }
}

fn projection_source_library(library_root: &Path, tier: PluginTier) -> Value {
    match tier {
        PluginTier::Public => json!("library/starter"),
        PluginTier::Private => json!(library_root.to_string_lossy().to_string()),
    }
}

fn source_ref(library_root: &Path) -> Value {
    let git_dir = library_root.join(".git");
    if !git_dir.exists() {
        return Value::Null;
    }
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(library_root)
        .args(["rev-parse", "HEAD"])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            json!(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        _ => Value::Null,
    }
}

fn install_instructions(tier: PluginTier) -> Vec<String> {
    match tier {
        PluginTier::Public => vec![
            "Export to a reviewed public plugin marketplace root.".to_string(),
            "Run metactl plugin verify and the public boundary scanner before publication."
                .to_string(),
        ],
        PluginTier::Private => vec![
            "Keep the generated marketplace root local or in a private Git repository.".to_string(),
            "Run metactl plugin verify on each machine before relying on the bundle.".to_string(),
        ],
    }
}

fn plugin_bundles(target: PluginTarget, path: &Path) -> Result<Vec<PathBuf>> {
    if path.join(target.plugin_manifest_path()).exists() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut bundles = Vec::new();
    find_plugin_bundles(target, path, 0, &mut bundles)?;
    bundles.sort();
    Ok(bundles)
}

fn find_plugin_bundles(
    target: PluginTarget,
    path: &Path,
    depth: usize,
    bundles: &mut Vec<PathBuf>,
) -> Result<()> {
    if depth > 2 || !path.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(path).with_context(|| format!("read {}", path.display()))? {
        let entry = entry?;
        let child = entry.path();
        if !child.is_dir() {
            continue;
        }
        if child.join(target.plugin_manifest_path()).exists() {
            bundles.push(child);
        } else {
            find_plugin_bundles(target, &child, depth + 1, bundles)?;
        }
    }
    Ok(())
}

fn public_projection_findings(projection: &Value, findings: &mut Vec<String>) {
    let source = projection
        .get("source_library")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if source.starts_with('/') {
        findings.push("public projection contains local source path".to_string());
    }
}

fn pack_ids_for_projection(projection: &Value) -> Vec<String> {
    projection
        .get("pack_ids")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn digest_suffix(digest: &str) -> String {
    digest
        .strip_prefix("sha256:")
        .unwrap_or(digest)
        .chars()
        .take(12)
        .collect()
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if ch == '-' || ch == '_' || ch.is_whitespace() || ch == '/' {
            if !slug.ends_with('-') {
                slug.push('-');
            }
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "plugin".to_string()
    } else {
        slug
    }
}

fn now_string() -> String {
    format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or_default()
    )
}
