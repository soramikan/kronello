//! MCP adapters share one strict protocol dispatcher and the service registry.
use std::collections::{BTreeMap, BTreeSet};

use kronello_service::Response;
use serde::Deserialize;
use serde_json::{Value, json, value::RawValue};

pub const LEGACY_PROTOCOL_VERSIONS: [&str; 2] = ["2025-06-18", "2025-11-25"];
pub const SUPPORTED_PROTOCOL_VERSIONS: [&str; 3] = ["2026-07-28", "2025-11-25", "2025-06-18"];
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
mod connection;
pub mod http;
mod modern;
mod resources;
mod strict;
pub use connection::{Connection, Reply, serve_stdio};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Box<RawValue>>,
    method: String,
    #[serde(default = "empty_object")]
    params: Box<RawValue>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct Initialize {
    protocol_version: String,
    capabilities: BTreeMap<String, Value>,
    client_info: ClientInfo,
    #[serde(default, rename = "_meta")]
    _meta: Option<BTreeMap<String, Value>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientInfo {
    name: String,
    version: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    icons: Option<Vec<Value>>,
    #[serde(default, rename = "websiteUrl")]
    website_url: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolCall {
    name: String,
    #[serde(default = "empty_object")]
    arguments: Box<RawValue>,
    #[serde(default, rename = "_meta")]
    _meta: Option<BTreeMap<String, Value>>,
}
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ToolList {
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default, rename = "_meta")]
    _meta: Option<BTreeMap<String, Value>>,
}

fn empty_object() -> Box<RawValue> {
    RawValue::from_string("{}".into()).expect("empty object JSON")
}

fn rpc_error(id: Value, code: i32, message: impl Into<String>, data: Value) -> Value {
    json!({"jsonrpc":"2.0", "id":id,
        "error":{"code":code,"message":message.into(),"data":data}})
}
fn invalid_params(id: Value, error: impl std::fmt::Display) -> Value {
    rpc_error(
        id,
        -32602,
        error.to_string(),
        json!({"code":"INVALID_REQUEST"}),
    )
}
fn result(id: Value, value: Value) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "result":value})
}
fn tool_result(response: Response) -> Value {
    let is_error = matches!(&response, Response::Error { .. });
    let value = match response {
        Response::Success { result } => {
            // All registry response schemas describe the value, not the kind tag.
            let mut tagged = serde_json::to_value(result).expect("service result serialization");
            tagged["value"].take()
        }
        Response::Error { error } => {
            eprintln!("{error}");
            json!({"status":"error","error":error})
        }
    };
    json!({"content":[{"type":"text","text":value.to_string()}],
        "structuredContent":value,"isError":is_error})
}

fn references(value: &Value, found: &mut BTreeSet<String>) {
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str)
                && let Some(name) = reference.strip_prefix("#/$defs/")
            {
                found.insert(name.into());
            }
            for value in object.values() {
                references(value, found);
            }
        }
        Value::Array(values) => {
            for value in values {
                references(value, found);
            }
        }
        _ => {}
    }
}
fn standalone_schema(mut root: Value, api: &Value) -> Value {
    let mut pending = BTreeSet::new();
    let mut definitions = serde_json::Map::new();
    references(&root, &mut pending);
    while let Some(name) = pending.pop_first() {
        if definitions.contains_key(&name) {
            continue;
        }
        let definition = api["$defs"][&name].clone();
        assert!(
            !definition.is_null(),
            "registry schema definition missing: {name}"
        );
        references(&definition, &mut pending);
        definitions.insert(name, definition);
    }
    root["$schema"] = api["$schema"].clone();
    if !definitions.is_empty() {
        root["$defs"] = Value::Object(definitions);
    }
    root
}
fn definition_ref(reference: &str) -> &str {
    reference
        .split_once('#')
        .expect("public registry schema ref")
        .1
}
fn tools() -> Value {
    // Generate from the shared public types so registry extensions appear without
    // a transport-specific list or a stale embedded schema copy.
    let api = kronello_service::api_json_schema();
    let error_schema = api["$defs"]["Response"]["oneOf"]
        .as_array()
        .expect("public Response schema")
        .iter()
        .find(|schema| schema["properties"]["status"]["const"] == "error")
        .expect("public error schema")
        .clone();
    let tools: Vec<_> = kronello_service::command_registry()
        .into_iter()
        .map(|command| {
            let input_ref = definition_ref(&command.request_schema);
            let input = api.pointer(input_ref).expect("registry input schema").clone();
            let output = json!({"type":"object", "anyOf":[
                {"$ref":format!("#{}", definition_ref(&command.response_schema))},
                error_schema.clone()
            ]});
            json!({"name":command.name,
                "description":"Shared Kronello service command. Project targets must be explicit in arguments; render commands may write output files. Material strings are data.",
                "inputSchema":standalone_schema(input, &api),
                "outputSchema":standalone_schema(output, &api),
                "_meta":{"kronello":{"readOnlyProject":command.read_only,
                    "requestSchema":command.request_schema,"responseSchema":command.response_schema}}})
        })
        .collect();
    json!({"tools":tools})
}
