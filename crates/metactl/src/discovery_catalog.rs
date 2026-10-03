//! Explicit discovery-only user catalog. Never fabricates a ProjectContext.
use super::*;
use metactl::{Config, InvocationOverlay};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UserCatalog {
    schema: u32,
    fallback_enabled: bool,
    sources: Vec<PathBuf>,
    role: String,
    policy: String,
    targets: Vec<String>,
    exclusions: Vec<String>,
    metadata_policy: String,
}

pub(super) struct Resolved {
    pub registry: LibraryRegistry,
    pub config: Config,
    pub overlay: Option<InvocationOverlay>,
    pub exclusions: Vec<String>,
    pub context: Value,
}

fn catalog_path() -> Result<PathBuf, CliError> {
    let path = metactl_user_config_dir()
        .ok_or_else(|| CliError::new(EXIT_STATE, "Cannot resolve user catalog directory."))?
        .join("discovery-catalog.json");
    if !path.is_absolute() {
        return Err(CliError::new(
            EXIT_STATE,
            "User catalog directory must be absolute.",
        ));
    }
    Ok(path)
}

fn validate(
    doc: &UserCatalog,
    path: &Path,
    target: &str,
) -> Result<(LibraryRegistry, Config), CliError> {
    if doc.schema != 1
        || doc.sources.is_empty()
        || doc.sources.len() > 32
        || doc.targets.is_empty()
        || !["local-only", "public-nonsensitive", "private-owned"]
            .contains(&doc.metadata_policy.as_str())
        || !doc.targets.iter().any(|id| id == target)
    {
        return Err(CliError::new(EXIT_STATE, "user_catalog_invalid: Check schema, enabled flag, sources, target and metadata policy."));
    }
    let roots = doc
        .sources
        .iter()
        .map(|source| {
            let root = if source.is_absolute() {
                source.clone()
            } else {
                path.parent().unwrap().join(source)
            };
            // An explicit source is mandatory; never substitute bundled/default libraries.
            let root = root.canonicalize().map_err(|_| {
                CliError::new(
                    EXIT_STATE,
                    "user_catalog_invalid: A declared source is unavailable.",
                )
            })?;
            if !root.is_dir() || !root.join("library.json").is_file() {
                return Err(CliError::new(
                    EXIT_STATE,
                    "user_catalog_invalid: Each source must be a local library with library.json.",
                ));
            }
            Ok(root)
        })
        .collect::<Result<Vec<_>, CliError>>()?;
    let registry = LibraryRegistry::load_from_roots(&roots).map_err(|_| {
        CliError::new(
            EXIT_STATE,
            "user_catalog_invalid: A declared library failed validation.",
        )
    })?;
    let role = registry
        .role_by_id(&doc.role)
        .ok_or_else(|| CliError::new(EXIT_STATE, "user_catalog_invalid: Unknown role."))?;
    let policy = registry
        .policy_by_id(&doc.policy)
        .ok_or_else(|| CliError::new(EXIT_STATE, "user_catalog_invalid: Unknown policy."))?;
    // Validate every saved target, then select exactly the concrete request target.
    for id in &doc.targets {
        if registry.target_by_id(id).is_none() {
            return Err(CliError::new(
                EXIT_STATE,
                "user_catalog_invalid: Unknown target.",
            ));
        }
    }
    let target = registry.target_by_id(target).unwrap();
    let config = Config {
        api_version: API_VERSION.into(),
        role: role.role_ref(),
        packs: vec![],
        policy: policy.policy_ref(),
        targets: vec![target.target_ref()],
        defaults: None,
        metadata: BTreeMap::new(),
    };
    Ok((registry, config))
}

fn identity(
    root: &Path,
    origin: &str,
    config_path: &Path,
    target: Option<&str>,
    policy: &str,
) -> Value {
    let opaque = hex::encode(Sha256::digest(format!(
        "{}\0{origin}\0{}",
        root.display(),
        config_path.display()
    )));
    json!({"catalog_origin": origin, "catalog_ready": true,
        "project_config_state": if origin == "project" {"present"} else {"absent"},
        "workspace_resolution": "exact", "context_identity": opaque,
        "effective_target": target, "metadata_policy": policy})
}

pub(super) fn resolve(cli: &Cli, root: &Path) -> Result<Resolved, CliError> {
    let root = root.canonicalize().map_err(internal_error)?;
    let project_path = project_config_path(&root, cli.config.as_deref());
    // symlink_metadata distinguishes genuine absence from dangling links/read errors.
    let exists = match fs::symlink_metadata(&project_path) {
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(_) => {
            return Err(CliError::new(
                EXIT_STATE,
                "project_discovery_failed: Cannot inspect the selected configuration.",
            ))
        }
    };
    if exists && project_path.canonicalize().is_err() {
        return Err(CliError::new(
            EXIT_STATE,
            "project_discovery_failed: Selected configuration is inaccessible or dangling.",
        ));
    }
    if cli.config.is_some() || exists || cli.catalog_mode != "project-or-user" {
        let context = load_required_context(cli, &root)?;
        let config = context
            .effective_config(&ConfigOverrides::default())
            .map_err(state_error)?;
        let registry = context
            .registry
            .ok_or_else(|| CliError::new(EXIT_STATE, "No configured library"))?;
        let target = config.targets.first().map(|target| target.id.as_str());
        let descriptor = identity(&root, "project", &project_path, target, "project-grant");
        return Ok(Resolved {
            registry,
            config,
            overlay: context.overlay,
            exclusions: vec![],
            context: descriptor,
        });
    }
    if cli.profile.is_some() || cli.overlay.is_some() {
        return Err(CliError::new(EXIT_CONFLICT, "user_catalog_conflict: Explicit project profile/overlay requires project configuration."));
    }
    let path = catalog_path()?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(missing_config_error(cli, &root))
        }
        Err(_) => {
            return Err(CliError::new(
                EXIT_STATE,
                "user_catalog_invalid: Cannot inspect saved user catalog.",
            ))
        }
    };
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err(CliError::new(
            EXIT_STATE,
            "user_catalog_invalid: Catalog must be a regular bounded JSON file.",
        ));
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(CliError::new(
            EXIT_STATE,
            "user_catalog_invalid: Catalog permissions must be 0600.",
        ));
    }
    let doc: UserCatalog = serde_json::from_slice(&fs::read(&path).map_err(internal_error)?)
        .map_err(|_| {
            CliError::new(
                EXIT_STATE,
                "user_catalog_invalid: Invalid saved catalog JSON.",
            )
        })?;
    if !doc.fallback_enabled {
        return Err(missing_config_error(cli, &root));
    }
    let target = cli.discovery_target.as_deref().ok_or_else(|| {
        CliError::new(
            EXIT_STATE,
            "user_catalog_target_required: Pass --discovery-target with a concrete runtime target.",
        )
    })?;
    let (registry, config) = validate(&doc, &path, target)?;
    let mut descriptor = identity(&root, "user", &path, Some(target), &doc.metadata_policy);
    // Bind the configuration snapshot, not source bytes: ordinary content edits
    // revalidate package digests; replacing sources/policy/target needs a fresh host.
    descriptor["context_identity"] = json!(hex::encode(Sha256::digest(
        serde_json::to_vec(&(root, &path, &doc)).map_err(internal_error)?
    )));

    Ok(Resolved {
        registry,
        config,
        overlay: None,
        exclusions: doc.exclusions,
        context: descriptor,
    })
}

pub(super) fn setup(_cli: &Cli, args: &SkillsSetupArgs) -> Result<CommandOutput, CliError> {
    if args.scope != DiscoveryScopeArg::User {
        return Err(CliError::new(
            EXIT_VALIDATION,
            "skills setup supports --scope user only; use metactl setup for a project.",
        ));
    }
    let path = catalog_path()?;
    let mut doc = if args.disable || args.enable {
        if !args.source.is_empty()
            || !args.target.is_empty()
            || !args.exclude.is_empty()
            || args.role != "builder"
            || args.policy != "brownfield-safe-builder"
            || args.metadata_policy != "local-only"
        {
            return Err(CliError::new(
                EXIT_CONFLICT,
                "Catalog toggles preserve saved settings; omit source, target and policy options.",
            ));
        }
        let metadata = fs::symlink_metadata(&path).map_err(|_| {
            CliError::new(
                EXIT_STATE,
                "No readable saved user catalog. Preview explicit --source and --target first.",
            )
        })?;
        if !metadata.is_file() || metadata.len() > 1024 * 1024 {
            return Err(CliError::new(
                EXIT_STATE,
                "user_catalog_invalid: Saved catalog must be regular and bounded.",
            ));
        }
        serde_json::from_slice::<UserCatalog>(&fs::read(&path).map_err(internal_error)?).map_err(
            |_| {
                CliError::new(
                    EXIT_STATE,
                    "user_catalog_invalid: Invalid saved catalog JSON.",
                )
            },
        )?
    } else {
        UserCatalog {
            schema: 1,
            fallback_enabled: true,
            sources: args
                .source
                .iter()
                .map(|p| p.canonicalize().map_err(internal_error))
                .collect::<Result<Vec<_>, _>>()?,
            role: args.role.clone(),
            policy: args.policy.clone(),
            targets: args.target.clone(),
            exclusions: args.exclude.clone(),
            metadata_policy: args.metadata_policy.clone(),
        }
    };
    if args.disable || args.enable {
        doc.fallback_enabled = args.enable;
    }
    let mut eligible = BTreeMap::new();
    for target in &doc.targets {
        let (registry, config) = validate(&doc, &path, target)?;
        let count = registry
            .skill_catalog(&config, None)
            .map_err(state_error)?
            .skills
            .iter()
            .filter(|s| !doc.exclusions.contains(&s.id) && !doc.exclusions.contains(&s.name))
            .count();
        eligible.insert(target, count);
    }
    let existing = fs::symlink_metadata(&path).ok();
    if args.apply {
        if existing.is_some() && !args.replace && !(args.disable || args.enable) {
            return Err(CliError::new(
                EXIT_CONFLICT,
                "A saved user catalog exists; preview changes and pass --replace --apply.",
            ));
        }
        if existing
            .as_ref()
            .is_some_and(|metadata| !metadata.is_file())
        {
            return Err(CliError::new(
                EXIT_CONFLICT,
                "Refusing to replace a non-regular user catalog.",
            ));
        }
        let directory = path.parent().unwrap();
        if directory.is_symlink() {
            return Err(CliError::new(
                EXIT_STATE,
                "user_catalog_invalid: Symlink catalog directories are not supported.",
            ));
        }
        fs::create_dir_all(directory).map_err(internal_error)?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory).map_err(internal_error)?;
        #[cfg(unix)]
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(internal_error)?;
        temporary
            .write_all(&serde_json::to_vec_pretty(&doc).map_err(internal_error)?)
            .map_err(internal_error)?;
        temporary.as_file().sync_all().map_err(internal_error)?;
        if args.replace || args.disable || args.enable {
            temporary.persist(&path).map_err(internal_error)?;
        } else {
            temporary.persist_noclobber(&path).map_err(|_| {
                CliError::new(
                    EXIT_CONFLICT,
                    "User catalog was created concurrently; preview again before replacing.",
                )
            })?;
        }
    }
    let value = json!({"action": if args.apply {"saved"} else {"preview"}, "catalog_path": path,
        "catalog_origin": "user", "fallback_enabled": doc.fallback_enabled, "sources": doc.sources, "targets": doc.targets, "eligible_skills": eligible,
        "metadata_policy": doc.metadata_policy, "workspace_enrollment_changed": false,
        "visibility": "Selected skill text is visible to the coding agent across explicitly connected workspaces.",
        "provider": "Metadata classification never grants Jev permission; exact workspace enrollment and saved preferences are still required.",
        "next": "Use --catalog-mode project-or-user --discovery-target TARGET skills catalog, or skills host --target TARGET --use-preferences.",
        "disable": "metactl skills setup --disable --apply (enable again with --enable --apply)."});
    Ok(CommandOutput {
        human: serde_json::to_string_pretty(&value).map_err(internal_error)?,
        json: success_json("skills", None, value),
    })
}
