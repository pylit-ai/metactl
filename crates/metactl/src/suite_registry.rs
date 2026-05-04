use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

use crate::types::{
    CompileManifest, Config, EnforcementStatus, ExplainResult, InvocationOverlay, PackManifest,
    PolicyEnforcementReport, PolicyManifest, ProvenanceEnvelope, Ref, ResolveGraph, RoleManifest,
    SearchResult, TargetCapabilityMatrix, ValidateParams, ValidationCheck, ValidationReport,
    ValidationStatus,
};

#[derive(Debug, Clone)]
pub struct SuiteContext {
    pub name: String,
    pub root: PathBuf,
    pub repo_root: PathBuf,
    pub config: Config,
    pub role_manifest: RoleManifest,
    pub policy_manifest: PolicyManifest,
    pub target_capability: TargetCapabilityMatrix,
    pub packs: Vec<PackManifest>,
    pub provenance: Vec<ProvenanceEnvelope>,
    pub search_result: SearchResult,
    pub resolve_graph: ResolveGraph,
    pub explain_result: ExplainResult,
    pub compile_manifest: CompileManifest,
    pub policy_enforcement_report: PolicyEnforcementReport,
    pub validation_report: ValidationReport,
}

impl SuiteContext {
    pub fn selected_target(&self) -> Ref {
        self.target_capability.target_ref()
    }

    pub fn policy_ref(&self) -> Ref {
        self.policy_manifest.policy_ref()
    }

    pub fn role_ref(&self) -> Ref {
        self.role_manifest.role_ref()
    }

    pub fn matches_config(&self, config: &Config, overlay: Option<&InvocationOverlay>) -> bool {
        let target = selected_target_from_config(config, overlay);
        self.role_ref() == config.role
            && self.policy_ref() == config.policy
            && Some(self.selected_target()) == target
    }

    pub fn matches_graph(&self, graph: &ResolveGraph) -> bool {
        self.role_ref() == graph.role && self.selected_target() == graph.selected_target
    }

    pub fn materialize_compile_manifest(&self) -> Result<CompileManifest> {
        let mut manifest = self.compile_manifest.clone();
        for output in &mut manifest.generated_outputs {
            let path = self.repo_root.join(&output.path);
            if !path.exists() {
                return Err(anyhow!("missing generated output {}", output.path));
            }
            output.digest = Some(sha256_digest(&path)?);
        }
        Ok(manifest)
    }

    pub fn validation_report_for(
        &self,
        compile_manifest: &CompileManifest,
        policy_report: Option<&PolicyEnforcementReport>,
    ) -> Result<ValidationReport> {
        let mut report = self.validation_report.clone();
        let digest_ok = compile_manifest.generated_outputs.iter().all(|output| {
            let expected = output.digest.as_deref().unwrap_or_default();
            let path = self.repo_root.join(&output.path);
            path.exists()
                && sha256_digest(&path)
                    .map(|actual| actual == expected)
                    .unwrap_or(false)
        });

        if !digest_ok {
            report.status = ValidationStatus::Fail;
            upsert_check(
                &mut report,
                ValidationCheck {
                    id: "generated-output-digests".to_string(),
                    status: ValidationStatus::Fail,
                    message: "Generated outputs do not match recorded digests.".to_string(),
                    artifact_ref: None,
                },
            );
            return Ok(report);
        }

        if let Some(policy_report) = policy_report {
            let has_degraded = policy_report
                .rules
                .iter()
                .any(|rule| rule.status == EnforcementStatus::Degraded);
            if has_degraded && report.status == ValidationStatus::Pass {
                report.status = ValidationStatus::Warn;
            }
        }

        Ok(report)
    }
}

#[derive(Debug, Clone)]
pub struct SuiteRegistry {
    suites: Vec<SuiteContext>,
}

impl SuiteRegistry {
    pub fn load_from_dir(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        let repo_root = root
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| anyhow!("unable to infer repo root from {}", root.display()))?
            .to_path_buf();
        let mut suites = Vec::new();
        for entry in
            fs::read_dir(root).with_context(|| format!("read fixtures root {}", root.display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            suites.push(Self::load_suite(&repo_root, &entry.path())?);
        }
        if suites.is_empty() {
            return Err(anyhow!("no suite directories found in {}", root.display()));
        }
        Ok(Self { suites })
    }

    pub fn suite_names(&self) -> Vec<String> {
        self.suites.iter().map(|suite| suite.name.clone()).collect()
    }

    pub fn find_by_config(
        &self,
        config: &Config,
        overlay: Option<&InvocationOverlay>,
    ) -> Result<&SuiteContext> {
        self.suites
            .iter()
            .find(|suite| suite.matches_config(config, overlay))
            .ok_or_else(|| {
                anyhow!(
                    "no suite matched role={} target={}",
                    config.role.id,
                    selected_target_from_config(config, overlay)
                        .map(|r| r.id)
                        .unwrap_or_else(|| "<none>".to_string())
                )
            })
    }

    pub fn find_by_graph(&self, graph: &ResolveGraph) -> Result<&SuiteContext> {
        self.suites
            .iter()
            .find(|suite| suite.matches_graph(graph))
            .ok_or_else(|| {
                anyhow!(
                    "no suite matched resolve graph role={} target={}",
                    graph.role.id,
                    graph.selected_target.id
                )
            })
    }

    pub fn find_for_validate(&self, params: &ValidateParams) -> Result<&SuiteContext> {
        if let Some(graph) = params.resolve_graph.as_ref() {
            return self.find_by_graph(graph);
        }
        let target = params
            .compile_manifest
            .as_ref()
            .map(|manifest| manifest.target.clone())
            .unwrap_or_else(|| params.subject_ref.clone());
        self.suites
            .iter()
            .find(|suite| suite.selected_target() == target)
            .ok_or_else(|| anyhow!("no suite matched validation subject {}", target.id))
    }

    fn load_suite(repo_root: &Path, dir: &Path) -> Result<SuiteContext> {
        let name = dir
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown")
            .to_string();
        let config = load_json(dir.join("config.json"))?;
        let role_manifest = load_json(dir.join("role.manifest.json"))?;
        let policy_manifest = load_json(dir.join("policy.manifest.json"))?;
        let target_capability = load_json(dir.join("target.capability.json"))?;
        let search_result = load_json(dir.join("search.result.json"))?;
        let resolve_graph = load_json(dir.join("resolve.graph.json"))?;
        let explain_result = load_json(dir.join("explain.result.json"))?;
        let compile_manifest = load_json(dir.join("compile.manifest.json"))?;
        let policy_enforcement_report = load_json(dir.join("policy.enforcement.report.json"))?;
        let validation_report = load_json(dir.join("validation.report.json"))?;
        let provenance = load_json(dir.join("provenance.bundle.json"))?;

        let mut packs = Vec::new();
        for path in sorted_glob(dir, "pack.*.json")? {
            packs.push(load_json(path)?);
        }

        Ok(SuiteContext {
            name,
            root: dir.to_path_buf(),
            repo_root: repo_root.to_path_buf(),
            config,
            role_manifest,
            policy_manifest,
            target_capability,
            packs,
            provenance,
            search_result,
            resolve_graph,
            explain_result,
            compile_manifest,
            policy_enforcement_report,
            validation_report,
        })
    }
}

pub fn selected_target_from_config(
    config: &Config,
    overlay: Option<&InvocationOverlay>,
) -> Option<Ref> {
    overlay
        .and_then(|item| item.selected_target_override.clone())
        .or_else(|| config.targets.first().cloned())
}

fn load_json<T: DeserializeOwned>(path: PathBuf) -> Result<T> {
    let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("decode {}", path.display()))
}

fn sorted_glob(dir: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    let mut entries = dir
        .read_dir()
        .with_context(|| format!("read {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|item| item.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| glob_match(pattern, value))
        })
        .collect::<Vec<_>>();
    entries.sort();
    Ok(entries)
}

fn glob_match(pattern: &str, value: &str) -> bool {
    let needle = pattern
        .strip_prefix("pack.")
        .and_then(|rest| rest.strip_suffix(".json"));
    if let Some(inner) = needle {
        value.starts_with("pack.")
            && value.ends_with(".json")
            && (inner == "*"
                || !value
                    .trim_start_matches("pack.")
                    .trim_end_matches(".json")
                    .is_empty())
    } else {
        false
    }
}

fn sha256_digest(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let digest = Sha256::digest(bytes);
    Ok(format!("sha256:{}", hex::encode(digest)))
}

fn upsert_check(report: &mut ValidationReport, check: ValidationCheck) {
    if let Some(existing) = report.checks.iter_mut().find(|item| item.id == check.id) {
        *existing = check;
    } else {
        report.checks.push(check);
    }
}
