//! Read-only inventory of projected paths against the Git index.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const AGENT_ROOTS: [&str; 6] = [
    ".agents",
    ".codex",
    ".claude",
    ".cursor",
    ".gemini",
    ".opencode",
];

#[derive(Clone)]
struct Owner {
    target: String,
    digest: Option<String>,
    pack: Value,
    surface: Value,
}

pub(super) fn build(root: &Path) -> Result<Value> {
    let owners = read_owners(root)?;
    let git = git_root_matches(root)?;
    let tracked = if git {
        git_paths(root, &["ls-files", "-z", "--cached", "--"], &[])?
    } else {
        BTreeSet::new()
    };
    let mut paths = owners.keys().cloned().collect::<BTreeSet<_>>();
    if git {
        paths.extend(tracked.iter().filter(|p| in_agent_root(p)).cloned());
        paths.extend(
            git_paths(root, &["ls-files", "-z", "--others", "--"], &AGENT_ROOTS)?
                .into_iter()
                .filter(|p| in_agent_root(p)),
        );
    } else {
        for name in AGENT_ROOTS {
            collect_paths(root, &root.join(name), &mut paths)?;
        }
    }

    let mut entries = Vec::new();
    let mut counts = BTreeMap::<&str, usize>::new();
    for path in paths {
        let absolute = root.join(&path);
        let exists = absolute.symlink_metadata().is_ok();
        let owner = owners.get(&path);
        let classification = match owner {
            None => "unowned",
            Some(_) if !exists => "missing",
            Some(owner) => match owner.digest.as_deref() {
                Some(expected) if file_digest(&absolute).as_deref() == Some(expected) => {
                    "managed_unchanged"
                }
                _ => "managed_edited",
            },
        };
        *counts.entry(classification).or_default() += 1;
        entries.push(json!({
            "path": path,
            "classification": classification,
            "present": exists,
            "git": if !git { "not_git" } else if tracked.contains(&path) { "tracked" } else { "untracked" },
            "target": owner.map(|item| item.target.as_str()),
            "pack_ref": owner.map(|item| item.pack.clone()),
            "surface_id": owner.map(|item| item.surface.clone()),
        }));
    }
    Ok(json!({
        "read_only": true,
        "git_repository": git,
        "counts": {
            "managed_unchanged": counts.get("managed_unchanged").copied().unwrap_or(0),
            "managed_edited": counts.get("managed_edited").copied().unwrap_or(0),
            "missing": counts.get("missing").copied().unwrap_or(0),
            "unowned": counts.get("unowned").copied().unwrap_or(0),
        },
        "paths": entries,
    }))
}

fn read_owners(root: &Path) -> Result<BTreeMap<String, Owner>> {
    let dir = root.join(".metactl/state");
    let mut owners = BTreeMap::new();
    if !dir.is_dir() {
        return Ok(owners);
    }
    let mut files = fs::read_dir(&dir)?.collect::<std::result::Result<Vec<_>, _>>()?;
    files.sort_by_key(|entry| entry.file_name());
    for file in files {
        let path = file.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json")
            || path.file_name().and_then(|n| n.to_str()) == Some("managed_files.json")
        {
            continue;
        }
        let data: Value = serde_json::from_slice(&fs::read(&path)?)
            .with_context(|| format!("parse state {}", path.display()))?;
        let Some(target) = data.pointer("/target/id").and_then(Value::as_str) else {
            continue;
        };
        let Some(outputs) = data.get("outputs").and_then(Value::as_array) else {
            continue;
        };
        for output in outputs {
            let Some(raw) = output.get("destination_path").and_then(Value::as_str) else {
                continue;
            };
            let Some(relative) = safe_relative(root, raw) else {
                continue;
            };
            let owner = Owner {
                target: target.to_string(),
                digest: output
                    .get("applied_digest")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                pack: output.get("pack_ref").cloned().unwrap_or(Value::Null),
                surface: output.get("surface_id").cloned().unwrap_or(Value::Null),
            };
            if owners.insert(relative.clone(), owner).is_some() {
                return Err(anyhow!("multiple MetaCTL state files own {relative}"));
            }
        }
    }
    Ok(owners)
}

fn safe_relative(root: &Path, raw: &str) -> Option<String> {
    let path = Path::new(raw);
    let relative = if path.is_absolute() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return None;
    }
    Some(relative.to_string_lossy().replace('\\', "/"))
}

fn file_digest(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}

fn git_root_matches(root: &Path) -> Result<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    if !output.status.success() {
        return Ok(false);
    }
    let top = String::from_utf8(output.stdout)?.trim().to_string();
    Ok(Path::new(&top).canonicalize()? == root.canonicalize()?)
}

fn git_paths(root: &Path, prefix: &[&str], paths: &[&str]) -> Result<BTreeSet<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(prefix)
        .args(paths)
        .output()
        .context("run git ls-files")?;
    if !output.status.success() {
        return Err(anyhow!("git ls-files failed"));
    }
    Ok(output
        .stdout
        .split(|b| *b == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).to_string())
        .collect())
}

fn in_agent_root(path: &str) -> bool {
    AGENT_ROOTS
        .iter()
        .any(|root| path.starts_with(&format!("{root}/")))
}

fn collect_paths(root: &Path, dir: &Path, result: &mut BTreeSet<String>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect_paths(root, &entry.path(), result)?;
        } else if let Ok(relative) = entry.path().strip_prefix(root) {
            result.insert(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}
