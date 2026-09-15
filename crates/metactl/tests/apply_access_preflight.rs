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

#[cfg(unix)]
#[test]
fn later_target_access_failure_precedes_every_target_apply() {
    use std::os::unix::fs::PermissionsExt;
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    assert!(cli(
        root,
        &["init", "--target", "codex-cli", "--target", "claude-code"]
    )
    .status
    .success());
    assert!(cli(root, &["compile"]).status.success());
    let preview = cli(root, &["--json", "apply", "--preview"]);
    assert!(preview.status.success(), "{preview:?}");
    let plan: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
    let targets = plan["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 2);
    let later = targets[1]["target"].as_str().unwrap();
    let denied = root.join(match later {
        "codex-cli" => ".agents/skills",
        "claude-code" => ".claude/skills",
        other => panic!("unexpected target {other}"),
    });
    fs::create_dir_all(&denied).unwrap();
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o500)).unwrap();
    if fs::write(denied.join("privilege-probe"), "probe").is_ok() {
        fs::remove_file(denied.join("privilege-probe")).unwrap();
        fs::set_permissions(&denied, fs::Permissions::from_mode(0o700)).unwrap();
        eprintln!("permission fixture skipped: runner bypasses directory permissions");
        return;
    }
    let denied_preview = cli(root, &["--json", "apply", "--preview"]);
    assert!(
        denied_preview.status.success(),
        "preview must not probe write access: {denied_preview:?}"
    );
    let failed = cli(root, &["--json", "apply"]);
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(!failed.status.success(), "{failed:?}");
    assert!(
        !root.join("AGENTS.md").exists(),
        "first target must not apply"
    );
    assert!(!root.join("CLAUDE.md").exists());
    for target in ["codex-cli", "claude-code"] {
        assert!(!root.join(format!(".metactl/state/{target}.json")).exists());
    }
    let retry = cli(root, &["--json", "apply"]);
    assert!(retry.status.success(), "{retry:?}");
    assert!(root.join("AGENTS.md").is_file());
    assert!(root.join("CLAUDE.md").is_file());
}

#[cfg(unix)]
#[test]
fn first_target_conflict_keeps_precedence_over_later_access_failure() {
    use std::os::unix::fs::PermissionsExt;
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    assert!(cli(
        root,
        &["init", "--target", "codex-cli", "--target", "claude-code"]
    )
    .status
    .success());
    assert!(cli(root, &["compile"]).status.success());
    let preview = cli(root, &["--json", "apply", "--preview"]);
    let plan: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
    let later = plan["targets"][1]["target"].as_str().unwrap();
    let denied = root.join(if later == "codex-cli" {
        ".agents/skills"
    } else {
        ".claude/skills"
    });
    fs::write(root.join("AGENTS.md"), "user instructions\n").unwrap();
    fs::create_dir_all(&denied).unwrap();
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o500)).unwrap();
    let failed = cli(root, &["--json", "apply"]);
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(!failed.status.success());
    let text = String::from_utf8_lossy(&failed.stdout);
    assert!(text.contains("Unmanaged destination exists"), "{text}");
    assert!(!text.contains("access preflight"), "{text}");
    assert_eq!(
        fs::read(root.join("AGENTS.md")).unwrap(),
        b"user instructions\n"
    );
}
