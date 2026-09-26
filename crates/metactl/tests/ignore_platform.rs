//! Focused platform runtime gate; deliberately independent of cli_workflow's
//! unrelated shell, permission, and Unix-only fixtures.
#[path = "../src/ignore_publication.rs"]
mod ignore_publication;

use ignore_publication::{publish_ignore_preserving_live, restore_preserving_live};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Repo {
    _temp: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
}

impl Repo {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project café");
        let home = temp.path().join("home");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(home.join(".config/git")).unwrap();
        fs::write(home.join(".gitconfig"), "").unwrap();
        fs::write(home.join(".config/git/ignore"), "*.private\n").unwrap();
        let fixture = Self {
            _temp: temp,
            root,
            home,
        };
        assert_ok(&fixture.git(&["init", "-q"]));
        fixture
    }

    fn command(&self, program: &str) -> Command {
        let mut cmd = Command::new(program);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                cmd.env_remove(key);
            }
        }
        cmd.env_remove("METACTL_PROFILE")
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("APPDATA", self.home.join("AppData"))
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .current_dir(&self.root);
        cmd
    }

    fn git(&self, args: &[&str]) -> Output {
        self.command("git").args(args).output().unwrap()
    }

    fn cli(&self, args: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_metactl"))
            .arg("--project")
            .arg(&self.root)
            .args(args)
            .output()
            .unwrap()
    }
}

fn assert_ok(output: &Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn payloads(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(payloads(&path));
        } else {
            out.push((path.clone(), fs::read(path).unwrap()));
        }
    }
    out
}

#[test]
fn new_ignore_recovery_snapshot_is_independent_and_creation_is_no_clobber() {
    let temp = tempfile::tempdir().unwrap();
    let recovery = temp.path().join("recovery");
    fs::create_dir(&recovery).unwrap();
    let path = temp.path().join(".gitignore");
    let backup =
        publish_ignore_preserving_live(&path, &recovery, None, b"initial\n", None).unwrap();
    fs::write(&path, b"later in-place edit\n").unwrap();
    assert_eq!(fs::read(&backup).unwrap(), b"initial\n");
    let error = publish_ignore_preserving_live(&path, &recovery, None, b"must not replace\n", None)
        .unwrap_err();
    assert!(error.to_string().contains("without replacement"));
    assert_eq!(fs::read(&path).unwrap(), b"later in-place edit\n");
}

#[test]
fn replacement_and_rollback_preserve_open_editor_handles() {
    let temp = tempfile::tempdir().unwrap();
    let recovery = temp.path().join("recovery");
    fs::create_dir(&recovery).unwrap();
    let path = temp.path().join(".gitignore");
    fs::write(&path, b"original\n").unwrap();
    let mut editor = fs::OpenOptions::new().append(true).open(&path).unwrap();
    let backup =
        publish_ignore_preserving_live(&path, &recovery, Some(b"original\n"), b"managed\n", None)
            .unwrap();
    editor.write_all(b"late original edit\n").unwrap();
    editor.sync_all().unwrap();
    assert_eq!(
        fs::read(&backup).unwrap(),
        b"original\nlate original edit\n"
    );
    // Windows needs exclusive access to the restore source, while an editor
    // may still hold the currently live destination open with delete sharing.
    drop(editor);
    let mut current_editor = fs::OpenOptions::new().append(true).open(&path).unwrap();
    let displaced = restore_preserving_live(&backup, &path).unwrap();
    current_editor.write_all(b"late displaced edit\n").unwrap();
    current_editor.sync_all().unwrap();
    assert_eq!(
        fs::read(&displaced).unwrap(),
        b"managed\nlate displaced edit\n"
    );
    assert_eq!(fs::read(&path).unwrap(), b"original\nlate original edit\n");
}

#[test]
fn unexpected_preimage_is_retained_and_reported() {
    let temp = tempfile::tempdir().unwrap();
    let recovery = temp.path().join("recovery");
    fs::create_dir(&recovery).unwrap();
    let path = temp.path().join(".gitignore");
    fs::write(&path, b"unexpected live edit\n").unwrap();
    let error =
        publish_ignore_preserving_live(&path, &recovery, Some(b"expected\n"), b"managed\n", None)
            .unwrap_err()
            .to_string();
    let retained = payloads(&recovery)
        .into_iter()
        .find(|(_, bytes)| bytes == b"unexpected live edit\n")
        .unwrap();
    assert!(error.contains(retained.0.to_str().unwrap()), "{error}");
    assert_eq!(fs::read(&path).unwrap(), b"managed\n");
}

#[test]
fn cli_replaces_existing_scopes_preserves_crlf_and_excludes_recovery() {
    for scope in ["repo", "local", "both"] {
        let repo = Repo::new();
        fs::write(repo.root.join(".gitignore"), b"# authored repo\r\n").unwrap();
        fs::write(repo.root.join(".git/info/exclude"), b"# authored local\r\n").unwrap();
        fs::write(repo.root.join("payload.private"), b"private fixture\n").unwrap();
        let args = [
            "--json",
            "ignore",
            "fix",
            "--scope",
            scope,
            "--target",
            "codex-cli",
            "--yes",
        ];
        let output = repo.cli(&args);
        assert_ok(&output);
        let data: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(data.to_string().contains("recovery_copy"));
        assert!(fs::read(repo.root.join(".gitignore"))
            .unwrap()
            .starts_with(b"# authored repo\r\n"));
        assert!(fs::read(repo.root.join(".git/info/exclude"))
            .unwrap()
            .starts_with(b"# authored local\r\n"));
        let before = (
            fs::read(repo.root.join(".gitignore")).unwrap(),
            fs::read(repo.root.join(".git/info/exclude")).unwrap(),
        );
        assert_ok(&repo.cli(&args));
        assert_eq!(
            before,
            (
                fs::read(repo.root.join(".gitignore")).unwrap(),
                fs::read(repo.root.join(".git/info/exclude")).unwrap()
            )
        );
        assert_ok(&repo.git(&["add", "-A"]));
        let tracked = repo.git(&["ls-files", "-z"]);
        assert_ok(&tracked);
        assert!(!String::from_utf8_lossy(&tracked.stdout).contains("ignore-recovery"));
        assert!(!String::from_utf8_lossy(&tracked.stdout).contains("payload.private"));
    }
}

#[test]
fn cli_default_and_selected_git_configs_preserve_global_ignore_parity() {
    for selected in [false, true] {
        let repo = Repo::new();
        let global_ignore = repo.home.join("selected-ignore");
        fs::write(&global_ignore, "*.private\n").unwrap();
        let selected_config = repo.home.join("selected-config");
        let git_path = metactl::git_privacy::git_path_argument(&global_ignore);
        fs::write(
            &selected_config,
            format!(
                "[core]\n\texcludesFile = \"{}\"\n",
                git_path.to_string_lossy()
            ),
        )
        .unwrap();
        fs::write(repo.root.join("payload.private"), "fixture").unwrap();
        let mut cmd = repo.command(env!("CARGO_BIN_EXE_metactl"));
        if selected {
            cmd.env("GIT_CONFIG_GLOBAL", &selected_config);
        }
        let output = cmd
            .args([
                "ignore",
                "install",
                "--scope",
                "local",
                "--target",
                "codex-cli",
            ])
            .output()
            .unwrap();
        assert_ok(&output);
    }
}

#[test]
fn nested_project_accepts_canonical_path_and_preserves_global_rules() {
    let repo = Repo::new();
    let nested = repo.root.join("nested project");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("payload.private"), "fixture").unwrap();
    fs::write(nested.join(".gitignore"), "# nested authored\n").unwrap();
    let output = repo
        .command(env!("CARGO_BIN_EXE_metactl"))
        .arg("--project")
        .arg(fs::canonicalize(&nested).unwrap())
        .args([
            "ignore",
            "fix",
            "--scope",
            "repo",
            "--target",
            "codex-cli",
            "--yes",
        ])
        .output()
        .unwrap();
    assert_ok(&output);
    assert!(fs::read_to_string(nested.join(".gitignore"))
        .unwrap()
        .contains("metactl:begin"));
    assert_ok(&repo.git(&["check-ignore", "--quiet", "nested project/payload.private"]));
    assert_ok(&repo.git(&["add", "-A"]));
    assert!(
        !String::from_utf8_lossy(&repo.git(&["ls-files", "-z"]).stdout).contains("ignore-recovery")
    );
}

#[cfg(windows)]
#[test]
fn windows_sharing_failure_preserves_staging_and_live_file() {
    use std::os::windows::fs::OpenOptionsExt;
    let temp = tempfile::tempdir().unwrap();
    let recovery = temp.path().join("recovery");
    fs::create_dir(&recovery).unwrap();
    let path = temp.path().join(".gitignore");
    fs::write(&path, b"old live file\n").unwrap();
    // Permit reads/writes but withhold FILE_SHARE_DELETE, as an editor can.
    let _guard = fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(&path)
        .unwrap();
    let error = publish_ignore_preserving_live(
        &path,
        &recovery,
        Some(b"old live file\n"),
        b"new bytes\n",
        None,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("retained destination"));
    assert_eq!(fs::read(path).unwrap(), b"old live file\n");
    assert!(payloads(&recovery)
        .iter()
        .any(|(_, bytes)| bytes == b"new bytes\n"));
}

#[cfg(windows)]
#[test]
fn windows_cli_second_write_failure_rolls_back_first_and_keeps_private_recovery() {
    use std::os::windows::fs::OpenOptionsExt;
    let repo = Repo::new();
    let ignore = repo.root.join(".gitignore");
    let exclude = repo.root.join(".git/info/exclude");
    fs::write(&ignore, b"# authored repo\n").unwrap();
    fs::write(&exclude, b"# authored local\n").unwrap();
    let guard = fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(&ignore)
        .unwrap();
    let output = repo.cli(&[
        "ignore",
        "fix",
        "--scope",
        "both",
        "--target",
        "codex-cli",
        "--yes",
    ]);
    assert!(!output.status.success());
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(message.contains("rollback restored"), "{message}");
    assert_eq!(fs::read(&ignore).unwrap(), b"# authored repo\n");
    assert_eq!(fs::read(&exclude).unwrap(), b"# authored local\n");
    drop(guard);
    assert_ok(&repo.git(&["add", "-A"]));
    let tracked = repo.git(&["ls-files", "-z"]);
    assert!(!String::from_utf8_lossy(&tracked.stdout).contains("ignore-recovery"));
}
