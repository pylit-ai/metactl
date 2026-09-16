//! Best-effort access checks before any managed destination is changed.
//!
//! Probes exercise actual operations rather than guessing from mode bits (ACLs,
//! elevated privileges, and read-only mounts can all change effective access).
//! They cannot guarantee later writes: races and target-specific restrictions
//! still require the materializer's snapshots and compensation.

use super::{destination_path_fallback, ensure_contained_regular_path, ActionKind, PlannedAction};
use anyhow::{anyhow, Context, Result};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub(super) fn check_planned_access(
    root: &Path,
    plans: &[Result<PlannedAction>],
    state_path: &Path,
    journal_path: &Path,
) -> Result<()> {
    let mut write_paths = Vec::new();
    for plan in plans.iter().filter_map(|plan| plan.as_ref().ok()) {
        if !matches!(plan.kind, ActionKind::Noop) {
            write_paths.push(root.join(destination_path_fallback(&plan.output)));
            if plan.existed_before
                && matches!(
                    plan.kind,
                    ActionKind::MergeJsonUnmanaged
                        | ActionKind::PatchUnmanaged
                        | ActionKind::TakeoverUnmanaged
                )
            {
                if let Some(backup_path) = &plan.backup_path {
                    write_paths.push(backup_path.clone());
                }
            }
        }
    }
    write_paths.push(state_path.to_path_buf());
    write_paths.push(journal_path.to_path_buf());
    check_write_paths(root, &write_paths)
}

pub(super) fn check_write_paths(root: &Path, paths: &[PathBuf]) -> Result<()> {
    let mut checked_parents = BTreeSet::new();
    for path in paths {
        let relative = path
            .strip_prefix(root)
            .context("preflight path escapes project")?;
        ensure_contained_regular_path(root, relative, true)
            .with_context(|| format!("access preflight for {}", path.display()))?;
        if let Ok(metadata) = fs::symlink_metadata(path) {
            if !metadata.is_file() && !metadata.file_type().is_symlink() {
                return Err(anyhow!(
                    "access preflight: {} is not a regular file",
                    path.display()
                ));
            }
        }
        let requested_parent = path.parent().context("preflight path has no parent")?;
        let mut parent = requested_parent;
        loop {
            // Containment above rejects symlink traversal below the project root.
            // The root itself may be a supported user-selected directory alias.
            match fs::metadata(parent) {
                Ok(metadata) if metadata.is_dir() => break,
                Ok(_) => {
                    return Err(anyhow!(
                        "access preflight: {} is not a directory",
                        parent.display()
                    ))
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    parent = parent
                        .parent()
                        .context("preflight cannot find existing parent")?;
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("access preflight: inspect {}", parent.display()))
                }
            }
        }
        let needs_directory = requested_parent != parent;
        if checked_parents.insert((parent.to_path_buf(), needs_directory)) {
            probe_parent(parent).with_context(|| {
                format!(
                    "access preflight for {} in {}",
                    path.display(),
                    parent.display()
                )
            })?;
            if needs_directory {
                probe_missing_directory(parent).with_context(|| {
                    format!("access preflight: create parent for {}", path.display())
                })?;
            }
        }
    }
    Ok(())
}

fn probe_missing_directory(parent: &Path) -> Result<()> {
    let directory = tempfile::Builder::new()
        .prefix(".metactl-access-")
        .tempdir_in(parent)?;
    // Check inherited access too, without creating any real destination parents.
    probe_parent(directory.path())?;
    directory.close()?;
    Ok(())
}

fn probe_parent(parent: &Path) -> Result<()> {
    // NamedTempFile uses exclusive random names, owns cleanup on error, and never
    // opens a user path. Both rename endpoints live in the directory being tested.
    let mut source = tempfile::Builder::new()
        .prefix(".metactl-access-")
        .tempfile_in(parent)?;
    source.write_all(b"metactl access probe\n")?;
    source.as_file().sync_all()?;
    let source = source.into_temp_path();
    let destination = tempfile::Builder::new()
        .prefix(".metactl-access-")
        .tempfile_in(parent)?
        .into_temp_path();
    fs::rename(&source, &destination)?;
    destination.close()?;
    // The source name no longer exists after rename; its guard safely drops.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_parent_probe_leaves_no_directories_or_files() {
        let project = tempfile::tempdir().unwrap();
        fs::write(project.path().join("existing"), "preserve").unwrap();
        check_write_paths(project.path(), &[project.path().join("new/deep/file")]).unwrap();
        assert_eq!(fs::read_dir(project.path()).unwrap().count(), 1);
        assert_eq!(
            fs::read(project.path().join("existing")).unwrap(),
            b"preserve"
        );
    }

    #[test]
    fn obstructed_leaf_and_ancestor_are_rejected_without_probe_litter() {
        let project = tempfile::tempdir().unwrap();
        let root = project.path();
        fs::create_dir(root.join("directory-leaf")).unwrap();
        fs::write(root.join("file-ancestor"), "preserve").unwrap();
        for relative in ["directory-leaf", "file-ancestor/output"] {
            assert!(check_write_paths(root, &[root.join(relative)]).is_err());
        }
        assert_eq!(fs::read_dir(root).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn readable_readonly_leaf_can_be_atomically_replaced() {
        use std::os::unix::fs::PermissionsExt;
        let project = tempfile::tempdir().unwrap();
        let path = project.path().join("readonly");
        fs::write(&path, "before").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
        check_write_paths(project.path(), std::slice::from_ref(&path)).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o400
        );
        assert_eq!(fs::read(&path).unwrap(), b"before");
        crate::project::atomic_write(&path, b"after").unwrap();
        assert_eq!(fs::read(path).unwrap(), b"after");
    }
}
