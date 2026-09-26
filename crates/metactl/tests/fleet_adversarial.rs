use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(
        std::env::var_os("METACTL_FLEET_TEST_BIN")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_metactl").into()),
    )
    .env_remove("METACTL_PROFILE")
    .env_remove("METACTL_FLEET_CONTROLLER")
    .env("HOME", root.join("home"))
    .env("XDG_CONFIG_HOME", root.join("home/.config"))
    .arg("--project")
    .arg(root)
    .args(args)
    .output()
    .unwrap()
}

fn config(root: &Path, extra: &str) {
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("metactl.yaml"), format!("api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets: [codex-cli]\n{extra}")).unwrap();
}

fn apply(root: &Path) -> Output {
    cli(
        root,
        &["--json", "--yes", "--no-input", "fleet", "sync", "--apply"],
    )
}

#[test]
fn fleet_adverse_middle_conflict_preserves_user_bytes_continues_and_retries() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    for name in ["first", "conflict", "last"] {
        config(
            &root.join(name),
            if name == "conflict" {
                "defaults:\n  fleet_sync_adopt: refuse\n"
            } else {
                ""
            },
        );
    }
    let user = root.join("conflict/AGENTS.md");
    fs::write(
        &user,
        "# User-owned instructions\nKeep these exact bytes.\n",
    )
    .unwrap();
    let original = fs::read(&user).unwrap();
    config(root, "linked_projects:\n- {id: first, path: first}\n- {id: conflict, path: conflict}\n- {id: last, path: last}\n");
    let out = apply(root);
    assert_eq!(
        out.status.code(),
        Some(10),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["ok"], false);
    assert_eq!(
        result["projects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["status"].clone())
            .collect::<Vec<_>>(),
        vec![json!("applied"), json!("failed"), json!("applied")]
    );
    assert_eq!(fs::read(&user).unwrap(), original);
    for name in ["first", "last"] {
        assert!(root.join(name).join("AGENTS.md").exists());
    }
    for name in ["", "first", "conflict", "last"] {
        assert!(!root
            .join(name)
            .join(".metactl/state/operation.lock")
            .exists());
    }
    let logs = fs::read_to_string(root.join(".metactl/logs/fleet-sync.jsonl")).unwrap();
    assert_eq!(logs.lines().count(), 1);
    let log: Value = serde_json::from_str(logs.lines().next().unwrap()).unwrap();
    assert_eq!(log["projects"].as_array().unwrap().len(), 3);
    assert!(log["projects"][1].get("message").is_none());
    config(
        &root.join("conflict"),
        "defaults:\n  fleet_sync_adopt: patch\n",
    );
    let retry = apply(root);
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stdout)
    );
    assert!(fs::read_to_string(user)
        .unwrap()
        .contains("Keep these exact bytes."));
}

#[test]
fn fleet_adverse_existing_member_lock_is_preserved_and_later_member_runs() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    config(&root.join("held"), "");
    config(&root.join("later"), "");
    let state = root.join("held/.metactl/state");
    fs::create_dir_all(&state).unwrap();
    let lock = state.join("operation.lock");
    let contents = b"pid=123\ncommand=sync\nstarted_at=18446744073709551615\n";
    fs::write(&lock, contents).unwrap();
    config(
        root,
        "linked_projects:\n- {id: held, path: held}\n- {id: later, path: later}\n",
    );
    let out = apply(root);
    assert_eq!(out.status.code(), Some(10));
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["projects"][0]["status"], "failed");
    assert_eq!(result["projects"][1]["status"], "applied");
    assert!(result["projects"][0]["message"]
        .as_str()
        .unwrap()
        .contains("operation_lock_active"));
    assert_eq!(fs::read(&lock).unwrap(), contents);
    assert!(!root.join(".metactl/state/operation.lock").exists());
    assert!(!root.join("held/AGENTS.md").exists());
}
