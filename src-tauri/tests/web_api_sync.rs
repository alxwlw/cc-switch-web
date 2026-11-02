//! `/api/sync/:backend/*` (WEB-11): manual WebDAV / S3 sync through the HTTP layer. This file
//! holds the route-shape tests that need no remote stub (backend dispatch, 400 for an unknown
//! backend with no side effect, errors passed through verbatim); the stub-backed matrix is added
//! on top of it. Settings live in the temp HOME's `settings.json`.
#![cfg(not(target_os = "windows"))]

mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};
use serial_test::serial;
use support::{create_test_state, ensure_test_home, reset_test_fs, web_call, web_router_with};

fn router() -> axum::Router {
    reset_test_fs();
    ensure_test_home();
    web_router_with(std::sync::Arc::new(create_test_state().unwrap()))
}

/// A localhost port that nothing listens on.
fn closed_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn webdav_settings(base_url: &str) -> Value {
    json!({
        "enabled": true,
        "baseUrl": base_url,
        "username": "alice",
        "password": "webdav-secret-1",
    })
}

fn s3_settings() -> Value {
    json!({
        "enabled": true,
        "bucket": "bucket",
        "region": "us-east-1",
        "accessKeyId": "AKIAEXAMPLE",
        "secretAccessKey": "s3-secret-1",
    })
}

/// One request per route with its own verb, with a body that would be valid for a real backend.
fn routes(backend: &str) -> Vec<(&'static str, String, Option<Value>)> {
    let settings = json!({ "settings": webdav_settings("http://127.0.0.1:1/dav") });
    vec![
        (
            "POST",
            format!("/api/sync/{backend}/test"),
            Some(settings.clone()),
        ),
        ("POST", format!("/api/sync/{backend}/upload"), None),
        ("POST", format!("/api/sync/{backend}/download"), None),
        (
            "PUT",
            format!("/api/sync/{backend}/settings"),
            Some(settings),
        ),
        ("GET", format!("/api/sync/{backend}/remote-info"), None),
    ]
}

#[tokio::test]
#[serial]
async fn unknown_backend_is_400_on_all_five_routes_and_writes_nothing() {
    let router = router();
    for (method, uri, body) in routes("ftp") {
        let (status, json) = web_call(&router, method, &uri, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {uri}");
        assert_eq!(json["ok"], json!(false), "{method} {uri}");
        assert_eq!(
            json["error"],
            json!("unknown sync backend: ftp (expected webdav or s3)"),
            "{method} {uri}"
        );
    }

    // The save route with a valid body stored nothing under either backend.
    let (status, json) = web_call(&router, "GET", "/api/settings", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["data"].get("webdavSync").is_none(), "{json}");
    assert!(json["data"].get("s3Sync").is_none(), "{json}");
}

#[tokio::test]
#[serial]
async fn unknown_backend_is_400_even_without_a_body() {
    let router = router();
    // `test` and `settings` take a body; the backend is checked before it is read.
    for (method, uri) in [
        ("POST", "/api/sync/ftp/test"),
        ("PUT", "/api/sync/ftp/settings"),
    ] {
        let (status, json) = web_call(&router, method, uri, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {uri}");
        assert_eq!(
            json,
            json!({ "ok": false, "error": "unknown sync backend: ftp (expected webdav or s3)" }),
            "{method} {uri}"
        );
    }
}

#[tokio::test]
#[serial]
async fn test_against_a_closed_port_answers_ok_false() {
    let router = router();
    let base_url = format!("http://127.0.0.1:{}/dav", closed_port());
    let (status, json) = web_call(
        &router,
        "POST",
        "/api/sync/webdav/test",
        Some(json!({ "settings": webdav_settings(&base_url) })),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(json["ok"], json!(false));
    assert!(
        !json["error"].as_str().unwrap_or_default().is_empty(),
        "{json}"
    );
}

#[tokio::test]
#[serial]
async fn each_backend_reaches_its_own_command_with_the_error_text_verbatim() {
    let router = router();
    for (backend, expected) in [
        (
            "webdav",
            "未配置 WebDAV 同步 (WebDAV sync is not configured.)",
        ),
        ("s3", "未配置 S3 同步 (S3 sync is not configured.)"),
    ] {
        for (method, uri) in [
            ("POST", format!("/api/sync/{backend}/upload")),
            ("POST", format!("/api/sync/{backend}/download")),
            ("GET", format!("/api/sync/{backend}/remote-info")),
        ] {
            let (status, json) = web_call(&router, method, &uri, None).await;
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{method} {uri}");
            assert_eq!(
                json,
                json!({ "ok": false, "error": expected }),
                "{method} {uri}"
            );
        }
    }
}

#[tokio::test]
#[serial]
async fn save_runs_the_backend_validation_and_returns_its_error_verbatim() {
    let router = router();
    let (status, json) = web_call(
        &router,
        "PUT",
        "/api/sync/webdav/settings",
        Some(json!({ "settings": webdav_settings("   ") })),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json,
        json!({ "ok": false, "error": "WebDAV 地址不能为空 (WebDAV URL is required.)" })
    );

    let mut s3 = s3_settings();
    s3["bucket"] = json!("");
    let (status, json) = web_call(
        &router,
        "PUT",
        "/api/sync/s3/settings",
        Some(json!({ "settings": s3 })),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json,
        json!({ "ok": false, "error": "S3 存储桶不能为空 (S3 bucket is required.)" })
    );
}

#[tokio::test]
#[serial]
async fn malformed_settings_body_is_400() {
    let router = router();
    let (status, json) = web_call(
        &router,
        "PUT",
        "/api/sync/webdav/settings",
        Some(json!({ "settings": { "enabled": "yes" } })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["ok"], json!(false));
    assert!(
        json["error"]
            .as_str()
            .unwrap_or_default()
            .starts_with("invalid sync settings:"),
        "{json}"
    );
}
