//! Publish ignore files while retaining displaced live files, including open
//! editor handles. Recovery is distinct from power-loss durability.
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn stage(recovery: &Path, bytes: &[u8], permissions: Option<&fs::Permissions>) -> Result<PathBuf> {
    let mut file = tempfile::NamedTempFile::new_in(recovery)?;
    file.write_all(bytes)?;
    if let Some(permissions) = permissions {
        file.as_file().set_permissions(permissions.clone())?;
    }
    file.as_file().sync_all()?;
    let (handle, path) = file.keep()?;
    // ReplaceFileW opens the replacement without sharing. Close our handle.
    drop(handle);
    Ok(path)
}

pub(super) fn publish_ignore_preserving_live(
    path: &Path,
    recovery_dir: &Path,
    original: Option<&[u8]>,
    bytes: &[u8],
    permissions: Option<&fs::Permissions>,
) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("ignore path has no parent"))?;
    fs::create_dir_all(parent)?;
    let staged = stage(recovery_dir, bytes, permissions)?;
    let backup = if original.is_none() {
        // A separate inode is essential: a permanent hard link to the live
        // destination would change the recovery snapshot on an in-place edit.
        let snapshot = stage(recovery_dir, bytes, permissions)?;
        if let Err(err) = fs::hard_link(&staged, path) {
            return Err(anyhow!(
                "create ignore file without replacement: {err}; destination {} retained; staging copy: {}; recovery copy: {}",
                path.display(), staged.display(), snapshot.display()
            ));
        }
        fs::remove_file(&staged).with_context(|| {
            format!(
                "remove staging link {}; destination {} published; recovery copy: {}",
                staged.display(),
                path.display(),
                snapshot.display()
            )
        })?;
        snapshot
    } else {
        let backup = replace_preserving_live(&staged, path)?;
        let displaced = fs::read(&backup).map_err(|err| {
            anyhow!(
                "read displaced ignore file: {err}; recovery copy: {}",
                backup.display()
            )
        })?;
        if Some(displaced.as_slice()) != original {
            return Err(anyhow!(
                "concurrent ignore edit retained at {}; destination {} also retained",
                backup.display(),
                path.display()
            ));
        }
        backup
    };
    sync_directories(&[parent, recovery_dir, backup.parent().unwrap()])
        .with_context(|| format!("recovery copy: {}", backup.display()))?;
    Ok(backup)
}

pub(super) fn restore_preserving_live(backup: &Path, path: &Path) -> Result<PathBuf> {
    let displaced = replace_preserving_live(backup, path)?;
    sync_directories(&[path.parent().unwrap(), displaced.parent().unwrap()])
        .with_context(|| format!("rollback recovery copy: {}", displaced.display()))?;
    Ok(displaced)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn replace_preserving_live(replacement: &Path, destination: &Path) -> Result<PathBuf> {
    use rustix::fs::{renameat_with, RenameFlags, CWD};
    renameat_with(CWD, replacement, CWD, destination, RenameFlags::EXCHANGE).with_context(
        || {
            format!(
                "ignore exchange failed; destination: {}; retained recovery/staging copy: {}",
                destination.display(),
                replacement.display()
            )
        },
    )?;
    Ok(replacement.to_path_buf())
}

#[cfg(windows)]
fn replace_preserving_live(replacement: &Path, destination: &Path) -> Result<PathBuf> {
    use std::os::windows::ffi::OsStrExt;
    // A new owned directory reserves a backup namespace. Do not reuse a
    // staging filename: ReplaceFileW consumes it and can partially mutate on
    // error. Keeping the directory also keeps every failure artifact.
    let namespace = tempfile::Builder::new()
        .prefix(".replace-")
        .tempdir_in(
            replacement
                .parent()
                .ok_or_else(|| anyhow!("replacement has no parent"))?,
        )?
        .keep();
    let backup = namespace.join("displaced");
    fn wide(path: &Path) -> Result<Vec<u16>> {
        let mut value: Vec<_> = path.as_os_str().encode_wide().collect();
        if value.contains(&0) {
            return Err(anyhow!("NUL in ignore replacement path"));
        }
        value.push(0);
        Ok(value)
    }
    let target = wide(destination)?;
    let source = wide(replacement)?;
    let retained = wide(&backup)?;
    #[link(name = "kernel32")]
    extern "system" {
        fn ReplaceFileW(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }
    // SAFETY: NUL-terminated UTF-16 buffers live across the synchronous call;
    // optional reserved pointers are null, and flags do not suppress ACL errors.
    let success = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            source.as_ptr(),
            retained.as_ptr(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if success == 0 {
        let err = std::io::Error::last_os_error();
        return Err(anyhow!(
            "ignore backed replacement failed: {err}; inspect retained destination: {}; staging/restore source: {}; recovery copy: {} (may exist after partial failure)",
            destination.display(), replacement.display(), backup.display()
        ));
    }
    Ok(backup)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn replace_preserving_live(replacement: &Path, destination: &Path) -> Result<PathBuf> {
    // Portable preservation fallback: capture the actual live inode, then
    // publish without clobbering a concurrent creator. There is a visible gap;
    // this is intentionally not described as an atomic exchange.
    let namespace = tempfile::Builder::new()
        .prefix(".replace-")
        .tempdir_in(
            replacement
                .parent()
                .ok_or_else(|| anyhow!("replacement has no parent"))?,
        )?
        .keep();
    let backup = namespace.join("displaced");
    fs::rename(destination, &backup).with_context(|| {
        format!(
            "capture ignore file; retained source: {}; destination: {}; recovery copy: {}",
            replacement.display(),
            destination.display(),
            backup.display()
        )
    })?;
    if let Err(err) = fs::hard_link(replacement, destination) {
        // Restore only into an absent destination. Every link is retained on
        // conflict, including the concurrently created destination.
        let restored = fs::hard_link(&backup, destination);
        return Err(anyhow!(
            "ignore publication failed: {err}; no-clobber restore: {restored:?}; retained destination: {}; source: {}; recovery copy: {}",
            destination.display(), replacement.display(), backup.display()
        ));
    }
    // The original source remains recoverable if removing its link fails.
    fs::remove_file(replacement).with_context(|| {
        format!(
            "ignore published at {}; retained source: {}; recovery copy: {}",
            destination.display(),
            replacement.display(),
            backup.display()
        )
    })?;
    Ok(backup)
}

fn sync_directories(paths: &[&Path]) -> Result<()> {
    #[cfg(unix)]
    for path in paths {
        fs::File::open(path)
            .and_then(|file| file.sync_all())
            .with_context(|| format!("sync ignore directory {}", path.display()))?;
    }
    // Windows regular File::open cannot open a directory for this purpose,
    // and ReplaceFileW's WRITE_THROUGH flag is unsupported. Files were flushed;
    // no claim of crash/power-loss durability is made for directory metadata.
    #[cfg(not(unix))]
    let _ = paths;
    Ok(())
}
