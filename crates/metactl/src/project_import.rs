use std::collections::BTreeSet;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::anyhow;
use clap::{Args, Subcommand, ValueEnum};
use metactl::project::{
    digest_path, ensure_gitignore_entries, ensure_project_layout, load_lock,
    load_partial_project_config, load_project_context, project_config_path, project_lock_path,
    write_lock, write_partial_project_config, PartialProjectConfig, ProjectConfigDefaults,
    ProjectConfigFile, SourceRecord, SourceVisibility,
};
use serde_json::{json, Value};

use super::*;

const PROJECT_IMPORT_LIST_DEFAULT_LIMIT: usize = 20;
const PROJECT_IMPORT_NAME_WIDTH: usize = 24;
const PROJECT_IMPORT_ID_WIDTH: usize = 24;
const PROJECT_IMPORT_STATUS_WIDTH: usize = 12;
const PROJECT_IMPORT_SOURCE_WIDTH: usize = 16;
const PROJECT_IMPORT_PATH_WIDTH: usize = 56;

#[derive(Debug, Subcommand)]
pub(super) enum ProjectImportCommand {
    /// List importable projects discovered from fleet configuration and search roots
    List(ProjectImportListArgs),
    /// Show importable configuration fields and aliases
    Fields(ProjectImportFieldsArgs),
    /// Inspect an importable source project without writing anything
    Inspect(ProjectImportSourceArgs),
    /// Preview the configuration that would be imported
    Plan(ProjectImportPlanArgs),
    /// Import configuration into the current project
    Apply(ProjectImportApplyArgs),
    /// Browse importable projects interactively
    Browse(ProjectImportBrowseArgs),
}

#[derive(Debug, Args)]
pub(super) struct ProjectImportListArgs {
    /// Additional roots to scan for metactl.yaml files
    #[arg(long = "search-root", value_name = "PATH")]
    search_root: Vec<PathBuf>,
    /// Include disabled, missing, or invalid projects in the listing
    #[arg(long)]
    all: bool,
    /// Limit human table rows; 0 shows all (JSON always includes all matches)
    #[arg(
        long,
        value_name = "N",
        default_value_t = PROJECT_IMPORT_LIST_DEFAULT_LIMIT
    )]
    limit: usize,
}

#[derive(Debug, Args)]
pub(super) struct ProjectImportFieldsArgs {}

#[derive(Debug, Args)]
pub(super) struct ProjectImportSourceArgs {
    /// Project id, project folder name, or direct path
    source: String,
    /// Additional roots to scan for metactl.yaml files
    #[arg(long = "search-root", value_name = "PATH")]
    search_root: Vec<PathBuf>,
    /// Permit selecting disabled, missing, or invalid projects for inspection
    #[arg(long)]
    include_unready: bool,
}

#[derive(Debug, Args)]
pub(super) struct ProjectImportPlanArgs {
    /// Project id, project folder name, or direct path
    source: String,
    /// Import mode
    #[arg(long = "mode", value_enum, default_value = "auto")]
    mode: ProjectImportModeArg,
    /// Comma-separated import fields (default: role,policy,packs,targets,extends-profile,defaults,artifact-policy)
    #[arg(long = "fields", value_name = "FIELDS")]
    fields: Option<String>,
    /// Include public source records in the import plan
    #[arg(long)]
    include_public_sources: bool,
    /// Include private source records in the import plan
    #[arg(long)]
    include_private_sources: bool,
    /// Additional roots to scan for metactl.yaml files
    #[arg(long = "search-root", value_name = "PATH")]
    search_root: Vec<PathBuf>,
}

#[derive(Debug, Args)]
pub(super) struct ProjectImportApplyArgs {
    /// Project id, project folder name, or direct path
    source: String,
    /// Import mode
    #[arg(long = "mode", value_enum, default_value = "auto")]
    mode: ProjectImportModeArg,
    /// Comma-separated import fields (default: role,policy,packs,targets,extends-profile,defaults,artifact-policy)
    #[arg(long = "fields", value_name = "FIELDS")]
    fields: Option<String>,
    /// Include public source records in the imported config
    #[arg(long)]
    include_public_sources: bool,
    /// Include private source records in the imported config
    #[arg(long)]
    include_private_sources: bool,
    /// Merge imported values into an existing metactl.yaml
    #[arg(long)]
    merge: bool,
    /// Replace selected fields in an existing metactl.yaml
    #[arg(long)]
    replace: bool,
    /// Additional roots to scan for metactl.yaml files
    #[arg(long = "search-root", value_name = "PATH")]
    search_root: Vec<PathBuf>,
    /// Confirm non-interactive project state writes
    #[arg(long, short = 'y')]
    yes: bool,
}

#[derive(Debug, Args)]
pub(super) struct ProjectImportBrowseArgs {
    /// Import mode
    #[arg(long = "mode", value_enum, default_value = "auto")]
    pub(super) mode: ProjectImportModeArg,
    /// Comma-separated import fields (default: role,policy,packs,targets,extends-profile,defaults,artifact-policy)
    #[arg(long = "fields", value_name = "FIELDS")]
    pub(super) fields: Option<String>,
    /// Include public source records in the import
    #[arg(long)]
    pub(super) include_public_sources: bool,
    /// Include private source records in the import
    #[arg(long)]
    pub(super) include_private_sources: bool,
    /// Write the selected import instead of only showing a plan
    #[arg(long)]
    pub(super) apply: bool,
    /// Merge imported values into an existing metactl.yaml when applying
    #[arg(long)]
    pub(super) merge: bool,
    /// Replace selected fields in an existing metactl.yaml when applying
    #[arg(long)]
    pub(super) replace: bool,
    /// Additional roots to scan for metactl.yaml files
    #[arg(long = "search-root", value_name = "PATH")]
    pub(super) search_root: Vec<PathBuf>,
    /// Confirm project state writes
    #[arg(long, short = 'y')]
    pub(super) yes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(super) enum ProjectImportModeArg {
    Auto,
    ProfileBound,
    Explicit,
    MaterializeEffective,
}

#[derive(Debug, Clone)]
struct ProjectImportCandidate {
    id: String,
    name: String,
    path: PathBuf,
    config_path: PathBuf,
    profile: Option<String>,
    status: &'static str,
    source: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectImportResolvedMode {
    ProfileBound,
    Explicit,
    MaterializeEffective,
}

impl ProjectImportResolvedMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::ProfileBound => "profile_bound",
            Self::Explicit => "explicit",
            Self::MaterializeEffective => "materialize_effective",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProjectImportApplyMode {
    Create,
    Merge,
    Replace,
}

impl ProjectImportApplyMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Merge => "merge",
            Self::Replace => "replace",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ProjectImportField {
    Role,
    Policy,
    Packs,
    Targets,
    ExtendsProfile,
    Defaults,
    ArtifactPolicy,
    Sources,
    StarterLibrary,
}

const PROJECT_IMPORT_DEFAULT_FIELDS: &[ProjectImportField] = &[
    ProjectImportField::Role,
    ProjectImportField::Policy,
    ProjectImportField::Packs,
    ProjectImportField::Targets,
    ProjectImportField::ExtendsProfile,
    ProjectImportField::Defaults,
    ProjectImportField::ArtifactPolicy,
];

const PROJECT_IMPORT_ALL_FIELDS: &[ProjectImportField] = &[
    ProjectImportField::Role,
    ProjectImportField::Policy,
    ProjectImportField::Packs,
    ProjectImportField::Targets,
    ProjectImportField::ExtendsProfile,
    ProjectImportField::Defaults,
    ProjectImportField::ArtifactPolicy,
    ProjectImportField::Sources,
    ProjectImportField::StarterLibrary,
];

const PROJECT_IMPORT_ALLOWED_FIELDS: &str = "role, policy, packs, targets, extends-profile, defaults, artifact-policy, sources, starter-library";

impl ProjectImportField {
    fn label(self) -> &'static str {
        match self {
            Self::Role => "role",
            Self::Policy => "policy",
            Self::Packs => "packs",
            Self::Targets => "targets",
            Self::ExtendsProfile => "extends-profile",
            Self::Defaults => "defaults",
            Self::ArtifactPolicy => "artifact-policy",
            Self::Sources => "sources",
            Self::StarterLibrary => "starter-library",
        }
    }

    fn aliases(self) -> &'static [&'static str] {
        match self {
            Self::Role => &[],
            Self::Policy => &[],
            Self::Packs => &["pack"],
            Self::Targets => &["target"],
            Self::ExtendsProfile => &["profile"],
            Self::Defaults => &[],
            Self::ArtifactPolicy => &["agent-artifact-policy"],
            Self::Sources => &["source"],
            Self::StarterLibrary => &["starter-libraries"],
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Role => "Project role value.",
            Self::Policy => "Project policy value.",
            Self::Packs => "Configured pack ids.",
            Self::Targets => "Configured target runtime ids.",
            Self::ExtendsProfile => "Profile binding when the source config extends a profile.",
            Self::Defaults => "Project default settings.",
            Self::ArtifactPolicy => "Agent artifact policy metadata.",
            Self::Sources => {
                "Source records; public/private source flags still control copied records."
            }
            Self::StarterLibrary => "Starter library ids.",
        }
    }

    fn is_default(self) -> bool {
        PROJECT_IMPORT_DEFAULT_FIELDS.contains(&self)
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().replace('_', "-").as_str() {
            "role" => Some(Self::Role),
            "policy" => Some(Self::Policy),
            "packs" | "pack" => Some(Self::Packs),
            "targets" | "target" => Some(Self::Targets),
            "extends-profile" | "profile" => Some(Self::ExtendsProfile),
            "defaults" => Some(Self::Defaults),
            "artifact-policy" | "agent-artifact-policy" => Some(Self::ArtifactPolicy),
            "sources" | "source" => Some(Self::Sources),
            "starter-library" | "starter-libraries" => Some(Self::StarterLibrary),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct ProjectImportPlan {
    candidate: ProjectImportCandidate,
    mode: ProjectImportResolvedMode,
    fields: BTreeSet<ProjectImportField>,
    projected_config: PartialProjectConfig,
    equivalence: &'static str,
    warnings: Vec<Value>,
    source_summary: Value,
}

pub(super) fn cmd_project_import(
    cli: &Cli,
    command: &ProjectImportCommand,
) -> std::result::Result<CommandOutput, CliError> {
    match command {
        ProjectImportCommand::List(args) => cmd_project_import_list(cli, args),
        ProjectImportCommand::Fields(_) => cmd_project_import_fields(cli),
        ProjectImportCommand::Inspect(args) => cmd_project_import_inspect(cli, args),
        ProjectImportCommand::Plan(args) => {
            let options = ProjectImportPlanOptions {
                source: &args.source,
                mode: args.mode,
                fields: args.fields.as_deref(),
                include_public_sources: args.include_public_sources,
                include_private_sources: args.include_private_sources,
                search_roots: &args.search_root,
                include_unready: false,
            };
            cmd_project_import_plan(cli, &options, "project import")
        }
        ProjectImportCommand::Apply(args) => {
            let options = ProjectImportPlanOptions {
                source: &args.source,
                mode: args.mode,
                fields: args.fields.as_deref(),
                include_public_sources: args.include_public_sources,
                include_private_sources: args.include_private_sources,
                search_roots: &args.search_root,
                include_unready: false,
            };
            let apply_mode = project_import_apply_mode(args.merge, args.replace)?;
            cmd_project_import_apply(cli, &options, apply_mode, args.yes, "project import")
        }
        ProjectImportCommand::Browse(args) => {
            cmd_project_import_browse(cli, args, "project import")
        }
    }
}

pub(super) struct ProjectImportPlanOptions<'a> {
    pub(super) source: &'a str,
    pub(super) mode: ProjectImportModeArg,
    pub(super) fields: Option<&'a str>,
    pub(super) include_public_sources: bool,
    pub(super) include_private_sources: bool,
    pub(super) search_roots: &'a [PathBuf],
    pub(super) include_unready: bool,
}

fn cmd_project_import_list(
    cli: &Cli,
    args: &ProjectImportListArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let projects = discover_project_import_candidates(cli, &project_root, &args.search_root)?;
    let visible = projects
        .into_iter()
        .filter(|candidate| args.all || candidate.status == "ready")
        .collect::<Vec<_>>();
    let mut lines = vec!["Importable projects:".to_string()];
    if visible.is_empty() {
        lines.push("  (none found)".to_string());
        lines.push(
            "Next: pass --search-root /path/to/projects or configure linked_projects.".to_string(),
        );
    } else {
        let display_count = project_import_display_count(visible.len(), args.limit);
        lines.push(format!(
            "Showing {} of {} importable projects:",
            display_count,
            visible.len()
        ));
        lines.push(project_import_list_header());
        lines.push(project_import_list_rule());
        for candidate in visible.iter().take(display_count) {
            lines.push(format!(
                "  {:<name_width$} {:<id_width$} {:<status_width$} {:<source_width$} {}",
                truncate_project_import_cell(&candidate.name, PROJECT_IMPORT_NAME_WIDTH),
                truncate_project_import_cell(&candidate.id, PROJECT_IMPORT_ID_WIDTH),
                truncate_project_import_cell(candidate.status, PROJECT_IMPORT_STATUS_WIDTH),
                truncate_project_import_cell(candidate.source, PROJECT_IMPORT_SOURCE_WIDTH),
                truncate_project_import_path(
                    &candidate.path.display().to_string(),
                    PROJECT_IMPORT_PATH_WIDTH
                ),
                name_width = PROJECT_IMPORT_NAME_WIDTH,
                id_width = PROJECT_IMPORT_ID_WIDTH,
                status_width = PROJECT_IMPORT_STATUS_WIDTH,
                source_width = PROJECT_IMPORT_SOURCE_WIDTH,
            ));
        }
        if visible.len() > display_count {
            lines.push(format!(
                "  {} more not shown. Use --limit {} or --json for all matches.",
                visible.len() - display_count,
                visible.len()
            ));
        }
        lines.push("Next: metactl project import inspect <id>".to_string());
        lines.push("Next: metactl project import plan <id>".to_string());
    }
    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "project import",
            Some(&project_root),
            json!({
                "action": "list",
                "projects": visible.iter().map(project_import_candidate_json).collect::<Vec<_>>(),
                "count": visible.len(),
                "displayed_count": project_import_display_count(visible.len(), args.limit),
                "limit": args.limit,
                "next_commands": [
                    "metactl project import inspect <id>",
                    "metactl project import plan <id>",
                    "metactl project import fields",
                ],
            }),
        ),
    })
}

fn cmd_project_import_fields(cli: &Cli) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let default_fields = PROJECT_IMPORT_DEFAULT_FIELDS
        .iter()
        .map(|field| field.label())
        .collect::<Vec<_>>();
    let mut lines = vec![
        "Import fields:".to_string(),
        format!("  Default: {}", default_fields.join(", ")),
        format!("  Allowed: {}", PROJECT_IMPORT_ALLOWED_FIELDS),
        String::new(),
        format!("  {:<20} {:<8} Description", "Field", "Default"),
        "  -------------------- -------- ----------------------------------------".to_string(),
    ];
    for field in PROJECT_IMPORT_ALL_FIELDS {
        lines.push(format!(
            "  {:<20} {:<8} {}",
            field.label(),
            if field.is_default() { "yes" } else { "no" },
            field.description()
        ));
    }
    lines.push(String::new());
    lines.push("Sources are excluded by default; opt in with --include-public-sources or --include-private-sources.".to_string());
    lines.push(
        "Next: metactl project import plan <project> --fields role,packs,targets".to_string(),
    );
    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "project import",
            Some(&project_root),
            json!({
                "action": "fields",
                "default_fields": default_fields,
                "allowed_fields": PROJECT_IMPORT_ALLOWED_FIELDS,
                "fields": PROJECT_IMPORT_ALL_FIELDS
                    .iter()
                    .map(project_import_field_json)
                    .collect::<Vec<_>>(),
                "next_commands": [
                    "metactl project import plan <project> --fields role,packs,targets",
                    "metactl project import apply <project> --fields role,packs,targets --yes",
                ],
            }),
        ),
    })
}

fn project_import_display_count(total: usize, limit: usize) -> usize {
    if limit == 0 {
        total
    } else {
        total.min(limit)
    }
}

fn project_import_list_header() -> String {
    format!(
        "  {:<name_width$} {:<id_width$} {:<status_width$} {:<source_width$} {}",
        "Name",
        "Id",
        "Status",
        "Source",
        "Path",
        name_width = PROJECT_IMPORT_NAME_WIDTH,
        id_width = PROJECT_IMPORT_ID_WIDTH,
        status_width = PROJECT_IMPORT_STATUS_WIDTH,
        source_width = PROJECT_IMPORT_SOURCE_WIDTH,
    )
}

fn project_import_list_rule() -> String {
    format!(
        "  {:<name_width$} {:<id_width$} {:<status_width$} {:<source_width$} {}",
        "-".repeat(PROJECT_IMPORT_NAME_WIDTH),
        "-".repeat(PROJECT_IMPORT_ID_WIDTH),
        "-".repeat(PROJECT_IMPORT_STATUS_WIDTH),
        "-".repeat(PROJECT_IMPORT_SOURCE_WIDTH),
        "-".repeat(PROJECT_IMPORT_PATH_WIDTH),
        name_width = PROJECT_IMPORT_NAME_WIDTH,
        id_width = PROJECT_IMPORT_ID_WIDTH,
        status_width = PROJECT_IMPORT_STATUS_WIDTH,
        source_width = PROJECT_IMPORT_SOURCE_WIDTH,
    )
}

fn truncate_project_import_cell(value: &str, width: usize) -> String {
    truncate_project_import_text(value, width, false)
}

fn truncate_project_import_path(value: &str, width: usize) -> String {
    truncate_project_import_text(value, width, true)
}

fn truncate_project_import_text(value: &str, width: usize, keep_tail: bool) -> String {
    if value.chars().count() <= width {
        return value.to_string();
    }
    if width == 0 {
        return String::new();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let keep = width - 3;
    if keep_tail {
        let tail = value
            .chars()
            .rev()
            .take(keep)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>();
        format!("...{tail}")
    } else {
        let head = value.chars().take(keep).collect::<String>();
        format!("{head}...")
    }
}

fn project_import_field_json(field: &ProjectImportField) -> Value {
    json!({
        "name": field.label(),
        "default": field.is_default(),
        "aliases": field.aliases(),
        "description": field.description(),
    })
}

fn cmd_project_import_inspect(
    cli: &Cli,
    args: &ProjectImportSourceArgs,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let candidate = resolve_project_import_candidate(
        cli,
        &project_root,
        &args.source,
        &args.search_root,
        args.include_unready,
    )?;
    if candidate.status != "ready" {
        return Err(project_import_error(
            "source_not_ready",
            format!(
                "Project `{}` is not ready for import ({})",
                candidate.name, candidate.status
            ),
            vec![format!("Check {}", candidate.config_path.display())],
        ));
    }
    let raw = load_partial_project_config(&candidate.config_path).map_err(state_error)?;
    let context_result =
        load_project_context(&candidate.path, None, candidate.profile.as_deref(), None);
    let (effective_summary, context_warning) = match context_result {
        Ok(context) => (project_import_effective_summary(&context), None),
        Err(err) => (
            Value::Null,
            Some(json!({
                "code": "source_context_unavailable",
                "message": err.to_string(),
            })),
        ),
    };
    let mut warnings = Vec::new();
    if let Some(warning) = context_warning {
        warnings.push(warning);
    }
    let raw_summary = project_import_partial_summary(&raw);
    let mut lines = vec![
        format!("Source: {} ({})", candidate.name, candidate.id),
        format!("Path: {}", candidate.path.display()),
        format!(
            "Role: {}",
            raw.role.as_deref().unwrap_or("(profile/default)")
        ),
        format!(
            "Policy: {}",
            raw.policy.as_deref().unwrap_or("(profile/default)")
        ),
        format!(
            "Targets: {}",
            if raw.targets.is_empty() {
                "(profile/default)".to_string()
            } else {
                raw.targets.join(", ")
            }
        ),
        format!(
            "Packs: {}",
            if raw.packs.is_empty() {
                "(profile/default)".to_string()
            } else {
                raw.packs.join(", ")
            }
        ),
    ];
    if !warnings.is_empty() {
        lines.push(format!("Warnings: {}", warnings.len()));
    }
    Ok(CommandOutput {
        human: project_human_output(&project_root, lines.join("\n")),
        json: success_json(
            "project import",
            Some(&project_root),
            json!({
                "action": "inspect",
                "source": project_import_candidate_json(&candidate),
                "raw_config": raw_summary,
                "effective_config": effective_summary,
                "warnings": warnings,
            }),
        ),
    })
}

pub(super) fn cmd_project_import_plan(
    cli: &Cli,
    options: &ProjectImportPlanOptions<'_>,
    command_label: &str,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let plan = build_project_import_plan(cli, &project_root, options)?;
    Ok(project_import_plan_output(
        &project_root,
        &plan,
        command_label,
    ))
}

pub(super) fn cmd_project_import_apply(
    cli: &Cli,
    options: &ProjectImportPlanOptions<'_>,
    apply_mode: ProjectImportApplyMode,
    yes: bool,
    command_label: &str,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    let plan = build_project_import_plan(cli, &project_root, options)?;
    apply_project_import_plan(cli, &project_root, &plan, apply_mode, yes, command_label)
}

pub(super) fn cmd_project_import_browse(
    cli: &Cli,
    args: &ProjectImportBrowseArgs,
    command_label: &str,
) -> std::result::Result<CommandOutput, CliError> {
    let project_root = project_root(cli).map_err(internal_error)?;
    if cli.no_input_enabled() || !io::stdin().is_terminal() {
        return Err(project_import_error(
            "browse_requires_tty",
            "Project import browse requires an interactive terminal.",
            vec![
                "Use `metactl project import list --json` to discover candidates.".to_string(),
                "Then run `metactl project import apply <project> --yes`.".to_string(),
            ],
        ));
    }
    let candidates = discover_project_import_candidates(cli, &project_root, &args.search_root)?
        .into_iter()
        .filter(|candidate| candidate.status == "ready")
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(project_import_error(
            "no_import_candidates",
            "No ready metactl-managed projects were found.",
            vec!["Pass --search-root /path/to/projects or configure linked_projects.".to_string()],
        ));
    }
    println!("Importable projects:");
    for (index, candidate) in candidates.iter().enumerate() {
        println!(
            "  {}. {} ({}) {}",
            index + 1,
            candidate.name,
            candidate.id,
            candidate.path.display()
        );
    }
    print!("Select project number: ");
    io::stdout()
        .flush()
        .map_err(|err| internal_error(anyhow!(err)))?;
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|err| internal_error(anyhow!(err)))?;
    let selected = line.trim().parse::<usize>().ok().and_then(|index| {
        if index == 0 {
            None
        } else {
            candidates.get(index - 1).cloned()
        }
    });
    let Some(candidate) = selected else {
        return Err(project_import_error(
            "invalid_selection",
            "Selection did not match a listed project.",
            vec!["Run `metactl project import browse` again.".to_string()],
        ));
    };
    let source = candidate.id.clone();
    let options = ProjectImportPlanOptions {
        source: &source,
        mode: args.mode,
        fields: args.fields.as_deref(),
        include_public_sources: args.include_public_sources,
        include_private_sources: args.include_private_sources,
        search_roots: &args.search_root,
        include_unready: false,
    };
    if args.apply {
        let apply_mode = project_import_apply_mode(args.merge, args.replace)?;
        cmd_project_import_apply(cli, &options, apply_mode, args.yes, command_label)
    } else {
        cmd_project_import_plan(cli, &options, command_label)
    }
}

fn build_project_import_plan(
    cli: &Cli,
    project_root: &Path,
    options: &ProjectImportPlanOptions<'_>,
) -> std::result::Result<ProjectImportPlan, CliError> {
    let candidate = resolve_project_import_candidate(
        cli,
        project_root,
        options.source,
        options.search_roots,
        options.include_unready,
    )?;
    if candidate.status != "ready" {
        return Err(project_import_error(
            "source_not_ready",
            format!(
                "Project `{}` is not ready for import ({})",
                candidate.name, candidate.status
            ),
            vec![format!("Check {}", candidate.config_path.display())],
        ));
    }
    let raw = load_partial_project_config(&candidate.config_path).map_err(state_error)?;
    let source_context =
        load_project_context(&candidate.path, None, candidate.profile.as_deref(), None);
    let mode = resolve_project_import_mode(options.mode, &raw);
    if mode == ProjectImportResolvedMode::MaterializeEffective && source_context.is_err() {
        return Err(project_import_error(
            "source_context_unavailable",
            format!(
                "Source project `{}` could not be loaded for materialized import.",
                candidate.name
            ),
            vec![source_context.err().unwrap().to_string()],
        ));
    }
    let source_context = source_context.ok();
    let mut fields = parse_project_import_fields(options.fields)?;
    let fields_requested_sources = fields.contains(&ProjectImportField::Sources);
    if options.include_public_sources || options.include_private_sources {
        fields.insert(ProjectImportField::Sources);
    }
    let source_summary = json!({
        "raw": project_import_partial_summary(&raw),
        "effective": source_context.as_ref().map(project_import_effective_summary).unwrap_or(Value::Null),
    });
    let mut warnings = Vec::new();
    let projected_config = project_import_project_config(
        mode,
        &fields,
        &raw,
        source_context.as_ref().map(|context| &context.config_file),
        fields_requested_sources,
        options.include_public_sources,
        options.include_private_sources,
        &mut warnings,
    )?;
    if mode == ProjectImportResolvedMode::ProfileBound && raw.extends_profile.is_some() {
        if source_context
            .as_ref()
            .and_then(|context| context.active_profile.as_ref())
            .is_none()
        {
            warnings.push(json!({
                "code": "profile_unavailable",
                "message": "The source uses extends_profile, but the profile did not resolve in this environment.",
                "profile": raw.extends_profile,
            }));
        }
    }
    let equivalence = project_import_equivalence(mode, &warnings);
    Ok(ProjectImportPlan {
        candidate,
        mode,
        fields,
        projected_config,
        equivalence,
        warnings,
        source_summary,
    })
}

fn project_import_plan_output(
    project_root: &Path,
    plan: &ProjectImportPlan,
    command_label: &str,
) -> CommandOutput {
    let fields = project_import_field_labels(&plan.fields);
    let mut lines = vec![
        "Import plan:".to_string(),
        format!("  Source: {} ({})", plan.candidate.name, plan.candidate.id),
        format!("  Path: {}", plan.candidate.path.display()),
        format!("  Mode: {}", plan.mode.as_str()),
        format!("  Fields: {}", fields.join(", ")),
        format!("  Equivalence: {}", plan.equivalence),
    ];
    if !plan.warnings.is_empty() {
        lines.push("  Warnings:".to_string());
        for warning in &plan.warnings {
            lines.push(format!(
                "    - {}",
                warning["code"].as_str().unwrap_or("warning")
            ));
        }
    }
    let command_selector = project_import_command_selector(&plan.candidate);
    lines.push(format!(
        "Next: metactl project import apply {} --yes",
        command_selector
    ));
    CommandOutput {
        human: project_human_output(project_root, lines.join("\n")),
        json: success_json(
            command_label,
            Some(project_root),
            json!({
                "action": "plan",
                "source": project_import_candidate_json(&plan.candidate),
                "mode": plan.mode.as_str(),
                "fields": fields,
                "equivalence": plan.equivalence,
                "projected_config": plan.projected_config,
                "source_summary": plan.source_summary,
                "warnings": plan.warnings,
                "next_commands": [
                    format!("metactl project import apply {} --yes", command_selector),
                    "metactl sync --preview".to_string(),
                ],
            }),
        ),
    }
}

fn apply_project_import_plan(
    cli: &Cli,
    project_root: &Path,
    plan: &ProjectImportPlan,
    apply_mode: ProjectImportApplyMode,
    yes: bool,
    command_label: &str,
) -> std::result::Result<CommandOutput, CliError> {
    confirm_project_import_write(cli, yes)?;
    ensure_project_layout(project_root).map_err(internal_error)?;
    ensure_gitignore_entries(project_root).map_err(internal_error)?;
    let config_path = project_config_path(project_root, cli.config.as_deref());
    let existing = if config_path.exists() {
        Some(load_partial_project_config(&config_path).map_err(state_error)?)
    } else {
        None
    };
    if existing.is_some() && apply_mode == ProjectImportApplyMode::Create {
        let command_selector = project_import_command_selector(&plan.candidate);
        return Err(project_import_error(
            "existing_config_requires_mode",
            format!("Project config {} already exists.", config_path.display()),
            vec![
                format!(
                    "Next: metactl project import apply {} --merge --yes",
                    command_selector
                ),
                format!(
                    "Next: metactl project import apply {} --replace --yes",
                    command_selector
                ),
            ],
        ));
    }
    let final_config = match (existing, apply_mode) {
        (Some(mut current), ProjectImportApplyMode::Merge) => {
            merge_project_import_config(&mut current, &plan.projected_config, &plan.fields);
            current
        }
        (Some(mut current), ProjectImportApplyMode::Replace) => {
            replace_project_import_config(&mut current, &plan.projected_config, &plan.fields);
            current
        }
        (None, _) => plan.projected_config.clone(),
        (Some(_), ProjectImportApplyMode::Create) => unreachable!(),
    };
    write_partial_project_config(&config_path, &final_config).map_err(internal_error)?;
    write_project_import_lock(project_root, &config_path, &final_config)?;
    Ok(CommandOutput {
        human: project_human_output(
            project_root,
            format!(
                "Import applied.\nSource: {} ({})\nMode: {}\nApply mode: {}\nConfig: {}\nNext: metactl sync --preview",
                plan.candidate.name,
                plan.candidate.id,
                plan.mode.as_str(),
                apply_mode.as_str(),
                config_path.display()
            ),
        ),
        json: success_json(
            command_label,
            Some(project_root),
            json!({
                "action": "apply",
                "applied": true,
                "apply_mode": apply_mode.as_str(),
                "mode": plan.mode.as_str(),
                "equivalence": plan.equivalence,
                "config_path": config_path,
                "source": project_import_candidate_json(&plan.candidate),
                "fields": project_import_field_labels(&plan.fields),
                "warnings": plan.warnings,
                "next_commands": ["metactl sync --preview", "metactl status"],
            }),
        ),
    })
}

fn project_import_apply_mode(
    merge: bool,
    replace: bool,
) -> std::result::Result<ProjectImportApplyMode, CliError> {
    match (merge, replace) {
        (true, true) => Err(project_import_error(
            "conflicting_apply_modes",
            "`--merge` and `--replace` cannot be used together.",
            vec!["Choose one apply mode.".to_string()],
        )),
        (true, false) => Ok(ProjectImportApplyMode::Merge),
        (false, true) => Ok(ProjectImportApplyMode::Replace),
        (false, false) => Ok(ProjectImportApplyMode::Create),
    }
}

fn confirm_project_import_write(cli: &Cli, yes: bool) -> std::result::Result<(), CliError> {
    if yes {
        return Ok(());
    }
    if cli.no_input_enabled() || !io::stdin().is_terminal() {
        return Err(project_import_error(
            "confirmation_required",
            "Non-interactive project import writes require --yes.",
            vec!["Rerun with --yes after reviewing `metactl project import plan`.".to_string()],
        ));
    }
    print!("Apply project import? [y/N] ");
    io::stdout()
        .flush()
        .map_err(|err| internal_error(anyhow!(err)))?;
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|err| internal_error(anyhow!(err)))?;
    if matches!(line.trim(), "y" | "Y" | "yes" | "YES") {
        Ok(())
    } else {
        Err(project_import_error(
            "cancelled",
            "Project import was cancelled.",
            Vec::new(),
        ))
    }
}

fn discover_project_import_candidates(
    cli: &Cli,
    project_root: &Path,
    search_roots: &[PathBuf],
) -> std::result::Result<Vec<ProjectImportCandidate>, CliError> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    let current_config = project_config_path(project_root, cli.config.as_deref());
    if current_config.exists() {
        if let Ok(context) = load_project_context(
            project_root,
            cli.config.as_deref(),
            cli.profile.as_deref(),
            cli.overlay.as_deref(),
        ) {
            push_linked_project_candidates(
                &mut candidates,
                &mut seen,
                project_root,
                &context.config_file,
                "current_project",
            );
        }
    }
    if let Ok(controller) = resolve_fleet_controller(cli) {
        push_linked_project_candidates(
            &mut candidates,
            &mut seen,
            &controller.project_root,
            &controller.context.config_file,
            "fleet_controller",
        );
    }
    for root in search_roots {
        let root = resolve_import_path(project_root, root);
        push_search_root_candidates(&mut candidates, &mut seen, &root)?;
    }
    candidates.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.id.cmp(&right.id))
            .then(left.path.cmp(&right.path))
    });
    Ok(candidates)
}

fn push_linked_project_candidates(
    candidates: &mut Vec<ProjectImportCandidate>,
    seen: &mut BTreeSet<String>,
    controller_root: &Path,
    config: &ProjectConfigFile,
    source: &'static str,
) {
    for project in fleet_projects_for_output(controller_root, config) {
        let status = linked_project_status_label(project.status);
        push_import_candidate(
            candidates,
            seen,
            ProjectImportCandidate {
                id: project.id.clone(),
                name: project
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(project.id.as_str())
                    .to_string(),
                path: project.path.clone(),
                config_path: project.config_path.clone(),
                profile: project.profile.clone(),
                status,
                source,
            },
        );
    }
}

fn push_search_root_candidates(
    candidates: &mut Vec<ProjectImportCandidate>,
    seen: &mut BTreeSet<String>,
    root: &Path,
) -> std::result::Result<(), CliError> {
    if !root.exists() {
        return Ok(());
    }
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let config_path = dir.join("metactl.yaml");
        if config_path.exists() {
            let name = dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("project")
                .to_string();
            push_import_candidate(
                candidates,
                seen,
                ProjectImportCandidate {
                    id: slugify_project_import_name(&name),
                    name,
                    path: dir.clone(),
                    config_path,
                    profile: None,
                    status: "ready",
                    source: "search_root",
                },
            );
            continue;
        }
        if depth >= 3 {
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && !is_hidden_import_scan_dir(&path) {
                stack.push((path, depth + 1));
            }
        }
    }
    Ok(())
}

fn resolve_project_import_candidate(
    cli: &Cli,
    project_root: &Path,
    source: &str,
    search_roots: &[PathBuf],
    include_unready: bool,
) -> std::result::Result<ProjectImportCandidate, CliError> {
    let candidates = discover_project_import_candidates(cli, project_root, search_roots)?;
    let source_lower = source.to_ascii_lowercase();
    let mut matches = candidates
        .iter()
        .filter(|candidate| {
            candidate.id.eq_ignore_ascii_case(source)
                || candidate.name.eq_ignore_ascii_case(source)
                || slugify_project_import_name(&candidate.name) == source_lower
                || candidate.path.to_string_lossy() == source
                || candidate
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.eq_ignore_ascii_case(source))
                    .unwrap_or(false)
        })
        .cloned()
        .collect::<Vec<_>>();
    if matches.is_empty() {
        if let Some(candidate) = direct_project_import_candidate(project_root, source) {
            matches.push(candidate);
        }
    }
    if matches.is_empty() {
        return Err(project_import_error(
            "source_not_found",
            format!("No metactl-managed project matched `{source}`."),
            candidates
                .iter()
                .take(8)
                .map(|candidate| format!("{} ({})", candidate.name, candidate.id))
                .collect(),
        ));
    }
    let ready_matches = matches
        .iter()
        .filter(|candidate| include_unready || candidate.status == "ready")
        .cloned()
        .collect::<Vec<_>>();
    if ready_matches.is_empty() {
        return Err(project_import_error(
            "source_not_ready",
            format!("Project `{source}` matched only unready projects."),
            matches
                .iter()
                .map(|candidate| format!("{}: {}", candidate.id, candidate.status))
                .collect(),
        ));
    }
    if ready_matches.len() > 1 {
        return Err(project_import_error(
            "ambiguous_source",
            format!("Project selector `{source}` matched multiple projects."),
            ready_matches
                .iter()
                .map(|candidate| format!("{}: {}", candidate.id, candidate.path.display()))
                .collect(),
        ));
    }
    Ok(ready_matches.into_iter().next().unwrap())
}

fn direct_project_import_candidate(
    project_root: &Path,
    source: &str,
) -> Option<ProjectImportCandidate> {
    let raw = resolve_import_path(project_root, Path::new(source));
    let (path, config_path) = if raw.is_file() {
        let parent = raw.parent()?.to_path_buf();
        (parent, raw.clone())
    } else {
        (raw.clone(), raw.join("metactl.yaml"))
    };
    if !path.exists() && !config_path.exists() {
        return None;
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project")
        .to_string();
    let status = if !path.exists() {
        "missing_path"
    } else if !config_path.exists() {
        "missing_config"
    } else {
        "ready"
    };
    Some(ProjectImportCandidate {
        id: slugify_project_import_name(&name),
        name,
        path,
        config_path,
        profile: None,
        status,
        source: "direct_path",
    })
}

fn push_import_candidate(
    candidates: &mut Vec<ProjectImportCandidate>,
    seen: &mut BTreeSet<String>,
    candidate: ProjectImportCandidate,
) {
    let key = candidate
        .path
        .canonicalize()
        .unwrap_or_else(|_| candidate.path.clone())
        .to_string_lossy()
        .to_string();
    if seen.insert(key) {
        candidates.push(candidate);
    }
}

fn resolve_import_path(project_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else if path.starts_with("~") {
        resolve_user_path(&path.to_string_lossy())
    } else {
        project_root.join(path)
    }
}

fn is_hidden_import_scan_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with('.') || name == "target" || name == "node_modules")
        .unwrap_or(false)
}

fn slugify_project_import_name(name: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn parse_project_import_fields(
    raw: Option<&str>,
) -> std::result::Result<BTreeSet<ProjectImportField>, CliError> {
    let mut fields = BTreeSet::new();
    if let Some(raw) = raw {
        for item in raw
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            let Some(field) = ProjectImportField::parse(item) else {
                return Err(project_import_error(
                    "unknown_import_field",
                    format!("Unknown import field `{item}`."),
                    vec![
                        "Allowed fields: role, policy, packs, targets, extends-profile, defaults, artifact-policy, sources, starter-library.".to_string(),
                    ],
                ));
            };
            fields.insert(field);
        }
        if fields.is_empty() {
            return Err(project_import_error(
                "empty_import_fields",
                "--fields cannot be empty.",
                Vec::new(),
            ));
        }
    } else {
        for field in [
            ProjectImportField::Role,
            ProjectImportField::Policy,
            ProjectImportField::Packs,
            ProjectImportField::Targets,
            ProjectImportField::ExtendsProfile,
            ProjectImportField::Defaults,
            ProjectImportField::ArtifactPolicy,
        ] {
            fields.insert(field);
        }
    }
    Ok(fields)
}

fn project_import_field_labels(fields: &BTreeSet<ProjectImportField>) -> Vec<&'static str> {
    fields.iter().map(|field| field.label()).collect()
}

fn resolve_project_import_mode(
    requested: ProjectImportModeArg,
    raw: &PartialProjectConfig,
) -> ProjectImportResolvedMode {
    match requested {
        ProjectImportModeArg::Auto => {
            if raw.extends_profile.is_some() {
                ProjectImportResolvedMode::ProfileBound
            } else {
                ProjectImportResolvedMode::Explicit
            }
        }
        ProjectImportModeArg::ProfileBound => ProjectImportResolvedMode::ProfileBound,
        ProjectImportModeArg::Explicit => ProjectImportResolvedMode::Explicit,
        ProjectImportModeArg::MaterializeEffective => {
            ProjectImportResolvedMode::MaterializeEffective
        }
    }
}

fn project_import_project_config(
    mode: ProjectImportResolvedMode,
    fields: &BTreeSet<ProjectImportField>,
    raw: &PartialProjectConfig,
    effective: Option<&ProjectConfigFile>,
    fields_requested_sources: bool,
    include_public_sources: bool,
    include_private_sources: bool,
    warnings: &mut Vec<Value>,
) -> std::result::Result<PartialProjectConfig, CliError> {
    let mut projected = PartialProjectConfig {
        api_version: Some(API_VERSION.to_string()),
        ..PartialProjectConfig::default()
    };
    match mode {
        ProjectImportResolvedMode::ProfileBound | ProjectImportResolvedMode::Explicit => {
            if mode == ProjectImportResolvedMode::ProfileBound
                && fields.contains(&ProjectImportField::ExtendsProfile)
            {
                projected.extends_profile = raw.extends_profile.clone();
            }
            if fields.contains(&ProjectImportField::Role) {
                projected.role = raw.role.clone();
            }
            if fields.contains(&ProjectImportField::Policy) {
                projected.policy = raw.policy.clone();
            }
            if fields.contains(&ProjectImportField::Packs) {
                projected.packs = raw.packs.clone();
            }
            if fields.contains(&ProjectImportField::Targets) {
                projected.targets = raw.targets.clone();
            }
            if fields.contains(&ProjectImportField::Defaults) {
                projected.defaults = raw.defaults.clone();
            }
            if fields.contains(&ProjectImportField::StarterLibrary) {
                projected.starter_library = raw.starter_library.clone();
            }
            if fields.contains(&ProjectImportField::ArtifactPolicy) {
                if let Some(policy) = raw.metadata.get(AGENT_ARTIFACT_POLICY_METADATA_KEY) {
                    projected.metadata.insert(
                        AGENT_ARTIFACT_POLICY_METADATA_KEY.to_string(),
                        policy.clone(),
                    );
                }
            }
            copy_import_sources(
                &mut projected,
                &raw.sources,
                fields_requested_sources,
                include_public_sources,
                include_private_sources,
                warnings,
            );
        }
        ProjectImportResolvedMode::MaterializeEffective => {
            let Some(effective) = effective else {
                return Err(project_import_error(
                    "source_context_unavailable",
                    "Materialized import requires a loadable source project context.",
                    Vec::new(),
                ));
            };
            if fields.contains(&ProjectImportField::Role) {
                projected.role = Some(effective.role.clone());
            }
            if fields.contains(&ProjectImportField::Policy) {
                projected.policy = Some(effective.policy.clone());
            }
            if fields.contains(&ProjectImportField::Packs) {
                projected.packs = effective.packs.clone();
            }
            if fields.contains(&ProjectImportField::Targets) {
                projected.targets = effective.targets.clone();
            }
            if fields.contains(&ProjectImportField::Defaults) {
                projected.defaults = effective.defaults.clone();
            }
            if fields.contains(&ProjectImportField::StarterLibrary) {
                projected.starter_library = effective.starter_library.clone();
            }
            if fields.contains(&ProjectImportField::ArtifactPolicy) {
                if let Some(policy) = effective.metadata.get(AGENT_ARTIFACT_POLICY_METADATA_KEY) {
                    projected.metadata.insert(
                        AGENT_ARTIFACT_POLICY_METADATA_KEY.to_string(),
                        policy.clone(),
                    );
                }
            }
            copy_import_sources(
                &mut projected,
                &effective.sources,
                fields_requested_sources,
                include_public_sources,
                include_private_sources,
                warnings,
            );
        }
    }
    Ok(projected)
}

fn copy_import_sources(
    projected: &mut PartialProjectConfig,
    sources: &[SourceRecord],
    fields_requested_sources: bool,
    include_public_sources: bool,
    include_private_sources: bool,
    warnings: &mut Vec<Value>,
) {
    let requested_sources =
        fields_requested_sources || include_public_sources || include_private_sources;
    if !requested_sources {
        if !sources.is_empty() {
            warnings.push(json!({
                "code": "source_omitted",
                "message": "Source records are not copied by default. Use --include-public-sources or --include-private-sources.",
                "omitted": sources.len(),
            }));
        }
        return;
    }
    let mut omitted_private = 0usize;
    let mut omitted_public = 0usize;
    for source in sources {
        let copy_private = include_private_sources;
        let copy_public = include_public_sources || fields_requested_sources;
        let should_copy = match source.visibility {
            SourceVisibility::Private => copy_private,
            SourceVisibility::Public => copy_public,
        };
        if should_copy {
            projected.sources.push(source.clone());
        } else if source.visibility == SourceVisibility::Private {
            omitted_private += 1;
        } else {
            omitted_public += 1;
        }
    }
    if omitted_private > 0 {
        warnings.push(json!({
            "code": "private_source_blocked",
            "message": "Private source records were omitted. Use --include-private-sources only when the destination should reference the same private library.",
            "omitted_private_sources": omitted_private,
        }));
    }
    if omitted_public > 0 {
        warnings.push(json!({
            "code": "source_omitted",
            "message": "Public source records were omitted.",
            "omitted_public_sources": omitted_public,
        }));
    }
}

fn project_import_equivalence(mode: ProjectImportResolvedMode, warnings: &[Value]) -> &'static str {
    if warnings
        .iter()
        .any(|warning| warning["code"] == "profile_unavailable")
    {
        "profile_unavailable"
    } else if warnings
        .iter()
        .any(|warning| warning["code"] == "private_source_blocked")
    {
        "private_source_blocked"
    } else if warnings
        .iter()
        .any(|warning| warning["code"] == "source_omitted")
    {
        "source_omitted"
    } else if mode == ProjectImportResolvedMode::ProfileBound {
        "profile_bound"
    } else {
        "equivalent"
    }
}

fn merge_project_import_config(
    current: &mut PartialProjectConfig,
    imported: &PartialProjectConfig,
    fields: &BTreeSet<ProjectImportField>,
) {
    if fields.contains(&ProjectImportField::ExtendsProfile) && current.extends_profile.is_none() {
        current.extends_profile = imported.extends_profile.clone();
    }
    if fields.contains(&ProjectImportField::Role) && current.role.is_none() {
        current.role = imported.role.clone();
    }
    if fields.contains(&ProjectImportField::Policy) && current.policy.is_none() {
        current.policy = imported.policy.clone();
    }
    if fields.contains(&ProjectImportField::Packs) {
        push_unique_strings(&mut current.packs, &imported.packs);
    }
    if fields.contains(&ProjectImportField::Targets) {
        push_unique_strings(&mut current.targets, &imported.targets);
    }
    if fields.contains(&ProjectImportField::StarterLibrary) {
        push_unique_strings(&mut current.starter_library, &imported.starter_library);
    }
    if fields.contains(&ProjectImportField::Sources) {
        push_unique_sources(&mut current.sources, &imported.sources);
    }
    if fields.contains(&ProjectImportField::Defaults) {
        merge_project_config_defaults(&mut current.defaults, imported.defaults.clone());
    }
    if fields.contains(&ProjectImportField::ArtifactPolicy) {
        if let Some(policy) = imported.metadata.get(AGENT_ARTIFACT_POLICY_METADATA_KEY) {
            current
                .metadata
                .entry(AGENT_ARTIFACT_POLICY_METADATA_KEY.to_string())
                .or_insert_with(|| policy.clone());
        }
    }
    if current.api_version.is_none() {
        current.api_version = imported.api_version.clone();
    }
}

fn replace_project_import_config(
    current: &mut PartialProjectConfig,
    imported: &PartialProjectConfig,
    fields: &BTreeSet<ProjectImportField>,
) {
    if fields.contains(&ProjectImportField::ExtendsProfile) {
        current.extends_profile = imported.extends_profile.clone();
    }
    if fields.contains(&ProjectImportField::Role) {
        current.role = imported.role.clone();
    }
    if fields.contains(&ProjectImportField::Policy) {
        current.policy = imported.policy.clone();
    }
    if fields.contains(&ProjectImportField::Packs) {
        current.packs = imported.packs.clone();
    }
    if fields.contains(&ProjectImportField::Targets) {
        current.targets = imported.targets.clone();
    }
    if fields.contains(&ProjectImportField::StarterLibrary) {
        current.starter_library = imported.starter_library.clone();
    }
    if fields.contains(&ProjectImportField::Sources) {
        current.sources = imported.sources.clone();
    }
    if fields.contains(&ProjectImportField::Defaults) {
        current.defaults = imported.defaults.clone();
    }
    if fields.contains(&ProjectImportField::ArtifactPolicy) {
        current.metadata.remove(AGENT_ARTIFACT_POLICY_METADATA_KEY);
        if let Some(policy) = imported.metadata.get(AGENT_ARTIFACT_POLICY_METADATA_KEY) {
            current.metadata.insert(
                AGENT_ARTIFACT_POLICY_METADATA_KEY.to_string(),
                policy.clone(),
            );
        }
    }
    if current.api_version.is_none() {
        current.api_version = imported.api_version.clone();
    }
}

fn push_unique_strings(target: &mut Vec<String>, values: &[String]) {
    for value in values {
        if !target.iter().any(|item| item == value) {
            target.push(value.clone());
        }
    }
}

fn push_unique_sources(target: &mut Vec<SourceRecord>, values: &[SourceRecord]) {
    for value in values {
        if !target.iter().any(|item| item.id == value.id) {
            target.push(value.clone());
        }
    }
}

fn merge_project_config_defaults(
    target: &mut Option<ProjectConfigDefaults>,
    imported: Option<ProjectConfigDefaults>,
) {
    let Some(imported) = imported else {
        return;
    };
    match target {
        Some(target) => {
            if target.brownfield_mode.is_none() {
                target.brownfield_mode = imported.brownfield_mode;
            }
            if target.fleet_sync_adopt.is_none() {
                target.fleet_sync_adopt = imported.fleet_sync_adopt;
            }
            if target.discovery_mode.is_none() {
                target.discovery_mode = imported.discovery_mode;
            }
            if target.surface_selection_mode.is_none() {
                target.surface_selection_mode = imported.surface_selection_mode;
            }
        }
        None => *target = Some(imported),
    }
}

fn write_project_import_lock(
    project_root: &Path,
    config_path: &Path,
    config: &PartialProjectConfig,
) -> std::result::Result<(), CliError> {
    let lock_path = project_lock_path(project_root);
    let mut lock = load_lock(&lock_path).map_err(internal_error)?;
    lock.config_digest = Some(digest_path(config_path).map_err(internal_error)?);
    lock.profile_name = config.extends_profile.clone();
    lock.profile_path = context_profile_path(config.extends_profile.as_deref());
    lock.profile_digest =
        context_profile_digest(config.extends_profile.as_deref()).map_err(internal_error)?;
    lock.sources.clear();
    lock.targets.clear();
    lock.updated_at = Some(now_string());
    write_lock(&lock_path, &lock).map_err(internal_error)?;
    Ok(())
}

fn project_import_partial_summary(config: &PartialProjectConfig) -> Value {
    let private_sources = config
        .sources
        .iter()
        .filter(|source| source.visibility == SourceVisibility::Private)
        .count();
    json!({
        "extends_profile": config.extends_profile,
        "role": config.role,
        "policy": config.policy,
        "packs": config.packs,
        "targets": config.targets,
        "starter_library_count": config.starter_library.len(),
        "sources_count": config.sources.len(),
        "private_sources_count": private_sources,
        "defaults_present": config.defaults.is_some(),
        "artifact_policy": config.metadata.get(AGENT_ARTIFACT_POLICY_METADATA_KEY),
    })
}

fn project_import_effective_summary(context: &metactl::project::ProjectContext) -> Value {
    let private_sources = context
        .config_file
        .sources
        .iter()
        .filter(|source| source.visibility == SourceVisibility::Private)
        .count();
    json!({
        "active_profile": context.active_profile.as_ref().map(|profile| profile.name.clone()),
        "role": context.config_file.role,
        "policy": context.config_file.policy,
        "packs": context.config_file.packs,
        "targets": context.config_file.targets,
        "starter_library_count": context.config_file.starter_library.len(),
        "sources_count": context.config_file.sources.len(),
        "private_sources_count": private_sources,
        "defaults_present": context.config_file.defaults.is_some(),
        "artifact_policy": context.config_file.metadata.get(AGENT_ARTIFACT_POLICY_METADATA_KEY),
    })
}

fn project_import_candidate_json(candidate: &ProjectImportCandidate) -> Value {
    json!({
        "id": candidate.id,
        "name": candidate.name,
        "path": candidate.path.to_string_lossy(),
        "config_path": candidate.config_path.to_string_lossy(),
        "profile": candidate.profile,
        "status": candidate.status,
        "source": candidate.source,
    })
}

fn project_import_command_selector(candidate: &ProjectImportCandidate) -> String {
    match candidate.source {
        "direct_path" | "search_root" => candidate.path.to_string_lossy().to_string(),
        _ => candidate.id.clone(),
    }
}

pub(super) fn project_import_error(
    code: &str,
    message: impl Into<String>,
    details: Vec<String>,
) -> CliError {
    let mut err = CliError::new(EXIT_STATE, message).with_details(details);
    if let Some(obj) = err.json.as_object_mut() {
        obj.insert("code".to_string(), json!(code));
        obj.insert("category".to_string(), json!("project_import"));
    }
    err
}
