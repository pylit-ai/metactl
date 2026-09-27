//! Native Git evaluation of a proposed ignore state, without copying payloads.
mod index_scope;
use anyhow::{anyhow, bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Write};
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

fn supported_source_variable(name: &str) -> bool {
    matches!(
        name,
        "GIT_CONFIG_GLOBAL"
            | "GIT_CONFIG_SYSTEM"
            | "GIT_CONFIG_NOSYSTEM"
            | "GIT_CONFIG_PARAMETERS"
            | "GIT_CONFIG_COUNT"
            | "GIT_CEILING_DIRECTORIES"
            | "GIT_DISCOVERY_ACROSS_FILESYSTEM"
    ) || ["GIT_CONFIG_KEY_", "GIT_CONFIG_VALUE_"]
        .iter()
        .any(|prefix| {
            name.strip_prefix(prefix).is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
            })
        })
}

fn source_environment() -> BTreeMap<std::ffi::OsString, std::ffi::OsString> {
    std::env::vars_os()
        .filter(|(name, _)| {
            let name = name.to_string_lossy();
            supported_source_variable(&name) || name == "GIT_DIR"
        })
        .collect()
}

fn source_git(root: &Path) -> Command {
    let mut command = git(root);
    for (name, value) in source_environment() {
        // An inherited Git directory is validated independently, then omitted:
        // -C discovers the same trusted repository without carrying redirection.
        if name != "GIT_DIR" {
            command.env(name, value);
        }
    }
    command
}

fn git_failure(stderr: &[u8]) -> String {
    if source_environment().keys().any(|key| {
        let key = key.to_string_lossy();
        key == "GIT_CONFIG_PARAMETERS"
            || key == "GIT_CONFIG_COUNT"
            || key.starts_with("GIT_CONFIG_KEY_")
            || key.starts_with("GIT_CONFIG_VALUE_")
    }) {
        "inherited Git configuration could not be applied".into()
    } else {
        String::from_utf8_lossy(stderr).into_owned()
    }
}

fn validate_repository_context(project: &Path) -> Result<()> {
    if let Some(inherited) = std::env::var_os("GIT_DIR") {
        let trusted = git(project)
            .args(["rev-parse", "--absolute-git-dir"])
            .output()?;
        let selected = git(project)
            .env("GIT_DIR", inherited)
            .args(["rev-parse", "--absolute-git-dir"])
            .output()?;
        if !trusted.status.success() || !selected.status.success() {
            bail!("unsupported Git environment override GIT_DIR; cannot establish repository identity");
        }
        let trusted = fs::canonicalize(String::from_utf8(trusted.stdout)?.trim_end_matches('\n'))?;
        let selected =
            fs::canonicalize(String::from_utf8(selected.stdout)?.trim_end_matches('\n'))?;
        if trusted != selected {
            bail!("unsupported Git environment override GIT_DIR; selected repository differs from project");
        }
    }
    Ok(())
}

fn output(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    command_output(source_git(root).args(args))
}

fn shadow_output(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    command_output(git(root).args(args))
}

fn command_output(command: &mut Command) -> Result<Vec<u8>> {
    let out = command.output()?;
    if !out.status.success() {
        bail!("Git privacy probe failed: {}", git_failure(&out.stderr));
    }
    Ok(out.stdout)
}

#[derive(Debug, PartialEq, Eq)]
struct IndexStamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
    created: Option<std::time::SystemTime>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

fn index_stamp(metadata: &fs::Metadata) -> IndexStamp {
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    IndexStamp {
        len: metadata.len(),
        modified: metadata.modified().ok(),
        created: metadata.created().ok(),
        #[cfg(unix)]
        device: metadata.dev(),
        #[cfg(unix)]
        inode: metadata.ino(),
    }
}

fn regular_index(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    metadata.is_file() && !metadata.file_type().is_symlink()
}

#[derive(Debug, PartialEq, Eq)]
struct IndexSnapshot {
    parent: PathBuf,
    state: Option<(IndexStamp, Vec<u8>)>,
}

fn snapshot_index(path: &Path) -> Result<IndexSnapshot> {
    let parent_path = path
        .parent()
        .ok_or_else(|| anyhow!("Git index has no parent"))?;
    let parent = fs::canonicalize(parent_path)?;
    let resolved = parent.join(
        path.file_name()
            .ok_or_else(|| anyhow!("Git index has no filename"))?,
    );
    let metadata = match fs::symlink_metadata(&resolved) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(IndexSnapshot {
                parent,
                state: None,
            });
        }
        Err(error) => return Err(error.into()),
    };
    if !regular_index(&metadata) {
        bail!("selected Git index must be a regular file");
    }
    let stamp = index_stamp(&metadata);
    let mut file = fs::File::open(&resolved)?;
    if index_stamp(&file.metadata()?) != stamp {
        bail!("selected Git index changed during snapshot");
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let current = fs::symlink_metadata(&resolved)?;
    if !regular_index(&current)
        || index_stamp(&current) != stamp
        || index_stamp(&file.metadata()?) != stamp
        || fs::canonicalize(parent_path)? != parent
    {
        bail!("selected Git index changed during snapshot");
    }
    Ok(IndexSnapshot {
        parent,
        state: Some((stamp, bytes)),
    })
}

struct SelectedIndex {
    project: PathBuf,
    inherited: Option<std::ffi::OsString>,
    path: PathBuf,
    effective: PathBuf,
    snapshot: IndexSnapshot,
}

#[derive(Debug, PartialEq, Eq)]
struct JunctionSnapshot {
    link: PathBuf,
    resolved: PathBuf,
}

fn namespace_junction(path: &Path) -> Result<Option<JunctionSnapshot>> {
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(None)
    }
    #[cfg(windows)]
    {
        use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
        #[repr(C)]
        struct AttributeTag {
            attributes: u32,
            tag: u32,
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetFileInformationByHandleEx(
                handle: *mut std::ffi::c_void,
                class: i32,
                info: *mut std::ffi::c_void,
                size: u32,
            ) -> i32;
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .access_mode(0)
            .share_mode(7)
            .custom_flags(0x00200000 | 0x02000000)
            .open(path)?;
        let mut info = AttributeTag {
            attributes: 0,
            tag: 0,
        };
        let ok = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                9,
                (&mut info as *mut AttributeTag).cast(),
                std::mem::size_of::<AttributeTag>() as u32,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // IO_REPARSE_TAG_MOUNT_POINT identifies junctions; directory symlinks
        // and unknown reparse kinds retain the strict parent refusal.
        if info.tag != 0xA0000003 {
            return Ok(None);
        }
        let link = fs::read_link(path)?;
        let resolved = fs::canonicalize(path)?;
        if !fs::metadata(&resolved)?.is_dir() {
            bail!("junction target must be a directory");
        }
        if fs::read_link(path)? != link || fs::canonicalize(path)? != resolved {
            bail!("junction changed during privacy snapshot");
        }
        Ok(Some(JunctionSnapshot { link, resolved }))
    }
}

impl SelectedIndex {
    fn capture(project: &Path) -> Result<Self> {
        // Hooks may inherit an alternate or commit -a temporary index. Restore
        // only that selection for source reads; every shadow command stays clean.
        let inherited = std::env::var_os("GIT_INDEX_FILE");
        Self::from_selection(project, inherited)
    }

    fn from_selection(project: &Path, inherited: Option<std::ffi::OsString>) -> Result<Self> {
        let path = Self::selection(project, inherited.as_deref())?;
        let snapshot = snapshot_index(&path)?;
        let effective = Self::resolve(project, inherited.as_deref())?;
        Ok(Self {
            project: project.to_owned(),
            inherited,
            path,
            effective,
            snapshot,
        })
    }

    fn selection(project: &Path, inherited: Option<&std::ffi::OsStr>) -> Result<PathBuf> {
        if let Some(value) = inherited {
            let path = PathBuf::from(value);
            return Ok(if path.is_absolute() {
                path
            } else {
                project.join(path)
            });
        }
        // Do not resolve the index leaf here: --git-path with absolute output
        // can resolve a symlink and conceal the unsafe selected leaf itself.
        let directory = output(project, &["rev-parse", "--absolute-git-dir"])?;
        Ok(PathBuf::from(String::from_utf8(directory)?.trim_end_matches('\n')).join("index"))
    }

    fn resolve(project: &Path, inherited: Option<&std::ffi::OsStr>) -> Result<PathBuf> {
        let mut command = source_git(project);
        if let Some(value) = inherited {
            command.env("GIT_INDEX_FILE", value);
        }
        let result = command
            .args(["rev-parse", "--path-format=absolute", "--git-path", "index"])
            .output()?;
        if !result.status.success() {
            bail!(
                "cannot resolve selected Git index: {}",
                git_failure(&result.stderr)
            );
        }
        Ok(PathBuf::from(
            String::from_utf8(result.stdout)?.trim_end_matches('\n'),
        ))
    }

    fn output(&self, root: &Path, args: &[&str]) -> Result<Vec<u8>> {
        let result = source_git(root)
            .env("GIT_INDEX_FILE", git_path_argument(&self.path))
            .args(args)
            .output()?;
        if !result.status.success() {
            bail!(
                "selected Git index enumeration failed: {}",
                git_failure(&result.stderr)
            );
        }
        Ok(result.stdout)
    }

    fn revalidate(&self) -> Result<()> {
        if Self::selection(&self.project, self.inherited.as_deref())? != self.path
            || Self::resolve(&self.project, self.inherited.as_deref())? != self.effective
            || snapshot_index(&self.path)? != self.snapshot
        {
            bail!("selected Git index changed during privacy preflight");
        }
        Ok(())
    }
}

fn config(root: &Path, key: &str) -> Result<Option<String>> {
    let out = source_git(root)
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

fn ignore_case(root: &Path) -> Result<bool> {
    let output = source_git(root)
        .args(["config", "--bool", "--get", "core.ignoreCase"])
        .output()?;
    match output.status.code() {
        Some(0) => Ok(output.stdout == b"true\n"),
        Some(1) => Ok(false),
        _ => bail!("cannot resolve effective Git core.ignoreCase"),
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
            let name = std::str::from_utf8(p)
                .context("non-UTF8 Git path")?
                .to_string();
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
    source: bool,
) -> Result<BTreeMap<String, Match>> {
    if paths.is_empty() {
        return Ok(BTreeMap::new());
    }
    let input: Vec<u8> = paths.iter().flat_map(|p| p.bytes().chain([0])).collect();
    let mut command = if source { source_git(root) } else { git(root) };
    let mut child = command
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
            git_failure(&result.stderr)
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
    evaluate_scoped(project, requested, writes, false)
}

fn evaluate_scoped(
    project: &Path,
    requested: &[String],
    writes: &[(PathBuf, Vec<u8>)],
    publication_only: bool,
) -> Result<(PathBuf, BTreeMap<String, Protection>)> {
    let environment = source_environment();
    for (name, _) in std::env::vars_os() {
        let name = name.to_string_lossy();
        if (name.starts_with("GIT_CONFIG") && !supported_source_variable(&name))
            || matches!(name.as_ref(), "GIT_WORK_TREE" | "GIT_COMMON_DIR")
        {
            bail!("unsupported Git environment override {name}; retry privacy preflight in the normal repository context");
        }
    }
    validate_repository_context(project)?;
    let root = fs::canonicalize(
        String::from_utf8(output(project, &["rev-parse", "--show-toplevel"])?)?
            .trim_end_matches('\n'),
    )?;
    let trusted = command_output(git(project).args(["rev-parse", "--show-toplevel"]))?;
    if fs::canonicalize(String::from_utf8(trusted)?.trim_end_matches('\n'))? != root {
        bail!("effective Git worktree differs from independently discovered project");
    }
    let project = fs::canonicalize(project)?;
    let prefix = project.strip_prefix(&root)?;
    // Reading an ignore source may follow Git's source-specific link rules;
    // proposed publication destinations must still be ordinary files.
    for (path, _) in writes {
        // An empty junction need not appear in Git's enumeration. Validate
        // every proposed destination independently of observed worktree paths.
        for ancestor in path.ancestors() {
            if fs::symlink_metadata(ancestor).is_ok_and(|m| m.file_type().is_symlink())
                && namespace_junction(ancestor)?.is_some()
            {
                bail!(
                    "junction private destination unsupported: {}",
                    path.display()
                );
            }
        }
        read_optional(path)?;
    }
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
    let index = SelectedIndex::capture(&project)?;
    let ignore_case = ignore_case(&root)?;
    let mut scopes = Vec::new();
    for path in requested {
        relative(path)?;
        scopes.push(git_relative(&prefix.join(path))?);
    }
    let private_state = git_relative(&prefix.join(".metactl"))?;
    let mut identities = index_scope::Identities::new();
    let cached = index.output(&root, &["ls-files", "--cached", "--full-name", "-z"])?;
    let tracked = if publication_only {
        let mut tracked = BTreeSet::new();
        for name in cached.split(|b| *b == 0).filter(|name| !name.is_empty()) {
            for scope in scopes.iter().chain(std::iter::once(&private_state)) {
                if index_scope::overlaps(&root, name, scope, &mut identities)? {
                    tracked.extend(names(name)?);
                    break;
                }
            }
        }
        tracked
    } else {
        names(&cached)?
    };
    let mut paths = if publication_only {
        tracked.clone()
    } else {
        names(&index.output(
            &root,
            &["ls-files", "--cached", "--others", "--full-name", "-z"],
        )?)?
    };
    index.revalidate()?;
    if publication_only {
        if let Some(path) = tracked.iter().next() {
            bail!("private destination or state path {path} is already tracked and must be untracked before private publication; resolve the index");
        }
    }
    let observed_paths = paths.clone();
    let requested_paths: Vec<PathBuf> = requested.iter().map(|path| project.join(path)).collect();
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
        shadow_output(&shadow, &["config", key, &value])?;
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
    let global_source = global_path
        .as_ref()
        .map(|p| {
            read_global_ignore(p)
                .map_err(|_| anyhow!("cannot snapshot effective global excludes source"))
        })
        .transpose()?;
    fs::write(
        &shadow_global,
        global_source
            .as_ref()
            .and_then(IgnoreSource::bytes)
            .unwrap_or_default(),
    )?;
    shadow_output(
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
        read_worktree_ignore(&root.join(".gitignore"))?,
    );
    let mut directories = BTreeSet::new();
    let mut junctions = BTreeMap::new();
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
            let junction = if metadata
                .as_ref()
                .is_some_and(|m| m.file_type().is_symlink())
            {
                namespace_junction(&src)?
            } else {
                None
            };
            if let Some(snapshot) = &junction {
                let writes_through = writes.iter().any(|(path, _)| {
                    path.starts_with(&src)
                        || path
                            .parent()
                            .and_then(|p| fs::canonicalize(p).ok())
                            .is_some_and(|p| p.starts_with(&snapshot.resolved))
                });
                if !observed_paths.contains(path)
                    || requested_paths.iter().any(|p| p.starts_with(&src))
                    || writes_through
                {
                    bail!("junction private destination unsupported: {path}");
                }
            }
            if let Some(snapshot) = junction.as_ref() {
                if let Some(previous) = junctions.get(&src) {
                    if previous != snapshot {
                        bail!("junction changed during privacy preflight");
                    }
                }
            }
            let is_junction = junction.is_some();
            if let Some(snapshot) = junction {
                junctions.insert(src.clone(), snapshot);
            }
            if index + 1 < parts.len() {
                if metadata
                    .as_ref()
                    .is_some_and(|m| m.file_type().is_symlink())
                    && !is_junction
                {
                    bail!("symlink parent in privacy probe: {path}");
                }
                if src.join(".git").exists() {
                    bail!("nested repository privacy probe unsupported: {path}");
                }
                fs::create_dir_all(&dst)?;
                let ignore = src.join(".gitignore");
                if !ignores.contains_key(&ignore) {
                    ignores.insert(ignore.clone(), read_worktree_ignore(&ignore)?);
                }
            } else if is_junction
                || metadata.as_ref().is_some_and(|m| m.is_dir())
                || path.ends_with('/')
            {
                fs::create_dir_all(&dst)?;
            } else if metadata
                .as_ref()
                .is_some_and(|m| m.file_type().is_symlink())
            {
                if !dst.is_symlink() {
                    #[cfg(unix)]
                    std::os::unix::fs::symlink("__metactl_payload_not_copied__", &dst)?;
                    #[cfg(not(unix))]
                    // Git matches a symlink leaf as a non-directory. Preserve
                    // that shape without needing symlink privileges or copying
                    // its target; original-state parity below remains required.
                    fs::write(&dst, [])?;
                }
            }
            // Git check-ignore treats an absent regular leaf as a regular
            // candidate. Only directory/link shape and ignore-file contents
            // affect matching. Full original-state parity below verifies this
            // assumption before trusting any proposed decisions.
        }
    }
    for (path, source) in &ignores {
        if let Some(bytes) = source.bytes() {
            fs::write(shadow.join(path.strip_prefix(&root)?), bytes)?;
        }
    }
    let before = probe(&root, &paths, &exclude, global_path.as_deref(), true)?;
    let shadow_before = probe(
        &shadow,
        &paths,
        &shadow_exclude,
        Some(&shadow_global),
        false,
    )?;
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
    let after = probe(
        &shadow,
        &paths,
        &shadow_exclude,
        Some(&shadow_global),
        false,
    )?;
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
            .map(|p| read_global_ignore(p))
            .transpose()?
            != global_source
    {
        bail!("Git exclude context changed during privacy preflight");
    }
    for (path, source) in ignores {
        if read_worktree_ignore(&path)? != source {
            bail!("Git ignore context changed during privacy preflight");
        }
    }
    if probe(&root, &paths, &exclude, global_path.as_deref(), true)? != before {
        bail!("Git ignore decisions changed during privacy preflight");
    }
    index.revalidate()?;
    if source_environment() != environment || self::ignore_case(&root)? != ignore_case {
        bail!("effective Git input context changed during privacy preflight");
    }
    validate_repository_context(&project)?;
    if fs::canonicalize(
        String::from_utf8(output(&project, &["rev-parse", "--show-toplevel"])?)?
            .trim_end_matches('\n'),
    )? != root
    {
        bail!("effective Git worktree changed during privacy preflight");
    }
    index_scope::revalidate(&identities)?;
    for (path, snapshot) in junctions {
        if namespace_junction(&path)? != Some(snapshot) {
            bail!("junction changed during privacy preflight");
        }
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

#[derive(Debug, PartialEq, Eq)]
enum IgnoreSource {
    Missing,
    Regular(Vec<u8>),
    SkippedSymlink(PathBuf),
    FollowedSymlink {
        link: PathBuf,
        resolved: PathBuf,
        bytes: Vec<u8>,
    },
}

impl IgnoreSource {
    fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Regular(bytes) | Self::FollowedSymlink { bytes, .. } => Some(bytes),
            Self::Missing | Self::SkippedSymlink(_) => None,
        }
    }
}

fn read_worktree_ignore(path: &Path) -> Result<IgnoreSource> {
    // Git never follows worktree .gitignore symlinks. Record the link itself
    // for revalidation without opening or resolving its potentially private target.
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Ok(IgnoreSource::SkippedSymlink(fs::read_link(path)?));
    }
    Ok(read_optional(path)?
        .map(IgnoreSource::Regular)
        .unwrap_or(IgnoreSource::Missing))
}

fn read_global_ignore(path: &Path) -> Result<IgnoreSource> {
    // core.excludesFile follows symlinks, unlike a worktree .gitignore. Snapshot
    // selection identity as well as target bytes so retargeting fails revalidation.
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        let link = fs::read_link(path)?;
        let resolved = fs::canonicalize(path)?;
        let bytes =
            read_optional(&resolved)?.ok_or_else(|| anyhow!("global ignore target disappeared"))?;
        if fs::read_link(path)? != link || fs::canonicalize(path)? != resolved {
            bail!("global ignore symlink changed during snapshot");
        }
        return Ok(IgnoreSource::FollowedSymlink {
            link,
            resolved,
            bytes,
        });
    }
    Ok(read_optional(path)?
        .map(IgnoreSource::Regular)
        .unwrap_or(IgnoreSource::Missing))
}

pub fn require_private(project: &Path, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let (root, evidence) = evaluate_scoped(project, paths, &[], true)?;
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
    fn publication_scope_rejects_tracked_directories_and_native_aliases() {
        for (stored, requested) in [
            ("state", "state"),
            ("STATE", "state"),
            ("ſtate", "state"),
            ("Kelvin", "kelvin"),
            ("ÄREA", "ärea"),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path();
            output(root, &["init", "-q"]).unwrap();
            output(root, &["config", "core.ignoreCase", "false"]).unwrap();
            // Keep the independently protected state root concrete so this
            // fixture isolates aliases of the requested destination.
            fs::create_dir(root.join(".metactl")).unwrap();
            fs::create_dir(root.join(stored)).unwrap();
            fs::write(root.join(stored).join("private.txt"), "synthetic private").unwrap();
            fs::write(
                root.join(".gitignore"),
                format!("{stored}/\n{requested}/\n"),
            )
            .unwrap();
            output(root, &["add", "--force", &format!("{stored}/private.txt")]).unwrap();
            let aliases = root.join(requested).join("private.txt").exists();
            if !aliases {
                fs::create_dir(root.join(requested)).unwrap();
            }
            let result = require_private(root, &[format!("{requested}/")]);
            if aliases {
                assert!(result.unwrap_err().to_string().contains("tracked"));
            } else {
                result.unwrap();
            }
            fs::remove_file(root.join(stored).join("private.txt")).unwrap();
            let result = require_private(root, &[format!("{requested}/private.txt")]);
            if aliases {
                assert!(result.unwrap_err().to_string().contains("tracked"));
            } else {
                result.unwrap();
            }
        }
    }

    #[test]
    fn publication_scope_refuses_uncertain_unicode_overlap_with_missing_state() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        output(root, &["init", "-q"]).unwrap();
        fs::create_dir(root.join("ſtate")).unwrap();
        fs::create_dir(root.join("private")).unwrap();
        fs::write(root.join("ſtate/tracked.txt"), "synthetic tracked").unwrap();
        fs::write(root.join(".gitignore"), "private/\n.metactl/\n").unwrap();
        output(root, &["add", "ſtate/tracked.txt"]).unwrap();
        let result = require_private(root, &["private/".into()]);
        assert!(result.unwrap_err().to_string().contains("tracked"));
        // Both directory identities now establish that the tracked Unicode
        // path is distinct from every protected scope.
        fs::create_dir(root.join(".metactl")).unwrap();
        require_private(root, &["private/".into()]).unwrap();
    }

    #[test]
    fn publication_scope_respects_literal_prefixes_nested_ignores_and_tracked_ancestors() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        output(root, &["init", "-q"]).unwrap();
        for path in [
            "nested/private[1]",
            "nested/private1",
            "nested/.metactl-other",
        ] {
            fs::create_dir_all(root.join(path)).unwrap();
        }
        fs::write(root.join(".gitignore"), "nested/private*/\n").unwrap();
        fs::write(root.join("nested/private1/file"), "unrelated").unwrap();
        fs::write(root.join("nested/.metactl-other/file"), "unrelated").unwrap();
        output(
            root,
            &[
                "add",
                "--force",
                "nested/private1/file",
                "nested/.metactl-other/file",
            ],
        )
        .unwrap();
        require_private(&root.join("nested"), &["private[1]/".into()]).unwrap();
        fs::write(root.join("nested/.gitignore"), "!private[[]1]/\n").unwrap();
        assert!(
            require_private(&root.join("nested"), &["private[1]/".into()])
                .unwrap_err()
                .to_string()
                .contains("effectively Git ignored")
        );
        fs::remove_file(root.join("nested/.gitignore")).unwrap();
        fs::write(root.join("ancestor"), "tracked file replaced by directory").unwrap();
        output(root, &["add", "ancestor"]).unwrap();
        fs::remove_file(root.join("ancestor")).unwrap();
        fs::create_dir(root.join("ancestor")).unwrap();
        fs::write(root.join(".gitignore"), "ancestor/\n").unwrap();
        assert!(require_private(root, &["ancestor/private.txt".into()])
            .unwrap_err()
            .to_string()
            .contains("tracked"));
    }

    #[cfg(unix)]
    #[test]
    fn selected_index_revalidation_rejects_same_bytes_parent_retarget() {
        let temp = tempfile::tempdir().unwrap();
        output(temp.path(), &["init", "-q"]).unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        let alias = temp.path().join("alias");
        for directory in [&first, &second] {
            fs::create_dir(directory).unwrap();
            fs::write(directory.join("index"), "same index bytes").unwrap();
        }
        std::os::unix::fs::symlink(&first, &alias).unwrap();
        let selected =
            SelectedIndex::from_selection(temp.path(), Some(alias.join("index").into_os_string()))
                .unwrap();
        selected.revalidate().unwrap();
        fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(&second, &alias).unwrap();
        assert!(selected
            .revalidate()
            .unwrap_err()
            .to_string()
            .contains("changed"));
    }

    #[test]
    fn selected_index_revalidation_rejects_mutation_and_creation() {
        let temp = tempfile::tempdir().unwrap();
        output(temp.path(), &["init", "-q"]).unwrap();
        let selected = SelectedIndex::capture(temp.path()).unwrap();
        selected.revalidate().unwrap();
        fs::write(&selected.path, "created after enumeration").unwrap();
        assert!(selected
            .revalidate()
            .unwrap_err()
            .to_string()
            .contains("changed"));
        let snapshot = snapshot_index(&selected.path).unwrap();
        let selected = SelectedIndex {
            snapshot,
            ..selected
        };
        fs::write(&selected.path, "mutated after enumeration").unwrap();
        assert!(selected
            .revalidate()
            .unwrap_err()
            .to_string()
            .contains("changed"));
    }

    #[cfg(unix)]
    #[test]
    fn ignore_source_snapshots_distinguish_skipped_followed_and_retargeted_links() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        let link = temp.path().join("link");
        fs::write(&first, "*.private\n").unwrap();
        fs::write(&second, "*.private\n").unwrap();
        std::os::unix::fs::symlink(&first, &link).unwrap();
        let skipped = read_worktree_ignore(&link).unwrap();
        let followed = read_global_ignore(&link).unwrap();
        assert!(skipped.bytes().is_none());
        assert_eq!(followed.bytes().unwrap(), b"*.private\n");
        assert!(
            read_optional(&link).is_err(),
            "publication target remains strict"
        );
        fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&second, &link).unwrap();
        assert_ne!(skipped, read_worktree_ignore(&link).unwrap());
        assert_ne!(followed, read_global_ignore(&link).unwrap());
        fs::write(&second, "changed\n").unwrap();
        assert_ne!(followed, read_global_ignore(&link).unwrap());
        fs::remove_file(&second).unwrap();
        assert!(read_worktree_ignore(&link).unwrap().bytes().is_none());
        assert!(
            read_global_ignore(&link).is_err(),
            "broken external link fails closed"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_link_leaf_proposals_match_native_directory_only_rules() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        let external = temp.path().join("external");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&external).unwrap();
        output(&root, &["init", "-q"]).unwrap();
        fs::write(external.join("payload"), "must stay outside probe").unwrap();
        fs::write(root.join("ordinary"), "ordinary payload").unwrap();
        fs::create_dir(root.join("directory")).unwrap();
        std::os::windows::fs::symlink_file(external.join("payload"), root.join("file-link"))
            .unwrap();
        std::os::windows::fs::symlink_file(external.join("missing"), root.join("dangling-link"))
            .unwrap();
        std::os::windows::fs::symlink_dir(&external, root.join("directory-link")).unwrap();
        let junction = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(root.join("junction"))
            .arg(&external)
            .output()
            .unwrap();
        assert!(
            junction.status.success(),
            "{}",
            String::from_utf8_lossy(&junction.stderr)
        );
        let names: Vec<String> = [
            "ordinary",
            "directory",
            "file-link",
            "dangling-link",
            "directory-link",
            "junction",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let ignore = root.join(".gitignore");
        for proposed in [
            names
                .iter()
                .map(|name| format!("{name}/\n"))
                .collect::<String>(),
            format!(
                "*\n{}",
                names
                    .iter()
                    .map(|name| format!("!{name}/\n"))
                    .collect::<String>()
            ),
        ] {
            fs::write(&ignore, []).unwrap();
            let requested: Vec<String> = names
                .iter()
                .filter(|name| name.as_str() != "junction")
                .cloned()
                .collect();
            let checked: Vec<String> = requested
                .iter()
                .cloned()
                .chain(["junction/payload".into()])
                .collect();
            let (_, predicted) = evaluate(
                &root,
                &requested,
                &[(ignore.clone(), proposed.as_bytes().to_vec())],
            )
            .unwrap();
            fs::write(&ignore, &proposed).unwrap();
            for name in &checked {
                let native = git(&root)
                    .args(["check-ignore", "--no-index", "--quiet", name])
                    .status()
                    .unwrap();
                assert!(matches!(native.code(), Some(0 | 1)));
                assert_eq!(
                    predicted[name].after,
                    native.success(),
                    "{name}: {proposed}"
                );
            }
        }
        for requested in ["junction", "junction/", "junction/payload"] {
            let error = evaluate(&root, &[requested.into()], &[]).unwrap_err();
            assert!(
                error.to_string().contains("junction private destination"),
                "{error}"
            );
        }
        let error = evaluate(
            &root,
            &[],
            &[(root.join("junction/.gitignore"), b"*\n".to_vec())],
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("junction private destination"),
            "{error}"
        );
        assert_eq!(
            fs::read_to_string(external.join("payload")).unwrap(),
            "must stay outside probe"
        );
        // An empty junction contributes no enumerated child. Publication must
        // still refuse it even when its internal target is otherwise modeled.
        let empty = root.join("empty-target");
        fs::create_dir(&empty).unwrap();
        let output = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(root.join("empty-junction"))
            .arg(&empty)
            .output()
            .unwrap();
        assert!(output.status.success());
        let error = evaluate(
            &root,
            &["empty-target/".into()],
            &[(root.join("empty-junction/.gitignore"), b"*\n".to_vec())],
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("junction private destination"),
            "{error}"
        );
        assert!(!empty.join(".gitignore").exists());
    }

    #[test]
    fn git_name_decoder_rejects_non_utf8_with_path_diagnostic() {
        let error = names(b"valid.private\0bad-\xff.private\0").unwrap_err();
        assert_eq!(error.to_string(), "non-UTF8 Git path");
        assert!(format!("{error:#}").contains("invalid utf-8"));
        assert_eq!(
            names("caf\u{e9}.private\0".as_bytes()).unwrap(),
            BTreeSet::from(["caf\u{e9}.private".to_string()])
        );
    }

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
