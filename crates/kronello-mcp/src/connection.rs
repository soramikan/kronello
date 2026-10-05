//! Per-connection protocol state; no implicit Project and no persistent job state.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kronello_service::{BackendSelection, ExecutionControl, Service};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::sync::{Notify, Semaphore, mpsc};

use crate::{
    Initialize, MAX_MESSAGE_BYTES, RpcRequest, SUPPORTED_PROTOCOL_VERSIONS, ToolCall, ToolList,
    invalid_params, resources, result, rpc_error, tool_result, tools,
};

pub const MAX_ACTIVE_REQUESTS: usize = 32;

#[derive(Default)]
enum Lifecycle {
    #[default]
    New,
    AwaitingInitialized,
    Ready,
    Closed,
}
#[derive(Default)]
struct State {
    lifecycle: Lifecycle,
    version: Option<String>,
    active: BTreeMap<String, Arc<Control>>,
}

struct RequestState {
    cancelled: bool,
    output: Option<mpsc::UnboundedSender<Value>>,
    token: Option<Value>,
    progress: u64,
    last_progress: Instant,
}
struct Control(Mutex<RequestState>, Notify);
impl Control {
    fn cancel(&self) {
        let mut state = self.0.lock().expect("request state");
        state.cancelled = true;
        state.output.take();
        self.1.notify_one();
    }
    fn finish(&self, response: Value) {
        let mut state = self.0.lock().expect("request state");
        if let Some(output) = state.output.take() {
            let _ = output.send(response);
        }
    }
}
impl ExecutionControl for Control {
    fn is_cancelled(&self) -> bool {
        self.0.lock().expect("request state").cancelled
    }
    fn progress(&self, completed: u64, total: u64) {
        let mut state = self.0.lock().expect("request state");
        if completed <= state.progress
            || (completed < total && state.last_progress.elapsed() < Duration::from_millis(100))
        {
            return;
        }
        if let (Some(output), Some(token)) = (&state.output, &state.token) {
            let _ = output.send(json!({"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":token,"progress":completed,"total":total}}));
        }
        state.progress = completed;
        state.last_progress = Instant::now();
    }
}

/// HTTP uses an SSE body for work; stdio forwards this same stream to stdout.
pub enum Reply {
    Immediate(Option<Value>),
    Stream(mpsc::UnboundedReceiver<Value>),
}

pub struct Connection {
    state: Mutex<State>,
    backend: BackendSelection,
    slots: Arc<Semaphore>,
}
impl Connection {
    pub fn new(backend: BackendSelection, slots: Arc<Semaphore>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            backend,
            slots,
        })
    }
    pub fn version(&self) -> Option<String> {
        self.state.lock().expect("connection state").version.clone()
    }
    /// Explicit connection termination cancels request work, never detached jobs.
    pub fn close(&self) {
        let mut state = self.state.lock().expect("connection state");
        state.lifecycle = Lifecycle::Closed;
        for control in state.active.values() {
            control.cancel();
        }
    }
    pub fn submit(self: &Arc<Self>, message: &str) -> Reply {
        let immediate = |value| Reply::Immediate(Some(value));
        if message.len() > MAX_MESSAGE_BYTES {
            return immediate(rpc_error(
                Value::Null,
                -32600,
                "Message exceeds 16 MiB",
                Value::Null,
            ));
        }
        let raw: Value = match serde_json::from_str(message) {
            Ok(raw) => raw,
            Err(error) => {
                return immediate(rpc_error(
                    Value::Null,
                    -32700,
                    error.to_string(),
                    Value::Null,
                ));
            }
        };
        let id = raw.get("id").filter(|id| valid_id(id)).cloned();
        let request: RpcRequest = match serde_json::from_str(message) {
            Ok(request) => request,
            Err(error) => {
                return immediate(rpc_error(
                    id.unwrap_or(Value::Null),
                    -32600,
                    error.to_string(),
                    Value::Null,
                ));
            }
        };
        if request.jsonrpc != "2.0" || (raw.get("id").is_some() && id.is_none()) {
            return immediate(rpc_error(
                Value::Null,
                -32600,
                "Invalid JSON-RPC envelope",
                Value::Null,
            ));
        }
        let _ = &request.id;
        let params = request.params.get();
        let mut state = self.state.lock().expect("connection state");
        let Some(id) = id else {
            if super::strict::validate(message).is_err() {
                return Reply::Immediate(None);
            }
            match request.method.as_str() {
                "notifications/initialized"
                    if matches!(state.lifecycle, Lifecycle::AwaitingInitialized)
                        && serde_json::from_str::<Empty>(params).is_ok() =>
                {
                    state.lifecycle = Lifecycle::Ready;
                }
                "notifications/cancelled" => {
                    if let Ok(cancel) = serde_json::from_str::<Cancellation>(params)
                        && valid_id(&cancel.request_id)
                        && let Some(control) = state.active.get(&cancel.request_id.to_string())
                    {
                        control.cancel();
                    }
                }
                _ => {}
            }
            return Reply::Immediate(None);
        };
        if let Err(error) = super::strict::validate(message) {
            // Keep the existing typed tool error contract for invalid service
            // arguments while rejecting duplicate protocol fields as RPC errors.
            if request.method == "tools/call"
                && let Ok(call) = serde_json::from_str::<ToolCall>(params)
                && super::strict::validate(call.arguments.get()).is_err()
            {
                return immediate(result(
                    id,
                    tool_result(kronello_service::Response::Error {
                        error: kronello_service::ServiceError::invalid(error),
                    }),
                ));
            }
            return immediate(invalid_params(id, error));
        }
        if matches!(state.lifecycle, Lifecycle::Closed) {
            return immediate(rpc_error(id, -32000, "Connection closed", Value::Null));
        }
        if state.active.contains_key(&id.to_string()) {
            return immediate(invalid_params(id, "Request ID is already active"));
        }
        // 2026-07-28 discovery is not implemented; advertise only actual support.
        if request.method == "server/discover" {
            return immediate(rpc_error(
                id,
                -32601,
                "server/discover is unsupported; use initialize",
                json!({"code":"UNSUPPORTED_PROTOCOL_VERSION","supported":SUPPORTED_PROTOCOL_VERSIONS}),
            ));
        }
        if request.method == "ping" {
            return immediate(match serde_json::from_str::<Empty>(params) {
                Ok(_) => result(id, json!({})),
                Err(error) => invalid_params(id, error),
            });
        }
        if request.method == "initialize" {
            if !matches!(state.lifecycle, Lifecycle::New) {
                return immediate(invalid_params(id, "Already initialized"));
            }
            let initialize: Initialize = match serde_json::from_str(params) {
                Ok(init) => init,
                Err(error) => return immediate(invalid_params(id, error)),
            };
            let version =
                if SUPPORTED_PROTOCOL_VERSIONS.contains(&initialize.protocol_version.as_str()) {
                    initialize.protocol_version
                } else {
                    "2025-11-25".into()
                };
            let client = initialize.client_info;
            let _ = (
                initialize.capabilities,
                client.name,
                client.version,
                client.title,
                client.description,
                client.icons,
                client.website_url,
            );
            state.version = Some(version.clone());
            state.lifecycle = Lifecycle::AwaitingInitialized;
            return immediate(result(
                id,
                json!({"protocolVersion":version,"capabilities":{
                "tools":{"listChanged":false},"resources":{"subscribe":false,"listChanged":false},"prompts":{"listChanged":false}
            },"serverInfo":{"name":"kronello-mcp","version":env!("CARGO_PKG_VERSION")}}),
            ));
        }
        if !matches!(state.lifecycle, Lifecycle::Ready) {
            return immediate(rpc_error(id, -32002, "Server not initialized", Value::Null));
        }
        match request.method.as_str() {
            "tools/list" => {
                return immediate(match serde_json::from_str::<ToolList>(params) {
                    Ok(list) if list.cursor.is_none() => result(id, tools()),
                    Ok(_) => invalid_params(id, "This unpaginated tool list has no cursors"),
                    Err(error) => invalid_params(id, error),
                });
            }
            "resources/list" | "resources/templates/list" | "prompts/list" => {
                return immediate(resources::list(&request.method, id, params));
            }
            "tools/call" | "resources/read" | "prompts/get" => {}
            _ => {
                return immediate(rpc_error(
                    id,
                    -32601,
                    "Method not found",
                    json!({"code":"UNSUPPORTED_FEATURE"}),
                ));
            }
        }
        let token = match progress_token(params) {
            Ok(token) => token,
            Err(error) => return immediate(invalid_params(id, error)),
        };
        if let Some(token) = &token
            && state.active.values().any(|control| {
                control.0.lock().expect("request state").token.as_ref() == Some(token)
            })
        {
            return immediate(invalid_params(id, "Progress token is already active"));
        }
        if state.active.len() >= MAX_ACTIVE_REQUESTS {
            return immediate(rpc_error(
                id,
                -32000,
                "Too many active requests",
                json!({"code":"RESOURCE_LIMIT"}),
            ));
        }
        let (output, receiver) = mpsc::unbounded_channel();
        if let Some(token) = &token {
            let _ = output.send(json!({"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":token,"progress":0,"message":"Request accepted"}}));
        }
        let control = Arc::new(Control(
            Mutex::new(RequestState {
                cancelled: false,
                output: Some(output),
                token,
                progress: 0,
                last_progress: Instant::now(),
            }),
            Notify::new(),
        ));
        state.active.insert(id.to_string(), control.clone());
        let connection = self.clone();
        let method = request.method;
        let params = params.to_owned();
        tokio::spawn(async move {
            let permit = tokio::select! {
                permit = connection.slots.clone().acquire_owned() => Some(permit),
                _ = control.1.notified() => None,
            };
            if matches!(&permit, Some(Ok(_))) && !control.is_cancelled() {
                let worker_control = control.clone();
                let worker_id = id.clone();
                let backend = connection.backend;
                let response = tokio::task::spawn_blocking(move || {
                    let service = Service::new(backend);
                    let response = if method == "tools/call" {
                        call(&service, worker_control.as_ref(), worker_id, &params)
                    } else {
                        resources::execute(
                            &service.with_read_only_inspection(),
                            worker_control.as_ref(),
                            &method,
                            worker_id,
                            &params,
                        )
                    };
                    // RenderSequence reports frames through service checkpoints.
                    if worker_control.0.lock().expect("request state").progress == 0 {
                        worker_control.progress(1, 1);
                    }
                    response
                })
                .await
                .unwrap_or_else(|error| {
                    rpc_error(id.clone(), -32603, error.to_string(), Value::Null)
                });
                control.finish(response);
            } else {
                control.cancel();
            }
            connection
                .state
                .lock()
                .expect("connection state")
                .active
                .remove(&id.to_string());
        });
        Reply::Stream(receiver)
    }
}

fn valid_id(id: &Value) -> bool {
    id.is_string() || id.as_i64().is_some() || id.as_u64().is_some()
}
fn progress_token(params: &str) -> Result<Option<Value>, &'static str> {
    let params: Value = serde_json::from_str(params).map_err(|_| "Invalid params")?;
    if let Some(meta) = params.get("_meta") {
        if !meta.is_object() {
            return Err("_meta must be an object");
        }
        if let Some(token) = meta.get("progressToken") {
            if !valid_id(token) {
                return Err("progressToken must be a string or integer");
            }
            return Ok(Some(token.clone()));
        }
    }
    Ok(None)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {
    #[serde(default, rename = "_meta")]
    _meta: Option<Value>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Cancellation {
    request_id: Value,
    #[serde(default, rename = "reason")]
    _reason: Option<String>,
    #[serde(default, rename = "_meta")]
    _meta: Option<Value>,
}

fn call(service: &Service<'_>, control: &dyn ExecutionControl, id: Value, params: &str) -> Value {
    let call: ToolCall = match serde_json::from_str(params) {
        Ok(call) => call,
        Err(error) => return invalid_params(id, error),
    };
    if !kronello_service::command_registry()
        .iter()
        .any(|command| command.name == call.name)
    {
        return invalid_params(id, format!("Unknown tool: {}", call.name));
    }
    let Some(tail) = call.arguments.get().trim_start().strip_prefix('{') else {
        return result(
            id,
            tool_result(kronello_service::Response::Error {
                error: kronello_service::ServiceError::invalid("Tool arguments must be an object"),
            }),
        );
    };
    let separator = if tail.trim_start().starts_with('}') {
        ""
    } else {
        ","
    };
    let tagged = format!(
        "{{\"operation\":{}{separator}{tail}",
        serde_json::to_string(&call.name).expect("tool name")
    );
    result(
        id,
        tool_result(service.execute_json_with_control(&tagged, control)),
    )
}

/// Reads bounded newline messages while work runs on separate blocking workers.
/// EOF closes request work cooperatively; detached jobs retain their own lifetime.
pub async fn serve_stdio(backend: BackendSelection) -> std::io::Result<()> {
    let connection = Connection::new(backend, Arc::new(Semaphore::new(3)));
    let (output, mut responses) = mpsc::unbounded_channel::<Value>();
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(response) = responses.recv().await {
            let mut bytes = serde_json::to_vec(&response)?;
            bytes.push(b'\n');
            stdout.write_all(&bytes).await?;
            stdout.flush().await?;
        }
        Ok::<_, std::io::Error>(())
    });
    let mut input = tokio::io::BufReader::new(tokio::io::stdin());
    let outcome = async {
        loop {
            let mut bytes = Vec::new();
            let mut oversized = false;
            loop {
                let buffer = input.fill_buf().await?;
                if buffer.is_empty() {
                    break;
                }
                let newline = buffer.iter().position(|byte| *byte == b'\n');
                let length = newline.map_or(buffer.len(), |i| i + 1);
                if bytes.len() + length > MAX_MESSAGE_BYTES + 1 {
                    oversized = true;
                } else if !oversized {
                    bytes.extend_from_slice(&buffer[..length]);
                }
                input.consume(length);
                if newline.is_some() {
                    break;
                }
            }
            if bytes.is_empty() && !oversized {
                break;
            }
            if bytes.last() == Some(&b'\n') {
                bytes.pop();
            }
            let reply = if oversized || bytes.len() > MAX_MESSAGE_BYTES {
                Reply::Immediate(Some(rpc_error(
                    Value::Null,
                    -32600,
                    "Message exceeds 16 MiB",
                    Value::Null,
                )))
            } else {
                match std::str::from_utf8(&bytes) {
                    Ok(message) => connection.submit(message),
                    Err(error) => Reply::Immediate(Some(rpc_error(
                        Value::Null,
                        -32700,
                        error.to_string(),
                        Value::Null,
                    ))),
                }
            };
            match reply {
                Reply::Immediate(Some(response)) => {
                    let _ = output.send(response);
                }
                Reply::Immediate(None) => {}
                Reply::Stream(mut stream) => {
                    let output = output.clone();
                    tokio::spawn(async move {
                        while let Some(message) = stream.recv().await {
                            let _ = output.send(message);
                        }
                    });
                }
            }
        }
        Ok::<_, std::io::Error>(())
    }
    .await;
    connection.close();
    drop(output);
    writer.await??;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    #[test]
    fn completion_cancel_race_has_one_terminal_outcome_and_no_late_progress() {
        for _ in 0..64 {
            let (output, mut receiver) = mpsc::unbounded_channel();
            let control = Control(
                Mutex::new(RequestState {
                    cancelled: false,
                    output: Some(output),
                    token: Some(json!("race")),
                    progress: 0,
                    last_progress: Instant::now(),
                }),
                Notify::new(),
            );
            let barrier = Barrier::new(3);
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    barrier.wait();
                    control.cancel();
                });
                scope.spawn(|| {
                    barrier.wait();
                    control.finish(result(json!(1), json!({})));
                });
                barrier.wait();
            });
            control.progress(1, 1);
            let messages: Vec<_> = std::iter::from_fn(|| receiver.try_recv().ok()).collect();
            assert!(messages.len() <= 1);
            if let Some(message) = messages.first() {
                assert_eq!(message["id"], 1);
            }
            assert!(receiver.is_closed());
        }
    }
}
