//! Spec 019 — Projection correctness regression tests.
//!
//! These tests encode the projection contracts from spec 019. They are
//! expected to FAIL on current code (the "red" state); later tasks in the
//! plan turn them green.
//!
//! Contracts exercised:
//! 1. No emitted file may leak `magicwormhole` strings.
//! 2. Pack resource paths must not contain doubled segments
//!    (e.g. `commands/<pack>/commands/...`).
//! 3. The kernel must not hardcode target-id string matches
//!    (ADR-0016 + `.claude/CLAUDE.md` Adapter-First Architecture rule).
//! 4. Claude Code must emit discoverable skills under `.claude/skills/`.
//! 5. Cursor's pack index must have valid `.mdc` frontmatter.
//! 6. Command resources must be wrapped with `description:` frontmatter.

mod support;

use std::fs;

use tempfile::TempDir;

use support::{init_and_sync, run_cli, stderr, walk_files};

const TARGETS: &[&str] = &[
    "claude-code",
    "cursor",
    "codex-cli",
    "gemini-cli",
    "openclaw",
];

// ---------------------------------------------------------------------------
// Test 1: no emitted file contains the substring "magicwormhole"
// ---------------------------------------------------------------------------
#[test]
fn no_emission_contains_magicwormhole() {
    let mut offenders: Vec<String> = Vec::new();

    for target in TARGETS {
        let tmp = TempDir::new().expect("tempdir");
        let project = tmp.path();
        init_and_sync(project, target);

        let mut files = Vec::new();
        walk_files(project, &mut files);

        for file in files {
            let bytes = match fs::read(&file) {
                Ok(b) => b,
                Err(_) => continue,
            };
            // case-insensitive substring search on bytes
            let hay = String::from_utf8_lossy(&bytes).to_lowercase();
            if hay.contains("magicwormhole") {
                let rel = file
                    .strip_prefix(project)
                    .unwrap_or(&file)
                    .to_string_lossy()
                    .into_owned();
                offenders.push(format!("[{}] {}", target, rel));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "Emitted files contain forbidden substring 'magicwormhole'. \
         Per spec 019, no projection output may reference private/legacy \
         magicwormhole identifiers. Offenders:\n  {}",
        offenders.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// Test 2: pack resource paths must not have doubled segments
// ---------------------------------------------------------------------------
#[test]
fn pack_resource_paths_have_no_doubled_segments() {
    const WATCH: &[&str] = &["commands", "rules", "scripts", "plugins", "hooks", "skills"];
    let mut offenders: Vec<String> = Vec::new();

    for target in TARGETS {
        let tmp = TempDir::new().expect("tempdir");
        let project = tmp.path();
        init_and_sync(project, target);
        let out = run_cli(
            project,
            &["add", "unit-test-loop", "--sync", "--no-input", "-y"],
        );
        // Don't hard-fail here on non-zero exit if add isn't supported for a
        // given target; the point is to exercise as many paths as possible.
        if !out.status.success() {
            eprintln!(
                "note: `add unit-test-loop --sync` failed for {}: {}",
                target,
                stderr(&out)
            );
        }

        let mut files = Vec::new();
        walk_files(project, &mut files);

        for file in files {
            let rel = file
                .strip_prefix(project)
                .unwrap_or(&file)
                .to_string_lossy()
                .into_owned();
            for seg in WATCH {
                let needle = format!("/{}/", seg);
                if let Some(first) = rel.find(&needle) {
                    let rest = &rel[first + needle.len()..];
                    if rest.contains(&needle) {
                        offenders.push(format!("[{}] {}", target, rel));
                        break;
                    }
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "Pack resource paths contain doubled segments (e.g. \
         `.claude/commands/<pack>/commands/...`). Per spec 019, the kernel \
         must not re-prefix pack-scoped directories. Offenders:\n  {}",
        offenders.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// Test 3: kernel must not hardcode target-id string matches
// ---------------------------------------------------------------------------
#[test]
fn kernel_has_no_target_id_string_match() {
    let src = include_str!("../src/library_registry.rs");

    let diag = "Per ADR-0016 and `.claude/CLAUDE.md` Adapter-First \
                Architecture rule, `crates/metactl/src/library_registry.rs` \
                must not branch on hardcoded target-id strings. All \
                tool-specific behavior must come from adapter packs.";

    assert!(
        !src.contains("match target.target_id.as_str()"),
        "library_registry.rs contains `match target.target_id.as_str()`. {}",
        diag
    );
    assert!(
        !src.contains("\"claude-code\" =>"),
        "library_registry.rs contains a `\"claude-code\" =>` string-literal \
         match arm. {}",
        diag
    );
    assert!(
        !src.contains("\"openclaw\" =>"),
        "library_registry.rs contains an `\"openclaw\" =>` string-literal \
         match arm. {}",
        diag
    );
}

#[test]
fn materializer_regular_file_policy_has_no_target_id_branches() {
    let src = include_str!("../src/materializer.rs");

    let diag = "Regular-file materialization policy must be carried by generated output metadata \
                instead of target-id string checks in the apply/revert safety path.";

    assert!(
        !src.contains("target.id == \"cursor\""),
        "materializer.rs contains a Cursor target-id branch. {}",
        diag
    );
    assert!(
        !src.contains("target.id == \"codex-cli\""),
        "materializer.rs contains a Codex target-id branch. {}",
        diag
    );
    assert!(
        src.contains("materialize_as_regular_file"),
        "materializer.rs no longer references the data-driven regular-file policy. {}",
        diag
    );
}

// ---------------------------------------------------------------------------
// Test 4: claude-code emits discoverable skills
// ---------------------------------------------------------------------------
#[test]
fn claude_code_emits_discoverable_skills() {
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path();
    init_and_sync(project, "claude-code");

    let out = run_cli(
        project,
        &["add", "migration-guard", "--sync", "--no-input", "-y"],
    );
    assert!(
        out.status.success(),
        "add migration-guard --sync failed: {}",
        stderr(&out)
    );

    let skill_path = project
        .join(".claude")
        .join("skills")
        .join("migration-guard")
        .join("migration-guard")
        .join("SKILL.md");

    assert!(
        skill_path.exists(),
        "Expected discoverable skill at {}. Per spec 019, claude-code must \
         emit skills under `.claude/skills/<pack>/<skill>/SKILL.md`.",
        skill_path.display()
    );

    let body = fs::read_to_string(&skill_path).expect("read SKILL.md");
    assert!(
        body.starts_with("---\n"),
        "SKILL.md must start with YAML frontmatter delimiter `---\\n` \
         (got first 40 bytes: {:?})",
        &body.chars().take(40).collect::<String>()
    );
    assert!(
        body.contains("description:"),
        "SKILL.md frontmatter must contain a `description:` field so Claude \
         Code can discover it."
    );
}

// ---------------------------------------------------------------------------
// Test 5: cursor pack index has .mdc frontmatter
// ---------------------------------------------------------------------------
#[test]
fn cursor_index_has_mdc_frontmatter() {
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path();
    init_and_sync(project, "cursor");

    let out = run_cli(
        project,
        &["add", "python-refactor", "--sync", "--no-input", "-y"],
    );
    assert!(
        out.status.success(),
        "add python-refactor --sync failed: {}",
        stderr(&out)
    );

    let index_path = project
        .join(".cursor")
        .join("rules")
        .join("metactl-pack-index.mdc");
    assert!(
        index_path.exists(),
        "Expected cursor pack index at {}",
        index_path.display()
    );

    let body = fs::read_to_string(&index_path).expect("read cursor index");
    assert!(
        body.starts_with("---\n"),
        "cursor pack index must start with `---\\n` (mdc frontmatter). \
         First 60 bytes: {:?}",
        &body.chars().take(60).collect::<String>()
    );
    assert!(
        body.contains("alwaysApply: true"),
        "cursor pack index frontmatter must contain `alwaysApply: true`"
    );
    assert!(
        body.contains("description:"),
        "cursor pack index frontmatter must contain `description:`"
    );
}

// ---------------------------------------------------------------------------
// Test 6: command resources have description: frontmatter
// ---------------------------------------------------------------------------
#[test]
fn command_resources_have_description_frontmatter() {
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path();
    init_and_sync(project, "claude-code");

    let out = run_cli(
        project,
        &["add", "unit-test-loop", "--sync", "--no-input", "-y"],
    );
    assert!(
        out.status.success(),
        "add unit-test-loop --sync failed: {}",
        stderr(&out)
    );

    let cmd_dir = project
        .join(".claude")
        .join("commands")
        .join("unit-test-loop");

    let mut files = Vec::new();
    walk_files(&cmd_dir, &mut files);
    let md_file = files
        .into_iter()
        .find(|p| p.extension().and_then(|e| e.to_str()) == Some("md"));

    let md_file = md_file.unwrap_or_else(|| {
        panic!(
            "No .md command resource found under {}. This may be the \
             doubled-path bug: the file may actually live at \
             `.claude/commands/unit-test-loop/commands/run-targeted-tests.md`.",
            cmd_dir.display()
        )
    });

    let body = fs::read_to_string(&md_file).expect("read command resource");
    assert!(
        body.starts_with("---\n"),
        "Command resource {} must start with `---\\n` frontmatter. \
         First 60 bytes: {:?}",
        md_file.display(),
        &body.chars().take(60).collect::<String>()
    );
    assert!(
        body.contains("description:"),
        "Command resource {} frontmatter must contain `description:`",
        md_file.display()
    );
}
