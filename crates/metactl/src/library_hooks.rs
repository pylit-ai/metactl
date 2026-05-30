use super::*;

/// Build the `hooks` map for a runtime template by aggregating every
/// `HookWiring` resource from the activated packs whose `compatible_targets`
/// includes the requested target. Returns an empty object when the target
/// does not advertise `deterministic_hooks`.
///
/// The kernel does not invent matchers, events, or commands — packs declare
/// them. The materialized command path mirrors the `.claude/hooks/{pack_id}/
/// {resource_path}` template that `emit_pack_resource_outputs` produces for
/// the sibling `Hook` script (see `library/starter/targets/claude-code.json`).
pub(super) fn aggregate_hook_wirings_for_target(
    target: &TargetCapabilityMatrix,
    packs: &[&DiscoveredPack],
) -> Result<serde_json::Value> {
    if !target.capabilities.deterministic_hooks {
        return Ok(serde_json::json!({}));
    }
    let mut events: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    for pack in packs {
        for resource in &pack.manifest.resources {
            if resource.kind != ResourceKind::HookWiring {
                continue;
            }
            let bytes = read_pack_resource(pack, resource)?;
            let wiring: serde_json::Value = serde_json::from_slice(&bytes)
                .with_context(|| format!("malformed hook wiring: {}", resource.path))?;
            if let Some(compat) = wiring.get("compatible_targets").and_then(|v| v.as_array()) {
                if !compat
                    .iter()
                    .any(|v| v.as_str() == Some(target.target_id.as_str()))
                {
                    continue;
                }
            }
            let event = wiring
                .get("event")
                .and_then(|v| v.as_str())
                .unwrap_or("PostToolUse")
                .to_string();
            let matcher = wiring
                .get("matcher")
                .and_then(|v| v.as_str())
                .unwrap_or("*")
                .to_string();
            let command_ref = wiring
                .get("command_ref")
                .and_then(|v| v.as_str())
                .with_context(|| format!("hook wiring {} missing command_ref", resource.path))?;
            // Resolve command_ref against the pack's Hook resources so the
            // emitted `command` matches the path that emit_pack_resource_outputs
            // will write into the project. Falls back to a basename derived from
            // command_ref if no matching Hook resource is declared.
            let hook_resource = pack
                .manifest
                .resources
                .iter()
                .find(|r| r.kind == ResourceKind::Hook && r.path == command_ref);
            let materialized = match hook_resource {
                Some(hook) => format!(
                    ".claude/hooks/{}/{}",
                    pack.manifest.id,
                    pack_resource_relative_path(pack, hook)
                ),
                None => {
                    let basename = command_ref
                        .rsplit('/')
                        .next()
                        .unwrap_or(command_ref)
                        .to_string();
                    format!(".claude/hooks/{}/{}", pack.manifest.id, basename)
                }
            };
            events.entry(event).or_default().push(serde_json::json!({
                "matcher": matcher,
                "hooks": [{ "type": "command", "command": materialized }],
            }));
        }
    }
    Ok(serde_json::to_value(events).unwrap_or_else(|_| serde_json::json!({})))
}

pub(super) fn primary_instruction_snippet(pack: &DiscoveredPack) -> Result<String> {
    let Some(bytes) = primary_instruction_bytes(pack)? else {
        return Ok(String::new());
    };
    Ok(String::from_utf8(bytes).unwrap_or_default())
}

pub(super) fn primary_instruction_bytes(pack: &DiscoveredPack) -> Result<Option<Vec<u8>>> {
    let instructions: Vec<&PackResource> = pack
        .manifest
        .resources
        .iter()
        .filter(|item| item.kind == ResourceKind::Instruction)
        .collect();
    if instructions.is_empty() {
        return Ok(None);
    }

    let mut combined = Vec::<u8>::new();
    for (index, resource) in instructions.iter().enumerate() {
        let bytes = read_pack_resource(pack, resource)?;
        if index == 0 {
            combined = bytes;
            continue;
        }
        let label = instruction_resource_heading(resource);
        combined.extend_from_slice(b"\n\n---\n\n## ");
        combined.extend_from_slice(label.as_bytes());
        combined.extend_from_slice(b"\n\n");
        combined.extend_from_slice(&bytes);
    }
    Ok(Some(combined))
}

pub(super) fn default_skill_surface_bytes(pack: &DiscoveredPack) -> Result<Vec<u8>> {
    let description = pack
        .manifest
        .description
        .clone()
        .unwrap_or_else(|| "No bundled instruction resource was available.".to_string());
    let mut frontmatter = BTreeMap::new();
    frontmatter.insert("name".to_string(), pack.manifest.title.clone());
    frontmatter.insert("description".to_string(), description.clone());
    let mut document = render_frontmatter(&frontmatter)?.into_bytes();
    document
        .extend_from_slice(format!("\n# {}\n\n{}\n", pack.manifest.title, description).as_bytes());
    Ok(document)
}

pub(super) fn pack_resource_paths(pack: &DiscoveredPack, kind: ResourceKind) -> Vec<String> {
    pack.manifest
        .resources
        .iter()
        .filter(|item| item.kind == kind)
        .map(|item| item.path.clone())
        .collect()
}

pub(super) fn resource_surface_slug(resource: &PackResource, contents: &[u8]) -> Option<String> {
    frontmatter_name(contents)
        .and_then(|value| slugify_surface_candidate(&value))
        .or_else(|| semantic_carrier_parent_slug(resource))
        .or_else(|| {
            Path::new(&resource.path)
                .file_stem()
                .and_then(|value| value.to_str())
                .and_then(slugify_surface_candidate)
        })
        .or_else(|| first_heading_slug(contents))
}

pub(super) fn resource_file_name(resource: &PackResource) -> Option<&str> {
    Path::new(&resource.path)
        .file_name()
        .and_then(|value| value.to_str())
}

fn first_heading_slug(contents: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(contents);
    text.lines()
        .find_map(|line| line.strip_prefix("# ").and_then(slugify_surface_candidate))
}
