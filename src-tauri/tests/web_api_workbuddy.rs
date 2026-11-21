//! `/api/workbuddy/*` (v4.0.35 parity). The test HOME has no WorkBuddy install, so nothing
//! here reaches a running WorkBuddy.
#![cfg(not(target_os = "windows"))]

mod support;

use axum::http::StatusCode;
use serde_json::json;
use serial_test::serial;
use support::{ensure_test_home, reset_test_fs, test_mutex, web_call, web_router};

#[tokio::test]
#[serial]
async fn live_state_without_workbuddy_reports_not_signed_in() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) = web_call(&router, "GET", "/api/workbuddy/live-state", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["data"]["running"], json!(false), "{body}");
    assert_eq!(body["data"]["signedIn"], json!(false), "{body}");
}

#[tokio::test]
#[serial]
async fn quota_for_an_unknown_account_is_an_error_envelope() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) = web_call(
        &router,
        "GET",
        "/api/workbuddy/accounts/no-such-account/quota",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert!(
        body["error"].as_str().is_some_and(|e| !e.is_empty()),
        "{body}"
    );
}

#[tokio::test]
#[serial]
async fn import_without_a_workbuddy_login_is_an_error() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (_, body) = web_call(&router, "POST", "/api/workbuddy/import-current", None).await;

    assert_eq!(body["ok"], json!(false), "{body}");
    assert!(
        body["error"].as_str().is_some_and(|e| !e.is_empty()),
        "{body}"
    );
}

#[tokio::test]
#[serial]
async fn model_owners_without_workbuddy_config_is_an_empty_list() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) = web_call(&router, "GET", "/api/workbuddy/model-owners", None).await;

    // A missing models.json reads as an empty document, not an error.
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["data"], json!([]), "{body}");
}
