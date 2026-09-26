use std::fs;
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

fn sync_error(project: &std::path::Path, mode: &str) -> (Value, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_metactl"))
        .env_remove("METACTL_PROFILE")
        .env("XDG_CONFIG_HOME", project.join("test-config"))
        .args(["--project", project.to_str().unwrap(), mode, "sync"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(10));
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value = if mode == "--no-input" {
        Value::Null
    } else {
        serde_json::from_slice(&output.stdout).unwrap()
    };
    (value, text)
}

#[test]
fn operation_lock_invalid_state_path_is_not_contention() {
    let project = TempDir::new().unwrap();
    fs::create_dir(project.path().join(".metactl")).unwrap();
    fs::write(project.path().join(".metactl/state"), "preserve").unwrap();
    for mode in ["--json", "--agent", "--no-input"] {
        let (json, text) = sync_error(project.path(), mode);
        if !json.is_null() {
            assert_eq!(json["code"], "operation_lock_io");
            assert_eq!(json["operation"], "create_state_directory");
            assert!(json["io_kind"].is_string());
        }
        assert!(!text.contains("operation_lock_active"), "{text}");
        assert!(
            !text.contains("remove .metactl/state/operation.lock"),
            "{text}"
        );
        assert!(text.contains(".metactl/state"), "{text}");
    }
    assert_eq!(
        fs::read_to_string(project.path().join(".metactl/state")).unwrap(),
        "preserve"
    );
    assert!(!project.path().join("metactl.yaml").exists());
}

#[test]
fn operation_lock_existing_lock_retains_codes_and_bytes() {
    for (started_at, expected_code) in [
        (u64::MAX, "operation_lock_active"),
        (1, "operation_lock_stale"),
    ] {
        let project = TempDir::new().unwrap();
        let state = project.path().join(".metactl/state");
        fs::create_dir_all(&state).unwrap();
        let lock = state.join("operation.lock");
        let bytes = format!("pid=123\ncommand=sync\nstarted_at={started_at}\n");
        fs::write(&lock, &bytes).unwrap();
        fs::write(project.path().join("AGENTS.md"), "untouched").unwrap();
        for mode in ["--json", "--agent"] {
            let (json, text) = sync_error(project.path(), mode);
            assert_eq!(json["code"], expected_code);
            assert!(
                text.contains("confirming no writer owns the lock"),
                "{text}"
            );
            assert_eq!(fs::read_to_string(&lock).unwrap(), bytes);
            assert_eq!(
                fs::read_to_string(project.path().join("AGENTS.md")).unwrap(),
                "untouched"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn operation_lock_permission_failure_preserves_cause_and_has_no_unlock_advice() {
    use std::os::unix::fs::PermissionsExt;
    let project = TempDir::new().unwrap();
    let state = project.path().join(".metactl/state");
    fs::create_dir_all(&state).unwrap();
    fs::set_permissions(&state, fs::Permissions::from_mode(0o555)).unwrap();
    // Privileged runners can bypass mode bits; verify the fixture really denies writes.
    let probe = state.join("probe");
    if fs::write(&probe, "probe").is_ok() {
        fs::remove_file(probe).unwrap();
        fs::set_permissions(&state, fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("permission fixture skipped: runner can write through mode 0555");
        return;
    }
    let results = ["--json", "--agent", "--no-input"].map(|mode| sync_error(project.path(), mode));
    fs::set_permissions(&state, fs::Permissions::from_mode(0o755)).unwrap();
    for (json, text) in results {
        if !json.is_null() {
            assert_eq!(json["code"], "operation_lock_permission_denied");
            assert_eq!(json["operation"], "create_lock");
            assert!(json["raw_os_error"].is_number());
        }
        assert!(text.contains("Permission denied"), "{text}");
        assert!(
            !text.contains("remove .metactl/state/operation.lock"),
            "{text}"
        );
        assert!(!text.contains("wait for the active command"), "{text}");
    }
    assert!(!state.join("operation.lock").exists());
}
