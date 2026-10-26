#[allow(dead_code)]
mod support;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use cc_switch_lib::{update_settings, AppSettings};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use serial_test::serial;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use support::{create_test_state, ensure_test_home, reset_test_fs, web_call, web_router_with};
use tower::ServiceExt;

/// Router over a fresh state with `hermes_config_dir` pointing at a temp dir.
fn fresh() -> (axum::Router, PathBuf) {
    reset_test_fs();
    let home = ensure_test_home();
    let dir = home.join(".hermes-web-api");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("hermes dir");
    update_settings(AppSettings {
        hermes_config_dir: Some(dir.to_string_lossy().into_owned()),
        ..AppSettings::default()
    })
    .expect("hermes dir override");
    std::env::remove_var("HERMES_WEB_PORT");
    (
        web_router_with(Arc::new(create_test_state().expect("state"))),
        dir,
    )
}

async fn data(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, resp) = web_call(router, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(true), "{method} {uri}: {resp}");
    resp["data"].clone()
}

const CONFIG: &str = r#"custom_providers:
  - name: alpha
    base_url: https://a.example.com/v1
    api_key: sk-a
  - name: beta
    base_url: https://b.example.com/v1
    api_key: sk-b
model:
  default: gpt-4
  provider: alpha
memory:
  memory_char_limit: 500
  user_char_limit: 300
  memory_enabled: true
  user_profile_enabled: true
"#;

#[tokio::test]
#[serial]
async fn memory_write_then_read_is_atomic() {
    let (router, dir) = fresh();
    let uri = "/api/hermes/memory/memory";
    assert_eq!(data(&router, "GET", uri, None).await, json!(""));
    data(&router, "PUT", uri, Some(json!({"content": "hello\nмир"}))).await;
    assert_eq!(data(&router, "GET", uri, None).await, json!("hello\nмир"));
    // the other kind is untouched
    assert_eq!(
        data(&router, "GET", "/api/hermes/memory/user", None).await,
        json!("")
    );
    let names: Vec<String> = fs::read_dir(dir.join("memories"))
        .expect("memories dir")
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["MEMORY.md".to_string()], "tmp leftovers");
}

#[tokio::test]
#[serial]
async fn unknown_memory_kind_is_400() {
    let (router, _dir) = fresh();
    for (method, uri, body) in [
        ("GET", "/api/hermes/memory/bogus", None),
        (
            "PUT",
            "/api/hermes/memory/bogus",
            Some(json!({"content": "x"})),
        ),
        (
            "PUT",
            "/api/hermes/memory/bogus/enabled",
            Some(json!({"enabled": true})),
        ),
    ] {
        let (status, resp) = web_call(&router, method, uri, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {uri}: {resp}");
        assert_eq!(resp["ok"], json!(false));
    }
}

#[tokio::test]
#[serial]
async fn enabled_toggles_only_its_own_flag_and_limits_report_it() {
    let (router, dir) = fresh();
    fs::write(dir.join("config.yaml"), CONFIG).expect("seed");

    let limits = data(&router, "GET", "/api/hermes/memory-limits", None).await;
    assert_eq!(
        limits,
        json!({"memory": 500, "user": 300, "memoryEnabled": true, "userEnabled": true})
    );

    data(
        &router,
        "PUT",
        "/api/hermes/memory/user/enabled",
        Some(json!({"enabled": false})),
    )
    .await;
    let limits = data(&router, "GET", "/api/hermes/memory-limits", None).await;
    assert_eq!(
        limits,
        json!({"memory": 500, "user": 300, "memoryEnabled": true, "userEnabled": false})
    );

    data(
        &router,
        "PUT",
        "/api/hermes/memory/memory/enabled",
        Some(json!({"enabled": false})),
    )
    .await;
    let limits = data(&router, "GET", "/api/hermes/memory-limits", None).await;
    assert_eq!(limits["memoryEnabled"], json!(false));
    assert_eq!(limits["userEnabled"], json!(false));
    // the rest of config.yaml survives
    let text = fs::read_to_string(dir.join("config.yaml")).unwrap();
    assert!(text.contains("custom_providers"), "{text}");
    assert!(text.contains("memory_char_limit"), "{text}");
}

#[tokio::test]
#[serial]
async fn import_live_imports_each_provider_and_lists_ids() {
    let (router, dir) = fresh();
    fs::write(dir.join("config.yaml"), CONFIG).expect("seed");

    let ids = data(&router, "GET", "/api/hermes/live-provider-ids", None).await;
    assert_eq!(ids, json!(["alpha", "beta"]));
    assert_eq!(
        data(&router, "POST", "/api/hermes/import-live", None).await,
        json!(2)
    );
    let model = data(&router, "GET", "/api/hermes/model-config", None).await;
    assert_eq!(model["default"], json!("gpt-4"));
    assert_eq!(model["provider"], json!("alpha"));
}

#[tokio::test]
#[serial]
async fn model_config_is_null_without_model_section() {
    let (router, _dir) = fresh();
    assert_eq!(
        data(&router, "GET", "/api/hermes/model-config", None).await,
        Value::Null
    );
}

/// Loopback stub that answers every request with 401 (what Hermes does
/// without a session token); returns its port.
async fn spawn_stub() -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stub");
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await;
                let _ = sock
                    .write_all(b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                    .await;
            });
        }
    });
    port
}

async fn web_ui(router: &axum::Router, host: Option<&str>, query: &str) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method("GET")
        .uri(format!("/api/hermes/web-ui{query}"));
    if let Some(h) = host {
        req = req.header(header::HOST, h);
    }
    let resp = router
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .expect("infallible");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).expect("json"))
}

#[tokio::test]
#[serial]
async fn web_ui_reachable_uses_request_host_and_hermes_port() {
    let (router, _dir) = fresh();
    let port = spawn_stub().await;
    std::env::set_var("HERMES_WEB_PORT", port.to_string());

    let (status, resp) = web_ui(&router, Some("box.tail.ts.net:8080"), "?path=/config").await;
    std::env::remove_var("HERMES_WEB_PORT");
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(resp["data"]["reachable"], json!(true));
    assert_eq!(
        resp["data"]["url"],
        json!(format!("http://box.tail.ts.net:{port}/config"))
    );
}

#[tokio::test]
#[serial]
async fn web_ui_unreachable_without_listener() {
    let (router, _dir) = fresh();
    // a port that was just free: bind, read, drop
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    std::env::set_var("HERMES_WEB_PORT", port.to_string());
    let (status, resp) = web_ui(&router, Some("box.example"), "").await;
    std::env::remove_var("HERMES_WEB_PORT");
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(resp["data"]["reachable"], json!(false));
    assert_eq!(
        resp["data"]["url"],
        json!(format!("http://box.example:{port}/"))
    );
}

#[tokio::test]
#[serial]
async fn web_ui_path_cannot_become_the_host() {
    let (router, _dir) = fresh();
    std::env::set_var("HERMES_WEB_PORT", "9119");
    let mut urls = Vec::new();
    for q in [
        "?path=//x",
        "?path=///evil.example/a",
        "?path=http://evil.example",
        "?path=/%5Cevil.example",
        "?path=x",
    ] {
        let (status, resp) = web_ui(&router, Some("h.example"), q).await;
        assert_eq!(status, StatusCode::OK, "{q}: {resp}");
        urls.push(resp["data"]["url"].as_str().unwrap().to_string());
    }
    std::env::remove_var("HERMES_WEB_PORT");
    assert_eq!(
        urls,
        vec![
            "http://h.example:9119/x",
            "http://h.example:9119/evil.example/a",
            "http://h.example:9119/http://evil.example",
            "http://h.example:9119/evil.example",
            "http://h.example:9119/x",
        ]
    );
}

#[tokio::test]
#[serial]
async fn web_ui_rejects_missing_or_hostile_host() {
    let (router, _dir) = fresh();
    for host in [None, Some("evil.example/x"), Some("a@b"), Some("")] {
        let (status, resp) = web_ui(&router, host, "").await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{host:?}: {resp}");
    }
    // IPv6 literal keeps its brackets, port is replaced
    std::env::set_var("HERMES_WEB_PORT", "9119");
    let (_, resp) = web_ui(&router, Some("[fd7a::1]:3000"), "").await;
    std::env::remove_var("HERMES_WEB_PORT");
    assert_eq!(resp["data"]["url"], json!("http://[fd7a::1]:9119/"));
}
