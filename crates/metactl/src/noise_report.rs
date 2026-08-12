use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: &str = "metactl.instruction_noise.v1";

const SURFACE_PREFIXES: &[&str] = &[
    ".agents/skills/",
    ".codex/commands/",
    ".claude/skills/",
    ".claude/commands/",
    ".claude/hooks/",
    ".cursor/rules/",
    ".cursor/skills/",
    ".gemini/extensions/",
    ".opencode/",
    ".metactl/filesystem-agent/",
];

const ROOT_SURFACES: &[&str] = &["AGENTS.md", "CLAUDE.md", "GEMINI.md", "OPENCLAW.md"];

pub(crate) fn instruction_noise_report(project_root: &Path) -> Result<Value> {
    let managed = managed_destinations(project_root)?;
    let state_outputs = managed_state_outputs(project_root)?;
    let candidate_files = candidate_surface_files(project_root)?;

    let mut findings = Vec::new();
    if !managed.is_empty() {
        for rel in &candidate_files {
            if !managed.contains(rel) {
                findings.push(json!({
                    "kind": "stray_unmanaged_surface",
                    "severity": "warn",
                    "path": rel,
                    "message": "Agent surface file is not tracked in metactl managed state.",
                    "next": "Remove the stray file or adopt it through a metactl sync/apply flow."
                }));
            }
        }
    }

    for (target, rel, expected_digest) in state_outputs {
        let path = project_root.join(&rel);
        if !path.exists() {
            continue;
        }
        let actual = sha256_path(&path)?;
        if actual != expected_digest {
            findings.push(json!({
                "kind": "drifted_managed_output",
                "severity": "warn",
                "target": target,
                "path": rel,
                "expected_digest": expected_digest,
                "actual_digest": actual,
                "message": "Managed output digest differs from the last recorded apply state.",
                "next": "Run metactl validate, then review and repair with sync/apply/revert."
            }));
        }
    }

    for duplicate in duplicate_triggers(project_root, &candidate_files)? {
        findings.push(duplicate);
    }

    findings.sort_by(|a, b| {
        let ak = a["kind"].as_str().unwrap_or_default();
        let bk = b["kind"].as_str().unwrap_or_default();
        let ap = a["path"].as_str().unwrap_or_default();
        let bp = b["path"].as_str().unwrap_or_default();
        (ak, ap).cmp(&(bk, bp))
    });

    Ok(json!({
        "schema_version": SCHEMA_VERSION,
        "finding_count": findings.len(),
        "findings": findings,
    }))
}

pub(crate) fn append_instruction_noise_lines(lines: &mut Vec<String>, report: &Value) {
    let count = report["finding_count"].as_u64().unwrap_or(0);
    lines.push(format!("  Instruction noise: {count} finding(s)"));
    if count == 0 {
        return;
    }
    if let Some(findings) = report["findings"].as_array() {
        for finding in findings.iter().take(5) {
            let kind = finding["kind"].as_str().unwrap_or("finding");
            let path = finding["path"].as_str().unwrap_or("(unknown)");
            let message = finding["message"].as_str().unwrap_or("review needed");
            lines.push(format!("    [{kind}] {path}: {message}"));
        }
        if findings.len() > 5 {
            lines.push(format!("    ... {} more", findings.len() - 5));
        }
    }
}

fn managed_destinations(project_root: &Path) -> Result<BTreeSet<String>> {
    let mut managed = BTreeSet::new();
    let path = project_root.join(".metactl/state/managed_files.json");
    if !path.exists() {
        return Ok(managed);
    }
    let value: Value = serde_json::from_slice(&fs::read(&path).context("read managed files")?)
        .context("parse managed files")?;
    if let Some(targets) = value.as_object() {
        for outputs in targets.values() {
            if let Some(outputs) = outputs.as_array() {
                for output in outputs {
                    if let Some(destination) = output["destination_path"].as_str() {
                        managed.insert(normalize_rel(destination));
                    }
                }
            }
        }
    }
    Ok(managed)
}

fn managed_state_outputs(project_root: &Path) -> Result<Vec<(String, String, String)>> {
    let state_dir = project_root.join(".metactl/state");
    let mut outputs = Vec::new();
    if !state_dir.exists() {
        return Ok(outputs);
    }
    for entry in fs::read_dir(&state_dir).context("read state dir")? {
        let entry = entry?;
        let path = entry.path();
        if path.file_name().and_then(|name| name.to_str()) == Some("managed_files.json") {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let value: Value = serde_json::from_slice(&fs::read(&path).context("read state file")?)
            .context("parse state file")?;
        let target = value["target"]["id"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        if let Some(items) = value["outputs"].as_array() {
            for item in items {
                let Some(destination) = item["destination_path"].as_str() else {
                    continue;
                };
                let Some(digest) = item["applied_digest"].as_str() else {
                    continue;
                };
                outputs.push((
                    target.clone(),
                    normalize_rel(destination),
                    digest.to_string(),
                ));
            }
        }
    }
    Ok(outputs)
}

fn candidate_surface_files(project_root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    for root in ROOT_SURFACES {
        if project_root.join(root).is_file() {
            files.push((*root).to_string());
        }
    }
    for prefix in SURFACE_PREFIXES {
        let root = project_root.join(prefix);
        if root.exists() {
            walk_files(project_root, &root, &mut files)?;
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn walk_files(project_root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            walk_files(project_root, &path, out)?;
        } else if (file_type.is_file() || file_type.is_symlink())
            && path.metadata().map(|meta| meta.is_file()).unwrap_or(false)
        {
            out.push(relative(project_root, &path));
        }
    }
    Ok(())
}

fn duplicate_triggers(project_root: &Path, files: &[String]) -> Result<Vec<Value>> {
    let mut by_runtime_trigger: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for rel in files {
        let path = project_root.join(rel);
        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        if let Some(trigger) = trigger_key(rel, &contents) {
            let runtime = runtime_family(rel).to_string();
            by_runtime_trigger
                .entry((runtime, trigger))
                .or_default()
                .push(rel.clone());
        }
    }

    let mut findings = Vec::new();
    for ((runtime, trigger), paths) in by_runtime_trigger {
        if paths.len() <= 1 {
            continue;
        }
        for path in &paths {
            findings.push(json!({
                "kind": "duplicate_trigger",
                "severity": "warn",
                "path": path,
                "runtime": runtime,
                "trigger": trigger,
                "duplicates": paths,
                "message": "Multiple files for the same agent runtime advertise the same trigger metadata.",
                "next": "Remove or rename duplicate skill/rule trigger metadata."
            }));
        }
    }
    Ok(findings)
}

fn runtime_family(rel: &str) -> &str {
    if rel.starts_with(".agents/") || rel.starts_with(".codex/") {
        "codex"
    } else if rel.starts_with(".claude/") {
        "claude"
    } else if rel.starts_with(".cursor/") {
        "cursor"
    } else if rel.starts_with(".gemini/") {
        "gemini"
    } else if rel.starts_with(".opencode/") {
        "opencode"
    } else if rel.starts_with(".metactl/filesystem-agent/") {
        "filesystem-agent"
    } else {
        "shared"
    }
}

fn trigger_key(rel: &str, contents: &str) -> Option<String> {
    let frontmatter = yaml_frontmatter(contents)?;
    if rel.ends_with("SKILL.md") {
        frontmatter_field(frontmatter, "name")
            .map(|value| format!("skill-name:{value}"))
            .or_else(|| {
                frontmatter_field(frontmatter, "description")
                    .map(|value| format!("skill-description:{value}"))
            })
    } else if rel.ends_with(".mdc") {
        frontmatter_field(frontmatter, "description")
            .map(|value| format!("rule-description:{value}"))
    } else {
        None
    }
}

fn yaml_frontmatter(contents: &str) -> Option<&str> {
    let rest = contents.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

fn frontmatter_field(frontmatter: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    frontmatter.lines().find_map(|line| {
        let line = line.trim();
        let value = line.strip_prefix(&prefix)?.trim();
        let value = value.trim_matches('"').trim_matches('\'').trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

fn sha256_path(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}

fn relative(project_root: &Path, path: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn normalize_rel(path: &str) -> String {
    path.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::duplicate_triggers;
    use std::fs;

    #[test]
    fn duplicate_triggers_are_scoped_to_one_runtime() {
        let project = tempfile::tempdir().expect("tempdir");
        let skill = "---\nname: shared-skill\ndescription: Shared fixture\n---\n\n# Shared\n";
        for path in [
            ".agents/skills/shared-skill/SKILL.md",
            ".claude/skills/shared-skill/SKILL.md",
            ".cursor/skills/shared-skill/SKILL.md",
        ] {
            let destination = project.path().join(path);
            fs::create_dir_all(destination.parent().expect("parent")).expect("create parent");
            fs::write(destination, skill).expect("write skill");
        }
        let files = vec![
            ".agents/skills/shared-skill/SKILL.md".to_string(),
            ".claude/skills/shared-skill/SKILL.md".to_string(),
            ".cursor/skills/shared-skill/SKILL.md".to_string(),
        ];

        assert!(duplicate_triggers(project.path(), &files)
            .expect("cross-runtime duplicates")
            .is_empty());
    }

    #[test]
    fn duplicate_triggers_still_report_same_runtime_collisions() {
        let project = tempfile::tempdir().expect("tempdir");
        let skill = "---\nname: shared-skill\ndescription: Shared fixture\n---\n\n# Shared\n";
        let files = vec![
            ".agents/skills/shared-skill/SKILL.md".to_string(),
            ".agents/skills/stray-shared-skill/SKILL.md".to_string(),
        ];
        for path in &files {
            let destination = project.path().join(path);
            fs::create_dir_all(destination.parent().expect("parent")).expect("create parent");
            fs::write(destination, skill).expect("write skill");
        }

        let findings = duplicate_triggers(project.path(), &files).expect("same-runtime duplicates");
        assert_eq!(findings.len(), 2);
        assert!(findings.iter().all(|finding| finding["runtime"] == "codex"));
    }
}
