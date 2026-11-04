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

#[tokio::test]
async fn full_router_capabilities_carry_the_build_sha() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) = web_call(&router, "GET", "/api/capabilities", None).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.as_object().unwrap().contains_key("buildSha"), "{body}");
    // The value is pinned in-crate (`web_api::tests`), where `web_update::build_sha()`
    // is reachable; here the field must exist and be a string or null.
    assert!(
        body["buildSha"].is_null() || body["buildSha"].is_string(),
        "{body}"
    );
    let boundaries = body["runtime_boundaries"].as_str().unwrap();
    assert!(!boundaries.contains("no auto-update"), "{boundaries}");
    assert!(
        boundaries.contains("updates: check in UI, install by operator"),
        "{boundaries}"
    );
}
