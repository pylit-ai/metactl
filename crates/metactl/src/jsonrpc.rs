use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::kernel::MetactlKernel;
use crate::types::{CompileParams, ExplainParams, ResolveParams, SearchParams, ValidateParams};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcRequestEnvelope {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcResponseEnvelope {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

pub struct JsonRpcService<K> {
    kernel: K,
}

impl<K: MetactlKernel> JsonRpcService<K> {
    pub fn new(kernel: K) -> Self {
        Self { kernel }
    }

    pub fn dispatch_bytes(&self, raw: &[u8]) -> Result<Vec<u8>> {
        let response = match serde_json::from_slice::<RpcRequestEnvelope>(raw) {
            Ok(request) => self.dispatch(request),
            Err(err) => error_response(
                Value::Null,
                -32700,
                "parse error",
                Some(Value::String(err.to_string())),
            ),
        };
        Ok(serde_json::to_vec(&response)?)
    }

    pub fn dispatch(&self, request: RpcRequestEnvelope) -> RpcResponseEnvelope {
        if request.jsonrpc != "2.0" {
            return error_response(
                request.id,
                -32600,
                "invalid request",
                Some(json!("jsonrpc must be 2.0")),
            );
        }
        let id = request.id.clone();
        let handled: Result<Value> = match request.method.as_str() {
            "metactl.search" => decode_and_call(&request.params, |params: SearchParams| {
                self.kernel.search(params)
            }),
            "metactl.resolve" => decode_and_call(&request.params, |params: ResolveParams| {
                self.kernel.resolve(params)
            }),
            "metactl.explain" => decode_and_call(&request.params, |params: ExplainParams| {
                self.kernel.explain(params)
            }),
            "metactl.compile" => decode_and_call(&request.params, |params: CompileParams| {
                self.kernel.compile(params)
            }),
            "metactl.validate" => decode_and_call(&request.params, |params: ValidateParams| {
                self.kernel.validate(params)
            }),
            other => {
                return error_response(
                    id,
                    -32601,
                    "method not found",
                    Some(Value::String(format!("unknown method {other}"))),
                )
            }
        };
        match handled {
            Ok(result) => RpcResponseEnvelope {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(result),
                error: None,
            },
            Err(err) => {
                let detail = err.to_string();
                if detail.contains("decode") {
                    error_response(id, -32602, "invalid params", Some(Value::String(detail)))
                } else {
                    error_response(id, -32000, "application error", Some(Value::String(detail)))
                }
            }
        }
    }
}

fn decode_and_call<T, U, F>(params: &Value, f: F) -> Result<Value>
where
    T: for<'de> Deserialize<'de>,
    U: Serialize,
    F: FnOnce(T) -> Result<U>,
{
    let decoded = serde_json::from_value::<T>(params.clone()).context("decode params")?;
    let value = f(decoded)?;
    serde_json::to_value(value).map_err(|err| anyhow!(err))
}

fn error_response(id: Value, code: i64, message: &str, data: Option<Value>) -> RpcResponseEnvelope {
    RpcResponseEnvelope {
        jsonrpc: "2.0".to_string(),
        id,
        result: None,
        error: Some(RpcError {
            code,
            message: message.to_string(),
            data,
        }),
    }
}
