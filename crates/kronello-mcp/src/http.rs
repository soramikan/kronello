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
use serde_json::{Value, json};
use tokio::sync::Semaphore;

use crate::connection::{Connection, Reply};
use crate::{MAX_MESSAGE_BYTES, SUPPORTED_PROTOCOL_VERSIONS, rpc_error};

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
    if let Some(version) = version
        && !SUPPORTED_PROTOCOL_VERSIONS.contains(&version)
    {
        return (StatusCode::BAD_REQUEST, Json(rpc_error(Value::Null, -32600, "Unsupported HTTP protocol version", json!({"code":"UNSUPPORTED_PROTOCOL_VERSION","supported":SUPPORTED_PROTOCOL_VERSIONS})))).into_response();
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    fn state() -> Arc<HttpState> {
        Arc::new(HttpState {
            config: HttpConfig::default(),
            backend: BackendSelection::CpuReference,
            slots: Arc::new(Semaphore::new(3)),
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
        let unsupported = post(state.clone(), ping.clone(), Some(&id), Some("2026-07-28")).await;
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
