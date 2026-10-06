//! Streamable HTTP (2025-11-25), with bounded process-local protocol sessions.
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::to_bytes;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response, Sse, sse::Event};
use axum::{Json, Router, routing::any};
use futures_util::stream;
use kronello_service::BackendSelection;
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use tokio::sync::Semaphore;

#[cfg(test)]
use crate::SUPPORTED_PROTOCOL_VERSIONS;
use crate::connection::{Connection, Reply};
use crate::{LEGACY_PROTOCOL_VERSIONS, MAX_MESSAGE_BYTES, rpc_error};

pub const MAX_SESSIONS: usize = 64;
pub const SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Clone)]
pub struct HttpConfig {
    pub bind: SocketAddr,
    pub bearer_token: Option<String>,
}
impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8765),
            bearer_token: None,
        }
    }
}
impl HttpConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !self.bind.ip().is_loopback() && self.bearer_token.is_none() {
            return Err("Non-loopback binding requires --auth-token-env".into());
        }
        if let Some(token) = &self.bearer_token
            && (token.len() < 32 || !token.bytes().all(|byte| (0x21..=0x7e).contains(&byte)))
        {
            return Err("Bearer token must contain at least 32 visible ASCII bytes".into());
        }
        Ok(())
    }
}
struct Session {
    connection: Arc<Connection>,
    touched: Instant,
}
struct HttpState {
    config: HttpConfig,
    backend: BackendSelection,
    slots: Arc<Semaphore>,
    modern_slots: Arc<Semaphore>,
    sessions: Mutex<BTreeMap<String, Session>>,
}
impl HttpState {
    fn close(&self) {
        for session in self.sessions.lock().expect("HTTP sessions").values() {
            session.connection.close();
        }
    }
}
/// Bind configuration is validated before opening a socket. Diagnostic only.
pub async fn serve_http(config: HttpConfig, backend: BackendSelection) -> std::io::Result<()> {
    config.validate().map_err(std::io::Error::other)?;
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    let mut config = config;
    config.bind = listener.local_addr()?;
    eprintln!("MCP_HTTP_LISTENING http://{}/mcp", config.bind);
    let state = Arc::new(HttpState {
        config,
        backend,
        slots: Arc::new(Semaphore::new(3)),
        modern_slots: Arc::new(Semaphore::new(crate::connection::MAX_ACTIVE_REQUESTS)),
        sessions: Mutex::new(BTreeMap::new()),
    });
    let app = Router::new()
        .route("/mcp", any(endpoint))
        .with_state(state.clone());
    let shutdown_state = state.clone();
    let outcome = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            shutdown_state.close();
        })
        .await;
    state.close();
    outcome
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, StatusCode> {
    let mut values = headers.get_all(name).iter();
    let value = values.next();
    if values.next().is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    value
        .map(|value| value.to_str().map_err(|_| StatusCode::BAD_REQUEST))
        .transpose()
}
fn has_media(value: &str, media: &str) -> bool {
    value
        .split(',')
        .any(|item| item.trim().split(';').next() == Some(media))
}
// Compare credential bytes without stopping at the first mismatched byte.
fn authorized(actual: &str, token: &str) -> bool {
    let expected = format!("Bearer {token}");
    let mut difference = actual.len() ^ expected.len();
    for (index, byte) in expected.bytes().enumerate() {
        difference |= usize::from(byte ^ actual.as_bytes().get(index).copied().unwrap_or(0));
    }
    difference == 0
}
fn secure(state: &HttpState, headers: &HeaderMap) -> Result<(), StatusCode> {
    // Browser access is deliberately disabled. No Origin is allowlisted and no
    // CORS headers are emitted, including on preflight or authenticated requests.
    if headers.contains_key("origin") {
        return Err(StatusCode::FORBIDDEN);
    }
    if state.config.bind.ip().is_loopback() {
        let host = header(headers, "host")?.ok_or(StatusCode::BAD_REQUEST)?;
        let bound = state.config.bind.to_string();
        let localhost = format!("localhost:{}", state.config.bind.port());
        if host != bound && host != localhost {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    if let Some(token) = &state.config.bearer_token {
        let credential = header(headers, "authorization")?.ok_or(StatusCode::UNAUTHORIZED)?;
        if !authorized(credential, token) {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }
    Ok(())
}

async fn endpoint(State(state): State<Arc<HttpState>>, request: Request) -> Response {
    if let Err(status) = secure(&state, request.headers()) {
        return if status == StatusCode::UNAUTHORIZED {
            (
                status,
                [("www-authenticate", "Bearer realm=\"kronello-mcp\"")],
            )
                .into_response()
        } else {
            status.into_response()
        };
    }
    let headers = request.headers();
    let version = match header(headers, "mcp-protocol-version") {
        Ok(version) => version,
        Err(status) => return status.into_response(),
    };
    if version.is_some_and(|value| !LEGACY_PROTOCOL_VERSIONS.contains(&value))
        || headers.contains_key("mcp-method")
    {
        return modern_post(state, request).await;
    }
    let all_headers = headers.clone();
    let session_id = match header(headers, "mcp-session-id") {
        Ok(id) => id.map(str::to_owned),
        Err(status) => return status.into_response(),
    };
    let connection = {
        let mut sessions = state.sessions.lock().expect("HTTP sessions");
        sessions.retain(|_, session| {
            if session.touched.elapsed() >= SESSION_IDLE_TIMEOUT {
                session.connection.close();
                false
            } else {
                true
            }
        });
        if let Some(id) = &session_id {
            let Some(session) = sessions.get_mut(id) else {
                return StatusCode::NOT_FOUND.into_response();
            };
            if let Some(version) = version
                && session.connection.version().as_deref() != Some(version)
            {
                return StatusCode::BAD_REQUEST.into_response();
            }
            session.touched = Instant::now();
            Some(session.connection.clone())
        } else {
            None
        }
    };
    match *request.method() {
        Method::DELETE => {
            let Some(id) = session_id else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            if let Some(session) = state.sessions.lock().expect("HTTP sessions").remove(&id) {
                session.connection.close();
            }
            return StatusCode::NO_CONTENT.into_response();
        }
        Method::GET => {
            if connection.is_none() {
                return StatusCode::BAD_REQUEST.into_response();
            }
            return (StatusCode::METHOD_NOT_ALLOWED, [("allow", "POST, DELETE")]).into_response();
        }
        Method::POST => {}
        _ => {
            return (
                StatusCode::METHOD_NOT_ALLOWED,
                [("allow", "POST, GET, DELETE")],
            )
                .into_response();
        }
    }
    let accept = match header(headers, "accept") {
        Ok(Some(accept)) => accept,
        _ => return StatusCode::NOT_ACCEPTABLE.into_response(),
    };
    if !has_media(accept, "application/json") || !has_media(accept, "text/event-stream") {
        return StatusCode::NOT_ACCEPTABLE.into_response();
    }
    match header(headers, "content-type") {
        Ok(Some(value)) if value.split(';').next().map(str::trim) == Some("application/json") => {}
        _ => return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response(),
    }
    let body = match to_bytes(request.into_body(), MAX_MESSAGE_BYTES).await {
        Ok(body) => body,
        Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };
    let message = match std::str::from_utf8(&body) {
        Ok(message) => message,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    if serde_json::from_str::<Value>(message)
        .ok()
        .is_some_and(|value| {
            value["params"]["_meta"]
                .get(crate::modern::VERSION_KEY)
                .is_some()
        })
    {
        return modern_message(state, &all_headers, message);
    }
    if serde_json::from_str::<Value>(message)
        .ok()
        .is_some_and(|value| {
            value.get("method").is_none()
                && (value.get("result").is_some() || value.get("error").is_some())
        })
    {
        // No adopted feature issues server requests. An unsolicited client
        // response therefore cannot be accepted by this endpoint.
        return (
            StatusCode::BAD_REQUEST,
            Json(rpc_error(
                Value::Null,
                -32600,
                "Unsolicited client response",
                Value::Null,
            )),
        )
            .into_response();
    }
    let can_start = serde_json::from_str::<Value>(message)
        .ok()
        .is_some_and(|value| {
            matches!(
                value["method"].as_str(),
                Some("initialize" | "server/discover")
            ) && value.get("id").is_some()
        });
    if connection.is_none() && !can_start {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let is_new = connection.is_none();
    let connection =
        connection.unwrap_or_else(|| Connection::new(state.backend, state.slots.clone()));
    match connection.submit(message) {
        Reply::Immediate(Some(response)) => {
            if is_new && response.get("result").is_some() {
                let mut sessions = state.sessions.lock().expect("HTTP sessions");
                if sessions.len() >= MAX_SESSIONS {
                    connection.close();
                    return StatusCode::SERVICE_UNAVAILABLE.into_response();
                }
                let id = uuid::Uuid::new_v4().to_string();
                sessions.insert(
                    id.clone(),
                    Session {
                        connection,
                        touched: Instant::now(),
                    },
                );
                ([("mcp-session-id", id)], Json(response)).into_response()
            } else {
                Json(response).into_response()
            }
        }
        Reply::Immediate(None) => StatusCode::ACCEPTED.into_response(),
        Reply::Stream(receiver) => {
            // Losing this POST stream does not cancel work. Cancellation is a
            // separate notification; DELETE/expiry explicitly end the session.
            let events = stream::unfold(receiver, |mut receiver| async {
                receiver.recv().await.map(|value| {
                    (
                        Ok::<_, Infallible>(
                            Event::default().event("message").data(value.to_string()),
                        ),
                        receiver,
                    )
                })
            });
            Sse::new(events).into_response()
        }
    }
}

async fn modern_post(state: Arc<HttpState>, request: Request) -> Response {
    if request.method() != Method::POST {
        return (StatusCode::METHOD_NOT_ALLOWED, [("allow", "POST")]).into_response();
    }
    let headers = request.headers().clone();
    if header(&headers, "accept")
        .ok()
        .flatten()
        .is_none_or(|value| {
            !has_media(value, "application/json") || !has_media(value, "text/event-stream")
        })
    {
        return StatusCode::NOT_ACCEPTABLE.into_response();
    }
    if header(&headers, "content-type")
        .ok()
        .flatten()
        .is_none_or(|value| value.split(';').next().map(str::trim) != Some("application/json"))
    {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    let body = match to_bytes(request.into_body(), MAX_MESSAGE_BYTES).await {
        Ok(body) => body,
        Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };
    let message = match std::str::from_utf8(&body) {
        Ok(message) => message,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    modern_message(state, &headers, message)
}
fn decoded_name(value: &str) -> Option<String> {
    if let Some(encoded) = value.strip_prefix("=?base64?") {
        let encoded = encoded.strip_suffix("?=")?;
        return String::from_utf8(data_encoding::BASE64.decode(encoded.as_bytes()).ok()?).ok();
    }
    if value.starts_with(' ')
        || value.ends_with(' ')
        || value.starts_with('\t')
        || value.ends_with('\t')
        || !value
            .bytes()
            .all(|byte| byte == b'\t' || (0x20..=0x7e).contains(&byte))
    {
        return None;
    }
    Some(value.to_owned())
}
fn modern_message(state: Arc<HttpState>, headers: &HeaderMap, message: &str) -> Response {
    let raw: Value = match serde_json::from_str(message) {
        Ok(raw) => raw,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(rpc_error(
                    Value::Null,
                    -32700,
                    error.to_string(),
                    Value::Null,
                )),
            )
                .into_response();
        }
    };
    let id = raw.get("id").cloned().unwrap_or(Value::Null);
    let mismatch = |detail| {
        (
            StatusCode::BAD_REQUEST,
            Json(rpc_error(id.clone(), -32020, detail, Value::Null)),
        )
            .into_response()
    };
    let version = match header(headers, "mcp-protocol-version") {
        Ok(Some(value)) => value,
        _ => return mismatch("Missing or malformed MCP-Protocol-Version"),
    };
    if version != crate::modern::VERSION {
        return (
            StatusCode::BAD_REQUEST,
            Json(crate::modern::unsupported(id, version)),
        )
            .into_response();
    }
    if raw["params"]["_meta"][crate::modern::VERSION_KEY] != version {
        return mismatch("MCP-Protocol-Version does not match request metadata");
    }
    let method = match header(headers, "mcp-method") {
        Ok(Some(value)) if raw["method"] == value => value,
        _ => return mismatch("Missing, malformed or mismatched Mcp-Method"),
    };
    if matches!(method, "tools/call" | "resources/read" | "prompts/get") {
        let source = if method == "resources/read" {
            "uri"
        } else {
            "name"
        };
        let decoded = header(headers, "mcp-name")
            .ok()
            .flatten()
            .and_then(decoded_name);
        match (decoded.as_deref(), raw["params"][source].as_str()) {
            (Some(header_name), Some(body_name)) if header_name == body_name => {}
            _ => return mismatch("Missing, malformed or mismatched Mcp-Name"),
        }
    }
    if raw.get("id").is_none() {
        // Cancellation of HTTP work is stream closure, not a notification POST.
        return (
            StatusCode::BAD_REQUEST,
            Json(rpc_error(
                Value::Null,
                -32600,
                "No modern HTTP client notification is implemented",
                Value::Null,
            )),
        )
            .into_response();
    }
    let request_permit = match state.modern_slots.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(rpc_error(
                    id,
                    -32000,
                    "Too many active HTTP requests",
                    serde_json::json!({"code":"RESOURCE_LIMIT"}),
                )),
            )
                .into_response();
        }
    };
    let connection = Connection::new(state.backend, state.slots.clone());
    match connection.submit(message) {
        Reply::Immediate(Some(value)) => {
            let status = match value["error"]["code"].as_i64() {
                Some(-32601) => StatusCode::NOT_FOUND,
                Some(_) => StatusCode::BAD_REQUEST,
                None => StatusCode::OK,
            };
            (status, Json(value)).into_response()
        }
        Reply::Immediate(None) => StatusCode::ACCEPTED.into_response(),
        Reply::Stream(receiver) => {
            struct RequestStream {
                receiver: tokio::sync::mpsc::UnboundedReceiver<Value>,
                connection: Arc<Connection>,
                _permit: tokio::sync::OwnedSemaphorePermit,
            }
            impl Drop for RequestStream {
                fn drop(&mut self) {
                    self.connection.close();
                }
            }
            let stream = RequestStream {
                receiver,
                connection,
                _permit: request_permit,
            };
            let events = stream::unfold(stream, |mut stream| async {
                stream.receiver.recv().await.map(|value| {
                    (
                        Ok::<_, Infallible>(
                            Event::default().event("message").data(value.to_string()),
                        ),
                        stream,
                    )
                })
            });
            ([("x-accel-buffering", "no")], Sse::new(events)).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    fn state() -> Arc<HttpState> {
        Arc::new(HttpState {
            config: HttpConfig::default(),
            backend: BackendSelection::CpuReference,
            slots: Arc::new(Semaphore::new(3)),
            modern_slots: Arc::new(Semaphore::new(crate::connection::MAX_ACTIVE_REQUESTS)),
            sessions: Mutex::new(BTreeMap::new()),
        })
    }
    async fn post(
        state: Arc<HttpState>,
        message: Value,
        session: Option<&str>,
        version: Option<&str>,
    ) -> Response {
        let mut request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("host", "127.0.0.1:8765")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if let Some(id) = session {
            request = request.header("mcp-session-id", id);
        }
        if let Some(version) = version {
            request = request.header("mcp-protocol-version", version);
        }
        endpoint(
            State(state),
            request.body(Body::from(message.to_string())).unwrap(),
        )
        .await
    }
    fn initialize(version: &str) -> Value {
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"test","version":"1"}}})
    }
    async fn json_body(response: Response) -> Value {
        serde_json::from_slice(
            &to_bytes(response.into_body(), MAX_MESSAGE_BYTES)
                .await
                .unwrap(),
        )
        .unwrap()
    }

    fn modern_request(method: &str, params: Value) -> Value {
        let mut params = params;
        params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}});
        json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})
    }
    async fn modern_post_test(
        state: Arc<HttpState>,
        value: Value,
        version: Option<&str>,
        method: Option<&str>,
        name: Option<&str>,
    ) -> Response {
        let mut request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("host", "127.0.0.1:8765")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if let Some(value) = version {
            request = request.header("mcp-protocol-version", value);
        }
        if let Some(value) = method {
            request = request.header("mcp-method", value);
        }
        if let Some(value) = name {
            request = request.header("mcp-name", value);
        }
        endpoint(
            State(state),
            request.body(Body::from(value.to_string())).unwrap(),
        )
        .await
    }
    #[tokio::test]
    async fn modern_http_is_stateless_and_validates_mirrored_headers() {
        let state = state();
        let value = modern_request("server/discover", json!({}));
        let response = modern_post_test(
            state.clone(),
            value.clone(),
            Some("2026-07-28"),
            Some("server/discover"),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key("mcp-session-id"));
        assert_eq!(
            json_body(response).await["result"]["resultType"],
            "complete"
        );
        assert!(state.sessions.lock().unwrap().is_empty());
        for (version, method) in [
            (None, Some("server/discover")),
            (Some("2026-07-28"), None),
            (Some("2026-07-28"), Some("tools/list")),
        ] {
            let response =
                modern_post_test(state.clone(), value.clone(), version, method, None).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(json_body(response).await["error"]["code"], -32020);
        }
        let call = modern_request(
            "tools/call",
            json!({"name":"capabilities.get","arguments":{}}),
        );
        let mismatch = modern_post_test(
            state.clone(),
            call,
            Some("2026-07-28"),
            Some("tools/call"),
            Some("project.info"),
        )
        .await;
        assert_eq!(json_body(mismatch).await["error"]["code"], -32020);
        for (method, params) in [
            ("tools/call", json!({"arguments":{}})),
            ("resources/read", json!({})),
            ("prompts/get", json!({"name":42})),
        ] {
            let response = modern_post_test(
                state.clone(),
                modern_request(method, params),
                Some("2026-07-28"),
                Some(method),
                Some("=?base64?%%%?="),
            )
            .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(json_body(response).await["error"]["code"], -32020);
        }
        let read = modern_request("resources/read", json!({"uri":"kronello://schema/api-v1"}));
        let name = format!(
            "=?base64?{}?=",
            data_encoding::BASE64.encode(b"kronello://schema/api-v1")
        );
        let response = modern_post_test(
            state.clone(),
            read,
            Some("2026-07-28"),
            Some("resources/read"),
            Some(&name),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), MAX_MESSAGE_BYTES)
            .await
            .unwrap();
        let body = std::str::from_utf8(&bytes).unwrap();
        let value: Value = serde_json::from_str(
            body.lines()
                .find_map(|line| line.strip_prefix("data: "))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(value["result"]["cacheScope"], "private");
        let unknown = modern_request("unsupported/method", json!({}));
        let response = modern_post_test(
            state,
            unknown,
            Some("2026-07-28"),
            Some("unsupported/method"),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_body(response).await["error"]["code"], -32601);
    }
    #[tokio::test]
    async fn modern_http_active_requests_have_a_process_limit() {
        let state = state();
        let permit = state
            .modern_slots
            .clone()
            .acquire_many_owned(crate::connection::MAX_ACTIVE_REQUESTS as u32)
            .await
            .unwrap();
        let value = modern_request("tools/list", json!({}));
        let response = modern_post_test(
            state.clone(),
            value.clone(),
            Some("2026-07-28"),
            Some("tools/list"),
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            json_body(response).await["error"]["data"]["code"],
            "RESOURCE_LIMIT"
        );
        drop(permit);
        assert_eq!(
            modern_post_test(state, value, Some("2026-07-28"), Some("tools/list"), None)
                .await
                .status(),
            StatusCode::OK
        );
    }
    #[tokio::test]
    async fn modern_http_stream_drop_cancels_queued_mutation() {
        let state = state();
        let _permit = state.slots.clone().acquire_many_owned(3).await.unwrap();
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("never-created.kronello");
        let document: Value =
            serde_json::from_str(include_str!("../../../examples/ffi-preview.project.json"))
                .unwrap();
        let value = modern_request(
            "tools/call",
            json!({"name":"project.create","arguments":{"project":path,"document":document}}),
        );
        let response = modern_post_test(
            state.clone(),
            value,
            Some("2026-07-28"),
            Some("tools/call"),
            Some("project.create"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        drop(response);
        drop(_permit);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!path.exists());
    }
    #[test]
    fn bind_defaults_and_remote_auth_are_explicit() {
        let default = HttpConfig::default();
        assert_eq!(default.bind.to_string(), "127.0.0.1:8765");
        assert!(default.validate().is_ok());
        let mut remote = HttpConfig {
            bind: "0.0.0.0:8765".parse().unwrap(),
            bearer_token: None,
        };
        assert!(remote.validate().is_err());
        remote.bearer_token = Some("short".into());
        assert!(remote.validate().is_err());
        remote.bearer_token = Some("x".repeat(32));
        assert!(remote.validate().is_ok());
        assert!(authorized(
            &format!("Bearer {}", "x".repeat(32)),
            remote.bearer_token.as_ref().unwrap()
        ));
        assert!(!authorized(
            &format!("Bearer {}", "y".repeat(32)),
            remote.bearer_token.as_ref().unwrap()
        ));
        let state = state();
        let mut headers = HeaderMap::new();
        headers.append("origin", "https://one.invalid".parse().unwrap());
        headers.append("origin", "https://two.invalid".parse().unwrap());
        assert_eq!(secure(&state, &headers), Err(StatusCode::FORBIDDEN));
    }

    #[tokio::test]
    async fn sessions_negotiate_headers_expire_and_never_accept_unknown_versions() {
        let state = state();
        // Unsupported initialize version negotiates the newest supported version.
        let response = post(state.clone(), initialize("2026-07-28"), None, None).await;
        let id = response.headers()["mcp-session-id"]
            .to_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            json_body(response).await["result"]["protocolVersion"],
            "2025-11-25"
        );
        let ready = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
        assert_eq!(
            post(state.clone(), ready.clone(), Some(&id), None)
                .await
                .status(),
            StatusCode::ACCEPTED
        );
        let ping = json!({"jsonrpc":"2.0","id":2,"method":"ping"});
        assert_eq!(
            post(state.clone(), ping.clone(), Some(&id), Some("2025-06-18"))
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
        let unsupported = post(state.clone(), ping.clone(), Some(&id), Some("2100-01-01")).await;
        assert_eq!(unsupported.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json_body(unsupported).await["error"]["data"]["supported"],
            json!(SUPPORTED_PROTOCOL_VERSIONS)
        );
        let discover = post(
            state.clone(),
            json!({"jsonrpc":"2.0","id":3,"method":"server/discover"}),
            None,
            None,
        )
        .await;
        assert_eq!(
            json_body(discover).await["error"]["data"]["code"],
            "UNSUPPORTED_PROTOCOL_VERSION"
        );
        state.sessions.lock().unwrap().get_mut(&id).unwrap().touched =
            Instant::now() - SESSION_IDLE_TIMEOUT;
        assert_eq!(
            post(state.clone(), ping, Some(&id), Some("2025-11-25"))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        assert!(state.sessions.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn session_limit_and_all_method_authentication_apply() {
        let state = state();
        for _ in 0..MAX_SESSIONS {
            assert_eq!(
                post(state.clone(), initialize("2025-11-25"), None, None)
                    .await
                    .status(),
                StatusCode::OK
            );
        }
        assert_eq!(
            post(state.clone(), initialize("2025-11-25"), None, None)
                .await
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        let authenticated = Arc::new(HttpState {
            config: HttpConfig {
                bearer_token: Some("x".repeat(32)),
                ..HttpConfig::default()
            },
            backend: BackendSelection::CpuReference,
            slots: Arc::new(Semaphore::new(3)),
            modern_slots: Arc::new(Semaphore::new(crate::connection::MAX_ACTIVE_REQUESTS)),
            sessions: Mutex::new(BTreeMap::new()),
        });
        for method in ["POST", "GET", "DELETE", "OPTIONS"] {
            let request = Request::builder()
                .method(method)
                .uri("/mcp")
                .header("host", "127.0.0.1:8765")
                .body(Body::empty())
                .unwrap();
            assert_eq!(
                endpoint(State(authenticated.clone()), request)
                    .await
                    .status(),
                StatusCode::UNAUTHORIZED
            );
        }
    }
}
