use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::kernel::MetactlKernel;
use crate::types::{CompileParams, ExplainParams, SearchParams, ValidateParams};

const LATEST_MCP_PROTOCOL_VERSION: &str = "2025-11-25";
const SUPPORTED_MCP_PROTOCOL_VERSIONS: &[&str] = &[LATEST_MCP_PROTOCOL_VERSION, "2025-06-18"];
const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_TOOL_TEXT_BYTES: usize = 32 * 1024;

#[derive(Debug, Deserialize)]
struct McpRequestEnvelope {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize)]
struct CallToolParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

#[derive(Debug, Deserialize)]
struct InitializeParams {
    #[serde(rename = "protocolVersion")]
    protocol_version: Option<String>,
}

pub struct McpService<K> {
    kernel: K,
}

impl<K: MetactlKernel> McpService<K> {
    pub fn new(kernel: K) -> Self {
        Self { kernel }
    }

    pub fn dispatch_bytes(&self, raw: &[u8]) -> Result<Option<Vec<u8>>> {
        if raw.len() > MAX_REQUEST_BYTES {
            return Ok(Some(serde_json::to_vec(&error_response(
                Value::Null,
                -32600,
                "invalid request",
                Some(json!("request too large")),
            ))?));
        }
        let request = match serde_json::from_slice::<McpRequestEnvelope>(raw) {
            Ok(request) => request,
            Err(err) => {
                return Ok(Some(serde_json::to_vec(&error_response(
                    Value::Null,
                    -32700,
                    "parse error",
                    Some(Value::String(err.to_string())),
                ))?));
            }
        };
        match self.dispatch(request)? {
            Some(response) => Ok(Some(serde_json::to_vec(&response)?)),
            None => Ok(None),
        }
    }

    fn dispatch(&self, request: McpRequestEnvelope) -> Result<Option<Value>> {
        if request.jsonrpc != "2.0" {
            return Ok(response_or_none(
                request.id,
                error_response(
                    Value::Null,
                    -32600,
                    "invalid request",
                    Some(json!("jsonrpc must be 2.0")),
                ),
            ));
        }

        let Some(id) = request.id.clone() else {
            return Ok(None);
        };

        let handled = match request.method.as_str() {
            "initialize" => initialize_result(request.params),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": readonly_tools() })),
            "tools/call" => self.call_tool(request.params),
            other => Err(anyhow!("unknown method {other}")),
        };

        Ok(Some(match handled {
            Ok(result) => success_response(id, result),
            Err(err) => {
                let detail = redact_secrets(&err.to_string());
                if detail.contains("decode")
                    || detail.contains("path traversal")
                    || detail.contains("invalid URI")
                {
                    error_response(id, -32602, "invalid params", Some(Value::String(detail)))
                } else {
                    error_response(id, -32601, "method not found", Some(Value::String(detail)))
                }
            }
        }))
    }

    fn call_tool(&self, params: Value) -> Result<Value> {
        let params: CallToolParams = serde_json::from_value(params).context("decode tool call")?;
        reject_unsafe_argument_strings(&params.arguments)?;
        match params.name.as_str() {
            "metactl_search_packs" => {
                let args: SearchParams =
                    serde_json::from_value(params.arguments).context("decode search params")?;
                Ok(tool_success(serde_json::to_value(
                    self.kernel.search(args)?,
                )?))
            }
            "metactl_explain" => {
                let args: ExplainParams =
                    serde_json::from_value(params.arguments).context("decode explain params")?;
                Ok(tool_success(serde_json::to_value(
                    self.kernel.explain(args)?,
                )?))
            }
            "metactl_compile_preview" => {
                let mut args: CompileParams =
                    serde_json::from_value(params.arguments).context("decode compile params")?;
                let scratch =
                    tempfile::tempdir().context("create compile preview scratch directory")?;
                args.project_root = Some(scratch.path().to_string_lossy().to_string());
                args.durable_staging = false;
                Ok(tool_success(serde_json::to_value(
                    self.kernel.compile(args)?,
                )?))
            }
            "metactl_validate" => {
                let args: ValidateParams =
                    serde_json::from_value(params.arguments).context("decode validate params")?;
                Ok(tool_success(serde_json::to_value(
                    self.kernel.validate(args)?,
                )?))
            }
            other => Err(anyhow!("unknown tool {other}")),
        }
    }
}

fn reject_unsafe_argument_strings(value: &Value) -> Result<()> {
    match value {
        Value::String(text) => {
            let normalized = text.replace('\\', "/");
            if normalized.contains("../") || normalized.starts_with("../") {
                return Err(anyhow!("path traversal is not allowed in MCP arguments"));
            }
            if normalized.starts_with("file:") {
                return Err(anyhow!("invalid URI is not allowed in MCP arguments"));
            }
        }
        Value::Array(items) => {
            for item in items {
                reject_unsafe_argument_strings(item)?;
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                reject_unsafe_argument_strings(item)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn redact_secrets(input: &str) -> String {
    let mut output = input.to_string();
    for prefix in ["sk_", "ghp_", "pat_", "xoxb-"] {
        while let Some(start) = output.find(prefix) {
            let end = output[start..]
                .char_indices()
                .find_map(|(idx, ch)| {
                    (!matches!(ch, 'A'..='Z' | 'a'..='z' | '0'..='9' | '_' | '-' | '='))
                        .then_some(start + idx)
                })
                .unwrap_or(output.len());
            output.replace_range(start..end, "[REDACTED]");
        }
    }
    output
}

fn response_or_none(id: Option<Value>, mut response: Value) -> Option<Value> {
    id.map(|id| {
        response["id"] = id;
        response
    })
}

fn initialize_result(params: Value) -> Result<Value> {
    let params: InitializeParams = serde_json::from_value(params).context("decode initialize")?;
    let protocol_version = params
        .protocol_version
        .filter(|requested| SUPPORTED_MCP_PROTOCOL_VERSIONS.contains(&requested.as_str()))
        .unwrap_or_else(|| LATEST_MCP_PROTOCOL_VERSION.to_string());

    Ok(json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": {
                "listChanged": false
            }
        },
        "serverInfo": {
            "name": "metactl",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": "Read-only metactl kernel adapter. Use search, explain, compile preview, and validate tools for local pack discovery and diagnostics. Mutating apply/revert behavior is intentionally not exposed."
    }))
}

fn readonly_tools() -> Vec<Value> {
    vec![
        tool(
            "metactl_search_packs",
            "Search metactl packs",
            "Search the configured metactl pack library. Arguments match metactl.search params.",
            search_input_schema(),
        ),
        tool(
            "metactl_explain",
            "Explain metactl resolution",
            "Explain an already resolved metactl graph. Arguments match metactl.explain params.",
            json!({
                "type": "object",
                "properties": {
                    "resolve_graph": {"type": "object"}
                },
                "required": ["resolve_graph"]
            }),
        ),
        tool(
            "metactl_compile_preview",
            "Preview metactl compile output",
            "Compile staged outputs in an ephemeral scratch directory. This ignores project_root and does not apply, revert, or write caller project files. Arguments otherwise match metactl.compile params.",
            json!({
                "type": "object",
                "properties": {
                    "resolve_graph": {"type": "object"},
                    "target_capability": {"type": "object"},
                    "apply_mode": {"type": "string"},
                    "surface_selection_mode": {"type": "string"},
                    "emit_policy_report": {"type": "boolean"}
                },
                "required": ["resolve_graph", "target_capability", "apply_mode"]
            }),
        ),
        tool(
            "metactl_validate",
            "Validate metactl artifacts",
            "Validate kernel artifacts. Arguments match metactl.validate params.",
            json!({
                "type": "object",
                "properties": {
                    "subject_ref": {"type": "object"},
                    "resolve_graph": {"type": "object"},
                    "compile_manifest": {"type": "object"},
                    "policy_enforcement_report": {"type": "object"},
                    "project_root": {"type": "string"}
                },
                "required": ["subject_ref"]
            }),
        ),
    ]
}

fn tool(name: &str, title: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": input_schema,
        "annotations": {
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true
        }
    })
}

fn search_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {"type": "string"},
            "config": {"type": "object"},
            "overlay": {"type": "object"},
            "candidate_packs": {
                "type": "array",
                "items": {"type": "object"}
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 50
            }
        },
        "required": ["query", "config"]
    })
}

fn tool_success(value: Value) -> Value {
    let mut text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    if text.len() > MAX_TOOL_TEXT_BYTES {
        text.truncate(MAX_TOOL_TEXT_BYTES);
        text.push_str("\n[truncated]");
    }
    json!({
        "content": [
            {
                "type": "text",
                "text": text
            }
        ],
        "structuredContent": value
    })
}

fn success_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn error_response(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
            "data": data
        }
    })
}
