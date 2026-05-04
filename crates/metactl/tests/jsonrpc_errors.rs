use std::path::PathBuf;

use metactl::{JsonRpcService, ReferenceKernel};
use serde_json::{json, Value};

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/golden")
}

fn response_for(raw: &[u8]) -> Value {
    let kernel = ReferenceKernel::load_from_dir(fixtures_root()).expect("reference kernel");
    let service = JsonRpcService::new(kernel);
    serde_json::from_slice(&service.dispatch_bytes(raw).expect("dispatch bytes"))
        .expect("json response")
}

#[test]
fn malformed_json_returns_parse_error() {
    let response = response_for(br#"{"jsonrpc":"2.0""#);
    assert_eq!(response["jsonrpc"], json!("2.0"));
    assert_eq!(response["id"], Value::Null);
    assert_eq!(response["error"]["code"], json!(-32700));
    assert_eq!(response["error"]["message"], json!("parse error"));
}

#[test]
fn wrong_protocol_version_returns_invalid_request() {
    let response = response_for(
        br#"{"jsonrpc":"1.0","id":"bad-version","method":"metactl.search","params":{"query":"x","config":{"api_version":"metactl/v2alpha1","role":{"kind":"role","id":"reviewer","version":"1.0.0"},"policy":{"kind":"policy","id":"safe-review","version":"1.0.0"},"targets":[{"kind":"target","id":"claude-code","version":"2026.03.25"}]}}}"#,
    );
    assert_eq!(response["id"], json!("bad-version"));
    assert_eq!(response["error"]["code"], json!(-32600));
    assert_eq!(response["error"]["message"], json!("invalid request"));
}

#[test]
fn unknown_method_returns_method_not_found() {
    let response = response_for(
        br#"{"jsonrpc":"2.0","id":"unknown-method","method":"metactl.unknown","params":{}}"#,
    );
    assert_eq!(response["id"], json!("unknown-method"));
    assert_eq!(response["error"]["code"], json!(-32601));
    assert_eq!(response["error"]["message"], json!("method not found"));
}

#[test]
fn invalid_params_return_invalid_params_error() {
    let response = response_for(
        br#"{"jsonrpc":"2.0","id":"bad-params","method":"metactl.search","params":{"query":17,"config":{"api_version":"metactl/v2alpha1","role":{"kind":"role","id":"reviewer","version":"1.0.0"},"policy":{"kind":"policy","id":"safe-review","version":"1.0.0"},"targets":[{"kind":"target","id":"claude-code","version":"2026.03.25"}]}}}"#,
    );
    assert_eq!(response["id"], json!("bad-params"));
    assert_eq!(response["error"]["code"], json!(-32602));
    assert_eq!(response["error"]["message"], json!("invalid params"));
}
