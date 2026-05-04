use std::fs;
use std::path::Path;
use std::process::{Child, Command, Output};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;

fn cli_bin() -> &'static str {
    env!("CARGO_BIN_EXE_metactl")
}

fn run_cli(project: &Path, args: &[&str]) -> Output {
    let test_home = project.join(".test-home");
    fs::create_dir_all(&test_home).expect("create test home");
    Command::new(cli_bin())
        .env_remove("METACTL_PROFILE")
        .env_remove("XDG_CONFIG_HOME")
        .env("HOME", &test_home)
        .arg("--project")
        .arg(project)
        .args(args)
        .output()
        .expect("run metactl")
}

fn run_cli_env(project: &Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
    let test_home = project.join(".test-home");
    fs::create_dir_all(&test_home).expect("create test home");
    let mut command = Command::new(cli_bin());
    command
        .env_remove("METACTL_PROFILE")
        .env_remove("XDG_CONFIG_HOME")
        .env("HOME", &test_home)
        .arg("--project")
        .arg(project)
        .args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().expect("run metactl with env")
}

fn spawn_cli_env(project: &Path, args: &[&str], envs: &[(&str, &str)]) -> Child {
    let test_home = project.join(".test-home");
    fs::create_dir_all(&test_home).expect("create test home");
    let mut command = Command::new(cli_bin());
    command
        .env_remove("METACTL_PROFILE")
        .env_remove("XDG_CONFIG_HOME")
        .env("HOME", &test_home)
        .arg("--project")
        .arg(project)
        .args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    command.spawn().expect("spawn metactl")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("utf8 stderr")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("utf8 stdout")
}

fn init_project(project: &Path, target: &str) {
    let output = run_cli(project, &["init", "--target", target]);
    assert!(
        output.status.success(),
        "init failed for {target}: {}",
        stderr(&output)
    );
}

fn init_project_with_targets(project: &Path, targets: &[&str]) {
    let mut args = vec!["init"];
    for target in targets {
        args.push("--target");
        args.push(target);
    }
    let output = run_cli(project, &args);
    assert!(
        output.status.success(),
        "init failed for {targets:?}: {}",
        stderr(&output)
    );
}

fn assert_regular_file(path: &Path) {
    let metadata = fs::symlink_metadata(path).unwrap_or_else(|error| {
        panic!("read metadata for {}: {error}", path.display());
    });
    assert!(
        metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
        "{} should be a regular file, not a symlink",
        path.display()
    );
}

fn assert_preserved_root_doc(project: &Path, doc: &str, sentinel: &str) {
    let path = project.join(doc);
    assert_regular_file(&path);
    let contents = fs::read_to_string(&path).expect("read root doc");
    assert!(
        contents.contains(sentinel),
        "{doc} should preserve repo-authored content: {contents}"
    );
    assert_eq!(
        contents.matches("metactl:begin").count(),
        1,
        "{doc} should have exactly one metactl managed block: {contents}"
    );
}

fn mutate_legacy_state(project: &Path, target: &str, destination_path: &str) {
    let state_path = project
        .join(".metactl/state")
        .join(format!("{target}.json"));
    let mut state: Value =
        serde_json::from_str(&fs::read_to_string(&state_path).expect("read state"))
            .expect("parse state");
    let output = state["outputs"]
        .as_array_mut()
        .expect("state outputs")
        .iter_mut()
        .find(|output| output["destination_path"] == destination_path)
        .unwrap_or_else(|| panic!("missing {destination_path} state for {target}"));

    output["patch_marker"] = Value::Null;
    output["backup_path"] = Value::Null;
    for key in [
        "instruction_mode",
        "pack_ref",
        "surface_id",
        "surface_slug",
        "source_resource_paths",
        "merge_status",
        "degradation_codes",
        "ownership_token",
    ] {
        output
            .as_object_mut()
            .expect("state output object")
            .remove(key);
    }

    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&state).expect("serialize state"),
    )
    .expect("write legacy state");
}

fn wait_for_path(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if path.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for {}", path.display());
}

#[test]
fn trust_root_doc_brownfield_matrix_refuses_then_patch_preserves_repeat_sync() {
    for (target, doc, sentinel) in [
        ("codex-cli", "AGENTS.md", "codex root trust sentinel"),
        ("cursor", "AGENTS.md", "cursor root trust sentinel"),
        ("claude-code", "CLAUDE.md", "claude root trust sentinel"),
        ("gemini-cli", "GEMINI.md", "gemini root trust sentinel"),
    ] {
        let project = TempDir::new().expect("tempdir");
        init_project(project.path(), target);
        fs::write(
            project.path().join(doc),
            format!("# Repo Instructions\n\n{sentinel}\n\nDurable brownfield guidance.\n"),
        )
        .expect("seed root doc");

        let refused = run_cli(project.path(), &["sync"]);
        assert!(
            !refused.status.success(),
            "{target} plain sync should refuse unmanaged {doc}"
        );
        let refusal_text = format!("{}{}", stdout(&refused), stderr(&refused));
        assert!(
            refusal_text.contains("Unmanaged destination exists")
                && refusal_text.contains("metactl sync --adopt patch"),
            "{target} refusal should explain patch adoption path: {refusal_text}"
        );

        let adopt = run_cli(project.path(), &["sync", "--adopt", "patch"]);
        assert!(
            adopt.status.success(),
            "{target} adopt patch failed: {}",
            stderr(&adopt)
        );
        let repeat = run_cli(project.path(), &["sync"]);
        assert!(
            repeat.status.success(),
            "{target} repeat sync failed: {}",
            stderr(&repeat)
        );

        assert_preserved_root_doc(project.path(), doc, sentinel);
    }
}

#[test]
fn trust_root_instruction_docs_remain_regular_files_for_copy_and_symlink_apply() {
    for mode in ["copy", "symlink"] {
        for (target, doc) in [
            ("codex-cli", "AGENTS.md"),
            ("cursor", "AGENTS.md"),
            ("claude-code", "CLAUDE.md"),
            ("gemini-cli", "GEMINI.md"),
        ] {
            let project = TempDir::new().expect("tempdir");
            init_project(project.path(), target);

            let compile = run_cli(project.path(), &["compile"]);
            assert!(
                compile.status.success(),
                "{target} compile failed: {}",
                stderr(&compile)
            );
            let apply = run_cli(project.path(), &["apply", "--mode", mode]);
            assert!(
                apply.status.success(),
                "{target} apply --mode {mode} failed: {}",
                stderr(&apply)
            );

            assert_regular_file(&project.path().join(doc));
        }
    }
}

#[test]
fn trust_legacy_managed_state_without_new_metadata_patches_restored_root_docs() {
    for (target, doc, sentinel) in [
        ("codex-cli", "AGENTS.md", "legacy codex restored sentinel"),
        ("cursor", "AGENTS.md", "legacy cursor restored sentinel"),
        (
            "claude-code",
            "CLAUDE.md",
            "legacy claude restored sentinel",
        ),
        ("gemini-cli", "GEMINI.md", "legacy gemini restored sentinel"),
    ] {
        let project = TempDir::new().expect("tempdir");
        init_project(project.path(), target);
        fs::write(
            project.path().join(doc),
            format!("# Restored Repo Instructions\n\n{sentinel}\n"),
        )
        .expect("seed root doc");

        let adopt = run_cli(project.path(), &["sync", "--adopt", "patch"]);
        assert!(
            adopt.status.success(),
            "{target} adopt patch failed: {}",
            stderr(&adopt)
        );
        mutate_legacy_state(project.path(), target, doc);
        fs::write(
            project.path().join(doc),
            format!("# Restored Repo Instructions\n\n{sentinel}\n"),
        )
        .expect("restore root doc");

        let repeat = run_cli(project.path(), &["sync"]);
        assert!(
            repeat.status.success(),
            "{target} legacy repeat sync failed: {}",
            stderr(&repeat)
        );

        assert_preserved_root_doc(project.path(), doc, sentinel);
    }
}

#[test]
fn trust_private_pack_stays_out_of_public_surfaces_and_in_local_surfaces() {
    let project = TempDir::new().expect("tempdir");
    init_project_with_targets(
        project.path(),
        &["codex-cli", "claude-code", "gemini-cli", "cursor"],
    );

    let add = run_cli(project.path(), &["add", "local-only-example"]);
    assert!(add.status.success(), "{}", stderr(&add));
    let compile = run_cli(project.path(), &["compile"]);
    assert!(compile.status.success(), "{}", stderr(&compile));

    let leak_markers = [
        "local-only-example",
        "Local-Only Example",
        "This example pack verifies that local-only pack routing stays out of committed agent surfaces.",
    ];
    for public_surface in [
        ".metactl/generated/codex-cli/AGENTS.md",
        ".metactl/generated/claude-code/CLAUDE.md",
        ".metactl/generated/gemini-cli/GEMINI.md",
        ".metactl/generated/cursor/.cursor/rules/metactl-pack-index.mdc",
    ] {
        let contents =
            fs::read_to_string(project.path().join(public_surface)).expect("read public surface");
        for marker in leak_markers {
            assert!(
                !contents.contains(marker),
                "{public_surface} leaked private marker {marker}: {contents}"
            );
        }
    }
    let cursor_shared_agents = project.path().join(".metactl/generated/cursor/AGENTS.md");
    if cursor_shared_agents.exists() {
        let contents = fs::read_to_string(&cursor_shared_agents).expect("read cursor AGENTS.md");
        for marker in leak_markers {
            assert!(
                !contents.contains(marker),
                "{} leaked private marker {marker}: {contents}",
                cursor_shared_agents.display()
            );
        }
    }

    for local_surface in [
        ".metactl/generated/claude-code/CLAUDE.local.md",
        ".metactl/generated/gemini-cli/GEMINI.local.md",
        ".metactl/generated/cursor/.cursor/rules/metactl-pack-index.local.mdc",
    ] {
        let contents =
            fs::read_to_string(project.path().join(local_surface)).expect("read local surface");
        assert!(
            contents.contains("local-only-example") || contents.contains("Local-Only Example"),
            "{local_surface} should include private pack routing: {contents}"
        );
    }
}

#[test]
fn trust_reference_targets_reject_takeover_with_actionable_patch_guidance() {
    for target in ["claude-code", "gemini-cli"] {
        let project = TempDir::new().expect("tempdir");
        init_project(project.path(), target);
        let compile = run_cli(project.path(), &["compile"]);
        assert!(
            compile.status.success(),
            "{target} compile failed: {}",
            stderr(&compile)
        );

        let takeover = run_cli(
            project.path(),
            &["apply", "--target", target, "--mode", "takeover"],
        );
        assert!(
            !takeover.status.success(),
            "{target} takeover should be rejected"
        );
        let text = format!("{}{}", stdout(&takeover), stderr(&takeover));
        assert!(
            text.contains("does not support takeover mode")
                && text.contains("metactl apply -t")
                && text.contains("--mode patch"),
            "{target} takeover rejection should be actionable: {text}"
        );
    }
}

#[test]
fn trust_active_operation_lock_blocks_second_mutating_command() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path(), "codex-cli");

    let mut first = spawn_cli_env(
        project.path(),
        &["sync"],
        &[("METACTL_TEST_HOLD_OPERATION_LOCK_MS", "1500")],
    );
    wait_for_path(&project.path().join(".metactl/state/operation.lock"));

    let second = run_cli(project.path(), &["sync"]);
    assert_eq!(
        second.status.code(),
        Some(10),
        "second sync should fail closed while another operation owns the lock; stdout={}, stderr={}",
        stdout(&second),
        stderr(&second)
    );
    let text = format!("{}{}", stdout(&second), stderr(&second));
    assert!(
        text.contains("another metactl write operation is already active")
            && text.contains(".metactl/state/operation.lock")
            && text.contains("Next: wait for the active command to finish"),
        "active operation error should be actionable: {text}"
    );

    let first_status = first.wait().expect("wait first sync");
    assert!(first_status.success(), "held sync should still finish");
}

#[test]
fn trust_stale_operation_lock_refuses_without_mutating_root_docs() {
    let project = TempDir::new().expect("tempdir");
    init_project(project.path(), "codex-cli");
    let sentinel = "stale operation lock must preserve repo content";
    fs::write(
        project.path().join("AGENTS.md"),
        format!("# Repo Instructions\n\n{sentinel}\n"),
    )
    .expect("seed AGENTS");
    let lock_path = project.path().join(".metactl/state/operation.lock");
    fs::create_dir_all(lock_path.parent().expect("lock parent")).expect("create lock parent");
    fs::write(
        &lock_path,
        "pid=999999\ncommand=metactl sync\nstarted_at=1\n",
    )
    .expect("seed stale operation lock");

    let refused = run_cli_env(
        project.path(),
        &["sync"],
        &[("METACTL_TEST_LOCK_STALE_SECS", "0")],
    );
    assert_eq!(
        refused.status.code(),
        Some(10),
        "stale operation lock should fail closed; stdout={}, stderr={}",
        stdout(&refused),
        stderr(&refused)
    );
    let text = format!("{}{}", stdout(&refused), stderr(&refused));
    assert!(
        text.contains("stale metactl operation lock")
            && text.contains("remove .metactl/state/operation.lock"),
        "stale lock error should include cleanup guidance: {text}"
    );
    let agents = fs::read_to_string(project.path().join("AGENTS.md")).expect("read AGENTS");
    assert!(
        agents.contains(sentinel) && !agents.contains("metactl:begin"),
        "refused stale-lock sync should not mutate root docs: {agents}"
    );
}
