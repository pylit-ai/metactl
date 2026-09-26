use std::io;
use std::path::PathBuf;

/// Evidence from acquiring a project operation lock, independent of CLI wording.
#[derive(Debug, thiserror::Error)]
pub enum OperationLockError {
    #[error("another metactl write operation may own the existing lock at {}", path.display())]
    Active { path: PathBuf },
    #[error("stale metactl operation lock at {}; age alone does not prove its owner stopped", path.display())]
    Stale { path: PathBuf },
    #[error("{operation} at {}: {source}", path.display())]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl OperationLockError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Active { .. } => "operation_lock_active",
            Self::Stale { .. } => "operation_lock_stale",
            Self::Io { source, .. } if source.kind() == io::ErrorKind::PermissionDenied => {
                "operation_lock_permission_denied"
            }
            Self::Io { .. } => "operation_lock_io",
        }
    }

    pub fn path(&self) -> &std::path::Path {
        match self {
            Self::Active { path } | Self::Stale { path } | Self::Io { path, .. } => path,
        }
    }

    pub fn next_steps(&self) -> Vec<&'static str> {
        match self {
            Self::Active { .. } | Self::Stale { .. } => vec![
                "wait for the active command to finish before retrying",
                "only after confirming no writer owns the lock, inspect the repo and remove .metactl/state/operation.lock if abandoned",
            ],
            Self::Io { source, .. } if source.kind() == io::ErrorKind::PermissionDenied => vec![
                "check filesystem permissions and sandbox access for the reported path, then retry with authorized write access",
            ],
            Self::Io { .. } => vec![
                "inspect the reported filesystem path and OS error, correct the filesystem problem, then retry",
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_initialization_releases_only_the_new_lock() {
        use super::super::OperationLock;
        use std::io::Write;
        let project = tempfile::tempdir().unwrap();
        let path = project.path().join(".metactl/state/operation.lock");
        let error = OperationLock::acquire_with_initializer(project.path(), "test", |file, _| {
            file.write_all(b"partial payload")?;
            Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "injected write failure",
            ))
        })
        .err()
        .unwrap();
        assert_eq!(
            error.downcast_ref::<OperationLockError>().unwrap().code(),
            "operation_lock_io"
        );
        assert!(!path.exists());
        let owner = OperationLock::acquire(project.path(), "owner").unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(OperationLock::acquire(project.path(), "contender").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        drop(owner);
        assert!(!path.exists());
        assert!(OperationLock::acquire(project.path(), "retry").is_ok());
    }

    #[test]
    fn filesystem_errors_never_claim_contention_or_suggest_unlocking() {
        for kind in [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::NotFound,
            io::ErrorKind::AlreadyExists,
            io::ErrorKind::WriteZero,
            io::ErrorKind::Other,
        ] {
            let error = OperationLockError::Io {
                operation: "initialize_lock",
                path: "operation.lock".into(),
                source: io::Error::new(kind, "original cause"),
            };
            assert!(matches!(
                error.code(),
                "operation_lock_permission_denied" | "operation_lock_io"
            ));
            assert!(error.to_string().contains("original cause"));
            assert!(!error.next_steps().join(" ").contains("remove"));
        }
    }
}
