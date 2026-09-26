//! Native Git evaluation of a proposed ignore state, without copying payloads.
use anyhow::{anyhow, bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

/// Git for Windows uses ordinary drive/UNC paths, not Rust's verbatim prefix.
/// Convert separators without changing case or resolving symlinks.
pub fn git_path_argument(path: &Path) -> std::ffi::OsString {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let mut path: Vec<u16> = path.as_os_str().encode_wide().collect();
        let prefix: Vec<u16> = "\\\\?\\".encode_utf16().collect();
        if path.starts_with(&prefix) {
            path.drain(..4);
            if path.starts_with(&"UNC\\".encode_utf16().collect::<Vec<_>>()) {
                path.splice(..4, "\\\\".encode_utf16());
            }
        }
        for c in &mut path {
            if *c == b'\\' as u16 {
                *c = b'/' as u16;
            }
        }
        std::ffi::OsString::from_wide(&path)
    }
    #[cfg(not(windows))]
    path.as_os_str().to_owned()
}

fn same_source_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        git_path_argument(left) == git_path_argument(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn git_relative(path: &Path) -> Result<String> {
    git_path_argument(path)
        .into_string()
        .map_err(|_| anyhow!("non-UTF8 Git path"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Match {
    source: String,
    line: String,
    pattern: String,
}

impl Match {
    fn ignored(&self) -> bool {
        !self.pattern.is_empty() && !self.pattern.starts_with('!')
    }
}

#[derive(Debug)]
pub struct Protection {
    pub before: bool,
    pub after: bool,
    pub tracked: bool,
}

fn git(root: &Path) -> Command {
    let mut cmd = Command::new("git");
    // Preserve supported configuration-source selectors so effective settings
    // are resolved in the real repository. Never redirect its index/worktree.
    for (name, _) in std::env::vars_os() {
        let key = name.to_string_lossy();
        if key.starts_with("GIT_")
            && !matches!(
                key.as_ref(),
                "GIT_CONFIG_GLOBAL" | "GIT_CONFIG_SYSTEM" | "GIT_CONFIG_NOSYSTEM"
            )
        {
            cmd.env_remove(name);
        }
    }
    cmd.arg("-C").arg(git_path_argument(root));
    cmd
}

fn output(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = git(root).args(args).output()?;
    if !out.status.success() {
        bail!(
            "Git privacy probe failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(out.stdout)
}

fn config(root: &Path, key: &str) -> Result<Option<String>> {
    let out = git(root)
        .args(["config", "--path", "--get", key])
        .output()?;
    match out.status.code() {
        Some(0) => Ok(Some(
            String::from_utf8(out.stdout)?.trim_end_matches('\n').into(),
        )),
        Some(1) => Ok(None),
        _ => bail!("cannot resolve effective Git configuration {key}"),
    }
}

fn relative(name: &str) -> Result<()> {
    if name.is_empty()
        || !Path::new(name)
            .components()
            .all(|p| matches!(p, Component::Normal(_)))
    {
        bail!("unsafe Git privacy path: {name}");
    }
    Ok(())
}

fn names(bytes: &[u8]) -> Result<BTreeSet<String>> {
    bytes
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| {
            let name = std::str::from_utf8(p)?.to_string();
            relative(&name)?;
            Ok(name)
        })
        .collect()
}

fn probe(
    root: &Path,
    paths: &BTreeSet<String>,
    exclude: &Path,
    global: Option<&Path>,
) -> Result<BTreeMap<String, Match>> {
    if paths.is_empty() {
        return Ok(BTreeMap::new());
    }
    let input: Vec<u8> = paths.iter().flat_map(|p| p.bytes().chain([0])).collect();
    let mut child = git(root)
        .args([
            "check-ignore",
            "--no-index",
            "--verbose",
            "--non-matching",
            "-z",
            "--stdin",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("missing Git input"))?;
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let result = child.wait_with_output();
    writer
        .join()
        .map_err(|_| anyhow!("Git input writer panicked"))??;
    let result = result?;
    if !matches!(result.status.code(), Some(0 | 1)) {
        bail!(
            "Git ignore evaluation failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    if !result.stdout.ends_with(&[0]) {
        bail!("unterminated Git ignore evidence");
    }
    let fields: Vec<_> = result.stdout[..result.stdout.len() - 1]
        .split(|b| *b == 0)
        .collect();
    if fields.len() != paths.len() * 4 {
        bail!("incomplete Git ignore evidence");
    }
    let mut matches = BTreeMap::new();
    for row in fields.chunks_exact(4) {
        let text = row
            .iter()
            .map(|s| std::str::from_utf8(s))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let path = text[3];
        if !paths.contains(path) || matches.contains_key(path) {
            bail!("unexpected Git ignore evidence");
        }
        let source_path = if Path::new(text[0]).is_absolute() {
            PathBuf::from(text[0])
        } else {
            root.join(text[0])
        };
        let source = if same_source_path(&source_path, exclude) {
            "<exclude>".into()
        } else if global.is_some_and(|p| same_source_path(p, &source_path)) {
            "<global>".into()
        } else {
            text[0].to_string()
        };
        matches.insert(
            path.into(),
            Match {
                source,
                line: text[1].into(),
                pattern: text[2].into(),
            },
        );
    }
    Ok(matches)
}

/// Results use repository-relative paths, including paths outside a nested
/// project affected by a repository-wide info/exclude update. All Git input
/// bytes are snapshotted; payload bytes are never copied to the probe tree.
pub fn evaluate(
    project: &Path,
    requested: &[String],
    writes: &[(PathBuf, Vec<u8>)],
) -> Result<(PathBuf, BTreeMap<String, Protection>)> {
    // High-priority inline overrides outrank the shadow's snapshotted local
    // configuration. Refuse those and repository/index redirection explicitly.
    // Ordinary global/system selectors are resolved by config() in the source.
    for (name, _) in std::env::vars_os() {
        let name = name.to_string_lossy();
        if (name.starts_with("GIT_CONFIG")
            && !matches!(
                name.as_ref(),
                "GIT_CONFIG_GLOBAL" | "GIT_CONFIG_SYSTEM" | "GIT_CONFIG_NOSYSTEM"
            ))
            || matches!(
                name.as_ref(),
                "GIT_DIR"
                    | "GIT_WORK_TREE"
                    | "GIT_COMMON_DIR"
                    | "GIT_INDEX_FILE"
                    | "GIT_CEILING_DIRECTORIES"
                    | "GIT_DISCOVERY_ACROSS_FILESYSTEM"
            )
        {
            bail!("unsupported Git environment override {name}; retry privacy preflight in the normal repository context");
        }
    }
    let root = fs::canonicalize(
        String::from_utf8(output(project, &["rev-parse", "--show-toplevel"])?)?
            .trim_end_matches('\n'),
    )?;
    let project = fs::canonicalize(project)?;
    let prefix = project.strip_prefix(&root)?;
    let exclude = PathBuf::from(
        String::from_utf8(output(
            &project,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "info/exclude",
            ],
        )?)?
        .trim_end_matches('\n'),
    );
    let tracked = names(&output(
        &root,
        &["ls-files", "--cached", "--full-name", "-z"],
    )?)?;
    let mut paths = names(&output(
        &root,
        &["ls-files", "--cached", "--others", "--full-name", "-z"],
    )?)?;
    for path in requested {
        relative(path)?;
        paths.insert(git_relative(&prefix.join(path))?);
    }
    let temp = tempfile::tempdir()?;
    let shadow = temp.path().join("worktree");
    fs::create_dir(&shadow)?;
    let empty_config = temp.path().join("empty-config");
    fs::write(&empty_config, [])?;
    let mut init = git(&shadow);
    init.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", &empty_config)
        .args(["init", "-q"]);
    if !init.status()?.success() {
        bail!("cannot create Git privacy probe");
    }
    // Override effective settings locally after resolving includes in the real
    // worktree. The probe's own location must not select different includeIfs.
    let mut settings = BTreeMap::new();
    for key in ["core.ignoreCase", "core.symlinks"] {
        let observed = config(&root, key)?;
        settings.insert(key, observed.clone());
        let value = observed.unwrap_or_else(|| {
            if key == "core.symlinks" {
                "true".into()
            } else {
                "false".into()
            }
        });
        output(&shadow, &["config", key, &value])?;
    }
    let configured_global = config(&root, "core.excludesFile")?;
    let global_path = configured_global.clone().map(PathBuf::from).or_else(|| {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|p| p.join("git/ignore"))
    });
    let global_path = global_path.map(|p| if p.is_absolute() { p } else { root.join(p) });
    let shadow_global = temp.path().join("global-ignore");
    let global_bytes = global_path
        .as_ref()
        .map(|p| read_optional(p))
        .transpose()?
        .flatten();
    fs::write(&shadow_global, global_bytes.as_deref().unwrap_or_default())?;
    output(
        &shadow,
        &[
            "config",
            "core.excludesFile",
            shadow_global
                .to_str()
                .ok_or_else(|| anyhow!("non-UTF8 probe"))?,
        ],
    )?;
    let shadow_exclude = shadow.join(".git/info/exclude");
    let exclude_bytes = read_optional(&exclude)?;
    fs::write(
        &shadow_exclude,
        exclude_bytes.as_deref().unwrap_or_default(),
    )?;
    let mut ignores = BTreeMap::new();
    ignores.insert(
        root.join(".gitignore"),
        read_optional(&root.join(".gitignore"))?,
    );
    let mut directories = BTreeSet::new();
    for path in &paths {
        let rel = Path::new(path);
        let mut src = root.clone();
        let mut dst = shadow.clone();
        let parts: Vec<_> = rel.components().collect();
        for (index, part) in parts.iter().enumerate() {
            src.push(part);
            dst.push(part);
            if index + 1 < parts.len() && !directories.insert(src.clone()) {
                continue;
            }
            let metadata = fs::symlink_metadata(&src).ok();
            if index + 1 < parts.len() {
                if metadata
                    .as_ref()
                    .is_some_and(|m| m.file_type().is_symlink())
                {
                    bail!("symlink parent in privacy probe: {path}");
                }
                if src.join(".git").exists() {
                    bail!("nested repository privacy probe unsupported: {path}");
                }
                fs::create_dir_all(&dst)?;
                let ignore = src.join(".gitignore");
                if !ignores.contains_key(&ignore) {
                    ignores.insert(ignore.clone(), read_optional(&ignore)?);
                }
            } else if metadata.as_ref().is_some_and(|m| m.is_dir()) || path.ends_with('/') {
                fs::create_dir_all(&dst)?;
            } else if metadata
                .as_ref()
                .is_some_and(|m| m.file_type().is_symlink())
            {
                if !dst.is_symlink() {
                    #[cfg(unix)]
                    std::os::unix::fs::symlink("__metactl_payload_not_copied__", &dst)?;
                    #[cfg(not(unix))]
                    bail!("symlink privacy probe unsupported on this platform");
                }
            }
            // Git check-ignore treats an absent regular leaf as a regular
            // candidate. Only directory/link shape and ignore-file contents
            // affect matching. Full original-state parity below verifies this
            // assumption before trusting any proposed decisions.
        }
    }
    for (path, bytes) in &ignores {
        if let Some(bytes) = bytes {
            fs::write(shadow.join(path.strip_prefix(&root)?), bytes)?;
        }
    }
    let before = probe(&root, &paths, &exclude, global_path.as_deref())?;
    let shadow_before = probe(&shadow, &paths, &shadow_exclude, Some(&shadow_global))?;
    if before != shadow_before {
        bail!("original Git ignore context parity could not be established");
    }
    for (path, bytes) in writes {
        let absolute = path
            .parent()
            .and_then(|p| fs::canonicalize(p).ok())
            .map(|p| p.join(path.file_name().unwrap()))
            .unwrap_or_else(|| path.clone());
        let destination = if same_source_path(&absolute, &exclude) {
            shadow_exclude.clone()
        } else if path.file_name().is_some_and(|n| n == ".gitignore") {
            shadow.join(absolute.strip_prefix(&root)?)
        } else {
            continue;
        };
        fs::write(destination, bytes)?;
    }
    let after = probe(&shadow, &paths, &shadow_exclude, Some(&shadow_global))?;
    // An editor or config change during the proof invalidates the result.
    for (key, value) in settings {
        if config(&root, key)? != value {
            bail!("effective Git configuration changed during privacy preflight");
        }
    }
    if config(&root, "core.excludesFile")? != configured_global {
        bail!("global excludes selection changed during privacy preflight");
    }
    if read_optional(&exclude)? != exclude_bytes
        || global_path
            .as_ref()
            .map(|p| read_optional(p))
            .transpose()?
            .flatten()
            != global_bytes
    {
        bail!("Git exclude context changed during privacy preflight");
    }
    for (path, bytes) in ignores {
        if read_optional(&path)? != bytes {
            bail!("Git ignore context changed during privacy preflight");
        }
    }
    if probe(&root, &paths, &exclude, global_path.as_deref())? != before {
        bail!("Git ignore decisions changed during privacy preflight");
    }
    let result = paths
        .into_iter()
        .map(|path| {
            let protection = Protection {
                before: before[&path].ignored(),
                after: after[&path].ignored(),
                tracked: tracked.contains(&path),
            };
            (path, protection)
        })
        .collect();
    Ok((root, result))
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    if path == Path::new("/dev/null") {
        return Ok(Some(Vec::new()));
    }
    match fs::symlink_metadata(path) {
        Ok(m) if !m.is_file() => bail!("ignore source must be a regular file: {}", path.display()),
        Ok(_) => Ok(Some(fs::read(path).with_context(|| {
            format!("read ignore source {}", path.display())
        })?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn require_private(project: &Path, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let (root, evidence) = evaluate(project, paths, &[])?;
    let project = fs::canonicalize(project)?;
    let prefix = project.strip_prefix(root)?;
    for path in paths {
        let key = git_relative(&prefix.join(path))?;
        let proof = &evidence[&key];
        if !proof.after || proof.tracked {
            bail!("private destination {path} must be effectively Git ignored and untracked before publication; run metactl ignore install and resolve conflicting rules");
        }
    }
    for (path, proof) in &evidence {
        if Path::new(path)
            .strip_prefix(prefix)
            .ok()
            .is_some_and(|p| p.starts_with(".metactl"))
            && proof.tracked
        {
            bail!("private state path {path} is already tracked; resolve the index before publication");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proposed_state_matches_native_git_with_nested_rules_and_global_excludes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        output(root, &["init", "-q"]).unwrap();
        let global = root.join("global");
        fs::write(&global, "*.secret\n").unwrap();
        output(
            root,
            &["config", "core.excludesFile", global.to_str().unwrap()],
        )
        .unwrap();
        fs::create_dir_all(root.join("nested/.claude")).unwrap();
        fs::write(
            root.join("nested/.claude/.gitignore"),
            "!settings.local.json\n",
        )
        .unwrap();
        let ignore = root.join("nested/.gitignore");
        fs::write(&ignore, ".claude/\n").unwrap();
        fs::write(
            root.join("nested/.claude/settings.local.json"),
            "private fixture\n",
        )
        .unwrap();
        let proposed = b".claude/settings.local.json\n".to_vec();
        let requested = vec![".claude/settings.local.json".into(), "future.secret".into()];
        let (_, proof) = evaluate(
            &root.join("nested"),
            &requested,
            &[(ignore.clone(), proposed.clone())],
        )
        .unwrap();
        assert!(proof["nested/.claude/settings.local.json"].before);
        assert!(!proof["nested/.claude/settings.local.json"].after);
        assert!(proof["nested/future.secret"].after);
        assert_eq!(fs::read(&ignore).unwrap(), b".claude/\n");
        fs::write(&ignore, proposed).unwrap();
        let (_, actual) = evaluate(&root.join("nested"), &requested, &[]).unwrap();
        for name in ["nested/.claude/settings.local.json", "nested/future.secret"] {
            assert_eq!(proof[name].after, actual[name].before);
        }
    }

    #[test]
    fn simultaneous_scopes_and_suffix_negation_are_not_filename_exemptions() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        output(root, &["init", "-q"]).unwrap();
        output(root, &["config", "core.excludesFile", "/dev/null"]).unwrap();
        fs::create_dir(root.join(".claude")).unwrap();
        let private = ".claude/settings.local.json";
        fs::write(root.join(private), "private fixture").unwrap();
        let ignore = root.join(".gitignore");
        let exclude = root.join(".git/info/exclude");
        fs::write(&ignore, ".claude/\n!.claude/settings.local.json\n").unwrap();
        fs::write(&exclude, ".claude/\n").unwrap();
        let (_, proof) = evaluate(
            root,
            &[private.into()],
            &[
                (
                    ignore,
                    b".claude/settings.local.json\n!.claude/settings.local.json\n".to_vec(),
                ),
                (exclude, b".claude/settings.local.json\n".to_vec()),
            ],
        )
        .unwrap();
        assert!(proof[private].before);
        assert!(!proof[private].after);
    }

    #[test]
    fn tracked_private_path_is_rejected_even_when_ignored() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        output(root, &["init", "-q"]).unwrap();
        fs::write(root.join("private.txt"), "private fixture").unwrap();
        output(root, &["add", "private.txt"]).unwrap();
        fs::write(root.join(".gitignore"), "private.txt\n").unwrap();
        assert!(require_private(root, &["private.txt".into()])
            .unwrap_err()
            .to_string()
            .contains("untracked"));
    }
}
