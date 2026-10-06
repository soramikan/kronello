use kronello_mcp::{Connection, Reply, SUPPORTED_PROTOCOL_VERSIONS};
use kronello_service::BackendSelection;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::Semaphore;
fn request(id: u64, method: &str, mut params: Value) -> Value {
    params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}});
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
async fn reply(connection: &Arc<Connection>, value: Value) -> Value {
    match connection.submit(&value.to_string()) {
        Reply::Immediate(Some(value)) => value,
        Reply::Stream(mut receiver) => loop {
            let value = receiver.recv().await.unwrap();
            if value.get("result").is_some() || value.get("error").is_some() {
                return value;
            }
        },
        Reply::Immediate(None) => panic!("request response required"),
    }
}
#[tokio::test]
async fn modern_discovery_per_request_validation_and_legacy_coexist() {
    let connection = Connection::new(BackendSelection::CpuReference, Arc::new(Semaphore::new(3)));
    let discovered = reply(&connection, request(1, "server/discover", json!({}))).await;
    assert_eq!(
        discovered["result"]["supportedVersions"],
        json!(SUPPORTED_PROTOCOL_VERSIONS)
    );
    assert_eq!(
        discovered["result"]["capabilities"],
        json!({"tools":{},"resources":{},"prompts":{}})
    );
    assert_eq!(discovered["result"]["resultType"], "complete");
    assert_eq!(discovered["result"]["ttlMs"], 0);
    assert_eq!(connection.version(), None);
    let listed = reply(&connection, request(2, "tools/list", json!({}))).await;
    assert!(!listed["result"]["tools"].as_array().unwrap().is_empty());
    assert_eq!(listed["result"]["cacheScope"], "private");
    let mut missing = request(3, "tools/list", json!({}));
    missing["params"]["_meta"]
        .as_object_mut()
        .unwrap()
        .remove("io.modelcontextprotocol/clientCapabilities");
    assert_eq!(reply(&connection, missing).await["error"]["code"], -32602);
    let mut unsupported = request(4, "tools/list", json!({}));
    unsupported["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!("2100-01-01");
    let unsupported = reply(&connection, unsupported).await;
    assert_eq!(unsupported["error"]["code"], -32022);
    assert_eq!(unsupported["error"]["data"]["requested"], "2100-01-01");
    assert_eq!(
        reply(
            &connection,
            json!({"jsonrpc":"2.0","id":5,"method":"tools/list"})
        )
        .await["error"]["code"],
        -32002
    );
    let initialized = reply(&connection, json!({"jsonrpc":"2.0","id":6,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"legacy","version":"1"}}})).await;
    assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
    assert!(initialized["result"].get("resultType").is_none());
    connection.submit(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    let legacy = reply(
        &connection,
        json!({"jsonrpc":"2.0","id":7,"method":"tools/list"}),
    )
    .await;
    assert!(legacy["result"].get("resultType").is_none());
    assert_eq!(
        reply(&connection, request(8, "ping", json!({}))).await["result"]["resultType"],
        "complete"
    );
    assert_eq!(connection.version().as_deref(), Some("2025-11-25"));
    let mut fractional = request(9, "ping", json!({}));
    fractional["id"] = json!(1.5);
    assert_eq!(reply(&connection, fractional).await["id"], json!(1.5));
}
