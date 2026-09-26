//! User-path regressions: invoke the installed-style binary against disposable
//! projects, inspecting all project bytes, path types, modes and symlink targets.
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn cli(root: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_metactl"))
        .env_remove("METACTL_PROFILE")
        .env_remove("METACTL_FLEET_CONTROLLER")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .arg("--project")
        .arg(root)
        .arg("--json")
        .args(args)
        .output()
        .unwrap()
}

fn config(root: &Path, extra: &str) {
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("metactl.yaml"), format!("api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets: [codex-cli]\n{extra}")).unwrap();
}

fn snapshot(root: &Path) -> BTreeMap<String, (String, u32, Vec<u8>)> {
    fn visit(root: &Path, path: &Path, entries: &mut BTreeMap<String, (String, u32, Vec<u8>)>) {
        let meta = fs::symlink_metadata(path).unwrap();
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            meta.permissions().mode()
        };
        #[cfg(not(unix))]
        let mode = u32::from(meta.permissions().readonly());
        let (kind, bytes) = if meta.file_type().is_symlink() {
            (
                "symlink",
                fs::read_link(path)
                    .unwrap()
                    .to_string_lossy()
                    .as_bytes()
                    .to_vec(),
            )
        } else if meta.is_dir() {
            ("dir", Vec::new())
        } else {
            ("file", fs::read(path).unwrap())
        };
        entries.insert(
            path.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .to_string(),
            (kind.into(), mode, bytes),
        );
        if meta.is_dir() && !meta.file_type().is_symlink() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), entries);
            }
        }
    }
    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}

#[test]
fn preview_checks_middle_conflicts_missing_inputs_and_preserves_every_project_path() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();
    for id in [
        "first",
        "conflict",
        "missing_pack",
        "missing_target",
        "missing_profile",
        "malformed",
        "last",
    ] {
        config(
            &root.join(id),
            if id == "conflict" {
                "defaults:\n  fleet_sync_adopt: refuse\n"
            } else {
                ""
            },
        );
    }
    fs::write(root.join("conflict/AGENTS.md"), "# User bytes\n").unwrap();
    config(
        &root.join("missing_pack"),
        "packs: [nonexistent-pack-xyz]\n",
    );
    fs::write(
        root.join("missing_target/metactl.yaml"),
        "api_version: metactl/v2alpha1\ntargets: [not-a-target]\n",
    )
    .unwrap();
    fs::write(
        root.join("malformed/metactl.yaml"),
        "api_version: [broken\n",
    )
    .unwrap();
    let members = [
        "first",
        "conflict",
        "missing_pack",
        "missing_target",
        "missing_profile",
        "malformed",
        "last",
    ]
    .iter()
    .map(|id| {
        format!(
            "- {{id: {id}, path: {id}{}}}\n",
            if *id == "missing_profile" {
                ", profile: absent-profile-xyz"
            } else {
                ""
            }
        )
    })
    .collect::<String>();
    config(root, &format!("linked_projects:\n{members}"));
    #[cfg(unix)]
    std::os::unix::fs::symlink("first", root.join("untouched-alias")).unwrap();
    let before = snapshot(root);
    let out = cli(root, home.path(), &["fleet", "sync", "--preview"]);
    assert_eq!(
        out.status.code(),
        Some(10),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    let members = value["projects"].as_array().unwrap();
    for (index, member) in members.iter().enumerate() {
        assert_eq!(
            member["status"],
            if index == 0 || index == 6 {
                "planned"
            } else {
                "failed"
            },
            "{member}"
        );
    }
    assert_eq!(snapshot(root), before, "preview changed project filesystem");
    assert!(members[0]["plan"]["targets"][0]["planned_output_files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "AGENTS.md"));
}

#[test]
fn preview_uses_effective_profile_and_reports_planned_skills_not_installed_skills() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();
    let profiles = home.path().join(".config/metactl/profiles");
    fs::create_dir_all(&profiles).unwrap();
    fs::write(
        profiles.join("fleet.yaml"),
        "api_version: metactl/v2alpha1\npacks: [unit-test-loop]\n",
    )
    .unwrap();
    config(&root.join("member"), "");
    config(root, "linked_projects:\n- {id: member, path: member}\n");
    let before = snapshot(root);
    let out = cli(
        root,
        home.path(),
        &["--profile", "fleet", "fleet", "sync", "--preview"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["projects"][0]["profile"], "fleet");
    assert!(
        value["projects"][0]["plan"]["targets"][0]["planned_skill_count"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(snapshot(root), before);
}

#[test]
fn controller_member_aliases_reuse_only_the_owned_lock_and_external_lock_is_preserved() {
    for alias in [".", "child/..", "alias"] {
        let temp = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir(root.join("child")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root, root.join("alias")).unwrap();
        #[cfg(not(unix))]
        if alias == "alias" {
            continue;
        }
        config(
            root,
            &format!("linked_projects:\n- {{id: controller, path: {alias}}}\n"),
        );
        let out = cli(
            root,
            home.path(),
            &["--yes", "--no-input", "fleet", "sync", "--apply"],
        );
        assert!(
            out.status.success(),
            "alias={alias}: {}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(root.join("AGENTS.md").exists());
        let lock = root.join(".metactl/state/operation.lock");
        assert!(!lock.exists());
        let held = b"pid=123\ncommand=sync\nstarted_at=18446744073709551615\n";
        fs::write(&lock, held).unwrap();
        let out = cli(
            root,
            home.path(),
            &["--yes", "--no-input", "fleet", "sync", "--apply"],
        );
        assert!(!out.status.success());
        assert_eq!(fs::read(lock).unwrap(), held);
    }
}

#[test]
fn log_failure_keeps_successful_member_outcomes_visible() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();
    config(&root.join("member"), "");
    config(root, "linked_projects:\n- {id: member, path: member}\n");
    fs::create_dir_all(root.join(".metactl/logs/fleet-sync.jsonl")).unwrap();
    let out = cli(
        root,
        home.path(),
        &["--yes", "--no-input", "fleet", "sync", "--apply"],
    );
    assert_eq!(out.status.code(), Some(10));
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["projects"][0]["status"], "applied");
    assert!(value["log_error"].is_string());
    assert!(root.join("member/AGENTS.md").exists());
}

#[test]
fn preview_of_managed_project_is_read_only_and_matches_real_coverage() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let root = temp.path();
    config(&root.join("member"), "");
    config(root, "linked_projects:\n- {id: member, path: member}\n");
    let out = cli(
        root,
        home.path(),
        &["--yes", "--no-input", "fleet", "sync", "--apply"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let before = snapshot(root);
    for _ in 0..2 {
        let out = cli(root, home.path(), &["fleet", "sync", "--preview"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        let value: Value = serde_json::from_slice(&out.stdout).unwrap();
        let target = &value["projects"][0]["plan"]["targets"][0];
        let files = target["planned_output_files"].as_array().unwrap();
        let mut skills = 0;
        for file in files {
            let relative = file.as_str().unwrap();
            assert!(root.join("member").join(relative).is_file());
            if relative.ends_with("/SKILL.md") {
                skills += 1;
            }
        }
        assert_eq!(target["planned_skill_count"].as_u64(), Some(skills));
        assert_eq!(snapshot(root), before);
    }
}

#[cfg(unix)]
#[test]
fn preview_refuses_destination_symlink_escape_without_touching_either_tree() {
    let temp = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let root = temp.path();
    config(&root.join("member"), "");
    config(root, "linked_projects:\n- {id: member, path: member}\n");
    fs::write(outside.path().join("keep"), "unaltered").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join("member/.agents")).unwrap();
    let before = snapshot(root);
    let external_before = snapshot(outside.path());
    let out = cli(root, home.path(), &["fleet", "sync", "--preview"]);
    assert_eq!(out.status.code(), Some(10));
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["projects"][0]["status"], "failed");
    assert_eq!(snapshot(root), before);
    assert_eq!(snapshot(outside.path()), external_before);
}
