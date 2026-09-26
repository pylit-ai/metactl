use super::*;

fn git_worktree_present(project_root: &Path) -> Result<bool> {
    let probe = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()?;
    if probe.status.success() {
        return match probe.stdout.as_slice() {
            b"true\n" => Ok(true),
            b"false\n" => Ok(false),
            _ => Err(anyhow!("Git worktree probe returned an invalid response")),
        };
    }

    // Git's non-repository diagnostic and exit code are not a stable API.
    // A worktree (including linked worktrees and submodules) has a .git
    // directory or indirection file in its ancestry. Some non-Git projects
    // use an info-only .git stub for local exclude rules; it cannot stage
    // files, so it is not evidence of a Git worktree.
    let explicit_git_dir = ["GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR"]
        .iter()
        .any(|key| std::env::var_os(key).is_some());
    if explicit_git_dir {
        return Err(anyhow!(
            "Git worktree probe failed for an explicit Git repository"
        ));
    }
    let resolved_root = fs::canonicalize(project_root)?;
    for ancestor in resolved_root.ancestors() {
        let marker = ancestor.join(".git");
        match fs::symlink_metadata(&marker) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                let mut entries = fs::read_dir(&marker)?;
                let info_only = entries.try_fold(true, |only_info, entry| {
                    let entry = entry?;
                    Ok::<_, io::Error>(only_info && entry.file_name() == "info")
                })?;
                if info_only {
                    continue;
                }
                return Err(anyhow!(
                    "Git worktree probe failed near {}",
                    marker.display()
                ));
            }
            Ok(_) => {
                return Err(anyhow!(
                    "Git worktree probe failed near {}",
                    marker.display()
                ))
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(anyhow!("inspect {}: {err}", marker.display())),
        }
    }
    Ok(false)
}

pub(super) fn ensure_ignore_recovery_dir(
    project_root: &Path,
) -> std::result::Result<PathBuf, CliError> {
    let in_git_worktree = git_worktree_present(project_root).map_err(state_error)?;
    let state = project_root.join(".metactl");
    let recovery = state.join("ignore-recovery");
    for dir in [&state, &recovery] {
        match fs::symlink_metadata(dir) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(CliError::new(
                    EXIT_STATE,
                    format!(
                        "Ignore recovery path is not a real directory: {}",
                        dir.display()
                    ),
                ))
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    let mut builder = fs::DirBuilder::new();
                    builder.mode(0o700);
                    builder.create(dir).map_err(internal_error)?;
                }
                #[cfg(not(unix))]
                fs::create_dir(dir).map_err(internal_error)?;
            }
            Err(err) => return Err(state_error(anyhow!("inspect {}: {err}", dir.display()))),
        }
    }
    #[cfg(unix)]
    if fs::metadata(&recovery)
        .map_err(internal_error)?
        .permissions()
        .mode()
        & 0o077
        != 0
    {
        return Err(CliError::new(
            EXIT_STATE,
            format!(
                "Ignore recovery directory is not private: {}",
                recovery.display()
            ),
        ));
    }
    // A failed multi-file repair can restore the old .git/info/exclude before
    // the final staging check. Protect retained copies independently of both
    // ignore files being repaired, including on that rollback path.
    let guard = recovery.join(".gitignore");
    let probe = recovery.join(".metactl-recovery-probe");
    if in_git_worktree {
        let probe_text = probe.to_str().ok_or_else(|| {
            state_error(anyhow!(
                "non-UTF-8 ignore recovery path: {}",
                probe.display()
            ))
        })?;
        let mut prior_check = Command::new("git")
            .arg("-C")
            .arg(project_root)
            .args(["check-ignore", "--verbose", "-z", "--stdin", "--no-index"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(internal_error)?;
        let mut prior_input = prior_check
            .stdin
            .take()
            .ok_or_else(|| internal_error(anyhow!("git check-ignore stdin unavailable")))?;
        prior_input
            .write_all(probe_text.as_bytes())
            .and_then(|()| prior_input.write_all(&[0]))
            .map_err(internal_error)?;
        drop(prior_input);
        let prior = prior_check.wait_with_output().map_err(internal_error)?;
        if !prior.status.success() && prior.status.code() != Some(1) {
            return Err(state_error(anyhow!(
                "inspect ignore recovery rules: {}",
                String::from_utf8_lossy(&prior.stderr)
            )));
        }
        let fields: Vec<_> = prior.stdout.split(|byte| *byte == 0).collect();
        if fields
            .get(2)
            .is_some_and(|pattern| pattern.starts_with(b"!"))
        {
            return Err(state_error(anyhow!(
                "recovery copy path is explicitly unignored: {}",
                recovery.display()
            )));
        }
    }
    match fs::symlink_metadata(&guard) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            if fs::read(&guard).map_err(internal_error)? != b"*\n" {
                return Err(state_error(anyhow!(
                    "recovery protection file is user-modified: {}",
                    guard.display()
                )));
            }
        }
        Ok(_) => {
            return Err(state_error(anyhow!(
                "recovery protection path is not a regular file: {}",
                guard.display()
            )));
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&guard)
                .map_err(internal_error)?;
            file.write_all(b"*\n").map_err(internal_error)?;
            file.sync_all().map_err(internal_error)?;
        }
        Err(err) => return Err(state_error(anyhow!("inspect {}: {err}", guard.display()))),
    }
    if in_git_worktree {
        recovery_copies_ignored_by_git(project_root, &[(0, guard), (0, probe)])
            .map_err(state_error)?;
    }
    Ok(recovery)
}

pub(super) fn recovery_copies_ignored_by_git(
    project_root: &Path,
    written: &[(usize, PathBuf)],
) -> Result<()> {
    if written.is_empty() {
        return Ok(());
    }
    if !git_worktree_present(project_root)? {
        return Ok(());
    }
    for (_, backup) in written {
        let output = Command::new("git")
            .arg("-C")
            .arg(project_root)
            .args(["check-ignore", "--quiet", "--no-index", "--"])
            .arg(backup)
            .output()?;
        if !output.status.success() {
            return Err(anyhow!(
                "recovery copy is eligible for Git staging: {}",
                backup.display()
            ));
        }
    }
    Ok(())
}
