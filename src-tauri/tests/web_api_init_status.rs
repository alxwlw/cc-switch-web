//! `GET /api/init-status` and the init-error router (WEB-32). The error state
//! is produced by the real bootstrap against a too-new database under the test
//! HOME. `init_status` is a process-global that cannot be cleared, so this
//! binary only holds tests that expect the error; the "no error" case is
//! `web_api_init_status_clean.rs`.

mod support;

use axum::http::StatusCode;
use cc_switch_lib::bootstrap::{run_headless_bootstrap, BootstrapError};
use cc_switch_lib::web_api;
use serde_json::json;
use support::{ensure_test_home, reset_test_fs, test_mutex, web_call};

/// A DB whose `user_version` is far above what this build supports, then the
/// real bootstrap: it must refuse it and record the init error.
fn fail_bootstrap_with_too_new_db() {
    reset_test_fs();
    let dir = ensure_test_home().join(".cc-switch");
    std::fs::create_dir_all(&dir).expect("config dir");
    let conn = rusqlite::Connection::open(dir.join("cc-switch.db")).expect("open db");
    conn.execute_batch("PRAGMA user_version = 9999;")
        .expect("set user_version");
    drop(conn);

    match run_headless_bootstrap() {
        Err(BootstrapError::DbVersionTooNew { version, .. }) => assert_eq!(version, 9999),
        Err(other) => panic!("expected DbVersionTooNew, got {other}"),
        Ok(_) => panic!("bootstrap must refuse a too-new database"),
    }
}

#[tokio::test]
async fn degraded_router_reports_the_init_error() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    fail_bootstrap_with_too_new_db();
    let router = web_api::init_error_router();

    let (status, body) = web_call(&router, "GET", "/api/init-status", None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], json!(true));
    let data = &body["data"];
    assert_eq!(data["kind"], json!("db_version_too_new"));
    assert_eq!(data["db_version"], json!(9999));
    assert!(data["supported_version"].as_i64().unwrap() < 9999);
    assert!(data["path"].as_str().unwrap().ends_with("cc-switch.db"));
    assert!(data["error"].as_str().unwrap().contains("9999"));
}

#[tokio::test]
async fn degraded_router_answers_503_with_the_error_text_elsewhere() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    fail_bootstrap_with_too_new_db();
    let router = web_api::init_error_router();
    let (_, status_body) = web_call(&router, "GET", "/api/init-status", None).await;
    let expected = status_body["data"]["error"].clone();

    for (method, uri) in [
        ("GET", "/api/claude/providers"),
        ("POST", "/api/codex/providers/switch"),
        ("GET", "/api/health"),
    ] {
        let (status, body) = web_call(&router, method, uri, None).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{method} {uri}");
        assert_eq!(body, json!({ "ok": false, "error": expected }), "{uri}");
    }
}

#[tokio::test]
async fn degraded_router_keeps_capabilities() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    fail_bootstrap_with_too_new_db();
    let router = web_api::init_error_router();

    let (status, body) = web_call(&router, "GET", "/api/capabilities", None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["mode"], json!("web"));
    assert_eq!(body["version"], json!(env!("CARGO_PKG_VERSION")));
}

#[tokio::test]
async fn full_router_serves_the_same_payload_when_an_error_exists() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    fail_bootstrap_with_too_new_db();
    // The full router needs a state; a fresh HOME gives a readable DB. The
    // init error recorded above stays in the process-global.
    reset_test_fs();
    ensure_test_home();
    let router = support::web_router();

    let (status, body) = web_call(&router, "GET", "/api/init-status", None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["kind"], json!("db_version_too_new"));
}
