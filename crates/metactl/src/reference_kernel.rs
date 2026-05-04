use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

use anyhow::{anyhow, Result};

use crate::kernel::MetactlKernel;
use crate::library_registry::LibraryRegistry;
use crate::suite_registry::{selected_target_from_config, SuiteContext, SuiteRegistry};
use crate::types::{
    ApplyMode, ApplyReport, CompileManifest, CompileParams, CompileResult, ExplainParams,
    ExplainResult, Ref, ResolveGraph, ResolveParams, RevertReport, SearchParams, SearchResult,
    TargetCapabilityMatrix, ValidateParams, ValidationReport,
};

#[derive(Debug, Clone)]
pub struct ReferenceKernel {
    backend: KernelBackend,
}

#[derive(Debug, Clone)]
enum KernelBackend {
    Suites(SuiteRegistry),
    Libraries(LibraryRegistry),
}

impl ReferenceKernel {
    pub fn load_from_dir(root: impl AsRef<Path>) -> Result<Self> {
        let registry = SuiteRegistry::load_from_dir(root)?;
        Ok(Self {
            backend: KernelBackend::Suites(registry),
        })
    }

    pub fn load_from_library_roots(roots: Vec<PathBuf>) -> Result<Self> {
        let registry = LibraryRegistry::load_from_roots(&roots)?;
        Ok(Self {
            backend: KernelBackend::Libraries(registry),
        })
    }

    pub fn suite_names(&self) -> Vec<String> {
        match &self.backend {
            KernelBackend::Suites(registry) => registry.suite_names(),
            KernelBackend::Libraries(registry) => registry.root_names(),
        }
    }

    pub fn apply_compiled_outputs(
        &self,
        project_root: impl AsRef<Path>,
        manifest: &CompileManifest,
        apply_mode: &ApplyMode,
    ) -> Result<ApplyReport> {
        match &self.backend {
            KernelBackend::Libraries(registry) => {
                registry.apply_manifest(project_root.as_ref(), manifest, apply_mode)
            }
            KernelBackend::Suites(_) => Err(anyhow!(
                "apply is not implemented for suite-backed fixtures"
            )),
        }
    }

    pub fn revert_target(
        &self,
        project_root: impl AsRef<Path>,
        target: &Ref,
    ) -> Result<RevertReport> {
        match &self.backend {
            KernelBackend::Libraries(registry) => {
                registry.revert_target(project_root.as_ref(), target)
            }
            KernelBackend::Suites(_) => Err(anyhow!(
                "revert is not implemented for suite-backed fixtures"
            )),
        }
    }

    pub fn detect_drift(
        &self,
        project_root: impl AsRef<Path>,
        target: &Ref,
    ) -> Result<ValidationReport> {
        match &self.backend {
            KernelBackend::Libraries(registry) => {
                registry.detect_drift(project_root.as_ref(), target)
            }
            KernelBackend::Suites(_) => Err(anyhow!(
                "drift detection is not implemented for suite-backed fixtures"
            )),
        }
    }

    fn search_result_for_suite(&self, suite: &SuiteContext, params: SearchParams) -> SearchResult {
        let mut result = suite.search_result.clone();
        result.query = params.query;
        if let Some(mode) = params
            .config
            .defaults
            .as_ref()
            .and_then(|defaults| defaults.discovery_mode.clone())
        {
            result.discovery_mode = mode;
        }

        let allowed_pack_refs = params
            .candidate_packs
            .iter()
            .map(|pack| pack.pack_ref().key())
            .collect::<BTreeSet<_>>();
        if !allowed_pack_refs.is_empty() {
            result
                .matches
                .retain(|item| allowed_pack_refs.contains(&item.pack_ref.key()));
            result
                .suppressed
                .retain(|item| allowed_pack_refs.contains(&item.pack_ref.key()));
        }
        if let Some(limit) = params.limit {
            result.matches.truncate(limit as usize);
        }
        result
    }

    fn resolve_graph_for_suite(
        &self,
        suite: &SuiteContext,
        params: &ResolveParams,
    ) -> Result<ResolveGraph> {
        let selected_target = selected_target_from_config(&params.config, params.overlay.as_ref())
            .ok_or_else(|| anyhow!("resolve request does not include a selected target"))?;
        let target_present = params
            .available_targets
            .iter()
            .any(|target| target.target_ref() == selected_target);
        if !target_present {
            return Err(anyhow!(
                "selected target {} is not available",
                selected_target.id
            ));
        }
        Ok(suite.resolve_graph.clone())
    }

    fn explain_result_for_suite(
        &self,
        suite: &SuiteContext,
        params: ExplainParams,
    ) -> ExplainResult {
        let mut result = suite.explain_result.clone();
        result.resolve_graph = params.resolve_graph;
        result
    }

    fn compile_manifest_for_suite(
        &self,
        suite: &SuiteContext,
        target_capability: &TargetCapabilityMatrix,
        apply_mode: &crate::types::ApplyMode,
    ) -> Result<CompileResult> {
        if suite.selected_target() != target_capability.target_ref() {
            return Err(anyhow!(
                "compile target mismatch: suite={} request={}",
                suite.selected_target().id,
                target_capability.target_id
            ));
        }

        let manifest = suite.materialize_compile_manifest()?;
        if !manifest.apply_modes_supported.contains(apply_mode) {
            return Err(anyhow!(
                "apply mode is not supported for {}",
                target_capability.target_id
            ));
        }

        Ok(CompileResult {
            compile_manifest: manifest,
            policy_enforcement_report: Some(suite.policy_enforcement_report.clone()),
        })
    }

    fn validation_report_for_suite(
        &self,
        suite: &SuiteContext,
        params: ValidateParams,
    ) -> Result<ValidationReport> {
        let compile_manifest = params
            .compile_manifest
            .unwrap_or_else(|| suite.compile_manifest.clone());
        suite.validation_report_for(&compile_manifest, params.policy_enforcement_report.as_ref())
    }
}

impl MetactlKernel for ReferenceKernel {
    fn search(&self, params: SearchParams) -> Result<SearchResult> {
        match &self.backend {
            KernelBackend::Suites(registry) => {
                let suite = registry.find_by_config(&params.config, params.overlay.as_ref())?;
                Ok(self.search_result_for_suite(suite, params))
            }
            KernelBackend::Libraries(registry) => registry.search(params),
        }
    }

    fn resolve(&self, params: ResolveParams) -> Result<ResolveGraph> {
        match &self.backend {
            KernelBackend::Suites(registry) => {
                let suite = registry.find_by_config(&params.config, params.overlay.as_ref())?;
                self.resolve_graph_for_suite(suite, &params)
            }
            KernelBackend::Libraries(registry) => registry.resolve(params),
        }
    }

    fn explain(&self, params: ExplainParams) -> Result<ExplainResult> {
        match &self.backend {
            KernelBackend::Suites(registry) => {
                let suite = registry.find_by_graph(&params.resolve_graph)?;
                Ok(self.explain_result_for_suite(suite, params))
            }
            KernelBackend::Libraries(registry) => Ok(registry.explain(params)),
        }
    }

    fn compile(&self, params: CompileParams) -> Result<CompileResult> {
        match &self.backend {
            KernelBackend::Suites(registry) => {
                let suite = registry.find_by_graph(&params.resolve_graph)?;
                let mut result = self.compile_manifest_for_suite(
                    suite,
                    &params.target_capability,
                    &params.apply_mode,
                )?;
                if !params.emit_policy_report {
                    result.policy_enforcement_report = None;
                }
                Ok(result)
            }
            KernelBackend::Libraries(registry) => registry.compile(params),
        }
    }

    fn validate(&self, params: ValidateParams) -> Result<ValidationReport> {
        match &self.backend {
            KernelBackend::Suites(registry) => {
                let suite = registry.find_for_validate(&params)?;
                self.validation_report_for_suite(suite, params)
            }
            KernelBackend::Libraries(registry) => registry.validate(params),
        }
    }
}
