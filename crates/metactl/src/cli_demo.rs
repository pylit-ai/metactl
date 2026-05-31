use super::*;

pub(super) fn cmd_demo(cli: &Cli, args: &DemoArgs) -> std::result::Result<CommandOutput, CliError> {
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
