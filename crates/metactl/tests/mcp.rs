use std::path::PathBuf;

use metactl::{
    ApplyMode, CompileParams, McpService, MetactlKernel, ReferenceKernel, ResolveParams,
    TargetCapabilityMatrix,
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

fn starter_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../library/starter")
}

fn service() -> McpService<ReferenceKernel> {
    let kernel =
        ReferenceKernel::load_from_library_roots(vec![starter_root()]).expect("library kernel");
    McpService::new(kernel)
}

fn builder_config() -> Value {
    json!({
        "api_version": "metactl/v2alpha1",
        "role": {"kind": "role", "id": "builder", "version": "1.0.0"},
        "policy": {"kind": "policy", "id": "brownfield-safe-builder", "version": "1.0.0"},
        "targets": [{"kind": "target", "id": "codex-cli", "version": "2026.03.26"}]
    })
}

fn builder_config_typed() -> metactl::Config {
    serde_json::from_value(builder_config()).expect("typed config")
}

fn codex_target() -> TargetCapabilityMatrix {
    let raw = std::fs::read(starter_root().join("targets/codex-cli.json")).expect("target bytes");
    serde_json::from_slice(&raw).expect("target")
}

fn response_for(raw: Value) -> Value {
    let bytes = serde_json::to_vec(&raw).expect("request bytes");
    let response = service()
        .dispatch_bytes(&bytes)
        .expect("dispatch")
        .expect("request response");
    serde_json::from_slice(&response).expect("response json")
}

#[test]
fn mcp_initialize_advertises_tools_capability() {
    let response = response_for(json!({
        "jsonrpc": "2.0",
        "id": "init-1",
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "metactl-test", "version": "0.0.0"}
        }
    }));

    assert_eq!(response["id"], json!("init-1"));
    assert_eq!(response["result"]["protocolVersion"], json!("2025-11-25"));
    assert_eq!(
        response["result"]["capabilities"]["tools"]["listChanged"],
        json!(false)
    );
    assert_eq!(response["result"]["serverInfo"]["name"], json!("metactl"));
}

#[test]
fn mcp_initialize_negotiates_cursor_protocol_version() {
    let response = response_for(json!({
        "jsonrpc": "2.0",
        "id": "init-cursor",
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {
                "tools": true,
                "prompts": true,
                "resources": true,
                "logging": false,
                "elicitation": {}
            },
            "clientInfo": {"name": "Cursor", "version": "1.0.0"}
        }
    }));

    assert_eq!(response["id"], json!("init-cursor"));
    assert_eq!(response["result"]["protocolVersion"], json!("2025-06-18"));
    assert_eq!(response["result"]["serverInfo"]["name"], json!("metactl"));
}

#[test]
fn mcp_tools_list_exposes_readonly_kernel_tools_only() {
    let response = response_for(json!({
        "jsonrpc": "2.0",
        "id": "tools-1",
        "method": "tools/list",
        "params": {}
    }));

    let tools = response["result"]["tools"].as_array().expect("tools");
    let names = tools
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "metactl_search_packs",
            "metactl_explain",
            "metactl_compile_preview",
            "metactl_validate",
        ]
    );
    assert!(names.iter().all(|name| !name.contains("apply")));
    assert!(names.iter().all(|name| !name.contains("revert")));
    assert_eq!(
        tools[0]["inputSchema"]["properties"]["config"]["type"],
        json!("object")
    );
}

#[test]
fn mcp_tools_call_search_packs_returns_structured_search_result() {
    let response = response_for(json!({
        "jsonrpc": "2.0",
        "id": "call-search",
        "method": "tools/call",
        "params": {
            "name": "metactl_search_packs",
            "arguments": {
                "query": "python refactor",
                "config": builder_config(),
                "limit": 2
            }
        }
    }));

    assert_eq!(response["id"], json!("call-search"));
    assert_eq!(response["result"]["isError"], Value::Null);
    assert_eq!(response["result"]["content"][0]["type"], json!("text"));
    assert_eq!(
        response["result"]["structuredContent"]["matches"][0]["pack_ref"]["id"],
        json!("python-refactor")
    );
}

#[test]
fn mcp_compile_preview_does_not_write_to_caller_project_root() {
    let kernel =
        ReferenceKernel::load_from_library_roots(vec![starter_root()]).expect("library kernel");
    let resolve = kernel
        .resolve(ResolveParams {
            config: builder_config_typed(),
            overlay: None,
            available_targets: vec![codex_target()],
            provenance: None,
        })
        .expect("resolve");
    let project = tempfile::tempdir().expect("project");

    let params = CompileParams {
        resolve_graph: resolve,
        target_capability: codex_target(),
        apply_mode: ApplyMode::Copy,
        surface_selection_mode: None,
        emit_policy_report: false,
        project_root: Some(project.path().to_string_lossy().to_string()),
    };

    let response = response_for(json!({
        "jsonrpc": "2.0",
        "id": "compile-preview",
        "method": "tools/call",
        "params": {
            "name": "metactl_compile_preview",
            "arguments": params
        }
    }));

    assert_eq!(response["id"], json!("compile-preview"));
    assert_eq!(response["error"], Value::Null);
    assert!(!project.path().join(".metactl").exists());
}

#[test]
fn mcp_initialized_notification_emits_no_response() {
    let bytes = serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {}
    }))
    .expect("request bytes");

    let response = service().dispatch_bytes(&bytes).expect("dispatch");
    assert!(response.is_none());
}

#[test]
fn mcp_unknown_tool_returns_protocol_error() {
    let response = response_for(json!({
        "jsonrpc": "2.0",
        "id": "missing-tool",
        "method": "tools/call",
        "params": {
            "name": "metactl_apply",
            "arguments": {}
        }
    }));

    assert_eq!(response["error"]["code"], json!(-32601));
    assert_eq!(response["error"]["message"], json!("method not found"));
}
