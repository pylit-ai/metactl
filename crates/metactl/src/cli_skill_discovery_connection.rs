use super::*;
use sha2::{Digest, Sha256};
use std::process::Command;

const SERVER: &str = "metactl-skills";
const BEGIN: &str = "# metactl-discovery:begin";
const END: &str = "# metactl-discovery:end";

fn target_path(root: &Path, target: &str, scope: DiscoveryScopeArg) -> Result<PathBuf, CliError> {
    let relative = match target {
        "codex-cli" => ".codex/config.toml",
        "claude-code" => ".mcp.json",
        "cursor" => ".cursor/mcp.json",
        "gemini-cli" => ".gemini/settings.json",
        "opencode" => "opencode.json",
        "openclaw" | "filesystem-agent" | "pi" | "omnigent" => {
            return Err(CliError::new(EXIT_VALIDATION, format!(
                "{target} has no verified automatic project registration. Use the manual adapter in docs/user/discovery-agent-adapters.md."
            )))
        }
        _ => return Err(CliError::new(EXIT_VALIDATION, format!("Unknown discovery target '{target}'."))),
    };
    if scope == DiscoveryScopeArg::User {
        if target != "codex-cli" {
            return Err(CliError::new(
                EXIT_VALIDATION,
                "User scope is supported only for codex-cli; choose --scope project.",
            ));
        }
        let home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|path| path.join(".codex")))
            .ok_or_else(|| CliError::new(EXIT_STATE, "Cannot resolve Codex home directory."))?;
        return Ok(home.join("config.toml"));
    }
    Ok(root.join(relative))
}

fn ledger_path(root: &Path) -> Result<PathBuf, CliError> {
    let home = home_dir().ok_or_else(|| {
        CliError::new(
            EXIT_STATE,
            "Cannot resolve home directory for private discovery log.",
        )
    })?;
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/state"));
    if !state.is_absolute() {
        return Err(CliError::new(
            EXIT_STATE,
            "XDG_STATE_HOME must be an absolute path for the private discovery log.",
        ));
    }
    let mut absent = Vec::new();
    let mut prefix = state.as_path();
    while !prefix.exists() {
        absent.push(
            prefix
                .file_name()
                .ok_or_else(|| CliError::new(EXIT_STATE, "Invalid state directory."))?
                .to_os_string(),
        );
        prefix = prefix
            .parent()
            .ok_or_else(|| CliError::new(EXIT_STATE, "Invalid state directory."))?;
    }
    let mut state = prefix.canonicalize().map_err(internal_error)?;
    for name in absent.into_iter().rev() {
        state.push(name);
    }
    let digest = hex::encode(Sha256::digest(root.to_string_lossy().as_bytes()));
    Ok(state
        .join("metactl/discovery")
        .join(format!("{}.jsonl", &digest[..16])))
}

fn host_command(
    cli: &Cli,
    root: &Path,
    target: &str,
    ledger: &Path,
    python: &Path,
) -> Result<(String, Vec<String>), CliError> {
    let exe = std::env::current_exe().map_err(internal_error)?;
    let python = if python.components().count() > 1 {
        python.canonicalize().map_err(internal_error)?
    } else {
        python.to_path_buf()
    };
    let mut args = vec!["--project".into(), root.to_string_lossy().into_owned()];
    if cli.no_profile {
        args.push("--no-profile".into());
    }
    if let Some(profile) = &cli.profile {
        args.extend(["--profile".into(), profile.clone()]);
    }
    for (flag, path) in [("--config", &cli.config), ("--overlay", &cli.overlay)] {
        if let Some(path) = path {
            let absolute = path.canonicalize().map_err(internal_error)?;
            args.extend([flag.into(), absolute.to_string_lossy().into_owned()]);
        }
    }
    args.extend([
        "skills".into(),
        "host".into(),
        "--python".into(),
        python.to_string_lossy().into_owned(),
        "--ranker".into(),
        "deterministic".into(),
        "--runtime".into(),
        target.into(),
        "--trial-mode".into(),
        "baseline".into(),
        "--event-log".into(),
        ledger.to_string_lossy().into_owned(),
    ]);
    Ok((exe.to_string_lossy().into_owned(), args))
}

fn expected_entry(target: &str, command: &str, args: &[String]) -> Value {
    if target == "opencode" {
        let mut vector = vec![command.to_string()];
        vector.extend_from_slice(args);
        json!({"type": "local", "enabled": true, "command": vector})
    } else {
        json!({"command": command, "args": args})
    }
}

fn entry_key(target: &str) -> &'static str {
    if target == "opencode" {
        "mcp"
    } else {
        "mcpServers"
    }
}

fn codex_block(command: &str, args: &[String]) -> String {
    let command = serde_json::to_string(command).unwrap_or_default();
    let args = serde_json::to_string(args).unwrap_or_default();
    format!("{BEGIN}\n[mcp_servers.{SERVER}]\ncommand = {command}\nargs = {args}\n{END}\n")
}

fn read_config(path: &Path) -> Result<Option<String>, CliError> {
    if !path.exists() {
        return Ok(None);
    }
    if path
        .symlink_metadata()
        .map_err(internal_error)?
        .file_type()
        .is_symlink()
    {
        return Err(CliError::new(
            EXIT_STATE,
            "Configuration is a symlink; refusing to edit it.",
        ));
    }
    fs::read_to_string(path).map(Some).map_err(internal_error)
}

fn ensure_safe_destination(path: &Path) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        if parent.exists()
            && parent
                .symlink_metadata()
                .map_err(internal_error)?
                .file_type()
                .is_symlink()
        {
            return Err(CliError::new(
                EXIT_STATE,
                "Configuration directory is a symlink; refusing to edit it.",
            ));
        }
    }
    Ok(())
}

fn tracked(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "--error-unmatch", "--"])
        .arg(relative)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn write_private(path: &Path, body: &str) -> Result<(), CliError> {
    let parent = path
        .parent()
        .ok_or_else(|| CliError::new(EXIT_STATE, "Invalid configuration path."))?;
    let parent_existed = parent.exists();
    fs::create_dir_all(parent).map_err(internal_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !parent_existed {
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                .map_err(internal_error)?;
        }
    }
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(internal_error)?;
    use std::io::Write;
    temp.write_all(body.as_bytes()).map_err(internal_error)?;
    temp.flush().map_err(internal_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(internal_error)?;
    }
    temp.persist(path).map_err(internal_error)?;
    Ok(())
}

fn managed_codex_block<'a>(
    old: &'a str,
    expected_args: &[String],
) -> Result<Option<(usize, usize, &'a str)>, CliError> {
    let header = format!("[mcp_servers.{SERVER}]");
    let starts = old.matches(BEGIN).count();
    let ends = old.matches(END).count();
    if starts != ends || starts > 1 {
        return Err(CliError::new(
            EXIT_STATE,
            "Malformed MetaCTL discovery marker in Codex config.",
        ));
    }
    if starts == 1 {
        let begin = old.find(BEGIN).unwrap();
        let end = old.find(END).unwrap() + END.len();
        if begin >= end {
            return Err(CliError::new(
                EXIT_STATE,
                "MetaCTL discovery block was changed; review it manually.",
            ));
        }
        let block = &old[begin..end];
        let lines: Vec<_> = block.lines().collect();
        let command = lines
            .get(2)
            .and_then(|line| line.strip_prefix("command = "))
            .and_then(|value| serde_json::from_str::<String>(value).ok());
        let old_args = lines
            .get(3)
            .and_then(|line| line.strip_prefix("args = "))
            .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok());
        let valid_args = old_args.as_ref().is_some_and(|values| {
            values.len() == expected_args.len()
                && values[..values.len() - 1] == expected_args[..expected_args.len() - 1]
                && Path::new(values.last().unwrap()).is_absolute()
        });
        if lines.len() != 5
            || lines[0] != BEGIN
            || lines[1] != header
            || lines[4] != END
            || command.is_none()
            || !valid_args
        {
            return Err(CliError::new(EXIT_STATE, "MetaCTL discovery block differs from the expected registration; review it manually."));
        }
        return Ok(Some((begin, end, block)));
    }
    Ok(None)
}

fn edit_codex(
    old: &str,
    block: &str,
    args: &[String],
    remove: bool,
) -> Result<(String, &'static str), CliError> {
    let header = format!("[mcp_servers.{SERVER}]");
    if let Some((begin, end, existing)) = managed_codex_block(old, args)? {
        if remove {
            let suffix = if old[end..].starts_with('\n') {
                end + 1
            } else {
                end
            };
            return Ok((format!("{}{}", &old[..begin], &old[suffix..]), "removed"));
        }
        if existing == block.trim_end() {
            return Ok((old.to_owned(), "already_connected"));
        }
        return Ok((
            format!("{}{}{}", &old[..begin], block.trim_end(), &old[end..]),
            "updated",
        ));
    }
    if remove {
        return Ok((old.to_owned(), "already_absent"));
    }
    if old.contains(&header) {
        return Err(CliError::new(EXIT_STATE, "An unmanaged metactl-skills server already exists in Codex config; review it manually."));
    }
    let separator = if old.is_empty() || old.ends_with("\n\n") {
        ""
    } else if old.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    Ok((format!("{old}{separator}{block}"), "connected"))
}

fn edit_json(
    old: &str,
    target: &str,
    expected: Value,
    remove: bool,
) -> Result<(String, &'static str), CliError> {
    let mut document: Value = if old.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(old).map_err(|_| {
            CliError::new(
                EXIT_STATE,
                "Target configuration is not valid JSON; refusing to edit it.",
            )
        })?
    };
    let map = document
        .as_object_mut()
        .ok_or_else(|| CliError::new(EXIT_STATE, "Target configuration must be a JSON object."))?;
    let servers = map.entry(entry_key(target)).or_insert_with(|| json!({}));
    let servers = servers.as_object_mut().ok_or_else(|| {
        CliError::new(
            EXIT_STATE,
            "Target MCP configuration must be a JSON object.",
        )
    })?;
    match servers.get(SERVER) {
        Some(value) if value != &expected => {
            return Err(CliError::new(
                EXIT_STATE,
                "Existing metactl-skills server differs; review it manually.",
            ))
        }
        Some(_) if remove => {
            servers.remove(SERVER);
        }
        Some(_) => return Ok((old.to_owned(), "already_connected")),
        None if remove => return Ok((old.to_owned(), "already_absent")),
        None => {
            servers.insert(SERVER.into(), expected);
        }
    }
    let action = if remove { "removed" } else { "connected" };
    let body = serde_json::to_string_pretty(&document).map_err(internal_error)? + "\n";
    Ok((body, action))
}

fn connection(
    cli: &Cli,
    target: &str,
    scope: DiscoveryScopeArg,
    python: &Path,
) -> Result<(PathBuf, PathBuf, PathBuf, String, Vec<String>), CliError> {
    let root = project_root(cli)
        .map_err(internal_error)?
        .canonicalize()
        .map_err(internal_error)?;
    let path = target_path(&root, target, scope)?;
    let ledger = ledger_path(&root)?;
    let (command, args) = host_command(cli, &root, target, &ledger, python)?;
    Ok((root, path, ledger, command, args))
}

fn offline_status(command: &str, args: &[String]) -> (String, Value) {
    let result = Command::new(command).args(args).arg("--status").output();
    match result {
        Ok(output) if output.status.success() => {
            match serde_json::from_slice::<Value>(&output.stdout) {
                Ok(value) if value.get("project_ready") == Some(&Value::Bool(true)) => (
                    "ready".into(),
                    value.get("eligible_skills").cloned().unwrap_or(Value::Null),
                ),
                _ => ("invalid_status".into(), Value::Null),
            }
        }
        Ok(_) => ("project_not_ready".into(), Value::Null),
        Err(_) => ("unavailable".into(), Value::Null),
    }
}

pub(super) fn connect(cli: &Cli, options: &SkillsConnectArgs) -> Result<CommandOutput, CliError> {
    let (root, path, ledger, command, args) =
        connection(cli, &options.target, options.scope, &options.python)?;
    if !options.remove {
        let (host, count) = offline_status(&command, &args);
        if host != "ready" || count.as_u64().unwrap_or(0) == 0 {
            return Err(CliError::new(EXIT_STATE,
                "Discovery catalog is not ready or has no eligible skills. Check Python 3.10+ (use --python if needed), the project/profile, then run `metactl skills host --status` and `metactl skills catalog --json`."));
        }
    }
    ensure_safe_destination(&path)?;
    let old = read_config(&path)?.unwrap_or_default();
    let (new, action) = if options.target == "codex-cli" {
        edit_codex(&old, &codex_block(&command, &args), &args, options.remove)?
    } else {
        edit_json(
            &old,
            &options.target,
            expected_entry(&options.target, &command, &args),
            options.remove,
        )?
    };
    if options.apply || options.remove {
        if tracked(&root, &path) {
            return Err(CliError::new(EXIT_STATE, format!("{} is tracked by Git; choose user scope or untrack the machine-specific configuration first.", path.display())));
        }
        if old != new {
            if !options.remove {
                let parent = ledger.parent().unwrap();
                fs::create_dir_all(parent).map_err(internal_error)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                        .map_err(internal_error)?;
                }
            }
            write_private(&path, &new)?;
        }
    }
    let phase = if options.apply || options.remove {
        action
    } else {
        "preview"
    };
    let entry = if options.target == "codex-cli" {
        codex_block(&command, &args)
    } else {
        serde_json::to_string_pretty(
            &json!({SERVER: expected_entry(&options.target, &command, &args)}),
        )
        .map_err(internal_error)?
    };
    let rollback = format!(
        "metactl --project {} skills connect --target {} --scope {} --remove",
        root.display(),
        options.target,
        if options.scope == DiscoveryScopeArg::User {
            "user"
        } else {
            "project"
        }
    );
    let human = format!("Discovery {phase} for {}\nConfig: {}\nServer: {}\nMode: baseline (deterministic; provider calls 0)\nPrivate event log: {}\nChange: {}\nEntry:\n{}\nRollback: {}\nNext: restart the agent, verify discover_skills and load_skill, then call them for a useful task.\nNative acceptance and benefit remain unknown until observed.",
        options.target, path.display(), SERVER, ledger.display(), if old == new { "none" } else if options.remove { "remove server entry" } else if action == "updated" { "update managed server entry" } else { "add server entry" }, entry, rollback);
    Ok(CommandOutput {
        human,
        json: success_json(
            "skills connect",
            Some(&root),
            json!({
                "action": phase, "target": options.target, "scope": format!("{:?}", options.scope).to_lowercase(),
                "config_path": path, "server": SERVER, "command": command, "args": args,
                "mode": "baseline", "provider_calls": 0, "event_log": ledger,
                "entry_preview": entry,
                "changed": old != new, "applied": options.apply || options.remove, "rollback": rollback,
                "native_acceptance": "unknown", "benefit": "unknown"
            }),
        ),
    })
}

pub(super) fn doctor(cli: &Cli, options: &SkillsDoctorArgs) -> Result<CommandOutput, CliError> {
    let (root, path, ledger, command, args) =
        connection(cli, &options.target, options.scope, &options.python)?;
    let config = read_config(&path)?.unwrap_or_default();
    let registration = if options.target == "codex-cli" {
        if config.contains(&codex_block(&command, &args)) {
            "configured"
        } else if managed_codex_block(&config, &args).ok().flatten().is_some() {
            "stale_registration"
        } else if config.contains(&format!("[mcp_servers.{SERVER}]")) {
            "conflict"
        } else {
            "missing"
        }
    } else if config.trim().is_empty() {
        "missing"
    } else {
        match serde_json::from_str::<Value>(&config) {
            Ok(value) => match value
                .get(entry_key(&options.target))
                .and_then(|v| v.get(SERVER))
            {
                Some(actual) if actual == &expected_entry(&options.target, &command, &args) => {
                    "configured"
                }
                Some(_) => "conflict",
                None => "missing",
            },
            Err(_) => "invalid_config",
        }
    };
    let (host, catalog) = offline_status(&command, &args);
    let mut latest = Value::Null;
    let mut observed = 0usize;
    let log_status = match fs::metadata(&ledger) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "missing",
        Err(_) => "unreadable",
        Ok(metadata) if metadata.len() > 8 * 1024 * 1024 => "too_large",
        Ok(_) => match fs::read_to_string(&ledger) {
            Err(_) => "unreadable",
            Ok(body) => {
                let mut valid = true;
                for line in body.lines() {
                    match serde_json::from_str::<Value>(line) {
                        Ok(value)
                            if value.get("schema").and_then(Value::as_str)
                                == Some("metactl.discovery_trial.v1") =>
                        {
                            if value.get("runtime").and_then(Value::as_str) == Some(&options.target)
                                && value.get("kind").and_then(Value::as_str) == Some("discover")
                            {
                                observed += 1;
                                latest = json!({
                                    "time": value.get("time"), "session_id": value.get("session_id"),
                                    "run_id": value.get("run_id"), "mode": value.get("arm"),
                                    "provider_attempts": value.get("provider_attempts"),
                                    "provider_calls": value.get("provider_calls"), "reason": value.get("reason")
                                });
                            }
                        }
                        _ => {
                            valid = false;
                            break;
                        }
                    }
                }
                if valid {
                    "readable"
                } else {
                    "invalid"
                }
            }
        },
    };
    let routing = if log_status == "readable" && observed > 0 {
        "observed"
    } else if log_status == "missing" || log_status == "readable" {
        "unknown_no_event"
    } else {
        "unknown_log_error"
    };
    let human = format!("Discovery doctor for {}\nCatalog: {} eligible skills\nRegistration: {} ({})\nLocal host: {} (offline status; no provider call)\nAgent tools in a fresh session: unknown; inspect the native client\nRouting: {} ({} matching discovery events)\nEvent log: {} ({})\nBenefit: unknown until task outcomes are compared\nMode: baseline; provider calls on this check: 0",
        options.target, catalog.as_u64().map(|n| n.to_string()).unwrap_or_else(|| "unknown".into()), registration, path.display(), host, routing, observed, log_status, ledger.display());
    Ok(CommandOutput {
        human,
        json: success_json(
            "skills doctor",
            Some(&root),
            json!({
                "target": options.target, "config_path": path, "catalog_eligible_skills": catalog,
                "registration": registration, "host": host, "agent_tools": "unknown",
                "routing": routing, "matching_discoveries": observed, "latest_discovery": latest,
                "event_log": ledger, "log_status": log_status, "benefit": "unknown", "check_provider_calls": 0
            }),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> Vec<String> {
        vec![
            "--project",
            "/tmp/example",
            "skills",
            "host",
            "--ranker",
            "deterministic",
            "--runtime",
            "codex-cli",
            "--trial-mode",
            "baseline",
            "--event-log",
            "/tmp/log",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    #[test]
    fn codex_preserves_unrelated_settings_and_rolls_back() {
        let original = "model = \"test\"\n\n[mcp_servers.other]\ncommand = \"other\"\n";
        let block = codex_block("/tmp/metactl", &args());
        let (connected, _) = edit_codex(original, &block, &args(), false).unwrap();
        assert!(connected.starts_with(original));
        assert_eq!(
            edit_codex(&connected, &block, &args(), false).unwrap().1,
            "already_connected"
        );
        let (removed, _) = edit_codex(&connected, &block, &args(), true).unwrap();
        assert_eq!(removed, format!("{original}\n"));
    }

    #[test]
    fn codex_updates_old_binary_and_rejects_changed_policy() {
        let old_block = codex_block("/old/metactl", &args());
        let new_block = codex_block("/new/metactl", &args());
        let (updated, action) = edit_codex(&old_block, &new_block, &args(), false).unwrap();
        assert_eq!(action, "updated");
        assert!(updated.contains("/new/metactl"));
        assert!(!updated.contains("/old/metactl"));
        let changed = old_block.replace("deterministic", "jev");
        assert!(edit_codex(&changed, &new_block, &args(), true).is_err());
    }

    #[test]
    fn json_targets_preserve_unrelated_values_and_reject_conflicts() {
        for target in ["claude-code", "cursor", "gemini-cli", "opencode"] {
            let expected = expected_entry(target, "/tmp/metactl", &args());
            let original = "{\"other\":{\"enabled\":true}}";
            let (connected, _) = edit_json(original, target, expected.clone(), false).unwrap();
            let value: Value = serde_json::from_str(&connected).unwrap();
            assert_eq!(value["other"]["enabled"], true);
            assert_eq!(value[entry_key(target)][SERVER], expected);
            assert!(edit_json(
                &connected.replace("/tmp/metactl", "/wrong"),
                target,
                expected.clone(),
                false
            )
            .is_err());
            let (removed, _) = edit_json(&connected, target, expected, true).unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&removed).unwrap()["other"]["enabled"],
                true
            );
        }
    }
}
