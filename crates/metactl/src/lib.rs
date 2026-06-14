pub mod fixture_kernel;
pub mod jsonrpc;
pub mod kernel;
pub mod library_registry;
pub mod materializer;
pub mod mcp;
pub mod project;
pub mod reference_kernel;
pub mod skill_audit;
pub mod suite_registry;
pub mod types;

pub use fixture_kernel::{FixtureKernel, FixtureSuite};
pub use jsonrpc::{JsonRpcService, RpcError, RpcRequestEnvelope, RpcResponseEnvelope};
pub use kernel::MetactlKernel;
pub use library_registry::LibraryRegistry;
pub use mcp::McpService;
pub use reference_kernel::ReferenceKernel;
pub use skill_audit::{
    ActionPlan, ActionPlanStep, Confidence, HostAdapterMetadata, JoinMethod, Recommendation,
    RecommendationAction, RelationKind, SkillAuditOptions, SkillAuditOutput, SkillAuditScope,
    SkillInventoryItem, SkillPortfolioAuditReport, SkillRelation, SkillReportFormat,
};
pub use suite_registry::{SuiteContext, SuiteRegistry};
pub use types::*;
