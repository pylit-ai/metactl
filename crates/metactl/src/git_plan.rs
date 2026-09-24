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
    let staged_changes = if git {
        git_paths(root, &["diff", "--cached", "--name-only", "-z", "--"], &[])?
    } else {
        BTreeSet::new()
    };
    let index_divergence = if git {
        git_paths(root, &["diff", "--name-only", "-z", "--"], &[])?
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
    let root_canonical = root.canonicalize()?;
    for path in paths {
        let absolute = root.join(&path);
        let metadata = match absolute.symlink_metadata() {
            Ok(metadata) => Some(metadata),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => return Err(err).with_context(|| format!("inspect {}", path)),
        };
        let exists = metadata.is_some();
        let path_kind = match metadata.as_ref() {
            Some(metadata) if metadata.file_type().is_symlink() => "symlink",
            Some(metadata) if metadata.is_file() => "regular_file",
            Some(metadata) if metadata.is_dir() => "directory",
            Some(_) => "other",
            None => "missing",
        };
        let contained = !exists
            || absolute
                .canonicalize()
                .map(|resolved| resolved.starts_with(&root_canonical))
                .unwrap_or(false);
        let owner = owners.get(&path);
        let index_differs = index_divergence.contains(&path);
        let classification = if !contained {
            "unsafe_alias"
        } else {
            match owner {
                None => "unowned",
                Some(_) if !exists => "missing",
                Some(owner) => match owner.digest.as_deref() {
                    Some(expected)
                        if !index_differs
                            && file_digest(&absolute).as_deref() == Some(expected) =>
                    {
                        "managed_unchanged"
                    }
                    _ => "managed_edited",
                },
            }
        };
        *counts.entry(classification).or_default() += 1;
        entries.push(json!({
            "path": path,
            "classification": classification,
            "present": exists,
            "path_kind": path_kind,
            "git": if !git { "not_git" } else if tracked.contains(&path) { "tracked" } else { "untracked" },
            "staged_change": staged_changes.contains(&path),
            "index_differs_from_worktree": index_differs,
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
            "unsafe_alias": counts.get("unsafe_alias").copied().unwrap_or(0),
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
    if !dir.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err(anyhow!("MetaCTL state resolves outside the project"));
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
        if !path.canonicalize()?.starts_with(root.canonicalize()?) {
            return Err(anyhow!("MetaCTL state file resolves outside the project"));
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
            let relative = safe_relative(root, raw)
                .ok_or_else(|| anyhow!("MetaCTL state contains an unsafe destination path"))?;
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
        .any(|root| path == *root || path.starts_with(&format!("{root}/")))
}

fn collect_paths(root: &Path, dir: &Path, result: &mut BTreeSet<String>) -> Result<()> {
    match dir.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if let Ok(relative) = dir.strip_prefix(root) {
                result.insert(relative.to_string_lossy().replace('\\', "/"));
            }
            return Ok(());
        }
        Ok(metadata) if !metadata.is_dir() => return Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
        _ => {}
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
