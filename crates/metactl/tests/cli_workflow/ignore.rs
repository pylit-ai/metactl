use super::*;

// Ignore and repository hygiene workflow tests.

fn agent_path_is_ignored(project: &Path, path: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(project)
        .args(["check-ignore", "--quiet", "--no-index", "--", path])
        .status()
        .expect("git check-ignore")
        .success()
}

fn run_git(project: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(project)
        .args(args)
        .output()
        .expect("git")
}

fn assert_agent_file_preserved(project: &Path, path: &str) {
    assert!(project.join(path).exists(), "missing {path}");
    assert!(
        git_ls_files(project).lines().any(|entry| entry == path),
        "not tracked: {path}"
    );
    assert!(!agent_path_is_ignored(project, path), "hidden: {path}");
}

#[test]
fn ignore_fix_preserves_custom_command_after_real_sync() {
    let project = TempDir::new().expect("tempdir");
    git_init_project(project.path());
    init_project(project.path());
    let sync = run_cli(project.path(), &["add", "unit-test-loop", "--sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    let command = ".codex/commands/custom.md";
    let file = project.path().join(command);
    fs::create_dir_all(file.parent().expect("parent")).expect("parent");
    fs::write(&file, "# Customized retained command\n").expect("command");
    git_add_forced(project.path(), &[command]);
    let again = run_cli(project.path(), &["sync", "--yes"]);
    assert!(again.status.success(), "{}", stderr(&again));
    assert_eq!(
        fs::read_to_string(&file).expect("command"),
        "# Customized retained command\n"
    );
    let plan = run_cli(
        project.path(),
        &["--json", "ignore", "fix", "--plan", "--target", "codex-cli"],
    );
    assert!(plan.status.success(), "{}", stderr(&plan));
    assert_eq!(json_output(&plan)["untrack_supported"], false);
    let fix = run_cli(
        project.path(),
        &["ignore", "fix", "--target", "codex-cli", "--yes"],
    );
    assert!(fix.status.success(), "{}", stderr(&fix));
    assert_agent_file_preserved(project.path(), command);
}

#[test]
fn ignore_fix_preserves_mixed_json_after_real_sync() {
    let project = TempDir::new().expect("tempdir");
    git_init_project(project.path());
    let init = run_cli(project.path(), &["init", "--target", "claude-code"]);
    assert!(init.status.success(), "{}", stderr(&init));
    let add = run_cli(project.path(), &["add", "migration-guard", "--sync"]);
    assert!(add.status.success(), "{}", stderr(&add));
    let path = ".claude/settings.json";
    let file = project.path().join(path);
    let mut settings: Value =
        serde_json::from_slice(&fs::read(&file).expect("settings")).expect("json");
    settings["customSetting"] = json!("keep-me");
    fs::write(
        &file,
        serde_json::to_vec_pretty(&settings).expect("serialize"),
    )
    .expect("edit");
    git_add_forced(project.path(), &[path]);
    let again = run_cli(project.path(), &["sync", "--yes"]);
    assert!(again.status.success(), "{}", stderr(&again));
    let resynced: Value =
        serde_json::from_slice(&fs::read(&file).expect("settings")).expect("json");
    assert_eq!(resynced["customSetting"], "keep-me");
    let fix = run_cli(
        project.path(),
        &["ignore", "fix", "--target", "claude-code", "--yes"],
    );
    assert!(fix.status.success(), "{}", stderr(&fix));
    assert_agent_file_preserved(project.path(), path);
}

#[cfg(unix)]
#[test]
fn ignore_fix_does_not_follow_symlinked_agent_or_state_ancestors() {
    use std::os::unix::fs::symlink;
    let project = TempDir::new().expect("tempdir");
    git_init_project(project.path());
    let outside = project.path().join("outside");
    fs::create_dir_all(&outside).expect("outside");
    fs::write(outside.join("authored.md"), "authored\n").expect("outside file");
    symlink(&outside, project.path().join(".codex")).expect("agent link");
    git_add_forced(project.path(), &[".codex"]);
    let state = project.path().join(".metactl/state");
    fs::create_dir_all(state.parent().expect("parent")).expect("parent");
    symlink(&outside, &state).expect("state link");
    let plan = run_cli(
        project.path(),
        &["ignore", "fix", "--plan", "--target", "codex-cli"],
    );
    assert!(plan.status.success(), "{}", stderr(&plan));
    let fix = run_cli(
        project.path(),
        &["ignore", "fix", "--target", "codex-cli", "--yes"],
    );
    assert!(fix.status.success(), "{}", stderr(&fix));
    assert!(project.path().join(".codex").is_symlink());
    assert_eq!(
        fs::read_to_string(outside.join("authored.md")).expect("outside"),
        "authored\n"
    );
    assert!(git_ls_files(project.path()).contains(".codex"));
    assert!(!agent_path_is_ignored(project.path(), ".codex"));
}

#[test]
fn ignore_fix_preserves_staged_divergence_and_intervening_edit() {
    let project = TempDir::new().expect("tempdir");
    git_init_project(project.path());
    let path = ".agents/skills/example/SKILL.md";
    let file = project.path().join(path);
    fs::create_dir_all(file.parent().expect("parent")).expect("parent");
    fs::write(&file, "first\n").expect("first");
    git_add_forced(project.path(), &[path]);
    let commit = run_git(
        project.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "seed",
        ],
    );
    assert!(commit.status.success(), "{}", stderr(&commit));
    fs::write(&file, "staged authored\n").expect("staged");
    git_add_forced(project.path(), &[path]);
    let staged_before = run_git(
        project.path(),
        &["show", ":.agents/skills/example/SKILL.md"],
    );
    assert!(staged_before.status.success(), "{}", stderr(&staged_before));
    let plan = run_cli(
        project.path(),
        &["ignore", "fix", "--plan", "--target", "codex-cli"],
    );
    assert!(plan.status.success(), "{}", stderr(&plan));
    fs::write(&file, "intervening edit\n").expect("edit");
    let fix = run_cli(
        project.path(),
        &["ignore", "fix", "--target", "codex-cli", "--yes"],
    );
    assert!(fix.status.success(), "{}", stderr(&fix));
    assert_eq!(
        run_git(
            project.path(),
            &["show", ":.agents/skills/example/SKILL.md"]
        )
        .stdout,
        staged_before.stdout
    );
    assert_eq!(
        fs::read_to_string(&file).expect("file"),
        "intervening edit\n"
    );
    assert_agent_file_preserved(project.path(), path);
}

#[test]
fn ignore_fix_install_alternation_removes_old_broad_blocks() {
    let project = TempDir::new().expect("tempdir");
    git_init_project(project.path());
    let authored = ".codex/notes.md";
    fs::create_dir_all(project.path().join(".codex")).expect("agent dir");
    fs::write(project.path().join(authored), "authored\n").expect("authored");
    let old = "# metactl:begin generated-agent-surfaces\n.codex/\n.agents/\n# metactl:end generated-agent-surfaces\n";
    fs::write(project.path().join(".gitignore"), old).expect("repo ignore");
    fs::write(project.path().join(".git/info/exclude"), old).expect("local ignore");
    let unsafe_single = run_cli(
        project.path(),
        &[
            "ignore",
            "fix",
            "--scope",
            "local",
            "--target",
            "codex-cli",
            "--yes",
        ],
    );
    assert!(
        !unsafe_single.status.success(),
        "other scope must be repaired too"
    );
    for command in [
        vec![
            "ignore",
            "fix",
            "--scope",
            "both",
            "--target",
            "codex-cli",
            "--yes",
        ],
        vec![
            "ignore",
            "install",
            "--scope",
            "both",
            "--target",
            "codex-cli",
            "--include-private-sources",
        ],
        vec![
            "ignore",
            "fix",
            "--scope",
            "both",
            "--target",
            "codex-cli",
            "--include-private-sources",
            "--yes",
        ],
    ] {
        let result = run_cli(project.path(), &command);
        assert!(result.status.success(), "{}", stderr(&result));
        assert!(!agent_path_is_ignored(project.path(), authored));
        for ignore in [".gitignore", ".git/info/exclude"] {
            let contents = fs::read_to_string(project.path().join(ignore)).expect("ignore");
            assert!(!contents
                .lines()
                .any(|line| matches!(line, ".codex/" | ".agents/")));
        }
    }
}

#[test]
fn anchored_managed_agent_roots_block_single_scope_and_are_repaired() {
    for root in [".agents", ".codex", ".claude", ".cursor", ".gemini"] {
        for (untouched, scope) in [(".gitignore", "local"), (".git/info/exclude", "repo")] {
            let project = TempDir::new().expect("tempdir");
            git_init_project(project.path());
            let authored = format!("{root}/authored.md");
            fs::create_dir_all(project.path().join(root)).expect("root");
            fs::write(project.path().join(&authored), "authored\n").expect("file");
            let old = format!("# metactl:begin generated-agent-surfaces\n/{root}/\n# metactl:end generated-agent-surfaces\n");
            fs::write(project.path().join(untouched), old).expect("old block");
            assert!(agent_path_is_ignored(project.path(), &authored));
            for action in ["fix", "install"] {
                let mut args = vec!["ignore", action, "--scope", scope, "--target", "codex-cli"];
                if action == "fix" {
                    args.push("--yes");
                }
                let result = run_cli(project.path(), &args);
                assert!(
                    !result.status.success(),
                    "{root} {scope} {action} unexpectedly succeeded"
                );
                assert!(agent_path_is_ignored(project.path(), &authored));
            }
            let fix = run_cli(
                project.path(),
                &[
                    "ignore",
                    "fix",
                    "--scope",
                    "both",
                    "--target",
                    "codex-cli",
                    "--yes",
                ],
            );
            assert!(fix.status.success(), "{}", stderr(&fix));
            assert!(!agent_path_is_ignored(project.path(), &authored));
            let install = run_cli(
                project.path(),
                &[
                    "ignore",
                    "install",
                    "--scope",
                    "both",
                    "--target",
                    "codex-cli",
                ],
            );
            assert!(install.status.success(), "{}", stderr(&install));
            assert!(!agent_path_is_ignored(project.path(), &authored));
        }
    }
}

#[test]
fn repo_scope_in_linked_worktree_reads_but_does_not_write_shared_exclude() {
    let parent = TempDir::new().expect("tempdir");
    let main = parent.path().join("main");
    let linked = parent.path().join("linked");
    fs::create_dir_all(&main).expect("main");
    git_init_project(&main);
    let commit = run_git(
        &main,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "seed",
        ],
    );
    assert!(commit.status.success(), "{}", stderr(&commit));
    let add = run_git(
        &main,
        &[
            "worktree",
            "add",
            "--detach",
            linked.to_str().expect("linked path"),
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));
    assert!(linked.join(".git").is_file());
    let exclude = main.join(".git/info/exclude");
    let initial = fs::read(&exclude).expect("exclude");
    let broad = b"# metactl:begin generated-agent-surfaces\n/.codex/\n# metactl:end generated-agent-surfaces\n";
    fs::write(&exclude, broad).expect("broad local rule");
    for action in ["fix", "install"] {
        let mut args = vec!["ignore", action, "--scope", "repo", "--target", "codex-cli"];
        if action == "fix" {
            args.push("--yes");
        }
        let refusal = run_cli(&linked, &args);
        assert!(
            !refusal.status.success(),
            "{action} missed shared broad rule"
        );
        assert_eq!(fs::read(&exclude).expect("exclude"), broad);
    }
    fs::write(&exclude, &initial).expect("restore fixture");
    for action in ["fix", "install"] {
        let mut args = vec!["ignore", action, "--scope", "repo", "--target", "codex-cli"];
        if action == "fix" {
            args.push("--yes");
        }
        let result = run_cli(&linked, &args);
        assert!(result.status.success(), "{action}: {}", stderr(&result));
        assert_eq!(fs::read(&exclude).expect("exclude"), initial);
    }
    assert!(linked.join(".gitignore").exists());
}

#[test]
fn repo_scope_in_submodule_does_not_write_parent_git_metadata() {
    let parent = TempDir::new().expect("tempdir");
    let source = parent.path().join("source");
    let host = parent.path().join("host");
    fs::create_dir_all(&source).expect("source");
    fs::create_dir_all(&host).expect("host");
    git_init_project(&source);
    git_init_project(&host);
    let commit = run_git(
        &source,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "seed",
        ],
    );
    assert!(commit.status.success(), "{}", stderr(&commit));
    let add = run_git(
        &host,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            source.to_str().expect("source path"),
            "embedded",
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));
    let embedded = host.join("embedded");
    assert!(embedded.join(".git").is_file());
    let metadata = host.join(".git/modules/embedded/info/exclude");
    let before = fs::read(&metadata).expect("exclude");
    for action in ["fix", "install"] {
        let mut args = vec!["ignore", action, "--scope", "repo", "--target", "codex-cli"];
        if action == "fix" {
            args.push("--yes");
        }
        let result = run_cli(&embedded, &args);
        assert!(result.status.success(), "{action}: {}", stderr(&result));
        assert_eq!(fs::read(&metadata).expect("exclude"), before);
    }
}

#[cfg(unix)]
#[test]
fn repo_scope_rejects_git_internal_symlink_ancestor() {
    use std::os::unix::fs::symlink;
    let project = TempDir::new().expect("tempdir");
    git_init_project(project.path());
    let git = project.path().join(".git");
    fs::rename(git.join("info"), git.join("real-info")).expect("move info");
    symlink(git.join("real-info"), git.join("info")).expect("symlink info");
    let before = fs::read(git.join("real-info/exclude")).expect("exclude");
    for action in ["fix", "install"] {
        let mut args = vec!["ignore", action, "--scope", "repo", "--target", "codex-cli"];
        if action == "fix" {
            args.push("--yes");
        }
        let result = run_cli(project.path(), &args);
        assert!(!result.status.success(), "{action} followed symlink");
        assert!(!project.path().join(".gitignore").exists());
        assert_eq!(
            fs::read(git.join("real-info/exclude")).expect("exclude"),
            before
        );
    }
}

#[test]
fn concurrent_authored_ignore_edit_is_preserved_or_reported_in_recovery_copy() {
    use std::time::{Duration, Instant};
    let project = TempDir::new().expect("tempdir");
    git_init_project(project.path());
    let ignore = project.path().join(".gitignore");
    let recovery_dir = project.path().join(".metactl/ignore-recovery");
    let original = "# authored fixture\n".repeat(270_000);
    fs::write(&ignore, original).expect("large ignore");
    let home = project.path().join(".test-home");
    fs::create_dir_all(&home).expect("home");
    let mut child = Command::new(cli_bin())
        .env_remove("METACTL_PROFILE")
        .env_remove("XDG_CONFIG_HOME")
        .env("HOME", &home)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .arg("--project")
        .arg(project.path())
        .args([
            "--json",
            "ignore",
            "fix",
            "--scope",
            "repo",
            "--target",
            "codex-cli",
            "--yes",
        ])
        .spawn()
        .expect("spawn repair");
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut injected = false;
    while Instant::now() < deadline {
        if recovery_dir.exists()
            && fs::read_dir(&recovery_dir)
                .expect("recovery directory")
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().starts_with(".tmp"))
        {
            use std::io::Write as _;
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(&ignore)
                .expect("append");
            file.write_all(b"authored-concurrent-rule\n")
                .expect("write concurrent rule");
            injected = true;
            break;
        }
        if child.try_wait().expect("poll child").is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let output = child.wait_with_output().expect("wait child");
    let status = output.status;
    assert!(injected, "did not observe staging file, status {status}");
    let destination = fs::read(&ignore).expect("destination");
    let backups: Vec<_> = fs::read_dir(&recovery_dir)
        .expect("recovery directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".tmp"))
        .collect();
    let in_backup = backups.iter().any(|entry| {
        fs::read(entry.path())
            .expect("backup")
            .windows(b"authored-concurrent-rule\n".len())
            .any(|window| window == b"authored-concurrent-rule\n")
    });
    let in_destination = destination
        .windows(b"authored-concurrent-rule\n".len())
        .any(|window| window == b"authored-concurrent-rule\n");
    assert!(in_destination || in_backup, "concurrent bytes disappeared");
    if !in_destination {
        let response = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            backups
                .iter()
                .any(|entry| response.contains(entry.path().to_str().expect("backup path"))),
            "displaced edit not reported: {response}"
        );
    }
}

#[cfg(unix)]
#[test]
fn unreadable_displaced_authored_edit_reports_exact_recovery_path() {
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    let mut reproduced = false;
    for _attempt in 0..3 {
        let project = TempDir::new().expect("tempdir");
        git_init_project(project.path());
        let ignore = project.path().join(".gitignore");
        let recovery_dir = project.path().join(".metactl/ignore-recovery");
        fs::write(&ignore, "# authored fixture\n".repeat(270_000)).expect("large ignore");
        let home = project.path().join(".test-home");
        fs::create_dir_all(&home).expect("home");
        let mut child = Command::new(cli_bin())
            .env_remove("METACTL_PROFILE")
            .env_remove("XDG_CONFIG_HOME")
            .env("HOME", &home)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .arg("--project")
            .arg(project.path())
            .args([
                "--json",
                "ignore",
                "fix",
                "--scope",
                "repo",
                "--target",
                "codex-cli",
                "--yes",
            ])
            .spawn()
            .expect("spawn repair");
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut injected = false;
        while Instant::now() < deadline {
            if recovery_dir.exists()
                && fs::read_dir(&recovery_dir)
                    .expect("recovery dir")
                    .filter_map(Result::ok)
                    .any(|entry| entry.file_name().to_string_lossy().starts_with(".tmp"))
            {
                let mut file = fs::OpenOptions::new()
                    .append(true)
                    .open(&ignore)
                    .expect("append");
                file.write_all(b"authored-edit-before-mode-change\n")
                    .expect("authored edit");
                fs::set_permissions(&ignore, fs::Permissions::from_mode(0o000)).expect("chmod");
                injected = true;
                break;
            }
            if child.try_wait().expect("poll child").is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let output = child.wait_with_output().expect("wait child");
        fs::set_permissions(&ignore, fs::Permissions::from_mode(0o644))
            .expect("restore fixture mode");
        assert!(injected, "did not observe staging file");
        let response = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let backups: Vec<_> = fs::read_dir(&recovery_dir)
            .expect("recovery dir")
            .filter_map(Result::ok)
            .collect();
        for backup in backups {
            fs::set_permissions(backup.path(), fs::Permissions::from_mode(0o644))
                .expect("restore copy mode");
            let bytes = fs::read(backup.path()).expect("read copy");
            if bytes
                .windows(b"authored-edit-before-mode-change\n".len())
                .any(|window| window == b"authored-edit-before-mode-change\n")
            {
                assert!(
                    !output.status.success(),
                    "unreadable displaced edit was silently accepted: {response}"
                );
                assert!(
                    response.contains(backup.path().to_str().expect("recovery path")),
                    "exact recovery path absent: {response}"
                );
                assert!(
                    !response.contains("prior writes restored"),
                    "false restoration claim: {response}"
                );
                reproduced = true;
            }
        }
        if reproduced {
            break;
        }
    }
    assert!(
        reproduced,
        "permission-change race did not exercise displaced recovery branch"
    );
}

#[test]
fn successful_repair_recovery_copies_are_not_eligible_for_git_add_all() {
    let project = TempDir::new().expect("tempdir");
    git_init_project(project.path());
    let originals = [
        (".gitignore", "authored-git-ignore\n"),
        (".cursorignore", "authored-cursor-ignore\n"),
        (".geminiignore", "authored-gemini-ignore\n"),
    ];
    for (path, bytes) in originals {
        fs::write(project.path().join(path), bytes).expect("authored ignore");
    }
    git_add_forced(
        project.path(),
        &[".gitignore", ".cursorignore", ".geminiignore"],
    );
    let commit = run_git(
        project.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "authored ignore files",
        ],
    );
    assert!(commit.status.success(), "{}", stderr(&commit));
    fs::write(project.path().join(".tmp-authored.md"), "authored\n")
        .expect("unrelated authored file");
    let repair = run_cli(
        project.path(),
        &[
            "--json",
            "ignore",
            "fix",
            "--scope",
            "repo",
            "--target",
            "codex-cli",
            "--target",
            "cursor",
            "--target",
            "gemini-cli",
            "--yes",
        ],
    );
    assert!(repair.status.success(), "{}", stderr(&repair));
    let value = json_output(&repair);
    let changes = value["changes"].as_array().expect("changes");
    for (path, original) in originals {
        let change = changes
            .iter()
            .find(|change| change["path"] == path)
            .expect("change");
        let backup = Path::new(change["recovery_copy"].as_str().expect("recovery copy"));
        assert!(backup.starts_with(project.path().join(".metactl/ignore-recovery")));
        assert_eq!(
            fs::read_to_string(backup).expect("read recovery copy"),
            original
        );
        assert!(agent_path_is_ignored(
            project.path(),
            backup.to_str().expect("backup path")
        ));
    }
    let dry_run = run_git(project.path(), &["add", "--dry-run", "--all"]);
    assert!(dry_run.status.success(), "{}", stderr(&dry_run));
    let staged = stdout(&dry_run);
    assert!(staged.contains(".gitignore"), "{staged}");
    assert!(
        staged.contains(".tmp-authored.md"),
        "unrelated authored file was hidden: {staged}"
    );
    assert!(
        !staged.contains("ignore-recovery"),
        "recovery was stageable: {staged}"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_private_recovery_root_is_refused_before_ignore_write() {
    use std::os::unix::fs::symlink;
    let project = TempDir::new().expect("project");
    let outside = TempDir::new().expect("outside");
    git_init_project(project.path());
    symlink(outside.path(), project.path().join(".metactl")).expect("state symlink");
    let result = run_cli(
        project.path(),
        &[
            "ignore",
            "fix",
            "--scope",
            "repo",
            "--target",
            "codex-cli",
            "--yes",
        ],
    );
    assert!(
        !result.status.success(),
        "symlinked private root was accepted"
    );
    assert!(stderr(&result).contains("not a real directory"));
    assert!(!project.path().join(".gitignore").exists());
    assert!(!outside.path().join("ignore-recovery").exists());
}

#[test]
fn explicit_unignore_of_recovery_directory_cannot_report_success() {
    let project = TempDir::new().expect("project");
    git_init_project(project.path());
    let ignore = project.path().join(".gitignore");
    fs::write(&ignore, "# metactl:begin generated-agent-surfaces\n.metactl/\n# metactl:end generated-agent-surfaces\n!/.metactl/\n!/.metactl/ignore-recovery/\n!/.metactl/ignore-recovery/**\n").expect("human unignore");
    let result = run_cli(
        project.path(),
        &[
            "ignore",
            "fix",
            "--scope",
            "repo",
            "--target",
            "codex-cli",
            "--yes",
        ],
    );
    assert!(!result.status.success(), "stageable recovery was accepted");
    assert!(
        stderr(&result).contains("recovery copy"),
        "{}",
        stderr(&result)
    );
}

#[test]
fn ignore_fix_rejects_invalid_bytes_and_markers_without_mutation() {
    for invalid in [
        b"\xff\n".to_vec(),
        b"# metactl:end generated-agent-surfaces\nkeep\n# metactl:begin generated-agent-surfaces\ntrailer\n".to_vec(),
        b"# metactl:begin generated-agent-surfaces\n# metactl:begin generated-agent-surfaces\n# metactl:end generated-agent-surfaces\n".to_vec(),
        b"# metactl:begin generated-agent-surfaces\ntrailer\n".to_vec(),
    ] {
        let project = TempDir::new().expect("tempdir");
        git_init_project(project.path());
        let ignore = project.path().join(".gitignore");
        fs::write(&ignore, &invalid).expect("invalid ignore");
        let exclude = project.path().join(".git/info/exclude");
        let before_exclude = fs::read(&exclude).expect("exclude");
        for suffix in [vec!["--plan"], vec!["--yes"]] {
            let mut args = vec!["ignore", "fix", "--scope", "both", "--target", "codex-cli"];
            args.extend(suffix);
            let result = run_cli(project.path(), &args);
            assert!(!result.status.success(), "invalid ignore should fail");
            assert_eq!(fs::read(&ignore).expect("ignore"), invalid);
            assert_eq!(fs::read(&exclude).expect("exclude"), before_exclude);
        }
    }
}

#[test]
fn ignore_fix_preserves_crlf_outside_managed_block() {
    let project = TempDir::new().expect("tempdir");
    let ignore = project.path().join(".gitignore");
    fs::write(&ignore, b"custom-before\r\n# metactl:begin generated-agent-surfaces\r\n.codex/\r\n# metactl:end generated-agent-surfaces\r\ncustom-after\r\n").expect("ignore");
    let args = [
        "ignore",
        "fix",
        "--scope",
        "repo",
        "--target",
        "codex-cli",
        "--yes",
    ];
    let first = run_cli(project.path(), &args);
    assert!(first.status.success(), "{}", stderr(&first));
    let updated = fs::read(&ignore).expect("ignore");
    assert!(updated.starts_with(b"custom-before\r\n"));
    assert!(updated.ends_with(b"custom-after\r\n"));
    assert!(!String::from_utf8_lossy(&updated)
        .lines()
        .any(|line| line.trim() == ".codex/"));
    let second = run_cli(project.path(), &args);
    assert!(second.status.success(), "{}", stderr(&second));
    assert_eq!(fs::read(&ignore).expect("ignore"), updated);
}

#[test]
fn ignore_fix_accepts_legal_resource_names_without_hiding_them() {
    let project = TempDir::new().expect("tempdir");
    git_init_project(project.path());
    let paths = [
        ".agents/skills/demo/assets/User Guide.md",
        ".agents/skills/demo/assets/test[1].json",
        ".agents/skills/demo/assets/#note.md",
        ".agents/skills/demo/assets/!note.md",
        ".agents/skills/demo/assets/trailing ",
    ];
    for path in paths {
        let file = project.path().join(path);
        fs::create_dir_all(file.parent().expect("parent")).expect("parent");
        fs::write(file, "authored\n").expect("resource");
        git_add_forced(project.path(), &[path]);
    }
    let plan = run_cli(
        project.path(),
        &["ignore", "fix", "--plan", "--target", "codex-cli"],
    );
    assert!(plan.status.success(), "{}", stderr(&plan));
    let fix = run_cli(
        project.path(),
        &["ignore", "fix", "--target", "codex-cli", "--yes"],
    );
    assert!(fix.status.success(), "{}", stderr(&fix));
    for path in paths {
        assert_agent_file_preserved(project.path(), path);
    }
}

#[cfg(unix)]
#[test]
fn ignore_fix_refuses_unreadable_or_symlinked_ignore_file() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    for link in [false, true] {
        let project = TempDir::new().expect("tempdir");
        git_init_project(project.path());
        let ignore = project.path().join(".gitignore");
        let original = b"authored-rule\n";
        if link {
            fs::write(project.path().join("referent"), original).expect("referent");
            symlink(project.path().join("referent"), &ignore).expect("link");
        } else {
            fs::write(&ignore, original).expect("ignore");
            fs::set_permissions(&ignore, fs::Permissions::from_mode(0o000)).expect("chmod");
        }
        for suffix in [vec!["--plan"], vec!["--yes"]] {
            let mut args = vec!["ignore", "fix", "--scope", "both", "--target", "codex-cli"];
            args.extend(suffix);
            let result = run_cli(project.path(), &args);
            assert!(!result.status.success(), "unsafe ignore file accepted");
        }
        if !link {
            fs::set_permissions(&ignore, fs::Permissions::from_mode(0o644)).expect("restore mode");
        }
        assert_eq!(fs::read(&ignore).expect("ignore"), original);
    }
}

#[cfg(unix)]
#[test]
fn ignore_fix_rolls_back_first_write_when_second_write_fails() {
    use std::os::unix::fs::PermissionsExt;
    let project = TempDir::new().expect("tempdir");
    git_init_project(project.path());
    fs::create_dir_all(project.path().join(".test-home")).expect("test home");
    fs::create_dir_all(project.path().join(".metactl/state")).expect("state dir");
    let exclude = project.path().join(".git/info/exclude");
    let original = fs::read(&exclude).expect("exclude");
    let root_mode = fs::metadata(project.path())
        .expect("metadata")
        .permissions()
        .mode();
    fs::set_permissions(project.path(), fs::Permissions::from_mode(0o555)).expect("readonly root");
    let result = run_cli(
        project.path(),
        &[
            "ignore",
            "fix",
            "--scope",
            "both",
            "--target",
            "codex-cli",
            "--yes",
        ],
    );
    fs::set_permissions(project.path(), fs::Permissions::from_mode(root_mode))
        .expect("restore root");
    assert!(!result.status.success(), "second write should fail");
    assert!(
        stderr(&result).contains("rollback outcomes:"),
        "{}",
        stderr(&result)
    );
    assert_eq!(fs::read(&exclude).expect("exclude"), original);
    assert!(!project.path().join(".gitignore").exists());
    let retry = run_cli(
        project.path(),
        &[
            "ignore",
            "fix",
            "--scope",
            "both",
            "--target",
            "codex-cli",
            "--yes",
        ],
    );
    assert!(retry.status.success(), "{}", stderr(&retry));
}

#[test]
fn ignore_status_reports_tracked_generated_roots() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_generated_root(project.path(), ".codex/skills/example/SKILL.md");

    let output = run_cli(
        project.path(),
        &["--json", "ignore", "status", "--target", "codex-cli"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "ignore", Some(project.path()));
    assert_eq!(value["tracked_generated_roots"][0]["root"], ".codex");
    assert!(value["next_commands"][0]
        .as_str()
        .expect("next command")
        .contains("metactl ignore fix --plan"));
}

#[test]
fn ignore_status_agent_json_has_next_commands() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let output = run_cli(
        project.path(),
        &["--agent", "ignore", "status", "--target", "codex-cli"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "ignore", Some(project.path()));
    assert!(!value["next_commands"]
        .as_array()
        .expect("next commands")
        .is_empty());
}

#[test]
fn ignore_target_resolution_prefers_configured_then_detected() {
    let project = TempDir::new().expect("tempdir");
    let init = run_cli(project.path(), &["init", "--target", "gemini-cli"]);
    assert!(init.status.success(), "{}", stderr(&init));
    fs::create_dir_all(project.path().join(".codex/skills/example")).expect("codex dir");

    let output = run_cli(project.path(), &["--json", "ignore", "status"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_eq!(value["target_source"], "config");
    assert_eq!(value["targets"], json!(["gemini-cli"]));
}

#[test]
fn ignore_fix_plan_reports_actions_without_writes() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_generated_root(project.path(), ".codex/skills/example/SKILL.md");
    let before_files = project_file_snapshot(project.path());
    let before_index = git_ls_files(project.path());

    let output = run_cli(
        project.path(),
        &["--json", "ignore", "fix", "--plan", "--target", "codex-cli"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "ignore", Some(project.path()));
    assert_eq!(value["plan"], json!(true));
    assert!(value["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .all(|item| item["kind"] != "untrack-generated"));
    assert_eq!(value["untrack_supported"], false);
    assert_eq!(project_file_snapshot(project.path()), before_files);
    assert_eq!(git_ls_files(project.path()), before_index);
}

#[test]
fn ignore_fix_refuses_untracking_without_changing_files() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_generated_root(project.path(), ".codex/skills/example/SKILL.md");
    let gitignore_before = fs::read(project.path().join(".gitignore")).expect("gitignore");

    let output = run_cli(
        project.path(),
        &[
            "--json",
            "ignore",
            "fix",
            "--scope",
            "both",
            "--target",
            "codex-cli",
            "--untrack-generated",
            "--yes",
        ],
    );
    assert!(!output.status.success());
    let value = json_output(&output);
    assert_eq!(value["code"], "untrack_ownership_unproven");
    assert!(project
        .path()
        .join(".codex/skills/example/SKILL.md")
        .exists());
    assert!(git_ls_files(project.path()).contains(".codex/skills/example/SKILL.md"));
    assert_eq!(
        fs::read(project.path().join(".gitignore")).expect("gitignore"),
        gitignore_before
    );
}

#[test]
fn ignore_fix_no_input_preserves_tracked_agent_files() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_generated_root(project.path(), ".codex/skills/example/SKILL.md");

    let output = run_cli(
        project.path(),
        &[
            "--no-input",
            "--json",
            "ignore",
            "fix",
            "--target",
            "codex-cli",
            "--yes",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_eq!(value["untrack_supported"], false);
    assert!(git_ls_files(project.path()).contains(".codex/skills/example/SKILL.md"));
}

#[test]
fn ignore_fix_agent_plan_json_is_parseable() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let output = run_cli(project.path(), &["--agent", "ignore", "fix", "--plan"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "ignore", Some(project.path()));
    assert_eq!(value["action"], "fix");
    assert_eq!(value["plan"], json!(true));
}

#[test]
fn doctor_reports_ignore_repair_checks() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_generated_root(project.path(), ".codex/skills/example/SKILL.md");

    let output = run_cli(project.path(), &["--json", "doctor"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let value = json_output(&output);
    assert_json_contract(&value, "doctor", Some(project.path()));
    let checks = value["checks"].as_array().expect("checks");
    let ignore = checks
        .iter()
        .find(|item| item["id"] == "ignore-repair")
        .expect("ignore check");
    assert_eq!(ignore["status"], "warn");
    assert_eq!(ignore["fix_plan_ref"], "metactl ignore fix --plan");
    assert!(!ignore["next_commands"]
        .to_string()
        .contains("--untrack-generated"));

    let repair = run_cli(
        project.path(),
        &[
            "ignore",
            "fix",
            "--scope",
            "both",
            "--target",
            "codex-cli",
            "--yes",
        ],
    );
    assert!(repair.status.success(), "{}", stderr(&repair));
    let after = run_cli(project.path(), &["--json", "doctor"]);
    assert!(after.status.success(), "{}", stderr(&after));
    let value = json_output(&after);
    let check = value["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .find(|item| item["id"] == "ignore-repair")
        .expect("ignore check");
    assert_eq!(check["status"], "pass", "{check}");
    assert_eq!(check["next_commands"], json!([]));
    assert!(git_ls_files(project.path()).contains(".codex/skills/example/SKILL.md"));
}

#[test]
fn doctor_does_not_mutate_ignore_or_git_index() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());
    seed_tracked_generated_root(project.path(), ".codex/skills/example/SKILL.md");
    let before_files = project_file_snapshot(project.path());
    let before_index = git_ls_files(project.path());

    let output = run_cli(project.path(), &["--json", "doctor"]);
    assert!(output.status.success(), "{}", stderr(&output));

    assert_eq!(project_file_snapshot(project.path()), before_files);
    assert_eq!(git_ls_files(project.path()), before_index);
}

#[test]
fn cli_ignore_install_local_writes_git_exclude_only() {
    let project = TempDir::new().expect("tempdir");
    fs::create_dir_all(project.path().join(".git/info")).expect("create git info");

    let output = run_cli(
        project.path(),
        &[
            "ignore",
            "install",
            "--scope",
            "local",
            "--target",
            "codex-cli",
            "--target",
            "cursor",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));

    let exclude =
        fs::read_to_string(project.path().join(".git/info/exclude")).expect("read exclude");
    assert!(exclude.contains("# metactl:begin generated-agent-surfaces"));
    assert!(!exclude
        .lines()
        .any(|line| matches!(line, ".agents/" | ".codex/" | ".cursor/")));
    assert!(exclude.contains("metactl.local.yaml"));
    assert!(!exclude.contains("/.agents/"));
    assert!(!exclude.contains("/.codex/"));
    assert!(!exclude.contains("/.cursor/"));
    assert!(!exclude.contains("/metactl.local.yaml"));
    assert!(!project.path().join(".cursorignore").exists());
    assert!(!project.path().join(".geminiignore").exists());

    let second = run_cli(
        project.path(),
        &[
            "ignore",
            "install",
            "--scope",
            "local",
            "--target",
            "codex-cli",
        ],
    );
    assert!(second.status.success(), "{}", stderr(&second));
    let updated =
        fs::read_to_string(project.path().join(".git/info/exclude")).expect("read exclude");
    assert_eq!(
        updated
            .matches("# metactl:begin generated-agent-surfaces")
            .count(),
        1,
        "managed ignore block should be replaced idempotently"
    );
    assert!(!updated.lines().any(|line| line == ".codex/"));
    assert!(!updated.contains(".cursor/"));

    let status = run_cli(project.path(), &["ignore", "status", "--target", "cursor"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_stdout = stdout(&status);
    assert!(
        !status_stdout.contains("repo-scoped Git ignores can hide Cursor skills"),
        "local exclude posture should not warn about repo-scoped agent allowlists:\n{}",
        status_stdout
    );
}

#[test]
fn cli_ignore_install_can_include_private_source_paths() {
    let project = TempDir::new().expect("tempdir");
    fs::create_dir_all(project.path().join(".git/info")).expect("create git info");

    let output = run_cli(
        project.path(),
        &[
            "ignore",
            "install",
            "--scope",
            "local",
            "--include-private-sources",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));

    let exclude =
        fs::read_to_string(project.path().join(".git/info/exclude")).expect("read exclude");
    assert!(exclude.contains(".metactl/cache/sources/"));
    assert!(exclude.contains(".metactl/private/source-lock.json"));
}

#[test]
fn cli_ignore_status_reports_private_source_protection() {
    let project = TempDir::new().expect("tempdir");
    fs::create_dir_all(project.path().join(".git/info")).expect("create git info");

    let before = run_cli(project.path(), &["--json", "ignore", "status"]);
    assert!(before.status.success(), "{}", stderr(&before));
    let before_json = json_output(&before);
    assert_eq!(before_json["private_sources"]["protected"], false);
    let before_human = run_cli(project.path(), &["ignore", "status"]);
    assert!(before_human.status.success(), "{}", stderr(&before_human));
    assert!(stdout(&before_human)
        .contains("next: metactl ignore install --scope local --include-private-sources"));

    let install = run_cli(
        project.path(),
        &[
            "ignore",
            "install",
            "--scope",
            "local",
            "--include-private-sources",
        ],
    );
    assert!(install.status.success(), "{}", stderr(&install));

    let after = run_cli(project.path(), &["--json", "ignore", "status"]);
    assert!(after.status.success(), "{}", stderr(&after));
    let after_json = json_output(&after);
    assert_eq!(after_json["private_sources"]["protected"], true);
    assert_eq!(after_json["private_sources"]["cache_protected"], true);
    assert_eq!(
        after_json["private_sources"]["private_lock_protected"],
        true
    );
}

#[test]
fn cli_ignore_install_repo_writes_gitignore_and_agent_allowlists() {
    let project = TempDir::new().expect("tempdir");

    let output = run_cli(
        project.path(),
        &["ignore", "install", "--scope", "repo", "--target", "all"],
    );
    assert!(output.status.success(), "{}", stderr(&output));

    let gitignore = fs::read_to_string(project.path().join(".gitignore")).expect("read gitignore");
    assert!(gitignore.contains(".metactl/"));
    assert!(!gitignore.lines().any(|line| matches!(
        line,
        ".agents/" | ".codex/" | ".claude/" | ".cursor/" | ".gemini/"
    )));
    assert!(gitignore.contains("CLAUDE.local.md"));
    assert!(gitignore.contains("GEMINI.local.md"));
    assert!(!gitignore.contains("/.agents/"));
    assert!(!gitignore.contains("/.metactl/"));
    assert!(!gitignore.contains("/.codex/"));
    assert!(!gitignore.contains("/.cursor/"));
    assert!(!gitignore.contains("/.claude/"));
    assert!(!gitignore.contains("/.gemini/"));
    assert!(!gitignore.contains("/CLAUDE.local.md"));
    assert!(!gitignore.contains("/GEMINI.local.md"));
    assert!(!gitignore.contains("/metactl.lock.json"));

    let cursorignore =
        fs::read_to_string(project.path().join(".cursorignore")).expect("read cursorignore");
    assert!(cursorignore.contains("# metactl:begin agent-surface-allowlist"));
    assert!(cursorignore.contains("!/.cursor/rules/**"));
    assert!(cursorignore.contains("!/.cursor/skills/**"));
    assert!(cursorignore.contains("!/.agents/skills/**"));

    let geminiignore =
        fs::read_to_string(project.path().join(".geminiignore")).expect("read geminiignore");
    assert!(geminiignore.contains("# metactl:begin agent-surface-allowlist"));
    assert!(geminiignore.contains("!/.gemini/extensions/**"));

    let second = run_cli(
        project.path(),
        &["ignore", "install", "--scope", "repo", "--target", "all"],
    );
    assert!(second.status.success(), "{}", stderr(&second));
    let updated = fs::read_to_string(project.path().join(".cursorignore"))
        .expect("read updated cursorignore");
    assert_eq!(
        updated
            .matches("# metactl:begin agent-surface-allowlist")
            .count(),
        1,
        "managed agent allowlist block should be replaced idempotently"
    );
}

#[test]
fn cli_ignore_status_warns_when_repo_gitignore_can_hide_cursor_surfaces() {
    let project = TempDir::new().expect("tempdir");
    fs::write(project.path().join(".gitignore"), "/.cursor/\n").expect("write gitignore");

    let output = run_cli(project.path(), &["ignore", "status", "--target", "cursor"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let output_stdout = stdout(&output);
    assert!(
        output_stdout.contains("repo-scoped Git ignores can hide Cursor skills"),
        "status should warn when repo gitignore can hide Cursor surfaces:\n{}",
        output_stdout
    );
}

#[test]
fn cli_check_strict_reports_warn_ignore_and_superseded_knowledge_sources() {
    let project = TempDir::new().expect("tempdir");
    let custom = TempDir::new().expect("custom library");
    let ks_dir = custom.path().join("knowledge_sources");
    fs::create_dir_all(&ks_dir).expect("knowledge dir");

    let write_source = |file_name: &str, manifest: Value| {
        fs::write(
            ks_dir.join(file_name),
            serde_json::to_string_pretty(&manifest).expect("knowledge json"),
        )
        .expect("write knowledge source");
    };
    let source = |id: &str,
                  freshness_policy: &str,
                  review_status: &str,
                  superseded_by: Vec<&str>| {
        json!({
            "kind": "knowledge_source",
            "id": id,
            "version": "1.0.0",
            "title": id,
            "source_kind": "filesystem_markdown",
            "uri_scheme": "file",
            "allowed_targets": ["codex-cli"],
            "byte_budget": {"max_search_bytes": 4096, "max_read_bytes": 4096, "max_search_results": 5},
            "trust_tier": "org_validated",
            "freshness": {
                "owner": "fixtures",
                "last_verified": "2000-01-01T00:00:00Z",
                "expires_after_days": 1,
                "source_digests": ["sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],
                "freshness_policy": freshness_policy,
                "review_status": review_status,
                "superseded_by": superseded_by
            },
            "operations": {
                "search": {"enabled": true, "max_bytes": 4096, "max_results": 5},
                "read": {"enabled": true, "max_bytes": 4096, "max_results": 1},
                "freshness": {"enabled": true, "max_bytes": 1024, "max_results": 1},
                "propose_update": {"enabled": false, "mode": "request_only"}
            },
            "adapter": {"base_path": "docs", "allowed_uri_prefixes": ["file:docs/"]}
        })
    };
    write_source(
        "expired-warn.json",
        source("expired-warn", "warn", "active", Vec::new()),
    );
    write_source(
        "expired-ignore.json",
        source("expired-ignore", "ignore", "active", Vec::new()),
    );
    let mut superseded_source = source(
        "superseded-source",
        "warn",
        "superseded",
        vec!["knowledge_source:current-source"],
    );
    superseded_source["freshness"]["last_verified"] = json!("2999-01-01T00:00:00Z");
    write_source("superseded-source.json", superseded_source);

    fs::write(
        project.path().join("metactl.yaml"),
        format!(
            "api_version: metactl/v2alpha1
role: builder
policy: brownfield-safe-builder
targets:
- codex-cli
starter_library:
- {}
- {}
",
            starter_library_root(),
            custom.path().display()
        ),
    )
    .expect("write config");

    let sync = run_cli(
        project.path(),
        &["--json", "sync", "--target", "codex", "--apply"],
    );
    assert!(sync.status.success(), "{}", stderr(&sync));

    let check = run_cli(project.path(), &["--json", "check", "--strict"]);
    assert!(check.status.success(), "{}", stderr(&check));
    let check_json = json_output(&check);
    let freshness = check_json["freshness"]
        .as_array()
        .expect("freshness findings");
    let by_id = |id: &str| {
        freshness
            .iter()
            .find(|item| item["id"] == id)
            .unwrap_or_else(|| panic!("missing freshness finding for {id}: {freshness:?}"))
    };

    let warn = by_id("expired-warn");
    assert_eq!(warn["status"], json!("warn"));
    assert_eq!(warn["code"], json!("METACTL_KS_EXPIRED_WARN"));
    assert_eq!(
        warn["source_digests"][0],
        json!("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
    assert_eq!(warn["trust_tier"], json!("org_validated"));

    let ignored = by_id("expired-ignore");
    assert_eq!(ignored["status"], json!("ignored"));
    assert_eq!(ignored["code"], json!("METACTL_KS_EXPIRED_IGNORE"));
    assert_eq!(ignored["freshness_policy"], json!("ignore"));

    let superseded = by_id("superseded-source");
    assert_eq!(superseded["status"], json!("warn"));
    assert_eq!(superseded["code"], json!("METACTL_KS_SUPERSEDED"));
    assert_eq!(
        superseded["superseded_by"][0],
        json!("knowledge_source:current-source")
    );
}

#[test]
fn local_config_layer_gitignored() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let gitignore = fs::read_to_string(project.path().join(".gitignore")).expect("read .gitignore");
    assert!(
        gitignore.contains("metactl.local.yaml"),
        "gitignore should contain metactl.local.yaml entry"
    );
}

#[test]
fn cli_doctor_ignores_brownfield_files_once_managed() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path());

    let sync = run_cli(project.path(), &["sync", "--adopt", "patch", "--yes"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let doctor = run_cli(project.path(), &["--json", "doctor"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let json = json_output(&doctor);
    let checks = json["checks"].as_array().expect("checks array");

    assert!(
        checks.iter().all(|c| c["id"] != "brownfield-detection"),
        "doctor should not report brownfield-detection after managed sync: {:?}",
        checks
    );
}
