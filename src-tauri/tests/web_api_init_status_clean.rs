//! `GET /api/init-status` with no startup error (WEB-32). Own test binary:
//! `init_status` is a process-global with no reset, so the error cases live in
//! `web_api_init_status.rs`.

mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};
use support::{ensure_test_home, reset_test_fs, test_mutex, web_call, web_router};

#[tokio::test]
async fn init_status_is_null_without_error() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) = web_call(&router, "GET", "/api/init-status", None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "ok": true, "data": Value::Null }));
}

/// `buildSha` of a test build: `null` unless the harness itself was compiled with
/// `CC_SWITCH_WEB_GIT_SHA` (same env rule as the library).
fn expected_build_sha() -> serde_json::Value {
    match option_env!("CC_SWITCH_WEB_GIT_SHA").map(str::trim) {
        Some(sha) if !sha.is_empty() => json!(sha),
        _ => Value::Null,
    }
}

#[tokio::test]
async fn full_router_capabilities_carry_the_build_sha() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) = web_call(&router, "GET", "/api/capabilities", None).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.as_object().unwrap().contains_key("buildSha"), "{body}");
    assert_eq!(body["buildSha"], expected_build_sha());
    let boundaries = body["runtime_boundaries"].as_str().unwrap();
    assert!(!boundaries.contains("no auto-update"), "{boundaries}");
    assert!(
        boundaries.contains("updates: check in UI, install by operator"),
        "{boundaries}"
    );
}
