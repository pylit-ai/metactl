//! Select relevant cached index names without walking unrelated worktree files.
use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

type NativeIdentity = (u64, u128);
pub(super) type Identities = BTreeMap<PathBuf, Option<NativeIdentity>>;

fn native_identity(path: &Path) -> Result<Option<NativeIdentity>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match fs::metadata(path) {
            Ok(metadata) => Ok(Some((metadata.dev(), metadata.ino().into()))),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
        #[repr(C)]
        struct FileId {
            volume: u64,
            id: [u8; 16],
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
        let file = match fs::OpenOptions::new()
            .read(true)
            .access_mode(0)
            .share_mode(7)
            .custom_flags(0x02000000)
            .open(path)
        {
            Ok(file) => file,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                return Ok(None)
            }
            Err(error) => return Err(error.into()),
        };
        let mut info = FileId {
            volume: 0,
            id: [0; 16],
        };
        let ok = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                18,
                (&mut info as *mut FileId).cast(),
                std::mem::size_of::<FileId>() as u32,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Some((info.volume, u128::from_le_bytes(info.id))))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Ok(None)
    }
}

fn index_component(bytes: &[u8]) -> Option<std::ffi::OsString> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Some(std::ffi::OsString::from_vec(bytes.to_vec()))
    }
    #[cfg(not(unix))]
    {
        std::str::from_utf8(bytes)
            .ok()
            .map(std::ffi::OsString::from)
    }
}

fn identity(path: &Path, cache: &mut Identities) -> Result<Option<NativeIdentity>> {
    if let Some(identity) = cache.get(path) {
        return Ok(*identity);
    }
    let identity = native_identity(path)?;
    cache.insert(path.to_owned(), identity);
    Ok(identity)
}

pub(super) fn overlaps(
    root: &Path,
    indexed: &[u8],
    requested: &str,
    identities: &mut Identities,
) -> Result<bool> {
    let mut indexed_path = root.to_owned();
    let mut requested_path = root.to_owned();
    for (left, right) in indexed.split(|b| *b == b'/').zip(
        requested
            .trim_end_matches('/')
            .as_bytes()
            .split(|b| *b == b'/'),
    ) {
        if left.is_empty() || matches!(left, b"." | b"..") {
            bail!("unsafe Git index path");
        }
        let left_component = index_component(left);
        if let Some(component) = &left_component {
            indexed_path.push(component);
        }
        requested_path.push(std::str::from_utf8(right)?);
        if left == right {
            continue;
        }
        let indexed_identity = if left_component.is_some() {
            identity(&indexed_path, identities)?
        } else {
            None
        };
        let requested_identity = identity(&requested_path, identities)?;
        if let (Some(left), Some(right)) = (indexed_identity, requested_identity) {
            if left != right {
                return Ok(false);
            }
            continue;
        }
        // Native aliases can fold Unicode into ASCII (long s, Kelvin sign).
        // Missing leaves do not disprove an alias: retain uncertain names.
        if left.is_ascii() && right.is_ascii() && !left.eq_ignore_ascii_case(right) {
            #[cfg(windows)]
            {
                let trim = |bytes: &[u8]| {
                    bytes
                        .iter()
                        .rposition(|b| !matches!(b, b'.' | b' '))
                        .map_or(0, |i| i + 1)
                };
                if left[..trim(left)].eq_ignore_ascii_case(&right[..trim(right)]) {
                    continue;
                }
            }
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn revalidate(identities: &Identities) -> Result<()> {
    for (path, previous) in identities {
        if native_identity(path)? != *previous {
            bail!("Git index path identity changed during privacy preflight");
        }
    }
    Ok(())
}
