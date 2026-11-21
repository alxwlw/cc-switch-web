//! `/api/dsh/*` and `/api/copilot/cli-account` (v4.0.35 parity). The test HOME holds no DSH
//! credential and no `~/.copilot/config.json`, so nothing here reaches the network.
//! `dsh_account_login_start` calls the DSH server and is covered by the adapter test only.
#![cfg(not(target_os = "windows"))]

mod support;

use axum::http::StatusCode;
use serde_json::json;
use serial_test::serial;
use support::{ensure_test_home, reset_test_fs, test_mutex, web_call, web_router};

#[tokio::test]
#[serial]
async fn status_without_credentials_is_signed_out() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) = web_call(&router, "GET", "/api/dsh/status", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["data"]["signedIn"], json!(false), "{body}");
    assert_eq!(body["data"]["expired"], json!(false), "{body}");
}

#[tokio::test]
#[serial]
async fn sign_out_without_credentials_succeeds() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) = web_call(&router, "POST", "/api/dsh/sign-out", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert!(body["data"].is_null(), "{body}");
}

#[tokio::test]
#[serial]
async fn poll_of_an_unknown_flow_is_an_error() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) = web_call(&router, "POST", "/api/dsh/login/nope/poll", None).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert!(
        body["error"].as_str().is_some_and(|e| !e.is_empty()),
        "{body}"
    );
}

#[tokio::test]
#[serial]
async fn cancel_of_an_unknown_flow_succeeds() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) = web_call(&router, "POST", "/api/dsh/login/nope/cancel", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert!(body["data"].is_null(), "{body}");
}

#[tokio::test]
#[serial]
async fn copilot_cli_account_without_a_cli_login_is_null() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) = web_call(&router, "GET", "/api/copilot/cli-account", None).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert!(body["data"].is_null(), "{body}");
}
