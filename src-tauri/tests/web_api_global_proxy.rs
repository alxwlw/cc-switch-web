//! `/api/global-proxy*` (WEB-23). No internet: `POST /test` probes a target served through
//! a local stub proxy (`global_proxy_test_targets_test_hook`), `POST /scan` finds a listener
//! the test binds on one of the well-known proxy ports. Own binary: the `http_client`
//! globals are process-wide, so every test ends with the proxy set back to direct.
#![cfg(not(target_os = "windows"))]

mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};
use serial_test::serial;
use support::{ensure_test_home, reset_test_fs, test_mutex, web_call, web_router};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Clears the process-global test-target seam even when a test panics.
struct HookReset;

impl Drop for HookReset {
    fn drop(&mut self) {
        cc_switch_lib::web_api::global_proxy_test_targets_test_hook(None);
    }
}

async fn call(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, resp) = web_call(router, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(true), "{method} {uri}: {resp}");
    resp["data"].clone()
}

async fn set(router: &axum::Router, url: &str) -> (StatusCode, Value) {
    web_call(
        router,
        "PUT",
        "/api/global-proxy",
        Some(json!({ "url": url })),
    )
    .await
}

/// Proxy stub: answers every request with 200 and closes.
async fn stub_proxy() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = sock.read(&mut buf).await;
                let _ = sock
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await;
            });
        }
    });
    port
}

/// A port nothing listens on.
fn closed_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

#[tokio::test]
#[serial]
async fn set_get_status_and_disable() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();
    let url = "http://127.0.0.1:18561";

    let (status, resp) = set(&router, url).await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(
        call(&router, "GET", "/api/global-proxy", None).await,
        json!(url)
    );
    let st = call(&router, "GET", "/api/global-proxy/status", None).await;
    assert_eq!(st["enabled"], json!(true));
    assert_eq!(st["proxyUrl"], json!(url));

    let (status, resp) = set(&router, "").await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(
        call(&router, "GET", "/api/global-proxy", None).await,
        Value::Null
    );
    let st = call(&router, "GET", "/api/global-proxy/status", None).await;
    assert_eq!(st["enabled"], json!(false));
    assert_eq!(st["proxyUrl"], Value::Null);
}

#[tokio::test]
#[serial]
async fn invalid_url_is_400_and_keeps_the_previous_value() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();
    let url = "http://127.0.0.1:18562";
    assert_eq!(set(&router, url).await.0, StatusCode::OK);

    let (status, resp) = set(&router, "ftp://127.0.0.1:18563").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{resp}");
    assert_eq!(resp["ok"], json!(false));
    assert!(
        resp["error"].as_str().is_some_and(|e| !e.is_empty()),
        "{resp}"
    );

    assert_eq!(
        call(&router, "GET", "/api/global-proxy", None).await,
        json!(url)
    );
    let st = call(&router, "GET", "/api/global-proxy/status", None).await;
    assert_eq!(st["proxyUrl"], json!(url));

    assert_eq!(set(&router, "").await.0, StatusCode::OK);
}

#[tokio::test]
#[serial]
async fn status_masks_credentials_but_get_returns_the_saved_url() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();
    let url = "http://user:secret@127.0.0.1:18564";
    assert_eq!(set(&router, url).await.0, StatusCode::OK);

    let got = call(&router, "GET", "/api/global-proxy", None).await;
    assert_eq!(got, json!(url));
    let st = call(&router, "GET", "/api/global-proxy/status", None).await;
    let shown = st["proxyUrl"].as_str().unwrap();
    assert!(!shown.contains("secret"), "{st}");
    assert!(shown.contains("127.0.0.1:18564"), "{st}");

    assert_eq!(set(&router, "").await.0, StatusCode::OK);
}

#[tokio::test]
#[serial]
async fn test_route_reports_success_via_stub_and_failure_via_closed_port() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let _reset = HookReset;
    let router = web_router();
    cc_switch_lib::web_api::global_proxy_test_targets_test_hook(Some(vec![
        "http://probe.invalid/".to_string(),
    ]));

    let port = stub_proxy().await;
    let ok = call(
        &router,
        "POST",
        "/api/global-proxy/test",
        Some(json!({ "url": format!("http://127.0.0.1:{port}") })),
    )
    .await;
    assert_eq!(ok["success"], json!(true), "{ok}");

    let bad = call(
        &router,
        "POST",
        "/api/global-proxy/test",
        Some(json!({ "url": format!("http://127.0.0.1:{}", closed_port()) })),
    )
    .await;
    assert_eq!(bad["success"], json!(false), "{bad}");
    assert!(
        bad["error"].as_str().is_some_and(|e| !e.is_empty()),
        "{bad}"
    );

    let (status, resp) = web_call(
        &router,
        "POST",
        "/api/global-proxy/test",
        Some(json!({ "url": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{resp}");
    assert_eq!(resp["ok"], json!(false));
}

#[tokio::test]
#[serial]
async fn scan_finds_a_listener_on_a_known_proxy_port() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    // Same list as PROXY_PORTS in commands/global_proxy.rs.
    let mut held = None;
    for port in [7890u16, 7891, 1080, 8080, 8888, 3128, 10808, 10809] {
        if let Ok(l) = std::net::TcpListener::bind(("127.0.0.1", port)) {
            held = Some((port, l));
            break;
        }
    }
    let Some((port, _listener)) = held else {
        eprintln!("SKIP scan test: every well-known proxy port is already taken");
        return;
    };

    let found = call(&router, "POST", "/api/global-proxy/scan", None).await;
    let ports: Vec<u64> = found
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["port"].as_u64().unwrap())
        .collect();
    assert!(ports.contains(&u64::from(port)), "{found}");
}
