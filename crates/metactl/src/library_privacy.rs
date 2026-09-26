use super::*;

/// Synthesis evidence, not installed-state testimony. No project files are
/// written while deriving these destination and byte identities.
#[derive(Debug, Clone)]
pub struct ProjectionProof {
    pub destination: String,
    pub staged_path: String,
    pub digest: String,
    pub private: bool,
}

fn private_output(
    output: &StagedOutputInput,
    graph: &ResolveGraph,
    target: &TargetCapabilityMatrix,
) -> bool {
    output
        .pack_ref
        .as_ref()
        .is_some_and(|p| graph.pack_visibility.get(&p.id) != Some(&VisibilityScope::Shared))
        || target
            .local_projection
            .as_ref()
            .and_then(|p| p.local_surface.as_ref())
            .is_some_and(|p| p == &output.destination_path)
}

pub(super) fn private_paths(
    outputs: &[StagedOutputInput],
    graph: &ResolveGraph,
    target: &TargetCapabilityMatrix,
) -> Vec<String> {
    let mut paths: Vec<_> = outputs
        .iter()
        .filter(|o| private_output(o, graph, target))
        .map(|o| o.destination_path.clone())
        .collect();
    if !paths.is_empty()
        || graph
            .pack_visibility
            .values()
            .any(|v| *v != VisibilityScope::Shared)
    {
        // Ignoring the directory itself protects future staged payloads and
        // journals, including filenames not allocated until publication.
        paths.push(".metactl/".into());
    }
    paths
}

impl LibraryRegistry {
    pub(super) fn validate_privacy_graph(&self, graph: &ResolveGraph) -> Result<()> {
        for pack_ref in &graph.activated_pack_refs {
            let pack = self
                .find_pack(pack_ref)
                .ok_or_else(|| anyhow!("missing pack {}", pack_ref.id))?;
            if graph.pack_visibility.get(&pack.manifest.id) != Some(&pack.manifest.visibility_scope)
            {
                anyhow::bail!("pack visibility provenance is stale; resolve and recompile");
            }
        }
        Ok(())
    }
    pub(super) fn protect_private_manifest(
        &self,
        root: &Path,
        manifest: &CompileManifest,
        apply_mode: &ApplyMode,
    ) -> Result<()> {
        let target = self.target_by_id(&manifest.target.id);
        let local = target
            .as_ref()
            .and_then(|t| t.local_projection.as_ref())
            .and_then(|l| l.local_surface.as_ref());
        let proofs = if let Some(graph) = &manifest.resolve_graph {
            let target = target
                .clone()
                .ok_or_else(|| anyhow!("unknown target; recompile before apply"))?;
            if graph.selected_target != manifest.target {
                anyhow::bail!("manifest target provenance mismatch");
            }
            Some(self.projection_proofs(&CompileParams {
                resolve_graph: graph.clone(),
                target_capability: target,
                apply_mode: apply_mode.clone(),
                surface_selection_mode: manifest.surface_selection_mode.clone(),
                emit_policy_report: false,
                durable_staging: false,
                project_root: None,
            })?)
        } else {
            None
        };
        let mut replay_private = std::collections::BTreeSet::new();
        for output in &manifest.generated_outputs {
            let Some(destination) = &output.destination_path else {
                continue;
            };
            let proof = proofs
                .as_ref()
                .and_then(|p| p.iter().find(|p| &p.destination == destination));
            if let Some(proof) = proof {
                if output.path != proof.staged_path || output.digest.as_ref() != Some(&proof.digest)
                {
                    anyhow::bail!("stale or altered projection evidence; recompile before apply");
                }
                if proof.private {
                    replay_private.insert(destination.clone());
                }
            } else if matches!(
                output.kind,
                GeneratedOutputKind::RuntimeJson
                    | GeneratedOutputKind::HookConfig
                    | GeneratedOutputKind::McpConfig
            ) || target
                .as_ref()
                .and_then(|t| t.runtime_template.as_ref())
                .is_some_and(|t| &t.destination_path == destination)
                || target.as_ref().is_some_and(|t| {
                    t.compile_targets.iter().any(|c| {
                        c.output_kind == CompileTargetKind::McpConfig
                            && &c.path_template == destination
                    })
                })
            {
                // Legacy aggregates lack independent synthesis evidence. They may
                // only be applied into protected private destinations.
                replay_private.insert(destination.clone());
            }
        }
        let mut paths: Vec<String> = manifest
            .generated_outputs
            .iter()
            .filter(|o| {
                o.pack_ref.as_ref().is_some_and(|p| {
                    self.find_pack(p)
                        .is_none_or(|p| p.manifest.visibility_scope != VisibilityScope::Shared)
                }) || o.destination_path.as_ref().is_some_and(|p| {
                    target.is_none() || Some(p) == local || replay_private.contains(p)
                })
            })
            .filter_map(|o| o.destination_path.clone())
            .collect();
        if !paths.is_empty()
            || manifest.resolve_graph.as_ref().is_some_and(|g| {
                g.pack_visibility
                    .values()
                    .any(|v| *v != VisibilityScope::Shared)
            })
        {
            paths.push(".metactl/".into());
        }
        crate::git_privacy::require_private(root, &paths)
    }

    pub fn projection_proofs(&self, params: &CompileParams) -> Result<Vec<ProjectionProof>> {
        let graph = &params.resolve_graph;
        let role = self.find_role(&graph.role)?;
        let policy = self.find_policy(
            graph
                .applied_policies
                .first()
                .ok_or_else(|| anyhow!("missing policy"))?,
        )?;
        let packs = graph
            .activated_pack_refs
            .iter()
            .map(|r| {
                self.find_pack(r)
                    .ok_or_else(|| anyhow!("missing pack {}", r.id))
            })
            .collect::<Result<Vec<_>>>()?;
        self.validate_privacy_graph(graph)?;
        let (outputs, _, _) = synthesize_outputs(
            role,
            policy,
            &packs,
            SynthesisContext {
                resolve_graph: graph,
                target: &params.target_capability,
                library_roots: &self.roots,
                apply_mode: &params.apply_mode,
                surface_selection_override: params.surface_selection_mode.clone(),
                auto_surface_selection: graph.auto_surface_selection.as_ref(),
            },
        )?;
        Ok(outputs
            .iter()
            .map(|o| ProjectionProof {
                destination: o.destination_path.clone(),
                staged_path: format!(
                    ".metactl/generated/{}/{}",
                    params.target_capability.target_id, o.destination_path
                ),
                digest: format!("sha256:{}", hex::encode(Sha256::digest(&o.contents))),
                private: private_output(o, graph, &params.target_capability),
            })
            .collect())
    }
}

#[cfg(test)]
#[path = "library_privacy/tests.rs"]
mod tests;
