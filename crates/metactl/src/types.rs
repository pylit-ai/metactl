use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const API_VERSION: &str = "metactl/v2alpha1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RefKind {
    Role,
    Pack,
    Policy,
    Target,
    Artifact,
    Rule,
    Output,
    Overlay,
    KnowledgeSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ref {
    pub kind: RefKind,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl Ref {
    pub fn key(&self) -> String {
        format!(
            "{:?}:{}:{}",
            self.kind,
            self.id,
            self.version.as_deref().unwrap_or_default()
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    SuppressedByPolicy,
    SuppressedByMode,
    UnsupportedTarget,
    IncompatibleRole,
    UntrustedPack,
    RequiresConfirmation,
    CapabilityGap,
    ConflictDetected,
    BrownfieldCollision,
    MissingMetadata,
    ValidationFailed,
    NotFound,
    ZeroMatch,
    DegradedEnforcement,
    Unverifiable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrustTier {
    FirstPartyValidated,
    OrgValidated,
    CandidateQuarantined,
    ExternalUnreviewed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActivationClass {
    Instruction,
    Hook,
    Script,
    Service,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SideEffectClass {
    None,
    FsWrite,
    Network,
    ExternalWrite,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RequestedEnforcementClass {
    Advisory,
    EnforceableLocal,
    EnforceableRemote,
    ExplainOnlyUnverifiable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RealizedEnforcementClass {
    Advisory,
    EnforceableLocal,
    EnforceableRemote,
    ExplainOnlyUnverifiable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnforcementStatus {
    Enforced,
    Degraded,
    NotRealized,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMode {
    None,
    CuratedOnly,
    CandidateSearch,
    Exploratory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrownfieldMode {
    ShadowCompile,
    ReviewDiff,
    PatchMode,
    TakeoverMode,
    RefuseDueToConflict,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntryPoint {
    Cli,
    Daemon,
    MagicwormholeHotkey,
    MagicwormholeTray,
    MagicwormholeNotch,
    MagicwormholeDrop,
    Automation,
    Api,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyMode {
    Normal,
    Restricted,
    LocalOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Instruction,
    Example,
    Hook,
    HookWiring,
    Script,
    Command,
    Rule,
    Plugin,
    Asset,
    Schema,
    Test,
    Subagent,
    KnowledgeSource,
}

impl ResourceKind {
    /// The directory segment this kind uses inside a pack's source layout
    /// (e.g. `packs/<id>/commands/foo.md`). Used by `pack_resource_relative_path`
    /// to strip kind prefixes so target templates do not double-nest them.
    pub fn as_directory_segment(&self) -> &'static str {
        match self {
            ResourceKind::Instruction => "instructions",
            ResourceKind::Example => "examples",
            ResourceKind::Hook => "hooks",
            ResourceKind::HookWiring => "hooks",
            ResourceKind::Script => "scripts",
            ResourceKind::Command => "commands",
            ResourceKind::Rule => "rules",
            ResourceKind::Plugin => "plugins",
            ResourceKind::Asset => "assets",
            ResourceKind::Schema => "schemas",
            ResourceKind::Test => "tests",
            ResourceKind::Subagent => "subagents",
            ResourceKind::KnowledgeSource => "knowledge_sources",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImportEcosystem {
    SkillMd,
    AgentsMd,
    Mcp,
    Custom,
    FirstParty,
    ThirdParty,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PolicySubject {
    Pack,
    Tool,
    Network,
    Filesystem,
    Approval,
    Budget,
    Runtime,
    Mcp,
    Hook,
    Artifact,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PolicyOperator {
    Allow,
    Deny,
    RequireApproval,
    Prefer,
    Readonly,
    BudgetCap,
    QuarantineOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompileTargetKind {
    AgentsMd,
    ClaudeMd,
    OpenclawMd,
    CodexSkill,
    PackResource,
    HookConfig,
    McpConfig,
    RuntimeJson,
    PackExtensionManifest,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedOutputKind {
    InstructionFile,
    SkillFolder,
    ResourceFile,
    HookConfig,
    McpConfig,
    RuntimeJson,
    PackExtensionManifest,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstructionProjectionMode {
    Inline,
    ReferenceIndex,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceSelectionMode {
    #[default]
    Minimal,
    Full,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VisibilityScope {
    #[default]
    Shared,
    Private,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleStatus {
    Active,
    Deprecated,
    Retired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceRelevanceTier {
    AlwaysOn,
    #[default]
    Suppressible,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApplyMode {
    Symlink,
    Copy,
    Patch,
    Takeover,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromotionStatus {
    Candidate,
    Promoted,
    Rejected,
    Retired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttachedArtifactKind {
    File,
    Directory,
    Selection,
    Url,
    Clipboard,
    Capture,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConfigDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brownfield_mode: Option<BrownfieldMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discovery_mode: Option<DiscoveryMode>,
    #[serde(
        default,
        alias = "surface_mode",
        skip_serializing_if = "Option::is_none"
    )]
    pub surface_selection_mode: Option<SurfaceSelectionMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub api_version: String,
    pub role: Ref,
    #[serde(default)]
    pub packs: Vec<Ref>,
    pub policy: Ref,
    pub targets: Vec<Ref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<ConfigDefaults>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoleManifest {
    pub kind: String,
    pub id: String,
    pub version: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intent_tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_pack_refs: Vec<Ref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_policy_ref: Option<Ref>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compatible_targets: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

impl RoleManifest {
    pub fn role_ref(&self) -> Ref {
        Ref {
            kind: RefKind::Role,
            id: self.id.clone(),
            version: Some(self.version.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackResource {
    pub path: String,
    pub kind: ResourceKind,
    #[serde(default = "default_true")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_relevance: Option<SurfaceRelevanceTier>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackImport {
    pub ecosystem: ImportEcosystem,
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeSourceKind {
    FilesystemMarkdown,
    LlmsTxtIndex,
    McpResource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeFreshnessPolicy {
    Ignore,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeReviewStatus {
    Draft,
    Active,
    Stale,
    Superseded,
    Retired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeByteBudget {
    pub max_search_bytes: u64,
    pub max_read_bytes: u64,
    pub max_search_results: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeFreshness {
    pub owner: String,
    pub last_verified: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_after_days: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_digests: Vec<String>,
    pub freshness_policy: KnowledgeFreshnessPolicy,
    pub review_status: KnowledgeReviewStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supersedes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub superseded_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BoundedKnowledgeOperation {
    pub enabled: bool,
    pub max_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProposeUpdateMode {
    Disabled,
    DraftOnly,
    PullRequestOnly,
    RequestOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProposeUpdateOperation {
    pub enabled: bool,
    pub mode: ProposeUpdateMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeSourceOperations {
    pub search: BoundedKnowledgeOperation,
    pub read: BoundedKnowledgeOperation,
    pub freshness: BoundedKnowledgeOperation,
    pub propose_update: ProposeUpdateOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct KnowledgeSourceAdapter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include_globs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_index_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_uri_prefixes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub static_fallback_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeSourceManifest {
    pub kind: String,
    pub id: String,
    pub version: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub source_kind: KnowledgeSourceKind,
    pub uri_scheme: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_targets: Vec<String>,
    pub byte_budget: KnowledgeByteBudget,
    pub trust_tier: TrustTier,
    pub freshness: KnowledgeFreshness,
    pub operations: KnowledgeSourceOperations,
    pub adapter: KnowledgeSourceAdapter,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

impl KnowledgeSourceManifest {
    pub fn knowledge_source_ref(&self) -> Ref {
        Ref {
            kind: RefKind::KnowledgeSource,
            id: self.id.clone(),
            version: Some(self.version.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeRef {
    pub source_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uris: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_targets: Vec<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_budget: Option<KnowledgeRefByteBudget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeRefByteBudget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_read_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_search_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackManifest {
    pub kind: String,
    pub id: String,
    pub version: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub activation_class: ActivationClass,
    pub side_effect_class: SideEffectClass,
    pub trust_tier: TrustTier,
    #[serde(default)]
    pub requires_confirmation: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compatible_roles: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compatible_targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub knowledge_refs: Vec<KnowledgeRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<PackResource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<PackImport>,
    #[serde(default)]
    pub visibility_scope: VisibilityScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<PackLifecycle>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

impl PackManifest {
    pub fn pack_ref(&self) -> Ref {
        Ref {
            kind: RefKind::Pack,
            id: self.id.clone(),
            version: Some(self.version.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicySelectors {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trust_tiers: Vec<TrustTier>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyParameters {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_budget_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyRule {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub requested_enforcement_class: RequestedEnforcementClass,
    pub subject: PolicySubject,
    pub operator: PolicyOperator,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selectors: Option<PolicySelectors>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<PolicyParameters>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyManifest {
    pub kind: String,
    pub id: String,
    pub version: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discovery_mode: Option<DiscoveryMode>,
    pub rules: Vec<PolicyRule>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

impl PolicyManifest {
    pub fn policy_ref(&self) -> Ref {
        Ref {
            kind: RefKind::Policy,
            id: self.id.clone(),
            version: Some(self.version.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TargetCapabilities {
    pub layered_instructions: bool,
    pub skill_folders: bool,
    pub deterministic_hooks: bool,
    pub subagents: bool,
    pub mcp_servers: bool,
    pub tool_allowlists: bool,
    pub approval_policies: bool,
    pub readonly_hints: bool,
    pub local_scripts: bool,
    pub scheduled_tasks: bool,
    pub ui_surfaces: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompileTarget {
    pub output_kind: CompileTargetKind,
    pub path_template: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_kinds: Vec<ResourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_mode: Option<InstructionProjectionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_selection_mode: Option<SurfaceSelectionMode>,
    #[serde(default)]
    pub supports_multi_surface_pack: bool,
    #[serde(default)]
    pub supports_surface_assets: bool,
    #[serde(default)]
    pub supports_surface_scripts: bool,
    #[serde(default)]
    pub supports_surface_frontmatter: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_merge_strategy: Option<SurfaceMergeStrategy>,
    /// Optional frontmatter key/value pairs to wrap around projected instruction
    /// content. Consumed by the kernel projection layer (spec 019 task 4.1) to
    /// emit YAML frontmatter on targets like Cursor's `.mdc` rule files where
    /// directives such as `alwaysApply: true` must precede the body.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub instruction_frontmatter: BTreeMap<String, String>,
    /// Optional adapter that wraps `Command` resource bodies in a per-target
    /// envelope (e.g. injecting Markdown frontmatter for Claude Code's
    /// `.claude/commands/**` slash-command discovery, or wrapping the body in
    /// a TOML prompt object for Gemini CLI). Consumed by the kernel
    /// `emit_pack_resource_outputs` path (spec 019 task 4.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_adapter: Option<CommandAdapter>,
    /// When true on a `pack_resource` compile target whose `resource_kinds`
    /// includes `Instruction`, only the pack's primary instruction (the first
    /// `Instruction` resource declared in the manifest) is emitted. Used by
    /// Gemini CLI extension bundles where each pack contributes a single
    /// `GEMINI.md`. (spec 019 task 4.3.)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub primary_instruction_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CommandAdapter {
    #[serde(default)]
    pub format: CommandAdapterFormat,
    #[serde(default)]
    pub inject_description: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CommandAdapterFormat {
    #[default]
    Markdown,
    Toml,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TargetCapabilityMatrix {
    pub kind: String,
    pub target_id: String,
    pub version: String,
    pub title: String,
    pub capabilities: TargetCapabilities,
    #[serde(default)]
    pub compile_targets: Vec<CompileTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_template: Option<RuntimeTemplateRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_projection: Option<TargetLocalProjection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apply_modes: Vec<ApplyMode>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

/// A pointer to a runtime-config template file living under the library root.
///
/// Used by targets that need to emit a per-tool runtime config file (e.g.
/// `.claude/settings.json`, `.openclaw/config.json`) without the kernel knowing
/// the target's name. The kernel reads `path`, substitutes `{{token}}`
/// placeholders from policy / target / resolve-graph context, and writes the
/// result to `destination_path` as a `GeneratedOutput` of `output_kind`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeTemplateRef {
    /// Path relative to the library root (e.g. `targets/templates/claude-code-settings.json.tmpl`).
    pub path: String,
    /// Format hint, currently one of `"json"` or `"toml"`. Used only for human
    /// diagnostics.
    pub format: String,
    /// Destination path inside the target project where the expanded template
    /// is written.
    pub destination_path: String,
    /// Optional override for the emitted output kind; defaults to
    /// `GeneratedOutputKind::RuntimeJson` when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_kind: Option<CompileTargetKind>,
}

impl TargetCapabilityMatrix {
    pub fn target_ref(&self) -> Ref {
        Ref {
            kind: RefKind::Target,
            id: self.target_id.clone(),
            version: Some(self.version.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OverlayTask {
    pub summary: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SelectedProject {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AttachedArtifact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub kind: AttachedArtifactKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InvocationOverlay {
    pub entrypoint: EntryPoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<OverlayTask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_project: Option<SelectedProject>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attached_artifacts: Vec<AttachedArtifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub privacy_mode: Option<PrivacyMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_budget_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_target_override: Option<Ref>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub temporary_approvals: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_pack_hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProvenanceReview {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promotion_status: Option<PromotionStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProvenanceEnvelope {
    pub api_version: String,
    pub subject_ref: Ref,
    pub digest: String,
    pub origin: String,
    pub imported_from_ecosystem: ImportEcosystem,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ProvenanceReview>,
    #[serde(default)]
    pub attestation_refs: Vec<String>,
    #[serde(default)]
    pub validation_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchMatch {
    pub pack_ref: Ref,
    pub score: f64,
    pub why: String,
    pub trust_tier: TrustTier,
    pub requires_confirmation: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_ref: Option<Ref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_evidence: Option<SearchMatchEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<PackLifecycle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SuppressedRef {
    pub pack_ref: Ref,
    pub reason_code: ReasonCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchResult {
    pub api_version: String,
    pub query: String,
    pub discovery_mode: DiscoveryMode,
    pub matches: Vec<SearchMatch>,
    #[serde(default)]
    pub suppressed: Vec<SuppressedRef>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchMatchEvidence {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_resource_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_terms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackLifecycle {
    pub status: LifecycleStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement_pack_ref: Option<Ref>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verified_targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_verified_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SurfaceSelectionDecision {
    pub pack_ref: Ref,
    pub surface_id: String,
    pub surface_slug: String,
    pub relevance_tier: SurfaceRelevanceTier,
    pub emitted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<ReasonCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_resource_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapabilityGap {
    pub feature: String,
    pub reason_code: ReasonCode,
    #[serde(default)]
    pub affected_refs: Vec<Ref>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResolveGraph {
    pub api_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_config_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_digest: Option<String>,
    pub role: Ref,
    pub selected_target: Ref,
    #[serde(default)]
    pub requested_pack_refs: Vec<Ref>,
    #[serde(default)]
    pub activated_pack_refs: Vec<Ref>,
    #[serde(default)]
    pub suppressed_packs: Vec<SuppressedRef>,
    #[serde(default)]
    pub applied_policies: Vec<Ref>,
    #[serde(default)]
    pub capability_gaps: Vec<CapabilityGap>,
    #[serde(default)]
    pub provenance_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brownfield_mode: Option<BrownfieldMode>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pack_visibility: BTreeMap<String, VisibilityScope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExplanationReason {
    pub subject_ref: Ref,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SuppressedSubject {
    pub subject_ref: Ref,
    pub reason_code: ReasonCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExplainResult {
    pub api_version: String,
    pub summary: String,
    #[serde(default)]
    pub what_is_active: Vec<String>,
    #[serde(default)]
    pub why_it_is_active: Vec<ExplanationReason>,
    #[serde(default)]
    pub what_was_suppressed: Vec<SuppressedSubject>,
    #[serde(default)]
    pub unknown_or_unsupported: Vec<String>,
    pub resolve_graph: ResolveGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneratedOutput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_path: Option<String>,
    pub kind: GeneratedOutputKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_mode: Option<InstructionProjectionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack_ref: Option<Ref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_resource_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_status: Option<SurfaceMergeStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degradation_codes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ownership_token: Option<String>,
    #[serde(default = "default_true")]
    pub managed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompileManifest {
    pub api_version: String,
    pub target: Ref,
    pub generated_outputs: Vec<GeneratedOutput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_selection_mode: Option<SurfaceSelectionMode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surface_selection: Vec<SurfaceSelectionDecision>,
    #[serde(default)]
    pub apply_modes_supported: Vec<ApplyMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brownfield_mode: Option<BrownfieldMode>,
    #[serde(default)]
    pub degradations: Vec<CapabilityGap>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyRuleReport {
    pub rule_id: String,
    pub requested_enforcement_class: RequestedEnforcementClass,
    pub realized_enforcement_class: RealizedEnforcementClass,
    pub status: EnforcementStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enforcement_surface: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default)]
    pub affected_refs: Vec<Ref>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyEnforcementReport {
    pub api_version: String,
    pub target: Ref,
    pub rules: Vec<PolicyRuleReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidationCheck {
    pub id: String,
    pub status: ValidationStatus,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_ref: Option<Ref>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidationReport {
    pub api_version: String,
    pub subject_ref: Ref,
    pub status: ValidationStatus,
    pub checks: Vec<ValidationCheck>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchParams {
    pub query: String,
    pub config: Config,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay: Option<InvocationOverlay>,
    #[serde(default)]
    pub candidate_packs: Vec<PackManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResolveParams {
    pub config: Config,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay: Option<InvocationOverlay>,
    #[serde(default)]
    pub available_targets: Vec<TargetCapabilityMatrix>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Vec<ProvenanceEnvelope>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExplainParams {
    pub resolve_graph: ResolveGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompileParams {
    pub resolve_graph: ResolveGraph,
    pub target_capability: TargetCapabilityMatrix,
    pub apply_mode: ApplyMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_selection_mode: Option<SurfaceSelectionMode>,
    #[serde(default)]
    pub emit_policy_report: bool,
    #[serde(default = "default_true")]
    pub durable_staging: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidateParams {
    pub subject_ref: Ref,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolve_graph: Option<ResolveGraph>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile_manifest: Option<CompileManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_enforcement_report: Option<PolicyEnforcementReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompileResult {
    pub compile_manifest: CompileManifest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_enforcement_report: Option<PolicyEnforcementReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApplyConflict {
    pub destination_path: String,
    pub reason_code: ReasonCode,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApplyReport {
    pub target: Ref,
    #[serde(default)]
    pub applied_paths: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<ApplyConflict>,
    pub state_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RevertReport {
    pub target: Ref,
    #[serde(default)]
    pub reverted_paths: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<ApplyConflict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_path: Option<String>,
}

pub fn default_true() -> bool {
    true
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceMergeStrategy {
    None,
    Optional,
    Required,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceMergeStatus {
    Separate,
    Merged,
    Suppressed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalProjectionSupport {
    Exact,
    Degraded,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TargetLocalProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_surface: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_surface: Option<String>,
    pub support: LocalProjectionSupport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gitignore_pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precedence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InputLayerProvenance {
    pub layer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExplanationCertificate {
    pub subject: String,
    pub premises: Vec<String>,
    pub evidence: Vec<String>,
    pub conclusion: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degraded: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceEntry {
    pub name: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}
