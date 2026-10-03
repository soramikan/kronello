//! Synchronous MCP transport. Project state and execution policy live in service.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};

use kronello_service::{BackendSelection, Response, Service, ServiceError};
use serde::Deserialize;
use serde_json::{Value, json, value::RawValue};

pub const SUPPORTED_PROTOCOL_VERSIONS: [&str; 2] = ["2025-06-18", "2025-11-25"];
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

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
struct Initialize {
    protocol_version: String,
    capabilities: BTreeMap<String, Value>,
    client_info: ClientInfo,
}
#[derive(Deserialize)]
struct ClientInfo {
    name: String,
    version: String,
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

#[derive(Default)]
enum Lifecycle {
    #[default]
    New,
    AwaitingInitialized,
    Ready,
}
/// Connection state contains only protocol readiness, never a project or session.
pub struct Server {
    lifecycle: Lifecycle,
    service: Service<'static>,
}
impl Server {
    pub fn new(backend: BackendSelection) -> Self {
        Self {
            lifecycle: Lifecycle::New,
            service: Service::new(backend),
        }
    }
    pub fn handle(&mut self, message: &str) -> Option<Value> {
        let raw: Value = match serde_json::from_str(message) {
            Ok(raw) => raw,
            Err(error) => {
                return Some(rpc_error(
                    Value::Null,
                    -32700,
                    error.to_string(),
                    Value::Null,
                ));
            }
        };
        let id = raw
            .get("id")
            .filter(|id| id.is_string() || id.as_i64().is_some() || id.as_u64().is_some())
            .cloned();
        let request: RpcRequest = match serde_json::from_str(message) {
            Ok(request) => request,
            Err(error) => {
                return Some(rpc_error(
                    id.unwrap_or(Value::Null),
                    -32600,
                    error.to_string(),
                    Value::Null,
                ));
            }
        };
        if request.jsonrpc != "2.0"
            || ((request.id.is_some() || raw.get("id").is_some()) && id.is_none())
        {
            return Some(rpc_error(
                Value::Null,
                -32600,
                "Invalid JSON-RPC envelope",
                Value::Null,
            ));
        }
        // Requests without IDs are notifications: they must never execute tools.
        let Some(id) = id else {
            if request.method == "notifications/initialized"
                && matches!(self.lifecycle, Lifecycle::AwaitingInitialized)
            {
                self.lifecycle = Lifecycle::Ready;
            }
            return None;
        };
        let params = request.params.get();
        if request.method == "ping" {
            return Some(result(id, json!({})));
        }
        if request.method == "initialize" {
            if !matches!(self.lifecycle, Lifecycle::New) {
                return Some(invalid_params(id, "Already initialized"));
            }
            let initialize: Initialize = match serde_json::from_str(params) {
                Ok(initialize) => initialize,
                Err(error) => return Some(invalid_params(id, error)),
            };
            if !SUPPORTED_PROTOCOL_VERSIONS.contains(&initialize.protocol_version.as_str()) {
                return Some(rpc_error(
                    id,
                    -32602,
                    "Unsupported protocol version",
                    json!({"code":"UNSUPPORTED_PROTOCOL_VERSION", "supported":SUPPORTED_PROTOCOL_VERSIONS,
                        "requested":initialize.protocol_version}),
                ));
            }
            let _ = (
                initialize.capabilities,
                initialize.client_info.name,
                initialize.client_info.version,
            );
            self.lifecycle = Lifecycle::AwaitingInitialized;
            return Some(result(
                id,
                json!({"protocolVersion":initialize.protocol_version,
                "capabilities":{"tools":{"listChanged":false}},
                "serverInfo":{"name":"kronello-mcp","version":env!("CARGO_PKG_VERSION")}}),
            ));
        }
        if !matches!(self.lifecycle, Lifecycle::Ready) {
            return Some(rpc_error(id, -32002, "Server not initialized", Value::Null));
        }
        Some(match request.method.as_str() {
            "tools/list" => match serde_json::from_str::<ToolList>(params) {
                Ok(list) if list.cursor.is_none() => result(id, tools()),
                Ok(_) => invalid_params(id, "This unpaginated tool list has no cursors"),
                Err(error) => invalid_params(id, error),
            },
            "tools/call" => match serde_json::from_str::<ToolCall>(params) {
                Ok(call) => self.call(id, call),
                Err(error) => invalid_params(id, error),
            },
            _ => rpc_error(id, -32601, "Method not found", Value::Null),
        })
    }
    fn call(&self, id: Value, call: ToolCall) -> Value {
        if !kronello_service::command_registry()
            .iter()
            .any(|command| command.name == call.name)
        {
            return invalid_params(id, format!("Unknown tool: {}", call.name));
        }
        let arguments = call.arguments.get();
        let Some(tail) = arguments.trim_start().strip_prefix('{') else {
            return result(
                id,
                tool_result(Response::Error {
                    error: ServiceError::invalid("Tool arguments must be an object"),
                }),
            );
        };
        let separator = if tail.trim_start().starts_with('}') {
            ""
        } else {
            ","
        };
        // Raw JSON reaches the strict service decoder, preserving duplicate keys
        // and float precision. The caller cannot override the operation tag.
        let tagged = format!(
            "{{\"operation\":{}{separator}{tail}",
            serde_json::to_string(&call.name).expect("tool name serialization")
        );
        result(id, tool_result(self.service.execute_json(&tagged)))
    }
}

/// Newline-delimited UTF-8 JSON-RPC; stdout contains protocol messages only.
pub fn serve(
    mut input: impl BufRead,
    mut output: impl Write,
    backend: BackendSelection,
) -> std::io::Result<()> {
    let mut server = Server::new(backend);
    loop {
        let mut line = Vec::new();
        let size = std::io::Read::take(&mut input, (MAX_MESSAGE_BYTES + 2) as u64)
            .read_until(b'\n', &mut line)?;
        if size == 0 {
            return Ok(());
        }
        if size > MAX_MESSAGE_BYTES + 1 || (size > MAX_MESSAGE_BYTES && line.last() != Some(&b'\n'))
        {
            // Bound allocation and discard the remainder before reading another message.
            if line.last() != Some(&b'\n') {
                loop {
                    let buffer = input.fill_buf()?;
                    if buffer.is_empty() {
                        break;
                    }
                    let newline = buffer.iter().position(|byte| *byte == b'\n');
                    let consumed = newline.map_or(buffer.len(), |position| position + 1);
                    input.consume(consumed);
                    if newline.is_some() {
                        break;
                    }
                }
            }
            let error = rpc_error(Value::Null, -32600, "Message exceeds 16 MiB", Value::Null);
            serde_json::to_writer(&mut output, &error)?;
            output.write_all(b"\n")?;
            output.flush()?;
            continue;
        }
        let response = match std::str::from_utf8(&line) {
            Ok(message) => server.handle(message),
            Err(error) => Some(rpc_error(
                Value::Null,
                -32700,
                error.to_string(),
                Value::Null,
            )),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut output, &response)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
}
