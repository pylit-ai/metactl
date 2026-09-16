use super::{
    read_pack_resource, BudgetedInstructionDocument, DiscoveredPack, InstructionReference,
    INSTRUCTION_INDEX_MAX_BYTES, INSTRUCTION_INDEX_POINTER, INSTRUCTION_INDEX_WARN_BYTES,
};
use crate::types::{ActivationClass, InstructionProjectionMode, PackResource};
use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::path::Path;

pub(super) fn instruction_mode_label(mode: &InstructionProjectionMode) -> &'static str {
    match mode {
        InstructionProjectionMode::Inline => "inline",
        InstructionProjectionMode::ReferenceIndex => "reference_index",
    }
}

pub(super) fn common_reference_root(references: &[InstructionReference]) -> String {
    let mut segments = references
        .first()
        .map(|reference| {
            reference
                .path
                .split('/')
                .map(|segment| segment.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if segments.is_empty() {
        return String::new();
    }
    for reference in references.iter().skip(1) {
        let other = reference.path.split('/').collect::<Vec<_>>();
        let mut prefix_len = 0usize;
        while prefix_len < segments.len()
            && prefix_len < other.len()
            && segments[prefix_len] == other[prefix_len]
        {
            prefix_len += 1;
        }
        segments.truncate(prefix_len);
    }
    if let Some(last) = segments.last() {
        if last.contains('.') {
            segments.pop();
        }
    }
    let mut root = segments.join("/");
    if !root.is_empty() {
        root.push('/');
    }
    root
}

pub(super) fn compact_reference_locator(path: &str) -> String {
    let trimmed = path
        .trim_end_matches("/SKILL.md")
        .trim_end_matches("/README.md")
        .trim_end_matches(".md");
    trimmed
        .rsplit_once('/')
        .map(|(_, tail)| tail.to_string())
        .unwrap_or_else(|| trimmed.to_string())
}

pub(super) fn when_to_open_for_pack(pack: &DiscoveredPack) -> Vec<String> {
    if !pack.manifest.task_tags.is_empty() {
        return pack.manifest.task_tags.clone();
    }
    vec![match pack.manifest.activation_class {
        ActivationClass::Instruction => "general guidance",
        ActivationClass::Script => "scripted workflows",
        ActivationClass::Hook => "approval or write boundaries",
        ActivationClass::Service => "service or MCP setup",
    }
    .to_string()]
}

pub(super) fn summarize_inline_snippet(snippet: &str) -> String {
    let compact = snippet
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("---"))
        .collect::<Vec<_>>()
        .join(" ");
    if compact.len() <= 200 {
        compact
    } else {
        format!("{}...", &compact[..197])
    }
}

pub(super) fn budget_instruction_document(content: String) -> Result<BudgetedInstructionDocument> {
    let mut budgeted = content;
    let mut truncated = false;
    if budgeted.len() > INSTRUCTION_INDEX_WARN_BYTES {
        budgeted = truncate_instruction_document(&budgeted, INSTRUCTION_INDEX_WARN_BYTES);
        truncated = true;
    }
    if budgeted.len() > INSTRUCTION_INDEX_WARN_BYTES
        && budgeted.len() <= INSTRUCTION_INDEX_MAX_BYTES
    {
        return Err(anyhow!(
            "instruction index could not fit within {} bytes using structured truncation",
            INSTRUCTION_INDEX_WARN_BYTES
        ));
    }
    if budgeted.len() > INSTRUCTION_INDEX_MAX_BYTES {
        return Err(anyhow!(
            "instruction index exceeds {} bytes after truncation; reduce active pack routing detail",
            INSTRUCTION_INDEX_MAX_BYTES
        ));
    }
    Ok(BudgetedInstructionDocument {
        content: budgeted,
        truncated,
    })
}

fn truncate_instruction_document(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.to_string();
    }

    let mut lines = content
        .lines()
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    let mut total_bytes = lines.join("\n").len();
    let mut candidates = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.starts_with("|pack:") || line.starts_with("|inline:"))
        .map(|(index, line)| (index, line.len()))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.1.cmp(&left.1));

    for (index, _) in candidates {
        if total_bytes <= max_bytes {
            break;
        }
        let original = &lines[index];
        let truncated_line = truncate_instruction_line(original);
        if truncated_line.len() < original.len() {
            total_bytes = total_bytes - original.len() + truncated_line.len();
            lines[index] = truncated_line;
        }
    }

    if total_bytes > max_bytes {
        while total_bytes > max_bytes {
            let Some(index) = lines
                .iter()
                .rposition(|line| line.starts_with("|gap:") || line.starts_with("|pack:"))
            else {
                break;
            };
            total_bytes -= lines[index].len() + 1;
            lines.remove(index);
        }
    }

    if !lines
        .iter()
        .any(|line| line.contains(INSTRUCTION_INDEX_POINTER))
    {
        lines.push(format!("|truncated:{}", INSTRUCTION_INDEX_POINTER));
        while lines.join("\n").len() > max_bytes {
            let Some(index) = lines
                .iter()
                .rposition(|line| line.starts_with("|pack:") || line.starts_with("|gap:"))
            else {
                break;
            };
            lines.remove(index);
        }
    }

    lines.join("\n")
}

fn truncate_instruction_line(line: &str) -> String {
    if line.len() <= 180 {
        return line.to_string();
    }
    let prefix: String = line.chars().take(140).collect();
    format!("{prefix}…|truncated:{INSTRUCTION_INDEX_POINTER}")
}

pub(super) fn capitalize_surface_word(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

pub(super) trait IfEmptyThen {
    fn if_empty_then(self, fallback: String) -> String;
}

impl IfEmptyThen for String {
    fn if_empty_then(self, fallback: String) -> String {
        if self.trim().is_empty() {
            fallback
        } else {
            self
        }
    }
}

pub(super) fn instruction_resource_heading(resource: &PackResource) -> String {
    resource
        .path
        .rsplit('/')
        .next()
        .unwrap_or(resource.path.as_str())
        .trim_end_matches(".md")
        .replace(['-', '_'], " ")
}

pub(super) fn pack_resource_relative_path(
    pack: &DiscoveredPack,
    resource: &PackResource,
) -> String {
    let prefix = format!("packs/{}/", pack.manifest.id);
    let stripped = resource
        .path
        .strip_prefix(&prefix)
        .unwrap_or(resource.path.as_str());
    // Also strip a leading kind-directory segment (e.g. "commands/") so target
    // templates of the form "{kind}/{pack_id}/{resource_path}" do not double-nest
    // the kind segment (bug: spec 019 task 1.1).
    let kind_prefix = format!("{}/", resource.kind.as_directory_segment());
    stripped
        .strip_prefix(&kind_prefix)
        .unwrap_or(stripped)
        .to_string()
}

pub(super) fn pack_resource_output_id(pack_id: &str, relative_path: &str) -> String {
    let slug = relative_path
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    format!("resource-{}-{}", pack_id, slug)
}

pub(super) fn declared_skill_card_metadata(
    pack: &DiscoveredPack,
    skill_resource: &PackResource,
) -> Result<(Vec<String>, Vec<String>, Vec<String>, Vec<String>)> {
    let Some(parent) = Path::new(&skill_resource.path).parent() else {
        return Ok((Vec::new(), Vec::new(), Vec::new(), Vec::new()));
    };
    let card_path = parent.join("skill-card.json").to_string_lossy().to_string();
    let Some(card_resource) = pack
        .manifest
        .resources
        .iter()
        .find(|resource| resource.path == card_path)
    else {
        return Ok((Vec::new(), Vec::new(), Vec::new(), Vec::new()));
    };
    let card: Value = serde_json::from_slice(&read_pack_resource(pack, card_resource)?)
        .with_context(|| format!("parse declared skill card {card_path}"))?;
    crate::skill_card::validate_skill_card(&card)
        .with_context(|| format!("validate declared skill card {card_path}"))?;
    let strings = |value: Option<&Value>| -> Vec<String> {
        value
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let intents = card.get("intents").and_then(Value::as_object);
    let compatibility = card.get("host_compatibility").and_then(Value::as_object);
    Ok((
        strings(card.get("aliases")),
        strings(intents.and_then(|value| value.get("positive"))),
        strings(intents.and_then(|value| value.get("negative"))),
        strings(compatibility.and_then(|value| value.get("targets"))),
    ))
}
