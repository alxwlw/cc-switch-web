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

/// Puts a `claude` that exits at once first on PATH (so no real login ever starts) and
/// restores PATH when dropped.
struct FakeClaudeOnPath {
    old_path: Option<std::ffi::OsString>,
}

impl FakeClaudeOnPath {
    fn install() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let dir = ensure_test_home().join("fake-bin");
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("claude");
        std::fs::write(&script, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let old_path = std::env::var_os("PATH");
        let mut paths = vec![dir];
        paths.extend(std::env::split_paths(&old_path.clone().unwrap_or_default()));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
        Self { old_path }
    }
}

impl Drop for FakeClaudeOnPath {
    fn drop(&mut self) {
        match &self.old_path {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
    }
}

/// The web login of `claude_official` must never ask `claude auth login` to open a browser
/// on the server host, whatever the client sends. Unix only: it needs a fake `claude` script.
#[cfg(unix)]
#[tokio::test]
#[serial]
async fn web_claude_login_never_opens_a_browser_on_the_server() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let _path = FakeClaudeOnPath::install();
    let router = web_router();

    for body in [json!({}), json!({"loginMethod": "device_code"})] {
        let _ = cc_switch_lib::claude_login_open_browser_test_hook();
        let _ = web_call(
            &router,
            "POST",
            "/api/auth/claude_official/login",
            Some(body.clone()),
        )
        .await;

        assert_eq!(
            cc_switch_lib::claude_login_open_browser_test_hook(),
            Some(false),
            "login body {body}"
        );
    }
}
