use std::collections::{BTreeMap, BTreeSet};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use metactl::project::{
    append_history_entry, atomic_write, brownfield_adoption_hint, bundled_starter_library_root,
    compile_manifest_path, current_config_digest, current_local_config_digest,
    current_overlay_digest, default_project_config, detect_brownfield_repo, digest_path,
    ensure_gitignore_entries, ensure_project_layout, is_candidate_pack, list_user_profiles,
    load_compile_manifest, load_partial_project_config, load_policy_report, load_profile_partial,
    load_project_context, load_user_settings, policy_report_path, preferred_apply_mode_for_target,
    private_source_lock_path, profile_path, profiles_directory, project_config_path,
    project_lock_path, resolve_profile_name_for_init, save_user_settings, strip_ansi_codes,
    target_supports_takeover, update_managed_files_index, user_settings_path, write_lock,
    write_partial_project_config, write_policy_report, write_private_source_lock, ConfigOverrides,
    FleetSyncAdoptMode, HistoryEntry, LinkedProject, LinkedProjectStatus, LockedSource,
    LockedTarget, OperationLock, PrivateSourceLock, ProfileActivationSource, ProjectConfigDefaults,
    ProjectConfigFile, ProjectLock, SourceLockPublicity, SourceRecord, SourceType,
    SourceVisibility,
};
use metactl::skill_audit::{self, SkillAuditOptions, SkillAuditScope, SkillReportFormat};
use metactl::{
    ApplyMode, ApplyReport, BrownfieldMode, CompileManifest, CompileParams, DiscoveryMode,
    ExplainParams, ExplainResult, LibraryRegistry, MetactlKernel, ReferenceKernel, ResolveParams,
    SearchParams, SearchResult, SurfaceMergeStrategy, TargetCapabilityMatrix, ValidateParams,
    ValidationReport, ValidationStatus, API_VERSION,
};
use serde_json::{json, Map, Value};

const EXIT_SUCCESS: u8 = 0;
const EXIT_INTERNAL: u8 = 1;
const EXIT_STATE: u8 = 10;
const EXIT_STALE_LOCK: u8 = 11;
const EXIT_CONFLICT: u8 = 12;
const EXIT_VALIDATION: u8 = 13;

const WORKFLOW_HELP: &str = "\
Quick start:
  metactl init -t claude-code        # scaffold a project for Claude Code
  metactl init -t all                # scaffold for every starter-supported target
  metactl init --detect              # detect targets from existing repo surfaces
  metactl profile set-default NAME   # machine default profile for init when no --profile
  metactl init --bind-profile        # record the active machine default in metactl.yaml
  metactl use python-refactor        # resolve, add, and sync a pack in one step
  metactl add python-refactor        # import a pack from the library
  metactl skills audit               # audit local and generated skill surfaces
  metactl add <pack> --sync          # add (or already added) then sync in one step
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
  metactl source add <name> <path>
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

#[derive(Debug, Subcommand)]
enum Commands {
    /// Create metactl.yaml, .metactl/, and starter layout in the project
    Init(InitArgs),
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
    /// Audit skill-like artifacts across local and generated roots
    Skills(SkillsArgs),
    /// Search the pack corpus for a natural-language or keyword query
    Search(SearchArgs),
    /// Show why packs and targets were selected for the current config
    Explain(ExplainArgs),
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
    command: IgnoreCommand,
}

#[derive(Debug, Subcommand)]
enum IgnoreCommand {
    /// Show installed ignore posture
    Status(IgnoreStatusArgs),
    /// Install managed ignore blocks
    Install(IgnoreInstallArgs),
}

#[derive(Debug, Args)]
struct IgnoreStatusArgs {
    /// Targets to inspect (`all` expands to every starter-supported agent target)
    #[arg(long, short = 't')]
    target: Vec<String>,
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
struct AuditArgs {
    #[command(subcommand)]
    command: AuditCommand,
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
    command: SourceCommand,
}

#[derive(Debug, Args)]
struct FleetArgs {
    #[command(subcommand)]
    command: FleetCommand,
}

#[derive(Debug, Subcommand)]
enum FleetCommand {
    /// List linked projects and discovery status
    List,
    /// Show linked project sync readiness
    Status(FleetStatusArgs),
    /// Preview by default or explicitly apply sync across linked projects
    Sync(FleetSyncArgs),
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
    /// Name for this source
    name: String,
    /// Path or Git URL to the source root
    location: String,
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
    name: String,
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
struct ProfileArgs {
    #[command(subcommand)]
    command: ProfileCommand,
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
    command: TargetCommand,
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

#[derive(Debug, Args)]
struct SkillsArgs {
    #[command(subcommand)]
    command: SkillsCommand,
}

#[derive(Debug, Subcommand)]
enum SkillsCommand {
    /// Audit skill-like artifacts across repo, generated, user, and explicit roots
    Audit(SkillsAuditArgs),
}

#[derive(Debug, Args)]
struct SkillsAuditArgs {
    /// Target host kind to model
    #[arg(long, short = 't', default_value = "codex-cli")]
    target: String,
    /// Scope selector: repo, user, all, or explicit-root
    #[arg(long, value_enum, default_value = "repo")]
    scope: SkillsAuditScopeArg,
    /// Override the cwd used for visibility calculations
    #[arg(long)]
    cwd: Option<PathBuf>,
    /// Explicit roots to scan in addition to scope-derived roots
    #[arg(long = "scan-root")]
    scan_root: Vec<PathBuf>,
    /// Include local filesystem paths in the output instead of hashes only
    #[arg(long)]
    include_local_paths: bool,
    /// Print the audit report as Markdown instead of human summary
    #[arg(long, value_enum)]
    format: Option<SkillsAuditFormatArg>,
    /// Write the selected report format to a custom path
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SkillsAuditScopeArg {
    Repo,
    User,
    All,
    ExplicitRoot,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SkillsAuditFormatArg {
    Human,
    Markdown,
    Json,
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

#[derive(Debug, Args)]
struct SyncArgs {
    /// Target runtimes to sync (default: all configured targets)
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
            if cli.json {
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
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&err.json).unwrap_or_else(|_| "{}".to_string())
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
        let project_root = project_root(cli).map_err(internal_error)?;
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
        Commands::Use(args) => cmd_use(cli, args),
        Commands::Add(args) => cmd_add(cli, args),
        Commands::Remove(args) => cmd_remove(cli, args),
        Commands::Target(args) => cmd_target(cli, args),
        Commands::Fleet(args) => cmd_fleet(cli, args),
        Commands::Status(args) => cmd_status(cli, args),
        Commands::List(args) => cmd_list(cli, args),
        Commands::Skills(args) => cmd_skills(cli, args),
        Commands::Search(args) => cmd_search(cli, args),
        Commands::Explain(args) => cmd_explain(cli, args),
        Commands::Sync(args) => cmd_sync(cli, args),
        Commands::Compile(args) => cmd_compile(cli, args),
        Commands::Apply(args) => cmd_apply(cli, args),
        Commands::Revert(args) => cmd_revert(cli, args),
        Commands::Validate(args) => cmd_validate(cli, args),
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
        Commands::Use(_) => Some("use"),
        Commands::Add(_) => Some("add"),
        Commands::Remove(_) => Some("remove"),
        Commands::Target(args) => match &args.command {
            TargetCommand::List(_) => None,
            TargetCommand::Add(_) => Some("target add"),
            TargetCommand::Remove(_) => Some("target remove"),
        },
        Commands::Fleet(args) => match &args.command {
            FleetCommand::List | FleetCommand::Status(_) => None,
            FleetCommand::Sync(args) => args.apply.then_some("fleet sync"),
        },
        Commands::Sync(_) => Some("sync"),
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
            IgnoreCommand::Status(_) => None,
            IgnoreCommand::Install(_) => Some("ignore install"),
        },
        Commands::Hook(args) => match &args.command {
            HookCommand::Install(_) => Some("hook install"),
            HookCommand::Status => None,
        },
        Commands::Source(args) => match &args.command {
            SourceCommand::List => None,
            SourceCommand::Add(_) => Some("source add"),
            SourceCommand::Sync(_) => Some("source sync"),
            SourceCommand::Remove(_) => Some("source remove"),
        },
        Commands::Status(_)
        | Commands::List(_)
        | Commands::Search(_)
        | Commands::Skills(_)
        | Commands::Explain(_)
        | Commands::Validate(_)
        | Commands::Doctor(_)
        | Commands::Audit(_)
        | Commands::Profile(_)
        | Commands::Version => None,
    }
}

fn project_root(cli: &Cli) -> Result<PathBuf> {
    match cli.project.clone() {
        Some(path) => Ok(path),
        None => std::env::current_dir().context("determine current directory"),
    }
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
        default_project_config().starter_library
    };

    let registry =
        load_registry_for_paths(&starter_library, &project_root).map_err(internal_error)?;
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
        let available_display = if available.is_empty() {
            "(none discovered)".to_string()
        } else {
            available.join(", ")
        };
        return Err(CliError::new(
            EXIT_STATE,
            &format!(
                "No target specified and none detected.\n\
                 Available targets: {}\n\
                 Hint: use `metactl init --target <id>` or `metactl init --target all`",
                available_display
            ),
        ));
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

fn cmd_use(cli: &Cli, args: &UseArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let config_path = project_config_path(&project_root, cli.config.as_deref());
    if !config_path.exists() {
        return Err(CliError::new(
            EXIT_STATE,
            "No metactl.yaml found. Run `metactl init` first.",
        ));
    }

    let context = load_required_context(cli, &project_root)?;
    if !context.has_corpus() {
        return Ok(no_corpus_output("use", &project_root));
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
        details: Vec::new(),
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
        TargetCommand::List(args) => cmd_target_list(cli, args),
        TargetCommand::Add(args) => cmd_target_add(cli, args),
        TargetCommand::Remove(args) => cmd_target_remove(cli, args),
    }
}

fn cmd_target_list(
    cli: &Cli,
    args: &TargetListArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_optional_context(cli, &project_root).map_err(internal_error)?;
    let registry = context.registry.or_else(|| {
        let default_root = bundled_starter_library_root();
        if default_root.exists() {
            LibraryRegistry::load_from_roots(&[default_root]).ok()
        } else {
            None
        }
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
        return Err(CliError::new(
            EXIT_STATE,
            "No metactl.yaml found. Run `metactl init` first.",
        ));
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
        return Err(CliError::new(
            EXIT_STATE,
            "No metactl.yaml found. Run `metactl init` first.",
        ));
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
        return Err(CliError::new(
            EXIT_STATE,
            "No metactl.yaml found. Run `metactl init` first.",
        ));
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
        return Err(CliError::new(
            EXIT_STATE,
            format!("Pack(s) not found in library: {}", not_found.join(", ")),
        )
        .with_details(vec![format!("Available packs: {}", available.join(", "))]));
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
        return Err(CliError::new(
            EXIT_STATE,
            "No metactl.yaml found. Run `metactl init` first.",
        ));
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
        FleetCommand::List => cmd_fleet_list(cli),
        FleetCommand::Status(args) => cmd_fleet_status(cli, args),
        FleetCommand::Sync(args) => cmd_fleet_sync(cli, args),
    }
}

fn cmd_fleet_list(cli: &Cli) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_required_context(cli, &project_root)?;
    let projects = fleet_projects_for_output(&project_root, &context.config_file);
    let project_json = projects
        .iter()
        .map(fleet_project_list_json)
        .collect::<Vec<_>>();
    let mut lines = vec!["Fleet projects:".to_string()];
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
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "fleet",
            Some(&project_root),
            json!({
                "action": "list",
                "projects": project_json,
            }),
        ),
    })
}

fn cmd_fleet_status(
    cli: &Cli,
    args: &FleetStatusArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_required_context(cli, &project_root)?;
    let projects = select_fleet_projects(
        &project_root,
        &context.config_file,
        &args.ids,
        args.include_disabled,
    )?;
    let statuses = projects
        .iter()
        .map(|project| fleet_project_status_json(project))
        .collect::<Vec<_>>();
    let mut lines = vec!["Fleet status:".to_string()];
    for status in &statuses {
        lines.push(format!(
            "  {:<18} {:<14} {}",
            status["id"].as_str().unwrap_or("?"),
            status["status"].as_str().unwrap_or("?"),
            status["path"].as_str().unwrap_or("?")
        ));
    }
    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "fleet",
            Some(&project_root),
            json!({
                "action": "status",
                "projects": statuses,
            }),
        ),
    })
}

fn cmd_fleet_sync(cli: &Cli, args: &FleetSyncArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_required_context(cli, &project_root)?;
    let apply = args.apply;
    if apply && !(cli.yes && cli.no_input) {
        return Err(CliError::new(
            EXIT_STATE,
            "fleet sync --apply requires explicit --yes --no-input confirmation",
        ));
    }
    let projects = select_fleet_projects(
        &project_root,
        &context.config_file,
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
        results.push(result);
    }
    if apply {
        write_fleet_sync_log(&project_root, &results).map_err(internal_error)?;
    }
    let failed = results.iter().any(|item| item["status"] == "failed");
    let mut lines = vec![if apply {
        "Fleet sync applied:".to_string()
    } else {
        "Fleet sync preview:".to_string()
    }];
    for item in &results {
        lines.push(format!(
            "  {:<18} {:<14} {}",
            item["id"].as_str().unwrap_or("?"),
            item["status"].as_str().unwrap_or("?"),
            item["path"].as_str().unwrap_or("?")
        ));
    }
    let mut json_payload = success_json(
        "fleet",
        Some(&project_root),
        json!({
            "action": "sync",
            "preview": !apply,
            "projects": results,
        }),
    );
    if failed {
        let mut err = CliError::new(EXIT_STATE, "one or more fleet projects failed");
        json_payload["ok"] = json!(false);
        json_payload["message"] = json!("one or more fleet projects failed");
        err.json = json_payload;
        err.message = "one or more fleet projects failed".to_string();
        return Err(err);
    }
    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: json_payload,
    })
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
    if source_count > 0 || !import_roots.is_empty() {
        let total = source_count + import_roots.len();
        lines.push(format!("  Sources: {} configured", total));
        if let Some(state) = source_state["state"].as_str() {
            lines.push(format!("  Source state: {}", state));
        }
    }

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
        let default_root = bundled_starter_library_root();
        if default_root.exists() {
            LibraryRegistry::load_from_roots(&[default_root]).ok()
        } else {
            None
        }
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

fn cmd_skills(cli: &Cli, args: &SkillsArgs) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        SkillsCommand::Audit(args) => cmd_skills_audit(cli, args),
    }
}

fn cmd_skills_audit(
    cli: &Cli,
    args: &SkillsAuditArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let cwd = match args.cwd.as_ref() {
        Some(cwd) => cwd.clone(),
        None => std::env::current_dir().map_err(|err| internal_error(err.into()))?,
    };
    let scope = match args.scope {
        SkillsAuditScopeArg::Repo => SkillAuditScope::Repo,
        SkillsAuditScopeArg::User => SkillAuditScope::User,
        SkillsAuditScopeArg::All => SkillAuditScope::All,
        SkillsAuditScopeArg::ExplicitRoot => SkillAuditScope::ExplicitRoot,
    };
    let format = match args.format.unwrap_or(if cli.json {
        SkillsAuditFormatArg::Json
    } else {
        SkillsAuditFormatArg::Human
    }) {
        SkillsAuditFormatArg::Human => SkillReportFormat::Human,
        SkillsAuditFormatArg::Markdown => SkillReportFormat::Markdown,
        SkillsAuditFormatArg::Json => SkillReportFormat::Json,
    };
    let output = skill_audit::run_audit(
        &project_root,
        SkillAuditOptions {
            target_id: args.target.clone(),
            scope,
            cwd,
            scan_roots: args.scan_root.clone(),
            include_local_paths: args.include_local_paths,
            format,
            output_path: args.output.clone(),
        },
    )
    .map_err(internal_error)?;

    let summary = &output.report.summary;
    let human = format!(
        "Project: {}\nTarget: {}\nScope: {}\nSkills: {}\nRelations: {}\nCollector: {}\nUsage: {}\nNext: inspect {}/latest.md",
        project_root.display(),
        output.report.target_id,
        output.report.scan_scope,
        summary.total_skills,
        summary.relation_count,
        output.report.collector_status,
        output.report.usage_window,
        output.report_markdown_path.parent().unwrap_or(&output.report_markdown_path).display()
    );
    let human = if output.report.notes.is_empty() {
        human
    } else {
        format!("{}\nNotes: {}", human, output.report.notes.join(" | "))
    };

    let command_json = output.json.clone();
    let command_human = match format {
        SkillReportFormat::Json => {
            serde_json::to_string_pretty(&command_json).unwrap_or_else(|_| "{}".to_string())
        }
        SkillReportFormat::Markdown => output.markdown.clone(),
        SkillReportFormat::Human => human.clone(),
    };

    Ok(CommandOutput {
        human: project_human_output(&project_root, command_human),
        json: success_json(
            "skills",
            Some(&project_root),
            json!({
                "action": "audit",
                "report": command_json,
                "report_json_path": relative_to_project(&project_root, &output.report_json_path),
                "report_markdown_path": relative_to_project(&project_root, &output.report_markdown_path),
                "inventory_path": relative_to_project(&project_root, &output.inventory_path),
                "relations_path": relative_to_project(&project_root, &output.relations_path),
                "plan_path": output.plan_path.as_ref().map(|path| relative_to_project(&project_root, path)),
            }),
        ),
    })
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
    let surface_details = derived_surface_details;
    Ok(explain_output(
        &project_root,
        args.query.as_deref(),
        &explain,
        &target_projection,
        surface_details.as_deref(),
        pack_lifecycle.as_ref(),
        &pack_sources,
    ))
}

fn cmd_sync(cli: &Cli, args: &SyncArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let context = load_required_context(cli, &project_root)?;
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
    let compile_out = cmd_compile(
        cli,
        &CompileArgs {
            target: args.target.clone(),
            all: args.all,
            role: args.role.clone(),
            policy: args.policy.clone(),
            update_lock: true,
            apply: false,
            apply_mode: None,
            surface_mode: args.surface_mode,
        },
    )?;

    let apply_args = match args.adopt {
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
        Some(cmd_validate(cli, &ValidateCmdArgs { target: None })?)
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
        args.target.clone()
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
        return Err(CliError {
            code: EXIT_VALIDATION,
            message: "Validation failed because private source state may be tracked or exposed."
                .to_string(),
            details: Vec::new(),
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
            }),
        ),
    })
}

fn cmd_doctor(cli: &Cli, args: &DoctorArgs) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
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
    load_project_context(
        project_root,
        cli.config.as_deref(),
        cli.profile.as_deref(),
        cli.overlay.as_deref(),
    )
    .map_err(state_error)
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
    let roots = paths
        .iter()
        .map(|item| {
            let path = PathBuf::from(item);
            if path.is_absolute() {
                path
            } else {
                project_root.join(path)
            }
        })
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
        let example = if bundled.exists() {
            format!(" (for example {})", bundled.to_string_lossy())
        } else {
            String::new()
        };
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

fn internal_error(error: anyhow::Error) -> CliError {
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
    match args.command {
        AuditCommand::Sources => cmd_audit_sources(cli),
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
        "subject": "sources",
        "findings": findings,
    });
    if failed {
        Err(CliError {
            code: EXIT_VALIDATION,
            message: "Source audit failed.".to_string(),
            details: Vec::new(),
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
            "remediation": "Remove the file from the index and install local private source ignores.",
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
                    "remediation": "Use lock_publicity: private and rewrite source locks.",
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

fn cmd_ignore(cli: &Cli, args: &IgnoreArgs) -> std::result::Result<CommandOutput, CliError> {
    match &args.command {
        IgnoreCommand::Status(status_args) => cmd_ignore_status(cli, status_args),
        IgnoreCommand::Install(install_args) => cmd_ignore_install(cli, install_args),
    }
}

fn cmd_ignore_status(
    cli: &Cli,
    args: &IgnoreStatusArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let targets = resolve_ignore_targets(&project_root, &args.target).map_err(state_error)?;
    let files = ignore_status_files(&project_root);
    let repo_ignore_file = project_root.join(".gitignore");
    let private_sources = private_source_ignore_status(&project_root);

    let mut lines = vec!["Ignore posture:".to_string()];
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

    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "ignore",
            Some(&project_root),
            json!({
                "action": "status",
                "targets": targets,
                "files": files,
                "private_sources": private_sources,
            }),
        ),
    })
}

fn cmd_ignore_install(
    cli: &Cli,
    args: &IgnoreInstallArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let targets = resolve_ignore_targets(&project_root, &args.target).map_err(state_error)?;
    let mut changes = Vec::new();

    match args.scope {
        IgnoreScopeArg::Local => {
            let git_dir = project_root.join(".git");
            if !git_dir.exists() {
                return Err(CliError::new(
                    EXIT_STATE,
                    "No .git directory found. Local ignore scope writes .git/info/exclude.",
                ));
            }
            let patterns =
                git_ignore_patterns(&targets, args.include_lock, args.include_private_sources);
            changes.push(write_marked_block(
                &project_root,
                &git_dir.join("info").join("exclude"),
                IGNORE_BLOCK_BEGIN,
                IGNORE_BLOCK_END,
                &patterns,
            )?);
        }
        IgnoreScopeArg::Repo => {
            let patterns =
                git_ignore_patterns(&targets, args.include_lock, args.include_private_sources);
            changes.push(write_marked_block(
                &project_root,
                &project_root.join(".gitignore"),
                IGNORE_BLOCK_BEGIN,
                IGNORE_BLOCK_END,
                &patterns,
            )?);
            if targets.iter().any(|target| target == "cursor") {
                changes.push(write_marked_block(
                    &project_root,
                    &project_root.join(".cursorignore"),
                    AGENT_ALLOWLIST_BEGIN,
                    AGENT_ALLOWLIST_END,
                    &cursor_allowlist_patterns(&targets),
                )?);
            }
            if targets.iter().any(|target| target == "gemini-cli") {
                changes.push(write_marked_block(
                    &project_root,
                    &project_root.join(".geminiignore"),
                    AGENT_ALLOWLIST_BEGIN,
                    AGENT_ALLOWLIST_END,
                    &gemini_allowlist_patterns(),
                )?);
            }
        }
    }

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
                "include_lock": args.include_lock,
                "include_private_sources": args.include_private_sources,
                "changes": changes,
            }),
        ),
    })
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

fn resolve_ignore_targets(project_root: &Path, requested: &[String]) -> Result<Vec<String>> {
    let raw_targets = if requested.is_empty() {
        let config_path = project_root.join("metactl.yaml");
        if config_path.exists() {
            let config = load_partial_project_config(&config_path)?;
            if config.targets.is_empty() {
                default_project_config().targets
            } else {
                config.targets
            }
        } else {
            IGNORE_TARGETS.iter().map(|item| item.to_string()).collect()
        }
    } else {
        requested.to_vec()
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
    Ok(targets.into_iter().collect())
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
        ProfileCommand::List => {
            let items = list_user_profiles().map_err(internal_error)?;
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
                    }),
                ),
            })
        }
        ProfileCommand::Show => {
            let settings = load_user_settings();
            let path = user_settings_path();
            let human = format!(
                "User settings file: {}\nDefault profile: {}\n",
                path.as_ref()
                    .map(|item| item.display().to_string())
                    .unwrap_or_else(|| "(unavailable — set HOME or XDG_CONFIG_HOME)".to_string()),
                settings.default_profile.as_deref().unwrap_or("(none)"),
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
                    }),
                ),
            })
        }
        ProfileCommand::SetDefault { name } => {
            let Some(profile_file) = profile_path(name) else {
                return Err(CliError::new(
                    EXIT_STATE,
                    "HOME (or XDG_CONFIG_HOME) is not set; cannot resolve profile path.",
                ));
            };
            if !profile_file.exists() {
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
        ProfileCommand::ClearDefault => {
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
        SourceCommand::List => cmd_source_list(cli),
        SourceCommand::Add(add_args) => cmd_source_add(cli, add_args),
        SourceCommand::Sync(sync_args) => cmd_source_sync(cli, sync_args),
        SourceCommand::Remove(remove_args) => cmd_source_remove(cli, remove_args),
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
        return Err(CliError::new(
            EXIT_STATE,
            "No metactl.yaml found. Run `metactl init` first.",
        ));
    }

    validate_source_id(&args.name)?;
    let inferred_type = args
        .source_type
        .map(Into::into)
        .unwrap_or_else(|| infer_source_type(&args.location));
    if inferred_type == SourceType::Git && args.ref_.is_none() && !args.allow_floating_ref {
        return Err(CliError::new(
            EXIT_STATE,
            "Git sources require --ref unless --allow-floating-ref is passed.",
        ));
    }

    let path = PathBuf::from(&args.location);
    if inferred_type == SourceType::Local && !path.exists() {
        if cli.no_input {
            return Err(CliError::new(
                EXIT_STATE,
                format!("Source path does not exist: {}", args.location),
            ));
        }
        eprintln!("Warning: source path does not exist: {}", args.location);
    }

    let mut raw = load_partial_project_config(&config_path).map_err(internal_error)?;
    if raw.sources.iter().any(|source| source.id == args.name) {
        return Ok(CommandOutput {
            human: project_human_output(
                &project_root,
                format!("Source '{}' already configured.", args.name),
            ),
            json: success_json(
                "source",
                Some(&project_root),
                json!({
                    "action": "add",
                    "name": args.name,
                    "source": raw.sources.iter().find(|source| source.id == args.name).map(|source| source_record_json(source, "config")),
                    "already_configured": true,
                }),
            ),
        });
    }

    let source = SourceRecord {
        id: args.name.clone(),
        source_type: inferred_type.clone(),
        path: (inferred_type == SourceType::Local).then(|| args.location.clone()),
        url: (inferred_type == SourceType::Git).then(|| args.location.clone()),
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
            format!("Added source '{}' at {}.", args.name, args.location),
        ),
        json: success_json(
            "source",
            Some(&project_root),
            json!({
                "action": "add",
                "name": args.name,
                "source": source_record_json(&source, "config"),
                "already_configured": false,
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
    let source = find_source_record(&context.config_file, &args.name).ok_or_else(|| {
        CliError::new(
            EXIT_STATE,
            format!("Source '{}' is not configured.", args.name),
        )
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
