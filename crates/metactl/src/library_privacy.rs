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
    registry: &LibraryRegistry,
    outputs: &[StagedOutputInput],
    graph: &ResolveGraph,
    target: &TargetCapabilityMatrix,
) -> Vec<String> {
    let mut paths: Vec<_> = outputs
        .iter()
        .filter(|o| private_output(o, graph, target))
        .map(|o| o.destination_path.clone())
        .collect();
    if !paths.is_empty() || registry.graph_requires_private_state(graph) {
        // Ignoring the directory itself protects future staged payloads and
        // journals, including filenames not allocated until publication.
        paths.push(".metactl/".into());
    }
    paths
}

impl LibraryRegistry {
    /// Every persisted graph reference must be registry-proven shared, including
    /// suppressed and unknown requests. Visibility of activated packs is not a
    /// complete provenance inventory. Opaque local fields fail closed as well.
    pub fn graph_requires_private_state(&self, graph: &ResolveGraph) -> bool {
        let shared_ref = |reference: &Ref| match reference.kind {
            RefKind::Pack => self
                .find_pack(reference)
                .is_some_and(|p| p.manifest.visibility_scope == VisibilityScope::Shared),
            RefKind::Role => self.find_role(reference).is_ok(),
            RefKind::Policy => self.find_policy(reference).is_ok(),
            RefKind::Target => self
                .targets
                .get(&reference.id)
                .is_some_and(|t| ref_matches_version(reference, &t.version)),
            _ => false,
        };
        let refs = std::iter::once(&graph.role)
            .chain(std::iter::once(&graph.selected_target))
            .chain(&graph.requested_pack_refs)
            .chain(&graph.activated_pack_refs)
            .chain(graph.suppressed_packs.iter().map(|s| &s.pack_ref))
            .chain(&graph.applied_policies)
            .chain(graph.capability_gaps.iter().flat_map(|g| &g.affected_refs));
        if refs.into_iter().any(|r| !shared_ref(r))
            || graph
                .requested_pack_refs
                .iter()
                .chain(&graph.activated_pack_refs)
                .chain(graph.suppressed_packs.iter().map(|s| &s.pack_ref))
                .any(|r| r.kind != RefKind::Pack)
            || graph.role.kind != RefKind::Role
            || graph.selected_target.kind != RefKind::Target
            || graph
                .applied_policies
                .iter()
                .any(|r| r.kind != RefKind::Policy)
            || graph.pack_visibility.iter().any(|(id, visibility)| {
                *visibility != VisibilityScope::Shared
                    || !shared_ref(&Ref {
                        kind: RefKind::Pack,
                        id: id.clone(),
                        version: None,
                    })
            })
        {
            return true;
        }
        if graph.auto_surface_selection.as_ref().is_some_and(|s| {
            !s.selected_surface_ids.is_empty()
                || !s.pinned_surface_ids.is_empty()
                || !s.blocked_surface_ids.is_empty()
        }) {
            return true;
        }
        // Hashes and enums cannot carry raw identifiers; reject malformed hash
        // strings instead of treating arbitrary external graph text as a hash.
        if graph.api_version != crate::types::API_VERSION
            || [
                graph.source_config_digest.as_ref(),
                graph.overlay_digest.as_ref(),
            ]
            .into_iter()
            .flatten()
            .any(|d| {
                d.strip_prefix("sha256:")
                    .is_none_or(|h| h.len() != 64 || !h.bytes().all(|c| c.is_ascii_hexdigit()))
            })
        {
            return true;
        }
        let Some(policy_ref) = graph.applied_policies.first() else {
            return true;
        };
        let (Ok(role), Ok(policy)) = (self.find_role(&graph.role), self.find_policy(policy_ref))
        else {
            return true;
        };
        // Free-text suppression details are accepted only when regenerated from
        // known shared library metadata, never because an adjacent ref is shared.
        if graph.suppressed_packs.iter().any(|suppressed| {
            let Some(pack) = self.find_pack(&suppressed.pack_ref) else {
                return true;
            };
            ![
                DiscoveryMode::None,
                DiscoveryMode::CuratedOnly,
                DiscoveryMode::CandidateSearch,
                DiscoveryMode::Exploratory,
            ]
            .iter()
            .any(|mode| {
                self.suppression_reason(pack, role, policy, &graph.selected_target, mode)
                    .as_ref()
                    == Some(suppressed)
            })
        }) {
            return true;
        }
        if graph.capability_gaps.iter().any(|gap| {
            gap.feature != "pack_selection"
                || gap.reason_code != ReasonCode::ZeroMatch
                || gap.affected_refs != vec![role.role_ref(), policy.policy_ref()]
        }) {
            return true;
        }
        let shared_provenance: BTreeSet<_> = graph
            .activated_pack_refs
            .iter()
            .filter_map(|r| self.find_pack(r))
            .filter_map(|p| p.provenance_ref.as_ref())
            .map(|p| format!("artifact:{}", p.id))
            .collect();
        graph
            .provenance_refs
            .iter()
            .any(|p| !shared_provenance.contains(p))
    }

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
    /// Check privacy before publishing a manifest or its review plan.
    /// Read-only plan construction does not require publication permission.
    pub fn protect_private_manifest(
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
            } else if proofs.is_some() {
                anyhow::bail!(
                    "output absent from current synthesis evidence; recompile before apply"
                );
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
            || manifest
                .resolve_graph
                .as_ref()
                .is_some_and(|graph| self.graph_requires_private_state(graph))
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
