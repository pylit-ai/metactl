use super::*;

pub(super) fn instruction_document_plan(
    _role: &RoleManifest,
    _policy: &PolicyManifest,
    packs: &[&DiscoveredPack],
    resolve_graph: &ResolveGraph,
    target: &TargetCapabilityMatrix,
    compile_target: &crate::types::CompileTarget,
) -> Result<InstructionDocumentPlan> {
    let desired_mode = compile_target
        .instruction_mode
        .clone()
        .unwrap_or(InstructionProjectionMode::Inline);
    if desired_mode == InstructionProjectionMode::Inline {
        return Ok(InstructionDocumentPlan {
            mode: InstructionProjectionMode::Inline,
            packs: packs
                .iter()
                .map(|pack| {
                    Ok(PlannedInstructionPack {
                        pack_ref: pack.manifest.pack_ref(),
                        title: pack.manifest.title.clone(),
                        description: pack.manifest.description.clone(),
                        when_to_open: when_to_open_for_pack(pack),
                        references: Vec::new(),
                        inline_snippet: Some(primary_instruction_snippet(pack)?),
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            source_resource_paths: packs
                .iter()
                .flat_map(|pack| {
                    pack.manifest
                        .resources
                        .iter()
                        .filter(|resource| resource.kind == ResourceKind::Instruction)
                        .map(|resource| resource.path.clone())
                        .collect::<Vec<_>>()
                })
                .collect(),
            degradation_codes: Vec::new(),
        });
    }

    let mut planned_packs = Vec::new();
    let mut source_resource_paths = Vec::new();
    let mut degradation_codes = Vec::new();
    let mut actual_mode = InstructionProjectionMode::ReferenceIndex;

    for pack in packs {
        let references = instruction_references_for_pack(pack, target)?;
        let mut inline_snippet = None;
        if references.is_empty() {
            let snippet = primary_instruction_snippet(pack)?;
            if !snippet.trim().is_empty() {
                inline_snippet = Some(snippet);
                actual_mode = InstructionProjectionMode::Inline;
                degradation_codes.push(format!("instruction_inline_fallback:{}", pack.manifest.id));
            }
        }
        source_resource_paths.extend(
            references
                .iter()
                .flat_map(|reference| reference.source_resource_paths.clone()),
        );
        if references.is_empty() {
            source_resource_paths.extend(
                pack.manifest
                    .resources
                    .iter()
                    .filter(|resource| resource.kind == ResourceKind::Instruction)
                    .map(|resource| resource.path.clone()),
            );
        }
        planned_packs.push(PlannedInstructionPack {
            pack_ref: pack.manifest.pack_ref(),
            title: pack.manifest.title.clone(),
            description: pack.manifest.description.clone(),
            when_to_open: when_to_open_for_pack(pack),
            references,
            inline_snippet,
        });
    }

    if !resolve_graph.capability_gaps.is_empty() && planned_packs.is_empty() {
        actual_mode = desired_mode;
    }

    source_resource_paths.sort();
    source_resource_paths.dedup();
    degradation_codes.sort();
    degradation_codes.dedup();

    Ok(InstructionDocumentPlan {
        mode: actual_mode,
        packs: planned_packs,
        source_resource_paths,
        degradation_codes,
    })
}

pub(super) fn instruction_document(
    role: &RoleManifest,
    policy: &PolicyManifest,
    plan: &InstructionDocumentPlan,
    resolve_graph: &ResolveGraph,
    target: &TargetCapabilityMatrix,
) -> Result<BudgetedInstructionDocument> {
    let mut lines = vec![
        format!("# {}", role.title),
        String::new(),
        format!(
            "[metactl Instruction Index]|target:{}|policy:{}|mode:{}",
            target.target_id,
            policy.id,
            instruction_mode_label(&plan.mode)
        ),
        "|IMPORTANT: Prefer retrieval-led reasoning over pre-training-led reasoning.".to_string(),
        format!(
            "|budget:warn={}B|max={}B",
            INSTRUCTION_INDEX_WARN_BYTES, INSTRUCTION_INDEX_MAX_BYTES
        ),
    ];
    if plan.packs.is_empty() {
        lines.push("|packs:none".to_string());
    }
    for pack in &plan.packs {
        if !pack.references.is_empty() {
            let root = common_reference_root(&pack.references);
            let surfaces = pack
                .references
                .iter()
                .map(|reference| reference.locator.clone())
                .collect::<Vec<_>>()
                .join(",");
            let mut line = format!(
                "|pack:{}|title:{}|open:{}|surfaces:{}",
                pack.pack_ref.id, pack.title, root, surfaces
            );
            if !pack.when_to_open.is_empty() {
                line.push_str(&format!("|when:{}", pack.when_to_open.join(",")));
            }
            if let Some(description) = &pack.description {
                line.push_str(&format!("|summary:{}", description.trim()));
            }
            lines.push(line);
        } else if let Some(snippet) = &pack.inline_snippet {
            lines.push(format!(
                "|inline:{}|summary:{}",
                pack.pack_ref.id,
                summarize_inline_snippet(snippet)
            ));
        } else {
            lines.push(format!(
                "|pack:{}|summary:no-emitted-body",
                pack.pack_ref.id
            ));
        }
    }
    for gap in &resolve_graph.capability_gaps {
        lines.push(format!("|gap:{}={:?}", gap.feature, gap.reason_code));
    }

    budget_instruction_document(lines.join("\n"))
}

fn instruction_references_for_pack(
    pack: &DiscoveredPack,
    target: &TargetCapabilityMatrix,
) -> Result<Vec<InstructionReference>> {
    if let Some(compile_target) = skill_compile_target_for(target) {
        let surfaces = derive_skill_surfaces(pack)?;
        if should_emit_separate_surfaces(target, compile_target, &surfaces)? {
            return surfaces
                .into_iter()
                .map(|surface| {
                    Ok(InstructionReference {
                        path: expand_skill_path(
                            &compile_target.path_template,
                            &pack.manifest.id,
                            Some(&surface.surface_slug),
                        )?,
                        locator: surface.surface_slug,
                        source_resource_paths: surface.instruction_resource_paths,
                    })
                })
                .collect();
        }

        return Ok(vec![InstructionReference {
            path: expand_skill_path(&compile_target.path_template, &pack.manifest.id, None)?,
            locator: "bundle".to_string(),
            source_resource_paths: surfaces
                .into_iter()
                .flat_map(|surface| surface.instruction_resource_paths)
                .collect(),
        }]);
    }

    let Some(compile_target) = instruction_resource_compile_target_for(target) else {
        return Ok(Vec::new());
    };

    pack.manifest
        .resources
        .iter()
        .filter(|resource| resource.kind == ResourceKind::Instruction)
        .map(|resource| {
            let path = expand_pack_resource_path(&compile_target.path_template, pack, resource)?;
            Ok(InstructionReference {
                locator: compact_reference_locator(&path),
                path,
                source_resource_paths: vec![resource.path.clone()],
            })
        })
        .collect()
}

pub(super) fn merged_skill_document(pack: &DiscoveredPack) -> Result<Vec<u8>> {
    if let Some(bytes) = primary_instruction_bytes(pack)? {
        return Ok(bytes);
    }
    let mut frontmatter = BTreeMap::new();
    frontmatter.insert("name".to_string(), pack.manifest.id.clone());
    frontmatter.insert(
        "description".to_string(),
        pack.manifest
            .description
            .clone()
            .unwrap_or_else(|| pack.manifest.title.clone()),
    );
    Ok(render_frontmatter(&frontmatter)?.into_bytes())
}

pub(super) fn derive_skill_surfaces(pack: &DiscoveredPack) -> Result<Vec<DerivedSkillSurface>> {
    let instruction_resources = pack
        .manifest
        .resources
        .iter()
        .filter(|item| item.kind == ResourceKind::Instruction)
        .collect::<Vec<_>>();
    if instruction_resources.is_empty() {
        return Ok(vec![DerivedSkillSurface {
            surface_id: format!("{}:skill", pack.manifest.id),
            surface_slug: "skill".to_string(),
            title: pack.manifest.title.clone(),
            instruction_resource_paths: Vec::new(),
            attached_script_paths: pack_resource_paths(pack, ResourceKind::Script),
            attached_reference_paths: pack_resource_paths(pack, ResourceKind::Example),
            attached_asset_paths: pack_resource_paths(pack, ResourceKind::Asset),
            contents: default_skill_surface_bytes(pack)?,
        }]);
    }

    let mut seen_slugs = BTreeSet::new();
    let primary_surface_index = instruction_resources
        .iter()
        .position(|resource| resource_file_name(resource) == Some("SKILL.md"))
        .unwrap_or(0);
    let mut surfaces = Vec::new();
    for (index, resource) in instruction_resources.iter().enumerate() {
        let contents = read_pack_resource(pack, resource)?;
        let base_candidate = resource_surface_slug(resource, &contents)
            .unwrap_or_else(|| format!("surface-{}", index + 1));
        let surface_slug = dedupe_surface_slug(&mut seen_slugs, &base_candidate);
        surfaces.push(DerivedSkillSurface {
            surface_id: format!("{}:{}", pack.manifest.id, surface_slug),
            surface_slug,
            title: surface_title(pack, resource, &contents),
            instruction_resource_paths: vec![resource.path.clone()],
            attached_script_paths: if index == primary_surface_index {
                pack_resource_paths(pack, ResourceKind::Script)
            } else {
                Vec::new()
            },
            attached_reference_paths: if index == primary_surface_index {
                pack_resource_paths(pack, ResourceKind::Example)
            } else {
                Vec::new()
            },
            attached_asset_paths: if index == primary_surface_index {
                pack_resource_paths(pack, ResourceKind::Asset)
            } else {
                Vec::new()
            },
            contents,
        });
    }
    Ok(surfaces)
}

pub(super) fn should_emit_separate_surfaces(
    target: &TargetCapabilityMatrix,
    compile_target: &crate::types::CompileTarget,
    surfaces: &[DerivedSkillSurface],
) -> Result<bool> {
    if surfaces.len() <= 1 {
        return Ok(compile_target.path_template.contains("{surface_slug}"));
    }
    if !target.capabilities.skill_folders {
        return Ok(false);
    }
    if !compile_target.supports_multi_surface_pack {
        return Ok(false);
    }
    if !compile_target.path_template.contains("{surface_slug}") {
        match compile_target
            .surface_merge_strategy
            .clone()
            .unwrap_or(SurfaceMergeStrategy::Optional)
        {
            SurfaceMergeStrategy::None => {
                return Err(anyhow!(
                "target {} requires separate surfaces but its path template omits {{surface_slug}}",
                target.target_id
            ))
            }
            SurfaceMergeStrategy::Optional | SurfaceMergeStrategy::Required => return Ok(false),
        }
    }
    Ok(true)
}

pub(super) fn effective_surface_selection_mode(
    compile_target: &crate::types::CompileTarget,
    override_mode: Option<SurfaceSelectionMode>,
) -> SurfaceSelectionMode {
    override_mode
        .or_else(|| compile_target.surface_selection_mode.clone())
        .unwrap_or(SurfaceSelectionMode::Full)
}

fn primary_instruction_path(pack: &DiscoveredPack) -> Option<&str> {
    pack.manifest
        .resources
        .iter()
        .find(|resource| resource.kind == ResourceKind::Instruction)
        .map(|resource| resource.path.as_str())
}

fn surface_relevance_tier(
    pack: &DiscoveredPack,
    surface: &DerivedSkillSurface,
) -> SurfaceRelevanceTier {
    let explicit = surface
        .instruction_resource_paths
        .first()
        .and_then(|resource_path| {
            pack.manifest
                .resources
                .iter()
                .find(|resource| resource.path == *resource_path)
                .and_then(|resource| resource.surface_relevance.clone())
        });
    explicit.unwrap_or_else(|| {
        if surface
            .instruction_resource_paths
            .first()
            .and_then(|resource_path| {
                primary_instruction_path(pack).map(|primary| resource_path == primary)
            })
            .unwrap_or(false)
        {
            SurfaceRelevanceTier::AlwaysOn
        } else {
            SurfaceRelevanceTier::Suppressible
        }
    })
}

pub(super) fn surface_selection_decisions(
    pack: &DiscoveredPack,
    surfaces: &[DerivedSkillSurface],
    mode: SurfaceSelectionMode,
) -> Vec<SurfaceSelectionDecision> {
    surfaces
        .iter()
        .map(|surface| {
            let relevance_tier = surface_relevance_tier(pack, surface);
            let emitted = match mode {
                SurfaceSelectionMode::Full => true,
                SurfaceSelectionMode::Minimal | SurfaceSelectionMode::Auto => {
                    relevance_tier == SurfaceRelevanceTier::AlwaysOn
                }
            };
            let reason_code = if emitted {
                None
            } else {
                Some(ReasonCode::SuppressedByMode)
            };
            let detail = if emitted {
                None
            } else {
                Some("Surface is suppressible and omitted in minimal surface mode.".to_string())
            };
            SurfaceSelectionDecision {
                pack_ref: pack.manifest.pack_ref(),
                surface_id: surface.surface_id.clone(),
                surface_slug: surface.surface_slug.clone(),
                relevance_tier,
                emitted,
                reason_code,
                detail,
                source_resource_paths: surface.instruction_resource_paths.clone(),
            }
        })
        .collect()
}

pub(super) fn expand_skill_path(
    template: &str,
    pack_id: &str,
    surface_slug: Option<&str>,
) -> Result<String> {
    let mut path = template.replace("{pack_id}", pack_id);
    if let Some(surface_slug) = surface_slug {
        path = path.replace("{surface_slug}", surface_slug);
    } else if path.contains("{surface_slug}") {
        return Err(anyhow!(
            "path template {} requires {{surface_slug}} for separate surface output",
            template
        ));
    }
    Ok(path)
}

fn expand_pack_resource_path(
    template: &str,
    pack: &DiscoveredPack,
    resource: &PackResource,
) -> Result<String> {
    // When the template has no per-resource token, callers must guarantee a
    // single resource per pack (e.g. `primary_instruction_only`). Otherwise
    // multiple resources would collide on the same destination.
    let has_resource_token = template.contains("{resource_path}")
        || template.contains("{resource_name}")
        || template.contains("{resource_slug}");
    if !has_resource_token && !template.contains("{pack_id}") {
        return Err(anyhow!(
            "path template {} requires one of {{resource_path}}, {{resource_name}}, or {{resource_slug}} for pack_resource output (or {{pack_id}} for primary-only emission)",
            template
        ));
    }

    let resource_path = pack_resource_relative_path(pack, resource);
    let resource_name = resource_file_name(resource)
        .ok_or_else(|| anyhow!("resource {} is missing a file name", resource.path))?;
    let resource_contents = read_pack_resource(pack, resource)?;
    let resource_slug = resource_surface_slug(resource, &resource_contents)
        .or_else(|| slugify_surface_candidate(resource_name))
        .ok_or_else(|| anyhow!("resource {} could not derive a slug", resource.path))?;

    Ok(template
        .replace("{pack_id}", &pack.manifest.id)
        .replace("{resource_path}", &resource_path)
        .replace("{resource_name}", resource_name)
        .replace("{resource_slug}", &resource_slug))
}

pub(super) fn emit_pack_extension_manifests(
    compile_target: &crate::types::CompileTarget,
    packs: &[&DiscoveredPack],
) -> Result<Vec<StagedOutputInput>> {
    let mut outputs = Vec::new();
    for pack in packs {
        let destination = compile_target
            .path_template
            .replace("{pack_id}", &pack.manifest.id);
        let description = pack
            .manifest
            .description
            .clone()
            .unwrap_or_else(|| pack.manifest.title.clone());
        let manifest = serde_json::json!({
            "name": pack.manifest.id,
            "description": description,
            "version": pack.manifest.version,
            "contextFileName": "GEMINI.md",
        });
        let contents = serde_json::to_vec_pretty(&manifest)?;
        outputs.push(StagedOutputInput {
            id: Some(format!("pack-extension-manifest-{}", pack.manifest.id)),
            destination_path: destination,
            kind: GeneratedOutputKind::PackExtensionManifest,
            contents,
            instruction_mode: None,
            pack_ref: Some(pack.manifest.pack_ref()),
            surface_id: None,
            surface_slug: None,
            source_resource_paths: Vec::new(),
            merge_status: None,
            degradation_codes: Vec::new(),
            ownership_token: Some(format!("{}::extension-manifest", pack.manifest.id)),
            materialize_as_regular_file: compile_target.materialize_as_regular_file,
        });
    }
    Ok(outputs)
}

pub(super) fn emit_pack_resource_outputs(
    compile_target: &crate::types::CompileTarget,
    packs: &[&DiscoveredPack],
) -> Result<Vec<StagedOutputInput>> {
    if compile_target.resource_kinds.is_empty() {
        return Err(anyhow!(
            "pack_resource compile target {} must declare resource_kinds",
            compile_target.path_template
        ));
    }

    // Pre-compute the pack's primary instruction path (first declared
    // `Instruction` resource) so `primary_instruction_only` can filter
    // siblings without re-scanning the manifest per resource.
    let mut outputs = Vec::new();
    for pack in packs {
        let primary_instruction_path: Option<String> = pack
            .manifest
            .resources
            .iter()
            .find(|r| r.kind == ResourceKind::Instruction)
            .map(|r| r.path.clone());
        for resource in pack
            .manifest
            .resources
            .iter()
            .filter(|resource| compile_target.resource_kinds.contains(&resource.kind))
        {
            if compile_target.primary_instruction_only && resource.kind == ResourceKind::Instruction
            {
                let is_primary = primary_instruction_path
                    .as_deref()
                    .map(|p| p == resource.path.as_str())
                    .unwrap_or(false);
                if !is_primary {
                    continue;
                }
            }
            let destination =
                expand_pack_resource_path(&compile_target.path_template, pack, resource)?;
            let relative_path = pack_resource_relative_path(pack, resource);
            let raw_contents = read_pack_resource(pack, resource)?;
            let contents = if resource.kind == ResourceKind::Command {
                apply_command_adapter(&raw_contents, compile_target.command_adapter.as_ref(), pack)
            } else {
                raw_contents
            };
            outputs.push(StagedOutputInput {
                id: Some(pack_resource_output_id(&pack.manifest.id, &relative_path)),
                destination_path: destination,
                kind: GeneratedOutputKind::ResourceFile,
                contents,
                instruction_mode: None,
                pack_ref: Some(pack.manifest.pack_ref()),
                surface_id: None,
                surface_slug: None,
                source_resource_paths: vec![resource.path.clone()],
                merge_status: None,
                degradation_codes: Vec::new(),
                ownership_token: Some(format!("{}::resource:{}", pack.manifest.id, relative_path)),
                materialize_as_regular_file: compile_target.materialize_as_regular_file,
            });
        }
    }
    Ok(outputs)
}

pub(super) fn skill_surface_document(
    pack: &DiscoveredPack,
    surface: &DerivedSkillSurface,
    supports_frontmatter: bool,
) -> Result<Vec<u8>> {
    if !supports_frontmatter {
        return Ok(surface.contents.clone());
    }
    if markdown_has_frontmatter(&surface.contents) {
        let contents = String::from_utf8_lossy(&surface.contents);
        let failures = validate_skill_frontmatter_text(&contents);
        if !failures.is_empty() {
            return Err(anyhow!(
                "instruction surface {} has invalid skill frontmatter: {}",
                surface.surface_id,
                failures.join(" ")
            ));
        }
        return Ok(surface.contents.clone());
    }
    let mut frontmatter = BTreeMap::new();
    frontmatter.insert("name".to_string(), surface.title.clone());
    frontmatter.insert(
        "description".to_string(),
        pack.manifest
            .description
            .clone()
            .unwrap_or_else(|| pack.manifest.title.clone()),
    );
    frontmatter.insert("pack_id".to_string(), pack.manifest.id.clone());
    frontmatter.insert("surface_slug".to_string(), surface.surface_slug.clone());
    let mut document = render_frontmatter(&frontmatter)?.into_bytes();
    document.push(b'\n');
    document.extend_from_slice(&surface.contents);
    Ok(document)
}

/// Expand a target's runtime-config template into emitted bytes + kind.
///
/// The kernel knows nothing about which target has which config shape — it
/// reads `target.runtime_template.path` from the library root, seeds a
/// context map from policy + target capabilities + resolve graph, and runs
/// `substitute_tokens`. Adding or changing a target's runtime config is now
/// a data-only edit (target JSON + template file).
pub(super) fn expand_runtime_template(
    library_roots: &[PathBuf],
    template_ref: &RuntimeTemplateRef,
    target: &TargetCapabilityMatrix,
    policy: &PolicyManifest,
    resolve_graph: &ResolveGraph,
    packs: &[&DiscoveredPack],
) -> Result<(GeneratedOutputKind, Vec<u8>)> {
    let (tmpl_path, raw) = library_roots
        .iter()
        .map(|root| root.join(&template_ref.path))
        .find_map(|candidate| {
            std::fs::read_to_string(&candidate)
                .ok()
                .map(|raw| (candidate, raw))
        })
        .ok_or_else(|| {
            anyhow!(
                "runtime template not found in any library root: {}",
                template_ref.path
            )
        })?;
    let _ = tmpl_path;

    let mut ctx: BTreeMap<String, String> = BTreeMap::new();
    ctx.insert("policy_id".into(), policy.id.clone());
    ctx.insert("target_id".into(), target.target_id.clone());
    let approval = if target.capabilities.approval_policies {
        "on-request"
    } else {
        "advisory"
    };
    ctx.insert("approval_policy".into(), approval.into());
    ctx.insert("approval_mode".into(), approval.into());
    ctx.insert("sandbox_mode".into(), "workspace-write".into());
    ctx.insert(
        "readonly_hints".into(),
        target.capabilities.readonly_hints.to_string(),
    );
    ctx.insert(
        "active_packs_json_array".into(),
        serde_json::to_string(
            &resolve_graph
                .activated_pack_refs
                .iter()
                .map(|item| &item.id)
                .collect::<Vec<_>>(),
        )
        .unwrap_or_else(|_| "[]".into()),
    );
    let hooks_value = aggregate_hook_wirings_for_target(target, packs)?;
    ctx.insert(
        "hooks_json".into(),
        serde_json::to_string(&hooks_value).unwrap_or_else(|_| "{}".into()),
    );

    let expanded = substitute_tokens(&raw, &ctx);
    let kind = match template_ref.output_kind.clone() {
        Some(CompileTargetKind::HookConfig) => GeneratedOutputKind::HookConfig,
        Some(CompileTargetKind::McpConfig) => GeneratedOutputKind::McpConfig,
        _ => GeneratedOutputKind::RuntimeJson,
    };
    Ok((kind, expanded.into_bytes()))
}

fn surface_title(pack: &DiscoveredPack, resource: &PackResource, contents: &[u8]) -> String {
    if let Some(name) = frontmatter_name(contents) {
        return name;
    }
    let text = String::from_utf8_lossy(contents);
    if let Some(title) = text.lines().find_map(|line| {
        line.strip_prefix("# ")
            .map(|value| value.trim().to_string())
    }) {
        return title;
    }
    instruction_resource_heading(resource)
        .split_whitespace()
        .map(capitalize_surface_word)
        .collect::<Vec<_>>()
        .join(" ")
        .if_empty_then(pack.manifest.title.clone())
}

pub(super) fn semantic_carrier_parent_slug(resource: &PackResource) -> Option<String> {
    let file_name = resource_file_name(resource)?;
    if !matches!(file_name, "SKILL.md" | "README.md" | "INDEX.md") {
        return None;
    }
    Path::new(&resource.path)
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|value| value.to_str())
        .and_then(slugify_surface_candidate)
}

pub(super) fn frontmatter_name(contents: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(contents);
    let mut lines = text.lines();
    if lines.next()? != "---" {
        return None;
    }
    for line in lines {
        if line == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix("name:") {
            let cleaned = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
    }
    None
}

fn dedupe_surface_slug(seen: &mut BTreeSet<String>, base: &str) -> String {
    if seen.insert(base.to_string()) {
        return base.to_string();
    }
    for index in 2.. {
        let candidate = format!("{}-{}", base, index);
        if seen.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!("surface slug dedupe must terminate")
}

pub(super) fn slugify_surface_candidate(candidate: &str) -> Option<String> {
    let slug = candidate
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        None
    } else {
        Some(
            slug.split('-')
                .filter(|segment| !segment.is_empty())
                .collect::<Vec<_>>()
                .join("-"),
        )
    }
}

fn markdown_has_frontmatter(contents: &[u8]) -> bool {
    String::from_utf8_lossy(contents).starts_with("---\n")
}

/// Wrap a `Command` resource body in a per-target envelope (Markdown
/// frontmatter or TOML prompt object) when the compile target declares a
/// `command_adapter`. Returning the body unchanged when no adapter is set
/// preserves byte-for-byte parity with the pre-spec-019 behaviour.
///
/// Description fallback chain: `pack.manifest.description` →
/// `pack.manifest.title` → empty string. `PackResource` itself does not
/// carry a per-resource description today, so the pack-level description
/// is the most specific source available.
fn apply_command_adapter(
    body: &[u8],
    adapter: Option<&crate::types::CommandAdapter>,
    pack: &DiscoveredPack,
) -> Vec<u8> {
    let Some(adapter) = adapter else {
        return body.to_vec();
    };
    let body_str = String::from_utf8_lossy(body).into_owned();
    let description = pack
        .manifest
        .description
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| pack.manifest.title.clone());
    let escaped = description.replace('\\', "\\\\").replace('"', "\\\"");
    match adapter.format {
        crate::types::CommandAdapterFormat::Markdown => {
            if !adapter.inject_description {
                return body.to_vec();
            }
            if markdown_has_frontmatter(body) {
                return body.to_vec();
            }
            let mut out = String::from("---\n");
            out.push_str(&format!("description: \"{}\"\n", escaped));
            out.push_str("---\n\n");
            out.push_str(&body_str);
            out.into_bytes()
        }
        crate::types::CommandAdapterFormat::Toml => {
            let mut out = String::new();
            out.push_str(&format!("description = \"{}\"\n", escaped));
            out.push_str("prompt = \"\"\"\n");
            out.push_str(&body_str);
            if !body_str.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("\"\"\"\n");
            out.into_bytes()
        }
    }
}

pub(super) fn skill_compile_target_for(
    target: &TargetCapabilityMatrix,
) -> Option<&crate::types::CompileTarget> {
    target
        .compile_targets
        .iter()
        .find(|item| item.output_kind == CompileTargetKind::CodexSkill)
}

fn instruction_resource_compile_target_for(
    target: &TargetCapabilityMatrix,
) -> Option<&crate::types::CompileTarget> {
    target.compile_targets.iter().find(|item| {
        item.output_kind == CompileTargetKind::PackResource
            && item.resource_kinds.contains(&ResourceKind::Instruction)
    })
}
