use std::fs;
use std::path::PathBuf;

use metactl::{JsonRpcService, ReferenceKernel};
use pretty_assertions::assert_eq;
use serde_json::Value;

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/golden")
}

#[test]
fn reference_kernel_loads_both_suites() {
    let kernel = ReferenceKernel::load_from_dir(fixtures_root()).expect("reference kernel");
    let mut suites = kernel.suite_names();
    suites.sort();
    assert_eq!(
        suites,
        vec![
            "brownfield-codex-cli".to_string(),
            "greenfield-claude-code".to_string()
        ]
    );
}

#[test]
fn jsonrpc_golden_requests_round_trip() {
    let kernel = ReferenceKernel::load_from_dir(fixtures_root()).expect("reference kernel");
    let service = JsonRpcService::new(kernel);
    for suite in ["greenfield-claude-code", "brownfield-codex-cli"] {
        for method in ["search", "resolve", "explain", "compile", "validate"] {
            let req_path = fixtures_root()
                .join(suite)
                .join("jsonrpc")
                .join(format!("{method}.request.json"));
            let expected_path = fixtures_root()
                .join(suite)
                .join("jsonrpc")
                .join(format!("{method}.response.json"));
            let req = fs::read(&req_path).expect("read request");
            let actual: Value =
                serde_json::from_slice(&service.dispatch_bytes(&req).expect("dispatch"))
                    .expect("actual json");
            let expected: Value =
                serde_json::from_slice(&fs::read(&expected_path).expect("read expected"))
                    .expect("expected json");
            assert_eq!(actual, expected, "suite={suite} method={method}");
        }
    }
}
