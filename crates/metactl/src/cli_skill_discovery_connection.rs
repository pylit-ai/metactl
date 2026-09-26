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

fn resolved_python(python: &Path) -> Result<PathBuf, CliError> {
    if python.is_absolute() {
        return Ok(python.to_path_buf());
    }
    if python.components().count() != 1 {
        return Err(CliError::new(
            EXIT_VALIDATION,
            "Use an absolute --python path or an executable name on PATH.",
        ));
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(python))
        .find(|candidate| executable_file(candidate))
        .ok_or_else(|| CliError::new(EXIT_STATE,
            format!("Python interpreter '{}' was not found on PATH. Pass --python /absolute/path/to/python3.", python.display())))
}

fn executable_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn canonicalize_missing_tail(path: &Path) -> Result<PathBuf, CliError> {
    let mut absent = Vec::new();
    let mut prefix = path;
    while !prefix.exists() {
        absent.push(
            prefix
                .file_name()
                .ok_or_else(|| {
                    CliError::new(EXIT_VALIDATION, "Use an absolute project path for removal.")
                })?
                .to_os_string(),
        );
        prefix = prefix.parent().ok_or_else(|| {
            CliError::new(EXIT_VALIDATION, "Use an absolute project path for removal.")
        })?;
    }
    let mut resolved = prefix.canonicalize().map_err(internal_error)?;
    for component in absent.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn host_command(
    cli: &Cli,
    root: &Path,
    target: &str,
    ledger: &Path,
    python: &Path,
) -> Result<(String, Vec<String>), CliError> {
    let exe = std::env::current_exe().map_err(internal_error)?;
    if python.components().count() > 1 && !python.is_absolute() {
        return Err(CliError::new(
            EXIT_VALIDATION,
            "Use an absolute --python path or an executable name on PATH.",
        ));
    }
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

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn parse_codex_toml(body: &str) -> Result<toml::Value, CliError> {
    let source = if body.trim().is_empty() { "\n" } else { body };
    toml::from_str(source).map_err(|error| {
        CliError::new(
            EXIT_STATE,
            format!("Codex configuration is invalid TOML; no change was made: {error}"),
        )
    })
}

fn parse_generated_codex_toml(body: &str) -> Result<toml::Value, CliError> {
    parse_codex_toml(body).map_err(|_| CliError::new(EXIT_STATE,
        "MetaCTL cannot add this server table to the valid Codex config without changing its structure; no change was made. Review inline mcp_servers tables manually."))
}

fn codex_has_server(document: &toml::Value) -> bool {
    document
        .get("mcp_servers")
        .and_then(|value| value.get(SERVER))
        .is_some()
}

fn codex_server(document: &toml::Value) -> Option<&toml::Value> {
    document.get("mcp_servers")?.get(SERVER)
}

fn codex_pinned_project(document: &toml::Value) -> Option<&str> {
    let args = codex_server(document)?.get("args")?.as_array()?;
    if args.first()?.as_str()? != "--project" {
        return None;
    }
    args.get(1)?.as_str()
}

fn codex_only_server_changed(
    before: &toml::Value,
    after: &toml::Value,
    replacement: Option<&toml::Value>,
) -> bool {
    let mut expected = before.clone();
    let Some(root) = expected.as_table_mut() else {
        return false;
    };
    if let Some(value) = replacement {
        let servers = root
            .entry("mcp_servers")
            .or_insert_with(|| toml::Value::Table(Default::default()));
        let Some(table) = servers.as_table_mut() else {
            return false;
        };
        table.insert(SERVER.to_owned(), value.clone());
    } else {
        let Some(table) = root
            .get_mut("mcp_servers")
            .and_then(toml::Value::as_table_mut)
        else {
            return false;
        };
        table.remove(SERVER);
        if table.is_empty() {
            root.remove("mcp_servers");
        }
    }
    &expected == after
}

fn policy_args(args: &[String]) -> Vec<&str> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--python" && i + 1 < args.len() {
            i += 2;
        } else {
            result.push(args[i].as_str());
            i += 1;
        }
    }
    result
}

fn require_policy_match(
    existing: &ManagedEntry,
    desired: &ManagedEntry,
    replace: bool,
) -> Result<(), CliError> {
    if policy_args(&existing.args) != policy_args(&desired.args) && !replace {
        return Err(CliError::new(EXIT_STATE,
            "The managed registration uses different profile, config, overlay or event-log options. Review the new entry with --replace, then use --apply --replace to change it. --remove still works without matching those options."));
    }
    Ok(())
}

fn codex_block(command: &str, args: &[String]) -> String {
    let command = serde_json::to_string(command).unwrap_or_default();
    let args = serde_json::to_string(args).unwrap_or_default();
    format!("{BEGIN}\n[mcp_servers.{SERVER}]\ncommand = {command}\nargs = {args}\n{END}\n")
}

#[derive(Clone)]
struct ManagedEntry {
    command: String,
    args: Vec<String>,
}

fn managed_entry(
    command: String,
    args: Vec<String>,
    root: &Path,
    target: &str,
) -> Option<ManagedEntry> {
    if !Path::new(&command).is_absolute()
        || !matches!(
            Path::new(&command).file_name()?.to_str()?,
            "metactl" | "metactl.exe"
        )
        || args.len() < 12
        || args.first()? != "--project"
        || args.get(1)? != &root.to_string_lossy()
    {
        return None;
    }
    let mut i = 2;
    while args.get(i).map(String::as_str) != Some("skills") {
        match args.get(i)?.as_str() {
            "--no-profile" => i += 1,
            "--profile" => {
                args.get(i + 1)?;
                i += 2;
            }
            "--config" | "--overlay" => {
                if !Path::new(args.get(i + 1)?).is_absolute() {
                    return None;
                }
                i += 2;
            }
            _ => return None,
        }
    }
    if args.get(i + 1)?.as_str() != "host" {
        return None;
    }
    i += 2;
    if args.get(i).map(String::as_str) == Some("--python") {
        let python = Path::new(args.get(i + 1)?);
        if python.components().count() > 1 && !python.is_absolute() {
            return None;
        }
        i += 2;
    }
    if args.get(i..i + 2)? != ["--ranker", "deterministic"]
        || args.get(i + 2..i + 4)? != ["--runtime", target]
        || args.get(i + 4..i + 6)? != ["--trial-mode", "baseline"]
        || args.get(i + 6)?.as_str() != "--event-log"
        || args.len() != i + 8
    {
        return None;
    }
    let event_log = PathBuf::from(args.get(i + 7)?);
    if !event_log.is_absolute() {
        return None;
    }
    Some(ManagedEntry { command, args })
}

fn managed_json_entry(value: &Value, root: &Path, target: &str) -> Option<ManagedEntry> {
    let map = value.as_object()?;
    if target == "opencode" {
        if map.len() != 3
            || map.get("type")?.as_str()? != "local"
            || !map.get("enabled")?.as_bool()?
        {
            return None;
        }
        let mut command: Vec<String> = serde_json::from_value(map.get("command")?.clone()).ok()?;
        if command.is_empty() {
            return None;
        }
        return managed_entry(command.remove(0), command, root, target);
    }
    if map.len() != 2 {
        return None;
    }
    let command = map.get("command")?.as_str()?.to_owned();
    let args = serde_json::from_value(map.get("args")?.clone()).ok()?;
    managed_entry(command, args, root, target)
}

fn read_config(path: &Path) -> Result<Option<String>, CliError> {
    let metadata = match path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(internal_error(error)),
    };
    if metadata.file_type().is_symlink() {
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

fn tracked(root: &Path, path: &Path) -> Result<bool, CliError> {
    let Ok(relative) = path.strip_prefix(root) else {
        return Ok(false);
    };
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "--error-unmatch", "--"])
        .arg(relative)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match status {
        Ok(status) if status.success() => Ok(true),
        Ok(status) if status.code() == Some(1) => Ok(false),
        _ if !root.ancestors().any(|parent| parent.join(".git").exists()) => Ok(false),
        _ => Err(CliError::new(EXIT_STATE,
            "Cannot verify whether the target config is tracked by Git; no change was made. Restore Git or review the config manually.")),
    }
}

fn git_visibility(root: &Path, path: &Path, scope: DiscoveryScopeArg) -> &'static str {
    if scope == DiscoveryScopeArg::User {
        return "user_config";
    }
    match tracked(root, path) {
        Ok(true) => return "tracked",
        Err(_) => return "unknown",
        Ok(false) => {}
    }
    if !Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        return "not_git";
    }
    let Ok(relative) = path.strip_prefix(root) else {
        return "outside_project";
    };
    if Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["check-ignore", "-q", "--"])
        .arg(relative)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        "ignored"
    } else {
        "unignored"
    }
}

fn acceptance_note(target: &str, scope: DiscoveryScopeArg) -> &'static str {
    match target {
        "codex-cli" if scope == DiscoveryScopeArg::Project => "Codex loads project config only after the project is trusted. Start a fresh session and check /mcp for both tools.",
        "codex-cli" => "Start a fresh Codex session and check /mcp for both tools. User scope shares this fixed catalog across projects.",
        "claude-code" => "Approve the project MCP server in Claude Code, start a fresh session, and check for both tools.",
        "gemini-cli" => "Trust the project folder in Gemini CLI, reload its MCP tools, and check for both tools.",
        "cursor" => "Reload Cursor's project MCP tools and check for both tools.",
        "opencode" => "Reload OpenCode's local MCP tools and check for both tools.",
        _ => "Check both tools in a fresh native client session.",
    }
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
        let mode = fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .unwrap_or(0o600);
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(mode))
            .map_err(internal_error)?;
    }
    temp.persist(path).map_err(internal_error)?;
    Ok(())
}

fn managed_codex_block<'a>(
    old: &'a str,
    root: &Path,
) -> Result<Option<(usize, usize, &'a str, ManagedEntry)>, CliError> {
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
        let entry = command.and_then(|command| {
            old_args.and_then(|args| managed_entry(command, args, root, "codex-cli"))
        });
        if lines.len() != 5
            || lines[0] != BEGIN
            || lines[1] != header
            || lines[4] != END
            || old.matches(&header).count() != 1
            || entry.is_none()
        {
            return Err(CliError::new(EXIT_STATE, "MetaCTL discovery block differs from the expected registration; review it manually."));
        }
        return Ok(Some((begin, end, block, entry.unwrap())));
    }
    Ok(None)
}

fn recover_rewritten_codex_block(
    old: &str,
    parsed: &toml::Value,
    root: &Path,
) -> Option<(String, ManagedEntry)> {
    // A client may rewrite comments away. The exact managed command shape and
    // a parsed single-server semantic diff establish ownership in that case.
    if old.matches(BEGIN).count() > 1 || old.matches(END).count() > 1 {
        return None;
    }
    let server = codex_server(parsed)?.as_table()?;
    if server.len() != 2 {
        return None;
    }
    let command = server.get("command")?.as_str()?.to_owned();
    let args = server
        .get("args")?
        .as_array()?
        .iter()
        .map(|value| value.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    let entry = managed_entry(command, args, root, "codex-cli")?;
    let header = format!("[mcp_servers.{SERVER}]");
    let quoted_header = format!("[mcp_servers.\"{SERVER}\"]");
    let mut found = false;
    let mut in_server = false;
    let mut cleaned = String::new();
    for line in old.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed == BEGIN || trimmed == END {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_server = trimmed == header || trimmed == quoted_header;
            if in_server {
                found = true;
                continue;
            }
        }
        if in_server && (trimmed.starts_with("command =") || trimmed.starts_with("args =")) {
            continue;
        }
        cleaned.push_str(line);
    }
    if !found {
        return None;
    }
    let parsed_cleaned = parse_codex_toml(&cleaned).ok()?;
    if codex_has_server(&parsed_cleaned)
        || !codex_only_server_changed(parsed, &parsed_cleaned, None)
    {
        return None;
    }
    Some((cleaned, entry))
}

fn edit_codex(
    old: &str,
    block: &str,
    root: &Path,
    desired: &ManagedEntry,
    remove: bool,
    replace: bool,
) -> Result<(String, &'static str), CliError> {
    let header = format!("[mcp_servers.{SERVER}]");
    let parsed = parse_codex_toml(old)?;
    let strict_block = managed_codex_block(old, root);
    if let Ok(Some((begin, end, existing, entry))) = &strict_block {
        let existing_semantic = parse_codex_toml(existing)?;
        if codex_server(&parsed) != codex_server(&existing_semantic) {
            return Err(CliError::new(
                EXIT_STATE,
                "Codex server has settings outside the managed block; review it manually.",
            ));
        }
        if remove {
            let suffix = if old[*end..].starts_with('\n') {
                *end + 1
            } else {
                *end
            };
            let updated = format!("{}{}", &old[..*begin], &old[suffix..]);
            let parsed_updated = parse_codex_toml(&updated)?;
            if codex_has_server(&parsed_updated) {
                return Err(CliError::new(EXIT_STATE,
                    "Codex config still defines metactl-skills after removing the managed block; review it manually."));
            }
            if !codex_only_server_changed(&parsed, &parsed_updated, None) {
                return Err(CliError::new(EXIT_STATE,
                    "Removing the Codex server would change unrelated settings; review it manually."));
            }
            return Ok((updated, "removed"));
        }
        if *existing == block.trim_end() {
            return Ok((old.to_owned(), "already_connected"));
        }
        require_policy_match(entry, desired, replace)?;
        let updated = format!("{}{}{}", &old[..*begin], block.trim_end(), &old[*end..]);
        let parsed_updated = parse_generated_codex_toml(&updated)?;
        let block_semantic = parse_codex_toml(block)?;
        if !codex_only_server_changed(&parsed, &parsed_updated, codex_server(&block_semantic)) {
            return Err(CliError::new(
                EXIT_STATE,
                "Updating the Codex server would change unrelated settings; review it manually.",
            ));
        }
        return Ok((updated, "updated"));
    }
    if let Some((cleaned, entry)) = recover_rewritten_codex_block(old, &parsed, root) {
        if remove {
            return Ok((cleaned, "removed"));
        }
        require_policy_match(&entry, desired, replace)?;
        let (updated, _) = edit_codex(&cleaned, block, root, desired, false, replace)?;
        return Ok((updated, "updated"));
    }
    if let Err(error) = strict_block {
        if let Some(owner) = codex_pinned_project(&parsed) {
            if owner != root.to_string_lossy() {
                return Err(CliError::new(EXIT_STATE, format!(
                    "The existing Codex metactl-skills server is pinned to project {owner}. Run its rollback with --project {owner}, or review the config manually."
                )));
            }
        }
        return Err(error);
    }
    if old.contains(&header) || codex_has_server(&parsed) {
        if let Some(owner) = codex_pinned_project(&parsed) {
            if owner != root.to_string_lossy() {
                return Err(CliError::new(EXIT_STATE, format!(
                    "The existing Codex metactl-skills server is pinned to project {owner}. Run its rollback with --project {owner}, or review the config manually."
                )));
            }
        }
        let operation = if remove { "remove" } else { "replace" };
        return Err(CliError::new(EXIT_STATE, format!(
            "Cannot {operation} an unmanaged metactl-skills server in Codex config; review it manually."
        )));
    }
    if remove {
        return Ok((old.to_owned(), "already_absent"));
    }
    let separator = if old.is_empty() || old.ends_with("\n\n") {
        ""
    } else if old.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let updated = format!("{old}{separator}{block}");
    let parsed_updated = parse_generated_codex_toml(&updated)?;
    let block_semantic = parse_codex_toml(block)?;
    if !codex_only_server_changed(&parsed, &parsed_updated, codex_server(&block_semantic)) {
        return Err(CliError::new(
            EXIT_STATE,
            "Adding the Codex server would change unrelated settings; review it manually.",
        ));
    }
    Ok((updated, "connected"))
}

fn edit_json(
    old: &str,
    target: &str,
    expected: Value,
    root: &Path,
    remove: bool,
    replace: bool,
) -> Result<(String, &'static str), CliError> {
    let mut document: Value = if old.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(old).map_err(|_| {
            CliError::new(
                EXIT_STATE,
                "Target configuration is not strict JSON; JSONC comments are not edited automatically. Review the config manually.",
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
        Some(value) if managed_json_entry(value, root, target).is_none() => {
            return Err(CliError::new(
                EXIT_STATE,
                "Existing metactl-skills server is not a matching MetaCTL baseline registration for this project; review it manually.",
            ))
        }
        Some(_) if remove => {
            servers.remove(SERVER);
        }
        Some(value) if value == &expected => return Ok((old.to_owned(), "already_connected")),
        Some(_) => {
            let existing = managed_json_entry(servers.get(SERVER).unwrap(), root, target).unwrap();
            let desired = managed_json_entry(&expected, root, target).unwrap();
            require_policy_match(&existing, &desired, replace)?;
            servers.insert(SERVER.into(), expected);
        },
        None if remove => return Ok((old.to_owned(), "already_absent")),
        None => {
            servers.insert(SERVER.into(), expected);
        }
    }
    let action = if remove {
        "removed"
    } else if old.contains(SERVER) {
        "updated"
    } else {
        "connected"
    };
    let body = serde_json::to_string_pretty(&document).map_err(internal_error)? + "\n";
    Ok((body, action))
}

fn connection(
    cli: &Cli,
    target: &str,
    scope: DiscoveryScopeArg,
    python: &Path,
    removing: bool,
    allow_unavailable_python: bool,
) -> Result<(PathBuf, PathBuf, PathBuf, String, Vec<String>), CliError> {
    let supplied = project_root(cli).map_err(internal_error)?;
    let root = match supplied.canonicalize() {
        Ok(path) => path,
        Err(_) if removing && scope == DiscoveryScopeArg::User && supplied.is_absolute() => {
            canonicalize_missing_tail(&supplied)?
        }
        Err(error) => return Err(internal_error(error)),
    };
    let path = target_path(&root, target, scope)?;
    let ledger = ledger_path(&root)?;
    let python = if removing {
        python.to_path_buf()
    } else if allow_unavailable_python {
        resolved_python(python).unwrap_or_else(|_| python.to_path_buf())
    } else {
        resolved_python(python)?
    };
    let (command, args) = host_command(cli, &root, target, &ledger, &python)?;
    Ok((root, path, ledger, command, args))
}

fn offline_status(command: &str, args: &[String]) -> (String, Value) {
    use std::io::{Read, Seek, SeekFrom};
    use std::time::{Duration, Instant};
    let Ok(mut output_file) = tempfile::tempfile() else {
        return ("unavailable".into(), Value::Null);
    };
    let Ok(child_output) = output_file.try_clone() else {
        return ("unavailable".into(), Value::Null);
    };
    let spawned = Command::new(command)
        .args(args)
        .arg("--status")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(child_output))
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = spawned else {
        return ("unavailable".into(), Value::Null);
    };
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(15) {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return ("project_not_ready".into(), Value::Null);
                }
                let mut output = Vec::new();
                if output_file.seek(SeekFrom::Start(0)).is_err()
                    || output_file.take(65537).read_to_end(&mut output).is_err()
                    || output.len() > 65536
                {
                    return ("invalid_status".into(), Value::Null);
                }
                return match serde_json::from_slice::<Value>(&output) {
                    Ok(value) if value.get("project_ready") == Some(&Value::Bool(true)) => (
                        "ready".into(),
                        value.get("eligible_skills").cloned().unwrap_or(Value::Null),
                    ),
                    _ => ("invalid_status".into(), Value::Null),
                };
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return ("unavailable".into(), Value::Null),
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    ("timeout".into(), Value::Null)
}

fn read_recent_log(path: &Path) -> Result<(String, bool), CliError> {
    use std::io::{Read, Seek, SeekFrom};
    const MAX: u64 = 2 * 1024 * 1024;
    let mut file = fs::File::open(path).map_err(internal_error)?;
    let metadata = file.metadata().map_err(internal_error)?;
    if !metadata.is_file() {
        return Err(CliError::new(
            EXIT_STATE,
            "Discovery event log is not a regular file.",
        ));
    }
    let truncated = metadata.len() > MAX;
    let start = metadata.len().saturating_sub(MAX);
    file.seek(SeekFrom::Start(start)).map_err(internal_error)?;
    let mut bytes = Vec::new();
    file.take(MAX + 1)
        .read_to_end(&mut bytes)
        .map_err(internal_error)?;
    let bytes = if truncated {
        bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| &bytes[index + 1..])
            .unwrap_or(&[])
    } else {
        &bytes
    };
    Ok((String::from_utf8_lossy(bytes).into_owned(), truncated))
}

pub(super) fn connect(cli: &Cli, options: &SkillsConnectArgs) -> Result<CommandOutput, CliError> {
    let (root, path, ledger, command, args) = connection(
        cli,
        &options.target,
        options.scope,
        &options.python,
        options.remove,
        false,
    )?;
    if options.target == "opencode"
        && !options.remove
        && !path.exists()
        && root.join("opencode.jsonc").exists()
    {
        return Err(CliError::new(EXIT_STATE,
            "An opencode.jsonc configuration already exists. MetaCTL will not create a second OpenCode config; add the server entry manually."));
    }
    if !options.remove {
        let (host, count) = offline_status(&command, &args);
        if host != "ready" || count.as_u64().unwrap_or(0) == 0 {
            return Err(CliError::new(EXIT_STATE,
                "Discovery catalog is not ready or has no eligible skills. Check Python 3.10+ (use --python if needed), the project/profile, then run `metactl skills host --status` and `metactl skills catalog --json`."));
        }
    }
    ensure_safe_destination(&path)?;
    let old = read_config(&path)?.unwrap_or_default();
    let desired =
        managed_entry(command.clone(), args.clone(), &root, &options.target).ok_or_else(|| {
            CliError::new(
                EXIT_STATE,
                "Generated discovery registration failed its own safety check.",
            )
        })?;
    let (new, action) = if options.target == "codex-cli" {
        edit_codex(
            &old,
            &codex_block(&command, &args),
            &root,
            &desired,
            options.remove,
            options.replace,
        )?
    } else {
        edit_json(
            &old,
            &options.target,
            expected_entry(&options.target, &command, &args),
            &root,
            options.remove,
            options.replace,
        )?
    };
    if options.apply || options.remove {
        if tracked(&root, &path)? {
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
    let codex_home = if options.scope == DiscoveryScopeArg::User {
        std::env::var("CODEX_HOME")
            .ok()
            .map(|value| format!("CODEX_HOME={} ", shell_quote(&value)))
    } else {
        None
    };
    let rollback = format!(
        "{}{} --project {} skills connect --target {} --scope {} --remove",
        codex_home.unwrap_or_default(),
        shell_quote(&command),
        shell_quote(&root.to_string_lossy()),
        shell_quote(&options.target),
        if options.scope == DiscoveryScopeArg::User {
            "user"
        } else {
            "project"
        }
    );
    let visibility = git_visibility(&root, &path, options.scope);
    let git_note = if visibility == "unignored" {
        "Machine-specific config is not ignored by Git. Add this path to .git/info/exclude or a reviewed .gitignore before committing."
    } else if visibility == "tracked" {
        "This config is tracked by Git, so --apply will refuse. Review a portable registration or move machine-specific settings to a local config."
    } else if visibility == "unknown" {
        "Git visibility could not be checked, so --apply will refuse until Git works."
    } else {
        ""
    };
    let human = format!("Discovery {phase} for {}\nConfig: {}\nServer: {}\nMode: baseline (deterministic; provider calls 0)\nPrivate event log: {}\nChange: {}\nEntry:\n{}\nRollback: {}\nNext: {}\n{}\nNative acceptance and benefit remain unknown until observed.",
        options.target, path.display(), SERVER, ledger.display(), if old == new { "none" } else if options.remove { "remove server entry" } else if action == "updated" { "update managed server entry" } else { "add server entry" }, entry, rollback, acceptance_note(&options.target, options.scope), git_note);
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
                "git_visibility": visibility, "native_acceptance_note": acceptance_note(&options.target, options.scope),
                "changed": old != new, "applied": options.apply || options.remove, "rollback": rollback,
                "native_acceptance": "unknown", "benefit": "unknown"
            }),
        ),
    })
}

pub(super) fn doctor(cli: &Cli, options: &SkillsDoctorArgs) -> Result<CommandOutput, CliError> {
    let (root, path, ledger, command, args) = connection(
        cli,
        &options.target,
        options.scope,
        &options.python,
        false,
        true,
    )?;
    let python_unavailable = resolved_python(&options.python)
        .map(|path| !executable_file(&path))
        .unwrap_or(true);
    let config_issue = if path
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        Some("symlink_refused")
    } else {
        None
    };
    let (config, config_issue) = if let Some(issue) = config_issue {
        (String::new(), Some(issue))
    } else {
        match read_config(&path) {
            Ok(value) => (value.unwrap_or_default(), None),
            Err(_) => (String::new(), Some("unreadable_config")),
        }
    };
    let (registration, registered) = if let Some(issue) = config_issue {
        (issue, None)
    } else if options.target == "codex-cli" {
        match parse_codex_toml(&config) {
            Err(_) => ("invalid_config", None),
            Ok(document) => {
                let recovered = recover_rewritten_codex_block(&config, &document, &root);
                match managed_codex_block(&config, &root) {
                    Ok(Some((_, _, block, entry)))
                        if parse_codex_toml(block).ok().is_some_and(|managed| {
                            codex_server(&document) == codex_server(&managed)
                        }) =>
                    {
                        ("configured", Some(entry))
                    }
                    _ if recovered.is_some() => {
                        let (_, entry) = recovered.unwrap();
                        ("configured", Some(entry))
                    }
                    _ if codex_has_server(&document) || config.contains(BEGIN) => {
                        ("conflict", None)
                    }
                    _ => ("missing", None),
                }
            }
        }
    } else if config.trim().is_empty() {
        ("missing", None)
    } else {
        match serde_json::from_str::<Value>(&config) {
            Ok(value) => match value
                .get(entry_key(&options.target))
                .and_then(|v| v.get(SERVER))
            {
                Some(actual) => match managed_json_entry(actual, &root, &options.target) {
                    Some(entry) => ("configured", Some(entry)),
                    None => ("conflict", None),
                },
                None => ("missing", None),
            },
            Err(_) => ("invalid_config", None),
        }
    };
    let matches_requested = registered
        .as_ref()
        .is_some_and(|entry| entry.command == command && entry.args == args);
    let catalog = cli_skills::cmd_skill_discovery(cli, &SkillsCommand::Catalog)
        .ok()
        .and_then(|output| {
            output
                .json
                .get("result")?
                .get("skills")?
                .as_array()
                .map(|skills| json!(skills.len()))
        })
        .unwrap_or(Value::Null);
    let registered_command = registered.as_ref().map(|entry| entry.command.as_str());
    let registered_log = registered
        .as_ref()
        .and_then(|entry| entry.args.last().map(String::as_str));
    let drift = registered.as_ref().and_then(|entry| {
        if entry.command != command {
            Some("command_differs")
        } else if entry.args.last() != args.last() {
            Some("event_log_differs")
        } else if entry.args != args {
            Some("options_differ")
        } else {
            None
        }
    });
    // A project-controlled client config is data for doctor, never a command to execute.
    let (host, host_catalog) = if python_unavailable {
        ("unavailable_python".into(), Value::Null)
    } else if registered.is_some() && !matches_requested {
        ("unknown_registration_drift".into(), Value::Null)
    } else {
        offline_status(&command, &args)
    };
    let mut latest = Value::Null;
    let mut observed = 0usize;
    let mut invalid_lines = 0usize;
    let mut log_window_truncated = false;
    let log_status = match fs::metadata(&ledger) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "missing",
        Err(_) => "unreadable",
        Ok(_) => match read_recent_log(&ledger) {
            Err(_) => "unreadable",
            Ok((body, truncated)) => {
                log_window_truncated = truncated;
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
                        Ok(_) => {}
                        Err(_) => {
                            invalid_lines += 1;
                        }
                    }
                }
                if invalid_lines == 0 {
                    "readable"
                } else {
                    "partial"
                }
            }
        },
    };
    let routing = if (log_status == "readable" || log_status == "partial") && observed > 0 {
        "observed"
    } else if matches!(log_status, "missing" | "readable" | "partial") {
        "unknown_no_event"
    } else {
        "unknown_log_error"
    };
    let human = format!("Discovery doctor for {}\nCatalog: {} skills listed (local)\nRegistration: {} ({})\nRegistered command: {}\nRegistered event log: {}\nRequested options match registration: {}{}\nLocal host: {} (offline check in this shell only; no provider call)\nAgent tools in a fresh session: unknown; inspect the native client\nRouting: {} ({} matching discovery events in the recent log window, including manual calls)\nEvent log checked: {} ({}; {} invalid lines; tail window: {})\nBenefit: unknown until task outcomes are compared\nMode: baseline; provider calls on this check: 0",
        options.target, catalog.as_u64().map(|n| n.to_string()).unwrap_or_else(|| "unknown".into()), registration, path.display(), registered_command.unwrap_or("unknown"), registered_log.unwrap_or("unknown"), matches_requested, drift.map(|reason| format!(" ({reason})")).unwrap_or_default(), host, routing, observed, ledger.display(), log_status, invalid_lines, log_window_truncated);
    Ok(CommandOutput {
        human,
        json: success_json(
            "skills doctor",
            Some(&root),
            json!({
                "target": options.target, "config_path": path, "catalog_eligible_skills": catalog,
                "registration": registration, "registration_matches_requested_options": matches_requested,
                "registered_command": registered_command, "registered_event_log": registered_log,
                "registration_drift": drift, "host": host, "host_catalog_eligible_skills": host_catalog,
                "agent_tools": "unknown",
                "routing": routing, "matching_discoveries": observed, "latest_discovery": latest,
                "event_log": ledger, "log_status": log_status, "invalid_log_lines": invalid_lines,
                "log_window_truncated": log_window_truncated,
                "benefit": "unknown", "check_provider_calls": 0
            }),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit_codex(
        old: &str,
        block: &str,
        root: &Path,
        remove: bool,
    ) -> Result<(String, &'static str), CliError> {
        let lines: Vec<_> = block.lines().collect();
        let command: String =
            serde_json::from_str(lines[2].strip_prefix("command = ").unwrap()).unwrap();
        let args: Vec<String> =
            serde_json::from_str(lines[3].strip_prefix("args = ").unwrap()).unwrap();
        let desired = managed_entry(command, args, root, "codex-cli").unwrap();
        super::edit_codex(old, block, root, &desired, remove, false)
    }

    fn edit_json(
        old: &str,
        target: &str,
        expected: Value,
        root: &Path,
        remove: bool,
    ) -> Result<(String, &'static str), CliError> {
        super::edit_json(old, target, expected, root, remove, false)
    }

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
        let (connected, _) =
            edit_codex(original, &block, Path::new("/tmp/example"), false).unwrap();
        assert!(connected.starts_with(original));
        assert_eq!(
            edit_codex(&connected, &block, Path::new("/tmp/example"), false)
                .unwrap()
                .1,
            "already_connected"
        );
        let (removed, _) = edit_codex(&connected, &block, Path::new("/tmp/example"), true).unwrap();
        assert_eq!(removed, format!("{original}\n"));
    }

    #[test]
    fn codex_updates_old_binary_and_rejects_changed_policy() {
        let old_block = codex_block("/old/metactl", &args());
        let new_block = codex_block("/new/metactl", &args());
        let (updated, action) =
            edit_codex(&old_block, &new_block, Path::new("/tmp/example"), false).unwrap();
        assert_eq!(action, "updated");
        assert!(updated.contains("/new/metactl"));
        assert!(!updated.contains("/old/metactl"));
        let changed = old_block.replace("deterministic", "jev");
        assert!(edit_codex(&changed, &new_block, Path::new("/tmp/example"), true).is_err());
    }

    #[test]
    fn codex_remove_refuses_unmanaged_and_leftover_server_table() {
        let block = codex_block("/tmp/metactl", &args());
        let unmanaged = "[mcp_servers.\"metactl-skills\"]\ncommand = \"other\"\n";
        assert!(edit_codex(unmanaged, &block, Path::new("/tmp/example"), true).is_err());
        let nested = format!("{block}\n[mcp_servers.metactl-skills.env]\nFOO = \"bar\"\n");
        assert!(edit_codex(&nested, &block, Path::new("/tmp/example"), true).is_err());
    }

    #[test]
    fn json_targets_preserve_unrelated_values_and_reject_conflicts() {
        for target in ["claude-code", "cursor", "gemini-cli", "opencode"] {
            let mut target_args = args();
            target_args[7] = target.into();
            let expected = expected_entry(target, "/tmp/metactl", &target_args);
            let original = "{\"other\":{\"enabled\":true}}";
            let (connected, _) = edit_json(
                original,
                target,
                expected.clone(),
                Path::new("/tmp/example"),
                false,
            )
            .unwrap();
            let value: Value = serde_json::from_str(&connected).unwrap();
            assert_eq!(value["other"]["enabled"], true);
            assert_eq!(value[entry_key(target)][SERVER], expected);
            assert!(edit_json(
                &connected.replace("/tmp/metactl", "/wrong"),
                target,
                expected.clone(),
                Path::new("/tmp/example"),
                false
            )
            .is_err());
            let (removed, _) = edit_json(
                &connected,
                target,
                expected,
                Path::new("/tmp/example"),
                true,
            )
            .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&removed).unwrap()["other"]["enabled"],
                true
            );
        }
    }
}
