use std::sync::Arc;
use std::time::Duration;

use kronello_mcp::{Connection, Reply};
use kronello_service::BackendSelection;
use serde_json::{Value, json};
use tokio::sync::Semaphore;

fn immediate(connection: &Arc<Connection>, value: Value) -> Option<Value> {
    match connection.submit(&value.to_string()) {
        Reply::Immediate(value) => value,
        Reply::Stream(_) => panic!("expected immediate reply"),
    }
}
fn ready(slots: Arc<Semaphore>) -> Arc<Connection> {
    let connection = Connection::new(BackendSelection::CpuReference, slots);
    let init = immediate(&connection, json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{"sampling":{},"tasks":{}},"clientInfo":{"name":"test","version":"1"}}})).unwrap();
    assert!(init["result"]["capabilities"].get("sampling").is_none());
    assert!(init["result"]["capabilities"].get("tasks").is_none());
    immediate(
        &connection,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
    connection
}
fn work(
    connection: &Arc<Connection>,
    id: Value,
    token: Value,
) -> tokio::sync::mpsc::UnboundedReceiver<Value> {
    match connection.submit(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"project.info","arguments":{"project":"missing.kronello"},"_meta":{"progressToken":token}}}).to_string()) {
        Reply::Stream(stream) => stream,
        Reply::Immediate(reply) => panic!("{reply:?}"),
    }
}

#[tokio::test]
async fn cancellation_maps_typed_ids_and_progress_tokens_and_isolates_sessions() {
    let slots = Arc::new(Semaphore::new(0));
    let a = ready(slots.clone());
    let b = ready(slots.clone());
    let mut number = work(&a, json!(7), json!("numeric"));
    let mut string = work(&a, json!("7"), json!("string"));
    let mut other = work(&b, json!(7), json!("other"));
    assert_eq!(
        number.recv().await.unwrap()["params"]["progressToken"],
        "numeric"
    );
    assert_eq!(
        string.recv().await.unwrap()["params"]["progressToken"],
        "string"
    );
    assert_eq!(
        other.recv().await.unwrap()["params"]["progressToken"],
        "other"
    );
    immediate(
        &a,
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":7}}),
    );
    assert!(number.recv().await.is_none());
    let duplicate = immediate(
        &a,
        json!({"jsonrpc":"2.0","id":"7","method":"tools/call","params":{"name":"job.list"}}),
    )
    .unwrap();
    assert_eq!(duplicate["error"]["code"], -32602);
    let token_duplicate = immediate(&a, json!({"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"job.list","_meta":{"progressToken":"string"}}})).unwrap();
    assert_eq!(token_duplicate["error"]["code"], -32602);
    // Unknown and completed cancellation notifications have no response.
    immediate(
        &a,
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":99}}),
    );
    slots.add_permits(3);
    for stream in [&mut string, &mut other] {
        let progress = stream.recv().await.unwrap();
        assert_eq!(progress["params"]["progress"], 1);
        let response = stream.recv().await.unwrap();
        assert_eq!(
            response["result"]["structuredContent"]["error"]["code"],
            "PROJECT_NOT_FOUND"
        );
        assert!(stream.recv().await.is_none());
    }
    immediate(
        &a,
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"7"}}),
    );
    assert_eq!(
        immediate(&a, json!({"jsonrpc":"2.0","id":50,"method":"ping"})).unwrap()["result"],
        json!({})
    );
}

#[tokio::test]
async fn close_stops_queued_work_and_stream_drop_does_not_cancel() {
    let slots = Arc::new(Semaphore::new(0));
    let connection = ready(slots.clone());
    let mut stream = work(&connection, json!(1), json!(1));
    stream.recv().await.unwrap();
    connection.close();
    assert!(stream.recv().await.is_none());
    slots.add_permits(1);
    let connection = ready(slots.clone());
    let stream = work(&connection, json!(2), json!(2));
    drop(stream);
    // The request remains active until service finishes, despite a lost receiver.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let reply = immediate(&connection, json!({"jsonrpc":"2.0","id":99,"method":"ping"})).unwrap();
            // ping is allowed independently; eventual tool request ID reuse below.
            assert_eq!(reply["result"], json!({}));
            match connection.submit(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"project.info","arguments":{"project":"missing.kronello"}}}).to_string()) {
                Reply::Stream(mut stream) => { assert!(stream.recv().await.unwrap().get("result").is_some()); break; }
                Reply::Immediate(_) => tokio::task::yield_now().await,
            }
        }
    }).await.unwrap();
}

#[tokio::test]
async fn unsupported_features_remain_unadvertised_and_unversioned_discovery_is_rejected() {
    let connection = ready(Arc::new(Semaphore::new(1)));
    for method in [
        "sampling/createMessage",
        "tasks/list",
        "tasks/get",
        "tasks/cancel",
        "resources/subscribe",
        "completion/complete",
    ] {
        let error =
            immediate(&connection, json!({"jsonrpc":"2.0","id":1,"method":method})).unwrap();
        assert_eq!(error["error"]["code"], -32601);
    }
    let error = immediate(
        &connection,
        json!({"jsonrpc":"2.0","id":1,"method":"server/discover"}),
    )
    .unwrap();
    assert_eq!(
        error["error"]["data"]["supported"],
        json!(kronello_mcp::SUPPORTED_PROTOCOL_VERSIONS)
    );
    assert_eq!(
        error["error"]["data"]["code"],
        "UNSUPPORTED_PROTOCOL_VERSION"
    );
    let mut stream = match connection.submit(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"project.info","arguments":{"project":"missing.kronello"},"task":{"ttl":1000}}}).to_string()) { Reply::Stream(stream) => stream, _ => panic!() };
    assert_eq!(stream.recv().await.unwrap()["error"]["code"], -32602);
}

#[tokio::test]
async fn lost_response_stream_does_not_cancel_command_execution() {
    let slots = Arc::new(Semaphore::new(0));
    let connection = ready(slots.clone());
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("still-created.kronello");
    let doc: Value =
        serde_json::from_str(include_str!("../../../examples/m1-demo.project.json")).unwrap();
    let reply = connection.submit(&json!({"jsonrpc":"2.0","id":"lost-stream","method":"tools/call","params":{"name":"project.create","arguments":{"project":path,"document":doc}}}).to_string());
    match reply {
        Reply::Stream(stream) => drop(stream),
        _ => panic!(),
    }
    slots.add_permits(1);
    tokio::time::timeout(Duration::from_secs(5), async {
        while !path.is_file() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn active_request_limit_and_invalid_tokens_are_rejected() {
    let connection = ready(Arc::new(Semaphore::new(0)));
    let mut streams = Vec::new();
    for id in 1..=32 {
        streams.push(work(&connection, json!(id), json!(id)));
    }
    let limit = immediate(
        &connection,
        json!({"jsonrpc":"2.0","id":33,"method":"tools/call","params":{"name":"job.list"}}),
    )
    .unwrap();
    assert_eq!(limit["error"]["data"]["code"], "RESOURCE_LIMIT");
    for token in [Value::Null, json!(false), json!([]), json!({})] {
        let invalid = immediate(&connection, json!({"jsonrpc":"2.0","id":34,"method":"tools/call","params":{"name":"job.list","_meta":{"progressToken":token}}})).unwrap();
        assert_eq!(invalid["error"]["code"], -32602);
    }
    connection.close();
    for mut stream in streams {
        assert_eq!(
            stream.recv().await.unwrap()["method"],
            "notifications/progress"
        );
        assert!(stream.recv().await.is_none());
    }
}
