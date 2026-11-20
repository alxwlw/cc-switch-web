//! `/api/claude-account/*` (v4.0.35 parity). No test starts a `claude` process: the quota
//! probe targets an account that does not exist and the paste targets an unknown flow.
#![cfg(not(target_os = "windows"))]

mod support;

use axum::http::StatusCode;
use serde_json::json;
use serial_test::serial;
use support::{ensure_test_home, reset_test_fs, test_mutex, web_call, web_router};

#[tokio::test]
#[serial]
async fn quota_for_unknown_account_reports_missing_credentials() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) = web_call(
        &router,
        "GET",
        "/api/claude-account/quota/no-such-account",
        None,
    )
    .await;

    // The service answers "not found" itself, before any CLI probe, so this is a 200.
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["data"]["tool"], json!("claude"));
    assert_eq!(body["data"]["credentialStatus"], json!("not_found"));
    assert_eq!(body["data"]["success"], json!(false));
}

/// Linux only: on macOS the default-plan reader consults the keychain, which a test HOME
/// does not isolate.
#[cfg(target_os = "linux")]
#[tokio::test]
#[serial]
async fn default_plan_without_a_claude_login_is_null() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) = web_call(&router, "GET", "/api/claude-account/default-plan", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert!(body["data"].is_null(), "{body}");
}

#[tokio::test]
#[serial]
async fn login_switch_reports_its_status() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) = web_call(&router, "GET", "/api/claude-account/login-switch", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert!(body["data"]["active"].is_boolean(), "{body}");
}

#[tokio::test]
#[serial]
async fn paste_for_an_unknown_flow_is_an_error() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) = web_call(
        &router,
        "POST",
        "/api/claude-account/paste",
        Some(json!({"flowId": "nope", "pasted": "code"})),
    )
    .await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert!(
        body["error"].as_str().is_some_and(|e| !e.is_empty()),
        "{body}"
    );
}
