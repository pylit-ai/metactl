use super::*;
use sha2::{Digest, Sha256};

fn seed_state(root: &Path, unchanged: &str, edited: &str, missing: &str) {
    let dir = root.join(".metactl/state");
    fs::create_dir_all(&dir).unwrap();
    let digest = format!("sha256:{}", hex::encode(Sha256::digest(b"generated\n")));
    fs::write(
        dir.join("codex-cli.json"),
        serde_json::to_vec(&json!({
            "target": {"id": "codex-cli"},
            "outputs": [
                {"destination_path": unchanged, "applied_digest": digest, "surface_id": "one"},
                {"destination_path": edited, "applied_digest": digest, "surface_id": "two"},
                {"destination_path": missing, "applied_digest": digest, "surface_id": "three"}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn git_plan_classifies_each_path_without_changing_index_or_worktree() {
    let project = TempDir::new().unwrap();
    let root = project.path();
    let dir = root.join(".agents/skills/local");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("managed.md"), "generated\n").unwrap();
    fs::write(dir.join("edited.md"), "authored edit\n").unwrap();
    fs::write(dir.join("authored.md"), "human instructions\n").unwrap();
    seed_state(
        root,
        ".agents/skills/local/managed.md",
        ".agents/skills/local/edited.md",
        ".agents/skills/local/missing.md",
    );
    assert!(Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["init", "--quiet"])
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "add",
            ".agents/skills/local/managed.md",
            ".agents/skills/local/authored.md"
        ])
        .status()
        .unwrap()
        .success());
    let before = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()
        .unwrap()
        .stdout;

    let result = run_cli(root, &["--json", "git", "plan"]);
    assert!(result.status.success(), "{}", stderr(&result));
    let plan = json_output(&result);
    assert_json_contract(&plan, "git", Some(root));
    assert_eq!(plan["read_only"], true);
    assert_eq!(
        plan["counts"],
        json!({"managed_unchanged": 1, "managed_edited": 1, "missing": 1, "unowned": 1, "unsafe_alias": 0})
    );
    let paths = plan["paths"].as_array().unwrap();
    let authored = paths
        .iter()
        .find(|item| item["path"] == ".agents/skills/local/authored.md")
        .unwrap();
    assert_eq!(authored["classification"], "unowned");
    assert_eq!(authored["git"], "tracked");
    let after = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()
        .unwrap()
        .stdout;
    assert_eq!(before, after);

    fs::write(dir.join("managed.md"), "staged authored version\n").unwrap();
    assert!(Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["add", ".agents/skills/local/managed.md"])
        .status()
        .unwrap()
        .success());
    fs::write(dir.join("managed.md"), "generated\n").unwrap();
    let staged_before = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()
        .unwrap()
        .stdout;
    let staged_plan = run_cli(root, &["--json", "git", "plan"]);
    assert!(staged_plan.status.success(), "{}", stderr(&staged_plan));
    let staged_json = json_output(&staged_plan);
    let managed = staged_json["paths"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["path"] == ".agents/skills/local/managed.md")
        .unwrap();
    assert_eq!(managed["classification"], "managed_edited");
    assert_eq!(managed["staged_change"], true);
    assert_eq!(managed["index_differs_from_worktree"], true);
    let staged_after = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()
        .unwrap()
        .stdout;
    assert_eq!(staged_before, staged_after);
}

#[test]
fn git_plan_reports_non_git_project_and_unowned_agent_file() {
    let project = TempDir::new().unwrap();
    let path = project.path().join(".codex/notes.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "authored\n").unwrap();
    for index in 0..20 {
        fs::write(
            project.path().join(format!(".codex/notes-{index}.md")),
            "authored\n",
        )
        .unwrap();
    }
    let result = run_cli(project.path(), &["--json", "git", "plan"]);
    assert!(result.status.success(), "{}", stderr(&result));
    let plan = json_output(&result);
    assert_eq!(plan["git_repository"], false);
    assert_eq!(plan["paths"][0]["classification"], "unowned");
    assert_eq!(plan["paths"][0]["git"], "not_git");
    assert_eq!(plan["paths"].as_array().unwrap().len(), 21);
    assert!(plan.get("paths_truncated").is_none());
}

#[test]
#[cfg(unix)]
fn git_plan_never_reads_outside_project_through_symlink() {
    let project = TempDir::new().unwrap();
    let external = TempDir::new().unwrap();
    let external_file = external.path().join("private.md");
    fs::write(&external_file, "external private bytes\n").unwrap();
    let managed = project.path().join(".agents/skills/external.md");
    fs::create_dir_all(managed.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&external_file, &managed).unwrap();
    seed_state(
        project.path(),
        ".agents/skills/external.md",
        "missing-one.md",
        "missing-two.md",
    );
    let plan = run_cli(project.path(), &["--json", "git", "plan"]);
    assert!(plan.status.success(), "{}", stderr(&plan));
    let json = json_output(&plan);
    let item = json["paths"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["path"] == ".agents/skills/external.md")
        .unwrap();
    assert_eq!(item["classification"], "unsafe_alias");
    assert_eq!(item["path_kind"], "symlink");
    assert!(!stdout(&plan).contains("external private bytes"));

    let alias_project = TempDir::new().unwrap();
    std::os::unix::fs::symlink(external.path(), alias_project.path().join(".agents")).unwrap();
    let alias_plan = run_cli(alias_project.path(), &["--json", "git", "plan"]);
    assert!(alias_plan.status.success(), "{}", stderr(&alias_plan));
    let alias_json = json_output(&alias_plan);
    assert_eq!(alias_json["paths"][0]["path"], ".agents");
    assert_eq!(alias_json["paths"][0]["classification"], "unsafe_alias");
    assert_eq!(alias_json["paths"].as_array().unwrap().len(), 1);
    assert!(Command::new("git")
        .arg("-C")
        .arg(alias_project.path())
        .args(["init", "--quiet"])
        .status()
        .unwrap()
        .success());
    let git_alias_plan = run_cli(alias_project.path(), &["--json", "git", "plan"]);
    assert!(
        git_alias_plan.status.success(),
        "{}",
        stderr(&git_alias_plan)
    );
    let git_alias = json_output(&git_alias_plan);
    assert_eq!(git_alias["paths"][0]["path"], ".agents");
    assert_eq!(git_alias["paths"][0]["classification"], "unsafe_alias");
}

#[test]
#[cfg(unix)]
fn status_follows_symlinked_library_directory_without_cycle() {
    let project = TempDir::new().unwrap();
    let source = TempDir::new().unwrap();
    let external = TempDir::new().unwrap();
    seed_private_source_library(source.path(), "team-pack-core-quality");
    let package = source.path().join("vendor/team-pack-core-quality");
    let external_package = external.path().join("team-pack-core-quality");
    fs::rename(&package, &external_package).unwrap();
    std::os::unix::fs::symlink(&external_package, &package).unwrap();
    init_project(project.path());
    let profile_dir = project.path().join(".test-home/.config/metactl/profiles");
    fs::create_dir_all(&profile_dir).unwrap();
    fs::write(
        profile_dir.join("team.yaml"),
        format!(
            "starter_library:\n  - {}\n  - {}\ntargets:\n  - codex-cli\n",
            starter_library_root(),
            source.path().display()
        ),
    )
    .unwrap();
    let sync = run_cli(project.path(), &["--profile", "team", "sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    let current = run_cli(project.path(), &["--profile", "team", "--json", "status"]);
    assert!(current.status.success(), "{}", stderr(&current));
    assert_eq!(
        json_output(&current)["library_source_comparison"],
        "current"
    );
    fs::write(
        external_package.join("SKILL.md"),
        "updated through symlinked directory\n",
    )
    .unwrap();
    let drifted = run_cli(project.path(), &["--profile", "team", "--json", "status"]);
    assert!(drifted.status.success(), "{}", stderr(&drifted));
    assert_eq!(
        json_output(&drifted)["library_source_comparison"],
        "drifted"
    );
}

#[test]
#[cfg(unix)]
fn library_fingerprint_rejects_directory_symlink_cycle() {
    let source = TempDir::new().unwrap();
    fs::write(source.path().join("library.json"), "{}\n").unwrap();
    fs::create_dir_all(source.path().join("packs")).unwrap();
    std::os::unix::fs::symlink(source.path(), source.path().join("packs/loop")).unwrap();
    assert!(metactl::project::library_content_digest(&[source.path().to_path_buf()]).is_err());
}

#[test]
#[cfg(unix)]
fn status_detects_local_library_change_with_same_explicit_profile() {
    let project = TempDir::new().unwrap();
    let source = TempDir::new().unwrap();
    let external = TempDir::new().unwrap();
    seed_private_source_library(source.path(), "team-pack-core-quality");
    let skill = source.path().join("vendor/team-pack-core-quality/SKILL.md");
    let external_skill = external.path().join("SKILL.md");
    fs::rename(&skill, &external_skill).unwrap();
    std::os::unix::fs::symlink(&external_skill, &skill).unwrap();
    init_project(project.path());
    let profile_dir = project.path().join(".test-home/.config/metactl/profiles");
    fs::create_dir_all(&profile_dir).unwrap();
    fs::write(
        profile_dir.join("team.yaml"),
        format!(
            "starter_library:\n  - {}\n  - {}\ntargets:\n  - codex-cli\n",
            starter_library_root(),
            source.path().display()
        ),
    )
    .unwrap();
    let sync = run_cli(project.path(), &["--profile", "team", "sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    let current = run_cli(project.path(), &["--profile", "team", "--json", "status"]);
    assert!(current.status.success(), "{}", stderr(&current));
    assert_eq!(
        json_output(&current)["library_source_comparison"],
        "current"
    );
    let lock_path = project.path().join("metactl.lock.json");
    let recorded_lock = fs::read(&lock_path).unwrap();
    let mut legacy_lock: Value = serde_json::from_slice(&recorded_lock).unwrap();
    legacy_lock
        .as_object_mut()
        .unwrap()
        .remove("library_content_digest");
    fs::write(&lock_path, serde_json::to_vec(&legacy_lock).unwrap()).unwrap();
    let legacy = run_cli(project.path(), &["--profile", "team", "--json", "status"]);
    assert!(legacy.status.success(), "{}", stderr(&legacy));
    assert_eq!(
        json_output(&legacy)["library_source_comparison"],
        "unverifiable"
    );
    assert_eq!(json_output(&legacy)["needs_sync"], true);
    fs::write(&lock_path, recorded_lock).unwrap();
    fs::write(&external_skill, "changed source\n").unwrap();
    let drifted = run_cli(project.path(), &["--profile", "team", "--json", "status"]);
    assert!(drifted.status.success(), "{}", stderr(&drifted));
    let json = json_output(&drifted);
    assert_eq!(json["library_source_comparison"], "drifted");
    assert_eq!(json["needs_sync"], true);

    let controller = TempDir::new().unwrap();
    fs::write(controller.path().join("metactl.yaml"), format!(
        "api_version: metactl/v2alpha1\nrole: builder\npolicy: brownfield-safe-builder\ntargets:\n- codex-cli\nlinked_projects:\n- id: child\n  path: {}\n  profile: team\n",
        project.path().display()
    )).unwrap();
    let xdg = project.path().join(".test-home/.config");
    let fleet = run_cli_env(
        controller.path(),
        &["--json", "fleet", "status"],
        &[("XDG_CONFIG_HOME", xdg.to_str().unwrap())],
    );
    assert!(fleet.status.success(), "{}", stderr(&fleet));
    let child = &json_output(&fleet)["projects"][0];
    assert_eq!(child["library_source_comparison"], "drifted");
    assert_eq!(child["needs_sync"], true);
}
