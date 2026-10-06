//! MCP 2026-07-28 per-request contracts; legacy lifecycle remains independent.
use crate::{SUPPORTED_PROTOCOL_VERSIONS, invalid_params, rpc_error};
use serde_json::{Value, json};
pub const VERSION: &str = "2026-07-28";
pub const VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";
pub const CAPABILITIES_KEY: &str = "io.modelcontextprotocol/clientCapabilities";

pub fn unsupported(id: Value, requested: &str) -> Value {
    rpc_error(
        id,
        -32022,
        "Unsupported protocol version",
        json!({"supported":SUPPORTED_PROTOCOL_VERSIONS,"requested":requested}),
    )
}
pub fn validate(message: &Value) -> Result<bool, Value> {
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let meta = &message["params"]["_meta"];
    let Some(version) = meta.get(VERSION_KEY) else {
        return Ok(false);
    };
    let Some(version) = version.as_str() else {
        return Err(invalid_params(
            id,
            "protocolVersion metadata must be a string",
        ));
    };
    if version != VERSION {
        return Err(unsupported(id, version));
    }
    if !meta[CAPABILITIES_KEY].is_object() {
        return Err(invalid_params(
            id,
            "clientCapabilities metadata must be an object on every modern request",
        ));
    }
    if let Some(info) = meta.get("io.modelcontextprotocol/clientInfo")
        && (!info.is_object() || !info["name"].is_string() || !info["version"].is_string())
    {
        return Err(invalid_params(
            id,
            "clientInfo metadata must identify name and version",
        ));
    }
    Ok(true)
}
pub fn complete(mut response: Value, method: &str) -> Value {
    if let Some(result) = response.get_mut("result").and_then(Value::as_object_mut) {
        result.insert("resultType".into(), json!("complete"));
        result.insert("_meta".into(), json!({"io.modelcontextprotocol/serverInfo":{"name":"kronello-mcp","version":env!("CARGO_PKG_VERSION")}}));
        if matches!(
            method,
            "server/discover"
                | "tools/list"
                | "resources/list"
                | "resources/templates/list"
                | "resources/read"
                | "prompts/list"
        ) {
            result.insert("ttlMs".into(), json!(0));
            result.insert("cacheScope".into(), json!("private"));
        }
    }
    if response["error"]["code"] == -32002 {
        response["error"]["code"] = json!(-32602);
    }
    response
}
pub fn discover(id: Value) -> Value {
    complete(
        crate::result(
            id,
            json!({"supportedVersions":SUPPORTED_PROTOCOL_VERSIONS,"capabilities":{"tools":{},"resources":{},"prompts":{}}}),
        ),
        "server/discover",
    )
}
