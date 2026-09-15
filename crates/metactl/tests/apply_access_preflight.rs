//! Exercise the shipped CLI, including denial recovery and repeat application.
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_metactl"))
        .env_remove("METACTL_PROFILE")
        .env("HOME", root.join("test-home"))
        .env("XDG_CONFIG_HOME", root.join("test-config"))
        .arg("--project")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}

#[cfg(unix)]
#[test]
fn denied_skill_directory_leaves_outputs_unapplied_and_retry_succeeds() {
    use std::os::unix::fs::PermissionsExt;
    for mode in ["--json", "--agent", "--no-input"] {
        let project = tempfile::tempdir().unwrap();
        let root = project.path();
        let init = cli(root, &["init", "--target", "codex-cli"]);
        assert!(init.status.success(), "{init:?}");
        let denied = root.join(".agents/skills");
        fs::create_dir_all(&denied).unwrap();
        fs::set_permissions(&denied, fs::Permissions::from_mode(0o500)).unwrap();
        if fs::write(denied.join("privilege-probe"), "probe").is_ok() {
            fs::remove_file(denied.join("privilege-probe")).unwrap();
            fs::set_permissions(&denied, fs::Permissions::from_mode(0o700)).unwrap();
            eprintln!("permission fixture skipped: runner bypasses directory permissions");
            continue;
        }
        let failed = cli(root, &[mode, "sync"]);
        fs::set_permissions(&denied, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!failed.status.success(), "{failed:?}");
        let message = format!(
            "{}{}",
            String::from_utf8_lossy(&failed.stdout),
            String::from_utf8_lossy(&failed.stderr)
        );
        assert!(message.contains("preflight"), "{message}");
        assert!(message.contains("Permission denied"), "{message}");
        assert!(!root.join("AGENTS.md").exists());
        assert!(!root.join(".metactl/state/codex-cli.json").exists());
        assert!(!root.join(".metactl/state/operation.lock").exists());
        assert_eq!(fs::read_dir(&denied).unwrap().count(), 0);
        let retry = cli(root, &[mode, "sync"]);
        assert!(retry.status.success(), "{retry:?}");
        assert!(root.join("AGENTS.md").is_file());
        let skill = root.join(".agents/skills/python-refactor/python-refactor/SKILL.md");
        assert!(skill.is_file());
        let bytes = fs::read(&skill).unwrap();
        let repeat = cli(root, &[mode, "sync"]);
        assert!(repeat.status.success(), "{repeat:?}");
        assert_eq!(fs::read(&skill).unwrap(), bytes);
    }
}

#[test]
fn obstructed_journal_preserves_user_instructions_and_recovery_is_actionable() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    assert!(cli(root, &["init", "--target", "codex-cli"])
        .status
        .success());
    fs::write(root.join("AGENTS.md"), "# User instructions\n").unwrap();
    fs::create_dir_all(root.join(".metactl/state")).unwrap();
    let obstruction = root.join(".metactl/state/apply-journal");
    fs::write(&obstruction, "preserve this obstruction").unwrap();
    let failed = cli(root, &["--json", "sync", "--adopt", "patch"]);
    assert!(!failed.status.success(), "{failed:?}");
    assert_eq!(
        fs::read(root.join("AGENTS.md")).unwrap(),
        b"# User instructions\n"
    );
    assert_eq!(
        fs::read(&obstruction).unwrap(),
        b"preserve this obstruction"
    );
    assert!(!root.join(".metactl/state/codex-cli.json").exists());
    fs::rename(&obstruction, root.join("saved-obstruction")).unwrap();
    let retry = cli(root, &["--json", "sync", "--adopt", "patch"]);
    assert!(retry.status.success(), "{retry:?}");
    assert!(fs::read_to_string(root.join("AGENTS.md"))
        .unwrap()
        .starts_with("# User instructions\n"));
}
