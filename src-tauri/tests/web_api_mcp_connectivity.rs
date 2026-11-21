mod support;

use axum::http::{header, StatusCode};
use serde_json::{json, Value};
use serial_test::serial;
use support::{reset_test_fs, test_mutex, web_call, web_call_raw, web_router};

/// A command name that exists on no OS's PATH (no Unix-only path literal).
const MISSING_COMMAND: &str = "cc-switch-web-test-no-such-mcp-command";

fn ndjson(bytes: &[u8]) -> Vec<Value> {
    std::str::from_utf8(bytes)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[tokio::test]
#[serial]
async fn stream_starts_with_the_batch_id_and_ends() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();
    let (status, saved) = web_call(
        &router,
        "PUT",
        "/api/mcp/servers",
        Some(json!({"server": {
            "id": "probe", "name": "probe",
            "server": {"type": "stdio", "command": MISSING_COMMAND},
            "apps": {"claude": true}
        }})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["ok"], true);

    let (status, headers, bytes) = web_call_raw(
        &router,
        "POST",
        "/api/mcp/connectivity",
        Some(json!({"serverIds": ["probe"]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/x-ndjson");
    let lines = ndjson(&bytes);
    assert_eq!(lines.len(), 2, "batch id line + one result, got {lines:?}");
    let batch_id = lines[0]["batchId"].as_str().unwrap();
    assert!(!batch_id.is_empty());
    assert_eq!(lines[1], json!({"serverId": "probe", "status": "failed"}));
}

#[tokio::test]
#[serial]
async fn cancel_of_an_unknown_batch_is_ok() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();
    let (status, body) = web_call(&router, "POST", "/api/mcp/connectivity/nope/cancel", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
}
