use anyhow::Result;

use crate::types::{
    CompileParams, CompileResult, ExplainParams, ExplainResult, ResolveGraph, ResolveParams,
    SearchParams, SearchResult, ValidateParams, ValidationReport,
};

pub trait MetactlKernel: Send + Sync + 'static {
    fn search(&self, params: SearchParams) -> Result<SearchResult>;
    fn resolve(&self, params: ResolveParams) -> Result<ResolveGraph>;
    fn explain(&self, params: ExplainParams) -> Result<ExplainResult>;
    fn compile(&self, params: CompileParams) -> Result<CompileResult>;
    fn validate(&self, params: ValidateParams) -> Result<ValidationReport>;
}
