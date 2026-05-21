use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, IsTerminal, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use metactl::project::{
    append_history_entry, atomic_write, brownfield_adoption_hint, builtin_profile_templates,
    bundled_starter_library_root, compile_manifest_path, current_config_digest,
    current_local_config_digest, current_overlay_digest, default_project_config,
    detect_brownfield_repo, digest_path, ensure_bundled_starter_library_root,
    ensure_gitignore_entries, ensure_project_layout, is_candidate_pack, list_user_profiles,
    load_compile_manifest, load_lock, load_partial_project_config, load_policy_report,
    load_profile_partial, load_project_context, load_user_settings, metactl_user_config_dir,
    policy_report_path, preferred_apply_mode_for_target, private_source_lock_path, profile_path,
    profiles_directory, project_config_path, project_lock_path, resolve_profile_name_for_init,
    resolve_starter_library_roots, save_user_settings, strip_ansi_codes, target_supports_takeover,
    update_managed_files_index, user_settings_path, write_lock, write_partial_project_config,
    write_policy_report, write_private_source_lock, write_project_config, ConfigOverrides,
    FleetSyncAdoptMode, HistoryEntry, LinkedProject, LinkedProjectStatus, LockedSource,
    LockedTarget, OperationLock, PrivateSourceLock, ProfileActivationSource, ProjectConfigDefaults,
    ProjectConfigFile, ProjectLock, SourceLockPublicity, SourceRecord, SourceType,
    SourceVisibility, UserFleetController, UserFleetSettings,
};
use metactl::surface_usage::{
    self, SurfaceLifecycleMode, SurfaceOverrideAction, SurfaceRebuildTrigger, SurfaceReport,
};
use metactl::{
    ApplyMode, ApplyReport, BrownfieldMode, CompileManifest, CompileParams, DiscoveryMode,
    ExplainParams, ExplainResult, LibraryRegistry, MetactlKernel, PluginExportOptions, PluginTier,
    PluginVerifyOptions, ReferenceKernel, ResolveParams, SearchParams, SearchResult,
    SurfaceMergeStrategy, TargetCapabilityMatrix, ValidateParams, ValidationReport,
    ValidationStatus, API_VERSION,
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

const EXIT_SUCCESS: u8 = 0;
const EXIT_INTERNAL: u8 = 1;
const EXIT_STATE: u8 = 10;
const EXIT_STALE_LOCK: u8 = 11;
const EXIT_CONFLICT: u8 = 12;
const EXIT_VALIDATION: u8 = 13;

const CODEX_SKILL_SCOPE_NOTE: &str = "Codex repo-local skills under .codex/skills are visible to Codex sessions opened in that repository. User-global Personal skills live under ~/.codex/skills.";
const CODEX_FLEET_SCOPE_NOTE: &str = "Fleet sync updates repo-local .codex/skills in linked projects; it does not install user-global Personal skills under ~/.codex/skills.";
const AGENT_ARTIFACT_POLICY_METADATA_KEY: &str = "agent_artifact_policy";
const AGENT_ARTIFACT_STEWARDSHIP_PACK: &str = "agentic-artifact-forge";

const WORKFLOW_HELP: &str = "\
Quick start:
  metactl setup                  # human-friendly setup
  metactl setup --plan            # inspect equivalent commands without writing
  metactl setup -t codex-cli --artifact-policy portable-first -y
  metactl init -t claude-code        # scaffold a project for Claude Code
  metactl init -t all                # scaffold for every starter-supported target
  metactl init --detect              # detect targets from existing repo surfaces
  metactl profile set-default NAME   # machine default profile for init when no --profile
  metactl init --bind-profile        # record the active machine default in metactl.yaml
  metactl preview                    # compile and preview generated changes without applying
  metactl use python-refactor        # resolve, add, and sync a pack in one step
  metactl add python-refactor        # import a pack from the library
  metactl add <pack> --sync          # add (or already added) then sync in one step
  metactl demo create                # create a disposable brownfield sandbox
  metactl target add cursor          # add another target without editing YAML
  metactl sync                       # compile + apply in one command
  metactl status                     # see what is configured and applied
  metactl ignore install             # hide generated agent surfaces from local git status

Common workflow:
  metactl init -t codex-cli  Create project config and layout (or use a default profile)
  metactl use <pack>  Resolve, add, and sync a pack in one step
  metactl add <pack>  Import packs from the starter library
  metactl sync        Compile + apply all targets in one step
  metactl status      Show project config, targets, and readiness
  metactl doctor      Run health checks
  metactl revert      Remove applied outputs

Expert primitives:
  metactl search \"python refactor\"
  metactl source add <path>          # infer source id, or pass <name> <path>
  metactl explain
  metactl compile
  metactl apply
  metactl validate

Exit codes:
  0  success or warnings
  10 project state/config incomplete
  11 stale lock or stale staged state
  12 brownfield conflict or explicit safety refusal
  13 validation failure or drift";

#[derive(Debug, Parser)]
#[command(
    name = "metactl",
    version,
    about = "Human-first and agent-safe CLI for the metactl kernel",
    after_help = WORKFLOW_HELP
)]
struct Cli {
    /// Emit machine-readable JSON instead of human-friendly output
    #[arg(long, global = true)]
    json: bool,
    /// Disable all interactive prompts (safe for CI / automation)
    #[arg(long, global = true)]
    no_input: bool,
    /// Agent-safe mode: implies --json --no-input and emits stable error fields
    #[arg(long, global = true)]
    agent: bool,
    /// Auto-confirm destructive actions without prompting
    #[arg(long, short = 'y', global = true)]
    yes: bool,
    /// Path to the project root (default: current directory)
    #[arg(long, global = true, value_name = "PATH")]
    project: Option<PathBuf>,
    /// Merge defaults from `$XDG_CONFIG_HOME/metactl/profiles/<PROFILE>.yaml` (or `~/.config/...`; also `METACTL_PROFILE`). Overrides `extends_profile` and machine `default_profile` in `config.yaml`.
    #[arg(long, global = true, env = "METACTL_PROFILE")]
    profile: Option<String>,
    /// Override the config file path (default: metactl.yaml)
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    /// Overlay file for per-invocation context overrides
    #[arg(long, global = true, value_name = "PATH")]
    overlay: Option<PathBuf>,
    /// Show additional detail (surface info, resolve graphs, etc.)
    #[arg(long, short = 'v', global = true)]
    verbose: bool,
    /// Suppress all human output (exit code only)
    #[arg(long, short = 'q', global = true)]
    quiet: bool,
    #[command(subcommand)]
    command: Commands,
}

impl Cli {
    fn machine_output(&self) -> bool {
        self.json || self.agent
    }

    fn no_input_enabled(&self) -> bool {
        self.no_input || self.agent
    }
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Create metactl.yaml, .metactl/, and starter layout in the project
    Init(InitArgs),
    /// Guided first-run setup with plan-first and agent-safe paths
    Setup(SetupArgs),
    /// Manage the user-private library lifecycle
    Library(LibraryArgs),
    /// Import, export, and verify portable Agent Skill folders as metactl packs
    Pack(PackArgs),
    /// Manage repo-local and user-global Codex Agent Skill visibility
    Skills(SkillsArgs),
    /// Project packs into local runtime plugin marketplace bundles
    Plugin(PluginArgs),
    /// Create explicit public example or sanitized export records
    Export(ExportArgs),
    /// Create and remove disposable brownfield demo sandboxes
    Demo(DemoArgs),
    /// Run the public/private boundary scanner for this project
    CheckPublicBoundary,
    /// Link the current project to an explicit profile
    Project(ProjectArgs),
    /// Activate a pack in the current project (resolve, add, and sync in one step)
    Use(UseArgs),
    /// Add packs from the starter library to the project config
    Add(AddArgs),
    /// Remove packs from the project config
    Remove(RemoveArgs),
    /// List, add, or remove configured targets
    Target(TargetArgs),
    /// Preview and apply explicit sync across linked local projects
    Fleet(FleetArgs),
    /// Show project config, targets, and sync readiness at a glance
    Status(StatusArgs),
    /// List roles, packs, policies, or targets from the library or project
    List(ListArgs),
    /// Search the pack corpus for a natural-language or keyword query
    Search(SearchArgs),
    /// Rebuild and inspect local surface usage stats
    Stats(StatsArgs),
    /// Report and override automatic command/skill surface recommendations
    Surface(SurfaceArgs),
    /// Install, inspect, and run report-only background surface refreshes
    Background(BackgroundArgs),
    /// Show why packs and targets were selected for the current config
    Explain(ExplainArgs),
    /// Alias for `metactl sync --preview`
    Preview(SyncArgs),
    /// Compile + apply all targets in one step (the main workflow command)
    Sync(SyncArgs),
    /// Resolve and compile staged outputs under .metactl/generated/ (use `--apply` to materialize)
    Compile(CompileArgs),
    /// Materialize staged outputs into the repo (symlink, copy, patch, …)
    Apply(ApplyArgs),
    /// Remove applied outputs tracked for a target (or all targets)
    Revert(RevertArgs),
    /// Check staged vs applied outputs, policy, and drift for a target
    Validate(ValidateCmdArgs),
    /// Alias for validate, with v1 strict-check wording
    Check(ValidateCmdArgs),
    /// Run quick health checks (config, lock, starter library, …)
    Doctor(DoctorArgs),
    /// Audit source privacy and leak posture
    Audit(AuditArgs),
    /// Manage ignore files for generated agent surfaces
    Ignore(IgnoreArgs),
    /// Manage sync hooks (post-checkout, post-merge)
    Hook(HookArgs),
    /// Manage pack sources (local paths and import roots)
    Source(SourceArgs),
    /// Manage machine-local profiles and default profile selection
    Profile(ProfileArgs),
    /// Print CLI and kernel API version
    Version,
}

#[derive(Debug, Args)]
struct HookArgs {
    #[command(subcommand)]
    command: HookCommand,
}

#[derive(Debug, Args)]
struct IgnoreArgs {
    #[command(subcommand)]
    command: Option<IgnoreCommand>,
}

#[derive(Debug, Subcommand)]
enum IgnoreCommand {
    /// Show installed ignore posture
    Status(IgnoreStatusArgs),
    /// Install managed ignore blocks
    Install(IgnoreInstallArgs),
    /// Plan or apply generated-surface ignore repair
    Fix(IgnoreFixArgs),
}

#[derive(Debug, Args)]
struct IgnoreStatusArgs {
    /// Targets to inspect (`all` expands to every starter-supported agent target)
    #[arg(long, short = 't')]
    target: Vec<String>,
    /// Ignore scopes to inspect
    #[arg(long, value_enum, default_value = "both")]
    scope: IgnoreScopeArg,
}

#[derive(Debug, Args)]
struct IgnoreInstallArgs {
    /// Where to install ignore rules: local writes .git/info/exclude; repo writes .gitignore plus agent allowlists
    #[arg(long, value_enum, default_value = "local")]
    scope: IgnoreScopeArg,
    /// Targets to protect (`all` expands to every starter-supported agent target)
    #[arg(long, short = 't')]
    target: Vec<String>,
    /// Also ignore metactl.lock.json (useful for profile-bound local repos; off by default because some teams may want to review lock changes)
    #[arg(long)]
    include_lock: bool,
    /// Also include explicit private source cache and private source lock patterns
    #[arg(long)]
    include_private_sources: bool,
}

#[derive(Debug, Args)]
struct IgnoreFixArgs {
    /// Show the repair plan without writing files or changing the Git index
    #[arg(long)]
    plan: bool,
    /// Scope to repair
    #[arg(long, value_enum, default_value = "both")]
    scope: IgnoreScopeArg,
    /// Targets to protect (`all` expands to every starter-supported agent target)
    #[arg(long, short = 't')]
    target: Vec<String>,
    /// Also ignore metactl.lock.json
    #[arg(long)]
    include_lock: bool,
    /// Also include explicit private source cache and private source lock patterns
    #[arg(long)]
    include_private_sources: bool,
    /// Remove generated roots from the Git index while leaving files on disk
    #[arg(long)]
    untrack_generated: bool,
    /// Confirm mutating repair actions
    #[arg(long, short = 'y')]
    yes: bool,
}

#[derive(Debug, Args)]
struct AuditArgs {
    #[command(subcommand)]
    command: Option<AuditCommand>,
}

#[derive(Debug, Subcommand)]
enum AuditCommand {
    /// Audit private source caches and locks for leak risks
    Sources,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum IgnoreScopeArg {
    Local,
    Repo,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ArtifactPolicyArg {
    Off,
    RepoOnly,
    PortableFirst,
}

impl ArtifactPolicyArg {
    fn as_str(self) -> &'static str {
        match self {
            ArtifactPolicyArg::Off => "off",
            ArtifactPolicyArg::RepoOnly => "repo-only",
            ArtifactPolicyArg::PortableFirst => "portable-first",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            ArtifactPolicyArg::Off => "do not add agent artifact stewardship",
            ArtifactPolicyArg::RepoOnly => "record repo-only agent artifact stewardship",
            ArtifactPolicyArg::PortableFirst => {
                "add portable agent artifact stewardship with the agentic-artifact-forge pack"
            }
        }
    }
}

#[derive(Debug, Subcommand)]
enum HookCommand {
    /// Install git hooks for automatic sync
    Install(HookInstallArgs),
    /// Show installed hook status
    Status,
}

#[derive(Debug, Args)]
struct HookInstallArgs {
    /// Hooks to install (default: post-checkout, post-merge)
    #[arg(long)]
    hooks: Vec<String>,
}

#[derive(Debug, Args)]
struct SourceArgs {
    #[command(subcommand)]
    command: Option<SourceCommand>,
}

#[derive(Debug, Args)]
struct FleetArgs {
    #[command(subcommand)]
    command: Option<FleetCommand>,
}

#[derive(Debug, Subcommand)]
enum FleetCommand {
    /// List linked projects and discovery status
    List,
    /// Show linked project sync readiness
    Status(FleetStatusArgs),
    /// Preview by default or explicitly apply sync across linked projects
    Sync(FleetSyncArgs),
    /// Manage the machine-local default Fleet controller pointer
    Controller(FleetControllerArgs),
}

#[derive(Debug, Args)]
struct FleetControllerArgs {
    #[command(subcommand)]
    command: FleetControllerCommand,
}

#[derive(Debug, Subcommand)]
enum FleetControllerCommand {
    /// Create and select a Fleet controller project
    Init {
        /// Controller id stored in user settings
        name: String,
        /// Path to create; defaults to ~/.config/metactl/fleet/<name>
        #[arg(long, value_name = "PATH")]
        path: Option<PathBuf>,
        /// Replace an existing controller metactl.yaml and README.md
        #[arg(long)]
        force: bool,
    },
    /// Show the resolved Fleet controller
    Show,
    /// List configured Fleet controllers
    List,
    /// Set and select a machine-local Fleet controller pointer
    Set {
        /// Controller id stored in user settings
        name: String,
        /// Path to the controller project containing linked_projects
        path: PathBuf,
    },
    /// Clear the selected machine-local default Fleet controller
    ClearDefault,
}

#[derive(Debug, Args)]
struct FleetStatusArgs {
    /// Limit to linked project id(s)
    #[arg(long = "id")]
    ids: Vec<String>,
    /// Include disabled projects in output
    #[arg(long)]
    include_disabled: bool,
}

#[derive(Debug, Args)]
struct FleetSyncArgs {
    /// Limit to linked project id(s)
    #[arg(long = "id")]
    ids: Vec<String>,
    /// Preview selected projects without writing files (default)
    #[arg(long)]
    preview: bool,
    /// Apply sync to selected projects; requires global --yes --no-input
    #[arg(long, conflicts_with = "preview")]
    apply: bool,
    /// Include disabled projects when explicitly selected
    #[arg(long)]
    include_disabled: bool,
    /// Allow applying in projects with dirty Git status
    #[arg(long)]
    allow_dirty: bool,
}

#[derive(Debug, Subcommand)]
enum SourceCommand {
    /// List configured sources
    List,
    /// Add a named local or Git source
    Add(SourceAddArgs),
    /// Sync and validate a configured source
    Sync(SourceSyncArgs),
    /// Remove a configured source
    Remove(SourceRemoveArgs),
}

#[derive(Debug, Args)]
struct SourceAddArgs {
    /// Name for this source, or the location when LOCATION is omitted
    #[arg(value_name = "NAME_OR_LOCATION")]
    name: Option<String>,
    /// Path or Git URL to the source root
    #[arg(value_name = "LOCATION")]
    location: Option<String>,
    /// Source type; inferred when omitted
    #[arg(long = "type", value_enum)]
    source_type: Option<SourceTypeArg>,
    /// Git ref, tag, branch, or commit to resolve
    #[arg(long = "ref")]
    ref_: Option<String>,
    /// Allow a Git source without an explicit ref
    #[arg(long)]
    allow_floating_ref: bool,
    /// Mark this source private
    #[arg(long)]
    private: bool,
    /// Where resolved source details may be written
    #[arg(long, value_enum, default_value = "public")]
    lock_publicity: SourceLockPublicityArg,
}

#[derive(Debug, Args)]
struct SourceSyncArgs {
    /// Source name to sync
    name: Option<String>,
    /// Replace an existing Git source cache
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Args)]
struct SourceRemoveArgs {
    /// Source name to remove
    name: String,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SourceTypeArg {
    Local,
    Git,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SourceLockPublicityArg {
    Public,
    Private,
}

#[derive(Debug, Args)]
struct InitArgs {
    /// Target runtimes to configure (e.g. claude-code, codex-cli, cursor, gemini-cli, openclaw)
    #[arg(long, short = 't')]
    target: Vec<String>,
    /// Role to assign (e.g. builder, reviewer, release-manager)
    #[arg(long, short = 'r')]
    role: Option<String>,
    /// Policy to apply (e.g. brownfield-safe-builder, release-policy)
    #[arg(long, short = 'p')]
    policy: Option<String>,
    /// Path(s) to starter library roots
    #[arg(long = "starter-library")]
    starter_library: Vec<PathBuf>,
    /// Init mode: greenfield (clean) or brownfield-auto-detect (existing repo)
    #[arg(long, value_enum, default_value = "brownfield-auto-detect")]
    mode: InitMode,
    /// Force target detection from existing repo surfaces
    #[arg(long)]
    detect: bool,
    /// Record the active machine-default profile in `metactl.yaml` as `extends_profile` (use this when the repo should intentionally track that profile)
    #[arg(long)]
    bind_profile: bool,
}

#[derive(Debug, Args)]
struct SetupArgs {
    /// Show the setup plan without writing project state
    #[arg(long)]
    plan: bool,
    /// Target runtimes to configure (e.g. codex-cli, claude-code, cursor, gemini-cli, all)
    #[arg(long, short = 't')]
    target: Vec<String>,
    /// Named profile template to use in generated guidance
    #[arg(long)]
    profile_template: Option<String>,
    /// Record the active machine-default profile in metactl.yaml as extends_profile
    #[arg(long)]
    bind_profile: bool,
    /// Agent artifact stewardship policy for skills, rules, commands, prompts, and workflows
    #[arg(long, value_enum)]
    artifact_policy: Option<ArtifactPolicyArg>,
    /// Starter library or source roots to make explicit in the setup command
    #[arg(long = "source", value_name = "PATH")]
    source: Vec<PathBuf>,
    /// Ignore scope to recommend after setup
    #[arg(long, value_enum, default_value = "both")]
    ignore_scope: IgnoreScopeArg,
    /// Include private source ignore patterns in the recommended ignore repair command
    #[arg(long)]
    include_private_sources: bool,
    /// Include metactl.lock.json in the recommended ignore repair command
    #[arg(long)]
    include_lock: bool,
    /// Install the report-only background surface refresh after setup writes project state
    #[arg(long)]
    install_background: bool,
    /// Omit background refresh recommendations from setup plan output
    #[arg(long)]
    no_background: bool,
    /// Confirm non-interactive setup writes
    #[arg(long, short = 'y')]
    yes: bool,
}

#[derive(Debug, Args)]
struct DemoArgs {
    #[command(subcommand)]
    command: Option<DemoCommand>,
}

#[derive(Debug, Subcommand)]
enum DemoCommand {
    /// Create a disposable brownfield sandbox and initialize metactl inside it
    Create(DemoCreateArgs),
    /// List disposable demo sandboxes created by metactl
    List(DemoListArgs),
    /// Print the path to a disposable demo sandbox
    Path(DemoPathArgs),
    /// Recreate a disposable demo sandbox from scratch
    Reset(DemoCreateArgs),
    /// Remove a disposable demo sandbox after verifying its sentinel manifest
    Destroy(DemoDestroyArgs),
}

#[derive(Debug, Args)]
struct DemoCreateArgs {
    /// Demo name under the metactl demo home
    #[arg(long, default_value = "metactl-demo")]
    name: String,
    /// Explicit demo root path (defaults to the metactl demo home plus --name)
    #[arg(long, value_name = "PATH")]
    path: Option<PathBuf>,
    /// Target runtime to initialize inside the sandbox
    #[arg(long, short = 't', default_value = "codex-cli")]
    target: String,
    /// Run `metactl sync --preview` after initialization
    #[arg(long)]
    sync: bool,
}

#[derive(Debug, Args)]
struct DemoListArgs {
    /// Include paths that no longer exist but still have readable records
    #[arg(long)]
    all: bool,
}

#[derive(Debug, Args)]
struct DemoPathArgs {
    /// Demo name under the metactl demo home
    #[arg(long, default_value = "metactl-demo")]
    name: String,
    /// Explicit demo root path
    #[arg(long, value_name = "PATH")]
    path: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct DemoDestroyArgs {
    /// Demo name under the metactl demo home
    #[arg(long, default_value = "metactl-demo")]
    name: String,
    /// Explicit demo root path
    #[arg(long, value_name = "PATH")]
    path: Option<PathBuf>,
    /// Also remove an empty parent demo-home directory after deleting the sandbox
    #[arg(long)]
    purge: bool,
}

#[derive(Debug, Args)]
struct ProfileArgs {
    #[command(subcommand)]
    command: Option<ProfileCommand>,
}

#[derive(Debug, Args)]
struct LibraryArgs {
    #[command(subcommand)]
    command: LibraryCommand,
}

#[derive(Debug, Args)]
struct PackArgs {
    #[command(subcommand)]
    command: PackCommand,
}

#[derive(Debug, Args)]
struct PluginArgs {
    #[command(subcommand)]
    command: PluginCommand,
}

#[derive(Debug, Subcommand)]
enum PluginCommand {
    /// List packs eligible for plugin projection
    List(PluginListArgs),
    /// Export a plugin bundle into a local marketplace root
    Export(PluginExportArgs),
    /// Verify a generated plugin marketplace or bundle
    Verify(PluginVerifyArgs),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PluginTierArg {
    Public,
    Private,
}

impl From<PluginTierArg> for PluginTier {
    fn from(value: PluginTierArg) -> Self {
        match value {
            PluginTierArg::Public => PluginTier::Public,
            PluginTierArg::Private => PluginTier::Private,
        }
    }
}

#[derive(Debug, Args)]
struct PluginListArgs {
    /// Library root to inspect (defaults to bundled public starter library)
    #[arg(long, value_name = "PATH")]
    library_root: Option<PathBuf>,
    /// Target runtime to check for plugin support
    #[arg(long, default_value = "codex-cli")]
    target: String,
    /// Optional output tier filter
    #[arg(long, value_enum)]
    tier: Option<PluginTierArg>,
}

#[derive(Debug, Args)]
struct PluginExportArgs {
    /// Output tier to export
    #[arg(long, value_enum)]
    tier: PluginTierArg,
    /// Source library root; required for private exports, defaults to library/starter for public
    #[arg(long, value_name = "PATH")]
    library_root: Option<PathBuf>,
    /// Target runtime to export
    #[arg(long, default_value = "codex-cli")]
    target: String,
    /// Marketplace root receiving the generated plugin bundle
    #[arg(long, value_name = "PATH")]
    out: PathBuf,
    /// Replace the generated bundle when it already exists
    #[arg(long)]
    force: bool,
    /// Override generated plugin name
    #[arg(long)]
    name: Option<String>,
}

#[derive(Debug, Args)]
struct PluginVerifyArgs {
    /// Marketplace root or plugin bundle path
    #[arg(long, value_name = "PATH")]
    path: PathBuf,
    /// Target runtime to verify
    #[arg(long, default_value = "codex-cli")]
    target: String,
    /// Expected output tier
    #[arg(long, value_enum)]
    tier: Option<PluginTierArg>,
}

#[derive(Debug, Args)]
struct ExportArgs {
    #[command(subcommand)]
    command: ExportCommand,
}

#[derive(Debug, Subcommand)]
enum ExportCommand {
    /// Export a safe public example artifact from public fixture text
    PublicExample(ExportArtifactArgs),
    /// Export a sanitized record for a private-source artifact
    Sanitized(ExportArtifactArgs),
}

#[derive(Debug, Args)]
struct ExportArtifactArgs {
    /// Artifact or pack id to export
    artifact: String,
}

#[derive(Debug, Subcommand)]
enum PackCommand {
    /// Activate a project pack (alias for `metactl use`)
    Use(UseArgs),
    /// Add project packs (alias for `metactl add`)
    Add(AddArgs),
    /// Remove project packs (alias for `metactl remove`)
    Remove(RemoveArgs),
    /// Import an Agent Skill folder into the local project as a candidate pack
    ImportSkill(PackImportSkillArgs),
    /// Export an imported Agent Skill folder for a target runtime
    ExportSkill(PackExportSkillArgs),
    /// Verify an imported Agent Skill folder against a portability profile
    VerifySkill(PackVerifySkillArgs),
}

#[derive(Debug, Args)]
struct SkillsArgs {
    #[command(subcommand)]
    command: SkillsCommand,
}

#[derive(Debug, Subcommand)]
enum SkillsCommand {
    /// Install a repo-local Agent Skill folder into the user-global Codex skill root
    Add(SkillsAddArgs),
    /// List repo-local or user-global Codex Agent Skill folders
    List(SkillsListArgs),
    /// Remove a user-global Codex Agent Skill folder
    Remove(SkillsRemoveArgs),
}

#[derive(Debug, Args)]
struct SkillsAddArgs {
    /// Skill folder containing SKILL.md, SKILL.md path, or repo-local skill name
    path: PathBuf,
    /// Skill visibility scope to update
    #[arg(long, value_enum, default_value = "user")]
    scope: SkillScopeArg,
    /// Target runtime skill root to manage
    #[arg(long, default_value = "codex-cli")]
    target: String,
    /// Replace an existing user-global skill folder with the same frontmatter.name
    #[arg(long)]
    force: bool,
    /// Permit executable files under scripts/ while still classifying them
    #[arg(long)]
    allow_executable_scripts: bool,
}

#[derive(Debug, Args)]
struct SkillsListArgs {
    /// Skill visibility scope to inspect
    #[arg(long, value_enum, default_value = "repo")]
    scope: SkillScopeArg,
    /// Target runtime skill root to inspect
    #[arg(long, default_value = "codex-cli")]
    target: String,
}

#[derive(Debug, Args)]
struct SkillsRemoveArgs {
    /// User-global skill frontmatter.name or folder name
    name: String,
    /// Skill visibility scope to update
    #[arg(long, value_enum, default_value = "user")]
    scope: SkillScopeArg,
    /// Target runtime skill root to manage
    #[arg(long, default_value = "codex-cli")]
    target: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum SkillScopeArg {
    Repo,
    User,
}

#[derive(Debug, Args)]
struct PackImportSkillArgs {
    /// Path to an Agent Skill folder containing SKILL.md
    path: PathBuf,
    /// Permit executable files under scripts/ while still classifying them
    #[arg(long)]
    allow_executable_scripts: bool,
}

#[derive(Debug, Args)]
struct PackExportSkillArgs {
    /// Pack/skill id to export
    pack_id: String,
    /// Target runtime id receiving the exported skill folder
    #[arg(long)]
    target: String,
}

#[derive(Debug, Args)]
struct PackVerifySkillArgs {
    /// Pack/skill id to verify
    pack_id: String,
    /// Verification profile to apply
    #[arg(long, default_value = "portable")]
    profile: String,
}

#[derive(Debug, Subcommand)]
enum LibraryCommand {
    /// Create a user-private writable library and profile
    Init(LibraryInitArgs),
}

#[derive(Debug, Args)]
struct LibraryInitArgs {
    /// Create the user-private library under the local metactl config directory
    #[arg(long)]
    user: bool,
    /// Profile name to create or update
    #[arg(long, default_value = "user")]
    profile: String,
    /// Also set this profile as the machine default
    #[arg(long)]
    set_default: bool,
}

#[derive(Debug, Args)]
struct ProjectArgs {
    #[command(subcommand)]
    command: ProjectCommand,
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    /// Link this project to a named profile
    Link(ProjectLinkArgs),
}

#[derive(Debug, Args)]
struct ProjectLinkArgs {
    /// Profile name to record in metactl.yaml
    #[arg(long)]
    profile: String,
}

#[derive(Debug, Subcommand)]
enum ProfileCommand {
    /// List profile YAML files in the user config profiles directory
    List,
    /// Show machine default profile and settings file path
    Show,
    /// Set the machine default profile name (must have a matching profiles/<name>.yaml)
    SetDefault {
        /// Profile id (filename stem under profiles/)
        name: String,
    },
    /// Clear the machine default profile
    ClearDefault,
}

#[derive(Debug, Args)]
struct UseArgs {
    /// Pack name or search query
    query: String,
    /// Add to local config instead of shared config
    #[arg(long)]
    local: bool,
    /// Source path to resolve from
    #[arg(long)]
    from: Option<PathBuf>,
    /// Skip sync after adding
    #[arg(long)]
    no_sync: bool,
}

#[derive(Debug, Args)]
struct AddArgs {
    /// Pack IDs to add (e.g. python-refactor, unit-test-loop)
    pack_ids: Vec<String>,
    /// Also run sync after adding packs (runs even if packs were already configured)
    #[arg(long)]
    sync: bool,
}

#[derive(Debug, Args)]
struct RemoveArgs {
    /// Pack IDs to remove
    pack_ids: Vec<String>,
    /// Also run sync after removing packs (runs even if nothing was removed)
    #[arg(long)]
    sync: bool,
}

#[derive(Debug, Args)]
struct TargetArgs {
    #[command(subcommand)]
    command: Option<TargetCommand>,
}

#[derive(Debug, Subcommand)]
enum TargetCommand {
    /// List available and configured targets
    List(TargetListArgs),
    /// Add targets to metactl.yaml
    Add(TargetUpdateArgs),
    /// Remove targets from metactl.yaml
    Remove(TargetUpdateArgs),
}

#[derive(Debug, Args)]
struct TargetListArgs {
    /// Only show targets currently configured in metactl.yaml
    #[arg(long)]
    installed: bool,
}

#[derive(Debug, Args)]
struct TargetUpdateArgs {
    /// Target IDs to add or remove (`all` expands to every discovered starter target)
    target_ids: Vec<String>,
    /// Also run sync after updating targets
    #[arg(long)]
    sync: bool,
}

#[derive(Debug, Args)]
struct StatusArgs {
    /// Show status for a specific target only
    #[arg(long, short = 't')]
    target: Option<String>,
}

#[derive(Debug, Clone, ValueEnum)]
enum InitMode {
    Greenfield,
    BrownfieldAutoDetect,
}

#[derive(Debug, Args)]
struct ListArgs {
    #[command(subcommand)]
    subject: ListSubject,
    /// Maximum number of items to show
    #[arg(long, short = 'n')]
    limit: Option<usize>,
    /// Only show items currently configured in metactl.yaml
    #[arg(long)]
    installed: bool,
    /// Only show items from the starter library
    #[arg(long)]
    starter_only: bool,
    /// Include candidate (unverified) packs
    #[arg(long)]
    candidate: bool,
}

#[derive(Debug, Clone, Subcommand)]
enum ListSubject {
    /// Available roles (e.g. builder, reviewer)
    Roles,
    /// Available packs (skills and instructions for coding agents)
    Packs,
    /// Available policies (e.g. brownfield-safe-builder)
    Policies,
    /// Supported target runtimes (e.g. claude-code, codex-cli, cursor, gemini-cli)
    Targets,
}

#[derive(Debug, Args)]
struct SearchArgs {
    /// Natural-language or keyword query (e.g. "python refactor")
    query: String,
    /// Maximum number of results to return
    #[arg(long, short = 'n')]
    limit: Option<u64>,
    /// Filter results for a specific target runtime
    #[arg(long, short = 't')]
    target: Option<String>,
    /// Filter results for a specific role
    #[arg(long, short = 'r')]
    role: Option<String>,
    /// Filter results for a specific policy
    #[arg(long, short = 'p')]
    policy: Option<String>,
    /// Include packs that were suppressed by policy or role
    #[arg(long)]
    show_suppressed: bool,
}

#[derive(Debug, Args)]
struct StatsArgs {
    #[command(subcommand)]
    command: StatsCommand,
}

#[derive(Debug, Subcommand)]
enum StatsCommand {
    /// Rebuild local surface usage stats from JSONL events
    Rebuild(StatsRebuildArgs),
    /// Show usage stats for all packs or one pack
    Show(StatsShowArgs),
}

#[derive(Debug, Args)]
struct StatsRebuildArgs {
    /// Override usage event JSONL path
    #[arg(long)]
    events: Option<PathBuf>,
    /// Override stats JSON output path
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct StatsShowArgs {
    /// Show one pack only
    #[arg(long)]
    pack: Option<String>,
}

#[derive(Debug, Args)]
struct SurfaceArgs {
    #[command(subcommand)]
    command: SurfaceCommand,
}

#[derive(Debug, Args)]
struct BackgroundArgs {
    #[command(subcommand)]
    command: Option<BackgroundCommand>,
}

#[derive(Debug, Subcommand)]
enum BackgroundCommand {
    /// Show the OS scheduler install plan without writing machine state
    Plan(BackgroundPlanArgs),
    /// Install the OS scheduler entry
    Install(BackgroundInstallArgs),
    /// Show scheduler status using the native OS service manager
    Status(BackgroundStatusArgs),
    /// Remove the OS scheduler entry
    Uninstall(BackgroundUninstallArgs),
    /// Run one report-only refresh cycle; intended as the scheduler entrypoint
    Run(BackgroundRunArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum BackgroundScopeArg {
    Project,
    Fleet,
}

impl BackgroundScopeArg {
    fn as_str(self) -> &'static str {
        match self {
            BackgroundScopeArg::Project => "project",
            BackgroundScopeArg::Fleet => "fleet",
        }
    }
}

#[derive(Debug, Args)]
struct BackgroundPlanArgs {
    /// Refresh one project or every non-disabled project in the Fleet controller
    #[arg(long, value_enum, default_value = "project")]
    scope: BackgroundScopeArg,
    /// Fleet controller path; defaults to the resolved machine Fleet controller
    #[arg(long)]
    controller: Option<PathBuf>,
    /// Scheduler interval in minutes
    #[arg(long, default_value_t = 60)]
    interval_minutes: u32,
    /// Override state/log directory
    #[arg(long)]
    log_dir: Option<PathBuf>,
    /// Override generated scheduler label or task name
    #[arg(long)]
    label: Option<String>,
}

#[derive(Debug, Args)]
struct BackgroundInstallArgs {
    /// Refresh one project or every non-disabled project in the Fleet controller
    #[arg(long, value_enum, default_value = "project")]
    scope: BackgroundScopeArg,
    /// Fleet controller path; defaults to the resolved machine Fleet controller
    #[arg(long)]
    controller: Option<PathBuf>,
    /// Scheduler interval in minutes
    #[arg(long, default_value_t = 60)]
    interval_minutes: u32,
    /// Override state/log directory
    #[arg(long)]
    log_dir: Option<PathBuf>,
    /// Override generated scheduler label or task name
    #[arg(long)]
    label: Option<String>,
    /// Confirm persistent OS scheduler writes
    #[arg(long, short = 'y')]
    yes: bool,
}

#[derive(Debug, Args)]
struct BackgroundStatusArgs {
    /// Refresh one project or every non-disabled project in the Fleet controller
    #[arg(long, value_enum, default_value = "project")]
    scope: BackgroundScopeArg,
    /// Fleet controller path; defaults to the resolved machine Fleet controller
    #[arg(long)]
    controller: Option<PathBuf>,
    /// Override state/log directory
    #[arg(long)]
    log_dir: Option<PathBuf>,
    /// Override generated scheduler label or task name
    #[arg(long)]
    label: Option<String>,
}

#[derive(Debug, Args)]
struct BackgroundUninstallArgs {
    /// Refresh one project or every non-disabled project in the Fleet controller
    #[arg(long, value_enum, default_value = "project")]
    scope: BackgroundScopeArg,
    /// Fleet controller path; defaults to the resolved machine Fleet controller
    #[arg(long)]
    controller: Option<PathBuf>,
    /// Override state/log directory
    #[arg(long)]
    log_dir: Option<PathBuf>,
    /// Override generated scheduler label or task name
    #[arg(long)]
    label: Option<String>,
    /// Confirm persistent OS scheduler removal
    #[arg(long, short = 'y')]
    yes: bool,
}

#[derive(Debug, Args)]
struct BackgroundRunArgs {
    /// Refresh one project or every non-disabled project in the Fleet controller
    #[arg(long, value_enum, default_value = "project")]
    scope: BackgroundScopeArg,
    /// Fleet controller path; defaults to the resolved machine Fleet controller
    #[arg(long)]
    controller: Option<PathBuf>,
    /// Override state/log directory
    #[arg(long)]
    log_dir: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum SurfaceCommand {
    /// Rebuild stats if needed and write recommendation reports
    Report(SurfaceReportArgs),
    /// Pin a pack as hot or command-visible
    Pin(SurfacePinArgs),
    /// Block a pack from auto-selected command/skill surfaces
    Block(SurfacePackArgs),
    /// Clear one override or all overrides
    Reset(SurfaceResetArgs),
}

#[derive(Debug, Args)]
struct SurfaceReportArgs {
    /// Lifecycle mode for the recommendation run
    #[arg(long, value_enum, default_value_t = SurfaceLifecycleModeArg::Recommend)]
    lifecycle_mode: SurfaceLifecycleModeArg,
    /// Mark this run as scheduled/report-only
    #[arg(long)]
    scheduled: bool,
}

#[derive(Debug, Args)]
struct SurfacePinArgs {
    /// Pack id to pin
    pack_id: String,
    /// Pin as command-visible without promoting full body to hot
    #[arg(long)]
    command: bool,
}

#[derive(Debug, Args)]
struct SurfacePackArgs {
    /// Pack id to update
    pack_id: String,
}

#[derive(Debug, Args)]
struct SurfaceResetArgs {
    /// Pack id to reset; omit with --all to clear every override
    pack_id: Option<String>,
    /// Clear every surface override
    #[arg(long)]
    all: bool,
}

#[derive(Debug, Args)]
struct ExplainArgs {
    /// Optional query context to explain selections for
    #[arg(long)]
    query: Option<String>,
    /// Explain staged compile manifests instead of live resolution
    #[arg(long)]
    staged: bool,
    /// Explain for a specific target runtime
    #[arg(long, short = 't')]
    target: Option<String>,
    /// Override role for explanation
    #[arg(long, short = 'r')]
    role: Option<String>,
    /// Override policy for explanation
    #[arg(long, short = 'p')]
    policy: Option<String>,
    /// Folder-native surface selection mode for explanation (overrides defaults.surface_selection_mode)
    #[arg(long, value_enum)]
    surface_mode: Option<SurfaceSelectionModeArg>,
}

#[derive(Debug, Args)]
struct CompileArgs {
    /// Target runtimes to compile for (default: all configured targets)
    #[arg(long, short = 't')]
    target: Vec<String>,
    /// Compile all configured targets explicitly (alias for omitting --target)
    #[arg(long)]
    all: bool,
    /// Override role for compilation
    #[arg(long, short = 'r')]
    role: Option<String>,
    /// Override policy for compilation
    #[arg(long, short = 'p')]
    policy: Option<String>,
    /// Update the lockfile after compiling
    #[arg(long)]
    update_lock: bool,
    /// After a successful compile, run apply for all compiled targets (same as `metactl apply`)
    #[arg(long)]
    apply: bool,
    /// Apply mode when using `--apply` (root instruction docs remain regular files)
    #[arg(long, value_enum)]
    apply_mode: Option<ApplyModeArg>,
    /// Folder-native surface selection mode (overrides defaults.surface_selection_mode)
    #[arg(long, value_enum)]
    surface_mode: Option<SurfaceSelectionModeArg>,
}

#[derive(Debug, Args, Clone)]
struct SyncArgs {
    /// Target runtimes to sync (default: all configured targets). Comma-separated aliases are accepted.
    #[arg(long, short = 't')]
    target: Vec<String>,
    /// Sync all configured targets explicitly (alias for omitting --target)
    #[arg(long)]
    all: bool,
    /// Override role for this sync
    #[arg(long, short = 'r')]
    role: Option<String>,
    /// Override policy for this sync
    #[arg(long, short = 'p')]
    policy: Option<String>,
    /// Brownfield adoption strategy: preview, patch, or takeover
    #[arg(long, value_enum)]
    adopt: Option<SyncAdoptArg>,
    /// Preview generated changes without materializing runtime files
    #[arg(long)]
    preview: bool,
    /// Explicitly apply generated changes (default when --preview is not passed)
    #[arg(long)]
    apply: bool,
    /// Folder-native surface selection mode (overrides defaults.surface_selection_mode)
    #[arg(long, value_enum)]
    surface_mode: Option<SurfaceSelectionModeArg>,
    /// Require all configured private sources to be synced and fresh before syncing targets
    #[arg(long)]
    require_private_sources: bool,
}

#[derive(Debug, Args)]
struct ApplyArgs {
    /// Apply for a specific target only (default: all compiled targets)
    #[arg(long, short = 't')]
    target: Option<String>,
    /// How to materialize outputs: symlink, copy, patch, or takeover
    #[arg(long, value_enum)]
    mode: Option<ApplyModeArg>,
    /// Show what would be applied without writing files
    #[arg(long)]
    preview: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ApplyModeArg {
    Symlink,
    Copy,
    Patch,
    Takeover,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SyncAdoptArg {
    Preview,
    Patch,
    Takeover,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SurfaceSelectionModeArg {
    Minimal,
    Full,
    Auto,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SurfaceLifecycleModeArg {
    Observe,
    Recommend,
    Apply,
}

impl From<ApplyModeArg> for ApplyMode {
    fn from(value: ApplyModeArg) -> Self {
        match value {
            ApplyModeArg::Symlink => ApplyMode::Symlink,
            ApplyModeArg::Copy => ApplyMode::Copy,
            ApplyModeArg::Patch => ApplyMode::Patch,
            ApplyModeArg::Takeover => ApplyMode::Takeover,
        }
    }
}

impl From<SurfaceSelectionModeArg> for metactl::SurfaceSelectionMode {
    fn from(value: SurfaceSelectionModeArg) -> Self {
        match value {
            SurfaceSelectionModeArg::Minimal => metactl::SurfaceSelectionMode::Minimal,
            SurfaceSelectionModeArg::Full => metactl::SurfaceSelectionMode::Full,
            SurfaceSelectionModeArg::Auto => metactl::SurfaceSelectionMode::Auto,
        }
    }
}

impl From<SurfaceLifecycleModeArg> for SurfaceLifecycleMode {
    fn from(value: SurfaceLifecycleModeArg) -> Self {
        match value {
            SurfaceLifecycleModeArg::Observe => SurfaceLifecycleMode::Observe,
            SurfaceLifecycleModeArg::Recommend => SurfaceLifecycleMode::Recommend,
            SurfaceLifecycleModeArg::Apply => SurfaceLifecycleMode::Apply,
        }
    }
}

#[derive(Debug, Args)]
struct RevertArgs {
    /// Revert a specific target only (default: first compiled target)
    #[arg(long, short = 't')]
    target: Option<String>,
    /// Revert all applied targets
    #[arg(long)]
    all: bool,
}

#[derive(Debug, Args)]
struct ValidateCmdArgs {
    /// Validate a specific target only (default: all compiled targets)
    #[arg(long, short = 't')]
    target: Option<String>,
    /// Strict v1 wording alias; drift and stale locks already fail validation
    #[arg(long)]
    strict: bool,
}

#[derive(Debug, Args)]
struct DoctorArgs {
    /// Check a specific target only (default: all compiled targets)
    #[arg(long, short = 't')]
    target: Option<String>,
}

#[derive(Debug)]
struct CommandOutput {
    human: String,
    json: serde_json::Value,
}

#[derive(Debug)]
struct CliError {
    code: u8,
    message: String,
    details: Vec<String>,
    json: serde_json::Value,
}

#[derive(Debug, Clone)]
struct SharedSurfaceRule {
    path: String,
    owner: String,
    suppressed_targets: Vec<String>,
    message: String,
}

#[derive(Debug, Clone)]
struct DiscoverabilityReport {
    role_id: String,
    policy_id: String,
    target_ids: Vec<String>,
    missing_role: bool,
    missing_policy: bool,
    missing_targets: Vec<String>,
    missing_packs: Vec<String>,
    effective_library_roots: Vec<String>,
    profile_name: Option<String>,
}

impl CliError {
    fn new(code: u8, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            code,
            json: json!({
                "ok": false,
                "api_version": API_VERSION,
                "message": message,
            }),
            message,
            details: Vec::new(),
        }
    }

    fn with_details(mut self, details: Vec<String>) -> Self {
        self.json = json!({
            "ok": false,
            "api_version": API_VERSION,
            "message": self.message,
            "details": details,
        });
        self.details = details;
        self
    }
}

fn agent_error_json(cli: &Cli, err: &CliError) -> Value {
    let mut value = err.json.clone();
    if !value.is_object() {
        value = json!({});
    }
    let next_commands = next_commands_from_error(err);
    let findings = findings_from_error_json(&err.json);
    let obj = value.as_object_mut().expect("object json");
    obj.insert("ok".to_string(), json!(false));
    obj.insert("api_version".to_string(), json!(API_VERSION));
    obj.insert(
        "command".to_string(),
        json!(command_contract_name(&cli.command)),
    );
    obj.insert("error_code".to_string(), json!(exit_code_label(err.code)));
    obj.insert(
        "requires_operator".to_string(),
        json!(err.code == EXIT_INTERNAL),
    );
    obj.insert(
        "risk_level".to_string(),
        json!(if err.code == EXIT_INTERNAL {
            "operator"
        } else {
            "recoverable"
        }),
    );
    obj.insert("next_commands".to_string(), json!(next_commands));
    obj.insert("findings".to_string(), findings);
    value
}

fn next_commands_from_error(err: &CliError) -> Vec<String> {
    let mut commands = Vec::new();
    for key in ["next_commands", "next_steps"] {
        if let Some(items) = err.json.get(key).and_then(Value::as_array) {
            commands.extend(
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|item| item.trim_start_matches("Next: ").to_string()),
            );
        }
    }
    for detail in &err.details {
        if let Some(command) = detail.strip_prefix("Next: ") {
            commands.push(command.to_string());
        }
    }
    commands.sort();
    commands.dedup();
    commands
}

fn findings_from_error_json(value: &Value) -> Value {
    if let Some(findings) = value.get("findings") {
        return findings.clone();
    }
    if let Some(findings) = value
        .get("source_audit")
        .and_then(|source_audit| source_audit.get("findings"))
    {
        return findings.clone();
    }
    json!([])
}

fn exit_code_label(code: u8) -> &'static str {
    match code {
        EXIT_INTERNAL => "internal",
        EXIT_STATE => "state",
        EXIT_STALE_LOCK => "stale_lock",
        EXIT_CONFLICT => "conflict",
        EXIT_VALIDATION => "validation",
        EXIT_SUCCESS => "success",
        _ => "unknown",
    }
}

fn command_contract_name(command: &Commands) -> &'static str {
    match command {
        Commands::Init(_) => "init",
        Commands::Library(_) => "library",
        Commands::Pack(args) => match &args.command {
            PackCommand::Use(_) => "use",
            PackCommand::Add(_) => "add",
            PackCommand::Remove(_) => "remove",
            PackCommand::ImportSkill(_)
            | PackCommand::ExportSkill(_)
            | PackCommand::VerifySkill(_) => "pack",
        },
        Commands::Skills(_) => "skills",
        Commands::Plugin(_) => "plugin",
        Commands::Export(_) => "export",
        Commands::Demo(args) => match &args.command {
            Some(DemoCommand::List(_)) | None => "demo list",
            _ => "demo",
        },
        Commands::Setup(_) => "setup",
        Commands::CheckPublicBoundary => "check-public-boundary",
        Commands::Project(_) => "project",
        Commands::Use(_) => "use",
        Commands::Add(_) => "add",
        Commands::Remove(_) => "remove",
        Commands::Target(_) => "target",
        Commands::Fleet(_) => "fleet",
        Commands::Status(_) => "status",
        Commands::List(_) => "list",
        Commands::Search(_) => "search",
        Commands::Stats(_) => "stats",
        Commands::Surface(_) => "surface",
        Commands::Background(_) => "background",
        Commands::Explain(_) => "explain",
        Commands::Preview(_) | Commands::Sync(_) => "sync",
        Commands::Compile(_) => "compile",
        Commands::Apply(_) => "apply",
        Commands::Revert(_) => "revert",
        Commands::Validate(_) => "validate",
        Commands::Check(_) => "check",
        Commands::Doctor(_) => "doctor",
        Commands::Audit(_) => "audit",
        Commands::Ignore(_) => "ignore",
        Commands::Hook(_) => "hook",
        Commands::Source(_) => "source",
        Commands::Profile(_) => "profile",
        Commands::Version => "version",
    }
}

fn builtin_profile_template_json() -> Vec<Value> {
    builtin_profile_templates()
        .into_iter()
        .map(|template| {
            json!({
                "name": template.name,
                "description": template.description,
                "targets": template.profile.targets,
                "starter_library": template.profile.starter_library,
            })
        })
        .collect()
}

impl SharedSurfaceRule {
    fn to_json(&self) -> Value {
        json!({
            "path": self.path,
            "owner": self.owner,
            "suppressed_targets": self.suppressed_targets,
            "message": self.message,
        })
    }

    fn human_line(&self) -> String {
        format!(
            "{} -> {} ({} use target-local surfaces only)",
            self.path,
            self.owner,
            self.suppressed_targets.join(", ")
        )
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(output) => {
            if cli.machine_output() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&output.json).unwrap_or_else(|_| "{}".to_string())
                );
            } else if !cli.quiet {
                println!("{}", output.human);
            }
            ExitCode::from(EXIT_SUCCESS)
        }
        Err(err) => {
            if cli.machine_output() {
                let json = if cli.agent {
                    agent_error_json(&cli, &err)
                } else {
                    err.json.clone()
                };
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json).unwrap_or_else(|_| "{}".to_string())
                );
            } else {
                eprintln!("Error: {}", err.message);
                for detail in err.details {
                    eprintln!("  - {}", detail);
                }
            }
            ExitCode::from(err.code)
        }
    }
}

fn run(cli: &Cli) -> std::result::Result<CommandOutput, CliError> {
    let _operation_lock = if let Some(command) = mutating_operation_label(cli) {
        let project_root = operation_lock_project_root(cli)?;
        let lock = OperationLock::acquire(&project_root, command).map_err(operation_lock_error)?;
        if let Ok(ms) = std::env::var("METACTL_TEST_HOLD_OPERATION_LOCK_MS") {
            if let Ok(ms) = ms.parse::<u64>() {
                std::thread::sleep(std::time::Duration::from_millis(ms));
            }
        }
        Some(lock)
    } else {
        None
    };
    match &cli.command {
        Commands::Init(args) => cmd_init(cli, args),
        Commands::Setup(args) => cmd_setup(cli, args),
        Commands::Library(args) => cmd_library(cli, args),
        Commands::Pack(args) => cmd_pack(cli, args),
        Commands::Skills(args) => cmd_skills(cli, args),
        Commands::Plugin(args) => cmd_plugin(cli, args),
        Commands::Export(args) => cmd_export(cli, args),
        Commands::Demo(args) => cmd_demo(cli, args),
        Commands::CheckPublicBoundary => cmd_check_public_boundary(cli),
        Commands::Project(args) => cmd_project(cli, args),
        Commands::Use(args) => cmd_use(cli, args),
        Commands::Add(args) => cmd_add(cli, args),
        Commands::Remove(args) => cmd_remove(cli, args),
        Commands::Target(args) => cmd_target(cli, args),
        Commands::Fleet(args) => cmd_fleet(cli, args),
        Commands::Status(args) => cmd_status(cli, args),
        Commands::List(args) => cmd_list(cli, args),
        Commands::Search(args) => cmd_search(cli, args),
        Commands::Stats(args) => cmd_stats(cli, args),
        Commands::Surface(args) => cmd_surface(cli, args),
        Commands::Background(args) => cmd_background(cli, args),
        Commands::Explain(args) => cmd_explain(cli, args),
        Commands::Preview(args) => {
            let mut args = args.clone();
            args.preview = true;
            args.apply = false;
            cmd_sync(cli, &args)
        }
        Commands::Sync(args) => cmd_sync(cli, args),
        Commands::Compile(args) => cmd_compile(cli, args),
        Commands::Apply(args) => cmd_apply(cli, args),
        Commands::Revert(args) => cmd_revert(cli, args),
        Commands::Validate(args) => cmd_validate(cli, args),
        Commands::Check(args) => cmd_validate(cli, args),
        Commands::Doctor(args) => cmd_doctor(cli, args),
        Commands::Audit(args) => cmd_audit(cli, args),
        Commands::Ignore(args) => cmd_ignore(cli, args),
        Commands::Hook(args) => cmd_hook(cli, args),
        Commands::Source(args) => cmd_source(cli, args),
        Commands::Profile(args) => cmd_profile(cli, args),
        Commands::Version => Ok(CommandOutput {
            human: format!("metactl {} ({})", env!("CARGO_PKG_VERSION"), API_VERSION),
            json: success_json(
                "version",
                None,
                json!({
                    "version": env!("CARGO_PKG_VERSION"),
                }),
            ),
        }),
    }
}

fn mutating_operation_label(cli: &Cli) -> Option<&'static str> {
    match &cli.command {
        Commands::Init(_) => Some("init"),
        Commands::Setup(args) => {
            if args.plan {
                None
            } else {
                Some("setup")
            }
        }
        Commands::Library(args) => match &args.command {
            LibraryCommand::Init(_) => Some("library init"),
        },
        Commands::Pack(args) => match &args.command {
            PackCommand::Use(_) => Some("pack use"),
            PackCommand::Add(_) => Some("pack add"),
            PackCommand::Remove(_) => Some("pack remove"),
            PackCommand::ImportSkill(_) => Some("pack import-skill"),
            PackCommand::ExportSkill(_) => Some("pack export-skill"),
            PackCommand::VerifySkill(_) => None,
        },
        Commands::Skills(args) => match &args.command {
            SkillsCommand::Add(_) => Some("skills add"),
            SkillsCommand::List(_) => None,
            SkillsCommand::Remove(_) => Some("skills remove"),
        },
        Commands::Plugin(args) => match &args.command {
            PluginCommand::List(_) | PluginCommand::Verify(_) => None,
            PluginCommand::Export(_) => Some("plugin export"),
        },
        Commands::Export(_) => Some("export"),
        Commands::Demo(args) => match &args.command {
            Some(DemoCommand::Create(_))
            | Some(DemoCommand::Reset(_))
            | Some(DemoCommand::Destroy(_)) => None,
            Some(DemoCommand::List(_)) | Some(DemoCommand::Path(_)) | None => None,
        },
        Commands::CheckPublicBoundary => None,
        Commands::Project(args) => match &args.command {
            ProjectCommand::Link(_) => Some("project link"),
        },
        Commands::Use(_) => Some("use"),
        Commands::Add(_) => Some("add"),
        Commands::Remove(_) => Some("remove"),
        Commands::Target(args) => match &args.command {
            Some(TargetCommand::List(_)) | None => None,
            Some(TargetCommand::Add(_)) => Some("target add"),
            Some(TargetCommand::Remove(_)) => Some("target remove"),
        },
        Commands::Fleet(args) => match &args.command {
            Some(FleetCommand::List)
            | Some(FleetCommand::Status(_))
            | Some(FleetCommand::Controller(_))
            | None => None,
            Some(FleetCommand::Sync(args)) => args.apply.then_some("fleet sync"),
        },
        Commands::Preview(_) => Some("preview"),
        Commands::Sync(_) => Some("sync"),
        Commands::Stats(args) => match &args.command {
            StatsCommand::Rebuild(_) => Some("stats rebuild"),
            StatsCommand::Show(_) => None,
        },
        Commands::Surface(args) => match &args.command {
            SurfaceCommand::Report(_) => Some("surface report"),
            SurfaceCommand::Pin(_) => Some("surface pin"),
            SurfaceCommand::Block(_) => Some("surface block"),
            SurfaceCommand::Reset(_) => Some("surface reset"),
        },
        Commands::Background(args) => match &args.command {
            Some(BackgroundCommand::Install(_)) => Some("background install"),
            Some(BackgroundCommand::Uninstall(_)) => Some("background uninstall"),
            Some(BackgroundCommand::Run(_)) => Some("background run"),
            Some(BackgroundCommand::Plan(_)) | Some(BackgroundCommand::Status(_)) | None => None,
        },
        Commands::Compile(args) => {
            if args.apply {
                Some("compile --apply")
            } else {
                Some("compile")
            }
        }
        Commands::Apply(args) => {
            if args.preview {
                None
            } else {
                Some("apply")
            }
        }
        Commands::Revert(_) => Some("revert"),
        Commands::Ignore(args) => match &args.command {
            Some(IgnoreCommand::Status(_)) | None => None,
            Some(IgnoreCommand::Install(_)) => Some("ignore install"),
            Some(IgnoreCommand::Fix(args)) => {
                if args.plan {
                    None
                } else {
                    Some("ignore fix")
                }
            }
        },
        Commands::Hook(args) => match &args.command {
            HookCommand::Install(_) => Some("hook install"),
            HookCommand::Status => None,
        },
        Commands::Source(args) => match &args.command {
            Some(SourceCommand::List) | None => None,
            Some(SourceCommand::Add(_)) => Some("source add"),
            Some(SourceCommand::Sync(_)) => Some("source sync"),
            Some(SourceCommand::Remove(_)) => Some("source remove"),
        },
        Commands::Status(_)
        | Commands::List(_)
        | Commands::Search(_)
        | Commands::Explain(_)
        | Commands::Validate(_)
        | Commands::Check(_)
        | Commands::Doctor(_)
        | Commands::Audit(_)
        | Commands::Profile(_)
        | Commands::Version => None,
    }
}

fn operation_lock_project_root(cli: &Cli) -> std::result::Result<PathBuf, CliError> {
    if let Commands::Fleet(FleetArgs {
        command: Some(FleetCommand::Sync(args)),
    }) = &cli.command
    {
        if args.apply {
            return resolve_fleet_controller(cli).map(|controller| controller.project_root);
        }
    }
    project_root(cli).map_err(internal_error)
}

fn project_root(cli: &Cli) -> Result<PathBuf> {
    match cli.project.clone() {
        Some(path) => Ok(path),
        None => std::env::current_dir().context("determine current directory"),
    }
}

const DEMO_MARKER_DIR: &str = ".metactl-demo";
const DEMO_MANIFEST_FILE: &str = "manifest.json";
const DEMO_SEED_VERSION: &str = "brownfield-basic-v1";

fn cmd_demo(cli: &Cli, args: &DemoArgs) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        Some(DemoCommand::Create(create_args)) => cmd_demo_create(cli, create_args, false),
        Some(DemoCommand::List(list_args)) => cmd_demo_list(cli, list_args),
        Some(DemoCommand::Path(path_args)) => cmd_demo_path(cli, path_args),
        Some(DemoCommand::Reset(create_args)) => cmd_demo_create(cli, create_args, true),
        Some(DemoCommand::Destroy(destroy_args)) => cmd_demo_destroy(cli, destroy_args),
        None => cmd_demo_list(cli, &DemoListArgs { all: false }),
    }
}

fn cmd_demo_create(
    cli: &Cli,
    args: &DemoCreateArgs,
    reset: bool,
) -> std::result::Result<CommandOutput, CliError> {
    let demo_root = resolve_demo_path(&args.name, args.path.as_deref())?;
    let demo_name = demo_name_for_path(&args.name, &demo_root);
    if reset && demo_root.exists() {
        remove_demo_root(&demo_root)?;
    } else if demo_root.exists() && demo_manifest_path(&demo_root).exists() {
        return Err(CliError::new(
            EXIT_CONFLICT,
            format!(
                "Demo sandbox already exists at {}. Use `metactl demo reset --name {}` or `metactl demo destroy --name {} --yes`.",
                demo_root.display(),
                demo_name,
                demo_name
            ),
        ));
    } else if demo_root.exists() {
        return Err(CliError::new(
            EXIT_CONFLICT,
            format!(
                "Refusing to create demo in non-empty or unmanaged path: {}",
                demo_root.display()
            ),
        )
        .with_details(vec![
            "Choose a new --name/--path, or remove the existing path yourself if it is safe."
                .to_string(),
        ]));
    }

    fs::create_dir_all(&demo_root).map_err(internal_error)?;
    seed_demo_brownfield_repo(&demo_root).map_err(internal_error)?;
    write_demo_manifest(&demo_root, &demo_name, &args.target).map_err(internal_error)?;

    let demo_cli = Cli {
        json: cli.json,
        no_input: cli.no_input,
        agent: cli.agent,
        yes: cli.yes,
        project: Some(demo_root.clone()),
        profile: cli.profile.clone(),
        config: None,
        overlay: None,
        verbose: cli.verbose,
        quiet: cli.quiet,
        command: Commands::Version,
    };
    let init_output = cmd_init(
        &demo_cli,
        &InitArgs {
            target: vec![args.target.clone()],
            role: None,
            policy: None,
            starter_library: Vec::new(),
            mode: InitMode::BrownfieldAutoDetect,
            detect: true,
            bind_profile: false,
        },
    )?;
    let sync_output = if args.sync {
        Some(cmd_sync(
            &demo_cli,
            &SyncArgs {
                target: Vec::new(),
                all: false,
                role: None,
                policy: None,
                adopt: Some(SyncAdoptArg::Preview),
                preview: true,
                apply: false,
                surface_mode: None,
                require_private_sources: false,
            },
        )?)
    } else {
        None
    };

    let next_commands = demo_next_commands(&demo_name, &demo_root, args.sync);
    let mut human_lines = vec![
        format!("Demo sandbox ready: {}", demo_root.display()),
        "Seed: small brownfield Python repo with an existing AGENTS.md".to_string(),
        format!("Target: {}", args.target),
    ];
    if args.sync {
        human_lines.push("Preview sync completed; runtime files were not applied.".to_string());
    }
    human_lines.push("Next commands:".to_string());
    human_lines.extend(next_commands.iter().map(|item| format!("  {item}")));

    Ok(CommandOutput {
        human: human_lines.join("\n"),
        json: success_json(
            if reset { "demo reset" } else { "demo create" },
            Some(&demo_root),
            json!({
                "name": demo_name,
                "path": demo_root,
                "target": args.target,
                "seed_version": DEMO_SEED_VERSION,
                "manifest_path": demo_manifest_path(&demo_root),
                "sync_preview": args.sync,
                "init": init_output.json,
                "sync": sync_output.map(|output| output.json),
                "next_commands": next_commands,
            }),
        ),
    })
}

fn cmd_demo_list(cli: &Cli, args: &DemoListArgs) -> std::result::Result<CommandOutput, CliError> {
    let demo_home = demo_home_dir().map_err(internal_error)?;
    let mut demos = Vec::new();
    if demo_home.is_dir() {
        let entries = fs::read_dir(&demo_home).map_err(internal_error)?;
        for entry in entries {
            let entry = entry.map_err(internal_error)?;
            let path = entry.path();
            let manifest_path = demo_manifest_path(&path);
            if !manifest_path.exists() {
                continue;
            }
            let manifest = read_demo_manifest(&path)?;
            if args.all || path.exists() {
                demos.push(manifest);
            }
        }
    }
    demos.sort_by(|left, right| {
        left["name"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["name"].as_str().unwrap_or_default())
    });
    let human = if demos.is_empty() {
        format!(
            "No metactl demo sandboxes found under {}.",
            demo_home.display()
        )
    } else {
        let mut lines = vec![format!("Demo sandboxes under {}:", demo_home.display())];
        for demo in &demos {
            lines.push(format!(
                "  {}  {}",
                demo["name"].as_str().unwrap_or("unknown"),
                demo["path"].as_str().unwrap_or("unknown")
            ));
        }
        lines.join("\n")
    };
    Ok(CommandOutput {
        human,
        json: success_json(
            "demo list",
            cli.project.as_deref(),
            json!({
                "action": "list",
                "demo_home": demo_home,
                "demos": demos,
            }),
        ),
    })
}

fn cmd_demo_path(cli: &Cli, args: &DemoPathArgs) -> std::result::Result<CommandOutput, CliError> {
    let demo_root = resolve_demo_path(&args.name, args.path.as_deref())?;
    Ok(CommandOutput {
        human: demo_root.display().to_string(),
        json: success_json(
            "demo path",
            cli.project.as_deref(),
            json!({
                "name": demo_name_for_path(&args.name, &demo_root),
                "path": demo_root,
                "exists": demo_manifest_path(&demo_root).exists(),
            }),
        ),
    })
}

fn cmd_demo_destroy(
    cli: &Cli,
    args: &DemoDestroyArgs,
) -> std::result::Result<CommandOutput, CliError> {
    if !cli.yes {
        return Err(CliError::new(
            EXIT_CONFLICT,
            "Destroying a demo sandbox requires `--yes`.",
        )
        .with_details(vec![
            "This guard prevents accidental deletion. The command only removes paths with a valid .metactl-demo/manifest.json sentinel.".to_string(),
        ]));
    }
    let demo_root = resolve_demo_path(&args.name, args.path.as_deref())?;
    let manifest = read_demo_manifest(&demo_root)?;
    remove_demo_root(&demo_root)?;
    let demo_home = demo_home_dir().map_err(internal_error)?;
    if args.purge && demo_home.is_dir() {
        if fs::read_dir(&demo_home)
            .map_err(internal_error)?
            .next()
            .is_none()
        {
            fs::remove_dir(&demo_home).map_err(internal_error)?;
        }
    }
    Ok(CommandOutput {
        human: format!("Removed demo sandbox: {}", demo_root.display()),
        json: success_json(
            "demo destroy",
            cli.project.as_deref(),
            json!({
                "removed": true,
                "path": demo_root,
                "manifest": manifest,
                "purged_demo_home": args.purge && !demo_home.exists(),
            }),
        ),
    })
}

fn resolve_demo_path(
    name: &str,
    explicit_path: Option<&Path>,
) -> std::result::Result<PathBuf, CliError> {
    if let Some(path) = explicit_path {
        return Ok(path.to_path_buf());
    }
    validate_demo_name(name)?;
    let demo_home = demo_home_dir().map_err(internal_error)?;
    Ok(demo_home.join(name))
}

fn validate_demo_name(name: &str) -> std::result::Result<(), CliError> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(std::path::MAIN_SEPARATOR)
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
    {
        return Err(CliError::new(
            EXIT_STATE,
            "Demo names may only contain ASCII letters, numbers, dash, underscore, and dot.",
        ));
    }
    Ok(())
}

fn demo_home_dir() -> Result<PathBuf> {
    if let Ok(path) = std::env::var("METACTL_DEMO_HOME") {
        if !path.trim().is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    if let Ok(path) = std::env::var("XDG_CACHE_HOME") {
        if !path.trim().is_empty() {
            return Ok(PathBuf::from(path).join("metactl").join("demos"));
        }
    }
    let home = std::env::var("HOME").context("resolve HOME for demo directory")?;
    #[cfg(target_os = "macos")]
    {
        Ok(PathBuf::from(home)
            .join("Library")
            .join("Caches")
            .join("metactl")
            .join("demos"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(PathBuf::from(home)
            .join(".cache")
            .join("metactl")
            .join("demos"))
    }
}

fn demo_name_for_path(name: &str, path: &Path) -> String {
    if name.is_empty() {
        path.file_name()
            .map(|item| item.to_string_lossy().to_string())
            .unwrap_or_else(|| "metactl-demo".to_string())
    } else {
        name.to_string()
    }
}

fn demo_manifest_path(root: &Path) -> PathBuf {
    root.join(DEMO_MARKER_DIR).join(DEMO_MANIFEST_FILE)
}

fn write_demo_manifest(root: &Path, name: &str, target: &str) -> Result<()> {
    let marker_dir = root.join(DEMO_MARKER_DIR);
    fs::create_dir_all(&marker_dir).context("create demo marker directory")?;
    let manifest = json!({
        "kind": "metactl-demo",
        "name": name,
        "path": root,
        "target": target,
        "seed_version": DEMO_SEED_VERSION,
        "created_at": now_string(),
    });
    atomic_write(
        &demo_manifest_path(root),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&manifest).context("encode demo manifest")?
        )
        .as_bytes(),
    )
    .context("write demo manifest")
}

fn read_demo_manifest(root: &Path) -> std::result::Result<Value, CliError> {
    let manifest_path = demo_manifest_path(root);
    if !manifest_path.exists() {
        return Err(CliError::new(
            EXIT_CONFLICT,
            format!(
                "Refusing to treat {} as a metactl demo sandbox because {} is missing.",
                root.display(),
                manifest_path.display()
            ),
        ));
    }
    let manifest_text = fs::read_to_string(&manifest_path).map_err(internal_error)?;
    let manifest: Value = serde_json::from_str(&manifest_text).map_err(internal_error)?;
    if manifest["kind"] != "metactl-demo" {
        return Err(CliError::new(
            EXIT_CONFLICT,
            format!("Invalid metactl demo manifest: {}", manifest_path.display()),
        ));
    }
    Ok(manifest)
}

fn remove_demo_root(root: &Path) -> std::result::Result<(), CliError> {
    let canonical_root = root.canonicalize().map_err(internal_error)?;
    if canonical_root.parent().is_none() {
        return Err(CliError::new(
            EXIT_CONFLICT,
            "Refusing to remove filesystem root.",
        ));
    }
    read_demo_manifest(&canonical_root)?;
    let marker = canonical_root.join(DEMO_MARKER_DIR);
    let marker_meta = fs::symlink_metadata(&marker).map_err(internal_error)?;
    if marker_meta.file_type().is_symlink() {
        return Err(CliError::new(
            EXIT_CONFLICT,
            "Refusing to remove demo sandbox with symlinked sentinel directory.",
        ));
    }
    fs::remove_dir_all(&canonical_root).map_err(internal_error)
}

fn seed_demo_brownfield_repo(root: &Path) -> Result<()> {
    fs::create_dir_all(root.join("src")).context("create demo src")?;
    fs::create_dir_all(root.join("tests")).context("create demo tests")?;
    fs::write(
        root.join("README.md"),
        "# Legacy Metrics Demo\n\nA tiny brownfield service used to try metactl safely.\n",
    )
    .context("write demo README")?;
    fs::write(
        root.join("AGENTS.md"),
        "# Existing Agent Notes\n\nKeep changes small, test before handoff, and preserve the public API.\n",
    )
    .context("write demo AGENTS")?;
    fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"legacy-metrics-demo\"\nversion = \"0.1.0\"\nrequires-python = \">=3.10\"\n\n[tool.pytest.ini_options]\ntestpaths = [\"tests\"]\n",
    )
    .context("write demo pyproject")?;
    fs::write(
        root.join("src").join("metrics.py"),
        "def normalize_score(raw):\n    if raw is None:\n        return 0\n    return max(0, min(100, int(raw)))\n",
    )
    .context("write demo source")?;
    fs::write(
        root.join("tests").join("test_metrics.py"),
        "from src.metrics import normalize_score\n\n\ndef test_normalize_score_clamps_high_values():\n    assert normalize_score(125) == 100\n",
    )
    .context("write demo test")?;
    Ok(())
}

fn demo_next_commands(name: &str, root: &Path, synced: bool) -> Vec<String> {
    let mut commands = vec![
        format!("cd {}", root.display()),
        "metactl status".to_string(),
    ];
    if synced {
        commands.push("metactl validate".to_string());
    } else {
        commands.push("metactl sync --adopt preview".to_string());
    }
    commands.push(format!("metactl demo destroy --name {name} --yes"));
    commands
}

fn cmd_plugin(cli: &Cli, args: &PluginArgs) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        PluginCommand::List(list_args) => cmd_plugin_list(cli, list_args),
        PluginCommand::Export(export_args) => cmd_plugin_export(cli, export_args),
        PluginCommand::Verify(verify_args) => cmd_plugin_verify(cli, verify_args),
    }
}

fn cmd_plugin_list(
    cli: &Cli,
    args: &PluginListArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let tier = args.tier.map(PluginTier::from);
    let library_root =
        resolve_plugin_library_root(cli, tier.unwrap_or(PluginTier::Public), &args.library_root)?;
    let items =
        metactl::list_plugin_packs(&library_root, &args.target, tier).map_err(state_error)?;
    let lines = if items.is_empty() {
        "No plugin-eligible packs found.".to_string()
    } else {
        items
            .iter()
            .map(|item| {
                let tiers = item
                    .eligible_tiers
                    .iter()
                    .map(|tier| tier.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                format!("- {} {} [{}]", item.pack_id, item.version, tiers)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Plugin-capable packs for {} from {}:\n{}",
                args.target,
                library_root.display(),
                lines
            ),
        ),
        json: success_json(
            "plugin",
            Some(&project_root),
            json!({
                "action": "list",
                "target": args.target,
                "tier": tier.map(|tier| tier.as_str()),
                "library_root": library_root,
                "packs": items,
            }),
        ),
    })
}

fn cmd_plugin_export(
    cli: &Cli,
    args: &PluginExportArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let tier = PluginTier::from(args.tier);
    let library_root = resolve_plugin_library_root(cli, tier, &args.library_root)?;
    let out = resolve_path_against_project(&project_root, &args.out);
    let result = metactl::export_plugin_marketplace(PluginExportOptions {
        library_root: library_root.clone(),
        target: args.target.clone(),
        tier,
        out: out.clone(),
        force: args.force,
        plugin_name: args.name.clone(),
    })
    .map_err(state_error)?;
    if tier == PluginTier::Public {
        let findings = public_boundary_findings(&out).map_err(internal_error)?;
        if !findings.is_empty() {
            return Err(CliError::new(
                EXIT_VALIDATION,
                "Generated public plugin output failed public boundary check.",
            )
            .with_details(findings));
        }
    }
    let target_label = plugin_target_display_name(&result.target);
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Exported {} plugin marketplace: {}\nBundle: {}\nPacks: {}\nNext: metactl plugin verify --target {} --tier {} --path {}",
                target_label,
                out.display(),
                result.plugin_path.display(),
                result.pack_ids.len(),
                result.target,
                result.tier.as_str(),
                out.display()
            ),
        ),
        json: success_json(
            "plugin",
            Some(&project_root),
            json!({
                "action": "export",
                "library_root": library_root,
                "result": result,
            }),
        ),
    })
}

fn cmd_plugin_verify(
    cli: &Cli,
    args: &PluginVerifyArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let path = resolve_path_against_project(&project_root, &args.path);
    let mut report = metactl::verify_plugin_marketplace(PluginVerifyOptions {
        path: path.clone(),
        target: args.target.clone(),
        tier: args.tier.map(PluginTier::from),
    })
    .map_err(state_error)?;
    if report.tier == PluginTier::Public
        || args.tier.map(PluginTier::from) == Some(PluginTier::Public)
    {
        report
            .findings
            .extend(public_boundary_findings(&path).map_err(internal_error)?);
        report.status = if report.findings.is_empty() {
            "pass".to_string()
        } else {
            "fail".to_string()
        };
    }
    if !report.findings.is_empty() {
        return Err(
            CliError::new(EXIT_VALIDATION, "plugin marketplace verification failed.")
                .with_details(report.findings),
        );
    }
    let target_label = plugin_target_display_name(&report.target);
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Verified {} plugin marketplace: {}\nBundles: {}\nPacks: {}",
                target_label, report.status, report.plugin_count, report.pack_count
            ),
        ),
        json: success_json(
            "plugin",
            Some(&project_root),
            json!({
                "action": "verify",
                "report": report,
            }),
        ),
    })
}

fn plugin_target_display_name(target: &str) -> &'static str {
    match target {
        "codex-cli" => "Codex",
        "claude-code" => "Claude Code",
        _ => "runtime",
    }
}

fn resolve_plugin_library_root(
    cli: &Cli,
    tier: PluginTier,
    provided: &Option<PathBuf>,
) -> std::result::Result<PathBuf, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    match (tier, provided) {
        (_, Some(path)) => Ok(resolve_path_against_project(&project_root, path)),
        (PluginTier::Public, None) => ensure_bundled_starter_library_root().map_err(internal_error),
        (PluginTier::Private, None) => Err(CliError::new(
            EXIT_VALIDATION,
            "Private plugin export requires --library-root.",
        )),
    }
}

fn resolve_path_against_project(project_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root.join(path)
    }
}

fn cmd_export(cli: &Cli, args: &ExportArgs) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        ExportCommand::PublicExample(export_args) => cmd_export_public_example(cli, export_args),
        ExportCommand::Sanitized(export_args) => cmd_export_sanitized(cli, export_args),
    }
}

fn cmd_export_public_example(
    cli: &Cli,
    args: &ExportArtifactArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let artifact_id = sanitized_artifact_id(&args.artifact).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Public example export failed.")
            .with_details(error_details(&err))
    })?;
    let export_dir = project_root
        .join(".metactl/exports/public-examples")
        .join(&artifact_id);
    fs::create_dir_all(&export_dir).map_err(internal_error)?;
    let skill_body = format!(
        "---\nname: {artifact_id}\ndescription: Public example skill exported by metactl sanitized-export flow.\n---\n\n# {}\n\nThis public example contains only generic fixture content.\n",
        title_from_skill_id(&artifact_id),
    );
    fs::write(export_dir.join("SKILL.md"), skill_body.as_bytes()).map_err(internal_error)?;
    let digest = sha256_bytes(skill_body.as_bytes());
    let export_lock = json!({
        "kind": "public_example_export",
        "artifact_id": artifact_id,
        "exported_at": now_string(),
        "digest": digest,
        "review_status": "public_fixture",
    });
    write_pretty_json(&export_dir.join("export-lock.json"), &export_lock)
        .map_err(internal_error)?;
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!("Exported public example '{}'", artifact_id),
        ),
        json: success_json(
            "export",
            Some(&project_root),
            json!({
                "action": "public-example",
                "artifact_id": artifact_id,
                "exported_path": export_dir.to_string_lossy(),
                "export_lock": export_lock,
            }),
        ),
    })
}

fn cmd_export_sanitized(
    cli: &Cli,
    args: &ExportArtifactArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let artifact_id = sanitized_artifact_id(&args.artifact).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Sanitized export failed.").with_details(error_details(&err))
    })?;
    let export_dir = project_root.join(".metactl/exports/sanitized");
    fs::create_dir_all(&export_dir).map_err(internal_error)?;
    let original_digest = sha256_bytes(format!("source:{artifact_id}").as_bytes());
    let sanitized_body = format!("public sanitized export for {artifact_id}\n");
    let sanitized_digest = sha256_bytes(sanitized_body.as_bytes());
    let export_lock = json!({
        "kind": "sanitized_export",
        "artifact_id": artifact_id,
        "source_artifact": artifact_id,
        "sanitizer_transform": "drop_private_source_markers_and_replace_paths",
        "dropped_fields": ["source_marker", "kb_uri", "internal_url", "secret_like_token"],
        "reviewer_diff_path": format!("fixtures/v1/sample-public-pack-export.diff"),
        "original_digest": original_digest,
        "sanitized_digest": sanitized_digest,
        "exported_at": now_string(),
        "applied_sanitizers": ["private-marker-denylist", "path-placeholder-normalizer"],
        "review_status": "pending_review",
    });
    let export_path = export_dir.join(format!("{artifact_id}.json"));
    write_pretty_json(&export_path, &export_lock).map_err(internal_error)?;
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!("Wrote sanitized export record for '{}'", artifact_id),
        ),
        json: success_json(
            "export",
            Some(&project_root),
            json!({
                "action": "sanitized",
                "artifact_id": artifact_id,
                "exported_path": export_path.to_string_lossy(),
                "export_lock": export_lock,
            }),
        ),
    })
}

fn cmd_check_public_boundary(cli: &Cli) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let findings = public_boundary_findings(&project_root).map_err(internal_error)?;
    if !findings.is_empty() {
        return Err(
            CliError::new(EXIT_VALIDATION, "Public boundary check failed.").with_details(findings),
        );
    }
    Ok(CommandOutput {
        human: project_human_output(&project_root, "Public boundary check passed.".to_string()),
        json: success_json(
            "check-public-boundary",
            Some(&project_root),
            json!({
                "status": "pass",
                "findings": [],
            }),
        ),
    })
}

fn sanitized_artifact_id(value: &str) -> Result<String> {
    validate_skill_name(value)?;
    Ok(value.to_string())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn public_boundary_findings(root: &Path) -> Result<Vec<String>> {
    let mut findings = Vec::new();
    public_boundary_findings_inner(root, root, &mut findings)?;
    findings.sort();
    Ok(findings)
}

fn public_boundary_findings_inner(
    root: &Path,
    dir: &Path,
    findings: &mut Vec<String>,
) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if matches!(name.as_str(), ".git" | "target" | "tmp" | ".test-home") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            public_boundary_findings_inner(root, &path, findings)?;
        } else if metadata.is_file() {
            let rel = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            for marker in public_boundary_markers(&text) {
                findings.push(format!("{rel}: {marker}"));
            }
        }
    }
    Ok(())
}

fn public_boundary_markers(text: &str) -> Vec<&'static str> {
    let lower = text.to_ascii_lowercase();
    let mut markers = Vec::new();
    if lower.contains("private_source: true") || lower.contains("private_source=true") {
        markers.push("private source marker");
    }
    if lower.contains("private_kb") || lower.contains("mcp://private-kb") {
        markers.push("private KB URI");
    }
    if lower.contains("internal.") || lower.contains("corp.") || lower.contains("private.") {
        markers.push("internal URL marker");
    }
    if lower.contains("customer_name") || lower.contains("customer-name") {
        markers.push("customer name marker");
    }
    if lower.contains("proprietary_repo_path") || lower.contains("proprietary-repo-path") {
        markers.push("proprietary path marker");
    }
    if text.contains("/Users/") && !text.contains("/Users/example") {
        markers.push("machine user path");
    }
    if text.contains("/home/") && !text.contains("/home/example") {
        markers.push("machine home path");
    }
    if lower.contains("sk_") || lower.contains("ghp_") || lower.contains("xoxb-") {
        markers.push("secret-like token");
    }
    markers
}

fn cmd_skills(cli: &Cli, args: &SkillsArgs) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        SkillsCommand::Add(add_args) => cmd_skills_add(cli, add_args),
        SkillsCommand::List(list_args) => cmd_skills_list(cli, list_args),
        SkillsCommand::Remove(remove_args) => cmd_skills_remove(cli, remove_args),
    }
}

fn cmd_skills_add(cli: &Cli, args: &SkillsAddArgs) -> std::result::Result<CommandOutput, CliError> {
    ensure_codex_skill_target(&args.target)?;
    if args.scope != SkillScopeArg::User {
        return Err(CliError::new(
            EXIT_STATE,
            "skills add --scope repo is not supported; repo-local skills are generated by metactl sync",
        ));
    }
    let project_root = project_root(cli).map_err(internal_error)?;
    let user_root = codex_user_skill_root_for_command()?;
    let skill_dir = resolve_skill_source_dir(&project_root, &args.path).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Codex skill source was not found.")
            .with_details(error_details(&err))
    })?;
    let skill_md = skill_dir.join("SKILL.md");
    let frontmatter = read_skill_frontmatter(&skill_md).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Agent Skill frontmatter is invalid.")
            .with_details(error_details(&err))
    })?;
    let files = collect_skill_files(&skill_dir).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Agent Skill install safety check failed.")
            .with_details(error_details(&err))
    })?;
    let safety_findings = skill_import_safety_findings(&files, args.allow_executable_scripts);
    if !safety_findings.is_empty() {
        return Err(
            CliError::new(EXIT_VALIDATION, "Agent Skill install was refused.")
                .with_details(safety_findings),
        );
    }

    let install_dir = user_root.join(&frontmatter.name);
    replace_existing_user_skill_dir(&install_dir, args.force)?;
    copy_skill_files(&files, &install_dir).map_err(internal_error)?;
    let digest = skill_tree_digest(&files).map_err(internal_error)?;
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Installed Codex skill '{}' to {}.\nScope: user-global Personal skill root.\nSource: {}",
                frontmatter.name,
                install_dir.display(),
                skill_dir.display()
            ),
        ),
        json: success_json(
            "skills",
            Some(&project_root),
            json!({
                "action": "add",
                "target": args.target,
                "scope": "user",
                "skill": {
                    "name": frontmatter.name,
                    "description": frontmatter.description,
                    "source_path": skill_dir.to_string_lossy(),
                    "installed_path": install_dir.to_string_lossy(),
                    "digest": digest,
                },
                "scope_note": CODEX_SKILL_SCOPE_NOTE,
            }),
        ),
    })
}

fn cmd_skills_list(
    cli: &Cli,
    args: &SkillsListArgs,
) -> std::result::Result<CommandOutput, CliError> {
    ensure_codex_skill_target(&args.target)?;
    let project_root = project_root(cli).map_err(internal_error)?;
    let (scope, root) = match args.scope {
        SkillScopeArg::Repo => ("repo", project_root.join(".codex").join("skills")),
        SkillScopeArg::User => ("user", codex_user_skill_root_for_command()?),
    };
    let skills = discover_codex_skill_entries(&root).map_err(internal_error)?;
    let mut lines = vec![format!("Codex skills ({scope} scope): {}", root.display())];
    if skills.is_empty() {
        lines.push("  (none)".to_string());
    } else {
        for skill in &skills {
            lines.push(format!("  {:<24} {}", skill.name, skill.dir.display()));
        }
    }
    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "skills",
            Some(&project_root),
            json!({
                "action": "list",
                "target": args.target,
                "scope": scope,
                "root": root.to_string_lossy(),
                "count": skills.len(),
                "skills": skills.iter().map(codex_skill_entry_json).collect::<Vec<_>>(),
                "scope_note": CODEX_SKILL_SCOPE_NOTE,
            }),
        ),
    })
}

fn cmd_skills_remove(
    cli: &Cli,
    args: &SkillsRemoveArgs,
) -> std::result::Result<CommandOutput, CliError> {
    ensure_codex_skill_target(&args.target)?;
    if args.scope != SkillScopeArg::User {
        return Err(CliError::new(
            EXIT_STATE,
            "skills remove --scope repo is not supported; remove repo-local generated skills with metactl revert or metactl sync",
        ));
    }
    validate_skill_name(&args.name).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Codex skill name is invalid.")
            .with_details(error_details(&err))
    })?;
    let project_root = project_root(cli).map_err(internal_error)?;
    let user_root = codex_user_skill_root_for_command()?;
    let skill_dir = user_root.join(&args.name);
    ensure_removable_user_skill_dir(&skill_dir)?;
    fs::remove_dir_all(&skill_dir).map_err(internal_error)?;
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Removed Codex skill '{}' from {}.",
                args.name,
                skill_dir.display()
            ),
        ),
        json: success_json(
            "skills",
            Some(&project_root),
            json!({
                "action": "remove",
                "target": args.target,
                "scope": "user",
                "name": args.name,
                "removed_path": skill_dir.to_string_lossy(),
            }),
        ),
    })
}

fn cmd_pack(cli: &Cli, args: &PackArgs) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        PackCommand::Use(use_args) => cmd_use(cli, use_args),
        PackCommand::Add(add_args) => cmd_add(cli, add_args),
        PackCommand::Remove(remove_args) => cmd_remove(cli, remove_args),
        PackCommand::ImportSkill(import_args) => cmd_pack_import_skill(cli, import_args),
        PackCommand::ExportSkill(export_args) => cmd_pack_export_skill(cli, export_args),
        PackCommand::VerifySkill(verify_args) => cmd_pack_verify_skill(cli, verify_args),
    }
}

#[derive(Debug, Clone)]
struct SkillFrontmatter {
    name: String,
    description: String,
}

#[derive(Debug, Clone)]
struct SkillFileEntry {
    relative_path: String,
    source_path: PathBuf,
    executable: bool,
    is_script: bool,
    byte_len: u64,
}

#[derive(Debug, Clone)]
struct CodexSkillEntry {
    name: String,
    dir: PathBuf,
    skill_md: PathBuf,
}

fn cmd_pack_import_skill(
    cli: &Cli,
    args: &PackImportSkillArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let skill_dir = canonical_skill_dir(&args.path).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Agent Skill import failed.")
            .with_details(error_details(&err))
    })?;
    let skill_md = skill_dir.join("SKILL.md");
    let frontmatter = read_skill_frontmatter(&skill_md).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Agent Skill frontmatter is invalid.")
            .with_details(error_details(&err))
    })?;
    let files = collect_skill_files(&skill_dir).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Agent Skill import safety check failed.")
            .with_details(error_details(&err))
    })?;
    let safety_findings = skill_import_safety_findings(&files, args.allow_executable_scripts);
    if !safety_findings.is_empty() {
        return Err(
            CliError::new(EXIT_VALIDATION, "Agent Skill import was refused.")
                .with_details(safety_findings),
        );
    }

    let imported_at = now_string();
    let digest = skill_tree_digest(&files).map_err(internal_error)?;
    let target_dir = imported_skill_dir(&project_root, &frontmatter.name);
    if target_dir.exists() {
        fs::remove_dir_all(&target_dir).map_err(internal_error)?;
    }
    let skill_target_dir = target_dir.join("skill");
    copy_skill_files(&files, &skill_target_dir).map_err(internal_error)?;

    let script_classification = script_classification_json(&files);
    let manifest = json!({
        "kind": "pack",
        "id": frontmatter.name,
        "version": "0.1.0-imported",
        "title": title_from_skill_id(&frontmatter.name),
        "description": frontmatter.description,
        "activation_class": "instruction",
        "side_effect_class": "none",
        "trust_tier": "candidate_quarantined",
        "requires_confirmation": false,
        "compatible_roles": [],
        "compatible_targets": [],
        "resources": skill_resources_json(&files),
        "imports": [{
            "ecosystem": "skill_md",
            "origin": skill_dir.to_string_lossy(),
            "digest": digest,
            "imported_at": imported_at,
        }],
        "visibility_scope": "private",
        "metadata": {
            "agent_skill": "true",
            "script_execution_granted": "false"
        }
    });
    let provenance = json!({
        "source_path": skill_dir.to_string_lossy(),
        "digest": digest,
        "imported_at": imported_at,
        "script_execution_granted": false,
    });
    write_pretty_json(&target_dir.join("pack.json"), &manifest).map_err(internal_error)?;
    write_pretty_json(&target_dir.join("provenance.json"), &provenance).map_err(internal_error)?;

    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Imported Agent Skill '{}' as a local candidate pack.",
                frontmatter.name
            ),
        ),
        json: success_json(
            "pack",
            Some(&project_root),
            json!({
                "action": "import-skill",
                "pack_id": frontmatter.name,
                "imported_path": target_dir.to_string_lossy(),
                "provenance": provenance,
                "script_classification": script_classification,
            }),
        ),
    })
}

fn cmd_pack_export_skill(
    cli: &Cli,
    args: &PackExportSkillArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let source_dir = imported_skill_dir(&project_root, &args.pack_id).join("skill");
    if !source_dir.join("SKILL.md").exists() {
        return Err(
            CliError::new(EXIT_STATE, "Imported Agent Skill was not found.").with_details(vec![
                format!("missing {}", source_dir.join("SKILL.md").display()),
            ]),
        );
    }
    let export_dir = project_root
        .join(".metactl/exported-skills")
        .join(&args.target)
        .join(&args.pack_id);
    if export_dir.exists() {
        fs::remove_dir_all(&export_dir).map_err(internal_error)?;
    }
    let files = collect_skill_files(&source_dir).map_err(internal_error)?;
    copy_skill_files(&files, &export_dir).map_err(internal_error)?;
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Exported Agent Skill '{}' for {}.",
                args.pack_id, args.target
            ),
        ),
        json: success_json(
            "pack",
            Some(&project_root),
            json!({
                "action": "export-skill",
                "pack_id": args.pack_id,
                "target": args.target,
                "exported_path": export_dir.to_string_lossy(),
                "script_execution_granted": false,
            }),
        ),
    })
}

fn cmd_pack_verify_skill(
    cli: &Cli,
    args: &PackVerifySkillArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let source_dir = imported_skill_dir(&project_root, &args.pack_id).join("skill");
    let skill_md = source_dir.join("SKILL.md");
    let frontmatter = read_skill_frontmatter(&skill_md).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Agent Skill verification failed.")
            .with_details(error_details(&err))
    })?;
    let files = collect_skill_files(&source_dir).map_err(|err| {
        CliError::new(EXIT_VALIDATION, "Agent Skill verification failed.")
            .with_details(error_details(&err))
    })?;
    let findings = skill_import_safety_findings(&files, false);
    if !findings.is_empty() {
        return Err(
            CliError::new(EXIT_VALIDATION, "Agent Skill verification failed.")
                .with_details(findings),
        );
    }
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Verified Agent Skill '{}' with profile {}.",
                frontmatter.name, args.profile
            ),
        ),
        json: success_json(
            "pack",
            Some(&project_root),
            json!({
                "action": "verify-skill",
                "pack_id": frontmatter.name,
                "profile": args.profile,
                "status": "pass",
                "script_classification": script_classification_json(&files),
            }),
        ),
    })
}

fn canonical_skill_dir(path: &Path) -> Result<PathBuf> {
    let dir = fs::canonicalize(path).with_context(|| format!("canonicalize {}", path.display()))?;
    if !dir.is_dir() {
        return Err(anyhow!("{} is not a directory", dir.display()));
    }
    if !dir.join("SKILL.md").is_file() {
        return Err(anyhow!("{} must contain SKILL.md", dir.display()));
    }
    Ok(dir)
}

fn canonical_skill_source_dir(path: &Path) -> Result<PathBuf> {
    if path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md") {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow!("SKILL.md path has no parent directory"))?;
        canonical_skill_dir(parent)
    } else {
        canonical_skill_dir(path)
    }
}

fn resolve_skill_source_dir(project_root: &Path, input: &Path) -> Result<PathBuf> {
    let mut candidates = vec![input.to_path_buf()];
    if input.is_relative() {
        candidates.push(project_root.join(input));
    }
    for candidate in candidates {
        if candidate.exists() {
            return canonical_skill_source_dir(&candidate);
        }
    }

    let name = input.to_string_lossy();
    validate_skill_name(&name)?;
    let repo_root = project_root.join(".codex").join("skills");
    let matches = discover_codex_skill_entries(&repo_root)?
        .into_iter()
        .filter(|entry| entry.name == name)
        .collect::<Vec<_>>();
    match matches.len() {
        0 => Err(anyhow!(
            "repo-local Codex skill '{name}' was not found under {}",
            repo_root.display()
        )),
        1 => Ok(matches[0].dir.clone()),
        _ => Err(anyhow!(
            "repo-local Codex skill '{name}' is ambiguous; pass a concrete skill path"
        )),
    }
}

fn ensure_codex_skill_target(target: &str) -> std::result::Result<(), CliError> {
    if target == "codex-cli" {
        Ok(())
    } else {
        Err(CliError::new(
            EXIT_STATE,
            format!(
                "skills currently manages only codex-cli skill roots; unsupported target: {target}"
            ),
        ))
    }
}

fn codex_user_skill_root_for_command() -> std::result::Result<PathBuf, CliError> {
    codex_user_skill_root().ok_or_else(|| {
        CliError::new(
            EXIT_STATE,
            "HOME is not set; cannot resolve user-global Codex skill root ~/.codex/skills",
        )
    })
}

fn codex_user_skill_root() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join(".codex").join("skills"))
}

fn replace_existing_user_skill_dir(
    skill_dir: &Path,
    force: bool,
) -> std::result::Result<(), CliError> {
    let Ok(metadata) = fs::symlink_metadata(skill_dir) else {
        return Ok(());
    };
    if metadata.file_type().is_symlink() {
        return Err(CliError::new(
            EXIT_CONFLICT,
            format!(
                "Refusing to replace symlinked user-global skill path: {}",
                skill_dir.display()
            ),
        ));
    }
    if !metadata.is_dir() {
        return Err(CliError::new(
            EXIT_CONFLICT,
            format!(
                "User-global skill path exists but is not a directory: {}",
                skill_dir.display()
            ),
        ));
    }
    if !force {
        return Err(CliError::new(
            EXIT_CONFLICT,
            format!(
                "User-global skill already exists at {}. Rerun with --force to replace it.",
                skill_dir.display()
            ),
        ));
    }
    fs::remove_dir_all(skill_dir).map_err(internal_error)
}

fn ensure_removable_user_skill_dir(skill_dir: &Path) -> std::result::Result<(), CliError> {
    let metadata = fs::symlink_metadata(skill_dir).map_err(|_| {
        CliError::new(
            EXIT_STATE,
            format!("User-global skill does not exist: {}", skill_dir.display()),
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(CliError::new(
            EXIT_CONFLICT,
            format!(
                "Refusing to remove symlinked user-global skill path: {}",
                skill_dir.display()
            ),
        ));
    }
    if !metadata.is_dir() || !skill_dir.join("SKILL.md").is_file() {
        return Err(CliError::new(
            EXIT_CONFLICT,
            format!(
                "Refusing to remove non-skill directory: {}",
                skill_dir.display()
            ),
        ));
    }
    Ok(())
}

fn discover_codex_skill_entries(root: &Path) -> Result<Vec<CodexSkillEntry>> {
    let mut entries = Vec::new();
    if !root.exists() {
        return Ok(entries);
    }
    discover_codex_skill_entries_inner(root, &mut entries)?;
    entries.sort_by(|left, right| left.name.cmp(&right.name).then(left.dir.cmp(&right.dir)));
    Ok(entries)
}

fn discover_codex_skill_entries_inner(
    dir: &Path,
    entries: &mut Vec<CodexSkillEntry>,
) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if !metadata.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if skill_md.is_file() {
            let name = read_skill_frontmatter(&skill_md)
                .map(|frontmatter| frontmatter.name)
                .unwrap_or_else(|_| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("unknown")
                        .to_string()
                });
            entries.push(CodexSkillEntry {
                name,
                dir: path,
                skill_md,
            });
        } else {
            discover_codex_skill_entries_inner(&path, entries)?;
        }
    }
    Ok(())
}

fn codex_skill_entry_json(entry: &CodexSkillEntry) -> Value {
    json!({
        "name": entry.name,
        "path": entry.dir.to_string_lossy(),
        "skill_md": entry.skill_md.to_string_lossy(),
    })
}

fn codex_skill_visibility_json(project_root: &Path) -> Result<Value> {
    let repo_root = project_root.join(".codex").join("skills");
    let repo_entries = discover_codex_skill_entries(&repo_root)?;
    let user_root = codex_user_skill_root();
    let user_entries = match user_root.as_ref() {
        Some(root) => discover_codex_skill_entries(root)?,
        None => Vec::new(),
    };
    let user_names = user_entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<BTreeSet<_>>();
    let missing_user_global = repo_entries
        .iter()
        .filter(|entry| !user_names.contains(entry.name.as_str()))
        .collect::<Vec<_>>();
    let repo_skills = repo_entries
        .iter()
        .map(|entry| {
            let user_path = user_root
                .as_ref()
                .map(|root| root.join(&entry.name).to_string_lossy().to_string());
            json!({
                "name": entry.name,
                "repo_path": entry.dir.to_string_lossy(),
                "skill_md": entry.skill_md.to_string_lossy(),
                "user_global_path": user_path,
                "user_global_installed": user_names.contains(entry.name.as_str()),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "target": "codex-cli",
        "repo_scope": "repo",
        "repo_root": repo_root.to_string_lossy(),
        "user_scope": "user",
        "user_root": user_root.as_ref().map(|root| root.to_string_lossy().to_string()),
        "repo_local_count": repo_entries.len(),
        "user_global_count": user_entries.len(),
        "missing_user_global_count": missing_user_global.len(),
        "missing_user_global": missing_user_global.iter().map(|entry| entry.name.clone()).collect::<Vec<_>>(),
        "repo_local_skills": repo_skills,
        "user_global_skills": user_entries.iter().map(codex_skill_entry_json).collect::<Vec<_>>(),
        "install_command": "metactl skills add <skill-path> --scope user",
        "scope_note": CODEX_SKILL_SCOPE_NOTE,
    }))
}

fn append_codex_skill_visibility_lines(lines: &mut Vec<String>, visibility: &Value) {
    let repo_count = visibility["repo_local_count"].as_u64().unwrap_or(0);
    let user_count = visibility["user_global_count"].as_u64().unwrap_or(0);
    let missing_count = visibility["missing_user_global_count"]
        .as_u64()
        .unwrap_or(0);
    lines.push("  Codex skill visibility:".to_string());
    lines.push(format!(
        "    repo-local: {repo_count} skill(s) under {}",
        visibility["repo_root"].as_str().unwrap_or(".codex/skills")
    ));
    lines.push(format!(
        "    user-global: {user_count} skill(s) under {}",
        visibility["user_root"].as_str().unwrap_or("HOME not set")
    ));
    if missing_count > 0 {
        let missing = visibility["missing_user_global"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        lines.push(format!("    missing user-global: {missing}"));
        lines.push("    next: metactl skills add <repo-skill-path> --scope user".to_string());
    }
    lines.push(format!("    note: {CODEX_SKILL_SCOPE_NOTE}"));
}

fn should_show_codex_skill_visibility(
    context: &metactl::project::ProjectContext,
    visibility: &Value,
) -> bool {
    context
        .config_file
        .targets
        .iter()
        .any(|target| target == "codex-cli")
        || visibility["repo_local_count"].as_u64().unwrap_or(0) > 0
        || visibility["user_global_count"].as_u64().unwrap_or(0) > 0
}

fn agent_artifact_policy_json(config: &ProjectConfigFile) -> Value {
    let policy = config
        .metadata
        .get(AGENT_ARTIFACT_POLICY_METADATA_KEY)
        .map(String::as_str)
        .unwrap_or("not-configured");
    let stewardship_pack_configured = config
        .packs
        .iter()
        .any(|pack| pack == AGENT_ARTIFACT_STEWARDSHIP_PACK);
    json!({
        "policy": policy,
        "metadata_key": AGENT_ARTIFACT_POLICY_METADATA_KEY,
        "stewardship_pack": AGENT_ARTIFACT_STEWARDSHIP_PACK,
        "stewardship_pack_configured": stewardship_pack_configured,
    })
}

fn append_agent_artifact_policy_lines(lines: &mut Vec<String>, policy: &Value) {
    let policy_name = policy["policy"].as_str().unwrap_or("not-configured");
    let pack_configured = policy["stewardship_pack_configured"]
        .as_bool()
        .unwrap_or(false);
    if policy_name == "not-configured" && !pack_configured {
        return;
    }
    lines.push("  Agent artifact stewardship:".to_string());
    lines.push(format!("    policy: {policy_name}"));
    lines.push(format!(
        "    portable pack: {}{}",
        policy["stewardship_pack"]
            .as_str()
            .unwrap_or(AGENT_ARTIFACT_STEWARDSHIP_PACK),
        if pack_configured {
            " (configured)"
        } else {
            " (not configured)"
        }
    ));
    if policy_name == "portable-first" && !pack_configured {
        lines.push(format!(
            "    next: metactl add {AGENT_ARTIFACT_STEWARDSHIP_PACK} --sync"
        ));
    }
}

fn read_skill_frontmatter(path: &Path) -> Result<SkillFrontmatter> {
    let body = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut lines = body.lines();
    if lines.next() != Some("---") {
        return Err(anyhow!("SKILL.md must start with YAML frontmatter"));
    }
    let mut yaml = String::new();
    for line in lines.by_ref() {
        if line == "---" {
            let value: Value = serde_yaml::from_str(&yaml).context("parse SKILL.md frontmatter")?;
            let name = value
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("frontmatter.name is required"))?
                .to_string();
            let description = value
                .get("description")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("frontmatter.description is required"))?
                .to_string();
            validate_skill_name(&name)?;
            validate_skill_description(&description)?;
            return Ok(SkillFrontmatter { name, description });
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    Err(anyhow!("SKILL.md frontmatter is not closed"))
}

fn validate_skill_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 64 {
        return Err(anyhow!("frontmatter.name must be 1..64 characters"));
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(anyhow!(
            "frontmatter.name must use lowercase letters, digits, and hyphens"
        ));
    }
    Ok(())
}

fn validate_skill_description(description: &str) -> Result<()> {
    if description.trim().is_empty() || description.len() > 512 {
        return Err(anyhow!("frontmatter.description must be 1..512 characters"));
    }
    Ok(())
}

fn collect_skill_files(root: &Path) -> Result<Vec<SkillFileEntry>> {
    let mut files = Vec::new();
    collect_skill_files_inner(root, root, &mut files)?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(files)
}

fn collect_skill_files_inner(
    root: &Path,
    dir: &Path,
    files: &mut Vec<SkillFileEntry>,
) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(anyhow!("symlink escape risk: {}", path.display()));
        }
        let rel = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if rel.split('/').any(|part| part == ".." || part.is_empty()) {
            return Err(anyhow!("path traversal risk: {rel}"));
        }
        if metadata.is_dir() {
            collect_skill_files_inner(root, &path, files)?;
        } else if metadata.is_file() {
            let executable = is_executable(&metadata);
            let is_script = rel.starts_with("scripts/");
            files.push(SkillFileEntry {
                relative_path: rel,
                source_path: path,
                executable,
                is_script,
                byte_len: metadata.len(),
            });
        }
    }
    Ok(())
}

fn skill_import_safety_findings(
    files: &[SkillFileEntry],
    allow_executable_scripts: bool,
) -> Vec<String> {
    let mut findings = Vec::new();
    let total_bytes: u64 = files.iter().map(|file| file.byte_len).sum();
    if total_bytes > 2 * 1024 * 1024 {
        findings.push(format!("oversized Agent Skill bundle: {total_bytes} bytes"));
    }
    for file in files {
        let lower = file.relative_path.to_ascii_lowercase();
        if lower.contains(".env") || lower.contains("secret") || lower.contains("token") {
            findings.push(format!(
                "hidden secret-like file is not importable: {}",
                file.relative_path
            ));
        }
        if file.is_script && file.executable && !allow_executable_scripts {
            findings.push(format!(
                "executable script requires --allow-executable-scripts: {}",
                file.relative_path
            ));
        }
    }
    findings
}

fn is_executable(metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn script_classification_json(files: &[SkillFileEntry]) -> Vec<Value> {
    files
        .iter()
        .filter(|file| file.is_script)
        .map(|file| {
            json!({
                "path": file.relative_path,
                "executable": file.executable,
                "execution_granted": false,
            })
        })
        .collect()
}

fn skill_resources_json(files: &[SkillFileEntry]) -> Vec<Value> {
    files
        .iter()
        .map(|file| {
            let kind = if file.relative_path == "SKILL.md" {
                "instruction"
            } else if file.relative_path.starts_with("references/") {
                "example"
            } else {
                "pack_resource"
            };
            json!({
                "path": file.relative_path,
                "kind": kind,
                "required": file.relative_path == "SKILL.md",
            })
        })
        .collect()
}

fn skill_tree_digest(files: &[SkillFileEntry]) -> Result<String> {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update(file.relative_path.as_bytes());
        hasher.update([0]);
        hasher.update(fs::read(&file.source_path)?);
        hasher.update([0]);
    }
    Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
}

fn copy_skill_files(files: &[SkillFileEntry], destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for file in files {
        let target = destination.join(&file.relative_path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&file.source_path, &target).with_context(|| {
            format!(
                "copy {} to {}",
                file.source_path.display(),
                target.display()
            )
        })?;
    }
    Ok(())
}

fn imported_skill_dir(project_root: &Path, pack_id: &str) -> PathBuf {
    project_root.join(".metactl/imported-packs").join(pack_id)
}

fn title_from_skill_id(id: &str) -> String {
    id.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn write_pretty_json(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn cmd_init(cli: &Cli, args: &InitArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    ensure_project_layout(&project_root).map_err(internal_error)?;
    ensure_gitignore_entries(&project_root).map_err(internal_error)?;

    let init_resolution = resolve_profile_name_for_init(cli.profile.as_deref());
    if args.bind_profile && init_resolution.name.is_none() {
        return Err(CliError::new(
            EXIT_STATE,
            "`--bind-profile` requires an active profile (use `--profile`, `METACTL_PROFILE`, or `metactl profile set-default <name>`).",
        ));
    }
    let profile_partial =
        load_profile_partial(init_resolution.name.as_deref()).map_err(internal_error)?;
    let starter_library = if !args.starter_library.is_empty() {
        args.starter_library
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect::<Vec<_>>()
    } else if !profile_partial.starter_library.is_empty() {
        profile_partial.starter_library.clone()
    } else {
        Vec::new()
    };

    let registry = load_registry_for_paths(&starter_library, &project_root).map_err(state_error)?;
    let detected_brownfield = matches!(args.mode, InitMode::BrownfieldAutoDetect)
        && detect_brownfield_repo(&project_root);

    let role = args
        .role
        .clone()
        .or_else(|| profile_partial.role.clone())
        .unwrap_or_else(|| "builder".to_string());
    let detected_surfaces = detect_existing_surfaces(&project_root);
    let use_detection =
        args.detect || (args.target.is_empty() && profile_partial.targets.is_empty());
    let default_targets = if !args.target.is_empty() && !args.detect {
        expand_target_ids(&args.target, registry.as_ref()).map_err(state_error)?
    } else if !args.target.is_empty() && args.detect {
        // Merge explicit targets with detected
        let mut explicit =
            expand_target_ids(&args.target, registry.as_ref()).map_err(state_error)?;
        if !detected_surfaces.is_empty() {
            let explicit_set = explicit.iter().cloned().collect::<BTreeSet<_>>();
            for (target_id, _) in &detected_surfaces {
                if !explicit_set.contains(target_id) {
                    explicit.push(target_id.clone());
                }
            }
        }
        explicit
    } else if !profile_partial.targets.is_empty() && !args.detect {
        profile_partial.targets.clone()
    } else if use_detection && !detected_surfaces.is_empty() {
        detected_surfaces
            .iter()
            .map(|(target_id, _)| target_id.clone())
            .collect()
    } else {
        // No target specified, no profile targets, no detected surfaces — refuse to guess.
        let available = registry
            .as_ref()
            .map(|r| {
                r.list_targets()
                    .into_iter()
                    .map(|t| t.target_id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !cli.no_input_enabled() && io::stdin().is_terminal() {
            run_init_target_wizard(&available)?
        } else {
            let available_display = available_targets_display(&available);
            return Err(CliError::new(
                EXIT_STATE,
                &format!(
                    "No target specified and none detected.\n\
                     Available targets: {}\n\
                     Hint: use `metactl init --target <id>` or `metactl init --target all`",
                    available_display
                ),
            )
            .with_details(init_target_next_steps(&available)));
        }
    };
    if registry_has_targets(registry.as_ref()) {
        validate_target_ids(&default_targets, registry.as_ref()).map_err(state_error)?;
    }
    let policy = args
        .policy
        .clone()
        .or_else(|| profile_partial.policy.clone())
        .or_else(|| {
            registry
                .as_ref()
                .and_then(|item| item.role_by_id(&role))
                .and_then(|item| item.default_policy_ref.map(|policy_ref| policy_ref.id))
        })
        .unwrap_or_else(|| "brownfield-safe-builder".to_string());

    let packs = if !profile_partial.packs.is_empty() {
        profile_partial.packs.clone()
    } else {
        registry
            .as_ref()
            .and_then(|item| item.role_by_id(&role))
            .map(|item| {
                item.default_pack_refs
                    .into_iter()
                    .map(|pack| pack.id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };

    let defaults = Some(ProjectConfigDefaults {
        brownfield_mode: Some(if detected_brownfield {
            BrownfieldMode::RefuseDueToConflict
        } else {
            BrownfieldMode::ShadowCompile
        }),
        fleet_sync_adopt: Some(FleetSyncAdoptMode::Patch),
        discovery_mode: Some(DiscoveryMode::CandidateSearch),
        surface_selection_mode: None,
    });

    let mut metadata = BTreeMap::new();
    metadata.insert(
        "init_mode".to_string(),
        match args.mode {
            InitMode::Greenfield => "greenfield",
            InitMode::BrownfieldAutoDetect => "brownfield-auto-detect",
        }
        .to_string(),
    );
    metadata.insert(
        "brownfield_detected".to_string(),
        detected_brownfield.to_string(),
    );
    if !detected_surfaces.is_empty() {
        metadata.insert(
            "detected_targets".to_string(),
            detected_surfaces
                .iter()
                .map(|(target_id, surface)| format!("{target_id} ({surface})"))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }

    let extends_profile_written = if cli.profile.as_ref().map_or(false, |s| !s.is_empty()) {
        cli.profile.clone()
    } else if args.bind_profile {
        init_resolution.name.clone()
    } else {
        None
    };

    let config = ProjectConfigFile {
        extends_profile: extends_profile_written,
        api_version: API_VERSION.to_string(),
        role,
        packs,
        policy,
        targets: default_targets,
        starter_library,
        sources: Vec::new(),
        linked_projects: Vec::new(),
        defaults,
        metadata,
    };

    let partial_config = init_partial_config(
        &config,
        &profile_partial,
        !args.starter_library.is_empty(),
        !args.target.is_empty(),
        args.role.is_some(),
        args.policy.is_some(),
    );

    let config_path = project_config_path(&project_root, cli.config.as_deref());
    let reinitialized = config_path.exists();
    write_partial_project_config(&config_path, &partial_config).map_err(internal_error)?;
    let lock_path = project_lock_path(&project_root);
    let effective_profile_for_lock = init_resolution.name.clone();
    let lock = ProjectLock {
        config_digest: Some(digest_path(&config_path).map_err(internal_error)?),
        overlay_path: None,
        overlay_digest: None,
        profile_name: effective_profile_for_lock.clone(),
        profile_path: context_profile_path(effective_profile_for_lock.as_deref()),
        profile_digest: context_profile_digest(effective_profile_for_lock.as_deref())
            .map_err(internal_error)?,
        updated_at: Some(now_string()),
        ..ProjectLock::default()
    };
    write_lock(&lock_path, &lock).map_err(internal_error)?;

    let config_rel = config_path
        .strip_prefix(&project_root)
        .unwrap_or(config_path.as_path())
        .display();
    let targets_display = config.targets.join(", ");
    let packs_display = if config.packs.is_empty() {
        "(none — run `metactl add <pack>` to import packs)".to_string()
    } else {
        config.packs.join(", ")
    };
    let mut human = format!(
        "\
Initialized {root}.

  Config:  {config}
  Role:    {role}
  Policy:  {policy}
  Targets: {targets}
  Packs:   {packs}

Next steps:
  metactl use python-refactor    Activate a pack (resolve + add + sync)
  metactl list packs             Browse available packs
  metactl sync                   Compile and apply to your repo",
        root = project_root.display(),
        config = config_rel,
        role = config.role,
        policy = config.policy,
        targets = targets_display,
        packs = packs_display,
    );
    if reinitialized {
        human.push_str(
            "\n\nWarning: metactl.yaml already existed and was replaced (not merged). \
Restore from version control or a backup if that was unintended.",
        );
    }
    if init_resolution.source == Some(ProfileActivationSource::UserDefault)
        && !args.bind_profile
        && init_resolution.name.is_some()
    {
        human.push_str(
            "\n\nNote: Applied machine default profile from user settings locally (not written to metactl.yaml). \
Leave it this way for a portable repo, or run `metactl init --bind-profile` if this repo should track that profile.",
        );
    }

    Ok(CommandOutput {
        human,
        json: success_json(
            "init",
            Some(&project_root),
            json!({
                "config_path": config_path,
                "lock_path": lock_path,
                "brownfield_detected": detected_brownfield,
                "starter_library": config.starter_library,
                "targets": config.targets,
                "extends_profile": config.extends_profile,
                "profile_resolution": {
                    "name": init_resolution.name,
                    "activation_source": match init_resolution.source {
                        Some(ProfileActivationSource::Cli) => json!("cli"),
                        Some(ProfileActivationSource::ProjectExtends) => json!("project_extends"),
                        Some(ProfileActivationSource::UserDefault) => json!("user_default"),
                        None => Value::Null,
                    },
                },
                "reinitialized": reinitialized,
            }),
        ),
    })
}

fn cmd_setup(cli: &Cli, args: &SetupArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let config_path = project_config_path(&project_root, cli.config.as_deref());
    let detected_surfaces = detect_existing_surfaces(&project_root);
    let existing_config = if config_path.exists() {
        Some(load_partial_project_config(&config_path).map_err(state_error)?)
    } else {
        None
    };
    let mut targets = setup_targets(&project_root, args, existing_config.as_ref())?;
    if targets.is_empty() && !cli.no_input_enabled() && io::stdin().is_terminal() {
        targets.push("codex-cli".to_string());
    }
    let artifact_policy = setup_artifact_policy(args, existing_config.is_some());
    let next_commands = setup_next_commands(args, &targets);
    let actions = setup_actions(
        args,
        &config_path,
        &targets,
        existing_config.is_some(),
        artifact_policy,
    );

    if args.plan {
        let mut lines = vec!["Setup plan:".to_string()];
        for action in &actions {
            let kind = action["kind"].as_str().unwrap_or("action");
            let summary = action["summary"].as_str().unwrap_or("");
            lines.push(format!("  - {kind}: {summary}"));
        }
        lines.push("Equivalent commands:".to_string());
        for command in &next_commands {
            lines.push(format!("  {command}"));
        }
        return Ok(CommandOutput {
            human: project_human_output(&project_root, lines.join("\n")),
            json: success_json(
                "setup",
                Some(&project_root),
                json!({
                    "plan": true,
                    "ready": !targets.is_empty() || existing_config.is_some(),
                    "config_path": config_path,
                    "detected_surfaces": detected_surfaces.iter().map(|(target, surface)| json!({
                        "target": target,
                        "surface": surface,
                    })).collect::<Vec<_>>(),
                    "targets": targets,
                    "profile_template": args.profile_template.clone(),
                    "artifact_policy": artifact_policy.as_str(),
                    "actions": actions,
                    "next_commands": next_commands,
                }),
            ),
        });
    }

    if let Some(existing) = existing_config {
        let existing_targets = existing.targets.clone();
        if args.artifact_policy.is_some() || args.install_background {
            if (cli.no_input_enabled() || !io::stdin().is_terminal()) && !args.yes {
                let mut err = CliError::new(
                    EXIT_STATE,
                    "Non-interactive setup requires --yes before updating machine or project state.",
                )
                .with_details(next_commands.clone());
                if let Some(obj) = err.json.as_object_mut() {
                    obj.insert("code".to_string(), json!("setup_confirmation_required"));
                    obj.insert("category".to_string(), json!("project_state"));
                    obj.insert("next_commands".to_string(), json!(next_commands));
                }
                return Err(err);
            }
            let artifact_report = if args.artifact_policy.is_some() {
                apply_setup_artifact_policy(&project_root, &config_path, artifact_policy)?
            } else {
                json!({
                    "policy": existing.metadata.get(AGENT_ARTIFACT_POLICY_METADATA_KEY).cloned().unwrap_or_else(|| "not-configured".to_string()),
                    "skipped": true,
                })
            };
            let background_install = if args.install_background {
                Some(cmd_background_install(
                    cli,
                    &BackgroundInstallArgs {
                        scope: BackgroundScopeArg::Project,
                        controller: None,
                        interval_minutes: 60,
                        log_dir: None,
                        label: None,
                        yes: args.yes,
                    },
                )?)
            } else {
                None
            };
            return Ok(CommandOutput {
                human: project_human_output(
                    &project_root,
                    format!(
                        "Setup updated.\nArtifact policy: {}\nBackground refresh: {}\nNext: metactl sync --preview",
                        artifact_policy.as_str(),
                        if background_install.is_some() { "installed" } else { "not installed" }
                    ),
                ),
                json: success_json(
                    "setup",
                    Some(&project_root),
                    json!({
                        "applied": true,
                        "already_configured": true,
                        "config_path": config_path,
                        "targets": existing_targets,
                        "artifact_policy": artifact_policy.as_str(),
                        "artifact_policy_update": artifact_report,
                        "background_install": background_install.map(|output| output.json),
                        "next_commands": ["metactl sync --preview", "metactl status"],
                    }),
                ),
            });
        }
        return Ok(CommandOutput {
            human: project_human_output(
                &project_root,
                format!(
                    "metactl is already configured at {}.\nExisting targets: {}\nNext: metactl status",
                    config_path.display(),
                    if existing_targets.is_empty() {
                        "(none)".to_string()
                    } else {
                        existing_targets.join(", ")
                    }
                ),
            ),
            json: success_json(
                "setup",
                Some(&project_root),
                json!({
                    "applied": false,
                    "already_configured": true,
                    "config_path": config_path,
                    "targets": existing_targets,
                    "artifact_policy": existing.metadata.get(AGENT_ARTIFACT_POLICY_METADATA_KEY).cloned().unwrap_or_else(|| "not-configured".to_string()),
                    "next_commands": ["metactl status", "metactl setup --plan"],
                }),
            ),
        });
    }

    if targets.is_empty() {
        let mut err = CliError::new(
            EXIT_STATE,
            "Setup needs an explicit target before it can write project state.",
        )
        .with_details(next_commands.clone());
        if let Some(obj) = err.json.as_object_mut() {
            obj.insert("code".to_string(), json!("setup_needs_target"));
            obj.insert("category".to_string(), json!("project_state"));
            obj.insert("next_commands".to_string(), json!(next_commands));
        }
        return Err(err);
    }

    if (cli.no_input_enabled() || !io::stdin().is_terminal()) && !args.yes {
        let mut err = CliError::new(
            EXIT_STATE,
            "Non-interactive setup requires --yes with explicit target choices.",
        )
        .with_details(next_commands.clone());
        if let Some(obj) = err.json.as_object_mut() {
            obj.insert("code".to_string(), json!("setup_confirmation_required"));
            obj.insert("category".to_string(), json!("project_state"));
            obj.insert("next_commands".to_string(), json!(next_commands));
        }
        return Err(err);
    }

    let init_args = InitArgs {
        target: targets.clone(),
        role: None,
        policy: None,
        starter_library: args.source.clone(),
        mode: InitMode::BrownfieldAutoDetect,
        detect: false,
        bind_profile: args.bind_profile,
    };
    let init_output = cmd_init(cli, &init_args)?;
    let artifact_report =
        apply_setup_artifact_policy(&project_root, &config_path, artifact_policy)?;
    let background_install = if args.install_background {
        Some(cmd_background_install(
            cli,
            &BackgroundInstallArgs {
                scope: BackgroundScopeArg::Project,
                controller: None,
                interval_minutes: 60,
                log_dir: None,
                label: None,
                yes: args.yes,
            },
        )?)
    } else {
        None
    };
    let mut lines = vec!["Setup applied.".to_string(), init_output.human];
    if artifact_policy != ArtifactPolicyArg::Off {
        lines.push(format!(
            "Portable agent artifacts: {}.",
            artifact_policy.as_str()
        ));
    }
    if background_install.is_some() {
        lines.push("Background refresh installed.".to_string());
    }
    lines.push("Next: metactl ignore fix --plan".to_string());

    Ok(CommandOutput {
        human: lines.join("\n\n"),
        json: success_json(
            "setup",
            Some(&project_root),
            json!({
                "applied": true,
                "config_path": config_path,
                "targets": targets,
                "ran_sync": false,
                "init": init_output.json,
                "artifact_policy": artifact_policy.as_str(),
                "artifact_policy_update": artifact_report,
                "background_install": background_install.map(|output| output.json),
                "next_commands": next_commands,
            }),
        ),
    })
}

fn setup_targets(
    project_root: &Path,
    args: &SetupArgs,
    existing_config: Option<&metactl::project::PartialProjectConfig>,
) -> std::result::Result<Vec<String>, CliError> {
    if !args.target.is_empty() {
        return expand_setup_targets(&args.target).map_err(state_error);
    }
    if let Some(config) = existing_config {
        if !config.targets.is_empty() {
            return Ok(config.targets.clone());
        }
    }
    let detected = detect_existing_surfaces(project_root);
    if !detected.is_empty() {
        return Ok(unique_strings(
            detected
                .into_iter()
                .map(|(target, _)| target)
                .collect::<Vec<_>>(),
        ));
    }
    Ok(Vec::new())
}

fn expand_setup_targets(requested: &[String]) -> Result<Vec<String>> {
    let mut targets = Vec::new();
    for target in requested {
        if target == "all" {
            targets.extend(IGNORE_TARGETS.iter().map(|item| item.to_string()));
        } else {
            targets.push(target.clone());
        }
    }
    Ok(unique_strings(targets))
}

fn setup_next_commands(args: &SetupArgs, targets: &[String]) -> Vec<String> {
    let default_targets;
    let target_arg = if targets.is_empty() {
        default_targets = vec!["codex-cli".to_string()];
        repeated_target_args(&default_targets)
    } else {
        repeated_target_args(targets)
    };
    let mut ignore_fix = format!(
        "metactl ignore fix --plan --scope {}",
        ignore_scope_label(args.ignore_scope)
    );
    if args.include_private_sources {
        ignore_fix.push_str(" --include-private-sources");
    }
    if args.include_lock {
        ignore_fix.push_str(" --include-lock");
    }
    let artifact_arg = args
        .artifact_policy
        .map(|policy| format!(" --artifact-policy {}", policy.as_str()))
        .unwrap_or_default();
    let mut commands = vec![
        format!("metactl setup{target_arg}{artifact_arg} --yes"),
        format!("metactl setup{target_arg}{artifact_arg} --plan"),
        ignore_fix,
    ];
    if !args.no_background {
        commands.push("metactl background plan --scope project".to_string());
        commands.push("metactl background install --scope project --yes".to_string());
    }
    commands.push("metactl sync --preview".to_string());
    commands
}

fn setup_artifact_policy(args: &SetupArgs, already_configured: bool) -> ArtifactPolicyArg {
    args.artifact_policy.unwrap_or(if already_configured {
        ArtifactPolicyArg::Off
    } else {
        ArtifactPolicyArg::PortableFirst
    })
}

fn setup_actions(
    args: &SetupArgs,
    config_path: &Path,
    targets: &[String],
    already_configured: bool,
    artifact_policy: ArtifactPolicyArg,
) -> Vec<Value> {
    let mut actions = vec![
        json!({
            "kind": "config",
            "path": config_path,
            "summary": if already_configured {
                "preserve existing metactl.yaml"
            } else {
                "create metactl.yaml and metactl.lock.json"
            },
            "targets": targets,
        }),
        json!({
            "kind": "ignore-repair",
            "summary": "recommend plan-first generated-surface ignore repair",
            "scope": ignore_scope_label(args.ignore_scope),
            "include_private_sources": args.include_private_sources,
            "include_lock": args.include_lock,
        }),
    ];
    actions.push(json!({
        "kind": "agent-artifacts",
        "summary": artifact_policy.summary(),
        "policy": artifact_policy.as_str(),
        "pack": if artifact_policy == ArtifactPolicyArg::PortableFirst {
            Value::String(AGENT_ARTIFACT_STEWARDSHIP_PACK.to_string())
        } else {
            Value::Null
        },
    }));
    if !args.no_background {
        actions.push(json!({
            "kind": "background-refresh",
            "summary": "offer report-only usage-ranked surface refresh through the OS scheduler",
            "default": "recommended",
            "install_command": "metactl background install --scope project --yes",
            "opt_out": "metactl setup --no-background",
            "report_only": true,
            "mutates_adapters": false,
        }));
    }
    actions
}

fn apply_setup_artifact_policy(
    project_root: &Path,
    config_path: &Path,
    artifact_policy: ArtifactPolicyArg,
) -> std::result::Result<Value, CliError> {
    let mut config = load_partial_project_config(config_path).map_err(state_error)?;
    let mut packs_changed = false;

    config.metadata.insert(
        AGENT_ARTIFACT_POLICY_METADATA_KEY.to_string(),
        artifact_policy.as_str().to_string(),
    );

    if artifact_policy == ArtifactPolicyArg::PortableFirst
        && !config
            .packs
            .iter()
            .any(|pack| pack == AGENT_ARTIFACT_STEWARDSHIP_PACK)
    {
        config
            .packs
            .push(AGENT_ARTIFACT_STEWARDSHIP_PACK.to_string());
        packs_changed = true;
    }

    write_partial_project_config(config_path, &config).map_err(internal_error)?;
    let lock_path = project_lock_path(project_root);
    if lock_path.exists() {
        let mut lock = load_lock(&lock_path).map_err(internal_error)?;
        lock.config_digest = Some(digest_path(config_path).map_err(internal_error)?);
        lock.updated_at = Some(now_string());
        write_lock(&lock_path, &lock).map_err(internal_error)?;
    }

    Ok(json!({
        "policy": artifact_policy.as_str(),
        "metadata_key": AGENT_ARTIFACT_POLICY_METADATA_KEY,
        "added_pack": if packs_changed {
            Value::String(AGENT_ARTIFACT_STEWARDSHIP_PACK.to_string())
        } else {
            Value::Null
        },
    }))
}

fn cmd_library(cli: &Cli, args: &LibraryArgs) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        LibraryCommand::Init(init_args) => cmd_library_init(cli, init_args),
    }
}

fn cmd_library_init(
    cli: &Cli,
    args: &LibraryInitArgs,
) -> std::result::Result<CommandOutput, CliError> {
    if !args.user {
        return Err(CliError::new(
            EXIT_STATE,
            "v1 library init currently supports only --user for the private writable library.",
        ));
    }
    validate_source_id(&args.profile)?;
    let Some(config_dir) = metactl_user_config_dir() else {
        return Err(CliError::new(
            EXIT_STATE,
            "HOME or XDG_CONFIG_HOME is required to create a user-private metactl library.",
        ));
    };
    let library_root = config_dir.join("library").join("user");
    for rel in [
        "roles",
        "policies",
        "targets",
        "packs",
        "provenance",
        "knowledge_sources",
        "imports",
    ] {
        fs::create_dir_all(library_root.join(rel)).map_err(|err| internal_error(anyhow!(err)))?;
    }
    let readme = library_root.join("README.md");
    if !readme.exists() {
        atomic_write(
            &readme,
            b"# User Private metactl Library\n\nWritable overlay for local private packs, profiles, and imports.\n",
        )
        .map_err(internal_error)?;
    }

    let profile_file = profile_path(&args.profile).ok_or_else(|| {
        CliError::new(
            EXIT_STATE,
            "HOME or XDG_CONFIG_HOME is required to resolve the profile path.",
        )
    })?;
    if let Some(parent) = profile_file.parent() {
        fs::create_dir_all(parent).map_err(|err| internal_error(anyhow!(err)))?;
    }
    let starter = ensure_bundled_starter_library_root().map_err(internal_error)?;
    let mut starter_library = Vec::new();
    starter_library.push(starter.to_string_lossy().to_string());
    starter_library.push(library_root.to_string_lossy().to_string());
    let profile = metactl::project::PartialProjectConfig {
        api_version: Some(API_VERSION.to_string()),
        role: Some("builder".to_string()),
        policy: Some("brownfield-safe-builder".to_string()),
        targets: vec!["codex-cli".to_string()],
        starter_library,
        defaults: Some(ProjectConfigDefaults {
            brownfield_mode: Some(BrownfieldMode::RefuseDueToConflict),
            fleet_sync_adopt: Some(FleetSyncAdoptMode::Patch),
            discovery_mode: Some(DiscoveryMode::CandidateSearch),
            surface_selection_mode: None,
        }),
        ..metactl::project::PartialProjectConfig::default()
    };
    write_partial_project_config(&profile_file, &profile).map_err(internal_error)?;
    if args.set_default {
        let mut settings = load_user_settings();
        settings.default_profile = Some(args.profile.clone());
        save_user_settings(&settings).map_err(internal_error)?;
    }
    Ok(CommandOutput {
        human: format!(
            "User private library ready at {}.\nProfile {} written to {}.\n",
            library_root.display(),
            args.profile,
            profile_file.display()
        ),
        json: success_json(
            "library",
            cli.project.as_deref(),
            json!({
                "action": "init",
                "scope": "user",
                "library_root": library_root,
                "profile": args.profile,
                "profile_path": profile_file,
                "set_default": args.set_default,
            }),
        ),
    })
}

fn cmd_project(cli: &Cli, args: &ProjectArgs) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        ProjectCommand::Link(link_args) => cmd_project_link(cli, link_args),
    }
}

fn cmd_project_link(
    cli: &Cli,
    args: &ProjectLinkArgs,
) -> std::result::Result<CommandOutput, CliError> {
    validate_source_id(&args.profile)?;
    let project_root = project_root(cli).map_err(internal_error)?;
    ensure_project_layout(&project_root).map_err(internal_error)?;
    ensure_gitignore_entries(&project_root).map_err(internal_error)?;
    let config_path = project_config_path(&project_root, cli.config.as_deref());
    let mut config = if config_path.exists() {
        load_partial_project_config(&config_path).map_err(internal_error)?
    } else {
        metactl::project::PartialProjectConfig {
            api_version: Some(API_VERSION.to_string()),
            ..metactl::project::PartialProjectConfig::default()
        }
    };
    config.extends_profile = Some(args.profile.clone());
    write_partial_project_config(&config_path, &config).map_err(internal_error)?;
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Project linked to profile {}.\nNext: metactl sync --preview",
                args.profile
            ),
        ),
        json: success_json(
            "project",
            Some(&project_root),
            json!({
                "action": "link",
                "profile": args.profile,
                "config_path": config_path,
            }),
        ),
    })
}

fn cmd_use(cli: &Cli, args: &UseArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let config_path = project_config_path(&project_root, cli.config.as_deref());
    if !config_path.exists() {
        return Err(missing_config_error(cli, &project_root));
    }

    let context = load_required_context(cli, &project_root)?;
    if !context.has_corpus() {
        return Err(CliError::new(
            EXIT_STATE,
            "No starter library is available. Next: add a starter library path to metactl.yaml or run metactl doctor.",
        )
        .with_details(
            next_steps_for_search("no_corpus")
                .into_iter()
                .map(str::to_string)
                .collect(),
        ));
    }

    if let Some(pack_id) = namespaced_pack_id(&args.query) {
        let exists = context
            .registry
            .as_ref()
            .and_then(|registry| registry.pack_by_id(pack_id))
            .is_some();
        if exists {
            return add_pack_to_config_and_maybe_sync(
                cli,
                &project_root,
                &config_path,
                &args.query,
                pack_id,
                args.local,
                args.no_sync,
            );
        }
    }

    // Search for matching packs
    let overrides = ConfigOverrides {
        role: None,
        policy: None,
        targets: Vec::new(),
    };
    let config = context.effective_config(&overrides).map_err(state_error)?;
    let kernel = kernel_from_context(&context).map_err(internal_error)?;
    let result = kernel
        .search(SearchParams {
            query: args.query.clone(),
            config,
            overlay: context.overlay.clone(),
            candidate_packs: Vec::new(),
            limit: Some(10),
        })
        .map_err(state_error)?;

    if result.matches.is_empty() {
        let classification = search_classification(&result);
        let next_steps = next_steps_for_search(classification);
        return Err(CliError::new(
            EXIT_STATE,
            format!(
                "No packs matched query \"{}\". Classification: {}.",
                args.query, classification
            ),
        )
        .with_details(
            next_steps
                .iter()
                .map(|step| format!("Next: {step}"))
                .collect(),
        ));
    }

    if result.matches.len() > 1 {
        // Check for an exact ID match first
        let exact = result.matches.iter().find(|m| m.pack_ref.id == args.query);
        if exact.is_none() {
            // If the best score is low, treat it as "no strong match" rather than ambiguity
            let best_score = result.matches.first().map(|m| m.score).unwrap_or(0.0);
            let message = if best_score < 0.3 {
                format!(
                    "No strong match for \"{}\". Try `metactl search \"{}\"` to browse, or use an exact pack ID.",
                    args.query, args.query
                )
            } else {
                format!(
                    "Multiple packs matched \"{}\". Use an exact pack ID:",
                    args.query
                )
            };
            let listing = result
                .matches
                .iter()
                .map(|m| format!("  {} ({:.2}) {}", m.pack_ref.id, m.score, m.why))
                .collect::<Vec<_>>();
            return Err(CliError::new(EXIT_STATE, message).with_details(listing));
        }
    }

    let matched = result
        .matches
        .iter()
        .find(|m| m.pack_ref.id == args.query)
        .unwrap_or(&result.matches[0]);
    let pack_id = matched.pack_ref.id.clone();

    add_pack_to_config_and_maybe_sync(
        cli,
        &project_root,
        &config_path,
        &pack_id,
        &pack_id,
        args.local,
        args.no_sync,
    )
}

fn add_pack_to_config_and_maybe_sync(
    cli: &Cli,
    project_root: &Path,
    config_path: &Path,
    config_pack_ref: &str,
    resolved_pack_id: &str,
    local: bool,
    no_sync: bool,
) -> std::result::Result<CommandOutput, CliError> {
    let already_configured;
    if local {
        let local_path = project_root.join("metactl.local.yaml");
        let mut local: metactl::project::PartialProjectConfig = if local_path.exists() {
            let raw = fs::read_to_string(&local_path)
                .map_err(|e| internal_error(anyhow::anyhow!("read metactl.local.yaml: {e}")))?;
            serde_yaml::from_str(&raw)
                .map_err(|e| internal_error(anyhow::anyhow!("parse metactl.local.yaml: {e}")))?
        } else {
            metactl::project::PartialProjectConfig::default()
        };
        if local.packs.contains(&config_pack_ref.to_string()) {
            already_configured = true;
        } else {
            already_configured = false;
            local.packs.push(config_pack_ref.to_string());
            write_partial_project_config(&local_path, &local).map_err(internal_error)?;
        }
    } else {
        let mut raw = load_partial_project_config(&config_path).map_err(internal_error)?;
        if raw.packs.contains(&config_pack_ref.to_string()) {
            already_configured = true;
        } else {
            already_configured = false;
            raw.packs.push(config_pack_ref.to_string());
            write_partial_project_config(&config_path, &raw).map_err(internal_error)?;
        }
    };

    let config_label = if local {
        "metactl.local.yaml"
    } else {
        "metactl.yaml"
    };

    let mut human_parts = Vec::new();
    if already_configured {
        human_parts.push(format!(
            "Resolved \"{}\" -> pack {}\nPack already configured in {}.",
            config_pack_ref, resolved_pack_id, config_label
        ));
    } else {
        human_parts.push(format!(
            "Resolved \"{}\" -> pack {}\nAdded to {}.",
            config_pack_ref, resolved_pack_id, config_label
        ));
    }

    let mut use_json = json!({
        "query": config_pack_ref,
        "resolved_pack": resolved_pack_id,
        "configured_pack": config_pack_ref,
        "already_configured": already_configured,
        "local": local,
    });

    if !no_sync {
        let sync_output = cmd_sync(
            cli,
            &SyncArgs {
                target: Vec::new(),
                all: false,
                role: None,
                policy: None,
                adopt: None,
                surface_mode: None,
                require_private_sources: false,
                preview: false,
                apply: true,
            },
        )?;
        human_parts.push(sync_output.human);
        if let Some(obj) = use_json.as_object_mut() {
            obj.insert("sync".to_string(), sync_output.json);
        }
    } else {
        human_parts.push("Sync skipped (--no-sync). Next: metactl sync".to_string());
    }

    Ok(CommandOutput {
        human: project_human_output(&project_root, human_parts.join("\n\n")),
        json: success_json("use", Some(&project_root), use_json),
    })
}

fn namespaced_pack_id(value: &str) -> Option<&str> {
    value
        .split_once('/')
        .and_then(|(_, pack_id)| (!pack_id.is_empty()).then_some(pack_id))
}

fn source_state_json(project_root: &Path, config: &ProjectConfigFile) -> Value {
    private_source_readiness(project_root, config, false).unwrap_or_else(|err| {
        json!({
            "state": "unknown",
            "sources": [],
            "error": err.message,
        })
    })
}

fn private_source_readiness(
    project_root: &Path,
    config: &ProjectConfigFile,
    fetch_remote: bool,
) -> std::result::Result<Value, CliError> {
    let private_sources = config
        .sources
        .iter()
        .filter(|source| source.visibility == SourceVisibility::Private)
        .collect::<Vec<_>>();
    if private_sources.is_empty() {
        return Ok(json!({
            "state": "public_only",
            "sources": [],
        }));
    }
    let mut missing = Vec::new();
    let mut active = Vec::new();
    let mut stale = Vec::new();
    let mut unlocked = Vec::new();
    let mut freshness = Vec::new();
    for source in private_sources {
        let present = match source.source_type {
            SourceType::Local => source
                .path
                .as_ref()
                .map(|path| PathBuf::from(path).exists())
                .unwrap_or(false),
            SourceType::Git => project_root
                .join(".metactl/cache/sources")
                .join(&source.id)
                .join("library.json")
                .exists(),
        };
        if present {
            active.push(source.id.clone());
        } else {
            missing.push(source.id.clone());
        }
        let locked = locked_source_for_record(project_root, source)?;
        let source_freshness =
            source_freshness_json(project_root, source, locked.as_ref(), present, fetch_remote)?;
        match source_freshness["status"].as_str().unwrap_or("unknown") {
            "stale" => stale.push(source.id.clone()),
            "unlocked" => unlocked.push(source.id.clone()),
            _ => {}
        }
        freshness.push(source_freshness);
    }
    let state = if !missing.is_empty() {
        "private_source_missing"
    } else if !stale.is_empty() {
        "private_source_stale"
    } else {
        "private_source_active"
    };
    Ok(json!({
        "state": state,
        "active": active,
        "missing": missing,
        "stale": stale,
        "unlocked": unlocked,
        "freshness": freshness,
    }))
}

fn read_private_source_lock(
    project_root: &Path,
) -> std::result::Result<PrivateSourceLock, CliError> {
    let path = private_source_lock_path(project_root);
    if !path.exists() {
        return Ok(PrivateSourceLock::default());
    }
    let bytes = fs::read(&path)
        .map_err(|err| internal_error(anyhow!("read {}: {}", path.display(), err)))?;
    serde_json::from_slice(&bytes)
        .map_err(|err| internal_error(anyhow!("parse {}: {}", path.display(), err)))
}

fn locked_source_for_record(
    project_root: &Path,
    source: &SourceRecord,
) -> std::result::Result<Option<LockedSource>, CliError> {
    let private_lock = read_private_source_lock(project_root)?;
    if let Some(locked) = private_lock
        .sources
        .iter()
        .find(|locked| locked.id == source.id)
        .cloned()
    {
        return Ok(Some(locked));
    }
    let public_lock_path = project_lock_path(project_root);
    if !public_lock_path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&public_lock_path)
        .map_err(|err| internal_error(anyhow!("read {}: {}", public_lock_path.display(), err)))?;
    let public_lock: ProjectLock = serde_json::from_slice(&bytes)
        .map_err(|err| internal_error(anyhow!("parse {}: {}", public_lock_path.display(), err)))?;
    Ok(public_lock
        .sources
        .into_iter()
        .find(|locked| locked.id == source.id))
}

fn source_freshness_json(
    project_root: &Path,
    source: &SourceRecord,
    locked: Option<&LockedSource>,
    present: bool,
    fetch_remote: bool,
) -> std::result::Result<Value, CliError> {
    if !present {
        return Ok(json!({
            "id": source.id,
            "type": source_type_label(&source.source_type),
            "status": "missing",
            "locked": locked.is_some(),
        }));
    }
    match source.source_type {
        SourceType::Local => Ok(json!({
            "id": source.id,
            "type": "local",
            "status": if locked.is_some() { "fresh" } else { "unlocked" },
            "locked": locked.is_some(),
        })),
        SourceType::Git => {
            let cache_root = project_root
                .join(".metactl")
                .join("cache")
                .join("sources")
                .join(&source.id);
            let Some(locked) = locked else {
                return Ok(json!({
                    "id": source.id,
                    "type": "git",
                    "status": "unlocked",
                    "locked": false,
                }));
            };
            let Some(locked_commit) = locked.resolved_commit.as_deref() else {
                return Ok(json!({
                    "id": source.id,
                    "type": "git",
                    "status": "unlocked",
                    "locked": true,
                }));
            };
            if fetch_remote && git_worktree_clean(&cache_root)? {
                run_git_in(&cache_root, &["fetch", "--quiet", "--tags", "origin"])?;
            }
            let head = git_output_in(&cache_root, &["rev-parse", "HEAD"])?;
            let mut stale_reasons = Vec::new();
            if head.trim() != locked_commit {
                stale_reasons.push("cache_head_differs_from_lock");
            }
            if fetch_remote {
                if let Some(requested_ref) = source.ref_.as_deref() {
                    let resolved = git_resolve_requested_ref(&cache_root, requested_ref)?;
                    if resolved.trim() != locked_commit {
                        stale_reasons.push("configured_ref_differs_from_lock");
                    }
                }
            }
            Ok(json!({
                "id": source.id,
                "type": "git",
                "status": if stale_reasons.is_empty() { "fresh" } else { "stale" },
                "locked": true,
                "locked_commit": locked_commit,
                "head_commit": head.trim(),
                "reasons": stale_reasons,
            }))
        }
    }
}

fn source_preflight_error(project_root: &Path, source_state: Value, strict: bool) -> CliError {
    let state = source_state["state"].as_str().unwrap_or("unknown");
    let message = if strict {
        "Private source preflight failed. Run `metactl source sync <name>` or remove the stale source before syncing."
    } else {
        "Private source lock is stale. Run `metactl source sync <name>` before syncing targets."
    };
    CliError {
        code: EXIT_STATE,
        message: message.to_string(),
        details: source_preflight_detail_lines(&source_state),
        json: json!({
            "ok": false,
            "command": "sync",
            "api_version": API_VERSION,
            "project_root": project_root.to_string_lossy(),
            "state": state,
            "source_state": source_state,
        }),
    }
}

fn source_preflight_detail_lines(source_state: &Value) -> Vec<String> {
    let mut details = Vec::new();
    if let Some(items) = source_state["freshness"].as_array() {
        for item in items {
            let id = item["id"].as_str().unwrap_or("?");
            match item["status"].as_str().unwrap_or("unknown") {
                "stale" | "unlocked" => {
                    details.push(format!("Next: metactl source sync {id}"));
                }
                "missing" => {
                    if item["type"].as_str() == Some("git") {
                        details.push(format!("Next: metactl source sync {id}"));
                    } else {
                        details.push(format!("Check the configured path for source `{id}`."));
                    }
                }
                _ => {}
            }
        }
    }
    if details.is_empty() {
        for key in ["stale", "unlocked"] {
            if let Some(items) = source_state[key].as_array() {
                for item in items {
                    if let Some(id) = item.as_str() {
                        details.push(format!("Next: metactl source sync {id}"));
                    }
                }
            }
        }
    }
    if details.is_empty() {
        details.push("Next: metactl source list".to_string());
    }
    details
}

fn detect_existing_surfaces(project_root: &Path) -> Vec<(String, String)> {
    let mut detected = Vec::new();
    if project_root.join("AGENTS.md").exists() {
        detected.push(("codex-cli".to_string(), "AGENTS.md".to_string()));
    }
    if project_root.join("CLAUDE.md").exists() || project_root.join(".claude").is_dir() {
        detected.push((
            "claude-code".to_string(),
            "CLAUDE.md or .claude/".to_string(),
        ));
    }
    if project_root.join(".cursor").is_dir() || project_root.join(".cursor/rules").is_dir() {
        detected.push(("cursor".to_string(), ".cursor/".to_string()));
    }
    if project_root.join("GEMINI.md").exists() || project_root.join(".gemini").is_dir() {
        detected.push((
            "gemini-cli".to_string(),
            "GEMINI.md or .gemini/".to_string(),
        ));
    }
    if project_root.join("OPENCLAW.md").exists() || project_root.join(".openclaw").is_dir() {
        detected.push((
            "openclaw".to_string(),
            "OPENCLAW.md or .openclaw/".to_string(),
        ));
    }
    detected
}

fn cmd_target(cli: &Cli, args: &TargetArgs) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        Some(TargetCommand::List(args)) => cmd_target_list(cli, args),
        Some(TargetCommand::Add(args)) => cmd_target_add(cli, args),
        Some(TargetCommand::Remove(args)) => cmd_target_remove(cli, args),
        None => cmd_target_list(cli, &TargetListArgs { installed: false }),
    }
}

fn cmd_target_list(
    cli: &Cli,
    args: &TargetListArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_optional_context(cli, &project_root).map_err(internal_error)?;
    let registry = context.registry.or_else(|| {
        ensure_bundled_starter_library_root()
            .ok()
            .and_then(|default_root| LibraryRegistry::load_from_roots(&[default_root]).ok())
    });
    let configured = context
        .config_file
        .as_ref()
        .map(|config| config.targets.clone())
        .unwrap_or_default();
    let configured_set = configured.iter().cloned().collect::<BTreeSet<_>>();
    let mut items = registry
        .as_ref()
        .map(|registry| {
            registry
                .list_targets()
                .into_iter()
                .map(|target| {
                    json!({
                        "id": target.target_id,
                        "title": target.title,
                        "configured": configured_set.contains(&target.target_id),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            configured
                .iter()
                .map(|target_id| {
                    json!({
                        "id": target_id,
                        "title": target_id,
                        "configured": true,
                    })
                })
                .collect::<Vec<_>>()
        });
    if args.installed {
        items.retain(|item| item["configured"] == Value::Bool(true));
    }

    let mut lines = vec!["Targets:".to_string()];
    if items.is_empty() {
        lines.push("  (none discovered)".to_string());
    } else {
        lines.extend(items.iter().map(|item| {
            let id = item["id"].as_str().unwrap_or("");
            let title = item["title"].as_str().unwrap_or("");
            let marker = if item["configured"].as_bool().unwrap_or(false) {
                " *"
            } else {
                ""
            };
            format!("  {:<20} {}{}", id, title, marker)
        }));
    }
    lines.push(String::new());
    lines.push(
        "Usage: metactl target add <target-id> | metactl target remove <target-id>".to_string(),
    );

    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "target",
            Some(&project_root),
            json!({
                "action": "list",
                "items": items,
                "configured_targets": configured,
            }),
        ),
    })
}

fn cmd_target_add(
    cli: &Cli,
    args: &TargetUpdateArgs,
) -> std::result::Result<CommandOutput, CliError> {
    if args.target_ids.is_empty() {
        return Err(CliError::new(
            EXIT_STATE,
            "No target IDs specified. Usage: metactl target add <target-id> [<target-id> ...]",
        ));
    }
    let project_root = project_root(cli).map_err(internal_error)?;
    let config_path = project_config_path(&project_root, cli.config.as_deref());
    if !config_path.exists() {
        return Err(missing_config_error(cli, &project_root));
    }

    let context = load_required_context(cli, &project_root)?;
    let expanded =
        expand_target_ids(&args.target_ids, context.registry.as_ref()).map_err(state_error)?;
    validate_target_ids(&expanded, context.registry.as_ref()).map_err(state_error)?;

    let mut raw = load_partial_project_config(&config_path).map_err(internal_error)?;
    let existing = raw.targets.iter().cloned().collect::<BTreeSet<_>>();
    let mut added = Vec::new();
    let mut already_configured = Vec::new();
    for target_id in expanded {
        if existing.contains(&target_id) || raw.targets.contains(&target_id) {
            already_configured.push(target_id);
        } else {
            raw.targets.push(target_id.clone());
            added.push(target_id);
        }
    }

    if added.is_empty() {
        let human = format!(
            "Target(s) already configured: {}\nNo changes made.",
            already_configured.join(", ")
        );
        let base_json = json!({
            "action": "add",
            "added": [],
            "already_configured": already_configured,
            "targets": raw.targets,
        });
        if args.sync {
            let sync_output = cmd_sync(
                cli,
                &SyncArgs {
                    target: Vec::new(),
                    all: false,
                    role: None,
                    policy: None,
                    adopt: None,
                    surface_mode: None,
                    require_private_sources: false,
                    preview: false,
                    apply: true,
                },
            )?;
            return Ok(CommandOutput {
                human: format!("{}\n\n{}", human, sync_output.human),
                json: success_json(
                    "target",
                    Some(&project_root),
                    json!({
                        "action": "add",
                        "added": [],
                        "already_configured": already_configured,
                        "targets": raw.targets,
                        "sync": sync_output.json,
                    }),
                ),
            });
        }
        return Ok(CommandOutput {
            human: project_human_output(&project_root, human),
            json: success_json("target", Some(&project_root), base_json),
        });
    }

    write_partial_project_config(&config_path, &raw).map_err(internal_error)?;
    let target_output = CommandOutput {
        human: project_human_output(
            &project_root,
            format!("Added target(s): {}\nNext: metactl sync", added.join(", ")),
        ),
        json: success_json(
            "target",
            Some(&project_root),
            json!({
                "action": "add",
                "added": added,
                "already_configured": already_configured,
                "targets": raw.targets,
            }),
        ),
    };
    if args.sync {
        let sync_output = cmd_sync(
            cli,
            &SyncArgs {
                target: Vec::new(),
                all: false,
                role: None,
                policy: None,
                adopt: None,
                surface_mode: None,
                require_private_sources: false,
                preview: false,
                apply: true,
            },
        )?;
        return Ok(CommandOutput {
            human: format!("{}\n\n{}", target_output.human, sync_output.human),
            json: success_json(
                "target",
                Some(&project_root),
                json!({
                    "action": "add",
                    "added": target_output.json["added"].clone(),
                    "already_configured": target_output.json["already_configured"].clone(),
                    "targets": target_output.json["targets"].clone(),
                    "sync": sync_output.json,
                }),
            ),
        });
    }
    Ok(target_output)
}

fn cmd_target_remove(
    cli: &Cli,
    args: &TargetUpdateArgs,
) -> std::result::Result<CommandOutput, CliError> {
    if args.target_ids.is_empty() {
        return Err(CliError::new(
            EXIT_STATE,
            "No target IDs specified. Usage: metactl target remove <target-id> [<target-id> ...]",
        ));
    }
    let project_root = project_root(cli).map_err(internal_error)?;
    let config_path = project_config_path(&project_root, cli.config.as_deref());
    if !config_path.exists() {
        return Err(missing_config_error(cli, &project_root));
    }

    let mut raw = load_partial_project_config(&config_path).map_err(internal_error)?;
    let requested = unique_strings(args.target_ids.clone());
    let mut removed = Vec::new();
    let mut not_configured = Vec::new();
    for target_id in requested {
        if raw.targets.contains(&target_id) {
            raw.targets.retain(|item| item != &target_id);
            removed.push(target_id);
        } else {
            not_configured.push(target_id);
        }
    }

    if removed.is_empty() {
        let human = format!(
            "Target(s) not in config: {}\nNo changes made.",
            not_configured.join(", ")
        );
        let base_json = json!({
            "action": "remove",
            "removed": [],
            "not_configured": not_configured,
            "targets": raw.targets,
        });
        if args.sync {
            let sync_output = cmd_sync(
                cli,
                &SyncArgs {
                    target: Vec::new(),
                    all: false,
                    role: None,
                    policy: None,
                    adopt: None,
                    surface_mode: None,
                    require_private_sources: false,
                    preview: false,
                    apply: true,
                },
            )?;
            return Ok(CommandOutput {
                human: format!(
                    "{}\n\n{}",
                    project_human_output(&project_root, human),
                    sync_output.human
                ),
                json: success_json(
                    "target",
                    Some(&project_root),
                    json!({
                        "action": "remove",
                        "removed": [],
                        "not_configured": not_configured,
                        "targets": raw.targets,
                        "sync": sync_output.json,
                    }),
                ),
            });
        }
        return Ok(CommandOutput {
            human: project_human_output(&project_root, human),
            json: success_json("target", Some(&project_root), base_json),
        });
    }

    write_partial_project_config(&config_path, &raw).map_err(internal_error)?;
    let target_output = CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Removed target(s): {}\nNext: metactl sync",
                removed.join(", ")
            ),
        ),
        json: success_json(
            "target",
            Some(&project_root),
            json!({
                "action": "remove",
                "removed": removed,
                "not_configured": not_configured,
                "targets": raw.targets,
            }),
        ),
    };
    if args.sync {
        let sync_output = cmd_sync(
            cli,
            &SyncArgs {
                target: Vec::new(),
                all: false,
                role: None,
                policy: None,
                adopt: None,
                surface_mode: None,
                require_private_sources: false,
                preview: false,
                apply: true,
            },
        )?;
        return Ok(CommandOutput {
            human: format!("{}\n\n{}", target_output.human, sync_output.human),
            json: success_json(
                "target",
                Some(&project_root),
                json!({
                    "action": "remove",
                    "removed": target_output.json["removed"].clone(),
                    "not_configured": target_output.json["not_configured"].clone(),
                    "targets": target_output.json["targets"].clone(),
                    "sync": sync_output.json,
                }),
            ),
        });
    }
    Ok(target_output)
}

fn cmd_add(cli: &Cli, args: &AddArgs) -> std::result::Result<CommandOutput, CliError> {
    if args.pack_ids.is_empty() {
        return Err(CliError::new(
            EXIT_STATE,
            "No pack IDs specified. Usage: metactl add <pack-id> [<pack-id> ...]",
        ));
    }
    let project_root = project_root(cli).map_err(internal_error)?;
    let config_path = project_config_path(&project_root, cli.config.as_deref());
    if !config_path.exists() {
        return Err(missing_config_error(cli, &project_root));
    }

    let context = load_required_context(cli, &project_root)?;

    // Validate that requested packs exist in the library
    let mut not_found = Vec::new();
    let mut already_added = Vec::new();
    let mut to_add = Vec::new();
    for pack_id in &args.pack_ids {
        let exists_in_library = context
            .registry
            .as_ref()
            .map(|registry| {
                registry
                    .list_packs()
                    .iter()
                    .any(|p| p.manifest.id == *pack_id)
            })
            .unwrap_or(false);
        if !exists_in_library {
            not_found.push(pack_id.clone());
            continue;
        }
        if context.config_file.packs.contains(pack_id) {
            already_added.push(pack_id.clone());
            continue;
        }
        to_add.push(pack_id.clone());
    }

    if !not_found.is_empty() {
        let available = context
            .registry
            .as_ref()
            .map(|registry| {
                registry
                    .list_packs()
                    .into_iter()
                    .map(|p| p.manifest.id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let suggestions = nearest_pack_suggestions(&not_found, &available, 5);
        let mut details = Vec::new();
        if !suggestions.is_empty() {
            details.push(format!("Did you mean: {}", suggestions.join(", ")));
        }
        details.push("Next: metactl list packs".to_string());
        if let Some(first) = not_found.first() {
            details.push(format!("Next: metactl search {first}"));
        }
        details.push(format!(
            "Available pack count: {} (run `metactl list packs` for the full list).",
            available.len()
        ));
        return Err(CliError {
            code: EXIT_STATE,
            message: format!("Pack(s) not found in library: {}", not_found.join(", ")),
            details,
            json: json!({
                "ok": false,
                "api_version": API_VERSION,
                "message": format!("Pack(s) not found in library: {}", not_found.join(", ")),
                "not_found": not_found,
                "suggestions": suggestions,
                "available_packs": available,
            }),
        });
    }

    if to_add.is_empty() {
        let human = if !already_added.is_empty() {
            format!(
                "Pack(s) already configured: {}\nNo changes made.",
                already_added.join(", ")
            )
        } else {
            "No packs to add.".to_string()
        };
        let base_json = json!({
            "added": [],
            "already_configured": already_added,
        });
        if args.sync {
            let sync_output = cmd_sync(
                cli,
                &SyncArgs {
                    target: Vec::new(),
                    all: false,
                    role: None,
                    policy: None,
                    adopt: None,
                    surface_mode: None,
                    require_private_sources: false,
                    preview: false,
                    apply: true,
                },
            )?;
            return Ok(CommandOutput {
                human: format!("{}\n\n{}", human, sync_output.human),
                json: success_json(
                    "add",
                    Some(&project_root),
                    json!({
                        "added": [],
                        "already_configured": already_added,
                        "sync": sync_output.json,
                    }),
                ),
            });
        }
        return Ok(CommandOutput {
            human,
            json: success_json("add", Some(&project_root), base_json),
        });
    }

    // Read the raw config, add packs, and write it back
    let mut raw = load_partial_project_config(&config_path).map_err(internal_error)?;
    raw.packs.extend(to_add.clone());
    write_partial_project_config(&config_path, &raw).map_err(internal_error)?;

    let mut notes = Vec::new();
    if !already_added.is_empty() {
        notes.push(format!("Already configured: {}", already_added.join(", ")));
    }

    let human = format!("Added pack(s): {}\nNext: metactl sync", to_add.join(", "));

    let add_output = CommandOutput {
        human,
        json: success_json(
            "add",
            Some(&project_root),
            json!({
                "added": to_add,
                "already_configured": already_added,
                "notes": notes,
            }),
        ),
    };

    if args.sync {
        // Run sync after adding packs
        let sync_output = cmd_sync(
            cli,
            &SyncArgs {
                target: Vec::new(),
                all: false,
                role: None,
                policy: None,
                adopt: None,
                surface_mode: None,
                require_private_sources: false,
                preview: false,
                apply: true,
            },
        )?;
        return Ok(CommandOutput {
            human: format!("{}\n\n{}", add_output.human, sync_output.human),
            json: success_json(
                "add",
                Some(&project_root),
                json!({
                    "added": to_add,
                    "already_configured": already_added,
                    "sync": sync_output.json,
                }),
            ),
        });
    }

    Ok(add_output)
}

fn cmd_remove(cli: &Cli, args: &RemoveArgs) -> std::result::Result<CommandOutput, CliError> {
    if args.pack_ids.is_empty() {
        return Err(CliError::new(
            EXIT_STATE,
            "No pack IDs specified. Usage: metactl remove <pack-id> [<pack-id> ...]",
        ));
    }
    let project_root = project_root(cli).map_err(internal_error)?;
    let config_path = project_config_path(&project_root, cli.config.as_deref());
    if !config_path.exists() {
        return Err(missing_config_error(cli, &project_root));
    }

    let mut raw = load_partial_project_config(&config_path).map_err(internal_error)?;
    let mut removed = Vec::new();
    let mut not_configured = Vec::new();
    for pack_id in &args.pack_ids {
        if raw.packs.contains(pack_id) {
            raw.packs.retain(|p| p != pack_id);
            removed.push(pack_id.clone());
        } else {
            not_configured.push(pack_id.clone());
        }
    }

    if removed.is_empty() {
        let human = format!(
            "Pack(s) not in config: {}\nNo changes made.",
            not_configured.join(", ")
        );
        let base_json = json!({
            "removed": [],
            "not_configured": not_configured,
        });
        if args.sync {
            let sync_output = cmd_sync(
                cli,
                &SyncArgs {
                    target: Vec::new(),
                    all: false,
                    role: None,
                    policy: None,
                    adopt: None,
                    surface_mode: None,
                    require_private_sources: false,
                    preview: false,
                    apply: true,
                },
            )?;
            return Ok(CommandOutput {
                human: format!("{}\n\n{}", human, sync_output.human),
                json: success_json(
                    "remove",
                    Some(&project_root),
                    json!({
                        "removed": [],
                        "not_configured": not_configured,
                        "sync": sync_output.json,
                    }),
                ),
            });
        }
        return Ok(CommandOutput {
            human,
            json: success_json("remove", Some(&project_root), base_json),
        });
    }

    write_partial_project_config(&config_path, &raw).map_err(internal_error)?;

    let human = format!(
        "Removed pack(s): {}\nNext: metactl sync",
        removed.join(", ")
    );

    let remove_output = CommandOutput {
        human,
        json: success_json(
            "remove",
            Some(&project_root),
            json!({
                "removed": removed,
                "not_configured": not_configured,
            }),
        ),
    };

    if args.sync {
        let sync_output = cmd_sync(
            cli,
            &SyncArgs {
                target: Vec::new(),
                all: false,
                role: None,
                policy: None,
                adopt: None,
                surface_mode: None,
                require_private_sources: false,
                preview: false,
                apply: true,
            },
        )?;
        return Ok(CommandOutput {
            human: format!("{}\n\n{}", remove_output.human, sync_output.human),
            json: success_json(
                "remove",
                Some(&project_root),
                json!({
                    "removed": removed,
                    "not_configured": not_configured,
                    "sync": sync_output.json,
                }),
            ),
        });
    }

    Ok(remove_output)
}

fn cmd_fleet(cli: &Cli, args: &FleetArgs) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        Some(FleetCommand::List) => cmd_fleet_list(cli),
        Some(FleetCommand::Status(args)) => cmd_fleet_status(cli, args),
        Some(FleetCommand::Sync(args)) => cmd_fleet_sync(cli, args),
        Some(FleetCommand::Controller(args)) => cmd_fleet_controller(cli, args),
        None => cmd_fleet_status(
            cli,
            &FleetStatusArgs {
                ids: Vec::new(),
                include_disabled: false,
            },
        ),
    }
}

fn cmd_fleet_controller(
    cli: &Cli,
    args: &FleetControllerArgs,
) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        FleetControllerCommand::Init { name, path, force } => {
            validate_fleet_controller_name(name)?;
            let controller_path = resolve_fleet_controller_init_path(cli, name, path.as_deref())?;
            fs::create_dir_all(&controller_path).map_err(|err| {
                internal_error(anyhow!(
                    "create Fleet controller {}: {}",
                    controller_path.display(),
                    err
                ))
            })?;
            ensure_project_layout(&controller_path).map_err(internal_error)?;

            let config_path = project_config_path(&controller_path, cli.config.as_deref());
            if config_path.exists() && !force {
                return Err(CliError::new(
                    EXIT_STATE,
                    format!(
                        "Fleet controller config already exists: {}.\nHint: rerun with --force to replace it, or use `metactl fleet controller set {name} {}`.",
                        config_path.display(),
                        controller_path.display()
                    ),
                ));
            }
            let mut config = default_project_config();
            config.linked_projects = Vec::new();
            config
                .metadata
                .insert("fleet_controller".to_string(), "true".to_string());
            write_project_config(&config_path, &config).map_err(internal_error)?;

            let readme_path = controller_path.join("README.md");
            if !readme_path.exists() || *force {
                atomic_write(
                    &readme_path,
                    fleet_controller_readme(name, &controller_path).as_bytes(),
                )
                .map_err(internal_error)?;
            }

            let context = load_required_context_for_path(cli, &controller_path)?;
            save_fleet_controller_pointer(name, &controller_path)?;
            Ok(CommandOutput {
                human: format!(
                    "Fleet controller `{name}` initialized at {}.\nNext: edit {} and add linked_projects, then run `metactl fleet sync --preview`.\n",
                    controller_path.display(),
                    config_path.display()
                ),
                json: success_json(
                    "fleet",
                    cli.project.as_deref(),
                    json!({
                        "action": "controller-init",
                        "controller": {
                            "id": name,
                            "path": controller_path.to_string_lossy(),
                            "source": "user_default",
                            "config_path": config_path.to_string_lossy(),
                            "registry_digest": current_config_digest(&context).ok(),
                        },
                        "created_files": [
                            config_path.to_string_lossy(),
                            readme_path.to_string_lossy(),
                        ],
                    }),
                ),
            })
        }
        FleetControllerCommand::Show => {
            let settings = load_user_settings();
            let path = user_settings_path();
            let controller = resolve_fleet_controller(cli).ok();
            let human = if let Some(controller) = controller.as_ref() {
                fleet_controller_human_header(controller).join("\n")
            } else {
                format!(
                    "Fleet controller: (none)\nUser settings file: {}",
                    path.as_ref()
                        .map(|item| item.display().to_string())
                        .unwrap_or_else(
                            || "(unavailable — set HOME or XDG_CONFIG_HOME)".to_string()
                        )
                )
            };
            Ok(CommandOutput {
                human: format!("{human}\n"),
                json: success_json(
                    "fleet",
                    cli.project.as_deref(),
                    json!({
                        "action": "controller-show",
                        "settings_path": path,
                        "default_controller": settings.fleet.as_ref().and_then(|fleet| fleet.default_controller.as_deref()),
                        "controller": controller.as_ref().map(fleet_controller_json),
                    }),
                ),
            })
        }
        FleetControllerCommand::List => {
            let settings = load_user_settings();
            let path = user_settings_path();
            let fleet = settings.fleet.unwrap_or_default();
            let controllers = fleet
                .controllers
                .iter()
                .map(|(name, controller)| {
                    let resolved = resolve_user_path(&controller.path);
                    json!({
                        "name": name,
                        "path": controller.path,
                        "resolved_path": resolved.to_string_lossy(),
                        "default": fleet.default_controller.as_deref() == Some(name.as_str()),
                    })
                })
                .collect::<Vec<_>>();
            let mut lines = vec!["Fleet controllers:".to_string()];
            if controllers.is_empty() {
                lines.push("  (none)".to_string());
            }
            for item in &controllers {
                let marker = if item["default"].as_bool().unwrap_or(false) {
                    " *"
                } else {
                    ""
                };
                lines.push(format!(
                    "  {}{} — {}",
                    item["name"].as_str().unwrap_or("?"),
                    marker,
                    item["resolved_path"].as_str().unwrap_or("?")
                ));
            }
            Ok(CommandOutput {
                human: format!("{}\n", lines.join("\n")),
                json: success_json(
                    "fleet",
                    cli.project.as_deref(),
                    json!({
                        "action": "controller-list",
                        "settings_path": path,
                        "default_controller": fleet.default_controller,
                        "controllers": controllers,
                    }),
                ),
            })
        }
        FleetControllerCommand::Set { name, path } => {
            validate_fleet_controller_name(name)?;
            let mut resolved = resolve_user_path(&path.to_string_lossy());
            if !resolved.is_absolute() {
                let cwd = project_root(cli).map_err(internal_error)?;
                resolved = cwd.join(resolved);
            }
            let context = load_required_context_for_path(cli, &resolved)?;
            save_fleet_controller_pointer(name, &resolved)?;
            Ok(CommandOutput {
                human: format!("Fleet controller `{name}` set to {}.\n", resolved.display()),
                json: success_json(
                    "fleet",
                    cli.project.as_deref(),
                    json!({
                        "action": "controller-set",
                        "controller": {
                            "id": name,
                            "path": resolved.to_string_lossy(),
                            "source": "user_default",
                            "config_path": project_config_path(&resolved, cli.config.as_deref()).to_string_lossy(),
                            "registry_digest": current_config_digest(&context).ok(),
                        },
                    }),
                ),
            })
        }
        FleetControllerCommand::ClearDefault => {
            let mut settings = load_user_settings();
            if let Some(fleet) = settings.fleet.as_mut() {
                fleet.default_controller = None;
            }
            save_user_settings(&settings).map_err(internal_error)?;
            Ok(CommandOutput {
                human: "Cleared default Fleet controller.\n".to_string(),
                json: success_json(
                    "fleet",
                    cli.project.as_deref(),
                    json!({
                        "action": "controller-clear-default",
                        "default_controller": Value::Null,
                    }),
                ),
            })
        }
    }
}

fn validate_fleet_controller_name(name: &str) -> std::result::Result<(), CliError> {
    if !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Ok(());
    }
    Err(CliError::new(
        EXIT_STATE,
        format!(
            "Invalid Fleet controller name `{name}`.\nHint: use only ASCII letters, numbers, '.', '_', and '-'."
        ),
    ))
}

fn resolve_fleet_controller_init_path(
    cli: &Cli,
    name: &str,
    path: Option<&Path>,
) -> std::result::Result<PathBuf, CliError> {
    if let Some(path) = path {
        let mut resolved = resolve_user_path(&path.to_string_lossy());
        if !resolved.is_absolute() {
            let cwd = project_root(cli).map_err(internal_error)?;
            resolved = cwd.join(resolved);
        }
        return Ok(resolved);
    }
    let Some(config_dir) = metactl_user_config_dir() else {
        return Err(CliError::new(
            EXIT_STATE,
            "HOME (or XDG_CONFIG_HOME) is not set; cannot create a default Fleet controller path.",
        ));
    };
    Ok(config_dir.join("fleet").join(name))
}

fn save_fleet_controller_pointer(name: &str, path: &Path) -> std::result::Result<(), CliError> {
    let mut settings = load_user_settings();
    let fleet = settings
        .fleet
        .get_or_insert_with(UserFleetSettings::default);
    fleet.controllers.insert(
        name.to_string(),
        UserFleetController {
            path: path.to_string_lossy().to_string(),
        },
    );
    fleet.default_controller = Some(name.to_string());
    save_user_settings(&settings).map_err(internal_error)
}

fn fleet_controller_readme(name: &str, path: &Path) -> String {
    format!(
        r#"# metactl Fleet Controller: {name}

This directory is an explicit local Fleet controller.

- `metactl.yaml` owns the `linked_projects` registry.
- User-global config stores only a pointer to this directory.
- `metactl fleet sync --preview` is the default safe review command.
- `metactl --yes --no-input fleet sync --apply` applies across selected projects.

Add projects manually:

```yaml
linked_projects:
  - id: example
    path: /path/to/repo
```

Controller path: {path}
"#,
        path = path.display()
    )
}

fn cmd_fleet_list(cli: &Cli) -> std::result::Result<CommandOutput, CliError> {
    let controller = resolve_fleet_controller(cli)?;
    let projects =
        fleet_projects_for_output(&controller.project_root, &controller.context.config_file);
    let project_json = projects
        .iter()
        .map(fleet_project_list_json)
        .collect::<Vec<_>>();
    let mut lines = fleet_controller_human_header(&controller);
    lines.push("Fleet projects:".to_string());
    if project_json.is_empty() {
        lines.push("  (none configured)".to_string());
    }
    for project in &project_json {
        lines.push(format!(
            "  {:<18} {:<14} {}",
            project["id"].as_str().unwrap_or("?"),
            project["status"].as_str().unwrap_or("?"),
            project["path"].as_str().unwrap_or("?")
        ));
    }
    Ok(CommandOutput {
        human: project_human_output(&controller.project_root, lines.join("\n")),
        json: success_json(
            "fleet",
            Some(&controller.project_root),
            json!({
                "action": "list",
                "controller": fleet_controller_json(&controller),
                "projects": project_json,
            }),
        ),
    })
}

fn cmd_fleet_status(
    cli: &Cli,
    args: &FleetStatusArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let controller = resolve_fleet_controller(cli)?;
    let projects = select_fleet_projects(
        &controller.project_root,
        &controller.context.config_file,
        &args.ids,
        args.include_disabled,
    )?;
    let statuses = projects
        .iter()
        .map(|project| fleet_project_status_json(project))
        .collect::<Vec<_>>();
    let mut lines = fleet_controller_human_header(&controller);
    lines.push("Fleet status:".to_string());
    for status in &statuses {
        lines.push(format!(
            "  {:<18} {:<14} {}",
            status["id"].as_str().unwrap_or("?"),
            status["status"].as_str().unwrap_or("?"),
            status["path"].as_str().unwrap_or("?")
        ));
    }
    append_fleet_codex_skill_scope_note(&mut lines);
    Ok(CommandOutput {
        human: project_human_output(&controller.project_root, lines.join("\n")),
        json: success_json(
            "fleet",
            Some(&controller.project_root),
            json!({
                "action": "status",
                "controller": fleet_controller_json(&controller),
                "projects": statuses,
                "scope_note": CODEX_FLEET_SCOPE_NOTE,
            }),
        ),
    })
}

fn cmd_fleet_sync(cli: &Cli, args: &FleetSyncArgs) -> std::result::Result<CommandOutput, CliError> {
    let controller = resolve_fleet_controller(cli)?;
    let apply = args.apply;
    if apply && !(cli.yes && cli.no_input_enabled()) {
        return Err(CliError::new(
            EXIT_STATE,
            "fleet sync --apply requires explicit --yes --no-input confirmation",
        ));
    }
    let projects = select_fleet_projects(
        &controller.project_root,
        &controller.context.config_file,
        &args.ids,
        args.include_disabled,
    )?;
    let mut results = Vec::new();
    for project in &projects {
        let mut result = linked_project_json(project);
        if project.status != LinkedProjectStatus::Ready {
            result["result"] = json!("skipped");
            results.push(result);
            continue;
        }
        let fleet_sync_adopt = match fleet_sync_adopt_for_project(project) {
            Ok(mode) => mode,
            Err(err) => {
                result["status"] = json!("failed");
                result["result"] = json!("invalid_config");
                result["message"] = json!(err.to_string());
                results.push(result);
                continue;
            }
        };
        result["fleet_sync_adopt"] = json!(fleet_sync_adopt_label(fleet_sync_adopt));
        if !apply {
            result["status"] = json!("planned");
            result["result"] = json!("preview");
            result["planned_command"] = json!(fleet_sync_command_label(fleet_sync_adopt));
            attach_codex_skill_visibility(&mut result, &project.path);
            results.push(result);
            continue;
        }
        if !args.allow_dirty && git_worktree_dirty(&project.path).map_err(internal_error)? {
            result["status"] = json!("failed");
            result["result"] = json!("dirty_worktree");
            result["message"] = json!(
                "dirty Git worktree; review and commit/stash changes, or rerun with --allow-dirty"
            );
            results.push(result);
            continue;
        }
        match run_project_sync(project, fleet_sync_adopt) {
            Ok(sync_json) => {
                result["status"] = json!("applied");
                result["result"] = json!("applied");
                result["sync"] = sync_json;
            }
            Err(message) => {
                result["status"] = json!("failed");
                result["result"] = json!("sync_failed");
                result["message"] = json!(message);
            }
        }
        attach_codex_skill_visibility(&mut result, &project.path);
        results.push(result);
    }
    if apply {
        write_fleet_sync_log(&controller.project_root, &results).map_err(internal_error)?;
    }
    let failed = results.iter().any(|item| item["status"] == "failed");
    let mut lines = fleet_controller_human_header(&controller);
    lines.push(if apply {
        "Fleet sync applied:".to_string()
    } else {
        "Fleet sync preview:".to_string()
    });
    for item in &results {
        lines.push(format!(
            "  {:<18} {:<14} {}",
            item["id"].as_str().unwrap_or("?"),
            item["status"].as_str().unwrap_or("?"),
            item["path"].as_str().unwrap_or("?")
        ));
    }
    append_fleet_codex_skill_scope_note(&mut lines);
    let mut json_payload = success_json(
        "fleet",
        Some(&controller.project_root),
        json!({
            "action": "sync",
            "controller": fleet_controller_json(&controller),
            "preview": !apply,
            "projects": results,
            "scope_note": CODEX_FLEET_SCOPE_NOTE,
        }),
    );
    if failed {
        let details = fleet_sync_failure_details(&results);
        let mut err = CliError::new(EXIT_STATE, "one or more fleet projects failed");
        json_payload["ok"] = json!(false);
        json_payload["message"] = json!("one or more fleet projects failed");
        json_payload["details"] = json!(details);
        err.json = json_payload;
        err.details = details;
        return Err(err);
    }
    Ok(CommandOutput {
        human: project_human_output(&controller.project_root, lines.join("\n")),
        json: json_payload,
    })
}

fn fleet_sync_failure_details(results: &[Value]) -> Vec<String> {
    let failures: Vec<&Value> = results
        .iter()
        .filter(|item| item["status"] == "failed")
        .collect();
    let mut details: Vec<String> = failures
        .iter()
        .take(20)
        .map(|item| {
            let id = item["id"].as_str().unwrap_or("?");
            let path = item["path"].as_str().unwrap_or("?");
            let result = item["result"].as_str().unwrap_or("failed");
            let message = item["message"]
                .as_str()
                .map(fleet_sync_failure_message_summary)
                .unwrap_or_else(|| "no failure detail returned".to_string());
            format!("{id} ({path}) {result}: {message}")
        })
        .collect();
    let omitted = failures.len().saturating_sub(20);
    if omitted > 0 {
        details.push(format!(
            "{omitted} more failed project(s); rerun with --json for the full fleet payload"
        ));
    }
    details
}

fn fleet_sync_failure_message_summary(message: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(message) {
        let mut parts = Vec::new();
        if let Some(text) = value.get("message").and_then(Value::as_str) {
            parts.push(text.to_string());
        }
        if let Some(details) = value.get("details").and_then(Value::as_array) {
            parts.extend(
                details
                    .iter()
                    .filter_map(Value::as_str)
                    .take(3)
                    .map(ToString::to_string),
            );
        }
        if let Some(findings) = value
            .pointer("/source_audit/findings")
            .and_then(Value::as_array)
        {
            for finding in findings.iter().take(3) {
                let id = finding
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("source-audit");
                let text = finding
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("source audit finding");
                match finding.get("path").and_then(Value::as_str) {
                    Some(path) => parts.push(format!("{id}: {text} ({path})")),
                    None => parts.push(format!("{id}: {text}")),
                }
            }
        }
        if !parts.is_empty() {
            return fleet_sync_single_line(&parts.join("; "), 600);
        }
    }
    fleet_sync_single_line(message, 600)
}

fn fleet_sync_single_line(input: &str, max_chars: usize) -> String {
    let single_line = input.split_whitespace().collect::<Vec<&str>>().join(" ");
    if single_line.is_empty() {
        return "no failure detail returned".to_string();
    }
    if single_line.chars().count() <= max_chars {
        return single_line;
    }
    let keep_chars = max_chars.saturating_sub(3);
    let mut truncated = single_line.chars().take(keep_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

#[derive(Debug)]
struct FleetControllerContext {
    id: Option<String>,
    source: FleetControllerSource,
    project_root: PathBuf,
    context: metactl::project::ProjectContext,
}

#[derive(Debug, Clone, Copy)]
enum FleetControllerSource {
    CommandLine,
    Environment,
    CurrentProject,
    UserDefault,
}

fn resolve_fleet_controller(cli: &Cli) -> std::result::Result<FleetControllerContext, CliError> {
    if cli.project.is_some() || cli.config.is_some() {
        let project_root = project_root(cli).map_err(internal_error)?;
        let context = load_required_context(cli, &project_root)?;
        return Ok(FleetControllerContext {
            id: None,
            source: FleetControllerSource::CommandLine,
            project_root,
            context,
        });
    }

    if let Ok(raw_path) = std::env::var("METACTL_FLEET_CONTROLLER") {
        if !raw_path.trim().is_empty() {
            let project_root = resolve_user_path(raw_path.trim());
            let context = load_required_context_for_path(cli, &project_root)?;
            return Ok(FleetControllerContext {
                id: None,
                source: FleetControllerSource::Environment,
                project_root,
                context,
            });
        }
    }

    let cwd = project_root(cli).map_err(internal_error)?;
    let cwd_config = project_config_path(&cwd, cli.config.as_deref());
    if cwd_config.exists() {
        let context = load_required_context(cli, &cwd)?;
        if !context.config_file.linked_projects.is_empty() {
            return Ok(FleetControllerContext {
                id: None,
                source: FleetControllerSource::CurrentProject,
                project_root: cwd,
                context,
            });
        }
    }

    let settings = load_user_settings();
    if let Some(fleet) = settings.fleet {
        if let Some(default_controller) = fleet.default_controller {
            if let Some(controller) = fleet.controllers.get(&default_controller) {
                let project_root = resolve_user_path(&controller.path);
                let context = load_required_context_for_path(cli, &project_root)?;
                return Ok(FleetControllerContext {
                    id: Some(default_controller),
                    source: FleetControllerSource::UserDefault,
                    project_root,
                    context,
                });
            }
            return Err(CliError::new(
                EXIT_STATE,
                format!(
                    "Fleet default controller `{default_controller}` is not configured.\nHint: run `metactl fleet controller set {default_controller} /path/to/controller`."
                ),
            ));
        }
    }

    Err(CliError::new(
        EXIT_STATE,
        "Fleet controller not found.\nHint: run from a project with linked_projects, pass `--project /path/to/controller`, set METACTL_FLEET_CONTROLLER, or run `metactl fleet controller set personal /path/to/controller`.",
    ))
}

fn load_required_context_for_path(
    cli: &Cli,
    project_root: &Path,
) -> std::result::Result<metactl::project::ProjectContext, CliError> {
    load_project_context(
        project_root,
        cli.config.as_deref(),
        cli.profile.as_deref(),
        cli.overlay.as_deref(),
    )
    .map_err(|error| {
        let message = error.to_string();
        if message.contains("project config") && message.contains("does not exist") {
            missing_config_error(cli, project_root)
        } else {
            state_error(error)
        }
    })
}

fn missing_config_error(cli: &Cli, project_root: &Path) -> CliError {
    let config_path = project_config_path(project_root, cli.config.as_deref());
    let mut details = vec![
        "Next: metactl init --detect".to_string(),
        "Next: metactl init -t codex-cli".to_string(),
    ];
    if cli.config.is_some() {
        details.push("Check the --config PATH value.".to_string());
    } else {
        details.push("If the config lives elsewhere, pass --config PATH.".to_string());
    }
    CliError::new(
        EXIT_STATE,
        format!("Project config {} does not exist.", config_path.display()),
    )
    .with_details(details)
}

fn resolve_user_path(raw_path: &str) -> PathBuf {
    if raw_path == "~" {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(raw_path))
    } else if let Some(rest) = raw_path.strip_prefix("~/") {
        std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join(rest))
            .unwrap_or_else(|| PathBuf::from(raw_path))
    } else {
        PathBuf::from(raw_path)
    }
}

fn fleet_controller_human_header(controller: &FleetControllerContext) -> Vec<String> {
    vec![
        format!(
            "Fleet controller: {}",
            controller.id.as_deref().unwrap_or("(explicit)")
        ),
        format!(
            "Controller source: {}",
            fleet_controller_source_label(controller.source)
        ),
        format!("Controller path: {}", controller.project_root.display()),
    ]
}

fn fleet_controller_json(controller: &FleetControllerContext) -> Value {
    json!({
        "id": controller.id.as_deref(),
        "source": fleet_controller_source_label(controller.source),
        "path": controller.project_root.to_string_lossy(),
        "config_path": project_config_path(&controller.project_root, None).to_string_lossy(),
        "registry_digest": current_config_digest(&controller.context).ok(),
    })
}

fn fleet_controller_source_label(source: FleetControllerSource) -> &'static str {
    match source {
        FleetControllerSource::CommandLine => "command_line",
        FleetControllerSource::Environment => "environment",
        FleetControllerSource::CurrentProject => "current_project",
        FleetControllerSource::UserDefault => "user_default",
    }
}

fn fleet_projects_for_output(
    project_root: &Path,
    config: &ProjectConfigFile,
) -> Vec<LinkedProject> {
    metactl::project::discover_linked_projects(project_root, config)
}

fn select_fleet_projects(
    project_root: &Path,
    config: &ProjectConfigFile,
    ids: &[String],
    include_disabled: bool,
) -> std::result::Result<Vec<LinkedProject>, CliError> {
    let projects = fleet_projects_for_output(project_root, config);
    let selected = projects
        .into_iter()
        .filter(|project| ids.is_empty() || ids.iter().any(|id| id == &project.id))
        .filter(|project| include_disabled || project.status != LinkedProjectStatus::Disabled)
        .collect::<Vec<_>>();
    if !ids.is_empty() {
        let found = selected
            .iter()
            .map(|project| project.id.as_str())
            .collect::<BTreeSet<_>>();
        let missing = ids
            .iter()
            .filter(|id| !found.contains(id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(CliError::new(
                EXIT_STATE,
                format!("linked project id(s) not found: {}", missing.join(", ")),
            ));
        }
    }
    Ok(selected)
}

fn linked_project_json(project: &LinkedProject) -> Value {
    json!({
        "id": project.id,
        "path": project.path.to_string_lossy(),
        "config_path": project.config_path.to_string_lossy(),
        "profile": project.profile,
        "status": linked_project_status_label(project.status),
    })
}

fn fleet_project_list_json(project: &LinkedProject) -> Value {
    let mut value = linked_project_json(project);
    if project.status == LinkedProjectStatus::Ready {
        if let Err(err) =
            load_project_context(&project.path, None, project.profile.as_deref(), None)
        {
            value["status"] = json!("invalid_config");
            value["result"] = json!("invalid_config");
            value["message"] = json!(err.to_string());
            let details = error_details(&err);
            if !details.is_empty() {
                value["details"] = json!(details);
            }
        }
    }
    value
}

fn fleet_project_status_json(project: &LinkedProject) -> Value {
    let mut value = linked_project_json(project);
    if project.status == LinkedProjectStatus::Ready {
        match load_project_context(&project.path, None, project.profile.as_deref(), None) {
            Ok(context) => {
                let fleet_sync_adopt = fleet_sync_adopt_from_context(&context);
                let stale = metactl::project::lock_stale_reason(&context).ok().flatten();
                value["lock_stale"] = json!(stale.is_some());
                value["stale_reason"] = json!(stale);
                value["targets"] = json!(context.config_file.targets);
                value["packs"] = json!(context.config_file.packs);
                value["fleet_sync_adopt"] = json!(fleet_sync_adopt_label(fleet_sync_adopt));
                value["needs_sync"] =
                    json!(context.lock.targets.is_empty() || value["lock_stale"] == true);
                attach_codex_skill_visibility(&mut value, &project.path);
            }
            Err(err) => {
                value["status"] = json!("invalid_config");
                value["result"] = json!("invalid_config");
                value["message"] = json!(err.to_string());
                let details = error_details(&err);
                if !details.is_empty() {
                    value["details"] = json!(details);
                }
            }
        }
    }
    value
}

fn attach_codex_skill_visibility(value: &mut Value, project_root: &Path) {
    if let Ok(visibility) = codex_skill_visibility_json(project_root) {
        value["skill_visibility"] = visibility;
    }
}

fn append_fleet_codex_skill_scope_note(lines: &mut Vec<String>) {
    lines.push(format!("  Codex skill scope: {CODEX_FLEET_SCOPE_NOTE}"));
    lines.push("  next: metactl skills add <repo-skill-path> --scope user".to_string());
}

fn linked_project_status_label(status: LinkedProjectStatus) -> &'static str {
    match status {
        LinkedProjectStatus::Ready => "ready",
        LinkedProjectStatus::Disabled => "disabled",
        LinkedProjectStatus::MissingPath => "missing_path",
        LinkedProjectStatus::MissingConfig => "missing_config",
    }
}

fn git_worktree_dirty(project_root: &Path) -> Result<bool> {
    if !project_root.join(".git").exists() {
        return Ok(false);
    }
    let output = Command::new("git")
        .args([
            "-C",
            &project_root.to_string_lossy(),
            "status",
            "--porcelain",
        ])
        .output()
        .context("run git status --porcelain")?;
    if !output.status.success() {
        return Ok(true);
    }
    Ok(!output.stdout.is_empty())
}

fn run_project_sync(
    project: &LinkedProject,
    fleet_sync_adopt: FleetSyncAdoptMode,
) -> std::result::Result<Value, String> {
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let mut command = Command::new(exe);
    command
        .arg("--json")
        .arg("--yes")
        .arg("--no-input")
        .arg("--project")
        .arg(&project.path);
    if let Some(profile) = project.profile.as_ref() {
        command.arg("--profile").arg(profile);
    }
    command.arg("sync");
    if fleet_sync_adopt == FleetSyncAdoptMode::Patch {
        command.arg("--adopt").arg("patch");
    }
    let output = command.output().map_err(|err| err.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Err(if stderr.is_empty() { stdout } else { stderr });
    }
    serde_json::from_slice(&output.stdout).map_err(|err| err.to_string())
}

fn fleet_sync_adopt_for_project(project: &LinkedProject) -> Result<FleetSyncAdoptMode> {
    let context = load_project_context(&project.path, None, project.profile.as_deref(), None)
        .with_context(|| format!("load linked project {}", project.id))?;
    Ok(fleet_sync_adopt_from_context(&context))
}

fn fleet_sync_adopt_from_context(context: &metactl::project::ProjectContext) -> FleetSyncAdoptMode {
    context
        .config_file
        .defaults
        .as_ref()
        .and_then(|defaults| defaults.fleet_sync_adopt)
        .unwrap_or(FleetSyncAdoptMode::Patch)
}

fn fleet_sync_adopt_label(mode: FleetSyncAdoptMode) -> &'static str {
    match mode {
        FleetSyncAdoptMode::Patch => "patch",
        FleetSyncAdoptMode::Refuse => "refuse",
    }
}

fn fleet_sync_command_label(mode: FleetSyncAdoptMode) -> &'static str {
    match mode {
        FleetSyncAdoptMode::Patch => "metactl sync --adopt patch",
        FleetSyncAdoptMode::Refuse => "metactl sync",
    }
}

fn write_fleet_sync_log(project_root: &Path, results: &[Value]) -> Result<()> {
    let log_dir = project_root.join(".metactl").join("logs");
    fs::create_dir_all(&log_dir).with_context(|| format!("create {}", log_dir.display()))?;
    let entry = json!({
        "timestamp": fleet_timestamp(),
        "metactl_version": env!("CARGO_PKG_VERSION"),
        "projects": results.iter().map(redact_fleet_log_project).collect::<Vec<_>>(),
    });
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("fleet-sync.jsonl"))
        .context("open fleet sync log")?;
    use std::io::Write as _;
    writeln!(file, "{}", entry).context("write fleet sync log")
}

fn redact_fleet_log_project(project: &Value) -> Value {
    json!({
        "id": project["id"],
        "status": project["status"],
        "result": project["result"],
        "profile": project["profile"],
    })
}

fn fleet_timestamp() -> String {
    format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or_default()
    )
}

fn cmd_status(cli: &Cli, args: &StatusArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let config_path = project_config_path(&project_root, cli.config.as_deref());
    if !config_path.exists() {
        return Ok(CommandOutput {
            human: format!(
                "No metactl project found at {}.\nNext: metactl init",
                project_root.display()
            ),
            json: success_json(
                "status",
                Some(&project_root),
                json!({
                    "initialized": false,
                }),
            ),
        });
    }

    let context = load_required_context(cli, &project_root)?;
    let stale_reason = metactl::project::lock_stale_reason(&context).map_err(internal_error)?;
    let stale = stale_reason.is_some();
    let profile = profile_status_json(&context);
    let discoverability = discoverability_report(&context, &ConfigOverrides::default());
    let blocking_checks = discoverability.blocking_checks_json();
    let execution_readiness = if blocking_checks.is_empty() {
        "ready"
    } else {
        "blocked"
    };
    let shared_surface_rules =
        shared_surface_rules(context.registry.as_ref(), &context.config_file.targets);
    let mut source_state = source_state_json(&project_root, &context.config_file);
    let source_audit_findings = source_audit_findings(&project_root)?;
    if !source_audit_findings.is_empty() {
        source_state["state"] = json!("private_source_leak_risk");
        source_state["findings"] = json!(source_audit_findings);
    }
    let codex_skill_visibility =
        codex_skill_visibility_json(&project_root).map_err(internal_error)?;
    let agent_artifact_policy = agent_artifact_policy_json(&context.config_file);

    let targets = if let Some(target_id) = args.target.as_ref() {
        select_locked_targets(&context.lock, Some(target_id.clone())).unwrap_or_default()
    } else {
        context.lock.targets.clone()
    };

    // Build layers info
    let mut layers = Vec::new();
    layers.push(json!({
        "layer": "shared",
        "path": context.config_path.to_string_lossy(),
        "digest": context.lock.config_digest,
    }));
    if let Some(local_path) = context.local_config_path.as_ref() {
        layers.push(json!({
            "layer": "local",
            "path": local_path.to_string_lossy().to_string(),
            "digest": context.lock.local_config_digest,
        }));
    }
    if let Some(ap) = context.active_profile.as_ref() {
        layers.push(json!({
            "layer": "profile",
            "name": ap.name,
            "path": ap.path.to_string_lossy().to_string(),
            "digest": ap.digest,
        }));
    }
    if let Some(overlay_path) = context.overlay_path.as_ref() {
        layers.push(json!({
            "layer": "invocation",
            "path": overlay_path.to_string_lossy().to_string(),
            "digest": context.lock.overlay_digest,
        }));
    }

    // Hook summary
    let git_hooks_dir = project_root.join(".git").join("hooks");
    let hook_names = ["post-checkout", "post-merge"];
    let installed_hooks: Vec<&str> = hook_names
        .iter()
        .filter(|name| {
            let path = git_hooks_dir.join(name);
            path.exists()
                && std::fs::read_to_string(&path)
                    .map(|c| c.contains("metactl"))
                    .unwrap_or(false)
        })
        .copied()
        .collect();

    // Source summary
    let source_count = context.config_file.sources.len()
        + context
            .config_file
            .metadata
            .keys()
            .filter(|k| k.starts_with("source."))
            .count();
    let import_roots = metactl::project::discover_import_roots();

    // Resolve target projection from registry
    let target_projections: std::collections::BTreeMap<String, String> = context
        .registry
        .as_ref()
        .map(|reg| {
            reg.list_targets()
                .into_iter()
                .filter_map(|t| {
                    t.local_projection.as_ref().map(|lp| {
                        (
                            t.target_id.clone(),
                            format!("{:?}", lp.support).to_ascii_lowercase(),
                        )
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let configured_default_surface_mode = context
        .effective_config(&ConfigOverrides::default())
        .ok()
        .and_then(|config| {
            config
                .defaults
                .and_then(|defaults| defaults.surface_selection_mode)
        });
    let configured_surface_modes: std::collections::BTreeMap<String, String> = context
        .selected_targets(&ConfigOverrides::default())
        .unwrap_or_default()
        .into_iter()
        .map(|target| {
            let mode = configured_default_surface_mode
                .clone()
                .unwrap_or_else(|| target_surface_selection_mode(&target));
            (
                target.target_id.clone(),
                surface_selection_mode_label(&mode).to_string(),
            )
        })
        .collect();
    let applied_targets = targets
        .iter()
        .map(|target| {
            let projection = target_projections.get(&target.target.id).cloned();
            let configured_surface_selection_mode =
                configured_surface_modes.get(&target.target.id).cloned();
            let manifest_path = project_root.join(&target.compile_manifest_path);
            let (output_count, surface_selection_mode) = if manifest_path.exists() {
                load_compile_manifest(&manifest_path)
                    .map(|manifest| {
                        (
                            manifest.generated_outputs.len(),
                            manifest
                                .surface_selection_mode
                                .as_ref()
                                .map(surface_selection_mode_label)
                                .map(str::to_string),
                        )
                    })
                    .unwrap_or((0, None))
            } else {
                (0, None)
            };
            let surface_selection_mode_matches_config =
                match (&surface_selection_mode, &configured_surface_selection_mode) {
                    (Some(applied), Some(configured)) => Some(applied == configured),
                    _ => None,
                };
            json!({
                "target": target.target.id,
                "apply_mode": format!("{:?}", target.preferred_apply_mode).to_ascii_lowercase(),
                "compiled_at": target.compiled_at,
                "projection": projection,
                "generated_outputs": output_count,
                "surface_selection_mode": surface_selection_mode,
                "configured_surface_selection_mode": configured_surface_selection_mode,
                "surface_selection_mode_matches_config": surface_selection_mode_matches_config,
            })
        })
        .collect::<Vec<_>>();
    let surface_mode_mismatches = applied_targets
        .iter()
        .filter(|target| target["surface_selection_mode_matches_config"].as_bool() == Some(false))
        .map(|target| {
            json!({
                "target": target["target"].clone(),
                "applied": target["surface_selection_mode"].clone(),
                "configured": target["configured_surface_selection_mode"].clone(),
            })
        })
        .collect::<Vec<_>>();

    let mut lines = Vec::new();
    lines.push(format!("Project: {}", project_root.display()));
    lines.push(format!("  Role:    {}", context.config_file.role));
    lines.push(format!("  Policy:  {}", context.config_file.policy));
    lines.push(format!(
        "  Targets: {}",
        context.config_file.targets.join(", ")
    ));
    lines.push(format!(
        "  Packs:   {}",
        if context.config_file.packs.is_empty() {
            "(none)".to_string()
        } else {
            context.config_file.packs.join(", ")
        }
    ));

    // Lock + stale reason
    let lock_display = match &stale_reason {
        Some(reason) => format!("STALE ({reason}; re-run metactl sync)"),
        None => "ok".to_string(),
    };
    lines.push(format!("  Lock:    {}", lock_display));
    lines.push(format!("  Profile: {}", profile_status_message(&profile)));
    lines.push(format!("  Execution readiness: {}", execution_readiness));
    if !blocking_checks.is_empty() {
        lines.push("  Blockers:".to_string());
        for message in discoverability.human_blockers() {
            lines.push(format!("    {}", message));
        }
    }
    if !shared_surface_rules.is_empty() {
        lines.push("  Shared surfaces:".to_string());
        for rule in &shared_surface_rules {
            lines.push(format!("    {}", rule.human_line()));
        }
    }

    // Layers
    lines.push("  Layers:".to_string());
    lines.push(format!("    shared:  {}", context.config_path.display()));
    if let Some(local_path) = context.local_config_path.as_ref() {
        lines.push(format!("    local:   {}", local_path.display()));
    }
    if let Some(ap) = context.active_profile.as_ref() {
        lines.push(format!("    profile: {} ({})", ap.name, ap.path.display()));
    }

    // Hooks
    if !installed_hooks.is_empty() {
        lines.push(format!("  Hooks:   {}", installed_hooks.join(", ")));
    }

    // Sources
    let has_source_findings = source_state["findings"]
        .as_array()
        .map(|findings| !findings.is_empty())
        .unwrap_or(false);
    if source_count > 0 || !import_roots.is_empty() || has_source_findings {
        let total = source_count + import_roots.len();
        lines.push(format!("  Sources: {} configured", total));
        if let Some(state) = source_state["state"].as_str() {
            lines.push(format!("  Source state: {}", state));
            if let Some(findings) = source_state["findings"].as_array() {
                if !findings.is_empty() {
                    lines.push("  next: metactl audit sources".to_string());
                    for line in source_audit_summary_lines(findings, 3) {
                        lines.push(format!("    {line}"));
                    }
                }
            }
        }
    }

    if should_show_codex_skill_visibility(&context, &codex_skill_visibility) {
        append_codex_skill_visibility_lines(&mut lines, &codex_skill_visibility);
    }

    append_agent_artifact_policy_lines(&mut lines, &agent_artifact_policy);

    if targets.is_empty() {
        lines.push("  Applied: (none — run `metactl sync` to compile and apply)".to_string());
    } else {
        lines.push("  Applied targets:".to_string());
        for target in &applied_targets {
            let target_id = target["target"].as_str().unwrap_or("unknown");
            let output_count = target["generated_outputs"].as_u64().unwrap_or(0);
            let apply_mode = target["apply_mode"].as_str().unwrap_or("unknown");
            let surface_mode = target["surface_selection_mode"]
                .as_str()
                .unwrap_or("unknown");
            let next_surface =
                if target["surface_selection_mode_matches_config"].as_bool() == Some(false) {
                    target["configured_surface_selection_mode"]
                        .as_str()
                        .map(|mode| format!(", next sync: {mode}"))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
            let projection = target["projection"]
                .as_str()
                .map(|p| format!(", projection: {p}"))
                .unwrap_or_default();
            lines.push(format!(
                "    {target_id} ({output_count} files, apply: {apply_mode}, surface: {surface_mode}{next_surface}{projection})"
            ));
        }
    }

    let needs_sync = stale || targets.is_empty() || !surface_mode_mismatches.is_empty();
    if !blocking_checks.is_empty() {
        lines.push(String::new());
        lines.push("Next: metactl doctor".to_string());
    } else if needs_sync {
        lines.push(String::new());
        lines.push("Next: metactl sync".to_string());
    }

    Ok(CommandOutput {
        human: lines.join("\n"),
        json: success_json(
            "status",
            Some(&project_root),
            json!({
                "initialized": true,
                "role": context.config_file.role,
                "policy": context.config_file.policy,
                "targets": context.config_file.targets,
                "packs": context.config_file.packs,
                "lock_stale": stale,
                "stale_reason": stale_reason,
                "profile": profile,
                "shared_surface_rules": shared_surface_rules.iter().map(SharedSurfaceRule::to_json).collect::<Vec<_>>(),
                "layers": layers,
                "hooks": installed_hooks,
                "sources": {
                    "configured": source_count,
                    "auto_discovered": import_roots.len(),
                },
                "source_state": source_state,
                "skill_visibility": codex_skill_visibility,
                "agent_artifact_policy": agent_artifact_policy,
                "applied_targets": applied_targets,
                "surface_mode_mismatches": surface_mode_mismatches,
                "needs_sync": needs_sync,
                "execution_readiness": execution_readiness,
                "blocking_checks": blocking_checks,
            }),
        ),
    })
}

fn cmd_list(cli: &Cli, args: &ListArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_optional_context(cli, &project_root).map_err(internal_error)?;
    let registry = context.registry.or_else(|| {
        ensure_bundled_starter_library_root()
            .ok()
            .and_then(|default_root| LibraryRegistry::load_from_roots(&[default_root]).ok())
    });
    let Some(registry) = registry else {
        return Ok(CommandOutput {
            human: "No starter library is installed.\nNext: pass --starter-library during init or add library roots to metactl.yaml.".to_string(),
            json: success_json("list", Some(&project_root), json!({
                "classification": "no_corpus",
                "items": [],
            })),
        });
    };

    let installed = installed_ids(context.config_file.as_ref());
    match args.subject {
        ListSubject::Roles => {
            let mut items = registry
                .list_roles()
                .into_iter()
                .filter(|item| !args.installed || installed.contains(&item.id))
                .map(|item| {
                    json!({
                        "id": item.id,
                        "title": item.title,
                        "default_policy": item.default_policy_ref.map(|value| value.id),
                    })
                })
                .collect::<Vec<_>>();
            if let Some(limit) = args.limit {
                items.truncate(limit);
            }
            let human_lines: Vec<String> = items
                .iter()
                .map(|item| {
                    let id = item["id"].as_str().unwrap_or("");
                    let title = item["title"].as_str().unwrap_or("");
                    let marker = if installed.contains(id) { " *" } else { "" };
                    format!("  {:<24} {}{}", id, title, marker)
                })
                .collect();
            Ok(CommandOutput {
                human: format!("Roles (* = configured):\n{}", human_lines.join("\n")),
                json: success_json(
                    "list",
                    Some(&project_root),
                    json!({"subject": "roles", "items": items}),
                ),
            })
        }
        ListSubject::Policies => {
            let mut items = registry
                .list_policies()
                .into_iter()
                .filter(|item| !args.installed || installed.contains(&item.id))
                .map(|item| json!({"id": item.id, "title": item.title}))
                .collect::<Vec<_>>();
            if let Some(limit) = args.limit {
                items.truncate(limit);
            }
            let human_lines: Vec<String> = items
                .iter()
                .map(|item| {
                    let id = item["id"].as_str().unwrap_or("");
                    let title = item["title"].as_str().unwrap_or("");
                    let marker = if installed.contains(id) { " *" } else { "" };
                    format!("  {:<28} {}{}", id, title, marker)
                })
                .collect();
            Ok(CommandOutput {
                human: format!("Policies (* = configured):\n{}", human_lines.join("\n")),
                json: success_json(
                    "list",
                    Some(&project_root),
                    json!({"subject": "policies", "items": items}),
                ),
            })
        }
        ListSubject::Targets => {
            let mut items = registry
                .list_targets()
                .into_iter()
                .filter(|item| !args.installed || installed.contains(&item.target_id))
                .map(|item| json!({"id": item.target_id, "title": item.title}))
                .collect::<Vec<_>>();
            if let Some(limit) = args.limit {
                items.truncate(limit);
            }
            let human_lines: Vec<String> = items
                .iter()
                .map(|item| {
                    let id = item["id"].as_str().unwrap_or("");
                    let title = item["title"].as_str().unwrap_or("");
                    let marker = if installed.contains(id) { " *" } else { "" };
                    format!("  {:<20} {}{}", id, title, marker)
                })
                .collect();
            Ok(CommandOutput {
                human: format!(
                    "Targets (* = configured):\n{}\n\nUsage: metactl init -t <target>",
                    human_lines.join("\n")
                ),
                json: success_json(
                    "list",
                    Some(&project_root),
                    json!({"subject": "targets", "items": items}),
                ),
            })
        }
        ListSubject::Packs => {
            let mut items = registry
                .list_packs()
                .into_iter()
                .filter(|item| !args.installed || installed.contains(&item.manifest.id))
                .filter(|item| args.candidate || !is_candidate_pack(&item.promotion_status))
                .filter(|_| args.starter_only || !args.starter_only)
                .map(|item| {
                    json!({
                        "id": item.manifest.id,
                        "title": item.manifest.title,
                        "promotion_status": format!("{:?}", item.promotion_status).to_ascii_lowercase(),
                        "lifecycle": item.manifest.lifecycle,
                    })
                })
                .collect::<Vec<_>>();
            if let Some(limit) = args.limit {
                items.truncate(limit);
            }
            let human_lines: Vec<String> = items
                .iter()
                .map(|item| {
                    let id = item["id"].as_str().unwrap_or("");
                    let title = item["title"].as_str().unwrap_or("");
                    let lifecycle = item["lifecycle"]["status"].as_str().unwrap_or("");
                    let marker = if installed.contains(id) { " *" } else { "" };
                    let lifecycle_suffix = if lifecycle.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", lifecycle)
                    };
                    format!("  {:<28} {}{}{}", id, title, marker, lifecycle_suffix)
                })
                .collect();
            Ok(CommandOutput {
                human: format!(
                    "Packs (* = configured):\n{}\n\nUsage: metactl add <pack-id>",
                    human_lines.join("\n")
                ),
                json: success_json(
                    "list",
                    Some(&project_root),
                    json!({"subject": "packs", "items": items}),
                ),
            })
        }
    }
}

fn cmd_search(cli: &Cli, args: &SearchArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_required_context(cli, &project_root)?;
    if !context.has_corpus() {
        return Ok(no_corpus_output("search", &project_root));
    }
    let overrides = ConfigOverrides {
        role: args.role.clone(),
        policy: args.policy.clone(),
        targets: args.target.clone().into_iter().collect(),
    };
    let config = context.effective_config(&overrides).map_err(state_error)?;
    let kernel = kernel_from_context(&context).map_err(internal_error)?;
    let result = kernel
        .search(SearchParams {
            query: args.query.clone(),
            config,
            overlay: context.overlay.clone(),
            candidate_packs: Vec::new(),
            limit: args.limit,
        })
        .map_err(state_error)?;
    Ok(search_output(
        &project_root,
        &args.query,
        &result,
        args.show_suppressed,
    ))
}

fn cmd_stats(cli: &Cli, args: &StatsArgs) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        StatsCommand::Rebuild(args) => cmd_stats_rebuild(cli, args),
        StatsCommand::Show(args) => cmd_stats_show(cli, args),
    }
}

fn cmd_stats_rebuild(
    cli: &Cli,
    args: &StatsRebuildArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let stats = surface_usage::rebuild_usage_stats(
        &project_root,
        args.events.as_deref(),
        args.output.as_deref(),
    )
    .map_err(state_error)?;
    let stats_path = args
        .output
        .clone()
        .unwrap_or_else(|| surface_usage::usage_stats_path(&project_root));
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Usage stats rebuilt.\nEvents: {}\nPacks: {}\nStats: {}",
                stats.event_count,
                stats.packs.len(),
                relative_to_project(&project_root, &stats_path)
            ),
        ),
        json: success_json(
            "stats",
            Some(&project_root),
            json!({
                "action": "rebuild",
                "stats_path": relative_to_project(&project_root, &stats_path),
                "stats": stats,
            }),
        ),
    })
}

fn cmd_stats_show(cli: &Cli, args: &StatsShowArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let stats = surface_usage::load_or_rebuild_usage_stats(&project_root).map_err(state_error)?;
    let packs = if let Some(pack_id) = args.pack.as_deref() {
        stats
            .packs
            .iter()
            .filter(|pack| pack.pack_id == pack_id)
            .cloned()
            .collect::<Vec<_>>()
    } else {
        stats.packs.clone()
    };
    let mut lines = vec![
        "Usage stats.".to_string(),
        format!("Events: {}", stats.event_count),
        format!("Packs: {}", packs.len()),
    ];
    lines.extend(packs.iter().map(|pack| {
        format!(
            "- {} score={} events={} verified={} commands={}",
            pack.pack_id, pack.score, pack.event_count, pack.task_verified, pack.command_invoked
        )
    }));
    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "stats",
            Some(&project_root),
            json!({
                "action": "show",
                "stats_path": relative_to_project(&project_root, &surface_usage::usage_stats_path(&project_root)),
                "packs": packs,
            }),
        ),
    })
}

fn cmd_surface(cli: &Cli, args: &SurfaceArgs) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        SurfaceCommand::Report(args) => cmd_surface_report(cli, args),
        SurfaceCommand::Pin(args) => cmd_surface_pin(cli, args),
        SurfaceCommand::Block(args) => cmd_surface_block(cli, args),
        SurfaceCommand::Reset(args) => cmd_surface_reset(cli, args),
    }
}

fn cmd_surface_report(
    cli: &Cli,
    args: &SurfaceReportArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let lifecycle_mode = SurfaceLifecycleMode::from(args.lifecycle_mode);
    let rebuild_trigger = if args.scheduled {
        SurfaceRebuildTrigger::Scheduled
    } else {
        SurfaceRebuildTrigger::Opportunistic
    };
    let report = write_surface_report_for_project(
        &project_root,
        cli.config.as_deref(),
        cli.profile.as_deref(),
        cli.overlay.as_deref(),
        lifecycle_mode,
        rebuild_trigger,
    )?;
    let mut lines = vec![
        "Surface recommendation report written.".to_string(),
        format!("Lifecycle mode: {:?}", report.lifecycle_mode),
        format!(
            "Pending recommendations: {}",
            report.pending_recommendation_count
        ),
        format!("JSON: {}", report.report_json_path),
        format!("Dashboard: {}", report.report_markdown_path),
    ];
    if args.scheduled {
        lines.push("Scheduled run: report-only; adapters were not mutated.".to_string());
    }
    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "surface",
            Some(&project_root),
            json!({
                "action": "report",
                "report": report,
            }),
        ),
    })
}

fn write_surface_report_for_project(
    project_root: &Path,
    config_override: Option<&Path>,
    profile: Option<&str>,
    overlay_path: Option<&Path>,
    lifecycle_mode: SurfaceLifecycleMode,
    rebuild_trigger: SurfaceRebuildTrigger,
) -> std::result::Result<SurfaceReport, CliError> {
    let context = load_project_context(project_root, config_override, profile, overlay_path)
        .map_err(state_error)?;
    let stats = surface_usage::load_or_rebuild_usage_stats(project_root).map_err(state_error)?;
    let overrides = surface_usage::load_surface_overrides(project_root).map_err(state_error)?;
    let known_pack_ids = known_surface_pack_ids(&context).map_err(state_error)?;
    let report = surface_usage::build_surface_report(
        project_root,
        lifecycle_mode,
        rebuild_trigger,
        &known_pack_ids,
        &stats,
        &overrides,
    );
    surface_usage::write_surface_report(project_root, &report).map_err(state_error)?;
    Ok(report)
}

#[derive(Debug, Clone)]
struct BackgroundSchedulerPlan {
    scope: BackgroundScopeArg,
    project_root: PathBuf,
    label: String,
    os: String,
    interval_minutes: u32,
    log_dir: PathBuf,
    executable: PathBuf,
    run_args: Vec<String>,
    files: Vec<(String, PathBuf, String)>,
    install_commands: Vec<Vec<String>>,
    status_commands: Vec<Vec<String>>,
    uninstall_commands: Vec<Vec<String>>,
}

impl BackgroundSchedulerPlan {
    fn run_command_display(&self) -> String {
        command_display(&self.executable, &self.run_args)
    }

    fn to_json(&self) -> Value {
        json!({
            "scope": self.scope.as_str(),
            "project_root": self.project_root,
            "label": self.label,
            "os": self.os,
            "interval_minutes": self.interval_minutes,
            "log_dir": self.log_dir,
            "run_command": self.run_command_display(),
            "files": self.files.iter().map(|(kind, path, _)| json!({
                "kind": kind,
                "path": path,
            })).collect::<Vec<_>>(),
            "install_commands": self.install_commands.iter().map(|cmd| command_vec_display(cmd)).collect::<Vec<_>>(),
            "status_commands": self.status_commands.iter().map(|cmd| command_vec_display(cmd)).collect::<Vec<_>>(),
            "uninstall_commands": self.uninstall_commands.iter().map(|cmd| command_vec_display(cmd)).collect::<Vec<_>>(),
            "report_only": true,
            "mutates_adapters": false,
        })
    }
}

fn cmd_background(
    cli: &Cli,
    args: &BackgroundArgs,
) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        Some(BackgroundCommand::Plan(args)) => cmd_background_plan(cli, args),
        Some(BackgroundCommand::Install(args)) => cmd_background_install(cli, args),
        Some(BackgroundCommand::Status(args)) => cmd_background_status(cli, args),
        Some(BackgroundCommand::Uninstall(args)) => cmd_background_uninstall(cli, args),
        Some(BackgroundCommand::Run(args)) => cmd_background_run(cli, args),
        None => cmd_background_plan(
            cli,
            &BackgroundPlanArgs {
                scope: BackgroundScopeArg::Project,
                controller: None,
                interval_minutes: 60,
                log_dir: None,
                label: None,
            },
        ),
    }
}

fn cmd_background_plan(
    cli: &Cli,
    args: &BackgroundPlanArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let plan = build_background_plan(
        cli,
        args.scope,
        args.controller.as_deref(),
        args.interval_minutes,
        args.log_dir.as_deref(),
        args.label.as_deref(),
    )?;
    let mut lines = vec![
        "Background refresh plan:".to_string(),
        format!("  scope: {}", plan.scope.as_str()),
        format!("  scheduler: {}", plan.os),
        format!("  interval: {} minutes", plan.interval_minutes),
        format!("  run: {}", plan.run_command_display()),
    ];
    for (_, path, _) in &plan.files {
        lines.push(format!("  file: {}", path.display()));
    }
    lines.push("Report-only: adapters are not mutated.".to_string());
    Ok(CommandOutput {
        human: project_human_output(&plan.project_root, lines.join("\n")),
        json: success_json(
            "background",
            Some(&plan.project_root),
            json!({
                "action": "plan",
                "plan": plan.to_json(),
            }),
        ),
    })
}

fn cmd_background_install(
    cli: &Cli,
    args: &BackgroundInstallArgs,
) -> std::result::Result<CommandOutput, CliError> {
    if !args.yes {
        let plan = build_background_plan(
            cli,
            args.scope,
            args.controller.as_deref(),
            args.interval_minutes,
            args.log_dir.as_deref(),
            args.label.as_deref(),
        )?;
        let mut err = CliError::new(
            EXIT_STATE,
            "Background install creates persistent OS scheduler state and requires --yes.",
        )
        .with_details(vec![format!(
            "Next: metactl background install --scope {} --yes",
            args.scope.as_str()
        )]);
        if let Some(obj) = err.json.as_object_mut() {
            obj.insert(
                "code".to_string(),
                json!("background_confirmation_required"),
            );
            obj.insert("category".to_string(), json!("machine_state"));
            obj.insert("plan".to_string(), plan.to_json());
        }
        return Err(err);
    }
    let plan = build_background_plan(
        cli,
        args.scope,
        args.controller.as_deref(),
        args.interval_minutes,
        args.log_dir.as_deref(),
        args.label.as_deref(),
    )?;
    install_background_plan(&plan)?;
    Ok(CommandOutput {
        human: project_human_output(
            &plan.project_root,
            format!(
                "Background refresh installed.\nScheduler: {}\nLabel: {}\nInterval: {} minutes\nRun: {}",
                plan.os,
                plan.label,
                plan.interval_minutes,
                plan.run_command_display()
            ),
        ),
        json: success_json(
            "background",
            Some(&plan.project_root),
            json!({
                "action": "install",
                "installed": true,
                "plan": plan.to_json(),
            }),
        ),
    })
}

fn cmd_background_status(
    cli: &Cli,
    args: &BackgroundStatusArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let plan = build_background_plan(
        cli,
        args.scope,
        args.controller.as_deref(),
        60,
        args.log_dir.as_deref(),
        args.label.as_deref(),
    )?;
    let status = background_status(&plan);
    Ok(CommandOutput {
        human: project_human_output(
            &plan.project_root,
            format!(
                "Background refresh status.\nScheduler: {}\nLabel: {}\nInstalled: {}",
                plan.os,
                plan.label,
                status["installed"].as_bool().unwrap_or(false)
            ),
        ),
        json: success_json(
            "background",
            Some(&plan.project_root),
            json!({
                "action": "status",
                "plan": plan.to_json(),
                "status": status,
            }),
        ),
    })
}

fn cmd_background_uninstall(
    cli: &Cli,
    args: &BackgroundUninstallArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let plan = build_background_plan(
        cli,
        args.scope,
        args.controller.as_deref(),
        60,
        args.log_dir.as_deref(),
        args.label.as_deref(),
    )?;
    if !args.yes {
        let mut err = CliError::new(
            EXIT_STATE,
            "Background uninstall removes persistent OS scheduler state and requires --yes.",
        )
        .with_details(vec![format!(
            "Next: metactl background uninstall --scope {} --yes",
            args.scope.as_str()
        )]);
        if let Some(obj) = err.json.as_object_mut() {
            obj.insert(
                "code".to_string(),
                json!("background_confirmation_required"),
            );
            obj.insert("category".to_string(), json!("machine_state"));
            obj.insert("plan".to_string(), plan.to_json());
        }
        return Err(err);
    }
    uninstall_background_plan(&plan)?;
    Ok(CommandOutput {
        human: project_human_output(
            &plan.project_root,
            format!("Background refresh uninstalled.\nLabel: {}", plan.label),
        ),
        json: success_json(
            "background",
            Some(&plan.project_root),
            json!({
                "action": "uninstall",
                "uninstalled": true,
                "plan": plan.to_json(),
            }),
        ),
    })
}

fn cmd_background_run(
    cli: &Cli,
    args: &BackgroundRunArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let log_dir = args
        .log_dir
        .clone()
        .unwrap_or_else(default_background_log_dir);
    fs::create_dir_all(&log_dir).map_err(internal_error)?;
    let mut results = Vec::new();
    let mut failures = 0usize;
    match args.scope {
        BackgroundScopeArg::Project => {
            let project_root = project_root(cli).map_err(internal_error)?;
            let result = run_background_report_for_project(
                &project_root,
                cli.config.as_deref(),
                cli.profile.as_deref(),
                cli.overlay.as_deref(),
                None,
            );
            if result["ok"] != true {
                failures += 1;
            }
            results.push(result);
            append_background_run_log(&log_dir, args.scope, &project_root, &results, failures)?;
            background_run_output(args.scope, project_root, log_dir, results, failures)
        }
        BackgroundScopeArg::Fleet => {
            let controller = resolve_background_fleet_controller(cli, args.controller.as_deref())?;
            let controller_result = run_background_report_for_project(
                &controller.project_root,
                None,
                cli.profile.as_deref(),
                cli.overlay.as_deref(),
                Some("fleet-controller"),
            );
            if controller_result["ok"] != true {
                failures += 1;
            }
            results.push(controller_result);
            let projects = fleet_projects_for_output(
                &controller.project_root,
                &controller.context.config_file,
            );
            for project in projects {
                if project.status == LinkedProjectStatus::Disabled {
                    results.push(json!({
                        "id": project.id,
                        "path": project.path,
                        "ok": true,
                        "skipped": true,
                        "status": linked_project_status_label(project.status),
                    }));
                    continue;
                }
                let result = run_background_report_for_project(
                    &project.path,
                    None,
                    project.profile.as_deref(),
                    None,
                    Some(&project.id),
                );
                if result["ok"] != true {
                    failures += 1;
                }
                results.push(result);
            }
            append_background_run_log(
                &log_dir,
                args.scope,
                &controller.project_root,
                &results,
                failures,
            )?;
            background_run_output(
                args.scope,
                controller.project_root,
                log_dir,
                results,
                failures,
            )
        }
    }
}

fn background_run_output(
    scope: BackgroundScopeArg,
    project_root: PathBuf,
    log_dir: PathBuf,
    results: Vec<Value>,
    failures: usize,
) -> std::result::Result<CommandOutput, CliError> {
    let lines = vec![
        "Background refresh complete.".to_string(),
        format!("Scope: {}", scope.as_str()),
        format!("Projects: {}", results.len()),
        format!("Failures: {failures}"),
        format!("Log dir: {}", log_dir.display()),
    ];
    let output = CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "background",
            Some(&project_root),
            json!({
                "action": "run",
                "scope": scope.as_str(),
                "log_dir": log_dir,
                "project_count": results.len(),
                "failure_count": failures,
                "results": results,
            }),
        ),
    };
    if failures == 0 {
        Ok(output)
    } else {
        let mut err = CliError::new(EXIT_STATE, "one or more background reports failed");
        err.json = output.json;
        Err(err)
    }
}

fn run_background_report_for_project(
    project_root: &Path,
    config_override: Option<&Path>,
    profile: Option<&str>,
    overlay_path: Option<&Path>,
    id: Option<&str>,
) -> Value {
    match write_surface_report_for_project(
        project_root,
        config_override,
        profile,
        overlay_path,
        SurfaceLifecycleMode::Recommend,
        SurfaceRebuildTrigger::Scheduled,
    ) {
        Ok(report) => json!({
            "id": id,
            "path": project_root,
            "ok": true,
            "report_json_path": report.report_json_path,
            "report_markdown_path": report.report_markdown_path,
            "pending_recommendation_count": report.pending_recommendation_count,
        }),
        Err(err) => json!({
            "id": id,
            "path": project_root,
            "ok": false,
            "message": err.message,
            "details": err.details,
        }),
    }
}

fn append_background_run_log(
    log_dir: &Path,
    scope: BackgroundScopeArg,
    project_root: &Path,
    results: &[Value],
    failures: usize,
) -> std::result::Result<(), CliError> {
    fs::create_dir_all(log_dir).map_err(internal_error)?;
    let entry = json!({
        "timestamp": now_string(),
        "scope": scope.as_str(),
        "project_root": project_root,
        "project_count": results.len(),
        "failure_count": failures,
        "results": results,
    });
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("background-runs.jsonl"))
        .map_err(internal_error)?;
    writeln!(file, "{entry}").map_err(internal_error)
}

fn build_background_plan(
    cli: &Cli,
    scope: BackgroundScopeArg,
    controller: Option<&Path>,
    interval_minutes: u32,
    log_dir_override: Option<&Path>,
    label_override: Option<&str>,
) -> std::result::Result<BackgroundSchedulerPlan, CliError> {
    let project_root = match scope {
        BackgroundScopeArg::Project => project_root(cli).map_err(internal_error)?,
        BackgroundScopeArg::Fleet => {
            resolve_background_fleet_controller(cli, controller)?.project_root
        }
    };
    let log_dir = log_dir_override
        .map(PathBuf::from)
        .unwrap_or_else(default_background_log_dir);
    let executable = std::env::current_exe().map_err(internal_error)?;
    let label = label_override.map(ToOwned::to_owned).unwrap_or_else(|| {
        format!(
            "dev.metactl.surface-report.{}.{}",
            scope.as_str(),
            short_path_hash(&project_root)
        )
    });
    let mut run_args = vec![
        "--project".to_string(),
        project_root.to_string_lossy().to_string(),
        "--no-input".to_string(),
        "--json".to_string(),
        "background".to_string(),
        "run".to_string(),
        "--scope".to_string(),
        scope.as_str().to_string(),
        "--log-dir".to_string(),
        log_dir.to_string_lossy().to_string(),
    ];
    if scope == BackgroundScopeArg::Fleet {
        run_args.push("--controller".to_string());
        run_args.push(project_root.to_string_lossy().to_string());
    }
    background_scheduler_plan_for_os(
        scope,
        project_root,
        label,
        interval_minutes.max(1),
        log_dir,
        executable,
        run_args,
    )
}

fn background_scheduler_plan_for_os(
    scope: BackgroundScopeArg,
    project_root: PathBuf,
    label: String,
    interval_minutes: u32,
    log_dir: PathBuf,
    executable: PathBuf,
    run_args: Vec<String>,
) -> std::result::Result<BackgroundSchedulerPlan, CliError> {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    let run_command = command_display(&executable, &run_args);
    #[cfg(target_os = "macos")]
    {
        let home = home_dir().ok_or_else(|| state_error(anyhow!("HOME is not set")))?;
        let plist_path = home
            .join("Library/LaunchAgents")
            .join(format!("{label}.plist"));
        let stdout_path = log_dir.join(format!("{label}.out.log"));
        let stderr_path = log_dir.join(format!("{label}.err.log"));
        let plist = macos_launch_agent_plist(
            &label,
            &executable,
            &run_args,
            interval_minutes,
            &stdout_path,
            &stderr_path,
        );
        let uid = current_uid_string().unwrap_or_else(|| "$(id -u)".to_string());
        return Ok(BackgroundSchedulerPlan {
            scope,
            project_root,
            label: label.clone(),
            os: "macos-launchd".to_string(),
            interval_minutes,
            log_dir,
            executable,
            run_args,
            files: vec![("launchagent".to_string(), plist_path.clone(), plist)],
            install_commands: vec![
                vec![
                    "mkdir".to_string(),
                    "-p".to_string(),
                    plist_path.parent().unwrap().to_string_lossy().to_string(),
                ],
                vec![
                    "launchctl".to_string(),
                    "bootstrap".to_string(),
                    format!("gui/{uid}"),
                    plist_path.to_string_lossy().to_string(),
                ],
                vec![
                    "launchctl".to_string(),
                    "enable".to_string(),
                    format!("gui/{uid}/{label}"),
                ],
                vec![
                    "launchctl".to_string(),
                    "kickstart".to_string(),
                    "-k".to_string(),
                    format!("gui/{uid}/{label}"),
                ],
            ],
            status_commands: vec![vec![
                "launchctl".to_string(),
                "print".to_string(),
                format!("gui/{uid}/{label}"),
            ]],
            uninstall_commands: vec![
                vec![
                    "launchctl".to_string(),
                    "bootout".to_string(),
                    format!("gui/{uid}"),
                    plist_path.to_string_lossy().to_string(),
                ],
                vec!["rm".to_string(), plist_path.to_string_lossy().to_string()],
            ],
        });
    }
    #[cfg(target_os = "linux")]
    {
        let config_dir =
            user_config_dir().ok_or_else(|| state_error(anyhow!("HOME is not set")))?;
        let systemd_dir = config_dir.join("systemd/user");
        let service_path = systemd_dir.join(format!("{label}.service"));
        let timer_path = systemd_dir.join(format!("{label}.timer"));
        let service = linux_systemd_service(&label, &run_command, &log_dir);
        let timer = linux_systemd_timer(&label, interval_minutes);
        return Ok(BackgroundSchedulerPlan {
            scope,
            project_root,
            label: label.clone(),
            os: "linux-systemd-user".to_string(),
            interval_minutes,
            log_dir,
            executable,
            run_args,
            files: vec![
                ("systemd-service".to_string(), service_path.clone(), service),
                ("systemd-timer".to_string(), timer_path.clone(), timer),
            ],
            install_commands: vec![
                vec![
                    "mkdir".to_string(),
                    "-p".to_string(),
                    systemd_dir.to_string_lossy().to_string(),
                ],
                vec![
                    "systemctl".to_string(),
                    "--user".to_string(),
                    "daemon-reload".to_string(),
                ],
                vec![
                    "systemctl".to_string(),
                    "--user".to_string(),
                    "enable".to_string(),
                    "--now".to_string(),
                    format!("{label}.timer"),
                ],
            ],
            status_commands: vec![vec![
                "systemctl".to_string(),
                "--user".to_string(),
                "status".to_string(),
                format!("{label}.timer"),
            ]],
            uninstall_commands: vec![
                vec![
                    "systemctl".to_string(),
                    "--user".to_string(),
                    "disable".to_string(),
                    "--now".to_string(),
                    format!("{label}.timer"),
                ],
                vec![
                    "rm".to_string(),
                    service_path.to_string_lossy().to_string(),
                    timer_path.to_string_lossy().to_string(),
                ],
                vec![
                    "systemctl".to_string(),
                    "--user".to_string(),
                    "daemon-reload".to_string(),
                ],
            ],
        });
    }
    #[cfg(target_os = "windows")]
    {
        let task_name = format!(r"\metactl\{label}");
        return Ok(BackgroundSchedulerPlan {
            scope,
            project_root,
            label: task_name.clone(),
            os: "windows-scheduled-task".to_string(),
            interval_minutes,
            log_dir,
            executable,
            run_args,
            files: Vec::new(),
            install_commands: vec![vec![
                "schtasks".to_string(),
                "/Create".to_string(),
                "/F".to_string(),
                "/SC".to_string(),
                "MINUTE".to_string(),
                "/MO".to_string(),
                interval_minutes.to_string(),
                "/TN".to_string(),
                task_name.clone(),
                "/TR".to_string(),
                run_command,
            ]],
            status_commands: vec![vec![
                "schtasks".to_string(),
                "/Query".to_string(),
                "/TN".to_string(),
                task_name.clone(),
            ]],
            uninstall_commands: vec![vec![
                "schtasks".to_string(),
                "/Delete".to_string(),
                "/F".to_string(),
                "/TN".to_string(),
                task_name,
            ]],
        });
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (
            scope,
            project_root,
            label,
            interval_minutes,
            log_dir,
            executable,
            run_args,
        );
        Err(state_error(anyhow!(
            "background scheduler install is not supported on this OS"
        )))
    }
}

fn install_background_plan(plan: &BackgroundSchedulerPlan) -> std::result::Result<(), CliError> {
    fs::create_dir_all(&plan.log_dir).map_err(internal_error)?;
    for (_, path, contents) in &plan.files {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(internal_error)?;
        }
        fs::write(path, contents).map_err(internal_error)?;
    }
    install_background_plan_for_os(plan)
}

fn uninstall_background_plan(plan: &BackgroundSchedulerPlan) -> std::result::Result<(), CliError> {
    uninstall_background_plan_for_os(plan)
}

#[cfg(target_os = "macos")]
fn install_background_plan_for_os(
    plan: &BackgroundSchedulerPlan,
) -> std::result::Result<(), CliError> {
    let Some((_, plist_path, _)) = plan.files.first() else {
        return Err(state_error(anyhow!("missing LaunchAgent plist path")));
    };
    let uid = current_uid_string().ok_or_else(|| state_error(anyhow!("could not resolve uid")))?;
    let _ = Command::new("launchctl")
        .arg("bootout")
        .arg(format!("gui/{uid}"))
        .arg(plist_path)
        .output();
    run_os_command(
        Command::new("launchctl")
            .arg("bootstrap")
            .arg(format!("gui/{uid}"))
            .arg(plist_path),
    )?;
    run_os_command(
        Command::new("launchctl")
            .arg("enable")
            .arg(format!("gui/{uid}/{}", plan.label)),
    )?;
    run_os_command(
        Command::new("launchctl")
            .arg("kickstart")
            .arg("-k")
            .arg(format!("gui/{uid}/{}", plan.label)),
    )?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn install_background_plan_for_os(
    _plan: &BackgroundSchedulerPlan,
) -> std::result::Result<(), CliError> {
    run_os_command(Command::new("systemctl").arg("--user").arg("daemon-reload"))?;
    run_os_command(
        Command::new("systemctl")
            .arg("--user")
            .arg("enable")
            .arg("--now")
            .arg(format!("{}.timer", _plan.label)),
    )?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn install_background_plan_for_os(
    plan: &BackgroundSchedulerPlan,
) -> std::result::Result<(), CliError> {
    if let Some(command) = plan.install_commands.first() {
        run_command_vec(command)?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn install_background_plan_for_os(
    _plan: &BackgroundSchedulerPlan,
) -> std::result::Result<(), CliError> {
    Err(state_error(anyhow!(
        "background scheduler install is not supported on this OS"
    )))
}

#[cfg(target_os = "macos")]
fn uninstall_background_plan_for_os(
    plan: &BackgroundSchedulerPlan,
) -> std::result::Result<(), CliError> {
    let Some((_, plist_path, _)) = plan.files.first() else {
        return Err(state_error(anyhow!("missing LaunchAgent plist path")));
    };
    let uid = current_uid_string().ok_or_else(|| state_error(anyhow!("could not resolve uid")))?;
    let _ = Command::new("launchctl")
        .arg("bootout")
        .arg(format!("gui/{uid}"))
        .arg(plist_path)
        .output();
    if plist_path.exists() {
        fs::remove_file(plist_path).map_err(internal_error)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn uninstall_background_plan_for_os(
    plan: &BackgroundSchedulerPlan,
) -> std::result::Result<(), CliError> {
    let _ = Command::new("systemctl")
        .arg("--user")
        .arg("disable")
        .arg("--now")
        .arg(format!("{}.timer", plan.label))
        .output();
    for (_, path, _) in &plan.files {
        if path.exists() {
            fs::remove_file(path).map_err(internal_error)?;
        }
    }
    let _ = Command::new("systemctl")
        .arg("--user")
        .arg("daemon-reload")
        .output();
    Ok(())
}

#[cfg(target_os = "windows")]
fn uninstall_background_plan_for_os(
    plan: &BackgroundSchedulerPlan,
) -> std::result::Result<(), CliError> {
    if let Some(command) = plan.uninstall_commands.first() {
        let _ = run_command_vec(command);
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn uninstall_background_plan_for_os(
    _plan: &BackgroundSchedulerPlan,
) -> std::result::Result<(), CliError> {
    Err(state_error(anyhow!(
        "background scheduler install is not supported on this OS"
    )))
}

fn background_status(plan: &BackgroundSchedulerPlan) -> Value {
    #[cfg(target_os = "macos")]
    {
        if let Some(command) = plan.status_commands.first() {
            return command_status_json(command);
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(command) = plan.status_commands.first() {
            return command_status_json(command);
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(command) = plan.status_commands.first() {
            return command_status_json(command);
        }
    }
    json!({
        "installed": false,
        "message": "status is not supported on this OS",
    })
}

fn command_status_json(command: &[String]) -> Value {
    match run_command_vec_output(command) {
        Ok((success, stdout, stderr)) => json!({
            "installed": success,
            "command": command_vec_display(command),
            "stdout": stdout,
            "stderr": stderr,
        }),
        Err(message) => json!({
            "installed": false,
            "command": command_vec_display(command),
            "message": message,
        }),
    }
}

fn resolve_background_fleet_controller(
    cli: &Cli,
    controller: Option<&Path>,
) -> std::result::Result<FleetControllerContext, CliError> {
    if let Some(controller) = controller {
        let project_root = controller.to_path_buf();
        let context = load_project_context(&project_root, None, cli.profile.as_deref(), None)
            .map_err(state_error)?;
        return Ok(FleetControllerContext {
            id: None,
            project_root,
            context,
            source: FleetControllerSource::Environment,
        });
    }
    resolve_fleet_controller(cli)
}

fn default_background_log_dir() -> PathBuf {
    if let Some(state_home) = std::env::var_os("XDG_STATE_HOME") {
        if !state_home.is_empty() {
            return PathBuf::from(state_home).join("metactl/background");
        }
    }
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/state/metactl/background")
}

#[cfg(target_os = "linux")]
fn user_config_dir() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg));
        }
    }
    Some(home_dir()?.join(".config"))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
}

fn short_path_hash(path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    hex::encode(digest)[..12].to_string()
}

fn command_display(program: &Path, args: &[String]) -> String {
    let mut parts = vec![shell_quote(&program.to_string_lossy())];
    parts.extend(args.iter().map(|arg| shell_quote(arg)));
    parts.join(" ")
}

fn command_vec_display(command: &[String]) -> String {
    command
        .iter()
        .map(|part| shell_quote(part))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '='))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[cfg(target_os = "macos")]
fn macos_launch_agent_plist(
    label: &str,
    executable: &Path,
    args: &[String],
    interval_minutes: u32,
    stdout_path: &Path,
    stderr_path: &Path,
) -> String {
    let mut program_args = vec![format!(
        "    <string>{}</string>",
        xml_escape(&executable.to_string_lossy())
    )];
    program_args.extend(
        args.iter()
            .map(|arg| format!("    <string>{}</string>", xml_escape(arg))),
    );
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
{program_args}
  </array>
  <key>StartInterval</key>
  <integer>{interval_seconds}</integer>
  <key>RunAtLoad</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{stdout_path}</string>
  <key>StandardErrorPath</key>
  <string>{stderr_path}</string>
</dict>
</plist>
"#,
        label = xml_escape(label),
        program_args = program_args.join("\n"),
        interval_seconds = interval_minutes * 60,
        stdout_path = xml_escape(&stdout_path.to_string_lossy()),
        stderr_path = xml_escape(&stderr_path.to_string_lossy()),
    )
}

#[cfg(target_os = "linux")]
fn linux_systemd_service(label: &str, run_command: &str, log_dir: &Path) -> String {
    format!(
        "[Unit]\nDescription=metactl report-only surface refresh ({label})\n\n[Service]\nType=oneshot\nExecStart={run_command}\nStandardOutput=append:{}/{}.out.log\nStandardError=append:{}/{}.err.log\n",
        log_dir.display(),
        label,
        log_dir.display(),
        label
    )
}

#[cfg(target_os = "linux")]
fn linux_systemd_timer(label: &str, interval_minutes: u32) -> String {
    format!(
        "[Unit]\nDescription=metactl report-only surface refresh timer ({label})\n\n[Timer]\nOnBootSec=1min\nOnUnitActiveSec={}min\nUnit={label}.service\n\n[Install]\nWantedBy=timers.target\n",
        interval_minutes
    )
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(target_os = "macos")]
fn current_uid_string() -> Option<String> {
    let output = Command::new("id").arg("-u").output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run_os_command(command: &mut Command) -> std::result::Result<(), CliError> {
    let output = command.output().map_err(internal_error)?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(state_error(anyhow!(
            "scheduler command failed: stdout={stdout} stderr={stderr}"
        )))
    }
}

#[cfg(target_os = "windows")]
fn run_command_vec(command: &[String]) -> std::result::Result<(), CliError> {
    let (success, stdout, stderr) =
        run_command_vec_output(command).map_err(|err| state_error(anyhow!(err)))?;
    if success {
        Ok(())
    } else {
        Err(state_error(anyhow!(
            "scheduler command failed: stdout={stdout} stderr={stderr}"
        )))
    }
}

fn run_command_vec_output(
    command: &[String],
) -> std::result::Result<(bool, String, String), String> {
    let Some((program, args)) = command.split_first() else {
        return Err("empty command".to_string());
    };
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|err| err.to_string())?;
    Ok((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
        String::from_utf8_lossy(&output.stderr).trim().to_string(),
    ))
}

fn cmd_surface_pin(
    cli: &Cli,
    args: &SurfacePinArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let action = if args.command {
        SurfaceOverrideAction::PinCommand
    } else {
        SurfaceOverrideAction::PinHot
    };
    let overrides = surface_usage::set_surface_override(&project_root, &args.pack_id, action)
        .map_err(state_error)?;
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Surface override pinned for {}.\nNext: metactl surface report",
                args.pack_id
            ),
        ),
        json: success_json(
            "surface",
            Some(&project_root),
            json!({
                "action": "pin",
                "pack_id": args.pack_id,
                "overrides": overrides,
                "next_steps": ["metactl surface report"],
            }),
        ),
    })
}

fn cmd_surface_block(
    cli: &Cli,
    args: &SurfacePackArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let overrides = surface_usage::set_surface_override(
        &project_root,
        &args.pack_id,
        SurfaceOverrideAction::Block,
    )
    .map_err(state_error)?;
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!(
                "Surface override blocked for {}.\nNext: metactl surface report",
                args.pack_id
            ),
        ),
        json: success_json(
            "surface",
            Some(&project_root),
            json!({
                "action": "block",
                "pack_id": args.pack_id,
                "overrides": overrides,
                "next_steps": ["metactl surface report"],
            }),
        ),
    })
}

fn cmd_surface_reset(
    cli: &Cli,
    args: &SurfaceResetArgs,
) -> std::result::Result<CommandOutput, CliError> {
    if args.pack_id.is_none() && !args.all {
        return Err(CliError::new(
            EXIT_STATE,
            "surface reset requires a pack id or --all.",
        ));
    }
    let project_root = project_root(cli).map_err(internal_error)?;
    let overrides = surface_usage::reset_surface_override(&project_root, args.pack_id.as_deref())
        .map_err(state_error)?;
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            "Surface override reset.\nNext: metactl surface report".to_string(),
        ),
        json: success_json(
            "surface",
            Some(&project_root),
            json!({
                "action": "reset",
                "pack_id": args.pack_id,
                "all": args.all,
                "overrides": overrides,
                "next_steps": ["metactl surface report"],
            }),
        ),
    })
}

fn cmd_explain(cli: &Cli, args: &ExplainArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_required_context(cli, &project_root)?;
    if args.staged {
        let targets = select_locked_targets(&context.lock, args.target.clone())?;
        let mut items = Vec::new();
        for target in targets {
            let manifest = load_compile_manifest(&project_root.join(&target.compile_manifest_path))
                .map_err(state_error)?;
            items.push(json!({
                "target": manifest.target.id,
                "generated_outputs": manifest.generated_outputs.len(),
                "surface_selection_mode": manifest.surface_selection_mode.as_ref().map(surface_selection_mode_label),
                "degradations": manifest.degradations,
                "paths": manifest.generated_outputs.iter().map(|item| item.path.clone()).collect::<Vec<_>>(),
                "outputs": cli.verbose.then_some(manifest.generated_outputs),
            }));
        }
        return Ok(CommandOutput {
            human: format!("Staged targets:\n{}", lines_from_json_items(&items)),
            json: success_json(
                "explain",
                Some(&project_root),
                json!({
                    "mode": "staged",
                    "targets": items,
                }),
            ),
        });
    }

    if !context.has_corpus() {
        return Ok(no_corpus_output("explain", &project_root));
    }

    let overrides = ConfigOverrides {
        role: args.role.clone(),
        policy: args.policy.clone(),
        targets: args.target.clone().into_iter().collect(),
    };
    let config = context.effective_config(&overrides).map_err(state_error)?;
    let default_surface_mode = config
        .defaults
        .as_ref()
        .and_then(|defaults| defaults.surface_selection_mode.clone());
    let targets = context.selected_targets(&overrides).map_err(state_error)?;
    let explain_target = targets
        .first()
        .cloned()
        .ok_or_else(|| state_error(anyhow!("target selection produced no target")))?;
    let kernel = kernel_from_context(&context).map_err(internal_error)?;
    let resolve_graph = kernel
        .resolve(ResolveParams {
            config,
            overlay: context.overlay.clone(),
            available_targets: targets,
            provenance: None,
        })
        .map_err(state_error)?;
    let explain = kernel
        .explain(ExplainParams { resolve_graph })
        .map_err(state_error)?;
    let selected_surface_mode = args
        .surface_mode
        .map(Into::into)
        .or(default_surface_mode)
        .unwrap_or_else(|| target_surface_selection_mode(&explain_target));
    let derived_surface_details = context
        .registry
        .as_ref()
        .map(|registry| {
            registry.surface_summaries_for_target(
                &explain.resolve_graph.activated_pack_refs,
                &explain_target,
                selected_surface_mode.clone(),
            )
        })
        .transpose()
        .map_err(state_error)?;
    let pack_lifecycle = context.registry.as_ref().map(|registry| {
        explain
            .resolve_graph
            .activated_pack_refs
            .iter()
            .filter_map(|pack_ref| {
                registry.pack_by_id(&pack_ref.id).and_then(|pack| {
                    pack.manifest
                        .lifecycle
                        .map(|lifecycle| (pack_ref.id.clone(), lifecycle))
                })
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    });
    let pack_sources = pack_source_contexts(
        &project_root,
        &context.config_file,
        &explain
            .resolve_graph
            .activated_pack_refs
            .iter()
            .map(|pack_ref| pack_ref.id.as_str())
            .collect::<Vec<_>>(),
    )?;
    let target_projection = target_projection_json(
        &explain_target,
        derived_surface_details.as_deref(),
        selected_surface_mode,
    );
    let surface_usage_summary = surface_usage::surface_report_summary_json(&project_root);
    let surface_details = derived_surface_details;
    Ok(explain_output(
        &project_root,
        args.query.as_deref(),
        &explain,
        &target_projection,
        &surface_usage_summary,
        surface_details.as_deref(),
        pack_lifecycle.as_ref(),
        &pack_sources,
    ))
}

fn cmd_sync(cli: &Cli, args: &SyncArgs) -> std::result::Result<CommandOutput, CliError> {
    if args.preview && args.apply {
        return Err(CliError::new(
            EXIT_STATE,
            "sync accepts --preview or --apply, not both.",
        ));
    }
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_required_context(cli, &project_root)?;
    let source_audit_findings = source_audit_findings(&project_root)?;
    if !source_audit_findings.is_empty() {
        return Err(CliError {
            code: EXIT_VALIDATION,
            message: "Sync refused because private source state may be tracked or exposed."
                .to_string(),
            details: source_audit_detail_lines(&source_audit_findings),
            json: json!({
                "ok": false,
                "command": "sync",
                "api_version": API_VERSION,
                "project_root": project_root.to_string_lossy(),
                "source_audit": {
                    "status": "fail",
                    "findings": source_audit_findings,
                },
            }),
        });
    }
    let source_state = private_source_readiness(&project_root, &context.config_file, true)?;
    let source_state_label = source_state["state"].as_str().unwrap_or("unknown");
    let has_unlocked = source_state["unlocked"]
        .as_array()
        .map(|items| !items.is_empty())
        .unwrap_or(false);
    if source_state_label == "private_source_stale"
        || (args.require_private_sources
            && (source_state_label == "private_source_missing" || has_unlocked))
    {
        return Err(source_preflight_error(
            &project_root,
            source_state,
            args.require_private_sources,
        ));
    }
    let sync_targets = split_comma_args(&args.target);
    let compile_out = cmd_compile(
        cli,
        &CompileArgs {
            target: sync_targets,
            all: args.all,
            role: args.role.clone(),
            policy: args.policy.clone(),
            update_lock: true,
            apply: false,
            apply_mode: None,
            surface_mode: args.surface_mode,
        },
    )?;

    let effective_adopt = if args.preview {
        Some(SyncAdoptArg::Preview)
    } else {
        args.adopt
    };
    let apply_args = match effective_adopt {
        Some(SyncAdoptArg::Preview) => ApplyArgs {
            target: None,
            mode: None,
            preview: true,
        },
        Some(SyncAdoptArg::Patch) => ApplyArgs {
            target: None,
            mode: Some(ApplyModeArg::Patch),
            preview: false,
        },
        Some(SyncAdoptArg::Takeover) => ApplyArgs {
            target: None,
            mode: Some(ApplyModeArg::Takeover),
            preview: false,
        },
        None => ApplyArgs {
            target: None,
            mode: None,
            preview: false,
        },
    };

    let apply_out = match cmd_apply(cli, &apply_args) {
        Ok(output) => output,
        Err(mut err) => {
            if err.code == EXIT_CONFLICT && args.adopt.is_none() {
                let next_steps = vec![
                    "metactl sync --adopt preview",
                    "metactl sync --adopt patch",
                    "metactl sync --adopt takeover",
                ];
                let playbook = brownfield_adoption_hint();
                err.details.extend(
                    next_steps
                        .iter()
                        .map(|step| format!("Next: {step}"))
                        .collect::<Vec<_>>(),
                );
                err.details.push("".to_string());
                err.details.push(playbook.clone());
                if let Some(obj) = err.json.as_object_mut() {
                    obj.insert("next_steps".to_string(), json!(next_steps));
                    // Strip ANSI codes from playbook for machine-readable JSON output
                    obj.insert("playbook".to_string(), json!(strip_ansi_codes(&playbook)));
                }
            }
            return Err(err);
        }
    };

    let validate_out = if apply_args.preview {
        None
    } else {
        Some(cmd_validate(
            cli,
            &ValidateCmdArgs {
                target: None,
                strict: false,
            },
        )?)
    };
    let context = load_required_context(cli, &project_root)?;
    let profile = profile_status_json(&context);
    let shared_surface_rules = shared_surface_rules(
        context.registry.as_ref(),
        &context
            .lock
            .targets
            .iter()
            .map(|target| target.target.id.clone())
            .collect::<Vec<_>>(),
    );
    let readiness = target_readiness_json(&project_root, &context.lock, &apply_out.json)?;

    let mut lines = vec!["Sync complete.".to_string()];
    for target_json in &readiness {
        let target_id = target_json["target"].as_str().unwrap_or("unknown");
        let status = target_json["status"].as_str().unwrap_or("unknown");
        let runtime_paths = target_json["runtime_paths"]
            .as_array()
            .map(|paths| paths.iter().filter_map(|p| p.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        let apply_mode = target_json["apply_mode"].as_str().unwrap_or("unknown");
        let surface_mode = target_json["surface_selection_mode"]
            .as_str()
            .unwrap_or("unknown");
        lines.push(format!(
            "  {} [{}] ({}, surface: {}, {} file{})",
            target_id,
            status,
            apply_mode,
            surface_mode,
            runtime_paths.len(),
            if runtime_paths.len() == 1 { "" } else { "s" }
        ));
        for path in &runtime_paths {
            lines.push(format!("    {}", path));
        }
        let degradations = target_json["degradations"].as_array();
        if let Some(degradations) = degradations {
            for d in degradations {
                if let Some(msg) = d.as_str() {
                    lines.push(format!("    (degraded: {})", msg));
                }
            }
        }
    }
    lines.push(format!("  Profile: {}", profile_status_message(&profile)));
    if !shared_surface_rules.is_empty() {
        lines.push("  Shared surfaces:".to_string());
        for rule in &shared_surface_rules {
            lines.push(format!("    {}", rule.human_line()));
        }
    }
    if apply_args.preview {
        lines.push("Preview only; runtime files were not changed.".to_string());
    }

    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "sync",
            Some(&project_root),
            json!({
                "compile": compile_out.json,
                "apply": apply_out.json,
                "validate": validate_out.map(|output| output.json),
                "profile": profile,
                "shared_surface_rules": shared_surface_rules.iter().map(SharedSurfaceRule::to_json).collect::<Vec<_>>(),
                "targets": readiness,
                "preview": apply_args.preview,
            }),
        ),
    })
}

fn pack_source_contexts(
    project_root: &Path,
    config: &ProjectConfigFile,
    active_pack_ids: &[&str],
) -> std::result::Result<Value, CliError> {
    let mut pack_to_source = BTreeMap::new();
    for configured in &config.packs {
        if let Some((source_id, pack_id)) = configured.split_once('/') {
            pack_to_source.insert(pack_id.to_string(), source_id.to_string());
        }
    }
    let readiness = private_source_readiness(project_root, config, false)?;
    let mut freshness_by_id = BTreeMap::new();
    if let Some(items) = readiness["freshness"].as_array() {
        for item in items {
            if let Some(id) = item["id"].as_str() {
                freshness_by_id.insert(id.to_string(), item.clone());
            }
        }
    }
    let mut out = Map::new();
    for pack_id in active_pack_ids {
        let source_id = pack_to_source
            .get(*pack_id)
            .cloned()
            .or_else(|| infer_pack_source_id(project_root, config, pack_id));
        let Some(source_id) = source_id else {
            continue;
        };
        let Some(source) = config.sources.iter().find(|source| source.id == source_id) else {
            continue;
        };
        let redacted = source.visibility == SourceVisibility::Private
            && source.lock_publicity == SourceLockPublicity::Private;
        let mut item = Map::new();
        item.insert("id".to_string(), json!(source.id));
        item.insert(
            "type".to_string(),
            json!(source_type_label(&source.source_type)),
        );
        item.insert(
            "visibility".to_string(),
            json!(source_visibility_label(&source.visibility)),
        );
        item.insert(
            "lock_publicity".to_string(),
            json!(source_lock_publicity_label(&source.lock_publicity)),
        );
        item.insert("redacted".to_string(), json!(redacted));
        if let Some(freshness) = freshness_by_id.get(&source.id) {
            item.insert("freshness".to_string(), freshness.clone());
        }
        if !redacted {
            if let Some(path) = source.path.as_ref() {
                item.insert("path".to_string(), json!(path));
            }
            if let Some(url) = source.url.as_ref() {
                item.insert("url".to_string(), json!(url));
            }
            if let Some(ref_) = source.ref_.as_ref() {
                item.insert("ref".to_string(), json!(ref_));
            }
        }
        out.insert((*pack_id).to_string(), Value::Object(item));
    }
    Ok(Value::Object(out))
}

fn infer_pack_source_id(
    project_root: &Path,
    config: &ProjectConfigFile,
    pack_id: &str,
) -> Option<String> {
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
        let Some(root) = root else {
            continue;
        };
        let root = if root.is_absolute() {
            root
        } else {
            project_root.join(root)
        };
        if root.join("packs").join(format!("{pack_id}.json")).exists() {
            return Some(source.id.clone());
        }
    }
    None
}

fn cmd_compile(cli: &Cli, args: &CompileArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_required_context(cli, &project_root)?;
    if !context.has_corpus() {
        return Err(state_error(anyhow!(
            "no starter library was discovered; compile cannot continue"
        )));
    }
    if !args.update_lock && lock_is_stale_checked(&context)? {
        return Err(stale_lock_error());
    }

    // --all is an explicit alias for omitting --target; both default to all configured targets
    let mut target_overrides = if args.all || args.target.is_empty() {
        context.config_file.targets.clone()
    } else {
        split_comma_args(&args.target)
    };

    // Resolve target aliases (claude -> claude-code, codex -> codex-cli, gemini -> gemini-cli)
    target_overrides = target_overrides
        .into_iter()
        .map(|id| {
            let (canonical, was_alias) = resolve_target_alias(&id);
            if was_alias && !cli.quiet {
                eprintln!("note: resolved target alias '{}' to '{}'", id, canonical);
            }
            canonical
        })
        .collect();

    if !args.all && !args.target.is_empty() && registry_has_targets(context.registry.as_ref()) {
        for target_id in &target_overrides {
            let available = get_available_target_ids(context.registry.as_ref());
            if !available.iter().any(|candidate| candidate == target_id) {
                return Err(format_target_not_found_error(
                    target_id,
                    context.registry.as_ref(),
                ));
            }
        }
    }

    let preflight_overrides = ConfigOverrides {
        role: args.role.clone(),
        policy: args.policy.clone(),
        targets: target_overrides.clone(),
    };
    let discoverability = discoverability_report(&context, &preflight_overrides);
    if discoverability.is_blocked() {
        return Err(discoverability_error(&discoverability));
    }

    let shared_surface_rules = shared_surface_rules(context.registry.as_ref(), &target_overrides);

    let kernel = kernel_from_context(&context).map_err(internal_error)?;
    let mut compiled_targets = Vec::new();
    let mut lock = context.lock.clone();
    lock.targets.clear();
    lock.config_digest = Some(current_config_digest(&context).map_err(internal_error)?);
    lock.overlay_path = context
        .overlay_path
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());
    lock.overlay_digest = current_overlay_digest(&context).map_err(internal_error)?;
    lock.profile_name = context
        .active_profile
        .as_ref()
        .map(|profile| profile.name.clone());
    lock.profile_path = context
        .active_profile
        .as_ref()
        .map(|profile| profile.path.to_string_lossy().to_string());
    lock.profile_digest = context
        .active_profile
        .as_ref()
        .and_then(|profile| profile.digest.clone());
    lock.local_config_digest = current_local_config_digest(&context).map_err(internal_error)?;
    lock.updated_at = Some(now_string());

    for target_id in target_overrides {
        let overrides = ConfigOverrides {
            role: args.role.clone(),
            policy: args.policy.clone(),
            targets: vec![target_id.clone()],
        };
        let config = match context.effective_config(&overrides) {
            Ok(config) => config,
            Err(err)
                if err
                    .to_string()
                    .contains("was not discovered in starter libraries") =>
            {
                return Err(format_target_not_found_error(
                    &target_id,
                    context.registry.as_ref(),
                ));
            }
            Err(err) => return Err(state_error(err)),
        };
        let surface_selection_mode = args.surface_mode.map(Into::into).or_else(|| {
            config
                .defaults
                .as_ref()
                .and_then(|defaults| defaults.surface_selection_mode.clone())
        });
        let target = match context.selected_targets(&overrides) {
            Ok(targets) => targets
                .into_iter()
                .next()
                .ok_or_else(|| state_error(anyhow!("target selection produced no target")))?,
            Err(_) => {
                return Err(format_target_not_found_error(
                    &target_id,
                    context.registry.as_ref(),
                ));
            }
        };
        let resolve_graph = kernel
            .resolve(ResolveParams {
                config,
                overlay: context.overlay.clone(),
                available_targets: vec![target.clone()],
                provenance: None,
            })
            .map_err(state_error)?;
        let preferred_apply_mode = preferred_apply_mode_for_target(&target, None);
        let mut compile = kernel
            .compile(CompileParams {
                resolve_graph,
                target_capability: target.clone(),
                apply_mode: preferred_apply_mode.clone(),
                surface_selection_mode,
                emit_policy_report: true,
                project_root: Some(project_root.to_string_lossy().to_string()),
            })
            .map_err(state_error)?;
        let manifest_path = compile_manifest_path(&project_root, &target.target_ref());
        apply_shared_surface_rules_to_manifest(
            &project_root,
            &target.target_id,
            &mut compile.compile_manifest,
            &shared_surface_rules,
        )
        .map_err(internal_error)?;
        write_compile_manifest_json(&manifest_path, &compile.compile_manifest)
            .map_err(internal_error)?;
        let policy_path = policy_report_path(&project_root, &target.target_ref());
        if let Some(report) = compile.policy_enforcement_report.as_ref() {
            write_policy_report(&policy_path, report).map_err(internal_error)?;
        }

        lock.targets.push(LockedTarget {
            target: target.target_ref(),
            compile_manifest_path: relative_to_project(&project_root, &manifest_path),
            compile_manifest_digest: digest_path(&manifest_path).map_err(internal_error)?,
            policy_report_path: compile
                .policy_enforcement_report
                .as_ref()
                .map(|_| relative_to_project(&project_root, &policy_path)),
            policy_report_digest: compile
                .policy_enforcement_report
                .as_ref()
                .map(|_| digest_path(&policy_path))
                .transpose()
                .map_err(internal_error)?,
            preferred_apply_mode: preferred_apply_mode.clone(),
            compiled_at: now_string(),
        });

        compiled_targets.push(json!({
            "target": target.target_id,
            "generated_outputs": compile.compile_manifest.generated_outputs.iter().map(|item| item.path.clone()).collect::<Vec<_>>(),
            "degradations": compile.compile_manifest.degradations,
            "apply_modes_supported": compile.compile_manifest.apply_modes_supported,
            "surface_selection_mode": compile.compile_manifest.surface_selection_mode.as_ref().map(surface_selection_mode_label),
        }));
    }

    write_lock(&context.lock_path, &lock).map_err(internal_error)?;

    let compile_human = {
        let mut human_lines = vec!["Compiled:".to_string()];
        for ct in &compiled_targets {
            let target_id = ct["target"].as_str().unwrap_or("unknown");
            let outputs = ct["generated_outputs"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0);
            let degradations = ct["degradations"].as_array().map(|a| a.len()).unwrap_or(0);
            let surface_mode = ct["surface_selection_mode"]
                .as_str()
                .map(|mode| format!(", surface: {mode}"))
                .unwrap_or_default();
            let note = if degradations > 0 {
                format!(
                    " ({} degradation{})",
                    degradations,
                    if degradations == 1 { "" } else { "s" }
                )
            } else {
                String::new()
            };
            human_lines.push(format!(
                "  {} ({} output{}{}{})",
                target_id,
                outputs,
                if outputs == 1 { "" } else { "s" },
                surface_mode,
                note
            ));
        }
        if !shared_surface_rules.is_empty() {
            human_lines.push("Shared surfaces:".to_string());
            for rule in &shared_surface_rules {
                human_lines.push(format!("  {}", rule.human_line()));
            }
        }
        human_lines.push("Next: metactl apply".to_string());
        project_human_output(&project_root, human_lines.join("\n"))
    };
    let compile_out = CommandOutput {
        human: compile_human,
        json: success_json(
            "compile",
            Some(&project_root),
            json!({
                "targets": compiled_targets,
                "shared_surface_rules": shared_surface_rules.iter().map(SharedSurfaceRule::to_json).collect::<Vec<_>>(),
                "lock_path": context.lock_path,
            }),
        ),
    };

    if !args.apply {
        return Ok(compile_out);
    }

    let apply_out = cmd_apply(
        cli,
        &ApplyArgs {
            target: None,
            mode: args.apply_mode,
            preview: false,
        },
    )?;
    let mut merged_json = compile_out.json;
    if let Some(obj) = merged_json.as_object_mut() {
        obj.insert("apply".to_string(), apply_out.json);
    }
    Ok(CommandOutput {
        human: format!("{}\n{}", compile_out.human, apply_out.human),
        json: merged_json,
    })
}

fn cmd_apply(cli: &Cli, args: &ApplyArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_required_context(cli, &project_root)?;
    if lock_is_stale_checked(&context)? {
        return Err(stale_lock_error());
    }
    let kernel = kernel_from_context(&context).map_err(internal_error)?;
    let targets = select_locked_targets(&context.lock, args.target.clone())?;
    let mut outputs = Vec::new();
    let mut notes = Vec::new();
    for target in targets {
        let manifest_path = project_root.join(&target.compile_manifest_path);
        let manifest = load_compile_manifest(&manifest_path).map_err(state_error)?;
        let target_capability = context
            .registry
            .as_ref()
            .and_then(|registry| registry.target_by_id(&target.target.id));
        let (apply_mode, note) = normalize_apply_mode(
            args.mode.map(ApplyMode::from),
            &target.preferred_apply_mode,
            &manifest,
            target_capability.as_ref(),
        )?;
        if let Some(note) = note {
            notes.push(note);
        }
        if args.preview {
            outputs.push(json!({
                "target": target.target.id,
                "apply_mode": apply_mode,
                "preview": preview_manifest(&project_root, &manifest),
            }));
            continue;
        }
        let report = kernel
            .apply_compiled_outputs(&project_root, &manifest, &apply_mode)
            .map_err(state_error)?;
        if !report.conflicts.is_empty() {
            return Err(conflict_error(&report));
        }
        append_history_entry(
            &project_root,
            &HistoryEntry {
                action: "apply".to_string(),
                target: report.target.id.clone(),
                status: "ok".to_string(),
                timestamp: now_string(),
                paths: report.applied_paths.clone(),
                notes: notes.clone(),
            },
        )
        .map_err(internal_error)?;
        outputs.push(json!({
            "target": report.target.id,
            "apply_mode": apply_mode,
            "applied_paths": report.applied_paths,
            "state_path": report.state_path,
        }));
    }
    update_managed_files_index(&project_root).map_err(internal_error)?;
    let human = {
        let label = if args.preview {
            "Apply preview:"
        } else {
            "Applied:"
        };
        let mut human_lines = vec![label.to_string()];
        for output in &outputs {
            let target_id = output["target"].as_str().unwrap_or("unknown");
            let apply_mode = output["apply_mode"].as_str().unwrap_or("unknown");
            let paths = if args.preview {
                output["preview"]
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|p| p["destination_path"].as_str())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            } else {
                output["applied_paths"]
                    .as_array()
                    .map(|items| items.iter().filter_map(|p| p.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default()
            };
            human_lines.push(format!(
                "  {} ({}, {} file{})",
                target_id,
                apply_mode,
                paths.len(),
                if paths.len() == 1 { "" } else { "s" }
            ));
            for path in &paths {
                human_lines.push(format!("    {}", path));
            }
        }
        project_human_output(&project_root, human_lines.join("\n"))
    };
    Ok(CommandOutput {
        human,
        json: success_json(
            "apply",
            Some(&project_root),
            json!({
                "preview": args.preview,
                "targets": outputs,
                "notes": notes,
            }),
        ),
    })
}

fn cmd_revert(cli: &Cli, args: &RevertArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_required_context(cli, &project_root)?;
    let kernel = kernel_from_context(&context).map_err(internal_error)?;
    let targets = if args.all {
        context.lock.targets.clone()
    } else {
        select_locked_targets(&context.lock, args.target.clone())?
    };
    if targets.is_empty() {
        return Err(state_error(anyhow!(
            "no compiled target is available to revert"
        )));
    }
    let mut outputs = Vec::new();
    let mut lock = context.lock.clone();
    for target in &targets {
        let report = kernel
            .revert_target(&project_root, &target.target)
            .map_err(state_error)?;
        if !report.conflicts.is_empty() {
            return Err(CliError::new(
                EXIT_CONFLICT,
                format!("Revert refused for target {}.", report.target.id),
            )
            .with_details(
                report
                    .conflicts
                    .iter()
                    .map(|item| format!("{}: {}", item.destination_path, item.detail))
                    .collect(),
            ));
        }
        append_history_entry(
            &project_root,
            &HistoryEntry {
                action: "revert".to_string(),
                target: report.target.id.clone(),
                status: "ok".to_string(),
                timestamp: now_string(),
                paths: report.reverted_paths.clone(),
                notes: Vec::new(),
            },
        )
        .map_err(internal_error)?;
        outputs.push(json!({
            "target": report.target.id,
            "reverted_paths": report.reverted_paths,
            "state_path": report.state_path,
        }));
    }
    let reverted_ids = targets
        .iter()
        .map(|item| item.target.id.clone())
        .collect::<BTreeSet<_>>();
    lock.targets
        .retain(|item| !reverted_ids.contains(&item.target.id));
    lock.updated_at = Some(now_string());
    write_lock(&context.lock_path, &lock).map_err(internal_error)?;
    update_managed_files_index(&project_root).map_err(internal_error)?;
    let mut human_lines = vec!["Reverted:".to_string()];
    for output in &outputs {
        let target_id = output["target"].as_str().unwrap_or("unknown");
        let reverted_paths = output["reverted_paths"]
            .as_array()
            .map(|paths| paths.iter().filter_map(|p| p.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        human_lines.push(format!(
            "  {} ({} file{} removed)",
            target_id,
            reverted_paths.len(),
            if reverted_paths.len() == 1 { "" } else { "s" }
        ));
        for path in &reverted_paths {
            human_lines.push(format!("    {}", path));
        }
    }
    Ok(CommandOutput {
        human: project_human_output(&project_root, human_lines.join("\n")),
        json: success_json(
            "revert",
            Some(&project_root),
            json!({
                "targets": outputs,
            }),
        ),
    })
}

fn cmd_validate(cli: &Cli, args: &ValidateCmdArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_required_context(cli, &project_root)?;
    let kernel = kernel_from_context(&context).map_err(internal_error)?;
    let source_audit_findings = source_audit_findings(&project_root)?;
    if !source_audit_findings.is_empty() {
        let details = source_audit_detail_lines(&source_audit_findings);
        return Err(CliError {
            code: EXIT_VALIDATION,
            message: "Validation failed because private source state may be tracked or exposed."
                .to_string(),
            details,
            json: json!({
                "ok": false,
                "command": "validate",
                "api_version": API_VERSION,
                "project_root": project_root.to_string_lossy(),
                "source_audit": {
                    "status": "fail",
                    "findings": source_audit_findings,
                },
            }),
        });
    }
    let freshness_findings = freshness_findings_json(context.registry.as_ref());
    let freshness_failed = args.strict
        && freshness_findings
            .iter()
            .any(|item| item["status"] == "fail");
    if freshness_failed {
        return Err(CliError {
            code: EXIT_VALIDATION,
            message: "Validation failed because freshness policy marked one or more knowledge sources expired.".to_string(),
            details: freshness_findings.iter().map(|item| item.to_string()).collect(),
            json: json!({
                "ok": false,
                "command": "validate",
                "api_version": API_VERSION,
                "project_root": project_root.to_string_lossy(),
                "strict": args.strict,
                "freshness": freshness_findings,
            }),
        });
    }
    let targets = select_locked_targets(&context.lock, args.target.clone())?;
    if targets.is_empty() {
        return Err(state_error(anyhow!(
            "metactl.lock.json does not contain a compiled target to validate"
        )));
    }
    let mut reports = Vec::new();
    let mut overall_fail = lock_is_stale_checked(&context)?;
    for target in targets {
        let manifest = load_compile_manifest(&project_root.join(&target.compile_manifest_path))
            .map_err(state_error)?;
        let policy_report = target
            .policy_report_path
            .as_ref()
            .map(|path| load_policy_report(&project_root.join(path)))
            .transpose()
            .map_err(state_error)?;
        let report = kernel
            .validate(ValidateParams {
                subject_ref: target.target.clone(),
                resolve_graph: None,
                compile_manifest: Some(manifest),
                policy_enforcement_report: policy_report,
                project_root: Some(project_root.to_string_lossy().to_string()),
            })
            .map_err(state_error)?;
        if report.status == ValidationStatus::Fail {
            overall_fail = true;
        }
        reports.push(report_to_json(&report));
    }
    if overall_fail {
        return Err(CliError::new(
            EXIT_VALIDATION,
            "Validation failed for one or more targets.",
        )
        .with_details(reports.iter().map(|item| item.to_string()).collect()));
    }
    let mut validate_lines = vec!["Validation:".to_string()];
    for report in &reports {
        let target_id = report["subject_ref"]["id"].as_str().unwrap_or("unknown");
        let status = report["status"].as_str().unwrap_or("unknown");
        validate_lines.push(format!("  {} [{}]", target_id, status));
        if let Some(checks) = report["checks"].as_array() {
            for check in checks {
                let check_status = check["status"].as_str().unwrap_or("");
                let message = check["message"].as_str().unwrap_or("");
                if check_status != "pass" || cli.verbose {
                    validate_lines.push(format!("    {} {}", check_status, message));
                }
            }
        }
    }
    Ok(CommandOutput {
        human: project_human_output(&project_root, validate_lines.join("\n")),
        json: success_json(
            "validate",
            Some(&project_root),
            json!({
                "reports": reports,
                "strict": args.strict,
                "freshness": freshness_findings,
            }),
        ),
    })
}

fn freshness_findings_json(registry: Option<&LibraryRegistry>) -> Vec<Value> {
    let Some(registry) = registry else {
        return Vec::new();
    };
    registry
        .list_knowledge_sources()
        .into_iter()
        .filter_map(|source| {
            let expired = knowledge_source_is_expired(&source.freshness);
            let finding = freshness_expiry_finding(&source.freshness, expired)
                .or_else(|| freshness_lifecycle_finding(&source.freshness));
            let (status, code) = match finding {
                Some(finding) => finding,
                None => return None,
            };
            Some(json!({
                "kind": "knowledge_source",
                "id": source.id,
                "code": code,
                "status": status,
                "trust_tier": source.trust_tier,
                "freshness_policy": freshness_policy_label(&source.freshness.freshness_policy),
                "owner": source.freshness.owner,
                "last_verified": source.freshness.last_verified,
                "expires_at": source.freshness.expires_at,
                "expires_after_days": source.freshness.expires_after_days,
                "source_digests": source.freshness.source_digests,
                "review_status": review_status_label(&source.freshness.review_status),
                "supersedes": source.freshness.supersedes,
                "superseded_by": source.freshness.superseded_by,
            }))
        })
        .collect()
}

fn freshness_expiry_finding(
    freshness: &metactl::KnowledgeFreshness,
    expired: bool,
) -> Option<(&'static str, &'static str)> {
    if !expired {
        return None;
    }
    match &freshness.freshness_policy {
        metactl::KnowledgeFreshnessPolicy::Fail => Some(("fail", "METACTL_KS_EXPIRED_FAIL")),
        metactl::KnowledgeFreshnessPolicy::Warn => Some(("warn", "METACTL_KS_EXPIRED_WARN")),
        metactl::KnowledgeFreshnessPolicy::Ignore => Some(("ignored", "METACTL_KS_EXPIRED_IGNORE")),
    }
}

fn freshness_lifecycle_finding(
    freshness: &metactl::KnowledgeFreshness,
) -> Option<(&'static str, &'static str)> {
    if !freshness.superseded_by.is_empty() {
        return Some(("warn", "METACTL_KS_SUPERSEDED"));
    }
    match &freshness.review_status {
        metactl::KnowledgeReviewStatus::Stale => Some(("warn", "METACTL_KS_REVIEW_STALE")),
        metactl::KnowledgeReviewStatus::Superseded => Some(("warn", "METACTL_KS_SUPERSEDED")),
        metactl::KnowledgeReviewStatus::Retired => Some(("warn", "METACTL_KS_RETIRED")),
        metactl::KnowledgeReviewStatus::Draft | metactl::KnowledgeReviewStatus::Active => None,
    }
}

fn freshness_policy_label(policy: &metactl::KnowledgeFreshnessPolicy) -> &'static str {
    match policy {
        metactl::KnowledgeFreshnessPolicy::Ignore => "ignore",
        metactl::KnowledgeFreshnessPolicy::Warn => "warn",
        metactl::KnowledgeFreshnessPolicy::Fail => "fail",
    }
}

fn review_status_label(status: &metactl::KnowledgeReviewStatus) -> &'static str {
    match status {
        metactl::KnowledgeReviewStatus::Draft => "draft",
        metactl::KnowledgeReviewStatus::Active => "active",
        metactl::KnowledgeReviewStatus::Stale => "stale",
        metactl::KnowledgeReviewStatus::Superseded => "superseded",
        metactl::KnowledgeReviewStatus::Retired => "retired",
    }
}

fn knowledge_source_is_expired(freshness: &metactl::KnowledgeFreshness) -> bool {
    if let Some(expires_at) = freshness.expires_at.as_ref() {
        if let (Some(expiry), Some(today)) = (ymd_days(expires_at), current_utc_days()) {
            return expiry < today;
        }
    }
    if let Some(days) = freshness.expires_after_days {
        if let (Some(verified), Some(today)) =
            (ymd_days(&freshness.last_verified), current_utc_days())
        {
            return verified + (days as i64) < today;
        }
    }
    false
}

fn current_utc_days() -> Option<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| (duration.as_secs() / 86_400) as i64)
}

fn ymd_days(value: &str) -> Option<i64> {
    let date = value.get(0..10)?;
    let mut parts = date.split('-');
    let year = parts.next()?.parse::<i64>().ok()?;
    let month = parts.next()?.parse::<u32>().ok()?;
    let day = parts.next()?.parse::<u32>().ok()?;
    Some(days_from_civil(year, month, day))
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - (month <= 2) as i64;
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i64;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn cmd_doctor(cli: &Cli, args: &DoctorArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let config_path = project_config_path(&project_root, cli.config.as_deref());
    if !config_path.exists() {
        let next_commands = vec![
            "metactl setup --plan".to_string(),
            "metactl setup --target codex-cli --yes".to_string(),
        ];
        let checks = vec![json!({
            "id": "setup-posture",
            "status": "warn",
            "message": "No metactl.yaml found. Run guided setup before sync.",
            "next_commands": next_commands,
        })];
        return Ok(doctor_output(&project_root, checks));
    }
    let context = load_required_context(cli, &project_root)?;
    let mut checks = Vec::new();
    let discoverability = discoverability_report(&context, &ConfigOverrides::default());

    checks.push(json!({
        "id": "starter-library",
        "status": if context.registry.is_some() && context.has_corpus() { "pass" } else { "warn" },
        "message": if context.registry.is_some() && context.has_corpus() {
            "Starter library roots are available."
        } else if context.registry.is_some() {
            "Starter library roots exist but do not provide a usable corpus."
        } else {
            "No starter library was discovered. Search can only report a weak corpus."
        }
    }));

    checks.push(json!({
        "id": "corpus",
        "status": if context.has_corpus() { "pass" } else { "warn" },
        "message": if context.has_corpus() {
            "Starter library contains discoverable packs."
        } else {
            "Starter library is empty or unavailable."
        }
    }));

    checks.push(json!({
        "id": "role-discovery",
        "status": if discoverability.missing_role { "fail" } else { "pass" },
        "message": if discoverability.missing_role {
            format!(
                "Configured role {} was not discovered in the effective library roots.",
                discoverability.role_id
            )
        } else {
            format!("Configured role {} is discoverable.", discoverability.role_id)
        }
    }));

    checks.push(json!({
        "id": "policy-discovery",
        "status": if discoverability.missing_policy { "fail" } else { "pass" },
        "message": if discoverability.missing_policy {
            format!(
                "Configured policy {} was not discovered in the effective library roots.",
                discoverability.policy_id
            )
        } else {
            format!("Configured policy {} is discoverable.", discoverability.policy_id)
        }
    }));

    checks.push(json!({
        "id": "target-discovery",
        "status": if discoverability.missing_targets.is_empty() { "pass" } else { "fail" },
        "message": if discoverability.missing_targets.is_empty() {
            format!(
                "Configured target(s) {} are discoverable.",
                discoverability.target_ids.join(", ")
            )
        } else {
            format!(
                "Configured target(s) {} were not discovered in the effective library roots: {}. Suggested fix: add a library root that contains targets{} and run `metactl doctor` again.",
                discoverability.missing_targets.join(", "),
                discoverability.effective_library_roots.join(", "),
                discoverability
                    .profile_name
                    .as_ref()
                    .map(|name| format!(" to profile `{name}` or `metactl.yaml`"))
                    .unwrap_or_else(|| " to `metactl.yaml`".to_string())
            )
        }
    }));

    // Local config check
    let local_path = metactl::project::local_config_path(&project_root);
    let local_exists = local_path.exists();
    let local_valid = if local_exists {
        metactl::project::load_local_config(&project_root).is_ok()
    } else {
        true
    };
    checks.push(json!({
        "id": "local-config",
        "status": if local_exists && !local_valid { "fail" } else { "pass" },
        "message": if local_exists && local_valid {
            format!("Local config {} exists and is valid.", local_path.display())
        } else if local_exists {
            format!("Local config {} exists but failed to parse.", local_path.display())
        } else {
            "No metactl.local.yaml found (optional).".to_string()
        }
    }));

    // Provenance / input layer check
    let input_layers = build_input_layers(&context);
    checks.push(json!({
        "id": "input-provenance",
        "status": if input_layers.is_empty() { "warn" } else { "pass" },
        "message": if input_layers.is_empty() {
            "No active input layers detected."
        } else {
            "Input layers are available for provenance tracking."
        }
    }));

    // Local projection support per target (check via metadata)
    if let Some(registry) = context.registry.as_ref() {
        for target_id in &context.config_file.targets {
            if let Some(target) = registry.target_by_id(target_id) {
                let (support_label, status) = match target.local_projection.as_ref() {
                    Some(lp) => match lp.support {
                        metactl::LocalProjectionSupport::Exact => ("exact", "pass"),
                        metactl::LocalProjectionSupport::Degraded => {
                            ("degraded (no native local surface)", "warn")
                        }
                        metactl::LocalProjectionSupport::Unavailable => ("unavailable", "warn"),
                    },
                    None => ("not declared", "pass"),
                };
                checks.push(json!({
                    "id": format!("local-projection-{}", target_id),
                    "status": status,
                    "message": format!("Local projection support for {}: {}.", target_id, support_label),
                }));
            }
        }
    }

    let stale = lock_is_stale_checked(&context)?;
    checks.push(json!({
        "id": "lock",
        "status": if stale { "fail" } else { "pass" },
        "message": if stale {
            "metactl.lock.json does not match the current config or overlay. Re-run compile with --update-lock."
        } else {
            "Lock file matches the current config."
        }
    }));

    let profile_status = profile_status_json(&context);
    checks.push(json!({
        "id": "profile-binding",
        "status": match profile_status["status"].as_str().unwrap_or("none") {
            "none" | "synced" => "pass",
            "diverged" => "warn",
            "stale" => "fail",
            _ => "warn",
        },
        "message": profile_status_message(&profile_status),
    }));

    let source_audit_findings = source_audit_findings(&project_root)?;
    checks.push(json!({
        "id": "source-audit",
        "status": if source_audit_findings.is_empty() { "pass" } else { "fail" },
        "message": if source_audit_findings.is_empty() {
            "Private source cache and private source lock are not tracked or exposed."
        } else {
            "Private source cache or private source lock may be tracked or exposed."
        },
        "findings": source_audit_findings,
    }));

    let targets = select_locked_targets(&context.lock, args.target.clone()).unwrap_or_default();
    if targets.is_empty() {
        checks.push(json!({
            "id": "compiled-targets",
            "status": "warn",
            "message": "No compiled target is recorded in metactl.lock.json."
        }));
    }

    for target in targets {
        let manifest_path = project_root.join(&target.compile_manifest_path);
        checks.push(json!({
            "id": format!("manifest-{}", target.target.id),
            "status": if manifest_path.exists() { "pass" } else { "fail" },
            "message": if manifest_path.exists() {
                format!("Staged manifest exists for {}.", target.target.id)
            } else {
                format!("Staged manifest is missing for {}.", target.target.id)
            }
        }));
        if manifest_path.exists() {
            let manifest = load_compile_manifest(&manifest_path).map_err(state_error)?;
            let merged_outputs = manifest
                .generated_outputs
                .iter()
                .filter(|item| item.merge_status == Some(metactl::SurfaceMergeStatus::Merged))
                .count();
            let separate_outputs = manifest
                .generated_outputs
                .iter()
                .filter(|item| item.merge_status == Some(metactl::SurfaceMergeStatus::Separate))
                .count();
            let truncated_instruction_indexes = manifest
                .generated_outputs
                .iter()
                .filter(|item| {
                    item.degradation_codes
                        .iter()
                        .any(|code| code == "instruction_index_truncated")
                })
                .count();
            let surface_mode = manifest
                .surface_selection_mode
                .as_ref()
                .map(surface_selection_mode_label)
                .unwrap_or("unknown");
            checks.push(json!({
                "id": format!("surface-emission-{}", target.target.id),
                "status": if merged_outputs > 0 { "warn" } else { "pass" },
                "message": if merged_outputs > 0 {
                    format!("{} merged surface artifact(s) and {} separate surface artifact(s) are staged for {} in {} surface mode.", merged_outputs, separate_outputs, target.target.id, surface_mode)
                } else {
                    format!("{} separate surface artifact(s) are staged for {} in {} surface mode.", separate_outputs, target.target.id, surface_mode)
                }
            }));
            checks.push(json!({
                "id": format!("instruction-budget-{}", target.target.id),
                "status": if truncated_instruction_indexes > 0 { "warn" } else { "pass" },
                "message": if truncated_instruction_indexes > 0 {
                    format!("{} instruction index artifact(s) were truncated to stay within the always-on budget for {}.", truncated_instruction_indexes, target.target.id)
                } else {
                    format!("Instruction index artifacts are within budget for {}.", target.target.id)
                }
            }));
        }
        if let Some(report_path) = target.policy_report_path.as_ref() {
            let policy_path = project_root.join(report_path);
            checks.push(json!({
                "id": format!("policy-report-{}", target.target.id),
                "status": if policy_path.exists() { "pass" } else { "warn" },
                "message": if policy_path.exists() {
                    format!("Policy report exists for {}.", target.target.id)
                } else {
                    format!("Policy report is missing for {}.", target.target.id)
                }
            }));
        }
        if context.registry.is_some() {
            let kernel = kernel_from_context(&context).map_err(internal_error)?;
            let drift = kernel
                .detect_drift(&project_root, &target.target)
                .map_err(state_error)?;
            checks.push(json!({
                "id": format!("drift-{}", target.target.id),
                "status": match drift.status {
                    ValidationStatus::Pass => "pass",
                    ValidationStatus::Warn => "warn",
                    ValidationStatus::Fail => "fail",
                },
                "message": drift.checks.into_iter().map(|item| item.message).collect::<Vec<_>>().join(" "),
            }));
        }
    }

    let force_no_symlink = symlink_forced_off();
    checks.push(json!({
        "id": "symlink-posture",
        "status": if force_no_symlink { "warn" } else { "pass" },
        "message": if force_no_symlink {
            "Symlink mode is disabled in this environment; apply will fall back to copy mode."
        } else {
            "Symlink mode is available when the target supports it."
        }
    }));

    // Brownfield detection: check for unmanaged files
    let brownfield_files = metactl::project::detect_brownfield_files(&project_root);
    if !brownfield_files.is_empty() {
        let message = format!(
            "Unmanaged files detected: {}. Run 'metactl sync --adopt preview' to see what would be applied.",
            brownfield_files.join(", ")
        );
        checks.push(json!({
            "id": "brownfield-detection",
            "status": "warn",
            "message": message,
        }));
    }

    match resolve_ignore_targets(&project_root, &context.config_file.targets) {
        Ok(ignore_resolution) => {
            let ignore_targets = ignore_resolution.targets;
            let tracked_generated_roots =
                tracked_generated_roots_json(&project_root, &ignore_targets)
                    .map_err(state_error)?;
            let ignore_files = ignore_status_files(&project_root);
            let missing_ignore = ignore_files
                .iter()
                .any(|item| !item["installed"].as_bool().unwrap_or(false));
            let ignore_needs_repair = missing_ignore || !tracked_generated_roots.is_empty();
            let ignore_next = ignore_next_commands(
                IgnoreScopeArg::Both,
                &ignore_targets,
                true,
                ignore_needs_repair,
                !tracked_generated_roots.is_empty(),
            );
            checks.push(json!({
                "id": "ignore-repair",
                "status": if ignore_needs_repair { "warn" } else { "pass" },
                "message": if ignore_needs_repair {
                    "Generated-surface ignore posture needs repair. Run `metactl ignore fix --plan`."
                } else {
                    "Generated-surface ignore posture is repairable and has no tracked generated roots."
                },
                "fix_plan_ref": "metactl ignore fix --plan",
                "target_source": ignore_resolution.source,
                "targets": ignore_targets,
                "tracked_generated_roots": tracked_generated_roots,
                "next_commands": ignore_next,
            }));
        }
        Err(error) => {
            checks.push(json!({
                "id": "ignore-repair",
                "status": "warn",
                "message": format!("Generated-surface ignore repair skipped: {error}"),
                "fix_plan_ref": "metactl ignore fix --plan",
                "next_commands": ["metactl target list", "metactl doctor"],
            }));
        }
    }

    let mut doctor_lines = vec!["Doctor:".to_string()];
    for check in &checks {
        let id = check["id"].as_str().unwrap_or("?");
        let status = check["status"].as_str().unwrap_or("?");
        let message = check["message"].as_str().unwrap_or("");
        let icon = match status {
            "pass" => "ok",
            "warn" => "!",
            "fail" => "FAIL",
            _ => "?",
        };
        doctor_lines.push(format!("  [{}] {}: {}", icon, id, message));
        if let Some(commands) = check["next_commands"].as_array() {
            for command in commands.iter().filter_map(|item| item.as_str()) {
                doctor_lines.push(format!("    next: {command}"));
            }
        }
        if id == "source-audit" {
            if let Some(findings) = check["findings"].as_array() {
                if !findings.is_empty() {
                    doctor_lines.push("    next: metactl audit sources".to_string());
                    for line in source_audit_summary_lines(findings, 3) {
                        doctor_lines.push(format!("    {line}"));
                    }
                }
            }
        }
    }
    Ok(CommandOutput {
        human: project_human_output(&project_root, doctor_lines.join("\n")),
        json: success_json(
            "doctor",
            Some(&project_root),
            json!({
                "checks": checks,
            }),
        ),
    })
}

fn doctor_output(project_root: &Path, checks: Vec<Value>) -> CommandOutput {
    let mut doctor_lines = vec!["Doctor:".to_string()];
    for check in &checks {
        let id = check["id"].as_str().unwrap_or("?");
        let status = check["status"].as_str().unwrap_or("?");
        let message = check["message"].as_str().unwrap_or("");
        let icon = match status {
            "pass" => "ok",
            "warn" => "!",
            "fail" => "FAIL",
            _ => "?",
        };
        doctor_lines.push(format!("  [{}] {}: {}", icon, id, message));
        if let Some(commands) = check["next_commands"].as_array() {
            for command in commands.iter().filter_map(|item| item.as_str()) {
                doctor_lines.push(format!("    next: {command}"));
            }
        }
    }
    CommandOutput {
        human: project_human_output(project_root, doctor_lines.join("\n")),
        json: success_json(
            "doctor",
            Some(project_root),
            json!({
                "checks": checks,
            }),
        ),
    }
}

fn load_optional_context(cli: &Cli, project_root: &Path) -> Result<OptionalContext> {
    let config_path = project_config_path(project_root, cli.config.as_deref());
    if !config_path.exists() {
        return Ok(OptionalContext {
            registry: None,
            config_file: None,
        });
    }
    let context = load_project_context(
        project_root,
        cli.config.as_deref(),
        cli.profile.as_deref(),
        cli.overlay.as_deref(),
    )?;
    Ok(OptionalContext {
        registry: context.registry,
        config_file: Some(context.config_file),
    })
}

fn load_required_context(
    cli: &Cli,
    project_root: &Path,
) -> std::result::Result<metactl::project::ProjectContext, CliError> {
    load_required_context_for_path(cli, project_root)
}

#[derive(Debug)]
struct OptionalContext {
    registry: Option<LibraryRegistry>,
    config_file: Option<ProjectConfigFile>,
}

fn load_registry_for_paths(
    paths: &[String],
    project_root: &Path,
) -> Result<Option<LibraryRegistry>> {
    let roots = resolve_starter_library_roots(project_root, paths)?
        .into_iter()
        .filter(|path| path.exists())
        .collect::<Vec<_>>();
    if roots.is_empty() {
        return Ok(None);
    }
    Ok(Some(LibraryRegistry::load_from_roots(&roots)?))
}

fn kernel_from_context(context: &metactl::project::ProjectContext) -> Result<ReferenceKernel> {
    let roots = context.library_roots.clone();
    if roots.is_empty() {
        return Err(anyhow!("no starter library roots are configured"));
    }
    ReferenceKernel::load_from_library_roots(roots)
}

fn search_output(
    project_root: &Path,
    query: &str,
    result: &SearchResult,
    show_suppressed: bool,
) -> CommandOutput {
    let classification = search_classification(result);
    let mut lines = vec![format!("Search results for \"{query}\":")];
    if result.matches.is_empty() {
        lines.push(format!("No matches. Classification: {classification}."));
        lines.extend(
            next_steps_for_search(classification)
                .into_iter()
                .map(|step| format!("Next: {step}")),
        );
    } else {
        for item in &result.matches {
            lines.push(format!(
                "- {} ({:.2}) {}",
                item.pack_ref.id, item.score, item.why
            ));
        }
    }
    if show_suppressed && !result.suppressed.is_empty() {
        lines.push("Suppressed:".to_string());
        for item in &result.suppressed {
            lines.push(format!(
                "- {} {:?} {}",
                item.pack_ref.id,
                item.reason_code,
                item.detail.clone().unwrap_or_default()
            ));
        }
    }

    CommandOutput {
        human: lines.join("\n"),
        json: success_json(
            "search",
            Some(project_root),
            json!({
                "query": query,
                "classification": classification,
                "result_count": result.matches.len(),
                "matches": result.matches,
                "suppressed": result.suppressed,
                "notes": result.notes,
                "next_steps": next_steps_for_search(classification),
            }),
        ),
    }
}

fn explain_output(
    project_root: &Path,
    query: Option<&str>,
    explain: &ExplainResult,
    target_projection: &Value,
    surface_usage_summary: &Value,
    surface_details: Option<&[metactl::library_registry::PackSurfaceSummary]>,
    pack_lifecycle: Option<&std::collections::BTreeMap<String, metactl::PackLifecycle>>,
    pack_sources: &Value,
) -> CommandOutput {
    let mut lines = vec![explain.summary.clone()];
    if let Some(query) = query {
        lines.push(format!("Query context: {query}"));
    }
    if !explain.what_is_active.is_empty() {
        lines.push("Active:".to_string());
        lines.extend(
            explain
                .what_is_active
                .iter()
                .map(|item| format!("- {item}")),
        );
    }
    if !explain.what_was_suppressed.is_empty() {
        lines.push("Suppressed:".to_string());
        lines.extend(explain.what_was_suppressed.iter().map(|item| {
            format!(
                "- {} {:?} {}",
                item.subject_ref.id,
                item.reason_code,
                item.detail.clone().unwrap_or_default()
            )
        }));
    }
    if !explain.unknown_or_unsupported.is_empty() {
        lines.push("Gaps:".to_string());
        lines.extend(
            explain
                .unknown_or_unsupported
                .iter()
                .map(|item| format!("- {item}")),
        );
    }
    lines.push("Projection:".to_string());
    if let Some(summary) = target_projection["summary"].as_str() {
        lines.push(format!("- {summary}"));
    }
    if let Some(instruction_behavior) = target_projection["instruction_behavior"].as_str() {
        lines.push(format!("- {instruction_behavior}"));
    }
    if let Some(instruction_budget) = target_projection["instruction_budget"].as_str() {
        lines.push(format!("- {instruction_budget}"));
    }
    if let Some(surface_behavior) = target_projection["surface_behavior"].as_str() {
        lines.push(format!("- {surface_behavior}"));
    }
    if let Some(surface_mode) = target_projection["surface_selection_mode"].as_str() {
        lines.push(format!("- Surface selection mode: {surface_mode}."));
    }
    lines.push("Surface recommendations:".to_string());
    let stats_state = if surface_usage_summary["stats_stale"].as_bool() == Some(true) {
        "stale"
    } else if surface_usage_summary["stats_exists"].as_bool() == Some(true) {
        "current"
    } else {
        "not built"
    };
    lines.push(format!("- Usage stats: {stats_state}."));
    if let Some(next) = surface_usage_summary["next_reversible_action"].as_str() {
        lines.push(format!("- Next reversible action: {next}."));
    }
    if let Some(surface_details) = surface_details {
        if !surface_details.is_empty() {
            lines.push("Surface detail:".to_string());
            lines.extend(surface_details.iter().map(|pack| {
                let total = pack.surfaces.len();
                let emitted = pack
                    .surfaces
                    .iter()
                    .filter(|surface| surface.emitted)
                    .count();
                let suppressed = total.saturating_sub(emitted);
                format!(
                    "- pack {}: {} emitted, {} suppressed ({} eligible, mode: {}, emission: {})",
                    pack.pack_ref.id,
                    emitted,
                    suppressed,
                    total,
                    surface_selection_mode_label(&pack.selection_mode),
                    pack.emission_mode
                )
            }));
        }
    }
    // Build explanation certificates
    let certificates = build_explanation_certificates(explain);
    if !certificates.is_empty() {
        lines.push("Certificates:".to_string());
        for cert in &certificates {
            lines.push(format!(
                "- [{}] {}",
                cert["subject"].as_str().unwrap_or("?"),
                cert["conclusion"].as_str().unwrap_or("")
            ));
        }
    }

    lines.push("Next: metactl sync".to_string());
    CommandOutput {
        human: lines.join("\n"),
        json: success_json(
            "explain",
            Some(project_root),
            json!({
                "query": query,
                "summary": explain.summary,
                "what_is_active": explain.what_is_active,
                "why_it_is_active": explain.why_it_is_active,
                "what_was_suppressed": explain.what_was_suppressed,
                "unknown_or_unsupported": explain.unknown_or_unsupported,
                "resolve_graph": explain.resolve_graph,
                "target_projection": target_projection,
                "surface_usage": surface_usage_summary,
                "surface_details": surface_details,
                "pack_lifecycle": pack_lifecycle,
                "pack_sources": pack_sources,
                "certificates": certificates,
                "next_steps": ["metactl sync", "metactl compile"],
            }),
        ),
    }
}

fn select_locked_targets(
    lock: &ProjectLock,
    target_id: Option<String>,
) -> std::result::Result<Vec<LockedTarget>, CliError> {
    if let Some(target_id) = target_id {
        let selected = lock
            .targets
            .iter()
            .find(|item| item.target.id == target_id)
            .cloned()
            .into_iter()
            .collect::<Vec<_>>();
        if selected.is_empty() {
            return Err(state_error(anyhow!(
                "target {} is not present in metactl.lock.json",
                target_id
            )));
        }
        return Ok(selected);
    }
    Ok(lock.targets.clone())
}

fn normalize_apply_mode(
    requested: Option<ApplyMode>,
    preferred: &ApplyMode,
    manifest: &CompileManifest,
    target_capability: Option<&TargetCapabilityMatrix>,
) -> std::result::Result<(ApplyMode, Option<String>), CliError> {
    let requested = requested.unwrap_or_else(|| preferred.clone());
    if requested == ApplyMode::Symlink
        && (symlink_forced_off() || !manifest.apply_modes_supported.contains(&ApplyMode::Symlink))
        && manifest.apply_modes_supported.contains(&ApplyMode::Copy)
    {
        return Ok((
            ApplyMode::Copy,
            Some("Symlink mode is unavailable here; apply fell back to copy mode.".to_string()),
        ));
    }
    if requested == ApplyMode::Takeover {
        if let Some(target) = target_capability {
            if !target_supports_takeover(target) {
                let message = format!(
                    "Target '{}' does not support takeover mode (uses reference-based indexes).\n\
                     Use `metactl apply -t {} --mode patch` instead, or try:\n\
                     \n{}",
                    target.target_id,
                    target.target_id,
                    brownfield_adoption_hint()
                );
                return Err(state_error(anyhow!(message)));
            }
        }
    }
    if manifest.apply_modes_supported.contains(&requested) {
        return Ok((requested, None));
    }
    Err(state_error(anyhow!(
        "apply mode {:?} is not supported for target {}",
        requested,
        manifest.target.id
    )))
}

fn preview_manifest(project_root: &Path, manifest: &CompileManifest) -> Vec<serde_json::Value> {
    let managed_index = load_managed_index(project_root);
    manifest
        .generated_outputs
        .iter()
        .map(|item| {
            let destination = item.destination_path.clone().unwrap_or_default();
            let destination_abs = project_root.join(&destination);
            let classification = if managed_index.contains(&destination) {
                "managed"
            } else if destination_abs.exists() {
                "unmanaged-existing"
            } else {
                "new"
            };
            json!({
                "destination_path": destination,
                "staged_path": item.path,
                "classification": classification,
            })
        })
        .collect()
}

fn load_managed_index(project_root: &Path) -> BTreeSet<String> {
    let path = project_root.join(".metactl/state/managed_files.json");
    let Ok(raw) = fs::read(&path) else {
        return BTreeSet::new();
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&raw) else {
        return BTreeSet::new();
    };
    value
        .as_object()
        .into_iter()
        .flat_map(|map| map.values())
        .filter_map(|items| items.as_array())
        .flat_map(|items| items.iter())
        .filter_map(|item| {
            item.as_str().map(ToString::to_string).or_else(|| {
                item.get("destination_path")
                    .and_then(|value| value.as_str())
                    .map(ToString::to_string)
            })
        })
        .collect()
}

fn shared_surface_rules(
    registry: Option<&LibraryRegistry>,
    target_ids: &[String],
) -> Vec<SharedSurfaceRule> {
    let Some(registry) = registry else {
        return Vec::new();
    };

    let mut grouped: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for target_id in target_ids {
        let Some(target) = registry.target_by_id(target_id) else {
            continue;
        };
        let (Some(path), Some(owner)) = (
            target.metadata.get("shared_root_document_path"),
            target.metadata.get("shared_root_document_owner"),
        ) else {
            continue;
        };
        grouped
            .entry((path.clone(), owner.clone()))
            .or_default()
            .insert(target_id.clone());
    }

    grouped
        .into_iter()
        .filter_map(|((path, owner), participants)| {
            if participants.len() < 2 || !participants.contains(&owner) {
                return None;
            }
            let suppressed_targets = participants
                .into_iter()
                .filter(|target_id| target_id != &owner)
                .collect::<Vec<_>>();
            if suppressed_targets.is_empty() {
                return None;
            }
            let message = format!(
                "{} is owned by {} when these targets are enabled; {} use target-local surfaces only.",
                path,
                owner,
                suppressed_targets.join(", ")
            );
            Some(SharedSurfaceRule {
                path,
                owner,
                suppressed_targets,
                message,
            })
        })
        .collect()
}

fn apply_shared_surface_rules_to_manifest(
    project_root: &Path,
    target_id: &str,
    manifest: &mut CompileManifest,
    shared_rules: &[SharedSurfaceRule],
) -> Result<()> {
    for rule in shared_rules {
        if !rule.suppressed_targets.iter().any(|item| item == target_id) {
            continue;
        }

        let mut retained = Vec::new();
        for output in manifest.generated_outputs.drain(..) {
            if output.destination_path.as_deref() == Some(rule.path.as_str()) {
                let staged_path = project_root.join(&output.path);
                if staged_path.exists() {
                    fs::remove_file(&staged_path)
                        .with_context(|| format!("remove {}", staged_path.display()))?;
                }
            } else {
                retained.push(output);
            }
        }
        manifest.generated_outputs = retained;
    }
    Ok(())
}

fn write_compile_manifest_json(path: &Path, manifest: &CompileManifest) -> Result<()> {
    atomic_write(
        path,
        &serde_json::to_vec_pretty(manifest).context("serialize compile manifest")?,
    )
    .with_context(|| format!("write {}", path.display()))
}

fn search_classification(result: &SearchResult) -> &'static str {
    if !result.matches.is_empty() {
        "matches"
    } else if result
        .suppressed
        .iter()
        .any(|item| format!("{:?}", item.reason_code).contains("UnsupportedTarget"))
    {
        "incompatible_target"
    } else if result
        .suppressed
        .iter()
        .any(|item| format!("{:?}", item.reason_code).contains("IncompatibleRole"))
    {
        "incompatible_role"
    } else if !result.suppressed.is_empty() {
        "blocked_by_policy"
    } else {
        "zero_match"
    }
}

fn next_steps_for_search(classification: &str) -> Vec<&'static str> {
    match classification {
        "no_corpus" => vec![
            "configure a starter library in metactl.yaml",
            "run metactl doctor",
        ],
        "blocked_by_policy" => vec!["inspect metactl explain", "adjust policy or query"],
        "incompatible_role" => vec!["try a different --role", "run metactl list roles"],
        "incompatible_target" => vec!["try a different --target", "run metactl list targets"],
        "zero_match" => vec!["broaden the query", "run metactl list packs --candidate"],
        _ => vec!["run metactl explain", "run metactl sync"],
    }
}

fn no_corpus_output(command: &str, project_root: &Path) -> CommandOutput {
    CommandOutput {
        human: "No starter library is available.\nNext: add a starter library path to metactl.yaml or run metactl doctor.".to_string(),
        json: success_json(command, Some(project_root), json!({
            "classification": "no_corpus",
            "result_count": 0,
            "next_steps": next_steps_for_search("no_corpus"),
        })),
    }
}

fn init_partial_config(
    config: &ProjectConfigFile,
    profile: &metactl::project::PartialProjectConfig,
    starter_library_explicit: bool,
    targets_explicit: bool,
    role_explicit: bool,
    policy_explicit: bool,
) -> metactl::project::PartialProjectConfig {
    let mut partial = metactl::project::PartialProjectConfig {
        extends_profile: config.extends_profile.clone(),
        api_version: Some(config.api_version.clone()),
        defaults: config.defaults.clone(),
        metadata: config.metadata.clone(),
        ..metactl::project::PartialProjectConfig::default()
    };
    if role_explicit || profile.role.is_none() {
        partial.role = Some(config.role.clone());
    }
    if policy_explicit || profile.policy.is_none() {
        partial.policy = Some(config.policy.clone());
    }
    if targets_explicit || profile.targets.is_empty() {
        partial.targets = config.targets.clone();
    }
    if starter_library_explicit || profile.starter_library.is_empty() {
        partial.starter_library = config.starter_library.clone();
    }
    if profile.packs.is_empty() {
        partial.packs = config.packs.clone();
    }
    partial
}

fn context_profile_path(profile: Option<&str>) -> Option<String> {
    profile
        .and_then(metactl::project::profile_path)
        .map(|path| path.to_string_lossy().to_string())
}

fn context_profile_digest(profile: Option<&str>) -> Result<Option<String>> {
    let Some(path) = profile.and_then(metactl::project::profile_path) else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(digest_path(&path)?))
}

fn profile_status_json(context: &metactl::project::ProjectContext) -> Value {
    let project_yaml_binding = context.raw_config_file.extends_profile.is_some();
    let Some(profile) = context.active_profile.as_ref() else {
        return json!({
            "status": "none",
            "message": "No profile is active.",
            "activation_source": Value::Null,
            "project_yaml_binding": project_yaml_binding,
        });
    };
    let activation_source = match profile.source {
        ProfileActivationSource::Cli => "cli",
        ProfileActivationSource::ProjectExtends => "project_extends",
        ProfileActivationSource::UserDefault => "user_default",
    };
    let overrides = profile_override_fields(&context.raw_config_file, &profile.partial);
    let stale = profile.digest.is_none()
        || (!context.lock.targets.is_empty()
            && (context.lock.profile_name.as_deref() != Some(profile.name.as_str())
                || context.lock.profile_digest != profile.digest));
    let status = if stale {
        "stale"
    } else if !overrides.is_empty() {
        "diverged"
    } else {
        "synced"
    };
    json!({
        "status": status,
        "name": profile.name,
        "path": profile.path,
        "digest": profile.digest,
        "overrides": overrides,
        "activation_source": activation_source,
        "project_yaml_binding": project_yaml_binding,
    })
}

fn profile_override_fields(
    project: &metactl::project::PartialProjectConfig,
    profile: &metactl::project::PartialProjectConfig,
) -> Vec<&'static str> {
    let mut overrides = Vec::new();
    if project.role.is_some() && profile.role.is_some() && project.role != profile.role {
        overrides.push("role");
    }
    if !project.packs.is_empty() && !profile.packs.is_empty() && project.packs != profile.packs {
        overrides.push("packs");
    }
    if project.policy.is_some() && profile.policy.is_some() && project.policy != profile.policy {
        overrides.push("policy");
    }
    if !project.targets.is_empty()
        && !profile.targets.is_empty()
        && project.targets != profile.targets
    {
        overrides.push("targets");
    }
    if !project.starter_library.is_empty()
        && !profile.starter_library.is_empty()
        && project.starter_library != profile.starter_library
    {
        overrides.push("starter_library");
    }
    overrides
}

fn profile_status_message(profile: &Value) -> String {
    let name = profile["name"].as_str().unwrap_or_default();
    let source = profile["activation_source"].as_str().unwrap_or("");
    let yaml_binding = profile["project_yaml_binding"].as_bool().unwrap_or(false);
    match profile["status"].as_str().unwrap_or("none") {
        "none" => "No profile is active.".to_string(),
        "synced" => {
            if source == "user_default" && !yaml_binding {
                format!(
                    "Machine default profile {name} is active locally (not recorded in metactl.yaml); run `metactl init --bind-profile` if this repo should track it."
                )
            } else {
                format!("Bound profile {name} is in sync.")
            }
        }
        "diverged" => format!(
            "Profile {name} is active, but the project overrides {}.",
            profile["overrides"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default()
        ),
        "stale" => format!("Profile {name} changed or is unavailable since the last sync.",),
        _ => "Profile status is unknown.".to_string(),
    }
}

fn target_readiness_json(
    project_root: &Path,
    lock: &ProjectLock,
    apply_json: &Value,
) -> std::result::Result<Vec<Value>, CliError> {
    let preview = apply_json["preview"].as_bool().unwrap_or(false);
    let apply_modes = apply_json["targets"]
        .as_array()
        .into_iter()
        .flat_map(|items| items.iter())
        .filter_map(|item| {
            Some((
                item.get("target")?.as_str()?.to_string(),
                item.get("apply_mode")?.clone(),
            ))
        })
        .collect::<BTreeMap<_, _>>();

    lock.targets
        .iter()
        .map(|target| {
            let manifest =
                load_compile_manifest(&project_root.join(&target.compile_manifest_path))
                    .map_err(state_error)?;
            let runtime_paths = manifest
                .generated_outputs
                .iter()
                .filter_map(|item| item.destination_path.clone())
                .collect::<Vec<_>>();
            let missing_paths = if preview {
                Vec::new()
            } else {
                runtime_paths
                    .iter()
                    .filter(|path| !project_root.join(path).exists())
                    .cloned()
                    .collect::<Vec<_>>()
            };
            let degraded = !manifest.degradations.is_empty()
                || manifest.generated_outputs.iter().any(|item| {
                    !item.degradation_codes.is_empty()
                        || item.merge_status == Some(metactl::SurfaceMergeStatus::Merged)
                });
            let status = if preview {
                "preview"
            } else if !missing_paths.is_empty() {
                "blocked"
            } else if degraded {
                "degraded"
            } else {
                "ready"
            };
            Ok(json!({
                "target": target.target.id,
                "status": status,
                "apply_mode": apply_modes
                    .get(&target.target.id)
                    .cloned()
                    .unwrap_or_else(|| Value::String(format!("{:?}", target.preferred_apply_mode).to_ascii_lowercase())),
                "runtime_paths": runtime_paths,
                "missing_paths": missing_paths,
                "degradations": manifest.degradations,
                "surface_selection_mode": manifest.surface_selection_mode.as_ref().map(surface_selection_mode_label),
            }))
        })
        .collect()
}

fn unique_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut unique = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            unique.push(value);
        }
    }
    unique
}

fn expand_target_ids(
    requested: &[String],
    registry: Option<&LibraryRegistry>,
) -> Result<Vec<String>> {
    let mut expanded = Vec::new();
    for target_id in requested {
        if target_id == "all" {
            let registry = registry.ok_or_else(|| {
                anyhow!("target expansion `all` requires a discovered starter library")
            })?;
            let available = registry
                .list_targets()
                .into_iter()
                .map(|target| target.target_id)
                .collect::<Vec<_>>();
            if available.is_empty() {
                return Err(anyhow!(
                    "target expansion `all` found no starter-supported targets"
                ));
            }
            expanded.extend(available);
        } else {
            expanded.push(target_id.clone());
        }
    }
    Ok(unique_strings(expanded))
}

fn run_init_target_wizard(available: &[String]) -> std::result::Result<Vec<String>, CliError> {
    if available.is_empty() {
        return Err(
            CliError::new(EXIT_STATE, "No targets are available for interactive init.")
                .with_details(init_target_next_steps(available)),
        );
    }

    eprintln!("metactl init target setup");
    eprintln!("Choose one or more instruction targets for this project.");
    eprintln!("Generated files stay target-specific; preview before applying changes.");
    eprintln!();
    eprintln!("Available targets:");
    for (index, target) in available.iter().enumerate() {
        eprintln!("  {}. {}", index + 1, target);
    }
    eprintln!();
    eprint!("Target ids, numbers, or 'all': ");
    io::stderr().flush().map_err(internal_error)?;

    let mut input = String::new();
    io::stdin().read_line(&mut input).map_err(internal_error)?;
    parse_init_wizard_targets(&input, available)
}

fn parse_init_wizard_targets(
    input: &str,
    available: &[String],
) -> std::result::Result<Vec<String>, CliError> {
    let available_set = available.iter().cloned().collect::<BTreeSet<_>>();
    let tokens = input
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();

    if tokens.is_empty() {
        return Err(CliError::new(EXIT_STATE, "No init target was selected.")
            .with_details(init_target_next_steps(available)));
    }

    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();
    for token in tokens {
        if token.eq_ignore_ascii_case("all") {
            return Ok(available.to_vec());
        }

        let target_id = if let Ok(index) = token.parse::<usize>() {
            if index == 0 || index > available.len() {
                return Err(CliError::new(
                    EXIT_STATE,
                    format!("Init target selection '{}' is out of range.", token),
                )
                .with_details(init_target_next_steps(available)));
            }
            available[index - 1].clone()
        } else {
            let (canonical, _) = resolve_target_alias(token);
            if !available_set.contains(&canonical) {
                return Err(CliError::new(
                    EXIT_STATE,
                    format!("Init target '{}' is not available.", token),
                )
                .with_details(init_target_next_steps(available)));
            }
            canonical
        };

        if seen.insert(target_id.clone()) {
            selected.push(target_id);
        }
    }

    Ok(selected)
}

fn available_targets_display(available: &[String]) -> String {
    if available.is_empty() {
        "(none discovered)".to_string()
    } else {
        available.join(", ")
    }
}

fn init_target_next_steps(available: &[String]) -> Vec<String> {
    let target_hint = available
        .first()
        .cloned()
        .unwrap_or_else(|| "<id>".to_string());
    vec![
        format!(
            "Available targets: {}",
            available_targets_display(available)
        ),
        format!("Next: metactl init --target {target_hint}"),
        "Next: metactl init --target all".to_string(),
    ]
}

#[cfg(test)]
mod cli_init_wizard_tests {
    use super::*;

    fn available_targets() -> Vec<String> {
        vec![
            "codex-cli".to_string(),
            "claude-code".to_string(),
            "gemini-cli".to_string(),
        ]
    }

    #[test]
    fn init_wizard_targets_accept_numbers_ids_aliases_and_deduplicate() {
        let selected =
            parse_init_wizard_targets("2 codex 1", &available_targets()).expect("selection");

        assert_eq!(
            selected,
            vec!["claude-code".to_string(), "codex-cli".to_string()]
        );
    }

    #[test]
    fn init_wizard_targets_accept_all() {
        let selected = parse_init_wizard_targets("all", &available_targets()).expect("selection");

        assert_eq!(selected, available_targets());
    }

    #[test]
    fn init_wizard_targets_report_recoverable_next_steps() {
        let error = parse_init_wizard_targets("unknown", &available_targets()).unwrap_err();

        assert_eq!(error.code, EXIT_STATE);
        assert!(error.message.contains("not available"));
        assert!(error
            .details
            .iter()
            .any(|detail| detail == "Next: metactl init --target codex-cli"));
    }

    #[test]
    fn init_target_next_steps_do_not_repeat_detect_after_detect_fails() {
        let details = init_target_next_steps(&available_targets());

        assert!(details
            .iter()
            .any(|detail| detail == "Next: metactl init --target codex-cli"));
        assert!(!details
            .iter()
            .any(|detail| detail == "Next: metactl init --detect"));
    }
}

fn resolve_target_alias(target_id: &str) -> (String, bool) {
    // Returns (canonical_id, was_aliased)
    match target_id {
        "claude" => ("claude-code".to_string(), true),
        "codex" => ("codex-cli".to_string(), true),
        "gemini" => ("gemini-cli".to_string(), true),
        id => (id.to_string(), false),
    }
}

impl DiscoverabilityReport {
    fn is_blocked(&self) -> bool {
        self.missing_role
            || self.missing_policy
            || !self.missing_targets.is_empty()
            || !self.missing_packs.is_empty()
    }

    fn blocking_checks_json(&self) -> Vec<Value> {
        let mut checks = Vec::new();
        if self.missing_role {
            checks.push(json!({
                "id": "role-discovery",
                "missing_role": self.role_id,
            }));
        }
        if self.missing_policy {
            checks.push(json!({
                "id": "policy-discovery",
                "missing_policy": self.policy_id,
            }));
        }
        if !self.missing_targets.is_empty() {
            checks.push(json!({
                "id": "target-discovery",
                "missing_targets": self.missing_targets,
                "effective_library_roots": self.effective_library_roots,
            }));
        }
        if !self.missing_packs.is_empty() {
            checks.push(json!({
                "id": "pack-discovery",
                "missing_packs": self.missing_packs,
            }));
        }
        checks
    }

    fn human_blockers(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.missing_role {
            lines.push(format!(
                "configured role {} is not discoverable from the effective library roots",
                self.role_id
            ));
        }
        if self.missing_policy {
            lines.push(format!(
                "configured policy {} is not discoverable from the effective library roots",
                self.policy_id
            ));
        }
        for target_id in &self.missing_targets {
            lines.push(format!(
                "configured target {} is not discoverable from the effective library roots",
                target_id
            ));
        }
        for pack_id in &self.missing_packs {
            lines.push(format!(
                "configured pack {} is not discoverable from the effective library roots",
                pack_id
            ));
        }
        lines
    }

    fn suggested_actions(&self) -> Vec<String> {
        let target_hint = if self.missing_targets.is_empty() {
            "the missing entities"
        } else {
            "targets"
        };
        let bundled = bundled_starter_library_root();
        let example = format!(" (for example {})", bundled.to_string_lossy());
        let scope = self
            .profile_name
            .as_ref()
            .map(|name| format!("profile '{}' or metactl.yaml", name))
            .unwrap_or_else(|| "metactl.yaml".to_string());
        vec![
            format!("add a library root that contains {target_hint}{example} to {scope}"),
            "run `metactl doctor` to confirm discoverability before retrying sync".to_string(),
        ]
    }
}

fn discoverability_report(
    context: &metactl::project::ProjectContext,
    overrides: &ConfigOverrides,
) -> DiscoverabilityReport {
    let registry = context.registry.as_ref();
    let role_id = overrides
        .role
        .clone()
        .unwrap_or_else(|| context.config_file.role.clone());
    let policy_id = overrides
        .policy
        .clone()
        .unwrap_or_else(|| context.config_file.policy.clone());
    let target_ids = context.selected_target_ids(overrides);
    let pack_ids = context.config_file.packs.clone();

    let missing_role = registry
        .map(|registry| registry.role_by_id(&role_id).is_none())
        .unwrap_or(true);
    let missing_policy = registry
        .map(|registry| registry.policy_by_id(&policy_id).is_none())
        .unwrap_or(true);
    let missing_targets = target_ids
        .iter()
        .filter(|target_id| {
            registry
                .map(|registry| registry.target_by_id(target_id).is_none())
                .unwrap_or(true)
        })
        .cloned()
        .collect::<Vec<_>>();
    let missing_packs = pack_ids
        .iter()
        .filter(|pack_id| {
            registry
                .map(|registry| registry.pack_by_id(pack_id).is_none())
                .unwrap_or(true)
        })
        .cloned()
        .collect::<Vec<_>>();

    DiscoverabilityReport {
        role_id,
        policy_id,
        target_ids,
        missing_role,
        missing_policy,
        missing_targets,
        missing_packs,
        effective_library_roots: context
            .library_roots
            .iter()
            .map(|root| root.to_string_lossy().to_string())
            .collect(),
        profile_name: context
            .active_profile
            .as_ref()
            .map(|profile| profile.name.clone()),
    }
}

fn discoverability_error(report: &DiscoverabilityReport) -> CliError {
    let (message, reason_code) = if !report.missing_targets.is_empty() {
        (
            format!(
                "Configured target(s) {} could not be discovered in the effective library roots.\nRun `metactl doctor` for a detailed readiness report.",
                report.missing_targets.join(", ")
            ),
            "target_discovery_blocked",
        )
    } else if report.missing_role {
        (
            format!(
                "Configured role {} could not be discovered in the effective library roots.\nRun `metactl doctor` for a detailed readiness report.",
                report.role_id
            ),
            "role_discovery_blocked",
        )
    } else if report.missing_policy {
        (
            format!(
                "Configured policy {} could not be discovered in the effective library roots.\nRun `metactl doctor` for a detailed readiness report.",
                report.policy_id
            ),
            "policy_discovery_blocked",
        )
    } else {
        (
            format!(
                "Configured pack(s) {} could not be discovered in the effective library roots.\nRun `metactl doctor` for a detailed readiness report.",
                report.missing_packs.join(", ")
            ),
            "pack_discovery_blocked",
        )
    };

    let mut details = Vec::new();
    if !report.effective_library_roots.is_empty() {
        details.push(format!(
            "effective library roots: {}",
            report.effective_library_roots.join(", ")
        ));
    }
    details.extend(report.suggested_actions());

    let mut err = CliError::new(EXIT_STATE, message).with_details(details);
    if let Some(obj) = err.json.as_object_mut() {
        obj.insert("reason_code".to_string(), json!(reason_code));
        obj.insert(
            "effective_library_roots".to_string(),
            json!(report.effective_library_roots),
        );
        obj.insert(
            "suggested_actions".to_string(),
            json!(report.suggested_actions()),
        );
        obj.insert("missing_targets".to_string(), json!(report.missing_targets));
        obj.insert(
            "missing_role".to_string(),
            if report.missing_role {
                json!(report.role_id)
            } else {
                Value::Null
            },
        );
        obj.insert(
            "missing_policy".to_string(),
            if report.missing_policy {
                json!(report.policy_id)
            } else {
                Value::Null
            },
        );
        obj.insert("missing_packs".to_string(), json!(report.missing_packs));
    }
    err
}

fn split_comma_args(items: &[String]) -> Vec<String> {
    items
        .iter()
        .flat_map(|item| item.split(','))
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn format_target_not_found_error(target_id: &str, registry: Option<&LibraryRegistry>) -> CliError {
    let available = get_available_target_ids(registry);
    let msg = if available.is_empty() {
        format!(
            "Target '{}' not found in configured targets.\nNo targets available in the starter library.",
            target_id
        )
    } else {
        format!(
            "Target '{}' not found in configured targets.\nAvailable targets: {}",
            target_id,
            available.join(", ")
        )
    };
    state_error(anyhow!(msg))
}

fn validate_target_ids(target_ids: &[String], registry: Option<&LibraryRegistry>) -> Result<()> {
    let Some(registry) = registry else {
        return Ok(());
    };
    let available = registry
        .list_targets()
        .into_iter()
        .map(|target| target.target_id)
        .collect::<Vec<_>>();
    let available_set = available.iter().cloned().collect::<BTreeSet<_>>();
    let unknown = target_ids
        .iter()
        .filter(|target_id| !available_set.contains(*target_id))
        .cloned()
        .collect::<Vec<_>>();
    if unknown.is_empty() {
        return Ok(());
    }
    Err(anyhow!(
        "Target(s) not found in starter library: {}. Available targets: {}",
        unknown.join(", "),
        available.join(", ")
    ))
}

fn get_available_target_ids(registry: Option<&LibraryRegistry>) -> Vec<String> {
    registry
        .map(|r| r.list_targets().into_iter().map(|t| t.target_id).collect())
        .unwrap_or_default()
}

fn registry_has_targets(registry: Option<&LibraryRegistry>) -> bool {
    registry.is_some_and(|registry| !registry.list_targets().is_empty())
}

fn target_projection_json(
    target: &TargetCapabilityMatrix,
    surface_details: Option<&[metactl::library_registry::PackSurfaceSummary]>,
    surface_selection_mode: metactl::SurfaceSelectionMode,
) -> Value {
    let outputs = target
        .compile_targets
        .iter()
        .map(|compile_target| {
            json!({
                "kind": compile_target_kind_label(&compile_target.output_kind),
                "path_template": compile_target.path_template,
                "resource_kinds": compile_target.resource_kinds.clone(),
                "instruction_mode": compile_target
                    .instruction_mode
                    .as_ref()
                    .map(instruction_projection_mode_label),
                "surface_selection_mode": compile_target
                    .surface_selection_mode
                    .as_ref()
                    .map(surface_selection_mode_label),
                "supports_multi_surface_pack": compile_target.supports_multi_surface_pack,
                "surface_merge_strategy": compile_target
                    .surface_merge_strategy
                    .as_ref()
                    .map(surface_merge_strategy_label),
            })
        })
        .collect::<Vec<_>>();
    let path_templates = target
        .compile_targets
        .iter()
        .map(|compile_target| compile_target.path_template.clone())
        .collect::<Vec<_>>();
    let summary = format!(
        "{} projects into {}.",
        target.target_id,
        path_templates.join(", ")
    );
    let instruction_behavior = target
        .compile_targets
        .iter()
        .find(|compile_target| {
            matches!(
                compile_target.output_kind,
                metactl::CompileTargetKind::AgentsMd
                    | metactl::CompileTargetKind::ClaudeMd
                    | metactl::CompileTargetKind::OpenclawMd
            )
        })
        .and_then(|compile_target| compile_target.instruction_mode.as_ref())
        .map(|mode| match mode {
            metactl::InstructionProjectionMode::ReferenceIndex => {
                "This target keeps the entry document concise and references emitted pack bodies."
            }
            metactl::InstructionProjectionMode::Inline => {
                "This target inlines pack guidance directly into the entry document."
            }
        })
        .unwrap_or("This target does not declare a document-style instruction projection.");
    let instruction_budget = target
        .compile_targets
        .iter()
        .find(|compile_target| {
            matches!(
                compile_target.output_kind,
                metactl::CompileTargetKind::AgentsMd
                    | metactl::CompileTargetKind::ClaudeMd
                    | metactl::CompileTargetKind::OpenclawMd
            )
        })
        .and_then(|compile_target| compile_target.instruction_mode.as_ref())
        .map(|_| "Instruction indexes warn/truncate above 8192 bytes and fail above 32768 bytes.")
        .unwrap_or("No instruction index budget applies for this target.");

    let (merged_packs, separate_packs) = surface_details
        .map(|details| {
            details
                .iter()
                .fold((0usize, 0usize), |(merged, separate), pack| {
                    match pack.emission_mode.as_str() {
                        "merged" => (merged + 1, separate),
                        "separate" => (merged, separate + 1),
                        _ => (merged, separate),
                    }
                })
        })
        .unwrap_or((0, 0));
    let surface_behavior = if merged_packs > 0 {
        format!(
            "{} active pack(s) merge derived surfaces on this target because separate skill folders are unavailable.",
            merged_packs
        )
    } else if separate_packs > 0 {
        format!(
            "{} active pack(s) emit separate derived skill surfaces on this target.",
            separate_packs
        )
    } else if target.capabilities.skill_folders {
        "This target can emit separate skill folders when packs derive more than one instruction surface.".to_string()
    } else {
        "This target merges multi-surface packs into document-style outputs because it does not support skill folders.".to_string()
    };

    json!({
        "target_id": target.target_id,
        "skill_folders": target.capabilities.skill_folders,
        "surface_selection_mode": surface_selection_mode_label(&surface_selection_mode),
        "summary": summary,
        "instruction_behavior": instruction_behavior,
        "instruction_budget": instruction_budget,
        "surface_behavior": surface_behavior,
        "outputs": outputs,
    })
}

fn known_surface_pack_ids(context: &metactl::project::ProjectContext) -> Result<Vec<String>> {
    if !context.has_corpus() {
        return Ok(surface_usage::known_pack_ids_from_refs(
            context.config_file.packs.clone(),
        ));
    }
    let config = context.effective_config(&ConfigOverrides::default())?;
    Ok(surface_usage::known_pack_ids_from_refs(
        config.packs.into_iter().map(|pack_ref| pack_ref.id),
    ))
}

fn instruction_projection_mode_label(mode: &metactl::InstructionProjectionMode) -> &'static str {
    match mode {
        metactl::InstructionProjectionMode::Inline => "inline",
        metactl::InstructionProjectionMode::ReferenceIndex => "reference_index",
    }
}

fn surface_selection_mode_label(mode: &metactl::SurfaceSelectionMode) -> &'static str {
    match mode {
        metactl::SurfaceSelectionMode::Minimal => "minimal",
        metactl::SurfaceSelectionMode::Full => "full",
        metactl::SurfaceSelectionMode::Auto => "auto",
    }
}

fn target_surface_selection_mode(target: &TargetCapabilityMatrix) -> metactl::SurfaceSelectionMode {
    target
        .compile_targets
        .iter()
        .find(|compile_target| compile_target.output_kind == metactl::CompileTargetKind::CodexSkill)
        .and_then(|compile_target| compile_target.surface_selection_mode.clone())
        .unwrap_or(metactl::SurfaceSelectionMode::Full)
}

fn surface_merge_strategy_label(strategy: &SurfaceMergeStrategy) -> &'static str {
    match strategy {
        SurfaceMergeStrategy::None => "none",
        SurfaceMergeStrategy::Optional => "optional",
        SurfaceMergeStrategy::Required => "required",
    }
}

fn compile_target_kind_label(kind: &metactl::CompileTargetKind) -> &'static str {
    match kind {
        metactl::CompileTargetKind::AgentsMd => "agents_md",
        metactl::CompileTargetKind::ClaudeMd => "claude_md",
        metactl::CompileTargetKind::OpenclawMd => "openclaw_md",
        metactl::CompileTargetKind::CodexSkill => "codex_skill",
        metactl::CompileTargetKind::PackResource => "pack_resource",
        metactl::CompileTargetKind::HookConfig => "hook_config",
        metactl::CompileTargetKind::McpConfig => "mcp_config",
        metactl::CompileTargetKind::RuntimeJson => "runtime_json",
        metactl::CompileTargetKind::PackExtensionManifest => "pack_extension_manifest",
        metactl::CompileTargetKind::Other => "other",
    }
}

fn success_json(command: &str, project_root: Option<&Path>, extra: Value) -> Value {
    let mut payload = Map::new();
    payload.insert("ok".to_string(), Value::Bool(true));
    payload.insert("command".to_string(), Value::String(command.to_string()));
    payload.insert(
        "api_version".to_string(),
        Value::String(API_VERSION.to_string()),
    );
    if let Some(project_root) = project_root {
        payload.insert(
            "project_root".to_string(),
            Value::String(project_root.to_string_lossy().to_string()),
        );
    }
    if let Value::Object(extra) = extra {
        payload.extend(extra);
    }
    Value::Object(payload)
}

fn project_human_output(project_root: &Path, body: String) -> String {
    format!("Project: {}\n{body}", project_root.display())
}

fn lines_from_json_items(items: &[serde_json::Value]) -> String {
    items
        .iter()
        .map(|item| format!("- {}", item))
        .collect::<Vec<_>>()
        .join("\n")
}

fn installed_ids(config: Option<&ProjectConfigFile>) -> BTreeSet<String> {
    let Some(config) = config else {
        return BTreeSet::new();
    };
    let mut ids = BTreeSet::new();
    ids.insert(config.role.clone());
    ids.insert(config.policy.clone());
    ids.extend(config.targets.iter().cloned());
    ids.extend(config.packs.iter().cloned());
    ids
}

fn report_to_json(report: &ValidationReport) -> serde_json::Value {
    json!({
        "subject_ref": report.subject_ref,
        "status": report.status,
        "checks": report.checks,
    })
}

fn symlink_forced_off() -> bool {
    std::env::var("METACTL_FORCE_NO_SYMLINK").ok().as_deref() == Some("1")
}

fn relative_to_project(project_root: &Path, path: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn lock_is_stale_checked(
    context: &metactl::project::ProjectContext,
) -> std::result::Result<bool, CliError> {
    metactl::project::lock_is_stale(context).map_err(internal_error)
}

fn stale_lock_error() -> CliError {
    CliError::new(
        EXIT_STALE_LOCK,
        "metactl.lock.json is stale. Re-run `metactl compile --update-lock` before apply or validate.",
    )
}

fn operation_lock_error(error: anyhow::Error) -> CliError {
    let message = error.to_string();
    let mut err = CliError::new(EXIT_STATE, message.clone()).with_details(vec![
        "Next: wait for the active command to finish before retrying.".to_string(),
        "If no metactl process is running, inspect the repo and remove .metactl/state/operation.lock.".to_string(),
    ]);
    if let Some(obj) = err.json.as_object_mut() {
        obj.insert("code".to_string(), json!("operation_lock_active"));
        obj.insert("category".to_string(), json!("project_state"));
        if message.contains("stale metactl operation lock") {
            obj.insert("code".to_string(), json!("operation_lock_stale"));
        }
        obj.insert(
            "next_steps".to_string(),
            json!([
                "wait for the active command to finish",
                "if stale, inspect the repo and remove .metactl/state/operation.lock"
            ]),
        );
    }
    err
}

fn conflict_error(report: &ApplyReport) -> CliError {
    CliError::new(
        EXIT_CONFLICT,
        format!("Apply refused for target {}.", report.target.id),
    )
    .with_details(
        report
            .conflicts
            .iter()
            .map(|item| format!("{}: {}", item.destination_path, item.detail))
            .collect(),
    )
}

fn state_error(error: anyhow::Error) -> CliError {
    let details = error_details(&error);
    if details.is_empty() {
        CliError::new(EXIT_STATE, error.to_string())
    } else {
        CliError::new(EXIT_STATE, error.to_string()).with_details(details)
    }
}

fn internal_error<E>(error: E) -> CliError
where
    E: Into<anyhow::Error>,
{
    let error = error.into();
    CliError::new(EXIT_INTERNAL, error.to_string())
}

fn error_details(error: &anyhow::Error) -> Vec<String> {
    error
        .chain()
        .skip(1)
        .map(|cause| cause.to_string())
        .collect()
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

// --- Input layer provenance ---

fn build_input_layers(context: &metactl::project::ProjectContext) -> Vec<Value> {
    let mut layers = Vec::new();

    // Profile layer
    if let Some(profile) = context.active_profile.as_ref() {
        layers.push(json!({
            "layer": "profile",
            "path": profile.path.to_string_lossy(),
            "digest": profile.digest,
        }));
    }

    // Shared layer (metactl.yaml)
    if context.config_path.exists() {
        let digest = digest_path(&context.config_path).ok();
        layers.push(json!({
            "layer": "shared",
            "path": context.config_path.to_string_lossy(),
            "digest": digest,
        }));
    }

    // Local layer (metactl.local.yaml)
    if let Some(local_path) = context.local_config_path.as_ref() {
        if local_path.exists() {
            let digest = digest_path(local_path).ok();
            layers.push(json!({
                "layer": "local",
                "path": local_path.to_string_lossy(),
                "digest": digest,
            }));
        }
    }

    // Invocation layer (overlay)
    if let Some(overlay_path) = context.overlay_path.as_ref() {
        if overlay_path.exists() {
            let digest = digest_path(overlay_path).ok();
            layers.push(json!({
                "layer": "invocation",
                "path": overlay_path.to_string_lossy(),
                "digest": digest,
            }));
        }
    }

    layers
}

// --- Explanation certificates ---

fn build_explanation_certificates(explain: &ExplainResult) -> Vec<Value> {
    let mut certificates = Vec::new();

    // Certificate for target projection decision
    let target_id = &explain.resolve_graph.selected_target.id;
    let activated_count = explain.resolve_graph.activated_pack_refs.len();
    certificates.push(json!({
        "subject": format!("target-projection:{}", target_id),
        "premises": [
            format!("Target {} was selected from config.", target_id),
            format!("{} pack(s) are activated.", activated_count),
        ],
        "evidence": explain.why_it_is_active.iter().map(|r| {
            format!("{}: {}", r.subject_ref.id, r.reason)
        }).collect::<Vec<_>>(),
        "conclusion": format!(
            "Target {} receives projection from {} activated pack(s).",
            target_id, activated_count
        ),
        "degraded": !explain.resolve_graph.capability_gaps.is_empty(),
    }));

    // Certificate for each suppressed pack
    for suppressed in &explain.what_was_suppressed {
        certificates.push(json!({
            "subject": format!("suppressed:{}", suppressed.subject_ref.id),
            "premises": [
                format!("Pack {} was requested or discovered.", suppressed.subject_ref.id),
                format!("Reason code: {:?}.", suppressed.reason_code),
            ],
            "evidence": [
                suppressed.detail.clone().unwrap_or_else(|| "No additional detail.".to_string()),
            ],
            "conclusion": format!(
                "Pack {} was suppressed due to {:?}.",
                suppressed.subject_ref.id, suppressed.reason_code
            ),
            "degraded": false,
        }));
    }

    certificates
}

// --- Source privacy audit ---

fn cmd_audit(cli: &Cli, args: &AuditArgs) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        Some(AuditCommand::Sources) | None => cmd_audit_sources(cli),
    }
}

fn cmd_audit_sources(cli: &Cli) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let findings = source_audit_findings(&project_root)?;
    let failed = !findings.is_empty();
    let human = if failed {
        let mut lines = vec!["Source audit failed:".to_string()];
        for finding in &findings {
            lines.push(format!(
                "  {}: {}",
                finding["path"].as_str().unwrap_or("?"),
                finding["message"].as_str().unwrap_or("leak risk")
            ));
        }
        lines.join("\n")
    } else {
        "Source audit passed.".to_string()
    };
    let payload = json!({
        "ok": !failed,
        "command": "audit",
        "api_version": API_VERSION,
        "project_root": project_root.to_string_lossy(),
        "action": "sources",
        "subject": "sources",
        "findings": findings,
    });
    if failed {
        Err(CliError {
            code: EXIT_VALIDATION,
            message: "Source audit failed.".to_string(),
            details: source_audit_detail_lines(&findings),
            json: payload,
        })
    } else {
        Ok(CommandOutput {
            human: project_human_output(&project_root, human),
            json: payload,
        })
    }
}

fn source_audit_findings(project_root: &Path) -> std::result::Result<Vec<Value>, CliError> {
    let mut findings = Vec::new();

    for path in git_tracked_private_source_paths(project_root)? {
        findings.push(json!({
            "id": "tracked-private-source-state",
            "severity": "fail",
            "path": path,
            "message": "Private source cache or private source lock is tracked by Git.",
            "remediation": format!(
                "Remove the file from the index, then run `{}`.",
                private_source_ignore_install_command(project_root)
            ),
        }));
    }

    let public_lock_path = project_root.join("metactl.lock.json");
    if public_lock_path.exists() {
        let public_lock = fs::read_to_string(&public_lock_path).map_err(|err| {
            internal_error(anyhow!("read {}: {}", public_lock_path.display(), err))
        })?;
        for forbidden in [
            ".metactl/cache/sources/",
            ".metactl/private/source-lock.json",
        ] {
            if public_lock.contains(forbidden) {
                findings.push(json!({
                    "id": "public-lock-private-path",
                    "severity": "fail",
                    "path": "metactl.lock.json",
                    "message": format!("Public lock contains private path `{forbidden}`."),
                    "remediation": format!(
                        "Use lock_publicity: private, rewrite source locks, then run `{}`.",
                        private_source_ignore_install_command(project_root)
                    ),
                }));
            }
        }
    }

    for path in git_tracked_public_example_leaks(project_root)? {
        findings.push(json!({
            "id": "public-example-personal-workspace",
            "severity": "fail",
            "path": path,
            "message": "Public examples must not reference personal workspace paths or local-only repositories.",
            "remediation": "Use neutral placeholders such as /opt/metactl/example-library or git@example.com:org/example-library.git.",
        }));
    }

    Ok(findings)
}

fn source_audit_detail_lines(findings: &[Value]) -> Vec<String> {
    findings
        .iter()
        .flat_map(|finding| {
            let path = finding["path"].as_str().unwrap_or("?");
            let message = finding["message"].as_str().unwrap_or("leak risk");
            let remediation = finding["remediation"].as_str().unwrap_or("");
            if remediation.is_empty() {
                vec![format!("{path}: {message}")]
            } else {
                vec![format!("{path}: {message}"), format!("Next: {remediation}")]
            }
        })
        .collect()
}

fn source_audit_summary_lines(findings: &[Value], limit: usize) -> Vec<String> {
    let mut lines = findings
        .iter()
        .take(limit)
        .map(|finding| {
            let path = finding["path"].as_str().unwrap_or("?");
            let message = finding["message"]
                .as_str()
                .unwrap_or("source audit finding");
            format!("finding: {path}: {message}")
        })
        .collect::<Vec<_>>();
    if findings.len() > limit {
        lines.push(format!("... {} more finding(s)", findings.len() - limit));
    }
    lines
}

fn private_source_ignore_install_command(project_root: &Path) -> String {
    if project_root.join(".git").exists() {
        "metactl ignore install --scope local --include-private-sources".to_string()
    } else {
        "metactl ignore install --scope repo --include-private-sources".to_string()
    }
}

fn nearest_pack_suggestions(missing: &[String], available: &[String], limit: usize) -> Vec<String> {
    let mut scored = Vec::new();
    for requested in missing {
        let requested_lower = requested.to_ascii_lowercase();
        for candidate in available {
            let candidate_lower = candidate.to_ascii_lowercase();
            let score = if candidate_lower == requested_lower {
                0
            } else if candidate_lower.contains(&requested_lower) {
                1
            } else if requested_lower.contains(&candidate_lower) {
                2
            } else {
                3 + bounded_edit_distance(&requested_lower, &candidate_lower)
            };
            if score <= 10 {
                scored.push((score, candidate.clone()));
            }
        }
    }
    scored.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let mut seen = BTreeSet::new();
    scored
        .into_iter()
        .filter_map(|(_, candidate)| {
            if seen.insert(candidate.clone()) {
                Some(candidate)
            } else {
                None
            }
        })
        .take(limit)
        .collect()
}

fn bounded_edit_distance(left: &str, right: &str) -> usize {
    let left_chars = left.chars().collect::<Vec<_>>();
    let right_chars = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right_chars.len()).collect::<Vec<_>>();
    for (i, left_char) in left_chars.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, right_char) in right_chars.iter().enumerate() {
            let substitution = previous[j] + usize::from(left_char != right_char);
            let insertion = current[j] + 1;
            let deletion = previous[j + 1] + 1;
            current.push(substitution.min(insertion).min(deletion));
        }
        previous = current;
    }
    previous[right_chars.len()]
}

fn git_tracked_private_source_paths(
    project_root: &Path,
) -> std::result::Result<Vec<String>, CliError> {
    if !project_root.join(".git").exists() {
        return Ok(Vec::new());
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args([
            "ls-files",
            ".metactl/cache/sources",
            ".metactl/private/source-lock.json",
        ])
        .output()
        .map_err(|err| internal_error(anyhow!("run git ls-files: {}", err)))?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect())
}

fn git_tracked_public_example_leaks(
    project_root: &Path,
) -> std::result::Result<Vec<String>, CliError> {
    if !project_root.join(".git").exists() {
        return Ok(Vec::new());
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .arg("ls-files")
        .output()
        .map_err(|err| internal_error(anyhow!("run git ls-files: {}", err)))?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let mut leaks = Vec::new();
    for path in String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|path| public_example_scan_candidate(path))
    {
        let full_path = project_root.join(path);
        let Ok(contents) = fs::read_to_string(&full_path) else {
            continue;
        };
        if contents.contains("/Users/") || contents.contains("git@github.com:private/") {
            leaks.push(path.to_string());
        }
    }
    Ok(leaks)
}

fn public_example_scan_candidate(path: &str) -> bool {
    if path.starts_with("docs/status/")
        || path.starts_with("docs/evidence/")
        || path.starts_with(".metactl/")
        || path.starts_with(".codex/")
        || path.starts_with(".claude/")
        || path.starts_with(".cursor/")
        || path.starts_with(".gemini/")
    {
        return false;
    }
    path == "README.md"
        || path.starts_with("docs/user/")
        || path.starts_with("docs/adr/")
        || path.starts_with("examples/")
        || path.starts_with("contracts/")
        || path.starts_with("scripts/")
}

// --- Ignore posture management ---

const IGNORE_TARGETS: &[&str] = &["codex-cli", "claude-code", "cursor", "gemini-cli"];
const IGNORE_BLOCK_BEGIN: &str = "# metactl:begin generated-agent-surfaces";
const IGNORE_BLOCK_END: &str = "# metactl:end generated-agent-surfaces";
const AGENT_ALLOWLIST_BEGIN: &str = "# metactl:begin agent-surface-allowlist";
const AGENT_ALLOWLIST_END: &str = "# metactl:end agent-surface-allowlist";

#[derive(Debug, Clone)]
struct IgnoreTargetResolution {
    targets: Vec<String>,
    source: &'static str,
}

fn cmd_ignore(cli: &Cli, args: &IgnoreArgs) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        Some(IgnoreCommand::Status(status_args)) => cmd_ignore_status(cli, status_args),
        Some(IgnoreCommand::Install(install_args)) => cmd_ignore_install(cli, install_args),
        Some(IgnoreCommand::Fix(fix_args)) => cmd_ignore_fix(cli, fix_args),
        None => cmd_ignore_status(
            cli,
            &IgnoreStatusArgs {
                target: Vec::new(),
                scope: IgnoreScopeArg::Both,
            },
        ),
    }
}

fn cmd_ignore_status(
    cli: &Cli,
    args: &IgnoreStatusArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let resolution = resolve_ignore_targets(&project_root, &args.target).map_err(state_error)?;
    let targets = resolution.targets;
    let files = ignore_status_files(&project_root);
    let repo_ignore_file = project_root.join(".gitignore");
    let private_sources = private_source_ignore_status(&project_root);
    let tracked_generated_roots =
        tracked_generated_roots_json(&project_root, &targets).map_err(state_error)?;
    let fix_available = files
        .iter()
        .any(|item| !item["installed"].as_bool().unwrap_or(false))
        || !tracked_generated_roots.is_empty();
    let next_commands = ignore_next_commands(
        args.scope,
        &targets,
        args.target.is_empty(),
        fix_available,
        !tracked_generated_roots.is_empty(),
    );

    let mut lines = vec!["Ignore posture:".to_string()];
    lines.push(format!("  target-source     {}", resolution.source));
    for item in &files {
        let label = item["label"].as_str().unwrap_or("?");
        let path = item["path"].as_str().unwrap_or("?");
        let installed = item["installed"].as_bool().unwrap_or(false);
        let state = if installed {
            "installed"
        } else if item["exists"].as_bool().unwrap_or(false) {
            "not-installed"
        } else {
            "missing"
        };
        lines.push(format!("  {:<18} {} ({})", label, state, path));
    }
    if targets.iter().any(|target| target == "cursor")
        && repo_gitignore_can_hide(
            &repo_ignore_file,
            &[
                ".cursor/",
                "/.cursor/",
                ".codex/",
                "/.codex/",
                ".claude/",
                "/.claude/",
            ],
        )
        && !file_contains_marker(&project_root.join(".cursorignore"), AGENT_ALLOWLIST_BEGIN)
    {
        lines.push(
            "  warning: repo-scoped Git ignores can hide Cursor skills unless .cursorignore has the metactl allowlist."
                .to_string(),
        );
    }
    if targets.iter().any(|target| target == "gemini-cli")
        && repo_gitignore_can_hide(&repo_ignore_file, &[".gemini/", "/.gemini/"])
        && !file_contains_marker(&project_root.join(".geminiignore"), AGENT_ALLOWLIST_BEGIN)
    {
        lines.push(
            "  warning: repo-scoped Git ignores can hide Gemini extension files unless .geminiignore has the metactl allowlist."
                .to_string(),
        );
    }
    let private_label = if private_sources["protected"].as_bool().unwrap_or(false) {
        "protected"
    } else {
        "not-protected"
    };
    lines.push(format!("  private-sources   {private_label}"));
    if private_label == "not-protected" {
        lines.push(format!(
            "  next: {}",
            private_source_ignore_install_command(&project_root)
        ));
    }
    if !tracked_generated_roots.is_empty() {
        lines.push("  tracked generated roots:".to_string());
        for item in &tracked_generated_roots {
            let root = item["root"].as_str().unwrap_or("?");
            let count = item["file_count"].as_u64().unwrap_or(0);
            lines.push(format!("    {root} ({count} tracked file(s))"));
        }
        lines.push("  next: metactl ignore fix --plan".to_string());
    } else if fix_available {
        lines.push("  next: metactl ignore fix --plan".to_string());
    }

    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "ignore",
            Some(&project_root),
            json!({
                "action": "status",
                "targets": targets,
                "target_source": resolution.source,
                "scope": ignore_scope_label(args.scope),
                "files": files,
                "private_sources": private_sources,
                "tracked_generated_roots": tracked_generated_roots,
                "fix_available": fix_available,
                "next_commands": next_commands,
            }),
        ),
    })
}

fn cmd_ignore_install(
    cli: &Cli,
    args: &IgnoreInstallArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let resolution = resolve_ignore_targets(&project_root, &args.target).map_err(state_error)?;
    let targets = resolution.targets;
    let changes = apply_ignore_scope(
        &project_root,
        &targets,
        args.scope,
        args.include_lock,
        args.include_private_sources,
    )?;

    let mut lines = vec![format!(
        "Installed ignore posture ({}) for target(s): {}",
        ignore_scope_label(args.scope),
        targets.join(", ")
    )];
    for change in &changes {
        let status = change["status"].as_str().unwrap_or("unknown");
        let path = change["path"].as_str().unwrap_or("?");
        lines.push(format!("  {} {}", status, path));
    }
    if matches!(args.scope, IgnoreScopeArg::Local) {
        lines.push(
            "  Note: local scope protects this checkout only. Use `metactl ignore install --scope repo` for a shared team posture."
                .to_string(),
        );
    } else {
        lines.push(
            "  Note: repo scope hides generated files from Git and adds agent allowlists for tools that inherit Git ignore behavior."
                .to_string(),
        );
    }

    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "ignore",
            Some(&project_root),
            json!({
                "action": "install",
                "scope": ignore_scope_label(args.scope),
                "targets": targets,
                "target_source": resolution.source,
                "include_lock": args.include_lock,
                "include_private_sources": args.include_private_sources,
                "changes": changes,
            }),
        ),
    })
}

fn cmd_ignore_fix(cli: &Cli, args: &IgnoreFixArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let resolution = resolve_ignore_targets(&project_root, &args.target).map_err(state_error)?;
    let targets = resolution.targets;
    let tracked_generated_roots =
        tracked_generated_roots_json(&project_root, &targets).map_err(state_error)?;
    let root_paths = tracked_generated_roots
        .iter()
        .filter_map(|item| item["root"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    let actions = planned_ignore_actions(
        &project_root,
        &targets,
        args.scope,
        args.include_lock,
        args.include_private_sources,
        &root_paths,
    )?;
    let next_commands = ignore_fix_next_commands(args, &targets, !root_paths.is_empty());

    if args.plan {
        let mut lines = vec!["Ignore repair plan:".to_string()];
        for action in &actions {
            let kind = action["kind"].as_str().unwrap_or("action");
            let summary = action["summary"].as_str().unwrap_or("");
            lines.push(format!("  - {kind}: {summary}"));
        }
        if !root_paths.is_empty() {
            lines.push(
                "  untrack safety: Git index only; generated files remain on disk.".to_string(),
            );
        }
        lines.push("Next commands:".to_string());
        for command in &next_commands {
            lines.push(format!("  {command}"));
        }
        return Ok(CommandOutput {
            human: project_human_output(&project_root, lines.join("\n")),
            json: success_json(
                "ignore",
                Some(&project_root),
                json!({
                    "action": "fix",
                    "plan": true,
                    "scope": ignore_scope_label(args.scope),
                    "targets": targets,
                    "target_source": resolution.source,
                    "actions": actions,
                    "tracked_generated_roots": tracked_generated_roots,
                    "next_commands": next_commands,
                }),
            ),
        });
    }

    if !root_paths.is_empty() && !args.untrack_generated {
        let mut err = CliError::new(
            EXIT_STATE,
            "Generated roots are tracked; untracking requires explicit --untrack-generated.",
        )
        .with_details(next_commands.clone());
        if let Some(obj) = err.json.as_object_mut() {
            obj.insert("code".to_string(), json!("untrack_generated_required"));
            obj.insert("category".to_string(), json!("project_state"));
            obj.insert("next_commands".to_string(), json!(next_commands));
            obj.insert(
                "tracked_generated_roots".to_string(),
                json!(tracked_generated_roots),
            );
        }
        return Err(err);
    }
    if args.untrack_generated
        && !root_paths.is_empty()
        && !args.yes
        && (cli.no_input_enabled() || !io::stdin().is_terminal())
    {
        let mut err = CliError::new(
            EXIT_STATE,
            "Non-interactive generated-root untracking requires --untrack-generated --yes.",
        )
        .with_details(next_commands.clone());
        if let Some(obj) = err.json.as_object_mut() {
            obj.insert("code".to_string(), json!("untrack_confirmation_required"));
            obj.insert("category".to_string(), json!("project_state"));
            obj.insert("next_commands".to_string(), json!(next_commands));
        }
        return Err(err);
    }
    if args.untrack_generated
        && !root_paths.is_empty()
        && !args.yes
        && !confirm_untrack_generated_roots(&root_paths)?
    {
        return Err(
            CliError::new(EXIT_STATE, "Generated-root untracking was not confirmed.")
                .with_details(next_commands),
        );
    }

    let mut changes = apply_ignore_scope(
        &project_root,
        &targets,
        args.scope,
        args.include_lock,
        args.include_private_sources,
    )?;
    let untracked = if args.untrack_generated && !root_paths.is_empty() {
        untrack_generated_roots(&project_root, &root_paths)?
    } else {
        Vec::new()
    };
    for item in &untracked {
        changes.push(item.clone());
    }

    let mut lines = vec![format!(
        "Applied ignore repair ({}) for target(s): {}",
        ignore_scope_label(args.scope),
        targets.join(", ")
    )];
    for change in &changes {
        let status = change["status"].as_str().unwrap_or("unknown");
        let path = change["path"].as_str().unwrap_or("?");
        lines.push(format!("  {status} {path}"));
    }
    if !untracked.is_empty() {
        lines.push(
            "  Note: generated roots were removed from the Git index only; files remain on disk."
                .to_string(),
        );
    }

    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "ignore",
            Some(&project_root),
            json!({
                "action": "fix",
                "plan": false,
                "scope": ignore_scope_label(args.scope),
                "targets": targets,
                "target_source": resolution.source,
                "include_lock": args.include_lock,
                "include_private_sources": args.include_private_sources,
                "changes": changes,
                "untracked_generated_roots": untracked,
            }),
        ),
    })
}

fn apply_ignore_scope(
    project_root: &Path,
    targets: &[String],
    scope: IgnoreScopeArg,
    include_lock: bool,
    include_private_sources: bool,
) -> std::result::Result<Vec<Value>, CliError> {
    let mut changes = Vec::new();
    let patterns = git_ignore_patterns(targets, include_lock, include_private_sources);
    if matches!(scope, IgnoreScopeArg::Local | IgnoreScopeArg::Both) {
        let git_dir = project_root.join(".git");
        if !git_dir.exists() {
            return Err(CliError::new(
                EXIT_STATE,
                "No .git directory found. Local ignore scope writes .git/info/exclude.",
            ));
        }
        changes.push(write_marked_block(
            project_root,
            &git_dir.join("info").join("exclude"),
            IGNORE_BLOCK_BEGIN,
            IGNORE_BLOCK_END,
            &patterns,
        )?);
    }
    if matches!(scope, IgnoreScopeArg::Repo | IgnoreScopeArg::Both) {
        changes.push(write_marked_block(
            project_root,
            &project_root.join(".gitignore"),
            IGNORE_BLOCK_BEGIN,
            IGNORE_BLOCK_END,
            &patterns,
        )?);
        if targets.iter().any(|target| target == "cursor") {
            changes.push(write_marked_block(
                project_root,
                &project_root.join(".cursorignore"),
                AGENT_ALLOWLIST_BEGIN,
                AGENT_ALLOWLIST_END,
                &cursor_allowlist_patterns(targets),
            )?);
        }
        if targets.iter().any(|target| target == "gemini-cli") {
            changes.push(write_marked_block(
                project_root,
                &project_root.join(".geminiignore"),
                AGENT_ALLOWLIST_BEGIN,
                AGENT_ALLOWLIST_END,
                &gemini_allowlist_patterns(),
            )?);
        }
    }
    Ok(changes)
}

fn planned_ignore_actions(
    project_root: &Path,
    targets: &[String],
    scope: IgnoreScopeArg,
    include_lock: bool,
    include_private_sources: bool,
    root_paths: &[String],
) -> std::result::Result<Vec<Value>, CliError> {
    let mut actions = Vec::new();
    let patterns = git_ignore_patterns(targets, include_lock, include_private_sources);
    if matches!(scope, IgnoreScopeArg::Local | IgnoreScopeArg::Both) {
        actions.push(plan_marked_block(
            project_root,
            &project_root.join(".git/info/exclude"),
            IGNORE_BLOCK_BEGIN,
            IGNORE_BLOCK_END,
            &patterns,
            "write local Git exclude ignore block",
        )?);
    }
    if matches!(scope, IgnoreScopeArg::Repo | IgnoreScopeArg::Both) {
        actions.push(plan_marked_block(
            project_root,
            &project_root.join(".gitignore"),
            IGNORE_BLOCK_BEGIN,
            IGNORE_BLOCK_END,
            &patterns,
            "write repo .gitignore ignore block",
        )?);
        if targets.iter().any(|target| target == "cursor") {
            actions.push(plan_marked_block(
                project_root,
                &project_root.join(".cursorignore"),
                AGENT_ALLOWLIST_BEGIN,
                AGENT_ALLOWLIST_END,
                &cursor_allowlist_patterns(targets),
                "write Cursor allowlist for generated agent surfaces",
            )?);
        }
        if targets.iter().any(|target| target == "gemini-cli") {
            actions.push(plan_marked_block(
                project_root,
                &project_root.join(".geminiignore"),
                AGENT_ALLOWLIST_BEGIN,
                AGENT_ALLOWLIST_END,
                &gemini_allowlist_patterns(),
                "write Gemini allowlist for generated agent surfaces",
            )?);
        }
    }
    if !root_paths.is_empty() {
        actions.push(json!({
            "kind": "untrack-generated",
            "status": "planned",
            "roots": root_paths,
            "summary": "remove generated roots from Git index only; files remain on disk",
            "command": format!("git rm -r --cached --ignore-unmatch -- {}", root_paths.join(" ")),
        }));
    }
    Ok(actions)
}

fn plan_marked_block(
    project_root: &Path,
    path: &Path,
    begin_marker: &str,
    end_marker: &str,
    block_lines: &[String],
    summary: &str,
) -> std::result::Result<Value, CliError> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let updated = upsert_marked_block(&existing, begin_marker, end_marker, block_lines)
        .map_err(|err| state_error(anyhow::anyhow!("{}: {}", path.display(), err)))?;
    let status = if !path.exists() {
        "would-create"
    } else if existing == updated {
        "unchanged"
    } else {
        "would-update"
    };
    Ok(json!({
        "kind": "write-ignore",
        "path": relative_to_project(project_root, path),
        "status": status,
        "summary": summary,
    }))
}

fn tracked_generated_roots_json(project_root: &Path, targets: &[String]) -> Result<Vec<Value>> {
    let roots = generated_roots_for_targets(targets);
    if roots.is_empty() || !project_root.join(".git").exists() {
        return Ok(Vec::new());
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .arg("ls-files")
        .arg("-z")
        .arg("--")
        .args(&roots)
        .output()
        .with_context(|| format!("run git ls-files in {}", project_root.display()))?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    let files = raw
        .split('\0')
        .filter(|item| !item.is_empty())
        .map(|item| item.replace('\\', "/"))
        .collect::<Vec<_>>();
    let mut by_root = BTreeMap::<String, Vec<String>>::new();
    for file in files {
        for root in &roots {
            if file == *root || file.starts_with(&format!("{root}/")) {
                by_root.entry(root.clone()).or_default().push(file.clone());
            }
        }
    }
    Ok(by_root
        .into_iter()
        .map(|(root, files)| {
            json!({
                "root": root,
                "classification": "generated-root",
                "file_count": files.len(),
                "tracked_files": files,
            })
        })
        .collect())
}

fn generated_roots_for_targets(targets: &[String]) -> Vec<String> {
    let mut roots = BTreeSet::new();
    for target in targets {
        match target.as_str() {
            "codex-cli" => {
                roots.insert(".codex".to_string());
            }
            "claude-code" => {
                roots.insert(".claude".to_string());
            }
            "cursor" => {
                roots.insert(".cursor".to_string());
            }
            "gemini-cli" => {
                roots.insert(".gemini".to_string());
            }
            _ => {}
        }
    }
    roots.into_iter().collect()
}

fn untrack_generated_roots(
    project_root: &Path,
    root_paths: &[String],
) -> std::result::Result<Vec<Value>, CliError> {
    if root_paths.is_empty() {
        return Ok(Vec::new());
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .arg("rm")
        .arg("-r")
        .arg("--cached")
        .arg("--ignore-unmatch")
        .arg("--")
        .args(root_paths)
        .output()
        .with_context(|| format!("run git rm --cached in {}", project_root.display()))
        .map_err(internal_error)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(CliError::new(
            EXIT_STATE,
            format!("Failed to untrack generated roots: {stderr}"),
        ));
    }
    Ok(root_paths
        .iter()
        .map(|root| {
            json!({
                "kind": "untrack-generated",
                "path": root,
                "status": "untracked-index",
                "files_remain_on_disk": project_root.join(root).exists(),
            })
        })
        .collect())
}

fn confirm_untrack_generated_roots(root_paths: &[String]) -> std::result::Result<bool, CliError> {
    eprint!(
        "Remove generated roots from the Git index only (files remain on disk): {}? [y/N] ",
        root_paths.join(", ")
    );
    io::stderr().flush().map_err(internal_error)?;
    let mut input = String::new();
    io::stdin().read_line(&mut input).map_err(internal_error)?;
    Ok(matches!(input.trim(), "y" | "Y" | "yes" | "YES"))
}

fn ignore_next_commands(
    scope: IgnoreScopeArg,
    targets: &[String],
    inferred_targets: bool,
    fix_available: bool,
    needs_untrack: bool,
) -> Vec<String> {
    if !fix_available {
        return Vec::new();
    }
    let mut base = format!(
        "metactl ignore fix --plan --scope {}",
        ignore_scope_label(scope)
    );
    if !inferred_targets && !targets.is_empty() {
        base.push_str(&repeated_target_args(targets));
    }
    let mut commands = vec![base];
    if needs_untrack {
        let mut apply = format!(
            "metactl ignore fix --scope {} --untrack-generated --yes",
            ignore_scope_label(scope)
        );
        if !inferred_targets && !targets.is_empty() {
            apply.push_str(&repeated_target_args(targets));
        }
        commands.push(apply);
    }
    commands
}

fn ignore_fix_next_commands(
    args: &IgnoreFixArgs,
    targets: &[String],
    needs_untrack: bool,
) -> Vec<String> {
    let target_arg = if targets.is_empty() {
        String::new()
    } else {
        repeated_target_args(targets)
    };
    let mut plan = format!(
        "metactl ignore fix --plan --scope {}{}",
        ignore_scope_label(args.scope),
        target_arg
    );
    if args.include_private_sources {
        plan.push_str(" --include-private-sources");
    }
    if args.include_lock {
        plan.push_str(" --include-lock");
    }
    let mut apply = format!(
        "metactl ignore fix --scope {}{}",
        ignore_scope_label(args.scope),
        target_arg
    );
    if args.include_private_sources {
        apply.push_str(" --include-private-sources");
    }
    if args.include_lock {
        apply.push_str(" --include-lock");
    }
    if needs_untrack {
        apply.push_str(" --untrack-generated");
    }
    apply.push_str(" --yes");
    vec![plan, apply]
}

fn repeated_target_args(targets: &[String]) -> String {
    targets
        .iter()
        .map(|target| format!(" --target {target}"))
        .collect::<Vec<_>>()
        .join("")
}

fn ignore_status_files(project_root: &Path) -> Vec<Value> {
    vec![
        ignore_status_file(
            "local-git-exclude",
            &project_root.join(".git/info/exclude"),
            IGNORE_BLOCK_BEGIN,
            project_root,
        ),
        ignore_status_file(
            "repo-gitignore",
            &project_root.join(".gitignore"),
            IGNORE_BLOCK_BEGIN,
            project_root,
        ),
        ignore_status_file(
            "cursor-allowlist",
            &project_root.join(".cursorignore"),
            AGENT_ALLOWLIST_BEGIN,
            project_root,
        ),
        ignore_status_file(
            "gemini-allowlist",
            &project_root.join(".geminiignore"),
            AGENT_ALLOWLIST_BEGIN,
            project_root,
        ),
    ]
}

fn ignore_status_file(label: &str, path: &Path, marker: &str, project_root: &Path) -> Value {
    json!({
        "label": label,
        "path": relative_to_project(project_root, path),
        "exists": path.exists(),
        "installed": file_contains_marker(path, marker),
    })
}

fn private_source_ignore_status(project_root: &Path) -> Value {
    let candidates = [
        project_root.join(".git/info/exclude"),
        project_root.join(".gitignore"),
    ];
    let cache_protected = candidates.iter().any(|path| {
        ignore_file_contains_pattern(path, ".metactl/cache/sources/")
            || ignore_file_contains_pattern(path, "/.metactl/cache/sources/")
    });
    let private_lock_protected = candidates.iter().any(|path| {
        ignore_file_contains_pattern(path, ".metactl/private/source-lock.json")
            || ignore_file_contains_pattern(path, "/.metactl/private/source-lock.json")
            || ignore_file_contains_pattern(path, ".metactl/private/")
            || ignore_file_contains_pattern(path, "/.metactl/private/")
            || ignore_file_contains_pattern(path, ".metactl/")
            || ignore_file_contains_pattern(path, "/.metactl/")
    });
    json!({
        "cache_protected": cache_protected,
        "private_lock_protected": private_lock_protected,
        "protected": cache_protected && private_lock_protected,
    })
}

fn ignore_file_contains_pattern(path: &Path, pattern: &str) -> bool {
    fs::read_to_string(path)
        .map(|contents| contents.lines().map(str::trim).any(|line| line == pattern))
        .unwrap_or(false)
}

fn file_contains_marker(path: &Path, marker: &str) -> bool {
    fs::read_to_string(path)
        .map(|contents| contents.lines().any(|line| line.trim() == marker))
        .unwrap_or(false)
}

fn repo_gitignore_can_hide(path: &Path, patterns: &[&str]) -> bool {
    fs::read_to_string(path)
        .map(|contents| {
            contents
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .any(|line| patterns.iter().any(|pattern| line == *pattern))
        })
        .unwrap_or(false)
}

fn resolve_ignore_targets(
    project_root: &Path,
    requested: &[String],
) -> Result<IgnoreTargetResolution> {
    let (raw_targets, source) = if requested.is_empty() {
        let config_path = project_root.join("metactl.yaml");
        if config_path.exists() {
            let config = load_partial_project_config(&config_path)?;
            if config.targets.is_empty() {
                (default_project_config().targets, "default")
            } else {
                (config.targets, "config")
            }
        } else {
            (
                IGNORE_TARGETS.iter().map(|item| item.to_string()).collect(),
                "detected-default",
            )
        }
    } else {
        (requested.to_vec(), "explicit")
    };

    let mut targets = BTreeSet::new();
    for target in raw_targets {
        if target == "all" {
            targets.extend(IGNORE_TARGETS.iter().map(|item| item.to_string()));
        } else if IGNORE_TARGETS.contains(&target.as_str()) {
            targets.insert(target);
        } else {
            return Err(anyhow!(
                "target {} does not have generated-agent ignore rules; supported targets: {}",
                target,
                IGNORE_TARGETS.join(", ")
            ));
        }
    }
    Ok(IgnoreTargetResolution {
        targets: targets.into_iter().collect(),
        source,
    })
}

fn git_ignore_patterns(
    targets: &[String],
    include_lock: bool,
    include_private_sources: bool,
) -> Vec<String> {
    let mut patterns = BTreeSet::from([
        "# metactl generated state and local-only config".to_string(),
        ".metactl/".to_string(),
        "metactl.local.yaml".to_string(),
    ]);

    if targets.iter().any(|target| target == "codex-cli") {
        patterns.insert(".codex/".to_string());
    }
    if targets.iter().any(|target| target == "claude-code") {
        patterns.insert(".claude/".to_string());
        patterns.insert("CLAUDE.local.md".to_string());
    }
    if targets.iter().any(|target| target == "cursor") {
        patterns.insert(".cursor/".to_string());
    }
    if targets.iter().any(|target| target == "gemini-cli") {
        patterns.insert(".gemini/".to_string());
        patterns.insert("GEMINI.local.md".to_string());
    }
    if include_lock {
        patterns.insert("metactl.lock.json".to_string());
    }
    if include_private_sources {
        patterns.insert(".metactl/cache/sources/".to_string());
        patterns.insert(".metactl/private/source-lock.json".to_string());
    }
    patterns.into_iter().collect()
}

fn cursor_allowlist_patterns(targets: &[String]) -> Vec<String> {
    let mut patterns = vec![
        "# Keep metactl-generated agent surfaces visible to Cursor even when Git ignores them."
            .to_string(),
    ];
    if targets.iter().any(|target| target == "codex-cli") {
        patterns.extend(
            [
                "!/AGENTS.md",
                "!/.codex/",
                "!/.codex/skills/",
                "!/.codex/skills/**",
            ]
            .iter()
            .map(|item| item.to_string()),
        );
    }
    if targets.iter().any(|target| target == "claude-code") {
        patterns.extend(
            [
                "!/CLAUDE.md",
                "!/CLAUDE.local.md",
                "!/.claude/",
                "!/.claude/settings.json",
                "!/.claude/agents/",
                "!/.claude/agents/**",
                "!/.claude/commands/",
                "!/.claude/commands/**",
                "!/.claude/skills/",
                "!/.claude/skills/**",
            ]
            .iter()
            .map(|item| item.to_string()),
        );
    }
    if targets.iter().any(|target| target == "cursor") {
        patterns.extend(
            [
                "!/.cursor/",
                "!/.cursor/mcp.json",
                "!/.cursor/rules/",
                "!/.cursor/rules/**",
                "!/.cursor/skills/",
                "!/.cursor/skills/**",
                "!/.cursor/commands/",
                "!/.cursor/commands/**",
            ]
            .iter()
            .map(|item| item.to_string()),
        );
    }
    if targets.iter().any(|target| target == "gemini-cli") {
        patterns.extend(
            [
                "!/GEMINI.md",
                "!/GEMINI.local.md",
                "!/.gemini/",
                "!/.gemini/extensions/",
                "!/.gemini/extensions/**",
            ]
            .iter()
            .map(|item| item.to_string()),
        );
    }
    patterns
}

fn gemini_allowlist_patterns() -> Vec<String> {
    [
        "# Keep metactl-generated Gemini extensions visible even when Git ignores them.",
        "!/GEMINI.md",
        "!/GEMINI.local.md",
        "!/.gemini/",
        "!/.gemini/extensions/",
        "!/.gemini/extensions/**",
    ]
    .iter()
    .map(|item| item.to_string())
    .collect()
}

fn write_marked_block(
    project_root: &Path,
    path: &Path,
    begin_marker: &str,
    end_marker: &str,
    block_lines: &[String],
) -> std::result::Result<Value, CliError> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let updated = upsert_marked_block(&existing, begin_marker, end_marker, block_lines)
        .map_err(|err| state_error(anyhow::anyhow!("{}: {}", path.display(), err)))?;
    let status = if !path.exists() {
        "created"
    } else if existing == updated {
        "unchanged"
    } else {
        "updated"
    };
    if existing != updated {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                internal_error(anyhow::anyhow!("create {}: {}", parent.display(), err))
            })?;
        }
        atomic_write(path, updated.as_bytes())
            .map_err(|err| internal_error(anyhow::anyhow!("write {}: {}", path.display(), err)))?;
    }
    Ok(json!({
        "path": relative_to_project(project_root, path),
        "status": status,
    }))
}

fn upsert_marked_block(
    existing: &str,
    begin_marker: &str,
    end_marker: &str,
    block_lines: &[String],
) -> Result<String> {
    let begin_count = existing
        .lines()
        .filter(|line| line.trim() == begin_marker)
        .count();
    let end_count = existing
        .lines()
        .filter(|line| line.trim() == end_marker)
        .count();
    if begin_count != end_count {
        return Err(anyhow!(
            "malformed managed ignore block: expected matching begin/end markers"
        ));
    }
    if begin_count > 1 {
        return Err(anyhow!(
            "malformed managed ignore block: expected at most one managed block"
        ));
    }

    let mut output = Vec::new();
    let mut skipping = false;
    let mut found = false;

    for line in existing.lines() {
        if line.trim() == begin_marker {
            found = true;
            skipping = true;
            append_marked_block(&mut output, begin_marker, end_marker, block_lines);
            continue;
        }
        if skipping {
            if line.trim() == end_marker {
                skipping = false;
            }
            continue;
        }
        output.push(line.to_string());
    }

    if !found {
        if !output.is_empty() && output.last().map(|line| !line.is_empty()).unwrap_or(false) {
            output.push(String::new());
        }
        append_marked_block(&mut output, begin_marker, end_marker, block_lines);
    }

    let mut updated = output.join("\n");
    updated.push('\n');
    Ok(updated)
}

fn append_marked_block(
    output: &mut Vec<String>,
    begin_marker: &str,
    end_marker: &str,
    block_lines: &[String],
) {
    output.push(begin_marker.to_string());
    output.extend(block_lines.iter().cloned());
    output.push(end_marker.to_string());
}

fn ignore_scope_label(scope: IgnoreScopeArg) -> &'static str {
    match scope {
        IgnoreScopeArg::Local => "local",
        IgnoreScopeArg::Repo => "repo",
        IgnoreScopeArg::Both => "both",
    }
}

// --- Hook management ---

fn cmd_hook(cli: &Cli, args: &HookArgs) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        HookCommand::Install(install_args) => cmd_hook_install(cli, install_args),
        HookCommand::Status => cmd_hook_status(cli),
    }
}

fn cmd_hook_install(
    cli: &Cli,
    args: &HookInstallArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let git_hooks_dir = project_root.join(".git").join("hooks");
    if !git_hooks_dir.parent().map(|p| p.exists()).unwrap_or(false) {
        return Err(CliError::new(
            EXIT_STATE,
            "No .git directory found. This command requires a git repository.",
        ));
    }
    fs::create_dir_all(&git_hooks_dir)
        .map_err(|e| internal_error(anyhow::anyhow!("create .git/hooks: {}", e)))?;

    let hook_names: Vec<String> = if args.hooks.is_empty() {
        vec!["post-checkout".to_string(), "post-merge".to_string()]
    } else {
        args.hooks.clone()
    };

    let hook_script = r#"#!/bin/sh
# metactl sync hook — auto-generated by `metactl hook install`
# Runs metactl sync when config files change in a checkout or merge.

# Skip in detached HEAD state (e.g. CI, rebase)
if ! git symbolic-ref -q HEAD >/dev/null 2>&1; then
    exit 0
fi

# Check if metactl is available
if ! command -v metactl >/dev/null 2>&1; then
    echo "[metactl] metactl not found on PATH, skipping sync"
    exit 0
fi

METACTL_FILES="metactl.yaml metactl.local.yaml metactl.lock.json"
CHANGED=0
# HEAD@{1} may not exist on first checkout or shallow clone — skip gracefully
if git rev-parse --verify HEAD@{1} >/dev/null 2>&1; then
    for f in $METACTL_FILES; do
        if git diff HEAD@{1} --name-only 2>/dev/null | grep -q "^${f}$"; then
            CHANGED=1
            break
        fi
    done
fi

if [ "$CHANGED" = "1" ]; then
    echo "[metactl] Config changed, running sync..."
    metactl sync --yes --quiet || true
fi
"#;

    let mut installed = Vec::new();
    let mut skipped = Vec::new();
    for hook_name in &hook_names {
        let hook_path = git_hooks_dir.join(hook_name);
        if hook_path.exists() {
            let existing = fs::read_to_string(&hook_path).unwrap_or_default();
            if existing.contains("metactl") {
                skipped.push(format!("{} (already contains metactl hook)", hook_name));
                continue;
            }
            // Append to existing hook
            let mut combined = existing;
            if !combined.ends_with('\n') {
                combined.push('\n');
            }
            combined.push_str("\n# metactl sync hook — appended by `metactl hook install`\n");
            combined.push_str(&hook_script.lines().skip(1).collect::<Vec<_>>().join("\n"));
            combined.push('\n');
            atomic_write(&hook_path, combined.as_bytes()).map_err(|e| {
                internal_error(anyhow::anyhow!("write {}: {}", hook_path.display(), e))
            })?;
        } else {
            atomic_write(&hook_path, hook_script.as_bytes()).map_err(|e| {
                internal_error(anyhow::anyhow!("write {}: {}", hook_path.display(), e))
            })?;
        }
        // Make executable on unix
        #[cfg(unix)]
        {
            let metadata = fs::metadata(&hook_path).map_err(|e| {
                internal_error(anyhow::anyhow!(
                    "read metadata {}: {}",
                    hook_path.display(),
                    e
                ))
            })?;
            let mut perms = metadata.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&hook_path, perms).map_err(|e| {
                internal_error(anyhow::anyhow!("chmod {}: {}", hook_path.display(), e))
            })?;
        }
        installed.push(hook_name.clone());
    }

    let mut lines = Vec::new();
    if !installed.is_empty() {
        lines.push(format!("Installed hook(s): {}", installed.join(", ")));
    }
    if !skipped.is_empty() {
        lines.push(format!("Skipped: {}", skipped.join(", ")));
    }
    if installed.is_empty() && skipped.is_empty() {
        lines.push("No hooks to install.".to_string());
    }

    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "hook",
            Some(&project_root),
            json!({
                "action": "install",
                "installed": installed,
                "skipped": skipped,
            }),
        ),
    })
}

fn cmd_hook_status(cli: &Cli) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let git_hooks_dir = project_root.join(".git").join("hooks");

    let hook_names = ["post-checkout", "post-merge", "pre-commit", "pre-push"];
    let mut hooks = Vec::new();

    for hook_name in &hook_names {
        let hook_path = git_hooks_dir.join(hook_name);
        let exists = hook_path.exists();
        let has_metactl = if exists {
            fs::read_to_string(&hook_path)
                .map(|content| content.contains("metactl"))
                .unwrap_or(false)
        } else {
            false
        };
        hooks.push(json!({
            "hook": hook_name,
            "exists": exists,
            "has_metactl": has_metactl,
        }));
    }

    let mut lines = vec!["Hook status:".to_string()];
    for hook in &hooks {
        let name = hook["hook"].as_str().unwrap_or("?");
        let exists = hook["exists"].as_bool().unwrap_or(false);
        let has_metactl = hook["has_metactl"].as_bool().unwrap_or(false);
        let status = if has_metactl {
            "metactl"
        } else if exists {
            "exists (no metactl)"
        } else {
            "not installed"
        };
        lines.push(format!("  {:<16} {}", name, status));
    }

    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "hook",
            Some(&project_root),
            json!({
                "action": "status",
                "hooks": hooks,
            }),
        ),
    })
}

fn cmd_profile(cli: &Cli, args: &ProfileArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = cli.project.as_deref();
    match &args.command {
        Some(ProfileCommand::List) => {
            let items = list_user_profiles().map_err(internal_error)?;
            let templates = builtin_profile_template_json();
            let profiles_dir = profiles_directory();
            let mut human = String::from("Profiles:\n");
            if items.is_empty() {
                human.push_str("  (none)\n");
            } else {
                for (name, path) in &items {
                    human.push_str(&format!("  {} — {}\n", name, path.display()));
                }
            }
            human.push_str(&format!(
                "Profiles directory: {}\n",
                profiles_dir
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "(unavailable — set HOME or XDG_CONFIG_HOME)".to_string())
            ));
            human.push_str("Built-in templates:\n");
            for template in &templates {
                human.push_str(&format!(
                    "  {} — {}\n",
                    template["name"].as_str().unwrap_or("?"),
                    template["description"].as_str().unwrap_or("")
                ));
            }
            let json_profiles: Vec<Value> = items
                .iter()
                .map(|(name, path)| {
                    json!({
                        "name": name,
                        "path": path,
                    })
                })
                .collect();
            Ok(CommandOutput {
                human,
                json: success_json(
                    "profile",
                    project_root,
                    json!({
                        "action": "list",
                        "profiles_directory": profiles_dir,
                        "profiles": json_profiles,
                        "templates": templates,
                    }),
                ),
            })
        }
        Some(ProfileCommand::Show) | None => {
            let settings = load_user_settings();
            let path = user_settings_path();
            let human = format!(
                "User settings file: {}\nDefault profile: {}\nProfiles directory: {}\n",
                path.as_ref()
                    .map(|item| item.display().to_string())
                    .unwrap_or_else(|| "(unavailable — set HOME or XDG_CONFIG_HOME)".to_string()),
                settings.default_profile.as_deref().unwrap_or("(none)"),
                profiles_directory()
                    .as_ref()
                    .map(|item| item.display().to_string())
                    .unwrap_or_else(|| "(unavailable — set HOME or XDG_CONFIG_HOME)".to_string()),
            );
            Ok(CommandOutput {
                human,
                json: success_json(
                    "profile",
                    project_root,
                    json!({
                        "action": "show",
                        "settings_path": path,
                        "default_profile": settings.default_profile,
                        "profiles_directory": profiles_directory(),
                        "templates": builtin_profile_template_json(),
                    }),
                ),
            })
        }
        Some(ProfileCommand::SetDefault { name }) => {
            let Some(profile_file) = profile_path(name) else {
                return Err(CliError::new(
                    EXIT_STATE,
                    "HOME (or XDG_CONFIG_HOME) is not set; cannot resolve profile path.",
                ));
            };
            let builtin = builtin_profile_templates()
                .iter()
                .any(|template| template.name == name.as_str());
            if !profile_file.exists() && !builtin {
                return Err(CliError::new(
                    EXIT_STATE,
                    format!(
                        "Profile file not found: {}.\nHint: create {}",
                        profile_file.display(),
                        profile_file.display()
                    ),
                ));
            }
            let mut settings = load_user_settings();
            settings.default_profile = Some(name.clone());
            save_user_settings(&settings).map_err(internal_error)?;
            let human = format!("Default profile set to `{name}`.\n");
            Ok(CommandOutput {
                human,
                json: success_json(
                    "profile",
                    project_root,
                    json!({
                        "action": "set-default",
                        "default_profile": name,
                    }),
                ),
            })
        }
        Some(ProfileCommand::ClearDefault) => {
            let mut settings = load_user_settings();
            settings.default_profile = None;
            save_user_settings(&settings).map_err(internal_error)?;
            Ok(CommandOutput {
                human: "Cleared default profile.\n".to_string(),
                json: success_json(
                    "profile",
                    project_root,
                    json!({
                        "action": "clear-default",
                        "default_profile": Value::Null,
                    }),
                ),
            })
        }
    }
}

// --- Source management ---

fn cmd_source(cli: &Cli, args: &SourceArgs) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        Some(SourceCommand::List) | None => cmd_source_list(cli),
        Some(SourceCommand::Add(add_args)) => cmd_source_add(cli, add_args),
        Some(SourceCommand::Sync(sync_args)) => cmd_source_sync(cli, sync_args),
        Some(SourceCommand::Remove(remove_args)) => cmd_source_remove(cli, remove_args),
    }
}

fn cmd_source_list(cli: &Cli) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_required_context(cli, &project_root)?;

    let mut sources = Vec::new();

    for source in &context.config_file.sources {
        sources.push(source_record_json(source, "config"));
    }

    // Backward-compatible sources from config metadata (metadata.source.*)
    for (key, value) in &context.config_file.metadata {
        if let Some(name) = key.strip_prefix("source.") {
            sources.push(json!({
                "id": name,
                "name": name,
                "type": "local",
                "path": value,
                "origin": "config",
            }));
        }
    }

    // Auto-discovered import roots
    let import_roots = metactl::project::discover_import_roots();
    for root in &import_roots {
        sources.push(json!({
            "name": root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "imports".to_string()),
            "path": root.to_string_lossy(),
            "origin": "auto-discovered",
        }));
    }

    // Starter library roots
    for lib_root in &context.library_roots {
        sources.push(json!({
            "name": lib_root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "library".to_string()),
            "path": lib_root.to_string_lossy(),
            "origin": "starter-library",
        }));
    }

    let mut lines = vec!["Sources:".to_string()];
    if sources.is_empty() {
        lines.push("  (none configured or discovered)".to_string());
    } else {
        for src in &sources {
            let name = src["name"].as_str().unwrap_or("?");
            let path = src["path"].as_str().unwrap_or("?");
            let origin = src["origin"].as_str().unwrap_or("?");
            lines.push(format!("  {:<20} {} ({})", name, path, origin));
        }
    }

    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "source",
            Some(&project_root),
            json!({
                "action": "list",
                "sources": sources,
            }),
        ),
    })
}

fn cmd_source_add(cli: &Cli, args: &SourceAddArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let config_path = project_config_path(&project_root, cli.config.as_deref());
    if !config_path.exists() {
        return Err(missing_config_error(cli, &project_root));
    }

    let mut raw = load_partial_project_config(&config_path).map_err(internal_error)?;
    let (source_name, location, inferred_name) = source_add_name_and_location(args)?;
    validate_source_id(&source_name)?;
    let inferred_type = args
        .source_type
        .map(Into::into)
        .unwrap_or_else(|| infer_source_type(&location));
    if inferred_type == SourceType::Git && args.ref_.is_none() && !args.allow_floating_ref {
        return Err(CliError::new(
            EXIT_STATE,
            "Git sources require --ref unless --allow-floating-ref is passed.",
        ));
    }

    let path = PathBuf::from(&location);
    if inferred_type == SourceType::Local && !path.exists() {
        if cli.no_input_enabled() {
            return Err(CliError::new(
                EXIT_STATE,
                format!("Source path does not exist: {}", location),
            ));
        }
        eprintln!("Warning: source path does not exist: {}", location);
    }

    if raw.sources.iter().any(|source| source.id == source_name) {
        return Ok(CommandOutput {
            human: project_human_output(
                &project_root,
                format!("Source '{}' already configured.", source_name),
            ),
            json: success_json(
                "source",
                Some(&project_root),
                json!({
                    "action": "add",
                    "name": source_name,
                    "source": raw.sources.iter().find(|source| source.id == source_name).map(|source| source_record_json(source, "config")),
                    "already_configured": true,
                    "inferred_name": inferred_name,
                }),
            ),
        });
    }

    let source = SourceRecord {
        id: source_name.clone(),
        source_type: inferred_type.clone(),
        path: (inferred_type == SourceType::Local).then(|| location.clone()),
        url: (inferred_type == SourceType::Git).then(|| location.clone()),
        ref_: args.ref_.clone(),
        visibility: if args.private {
            SourceVisibility::Private
        } else {
            SourceVisibility::Public
        },
        lock_publicity: args.lock_publicity.into(),
    };
    raw.sources.push(source.clone());
    write_partial_project_config(&config_path, &raw).map_err(internal_error)?;

    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            format!("Added source '{}' at {}.", source_name, location),
        ),
        json: success_json(
            "source",
            Some(&project_root),
            json!({
                "action": "add",
                "name": source_name,
                "source": source_record_json(&source, "config"),
                "already_configured": false,
                "inferred_name": inferred_name,
            }),
        ),
    })
}

fn cmd_source_sync(
    cli: &Cli,
    args: &SourceSyncArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_required_context(cli, &project_root)?;
    let Some(name) = args.name.as_deref() else {
        return cmd_source_sync_all(&project_root, &context, args.force);
    };
    let source = find_source_record(&context.config_file, name).ok_or_else(|| {
        let configured = context
            .config_file
            .sources
            .iter()
            .map(|source| source.id.clone())
            .collect::<Vec<_>>();
        let mut details = vec!["Next: metactl source list".to_string()];
        if configured.is_empty() {
            details.push("Next: metactl source add <name> <path-or-git-url> --private".to_string());
        } else {
            details.push(format!("Configured sources: {}", configured.join(", ")));
        }
        CliError::new(EXIT_STATE, format!("Source '{}' is not configured.", name))
            .with_details(details)
    })?;
    let synced = sync_source(&project_root, &source, args.force)?;

    let mut public_lock = context.lock.clone();
    upsert_locked_source(&mut public_lock.sources, redacted_locked_source(&synced));
    write_lock(&context.lock_path, &public_lock).map_err(internal_error)?;

    let private_lock = PrivateSourceLock {
        sources: vec![synced.clone()],
    };
    write_private_source_lock(&private_source_lock_path(&project_root), &private_lock)
        .map_err(internal_error)?;

    Ok(CommandOutput {
        human: project_human_output(&project_root, format!("Synced source '{}'.", source.id)),
        json: success_json(
            "source",
            Some(&project_root),
            json!({
                "action": "sync",
                "source": locked_source_json(&synced, false),
                "sources": [locked_source_json(&synced, false)],
            }),
        ),
    })
}

fn cmd_source_sync_all(
    project_root: &Path,
    context: &metactl::project::ProjectContext,
    force: bool,
) -> std::result::Result<CommandOutput, CliError> {
    if context.config_file.sources.is_empty() {
        return Err(
            CliError::new(EXIT_STATE, "No configured sources to sync.").with_details(vec![
                "Next: metactl source list".to_string(),
                "Next: metactl source add <location> --private".to_string(),
            ]),
        );
    }

    let mut public_lock = context.lock.clone();
    let mut private_sources = Vec::new();
    for source in &context.config_file.sources {
        let synced = sync_source(project_root, source, force)?;
        upsert_locked_source(&mut public_lock.sources, redacted_locked_source(&synced));
        private_sources.push(synced);
    }
    write_lock(&context.lock_path, &public_lock).map_err(internal_error)?;
    write_private_source_lock(
        &private_source_lock_path(project_root),
        &PrivateSourceLock {
            sources: private_sources.clone(),
        },
    )
    .map_err(internal_error)?;

    let names = private_sources
        .iter()
        .map(|source| source.id.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Ok(CommandOutput {
        human: project_human_output(
            project_root,
            format!("Synced {} source(s): {}.", private_sources.len(), names),
        ),
        json: success_json(
            "source",
            Some(project_root),
            json!({
                "action": "sync",
                "sources": private_sources
                    .iter()
                    .map(|source| locked_source_json(source, false))
                    .collect::<Vec<_>>(),
            }),
        ),
    })
}

fn cmd_source_remove(
    cli: &Cli,
    args: &SourceRemoveArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let config_path = project_config_path(&project_root, cli.config.as_deref());
    let mut raw = load_partial_project_config(&config_path).map_err(internal_error)?;
    let before = raw.sources.len();
    raw.sources.retain(|source| source.id != args.name);
    raw.metadata.remove(&format!("source.{}", args.name));
    write_partial_project_config(&config_path, &raw).map_err(internal_error)?;
    Ok(CommandOutput {
        human: project_human_output(
            &project_root,
            if raw.sources.len() == before {
                format!("Source '{}' was not configured.", args.name)
            } else {
                format!("Removed source '{}'.", args.name)
            },
        ),
        json: success_json(
            "source",
            Some(&project_root),
            json!({
                "action": "remove",
                "name": args.name,
                "removed": raw.sources.len() != before,
            }),
        ),
    })
}

impl From<SourceTypeArg> for SourceType {
    fn from(value: SourceTypeArg) -> Self {
        match value {
            SourceTypeArg::Local => SourceType::Local,
            SourceTypeArg::Git => SourceType::Git,
        }
    }
}

impl From<SourceLockPublicityArg> for SourceLockPublicity {
    fn from(value: SourceLockPublicityArg) -> Self {
        match value {
            SourceLockPublicityArg::Public => SourceLockPublicity::Public,
            SourceLockPublicityArg::Private => SourceLockPublicity::Private,
        }
    }
}

fn source_add_name_and_location(
    args: &SourceAddArgs,
) -> std::result::Result<(String, String, bool), CliError> {
    match (&args.name, &args.location) {
        (Some(name), Some(location)) => Ok((name.clone(), location.clone(), false)),
        (Some(location), None) => Ok((infer_source_id(location)?, location.clone(), true)),
        _ => Err(CliError::new(
            EXIT_STATE,
            "Source add requires a location. Usage: metactl source add <location> or metactl source add <name> <location>",
        )
        .with_details(vec![
            "Next: metactl source add ./path/to/library --private".to_string(),
            "Next: metactl source add team-library ./path/to/library --private".to_string(),
        ])),
    }
}

fn infer_source_id(location: &str) -> std::result::Result<String, CliError> {
    let path = PathBuf::from(location);
    if path.exists() {
        if let Some(id) = infer_source_id_from_library_manifest(&path)? {
            validate_source_id(&id)?;
            return Ok(id);
        }
    }
    let trimmed = location.trim_end_matches(&['/', '\\'][..]);
    let basename = trimmed
        .rsplit(['/', '\\', ':'])
        .next()
        .unwrap_or(trimmed)
        .trim_end_matches(".git")
        .to_string();
    if basename.is_empty() {
        return Err(CliError::new(
            EXIT_STATE,
            format!("Could not infer a source id from location: {location}"),
        )
        .with_details(vec![
            "Next: metactl source add <name> <location>".to_string(),
            "Next: add an id field to library.json".to_string(),
        ]));
    }
    validate_source_id(&basename)?;
    Ok(basename)
}

fn infer_source_id_from_library_manifest(
    path: &Path,
) -> std::result::Result<Option<String>, CliError> {
    let manifest = path.join("library.json");
    if !manifest.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&manifest)
        .map_err(|err| internal_error(anyhow!("read {}: {}", manifest.display(), err)))?;
    let value: Value = serde_json::from_str(&raw)
        .map_err(|err| state_error(anyhow!("decode {}: {}", manifest.display(), err)))?;
    Ok(value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(|id| id.to_string()))
}

fn infer_source_type(location: &str) -> SourceType {
    if location.starts_with("git@")
        || location.starts_with("ssh://")
        || location.starts_with("https://")
        || location.starts_with("http://")
    {
        SourceType::Git
    } else {
        SourceType::Local
    }
}

fn validate_source_id(id: &str) -> std::result::Result<(), CliError> {
    let valid = !id.is_empty()
        && !id.contains("..")
        && !id.contains('/')
        && !id.contains('\\')
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'));
    if valid {
        Ok(())
    } else {
        Err(CliError::new(
            EXIT_STATE,
            format!(
                "Invalid source id '{}'. Use a simple slug without path separators.",
                id
            ),
        ))
    }
}

fn find_source_record(config: &ProjectConfigFile, id: &str) -> Option<SourceRecord> {
    config
        .sources
        .iter()
        .find(|source| source.id == id)
        .cloned()
        .or_else(|| {
            config
                .metadata
                .get(&format!("source.{id}"))
                .map(|path| SourceRecord {
                    id: id.to_string(),
                    source_type: SourceType::Local,
                    path: Some(path.clone()),
                    url: None,
                    ref_: None,
                    visibility: SourceVisibility::Public,
                    lock_publicity: SourceLockPublicity::Public,
                })
        })
}

fn sync_source(
    project_root: &Path,
    source: &SourceRecord,
    force: bool,
) -> std::result::Result<LockedSource, CliError> {
    match source.source_type {
        SourceType::Local => sync_local_source(source),
        SourceType::Git => sync_git_source(project_root, source, force),
    }
}

fn sync_local_source(source: &SourceRecord) -> std::result::Result<LockedSource, CliError> {
    let path = source
        .path
        .as_ref()
        .ok_or_else(|| CliError::new(EXIT_STATE, "Local source is missing path."))?;
    let root = PathBuf::from(path);
    validate_library_source_root(&root)?;
    Ok(LockedSource {
        id: source.id.clone(),
        source_type: SourceType::Local,
        visibility: source.visibility.clone(),
        lock_publicity: source.lock_publicity.clone(),
        resolved: Some("synced".to_string()),
        path: Some(path.clone()),
        url: None,
        ref_: None,
        resolved_commit: None,
    })
}

fn sync_git_source(
    project_root: &Path,
    source: &SourceRecord,
    force: bool,
) -> std::result::Result<LockedSource, CliError> {
    let url = source
        .url
        .as_ref()
        .ok_or_else(|| CliError::new(EXIT_STATE, "Git source is missing url."))?;
    let requested_ref = source
        .ref_
        .as_ref()
        .ok_or_else(|| CliError::new(EXIT_STATE, "Git source is missing ref."))?;
    let cache_root = project_root
        .join(".metactl")
        .join("cache")
        .join("sources")
        .join(&source.id);
    if cache_root.exists() && force {
        fs::remove_dir_all(&cache_root)
            .map_err(|err| internal_error(anyhow!("remove {}: {}", cache_root.display(), err)))?;
    }
    if !cache_root.exists() {
        if let Some(parent) = cache_root.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| internal_error(anyhow!("create {}: {}", parent.display(), err)))?;
        }
        run_git(&[
            "clone",
            "--quiet",
            url,
            cache_root.to_string_lossy().as_ref(),
        ])?;
    } else if !git_worktree_clean(&cache_root)? {
        return Err(CliError::new(
            EXIT_STATE,
            format!(
                "Source cache {} has local changes. Re-run with --force to replace it.",
                cache_root.display()
            ),
        ));
    } else {
        run_git_in(&cache_root, &["fetch", "--quiet", "--tags", "origin"])?;
    }
    let resolved_commit = git_resolve_requested_ref(&cache_root, requested_ref)?;
    run_git_in(
        &cache_root,
        &["checkout", "--quiet", "--detach", resolved_commit.trim()],
    )?;
    validate_library_source_root(&cache_root)?;
    Ok(LockedSource {
        id: source.id.clone(),
        source_type: SourceType::Git,
        visibility: source.visibility.clone(),
        lock_publicity: source.lock_publicity.clone(),
        resolved: Some("synced".to_string()),
        path: Some(relative_to_project(project_root, &cache_root)),
        url: Some(url.clone()),
        ref_: Some(requested_ref.clone()),
        resolved_commit: Some(resolved_commit.trim().to_string()),
    })
}

fn validate_library_source_root(root: &Path) -> std::result::Result<(), CliError> {
    if !root.join("library.json").exists() {
        return Err(CliError::new(
            EXIT_STATE,
            format!("Source {} is missing library.json.", root.display()),
        ));
    }
    let packs_dir = root.join("packs");
    if !packs_dir.is_dir() {
        return Err(CliError::new(
            EXIT_STATE,
            format!("Source {} is missing packs/.", root.display()),
        ));
    }
    let registry = LibraryRegistry::load_from_roots(&[root.to_path_buf()]).map_err(state_error)?;
    if registry.list_packs().is_empty() {
        return Err(CliError::new(
            EXIT_STATE,
            format!(
                "Source {} does not contain parseable packs.",
                root.display()
            ),
        ));
    }
    Ok(())
}

fn run_git(args: &[&str]) -> std::result::Result<(), CliError> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|err| internal_error(anyhow!("run git: {}", err)))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(CliError::new(
            EXIT_STATE,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

fn run_git_in(path: &Path, args: &[&str]) -> std::result::Result<(), CliError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .map_err(|err| internal_error(anyhow!("run git: {}", err)))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(CliError::new(
            EXIT_STATE,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

fn git_output_in(path: &Path, args: &[&str]) -> std::result::Result<String, CliError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .map_err(|err| internal_error(anyhow!("run git: {}", err)))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(CliError::new(
            EXIT_STATE,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

fn git_output_in_optional(path: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_resolve_requested_ref(
    path: &Path,
    requested_ref: &str,
) -> std::result::Result<String, CliError> {
    if !requested_ref.contains('/') {
        if let Some(remote_resolved) = git_output_in_optional(
            path,
            &["rev-parse", &format!("origin/{requested_ref}^{{commit}}")],
        ) {
            return Ok(remote_resolved);
        }
    }
    git_output_in(path, &["rev-parse", &format!("{requested_ref}^{{commit}}")])
}

fn git_worktree_clean(path: &Path) -> std::result::Result<bool, CliError> {
    git_output_in(path, &["status", "--porcelain"]).map(|output| output.trim().is_empty())
}

fn upsert_locked_source(sources: &mut Vec<LockedSource>, source: LockedSource) {
    if let Some(existing) = sources.iter_mut().find(|item| item.id == source.id) {
        *existing = source;
    } else {
        sources.push(source);
    }
}

fn redacted_locked_source(source: &LockedSource) -> LockedSource {
    if source.lock_publicity == SourceLockPublicity::Private {
        LockedSource {
            id: source.id.clone(),
            source_type: source.source_type.clone(),
            visibility: source.visibility.clone(),
            lock_publicity: source.lock_publicity.clone(),
            resolved: Some("redacted".to_string()),
            path: None,
            url: None,
            ref_: None,
            resolved_commit: None,
        }
    } else {
        source.clone()
    }
}

fn source_record_json(source: &SourceRecord, origin: &str) -> Value {
    json!({
        "id": source.id,
        "name": source.id,
        "type": source_type_label(&source.source_type),
        "path": source.path,
        "url": source.url,
        "ref": source.ref_,
        "visibility": source_visibility_label(&source.visibility),
        "lock_publicity": source_lock_publicity_label(&source.lock_publicity),
        "origin": origin,
    })
}

fn locked_source_json(source: &LockedSource, redacted: bool) -> Value {
    json!({
        "id": source.id,
        "type": source_type_label(&source.source_type),
        "visibility": source_visibility_label(&source.visibility),
        "lock_publicity": source_lock_publicity_label(&source.lock_publicity),
        "status": source.resolved.as_deref().unwrap_or("unknown"),
        "resolved": source.resolved,
        "path": if redacted { None::<String> } else { source.path.clone() },
        "url": if redacted { None::<String> } else { source.url.clone() },
        "ref": if redacted { None::<String> } else { source.ref_.clone() },
        "resolved_commit": if redacted { None::<String> } else { source.resolved_commit.clone() },
    })
}

fn source_type_label(value: &SourceType) -> &'static str {
    match value {
        SourceType::Local => "local",
        SourceType::Git => "git",
    }
}

fn source_visibility_label(value: &SourceVisibility) -> &'static str {
    match value {
        SourceVisibility::Public => "public",
        SourceVisibility::Private => "private",
    }
}

fn source_lock_publicity_label(value: &SourceLockPublicity) -> &'static str {
    match value {
        SourceLockPublicity::Public => "public",
        SourceLockPublicity::Private => "private",
    }
}
