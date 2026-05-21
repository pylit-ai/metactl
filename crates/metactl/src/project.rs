use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use include_dir::{include_dir, Dir, DirEntry};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::library_registry::LibraryRegistry;
use crate::types::{
    ApplyMode, BrownfieldMode, CompileManifest, Config, ConfigDefaults, DiscoveryMode,
    InvocationOverlay, PolicyEnforcementReport, PromotionStatus, Ref, SurfaceSelectionMode,
};

pub const METACTL_DIRS: &[&str] = &["generated", "state", "history", "private", "cache"];
pub const METACTL_GITIGNORE_ENTRY: &str = "/.metactl/";
pub const LOCAL_CONFIG_GITIGNORE_ENTRY: &str = "/metactl.local.yaml";
const DEFAULT_OPERATION_LOCK_STALE_SECS: u64 = 6 * 60 * 60;
static BUNDLED_STARTER_LIBRARY: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/assets/starter");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ProjectConfigDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brownfield_mode: Option<BrownfieldMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fleet_sync_adopt: Option<FleetSyncAdoptMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discovery_mode: Option<DiscoveryMode>,
    #[serde(
        default,
        alias = "surface_mode",
        skip_serializing_if = "Option::is_none"
    )]
    pub surface_selection_mode: Option<SurfaceSelectionMode>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FleetSyncAdoptMode {
    Patch,
    Refuse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends_profile: Option<String>,
    pub api_version: String,
    pub role: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packs: Vec<String>,
    pub policy: String,
    pub targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub starter_library: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked_projects: Vec<LinkedProjectRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<ProjectConfigDefaults>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PartialProjectConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub starter_library: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub linked_projects: Vec<LinkedProjectRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<ProjectConfigDefaults>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    Local,
    Git,
}

impl Default for SourceType {
    fn default() -> Self {
        Self::Local
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceVisibility {
    Public,
    Private,
}

impl Default for SourceVisibility {
    fn default() -> Self {
        Self::Public
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceLockPublicity {
    Public,
    Private,
}

impl Default for SourceLockPublicity {
    fn default() -> Self {
        Self::Public
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceRecord {
    pub id: String,
    #[serde(rename = "type", default)]
    pub source_type: SourceType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
    #[serde(default)]
    pub visibility: SourceVisibility,
    #[serde(default)]
    pub lock_publicity: SourceLockPublicity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkedProjectRecord {
    pub id: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinkedProjectStatus {
    Ready,
    Disabled,
    MissingPath,
    MissingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkedProject {
    pub id: String,
    pub path: PathBuf,
    pub config_path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub status: LinkedProjectStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedTarget {
    pub target: Ref,
    pub compile_manifest_path: String,
    pub compile_manifest_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_report_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_report_digest: Option<String>,
    pub preferred_apply_mode: ApplyMode,
    pub compiled_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectLock {
    pub api_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_config_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<LockedSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<LockedTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

impl Default for ProjectLock {
    fn default() -> Self {
        Self {
            api_version: crate::types::API_VERSION.to_string(),
            config_digest: None,
            overlay_path: None,
            overlay_digest: None,
            profile_name: None,
            profile_path: None,
            profile_digest: None,
            local_config_digest: None,
            sources: Vec::new(),
            targets: Vec::new(),
            last_query: None,
            updated_at: Some(timestamp_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryEntry {
    pub action: String,
    pub target: String,
    pub status: String,
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ProjectContext {
    pub project_root: PathBuf,
    pub config_path: PathBuf,
    pub raw_config_file: PartialProjectConfig,
    pub config_file: ProjectConfigFile,
    pub active_profile: Option<ActiveProfile>,
    pub local_config_path: Option<PathBuf>,
    pub overlay_path: Option<PathBuf>,
    pub overlay: Option<InvocationOverlay>,
    pub library_roots: Vec<PathBuf>,
    pub registry: Option<LibraryRegistry>,
    pub lock_path: PathBuf,
    pub lock: ProjectLock,
}

/// How the active profile name was chosen for this invocation (CLI/env, project binding, or machine default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileActivationSource {
    Cli,
    ProjectExtends,
    UserDefault,
}

#[derive(Debug, Clone)]
pub struct ActiveProfile {
    pub name: String,
    pub path: PathBuf,
    pub digest: Option<String>,
    pub partial: PartialProjectConfig,
    pub source: ProfileActivationSource,
}

/// Machine-local user settings under the metactl XDG config directory (`config.yaml`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fleet: Option<UserFleetSettings>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserFleetSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_controller: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub controllers: BTreeMap<String, UserFleetController>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserFleetController {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileResolution {
    pub name: Option<String>,
    pub source: Option<ProfileActivationSource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinProfileTemplate {
    pub name: &'static str,
    pub description: &'static str,
    pub profile: PartialProjectConfig,
}

#[derive(Debug, Clone, Default)]
pub struct ConfigOverrides {
    pub role: Option<String>,
    pub policy: Option<String>,
    pub targets: Vec<String>,
}

pub fn bundled_starter_library_root() -> PathBuf {
    bundled_starter_cache_root().join(bundled_starter_library_digest())
}

pub fn ensure_bundled_starter_library_root() -> Result<PathBuf> {
    let root = bundled_starter_library_root();
    let marker = root.join(".metactl-bundled-starter.complete");
    if marker.exists() {
        return Ok(root);
    }

    fs::create_dir_all(&root).with_context(|| format!("create {}", root.display()))?;
    materialize_bundled_starter_dir(&BUNDLED_STARTER_LIBRARY, &root)?;
    atomic_write(&marker, bundled_starter_library_digest().as_bytes())
        .with_context(|| format!("write {}", marker.display()))?;
    Ok(root)
}

fn materialize_bundled_starter_dir(dir: &Dir<'_>, root: &Path) -> Result<()> {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(child) => materialize_bundled_starter_dir(child, root)?,
            DirEntry::File(file) => {
                let path = root.join(file.path());
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("create {}", parent.display()))?;
                }
                atomic_write(&path, file.contents())
                    .with_context(|| format!("write {}", path.display()))?;
            }
        }
    }
    Ok(())
}

fn bundled_starter_cache_root() -> PathBuf {
    metactl_user_config_dir()
        .unwrap_or_else(|| env::temp_dir().join("metactl"))
        .join("cache")
        .join("bundled-starter")
}

fn bundled_starter_library_digest() -> String {
    let mut files = Vec::new();
    collect_bundled_starter_files(&BUNDLED_STARTER_LIBRARY, &mut files);
    digest_starter_files(files)
}

fn digest_starter_files<T: AsRef<[u8]>>(mut files: Vec<(String, T)>) -> String {
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    for (path, contents) in files {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(contents.as_ref());
        hasher.update([0]);
    }
    hex::encode(hasher.finalize())
}

fn filesystem_starter_library_digest(root: &Path) -> Result<String> {
    let mut files = Vec::new();
    collect_filesystem_starter_files(root, root, &mut files)?;
    Ok(digest_starter_files(files))
}

fn collect_filesystem_starter_files(
    root: &Path,
    dir: &Path,
    files: &mut Vec<(String, Vec<u8>)>,
) -> Result<()> {
    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("read {}", dir.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("read {}", dir.display()))?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path
            .file_name()
            .is_some_and(|name| name == ".metactl-bundled-starter.complete")
        {
            continue;
        }
        let metadata = entry
            .metadata()
            .with_context(|| format!("stat {}", path.display()))?;
        if metadata.is_dir() {
            collect_filesystem_starter_files(root, &path, files)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let contents = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
            files.push((relative, contents));
        }
    }
    Ok(())
}

fn collect_bundled_starter_files<'a>(dir: &'a Dir<'a>, files: &mut Vec<(String, &'a [u8])>) {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(child) => collect_bundled_starter_files(child, files),
            DirEntry::File(file) => files.push((
                file.path().to_string_lossy().replace('\\', "/"),
                file.contents(),
            )),
        }
    }
}

pub fn default_project_config() -> ProjectConfigFile {
    ProjectConfigFile {
        extends_profile: None,
        api_version: crate::types::API_VERSION.to_string(),
        role: "builder".to_string(),
        packs: Vec::new(),
        policy: "brownfield-safe-builder".to_string(),
        targets: vec!["codex-cli".to_string()],
        starter_library: Vec::new(),
        sources: Vec::new(),
        linked_projects: Vec::new(),
        defaults: Some(ProjectConfigDefaults {
            brownfield_mode: Some(BrownfieldMode::RefuseDueToConflict),
            fleet_sync_adopt: Some(FleetSyncAdoptMode::Patch),
            discovery_mode: Some(DiscoveryMode::CandidateSearch),
            surface_selection_mode: None,
        }),
        metadata: BTreeMap::new(),
    }
}

pub fn project_config_path(project_root: &Path, override_path: Option<&Path>) -> PathBuf {
    override_path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| project_root.join("metactl.yaml"))
}

pub fn local_config_path(project_root: &Path) -> PathBuf {
    project_root.join("metactl.local.yaml")
}

pub fn load_local_config(project_root: &Path) -> Result<Option<PartialProjectConfig>> {
    let path = local_config_path(project_root);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let config: PartialProjectConfig =
        serde_yaml::from_str(&raw).with_context(|| format!("decode {}", path.display()))?;
    Ok(Some(config))
}

pub fn project_lock_path(project_root: &Path) -> PathBuf {
    project_root.join("metactl.lock.json")
}

pub fn ensure_project_layout(project_root: &Path) -> Result<()> {
    fs::create_dir_all(project_root.join(".metactl"))
        .with_context(|| format!("create {}", project_root.join(".metactl").display()))?;
    for item in METACTL_DIRS {
        let dir = project_root.join(".metactl").join(item);
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    }
    Ok(())
}

pub fn ensure_gitignore_entries(project_root: &Path) -> Result<()> {
    let gitignore = project_root.join(".gitignore");
    let mut contents = if gitignore.exists() {
        fs::read_to_string(&gitignore).with_context(|| format!("read {}", gitignore.display()))?
    } else {
        String::new()
    };
    let mut changed = false;
    for entry in &[METACTL_GITIGNORE_ENTRY, LOCAL_CONFIG_GITIGNORE_ENTRY] {
        if !contents.lines().any(|line| line.trim() == *entry) {
            if !contents.is_empty() && !contents.ends_with('\n') {
                contents.push('\n');
            }
            contents.push_str(entry);
            contents.push('\n');
            changed = true;
        }
    }
    if changed {
        atomic_write(&gitignore, contents.as_bytes())
            .with_context(|| format!("write {}", gitignore.display()))?;
    }
    Ok(())
}

pub fn write_project_config(path: &Path, config: &ProjectConfigFile) -> Result<()> {
    let yaml = serde_yaml::to_string(config).context("serialize metactl.yaml")?;
    atomic_write(path, yaml.as_bytes()).with_context(|| format!("write {}", path.display()))
}

pub fn write_partial_project_config(path: &Path, config: &PartialProjectConfig) -> Result<()> {
    let yaml = serde_yaml::to_string(config).context("serialize metactl.yaml")?;
    atomic_write(path, yaml.as_bytes()).with_context(|| format!("write {}", path.display()))
}

pub fn read_project_config(path: &Path, profile: Option<&str>) -> Result<ProjectConfigFile> {
    let defaults = default_project_config();
    let project = load_partial_project_config(path)?;
    let resolution = resolve_profile_cli_chain(profile, &project);
    let profile_config = load_profile_partial(resolution.name.as_deref())?;
    Ok(merge_project_config(defaults, profile_config, project))
}

fn merge_project_config(
    mut merged: ProjectConfigFile,
    profile: PartialProjectConfig,
    project: PartialProjectConfig,
) -> ProjectConfigFile {
    if let Some(api_version) = profile.api_version {
        merged.api_version = api_version;
    }
    if let Some(role) = profile.role {
        merged.role = role;
    }
    if !profile.packs.is_empty() {
        merged.packs = profile.packs;
    }
    if let Some(policy) = profile.policy {
        merged.policy = policy;
    }
    if !profile.targets.is_empty() {
        merged.targets = profile.targets;
    }
    if !profile.starter_library.is_empty() {
        merged.starter_library = profile.starter_library;
    }
    if !profile.sources.is_empty() {
        merged.sources = profile.sources;
    }
    merge_linked_projects(&mut merged.linked_projects, profile.linked_projects);
    if let Some(defaults) = profile.defaults {
        merged.defaults = Some(merge_config_defaults(merged.defaults, defaults));
    }
    if !profile.metadata.is_empty() {
        merged.metadata.extend(profile.metadata);
    }

    merged.extends_profile = project.extends_profile.clone();
    if let Some(api_version) = project.api_version {
        merged.api_version = api_version;
    }
    if let Some(role) = project.role {
        merged.role = role;
    }
    if !project.packs.is_empty() {
        merged.packs = project.packs;
    }
    if let Some(policy) = project.policy {
        merged.policy = policy;
    }
    if !project.targets.is_empty() {
        merged.targets = project.targets;
    }
    if !project.starter_library.is_empty() {
        merged.starter_library = project.starter_library;
    }
    if !project.sources.is_empty() {
        merged.sources = project.sources;
    }
    merge_linked_projects(&mut merged.linked_projects, project.linked_projects);
    if let Some(defaults) = project.defaults {
        merged.defaults = Some(merge_config_defaults(merged.defaults, defaults));
    }
    if !project.metadata.is_empty() {
        merged.metadata.extend(project.metadata);
    }
    merged
}

fn merge_linked_projects(merged: &mut Vec<LinkedProjectRecord>, overlay: Vec<LinkedProjectRecord>) {
    for project in overlay {
        if let Some(existing) = merged.iter_mut().find(|item| item.id == project.id) {
            *existing = project;
        } else {
            merged.push(project);
        }
    }
}

pub fn discover_linked_projects(
    project_root: &Path,
    config: &ProjectConfigFile,
) -> Vec<LinkedProject> {
    config
        .linked_projects
        .iter()
        .map(|record| {
            let path = resolve_linked_project_path(project_root, &record.path);
            let config_path = project_config_path(&path, None);
            let status = if record.disabled {
                LinkedProjectStatus::Disabled
            } else if !path.exists() {
                LinkedProjectStatus::MissingPath
            } else if !config_path.exists() {
                LinkedProjectStatus::MissingConfig
            } else {
                LinkedProjectStatus::Ready
            };
            LinkedProject {
                id: record.id.clone(),
                path,
                config_path,
                profile: record.profile.clone(),
                status,
            }
        })
        .collect()
}

fn resolve_linked_project_path(project_root: &Path, raw_path: &str) -> PathBuf {
    let path = if raw_path == "~" {
        env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(raw_path))
    } else if let Some(rest) = raw_path.strip_prefix("~/") {
        env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(rest))
            .unwrap_or_else(|| PathBuf::from(raw_path))
    } else {
        PathBuf::from(raw_path)
    };
    if path.is_absolute() {
        path
    } else {
        project_root.join(path)
    }
}

fn merge_config_defaults(
    base: Option<ProjectConfigDefaults>,
    overlay: ProjectConfigDefaults,
) -> ProjectConfigDefaults {
    let mut merged = base.unwrap_or_default();
    if overlay.brownfield_mode.is_some() {
        merged.brownfield_mode = overlay.brownfield_mode;
    }
    if overlay.fleet_sync_adopt.is_some() {
        merged.fleet_sync_adopt = overlay.fleet_sync_adopt;
    }
    if overlay.discovery_mode.is_some() {
        merged.discovery_mode = overlay.discovery_mode;
    }
    if overlay.surface_selection_mode.is_some() {
        merged.surface_selection_mode = overlay.surface_selection_mode;
    }
    merged
}

pub fn load_partial_project_config(path: &Path) -> Result<PartialProjectConfig> {
    if !path.exists() {
        return Err(anyhow!("project config {} does not exist", path.display()));
    }
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_yaml::from_str::<PartialProjectConfig>(&raw)
        .with_context(|| format!("decode {}", path.display()))
}

/// Base directory for machine-local metactl settings: `$XDG_CONFIG_HOME/metactl` or `$HOME/.config/metactl`.
pub fn metactl_user_config_dir() -> Option<PathBuf> {
    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("metactl"));
        }
    }
    let home = env::var_os("HOME")?;
    Some(Path::new(&home).join(".config").join("metactl"))
}

pub fn user_settings_path() -> Option<PathBuf> {
    Some(metactl_user_config_dir()?.join("config.yaml"))
}

pub fn profiles_directory() -> Option<PathBuf> {
    Some(metactl_user_config_dir()?.join("profiles"))
}

pub fn load_user_settings() -> UserSettings {
    let Some(path) = user_settings_path() else {
        return UserSettings::default();
    };
    if !path.exists() {
        return UserSettings::default();
    }
    let Ok(raw) = fs::read_to_string(&path) else {
        return UserSettings::default();
    };
    serde_yaml::from_str::<UserSettings>(&raw).unwrap_or_default()
}

pub fn save_user_settings(settings: &UserSettings) -> Result<()> {
    let Some(path) = user_settings_path() else {
        return Err(anyhow!(
            "HOME (or XDG_CONFIG_HOME) is not set; cannot save metactl user settings"
        ));
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let yaml = serde_yaml::to_string(settings).context("serialize user settings")?;
    atomic_write(&path, yaml.as_bytes()).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// Returns `(profile_id, path)` for each `*.yaml` in the user profiles directory.
pub fn list_user_profiles() -> Result<Vec<(String, PathBuf)>> {
    let Some(dir) = profiles_directory() else {
        return Ok(Vec::new());
    };
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            items.push((stem.to_string(), path));
        }
    }
    items.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(items)
}

pub fn builtin_profile_templates() -> Vec<BuiltinProfileTemplate> {
    vec![
        BuiltinProfileTemplate {
            name: "neutral",
            description: "No implicit runtime target; detect or choose targets explicitly.",
            profile: PartialProjectConfig {
                defaults: Some(ProjectConfigDefaults {
                    brownfield_mode: Some(BrownfieldMode::RefuseDueToConflict),
                    discovery_mode: Some(DiscoveryMode::CandidateSearch),
                    ..ProjectConfigDefaults::default()
                }),
                ..PartialProjectConfig::default()
            },
        },
        BuiltinProfileTemplate {
            name: "multi-agent",
            description: "Project posture for several configured agent runtimes.",
            profile: PartialProjectConfig {
                targets: vec![
                    "codex-cli".to_string(),
                    "claude-code".to_string(),
                    "cursor".to_string(),
                    "gemini-cli".to_string(),
                    "openclaw".to_string(),
                ],
                defaults: Some(ProjectConfigDefaults {
                    brownfield_mode: Some(BrownfieldMode::RefuseDueToConflict),
                    discovery_mode: Some(DiscoveryMode::CandidateSearch),
                    ..ProjectConfigDefaults::default()
                }),
                ..PartialProjectConfig::default()
            },
        },
        BuiltinProfileTemplate {
            name: "agent-ci",
            description: "Automation posture for JSON/no-input validation and explicit apply.",
            profile: PartialProjectConfig {
                defaults: Some(ProjectConfigDefaults {
                    brownfield_mode: Some(BrownfieldMode::RefuseDueToConflict),
                    discovery_mode: Some(DiscoveryMode::CandidateSearch),
                    ..ProjectConfigDefaults::default()
                }),
                metadata: BTreeMap::from([("profile.posture".to_string(), "agent-ci".to_string())]),
                ..PartialProjectConfig::default()
            },
        },
        BuiltinProfileTemplate {
            name: "solo-codex",
            description: "Explicit Codex CLI posture for users who choose that target.",
            profile: PartialProjectConfig {
                targets: vec!["codex-cli".to_string()],
                defaults: Some(ProjectConfigDefaults {
                    brownfield_mode: Some(BrownfieldMode::RefuseDueToConflict),
                    discovery_mode: Some(DiscoveryMode::CandidateSearch),
                    ..ProjectConfigDefaults::default()
                }),
                ..PartialProjectConfig::default()
            },
        },
        BuiltinProfileTemplate {
            name: "private-overlay",
            description:
                "Public skeleton for private-overlay workflows; add private sources locally.",
            profile: PartialProjectConfig {
                defaults: Some(ProjectConfigDefaults {
                    brownfield_mode: Some(BrownfieldMode::RefuseDueToConflict),
                    discovery_mode: Some(DiscoveryMode::CandidateSearch),
                    ..ProjectConfigDefaults::default()
                }),
                metadata: BTreeMap::from([(
                    "profile.posture".to_string(),
                    "private-overlay".to_string(),
                )]),
                ..PartialProjectConfig::default()
            },
        },
    ]
}

fn builtin_profile_template(name: &str) -> Option<PartialProjectConfig> {
    builtin_profile_templates()
        .into_iter()
        .find(|template| template.name == name)
        .map(|template| template.profile)
}

/// Resolve which profile applies: CLI/env > `extends_profile` > user `default_profile`.
pub fn resolve_profile_cli_chain(
    profile_cli: Option<&str>,
    project: &PartialProjectConfig,
) -> ProfileResolution {
    if let Some(name) = profile_cli.filter(|s| !s.is_empty()) {
        return ProfileResolution {
            name: Some(name.to_string()),
            source: Some(ProfileActivationSource::Cli),
        };
    }
    if let Some(name) = project.extends_profile.as_ref().filter(|s| !s.is_empty()) {
        return ProfileResolution {
            name: Some(name.clone()),
            source: Some(ProfileActivationSource::ProjectExtends),
        };
    }
    if let Some(name) = load_user_settings()
        .default_profile
        .as_ref()
        .filter(|s| !s.is_empty())
    {
        return ProfileResolution {
            name: Some(name.clone()),
            source: Some(ProfileActivationSource::UserDefault),
        };
    }
    ProfileResolution {
        name: None,
        source: None,
    }
}

/// Profile selection for `metactl init`: CLI/env, then machine `default_profile` (not project `extends_profile`).
pub fn resolve_profile_name_for_init(profile_cli: Option<&str>) -> ProfileResolution {
    if let Some(name) = profile_cli.filter(|s| !s.is_empty()) {
        return ProfileResolution {
            name: Some(name.to_string()),
            source: Some(ProfileActivationSource::Cli),
        };
    }
    if let Some(name) = load_user_settings()
        .default_profile
        .as_ref()
        .filter(|s| !s.is_empty())
    {
        return ProfileResolution {
            name: Some(name.clone()),
            source: Some(ProfileActivationSource::UserDefault),
        };
    }
    ProfileResolution {
        name: None,
        source: None,
    }
}

pub fn resolve_profile_name(
    profile_cli: Option<&str>,
    project: &PartialProjectConfig,
) -> Option<String> {
    resolve_profile_cli_chain(profile_cli, project).name
}

pub fn profile_path(profile_name: &str) -> Option<PathBuf> {
    Some(profiles_directory()?.join(format!("{profile_name}.yaml")))
}

/// Load optional user profile from the user config profiles directory.
pub fn load_profile_partial(profile: Option<&str>) -> Result<PartialProjectConfig> {
    let Some(profile_name) = profile else {
        return Ok(PartialProjectConfig::default());
    };
    let Some(path) = profile_path(profile_name) else {
        return Ok(builtin_profile_template(profile_name).unwrap_or_default());
    };
    if !path.exists() {
        return Ok(builtin_profile_template(profile_name).unwrap_or_default());
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_yaml::from_str(&raw).with_context(|| format!("decode {}", path.display()))
}

pub fn load_overlay(path: Option<&Path>) -> Result<Option<InvocationOverlay>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
        serde_json::from_str(&raw)
            .map(Some)
            .with_context(|| format!("decode {}", path.display()))
    } else {
        serde_yaml::from_str(&raw)
            .map(Some)
            .with_context(|| format!("decode {}", path.display()))
    }
}

pub fn load_project_context(
    project_root: &Path,
    config_override: Option<&Path>,
    profile: Option<&str>,
    overlay_path: Option<&Path>,
) -> Result<ProjectContext> {
    let config_path = project_config_path(project_root, config_override);
    let raw_config_file = load_partial_project_config(&config_path)?;
    let resolution = resolve_profile_cli_chain(profile, &raw_config_file);
    let active_profile = build_active_profile_from_resolution(&resolution)?;
    let config_file = merge_project_config(
        default_project_config(),
        active_profile
            .as_ref()
            .map(|profile| profile.partial.clone())
            .unwrap_or_default(),
        raw_config_file.clone(),
    );
    let overlay = load_overlay(overlay_path)?;
    let local_cfg_path = local_config_path(project_root);
    let local_config_path_opt = if local_cfg_path.exists() {
        Some(local_cfg_path)
    } else {
        None
    };
    let library_roots = resolve_library_roots(project_root, &config_file)?;
    let registry = load_registry(&library_roots)?;
    let lock_path = project_lock_path(project_root);
    let lock = load_lock(&lock_path)?;
    Ok(ProjectContext {
        project_root: project_root.to_path_buf(),
        config_path,
        raw_config_file,
        config_file,
        active_profile,
        local_config_path: local_config_path_opt,
        overlay_path: overlay_path.map(Path::to_path_buf),
        overlay,
        library_roots,
        registry,
        lock_path,
        lock,
    })
}

fn build_active_profile_from_resolution(
    resolution: &ProfileResolution,
) -> Result<Option<ActiveProfile>> {
    let (Some(name), Some(source)) = (&resolution.name, resolution.source) else {
        return Ok(None);
    };
    let Some(path) = profile_path(name) else {
        return Ok(None);
    };
    let partial = load_profile_partial(Some(name))?;
    let digest = path.exists().then(|| digest_path(&path)).transpose()?;
    Ok(Some(ActiveProfile {
        name: name.clone(),
        path,
        digest,
        partial,
        source,
    }))
}

fn load_registry(library_roots: &[PathBuf]) -> Result<Option<LibraryRegistry>> {
    let existing = library_roots
        .iter()
        .filter(|root| root.exists())
        .cloned()
        .collect::<Vec<_>>();
    if existing.is_empty() {
        return Ok(None);
    }
    Ok(Some(LibraryRegistry::load_from_roots(&existing)?))
}

pub fn resolve_starter_library_roots(
    project_root: &Path,
    starter_library: &[String],
) -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    if !starter_library.is_empty() {
        let mut has_explicit_manifest = false;
        for item in starter_library {
            let path = PathBuf::from(item);
            let root = if path.is_absolute() {
                path
            } else {
                project_root.join(path)
            };
            has_explicit_manifest |= root.join("library.json").exists();
            push_unique_library_root(&mut roots, &mut seen, root);
        }
        if !has_explicit_manifest {
            let bundled = ensure_bundled_starter_library_root()?;
            push_unique_library_root(&mut roots, &mut seen, bundled);
        }
        return Ok(roots);
    }

    let bundled = ensure_bundled_starter_library_root()?;
    push_unique_library_root(&mut roots, &mut seen, bundled);
    Ok(roots)
}

fn is_bundled_starter_equivalent(
    root: &Path,
    bundled_manifest_digest: Option<&str>,
    bundled_library_digest: &str,
) -> bool {
    let Some(bundled_manifest_digest) = bundled_manifest_digest else {
        return false;
    };
    let manifest = root.join("library.json");
    let manifest_matches = manifest.exists()
        && digest_path(&manifest)
            .map(|digest| digest == bundled_manifest_digest)
            .unwrap_or(false);
    manifest_matches
        && filesystem_starter_library_digest(root)
            .map(|digest| digest == bundled_library_digest)
            .unwrap_or(false)
}

fn resolve_library_roots(project_root: &Path, config: &ProjectConfigFile) -> Result<Vec<PathBuf>> {
    let mut roots = resolve_starter_library_roots(project_root, &config.starter_library)?;
    let mut seen = roots
        .iter()
        .map(|root| root.canonicalize().unwrap_or_else(|_| root.clone()))
        .collect::<std::collections::BTreeSet<_>>();
    let bundled_manifest_digest = roots
        .first()
        .and_then(|root| digest_path(&root.join("library.json")).ok());
    let bundled_library_digest = bundled_starter_library_digest();
    for source in &config.sources {
        let root = match source.source_type {
            SourceType::Local => source.path.as_ref().map(PathBuf::from),
            SourceType::Git => Some(
                project_root
                    .join(".metactl")
                    .join("cache")
                    .join("sources")
                    .join(&source.id),
            ),
        };
        if let Some(path) = root {
            let root = if path.is_absolute() {
                path
            } else {
                project_root.join(path)
            };
            if is_bundled_starter_equivalent(
                &root,
                bundled_manifest_digest.as_deref(),
                &bundled_library_digest,
            ) {
                continue;
            }
            push_unique_library_root(&mut roots, &mut seen, root);
        }
    }
    Ok(roots)
}

fn push_unique_library_root(
    roots: &mut Vec<PathBuf>,
    seen: &mut std::collections::BTreeSet<PathBuf>,
    root: PathBuf,
) {
    let key = root.canonicalize().unwrap_or_else(|_| root.clone());
    if seen.insert(key) {
        roots.push(root);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedSource {
    pub id: String,
    #[serde(rename = "type")]
    pub source_type: SourceType,
    pub visibility: SourceVisibility,
    pub lock_publicity: SourceLockPublicity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "ref")]
    pub ref_: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_commit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PrivateSourceLock {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<LockedSource>,
}

pub fn private_source_lock_path(project_root: &Path) -> PathBuf {
    project_root
        .join(".metactl")
        .join("private")
        .join("source-lock.json")
}

pub fn write_private_source_lock(path: &Path, lock: &PrivateSourceLock) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(lock).context("serialize private source lock")?;
    atomic_write(path, &bytes).with_context(|| format!("write {}", path.display()))
}

impl ProjectContext {
    pub fn has_corpus(&self) -> bool {
        self.registry
            .as_ref()
            .is_some_and(|registry| !registry.list_packs().is_empty())
    }

    pub fn selected_target_ids(&self, overrides: &ConfigOverrides) -> Vec<String> {
        if !overrides.targets.is_empty() {
            return overrides.targets.clone();
        }
        if let Some(overlay_target) = self
            .overlay
            .as_ref()
            .and_then(|overlay| overlay.selected_target_override.as_ref())
        {
            return vec![overlay_target.id.clone()];
        }
        self.config_file.targets.clone()
    }

    pub fn effective_config(&self, overrides: &ConfigOverrides) -> Result<Config> {
        let registry = self
            .registry
            .as_ref()
            .ok_or_else(|| anyhow!("no starter library was discovered"))?;

        let role_id = overrides
            .role
            .clone()
            .unwrap_or_else(|| self.config_file.role.clone());
        let role = registry
            .role_by_id(&role_id)
            .ok_or_else(|| anyhow!("role {} was not discovered in starter libraries", role_id))?;

        let policy_id = overrides
            .policy
            .clone()
            .or_else(|| self.config_file.policy.is_empty().then_some(String::new()))
            .unwrap_or_else(|| self.config_file.policy.clone());
        let policy = registry.policy_by_id(&policy_id).ok_or_else(|| {
            anyhow!(
                "policy {} was not discovered in starter libraries",
                policy_id
            )
        })?;

        let target_ids = self.selected_target_ids(overrides);
        if target_ids.is_empty() {
            return Err(anyhow!("project config does not define any targets"));
        }
        let mut targets = Vec::new();
        for target_id in target_ids {
            let target = registry.target_by_id(&target_id).ok_or_else(|| {
                anyhow!(
                    "target {} was not discovered in starter libraries",
                    target_id
                )
            })?;
            targets.push(target.target_ref());
        }

        let pack_ids = if self.config_file.packs.is_empty() {
            role.default_pack_refs
                .iter()
                .map(|item| item.id.clone())
                .collect::<Vec<_>>()
        } else {
            self.config_file.packs.clone()
        };
        let mut packs = Vec::new();
        for pack_id in pack_ids {
            let lookup_id = namespaced_pack_id(&pack_id).unwrap_or(&pack_id);
            let pack = registry.pack_by_id(lookup_id).ok_or_else(|| {
                anyhow!("pack {} was not discovered in starter libraries", pack_id)
            })?;
            packs.push(pack.manifest.pack_ref());
        }

        Ok(Config {
            api_version: self.config_file.api_version.clone(),
            role: role.role_ref(),
            packs,
            policy: policy.policy_ref(),
            targets,
            defaults: self
                .config_file
                .defaults
                .as_ref()
                .map(|defaults| ConfigDefaults {
                    brownfield_mode: defaults.brownfield_mode.clone(),
                    discovery_mode: defaults.discovery_mode.clone(),
                    surface_selection_mode: defaults.surface_selection_mode.clone(),
                }),
            metadata: self.config_file.metadata.clone(),
        })
    }

    pub fn selected_targets(
        &self,
        overrides: &ConfigOverrides,
    ) -> Result<Vec<crate::TargetCapabilityMatrix>> {
        let registry = self
            .registry
            .as_ref()
            .ok_or_else(|| anyhow!("no starter library was discovered"))?;
        let target_ids = self.selected_target_ids(overrides);
        if target_ids.is_empty() {
            return Err(anyhow!("project config does not define any targets"));
        }
        target_ids
            .into_iter()
            .map(|target_id| {
                registry.target_by_id(&target_id).ok_or_else(|| {
                    anyhow!(
                        "target {} was not discovered in starter libraries",
                        target_id
                    )
                })
            })
            .collect()
    }
}

fn namespaced_pack_id(value: &str) -> Option<&str> {
    value
        .split_once('/')
        .and_then(|(_, pack_id)| (!pack_id.is_empty()).then_some(pack_id))
}

pub fn load_lock(path: &Path) -> Result<ProjectLock> {
    if !path.exists() {
        return Ok(ProjectLock::default());
    }
    let raw = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&raw).with_context(|| format!("decode {}", path.display()))
}

pub fn write_lock(path: &Path, lock: &ProjectLock) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(lock).context("serialize metactl.lock.json")?;
    atomic_write(path, &bytes).with_context(|| format!("write {}", path.display()))
}

pub fn write_lock_relaxed(path: &Path, lock: &ProjectLock) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(lock).context("serialize metactl.lock.json")?;
    atomic_write_relaxed(path, &bytes).with_context(|| format!("write {}", path.display()))
}

pub fn compile_manifest_path(project_root: &Path, target: &Ref) -> PathBuf {
    project_root
        .join(".metactl")
        .join("generated")
        .join(&target.id)
        .join("compile.manifest.json")
}

pub fn policy_report_path(project_root: &Path, target: &Ref) -> PathBuf {
    project_root
        .join(".metactl")
        .join("private")
        .join(format!("{}-policy-report.json", target.id))
}

pub fn load_compile_manifest(path: &Path) -> Result<CompileManifest> {
    let raw = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&raw).with_context(|| format!("decode {}", path.display()))
}

pub fn load_policy_report(path: &Path) -> Result<PolicyEnforcementReport> {
    let raw = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&raw).with_context(|| format!("decode {}", path.display()))
}

pub fn write_policy_report(path: &Path, report: &PolicyEnforcementReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(report).context("serialize policy report")?;
    atomic_write(path, &bytes).with_context(|| format!("write {}", path.display()))
}

pub fn write_policy_report_relaxed(path: &Path, report: &PolicyEnforcementReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(report).context("serialize policy report")?;
    atomic_write_relaxed(path, &bytes).with_context(|| format!("write {}", path.display()))
}

pub fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

pub fn digest_path(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(digest_bytes(&bytes))
}

pub fn digest_json<T: Serialize>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value).context("serialize digest input")?;
    Ok(digest_bytes(&bytes))
}

pub fn current_config_digest(context: &ProjectContext) -> Result<String> {
    let bytes = fs::read(&context.config_path)
        .with_context(|| format!("read {}", context.config_path.display()))?;
    Ok(digest_bytes(&bytes))
}

pub fn current_overlay_digest(context: &ProjectContext) -> Result<Option<String>> {
    match context.overlay_path.as_ref() {
        Some(path) => Ok(Some(digest_path(path)?)),
        None => Ok(None),
    }
}

pub fn current_local_config_digest(context: &ProjectContext) -> Result<Option<String>> {
    match context.local_config_path.as_ref() {
        Some(path) if path.exists() => Ok(Some(digest_path(path)?)),
        _ => Ok(None),
    }
}

pub fn lock_is_stale(context: &ProjectContext) -> Result<bool> {
    if context.lock.targets.is_empty() {
        return Ok(false);
    }
    if let Some(expected) = context.lock.config_digest.as_deref() {
        if expected != current_config_digest(context)? {
            return Ok(true);
        }
    }
    if context.lock.overlay_digest != current_overlay_digest(context)? {
        return Ok(true);
    }
    let current_profile_name = context
        .active_profile
        .as_ref()
        .map(|profile| profile.name.clone());
    let current_profile_digest = context
        .active_profile
        .as_ref()
        .and_then(|profile| profile.digest.clone());
    if context.lock.profile_name != current_profile_name {
        return Ok(true);
    }
    if context.lock.profile_digest != current_profile_digest {
        return Ok(true);
    }
    if context.lock.local_config_digest != current_local_config_digest(context)? {
        return Ok(true);
    }
    Ok(false)
}

/// Returns the reason the lock is stale, or None if it is not stale.
pub fn lock_stale_reason(context: &ProjectContext) -> Result<Option<String>> {
    if context.lock.targets.is_empty() {
        return Ok(None);
    }
    if let Some(expected) = context.lock.config_digest.as_deref() {
        if expected != current_config_digest(context)? {
            return Ok(Some("config changed".to_string()));
        }
    }
    if context.lock.overlay_digest != current_overlay_digest(context)? {
        return Ok(Some("overlay changed".to_string()));
    }
    let current_profile_name = context
        .active_profile
        .as_ref()
        .map(|profile| profile.name.clone());
    let current_profile_digest = context
        .active_profile
        .as_ref()
        .and_then(|profile| profile.digest.clone());
    if context.lock.profile_name != current_profile_name {
        return Ok(Some("profile binding changed".to_string()));
    }
    if context.lock.profile_digest != current_profile_digest {
        return Ok(Some("profile changed".to_string()));
    }
    if context.lock.local_config_digest != current_local_config_digest(context)? {
        return Ok(Some("local config changed".to_string()));
    }
    Ok(None)
}

pub fn update_managed_files_index(project_root: &Path) -> Result<()> {
    let state_dir = project_root.join(".metactl").join("state");
    fs::create_dir_all(&state_dir).with_context(|| format!("create {}", state_dir.display()))?;
    let index_path = state_dir.join("managed_files.json");
    let mut managed = BTreeMap::<String, Vec<serde_json::Value>>::new();
    if state_dir.exists() {
        let mut entries = fs::read_dir(&state_dir)
            .with_context(|| format!("read {}", state_dir.display()))?
            .filter_map(|entry| entry.ok().map(|item| item.path()))
            .filter(|path| {
                path.extension().and_then(|ext| ext.to_str()) == Some("json")
                    && path.file_name().and_then(|name| name.to_str()) != Some("managed_files.json")
            })
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            let raw = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
            let json = serde_json::from_slice::<serde_json::Value>(&raw)
                .with_context(|| format!("decode {}", path.display()))?;
            let target = json
                .get("target")
                .and_then(|value| value.get("id"))
                .and_then(|value| value.as_str())
                .unwrap_or("unknown")
                .to_string();
            let paths = json
                .get("outputs")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| {
                            let destination_path = item
                                .get("destination_path")
                                .and_then(|value| value.as_str())
                                .map(ToString::to_string)?;
                            Some(serde_json::json!({
                                "destination_path": destination_path,
                                "pack_ref": item.get("pack_ref").cloned().unwrap_or(serde_json::Value::Null),
                                "surface_id": item.get("surface_id").cloned().unwrap_or(serde_json::Value::Null),
                                "surface_slug": item.get("surface_slug").cloned().unwrap_or(serde_json::Value::Null),
                                "merge_status": item.get("merge_status").cloned().unwrap_or(serde_json::Value::Null),
                                "ownership_token": item.get("ownership_token").cloned().unwrap_or(serde_json::Value::Null),
                            }))
                        })
                        .collect::<Vec<serde_json::Value>>()
                })
                .unwrap_or_default();
            if !paths.is_empty() {
                managed.insert(target, paths);
            }
        }
    }
    let bytes = serde_json::to_vec_pretty(&managed).context("serialize managed_files.json")?;
    atomic_write(&index_path, &bytes).context("write managed_files.json")
}

pub fn append_history_entry(project_root: &Path, entry: &HistoryEntry) -> Result<PathBuf> {
    let history_dir = project_root.join(".metactl").join("history");
    fs::create_dir_all(&history_dir)
        .with_context(|| format!("create {}", history_dir.display()))?;
    let filename = format!(
        "{}-{}-{}.json",
        timestamp_filename(),
        sanitize_filename(&entry.action),
        sanitize_filename(&entry.target)
    );
    let path = history_dir.join(filename);
    let bytes = serde_json::to_vec_pretty(entry).context("serialize history entry")?;
    atomic_write(&path, &bytes).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    atomic_write_with_durability(path, bytes, true)
}

pub fn atomic_write_relaxed(path: &Path, bytes: &[u8]) -> Result<()> {
    atomic_write_with_durability(path, bytes, false)
}

fn atomic_write_with_durability(path: &Path, bytes: &[u8], durable: bool) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp_path = parent.join(format!(".{filename}.tmp-{}-{stamp}", std::process::id()));
    {
        let mut file = File::create(&tmp_path)
            .with_context(|| format!("create temp {}", tmp_path.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("write temp {}", tmp_path.display()))?;
        if durable {
            let _ = file.sync_all();
        }
    }
    #[cfg(windows)]
    if path.exists() {
        let _ = fs::remove_file(path);
    }
    fs::rename(&tmp_path, path)
        .with_context(|| format!("rename {} -> {}", tmp_path.display(), path.display()))?;
    if durable {
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

pub struct OperationLock {
    path: PathBuf,
}

impl OperationLock {
    pub fn acquire(project_root: &Path, command: &str) -> Result<Self> {
        let state_dir = project_root.join(".metactl").join("state");
        fs::create_dir_all(&state_dir)
            .with_context(|| format!("create {}", state_dir.display()))?;
        let path = state_dir.join("operation.lock");
        let payload = format!(
            "pid={}\ncommand={}\nstarted_at={}\n",
            std::process::id(),
            command,
            unix_secs()
        );
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(payload.as_bytes())
                    .with_context(|| format!("write {}", path.display()))?;
                let _ = file.sync_all();
                Ok(Self { path })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = fs::read_to_string(&path).unwrap_or_default();
                let age_secs = operation_lock_age_secs(&path, &existing).unwrap_or_default();
                let stale_after = operation_lock_stale_after_secs();
                if age_secs >= stale_after {
                    Err(anyhow!(
                        "stale metactl operation lock at {}. Another metactl write may have been interrupted.\nNext: inspect the repo, then remove .metactl/state/operation.lock and retry.",
                        path.display()
                    ))
                } else {
                    Err(anyhow!(
                        "another metactl write operation is already active for this project (lock: {}).\nNext: wait for the active command to finish, then retry. If no metactl process is running, inspect the repo before removing .metactl/state/operation.lock.",
                        path.display()
                    ))
                }
            }
            Err(error) => Err(error).with_context(|| format!("create {}", path.display())),
        }
    }
}

impl Drop for OperationLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn operation_lock_stale_after_secs() -> u64 {
    env::var("METACTL_TEST_LOCK_STALE_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_OPERATION_LOCK_STALE_SECS)
}

fn operation_lock_age_secs(path: &Path, contents: &str) -> Option<u64> {
    let now = unix_secs();
    if let Some(started_at) = contents.lines().find_map(|line| {
        line.strip_prefix("started_at=")
            .and_then(|value| value.parse::<u64>().ok())
    }) {
        return Some(now.saturating_sub(started_at));
    }
    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| {
            SystemTime::now()
                .duration_since(modified)
                .ok()
                .map(|duration: Duration| duration.as_secs())
        })
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default()
}

pub fn detect_brownfield_repo(project_root: &Path) -> bool {
    [
        "AGENTS.md",
        "CLAUDE.md",
        "GEMINI.md",
        "OPENCLAW.md",
        ".claude/settings.json",
        ".codex/config.toml",
        ".openclaw/config.json",
    ]
    .iter()
    .any(|item| project_root.join(item).exists())
}

pub fn preferred_apply_mode_for_target(
    target: &crate::TargetCapabilityMatrix,
    requested: Option<ApplyMode>,
) -> ApplyMode {
    requested.unwrap_or_else(|| {
        if target.capabilities.local_scripts {
            ApplyMode::Symlink
        } else {
            ApplyMode::Copy
        }
    })
}

pub fn is_candidate_pack(status: &PromotionStatus) -> bool {
    matches!(status, PromotionStatus::Candidate)
}

fn sanitize_filename(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect()
}

fn timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

fn timestamp_filename() -> String {
    timestamp_string()
}

/// Discover import roots from well-known locations (e.g. `~/.metactl/imports/`).
pub fn discover_import_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = env::var_os("HOME") {
        let imports_dir = Path::new(&home).join(".metactl").join("imports");
        if imports_dir.exists() {
            roots.push(imports_dir);
        }
    }
    roots
}

/// Detect unmanaged brownfield files in the project.
/// Returns a list of detected candidate files/directories that should be adopted
/// via `metactl sync --adopt preview` before syncing.
pub fn detect_brownfield_files(project_root: &Path) -> Vec<String> {
    let managed = managed_brownfield_roots(project_root);
    let mut unmanaged = Vec::new();
    let candidates = vec![
        "AGENTS.md",
        "CLAUDE.md",
        "GEMINI.md",
        ".codex/",
        ".claude/",
        ".cursor/",
    ];
    for candidate in candidates {
        let path = project_root.join(candidate);
        if path.exists() && !managed.contains(candidate) {
            unmanaged.push(candidate.to_string());
        }
    }
    unmanaged
}

fn managed_brownfield_roots(project_root: &Path) -> BTreeSet<String> {
    let index_path = project_root.join(".metactl/state/managed_files.json");
    let Ok(raw) = fs::read(&index_path) else {
        return BTreeSet::new();
    };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&raw) else {
        return BTreeSet::new();
    };
    let mut managed = BTreeSet::new();
    let Some(by_target) = json.as_object() else {
        return managed;
    };
    for outputs in by_target.values() {
        let Some(outputs) = outputs.as_array() else {
            continue;
        };
        for output in outputs {
            let Some(destination_path) = output
                .get("destination_path")
                .and_then(|value| value.as_str())
            else {
                continue;
            };
            if let Some(root) = brownfield_root_for_destination(destination_path) {
                managed.insert(root.to_string());
            }
        }
    }
    managed
}

fn brownfield_root_for_destination(destination_path: &str) -> Option<&'static str> {
    if destination_path == "AGENTS.md" {
        Some("AGENTS.md")
    } else if destination_path == "CLAUDE.md" || destination_path == "CLAUDE.local.md" {
        Some("CLAUDE.md")
    } else if destination_path == "GEMINI.md" || destination_path == "GEMINI.local.md" {
        Some("GEMINI.md")
    } else if destination_path.starts_with(".codex/") {
        Some(".codex/")
    } else if destination_path.starts_with(".claude/") {
        Some(".claude/")
    } else if destination_path.starts_with(".cursor/") {
        Some(".cursor/")
    } else {
        None
    }
}

/// Check if a target supports takeover mode.
/// Reference-based targets (claude-code, gemini-cli) don't support takeover because
/// they use reference indexes instead of inline content.
pub fn target_supports_takeover(target: &crate::TargetCapabilityMatrix) -> bool {
    // Check if the target uses reference-based instruction projection.
    // Reference-based targets don't support takeover mode.
    target.compile_targets.iter().all(|ct| {
        ct.instruction_mode.as_ref() != Some(&crate::InstructionProjectionMode::ReferenceIndex)
    })
}

/// Brownfield adoption playbook hint for error messages.
/// Provides users with a clear three-step strategy when sync refuses due to unmanaged files.
/// Uses ANSI colors for terminal output: green for steps, yellow for commands.
pub fn brownfield_adoption_hint() -> String {
    // ANSI color codes for terminal output
    let green = "\x1b[32m"; // Green for step numbers
    let yellow = "\x1b[33m"; // Yellow for commands
    let bold = "\x1b[1m"; // Bold for section header
    let reset = "\x1b[0m"; // Reset to default

    format!(
        "{bold}Brownfield adoption strategy:{reset}\n\
        {green}1.{reset} Run {yellow}'metactl sync --adopt preview'{reset} to see what would be applied\n\
        {green}2.{reset} Use {yellow}'metactl sync --adopt patch'{reset} to apply with conflict resolution\n\
        {green}3.{reset} For targets that explicitly support takeover:\n\
           {yellow}'metactl sync --adopt takeover'{reset}\n\
        \n\
        {bold}Note: takeover is not supported for targets that use reference-based indexes{reset}",
        green = green,
        yellow = yellow,
        bold = bold,
        reset = reset,
    )
}

/// Strip ANSI escape sequences from a string for machine-readable output (e.g., JSON).
pub fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::new();
    let mut in_escape = false;

    for ch in s.chars() {
        if ch == '\x1b' {
            in_escape = true;
        } else if in_escape && ch == 'm' {
            in_escape = false;
        } else if !in_escape {
            result.push(ch);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_keeps_bundled_starter_out_of_project_yaml() {
        assert!(default_project_config().starter_library.is_empty());
    }

    #[test]
    fn bundled_starter_resolves_without_project_config_paths() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = resolve_starter_library_roots(temp.path(), &[]).expect("starter roots");

        assert_eq!(roots.len(), 1);
        assert!(roots[0].join("library.json").exists());
        assert!(roots[0].join("packs").join("python-refactor.json").exists());
    }

    #[test]
    fn explicit_starter_library_replaces_bundled_default() {
        let temp = tempfile::tempdir().expect("tempdir");
        let explicit = temp.path().join("starter");
        fs::create_dir_all(&explicit).expect("starter dir");
        fs::write(explicit.join("library.json"), b"{}").expect("library manifest");

        let roots = resolve_starter_library_roots(temp.path(), &["starter".to_string()])
            .expect("starter roots");

        assert_eq!(roots, vec![explicit]);
    }

    #[test]
    fn linked_projects_merge_profile_and_project_by_id() {
        let mut profile = PartialProjectConfig::default();
        profile.linked_projects = vec![
            LinkedProjectRecord {
                id: "alpha".to_string(),
                path: "../alpha".to_string(),
                profile: Some("team".to_string()),
                disabled: false,
            },
            LinkedProjectRecord {
                id: "beta".to_string(),
                path: "../beta".to_string(),
                profile: None,
                disabled: false,
            },
        ];
        let mut project = PartialProjectConfig::default();
        project.linked_projects = vec![
            LinkedProjectRecord {
                id: "beta".to_string(),
                path: "../beta-local".to_string(),
                profile: Some("local".to_string()),
                disabled: true,
            },
            LinkedProjectRecord {
                id: "gamma".to_string(),
                path: "../gamma".to_string(),
                profile: None,
                disabled: false,
            },
        ];

        let merged = merge_project_config(default_project_config(), profile, project);

        assert_eq!(
            merged
                .linked_projects
                .iter()
                .map(|item| (
                    item.id.as_str(),
                    item.path.as_str(),
                    item.profile.as_deref(),
                    item.disabled
                ))
                .collect::<Vec<_>>(),
            vec![
                ("alpha", "../alpha", Some("team"), false),
                ("beta", "../beta-local", Some("local"), true),
                ("gamma", "../gamma", None, false),
            ]
        );
    }

    #[test]
    fn linked_project_status_reports_disabled_missing_and_ready_projects() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("fleet");
        let ready = temp.path().join("ready");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(&ready).expect("ready");
        fs::write(
            ready.join("metactl.yaml"),
            "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\n",
        )
        .expect("ready config");

        let config = ProjectConfigFile {
            linked_projects: vec![
                LinkedProjectRecord {
                    id: "ready".to_string(),
                    path: "../ready".to_string(),
                    profile: None,
                    disabled: false,
                },
                LinkedProjectRecord {
                    id: "disabled".to_string(),
                    path: "../ready".to_string(),
                    profile: None,
                    disabled: true,
                },
                LinkedProjectRecord {
                    id: "missing".to_string(),
                    path: "../missing".to_string(),
                    profile: None,
                    disabled: false,
                },
            ],
            ..default_project_config()
        };

        let projects = discover_linked_projects(&root, &config);

        assert_eq!(
            projects
                .iter()
                .map(|item| (item.id.as_str(), item.status))
                .collect::<Vec<_>>(),
            vec![
                ("ready", LinkedProjectStatus::Ready),
                ("disabled", LinkedProjectStatus::Disabled),
                ("missing", LinkedProjectStatus::MissingPath),
            ]
        );
    }

    #[test]
    fn relaxed_atomic_write_preserves_atomic_write_visible_behavior() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("generated").join("compile.manifest.json");

        atomic_write_relaxed(&path, b"first").expect("relaxed write first");
        assert_eq!(fs::read(&path).expect("read first"), b"first");

        atomic_write_relaxed(&path, b"second").expect("relaxed write second");
        assert_eq!(fs::read(&path).expect("read second"), b"second");
        assert!(temp
            .path()
            .join("generated")
            .read_dir()
            .expect("read generated dir")
            .all(|entry| {
                let name = entry
                    .expect("dir entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned();
                !name.contains(".tmp-")
            }));
    }
}
