//! Historical Codex commands are preserved, never republished from staging.
use super::*;

const REASON: &str = "unsupported_codex_command_retained_for_compatibility";
const RECEIPT: &str = ".metactl/retained-codex-commands.json";

#[derive(Serialize, Deserialize)]
struct Receipt {
    target: Ref,
    outputs: Vec<GeneratedOutput>,
}

fn receipt(root: &Path) -> Result<Option<Receipt>> {
    read_evidence(root, Path::new(RECEIPT))?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(Into::into))
        .transpose()
}

fn read_evidence(root: &Path, relative: &Path) -> Result<Option<Vec<u8>>> {
    ensure_contained_regular_path(root, relative, true)?;
    let path = root.join(relative);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            Ok(Some(fs::read(path)?))
        }
        Ok(_) => anyhow::bail!("retained command evidence must be a regular file"),
    }
}

pub(crate) fn candidates(root: &Path, target: &Ref) -> Result<Vec<GeneratedOutput>> {
    if target.id != "codex-cli" {
        return Ok(Vec::new());
    }
    let stage = root.join(".metactl/generated/codex-cli");
    let manifest_path = Path::new(".metactl/generated/codex-cli/compile.manifest.json");
    let Some(bytes) = read_evidence(root, manifest_path)? else {
        return Ok(Vec::new());
    };
    let previous: CompileManifest = serde_json::from_slice(&bytes)?;
    if previous.target != *target {
        anyhow::bail!("retained command target mismatch; preserve installed commands and reconcile the previous manifest");
    }
    let saved = receipt(root)?;
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for mut output in previous.generated_outputs {
        let Some(destination) = output.destination_path.as_deref() else {
            continue;
        };
        if !destination.starts_with(".codex/commands/") {
            continue;
        }
        validate_relative_output_path("retained command destination", destination)?;
        ensure_platform_unique_path(&mut seen, destination)?;
        ensure_contained_regular_path(root, Path::new(destination), true)?;
        if path_identity(root, &root.join(destination))? == "missing" {
            continue;
        }
        let canonical_stage =
            normalize_relative(&Path::new(".metactl/generated/codex-cli").join(destination));
        if output.path != canonical_stage {
            anyhow::bail!("retained command staging path mismatch; preserve installed commands and reconcile the previous manifest");
        }
        let staged = safe_staged_output_path(root, &stage, &output.path)?;
        ensure_contained_regular_path(root, Path::new(&output.path), false)?;
        if output.digest.as_deref() != Some(sha256_path(&staged)?.as_str()) {
            anyhow::bail!("retained command staged bytes changed; preserve installed commands and reconcile the previous manifest");
        }
        let already_retained = output.degradation_codes.iter().any(|code| code == REASON);
        if !already_retained {
            output.degradation_codes.push(REASON.into());
        }
        if let Some(saved) = &saved {
            if saved.target != *target || !saved.outputs.contains(&output) {
                anyhow::bail!("retained command metadata differs from saved previous-manifest evidence; preserve installed commands and reconcile the receipt");
            }
        } else if already_retained && previous.resolve_graph.is_some() {
            anyhow::bail!("retained command evidence is missing; preserve installed commands and restore the previous-manifest receipt");
        }
        result.push(output);
    }
    Ok(result)
}

pub(super) fn preserve_receipt(
    root: &Path,
    target: &Ref,
    outputs: &[GeneratedOutput],
) -> Result<()> {
    if outputs.is_empty() || receipt(root)?.is_some() {
        return Ok(());
    }
    atomic_write(
        &root.join(RECEIPT),
        &serde_json::to_vec_pretty(&Receipt {
            target: target.clone(),
            outputs: outputs.to_vec(),
        })?,
    )
}

pub(crate) fn validate(root: &Path, target: &Ref, output: &GeneratedOutput) -> Result<bool> {
    if target.id != "codex-cli" || !retained_codex_command(output) {
        return Ok(false);
    }
    if receipt(root)?.is_none() {
        anyhow::bail!("retained command evidence is missing; preserve installed commands and restore the previous-manifest receipt");
    }
    Ok(candidates(root, target)?.contains(output))
}
