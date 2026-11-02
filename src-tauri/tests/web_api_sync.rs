//! `/api/sync/:backend/*` (WEB-11): manual WebDAV / S3 sync through the HTTP layer. This file
//! holds the route-shape tests that need no remote stub (backend dispatch, 400 for an unknown
//! backend with no side effect, errors passed through verbatim); the stub-backed matrix is added
//! on top of it. Settings live in the temp HOME's `settings.json`.
#![cfg(not(target_os = "windows"))]

mod support;

use std::io::{Read, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

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

// ---- Content-Type guard (DP-518) -------------------------------------------------------------

/// `web_call` always sends JSON; this one sends the given Content-Type (or none) and extra
/// headers, so a CORS-simple request can be replayed.
async fn raw_call(
    router: &axum::Router,
    method: &str,
    uri: &str,
    content_type: Option<&str>,
    extra_headers: &[(&str, &str)],
    body: &Value,
) -> (StatusCode, Value) {
    use axum::body::Body;
    use axum::http::{header, Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    let mut req = Request::builder().method(method).uri(uri);
    if let Some(ct) = content_type {
        req = req.header(header::CONTENT_TYPE, ct);
    }
    for (k, v) in extra_headers {
        req = req.header(*k, *v);
    }
    let resp = router
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .expect("router is infallible");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).expect("json body"))
}

/// Loopback WebDAV stub. It records only PROPFIND requests (method, path, Authorization): the
/// host's moshi-hook probes loopback listeners with `GET /`, which must not count.
struct PropfindStub {
    port: u16,
    seen: Arc<Mutex<Vec<(String, String)>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl PropfindStub {
    fn start() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (seen_t, stop_t) = (seen.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            for conn in listener.incoming() {
                if stop_t.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut conn) = conn else { continue };
                conn.set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .ok();
                let mut head = Vec::new();
                let mut buf = [0u8; 1024];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match conn.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&head).to_string();
                let mut lines = head.lines();
                let method = lines
                    .next()
                    .and_then(|l| l.split_whitespace().next())
                    .unwrap_or_default()
                    .to_string();
                if method == "PROPFIND" {
                    let auth = lines
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case("authorization")
                                .then(|| v.trim().to_string())
                        })
                        .unwrap_or_default();
                    seen_t.lock().unwrap().push((method, auth));
                }
                let _ = conn.write_all(
                    b"HTTP/1.1 207 Multi-Status\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });
        Self {
            port,
            seen,
            stop,
            thread: Some(thread),
        }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/dav", self.port)
    }

    /// Authorization headers of the PROPFIND requests received so far.
    fn propfind_auth(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|(_, a)| a.clone())
            .collect()
    }
}

impl Drop for PropfindStub {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Unblock `accept` so the thread sees the flag.
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// `Basic base64("alice:webdav-secret-1")`: the stored credential from `webdav_settings`.
const STORED_BASIC: &str = "Basic YWxpY2U6d2ViZGF2LXNlY3JldC0x";

/// Stores WebDAV settings (with the password) through the save route.
async fn store_webdav(router: &axum::Router, base_url: &str) {
    let (status, json) = web_call(
        router,
        "PUT",
        "/api/sync/webdav/settings",
        Some(json!({ "settings": webdav_settings(base_url), "passwordTouched": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
}

async fn stored_base_url(router: &axum::Router) -> Value {
    let (status, json) = web_call(router, "GET", "/api/settings", None).await;
    assert_eq!(status, StatusCode::OK);
    json["data"]["webdavSync"]["baseUrl"].clone()
}

#[tokio::test]
#[serial]
async fn text_plain_test_request_is_415_and_never_reaches_the_attacker_url() {
    let router = router();
    store_webdav(&router, "http://127.0.0.1:1/dav").await;
    let attacker = PropfindStub::start();
    // The attacker's body: its own URL and an empty password (`preserveEmptyPassword` defaults
    // to true, so the command would fill in the stored one).
    let mut attacker_settings = webdav_settings(&attacker.base_url());
    attacker_settings["password"] = json!("");
    let body = json!({ "settings": attacker_settings });

    // Negative control: as application/json the same body does reach the stub, carrying the
    // stored password. Without it the "nothing arrived" check below proves nothing.
    let (status, json) = raw_call(
        &router,
        "POST",
        "/api/sync/webdav/test",
        Some("application/json"),
        &[],
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(attacker.propfind_auth(), vec![STORED_BASIC.to_string()]);

    // The CORS-simple form of it.
    let (status, json) = raw_call(
        &router,
        "POST",
        "/api/sync/webdav/test",
        Some("text/plain;charset=UTF-8"),
        &[("origin", "https://evil.example")],
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{json}");
    assert_eq!(json["ok"], json!(false));
    // The handler awaits its outbound call before answering, so the count is final here.
    assert_eq!(
        attacker.propfind_auth().len(),
        1,
        "the text/plain request must not trigger a PROPFIND"
    );
}

#[tokio::test]
#[serial]
async fn non_json_content_types_are_415_on_both_body_routes_and_change_nothing() {
    let router = router();
    store_webdav(&router, "http://127.0.0.1:1/dav").await;
    let attacker = PropfindStub::start();
    let body = json!({ "settings": webdav_settings(&attacker.base_url()) });

    let bad: [Option<&str>; 6] = [
        Some("text/plain;charset=UTF-8"),
        Some("application/x-www-form-urlencoded"),
        // JSON only inside a parameter or as a prefix of another subtype.
        Some("text/plain; note=application/json"),
        Some("application/jsonfoo"),
        Some("application/json-seq"),
        None,
    ];
    for ct in bad {
        for (method, uri) in [
            ("POST", "/api/sync/webdav/test"),
            ("PUT", "/api/sync/webdav/settings"),
            ("POST", "/api/sync/s3/test"),
            ("PUT", "/api/sync/s3/settings"),
        ] {
            let (status, json) = raw_call(&router, method, uri, ct, &[], &body).await;
            assert_eq!(
                status,
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "{method} {uri} {ct:?}: {json}"
            );
            assert_eq!(json["ok"], json!(false), "{method} {uri} {ct:?}");
        }
    }

    // The rejected PUTs stored nothing, the rejected POSTs sent nothing.
    assert_eq!(
        stored_base_url(&router).await,
        json!("http://127.0.0.1:1/dav")
    );
    let (_, settings) = web_call(&router, "GET", "/api/settings", None).await;
    assert!(settings["data"].get("s3Sync").is_none(), "{settings}");
    assert!(attacker.propfind_auth().is_empty());
}

#[tokio::test]
#[serial]
async fn json_content_types_with_parameters_or_a_json_suffix_pass_on_both_routes() {
    let router = router();
    let stub = PropfindStub::start();
    let body = json!({ "settings": webdav_settings(&stub.base_url()) });

    let mut expected_requests = 0;
    for ct in [
        "application/json; charset=utf-8",
        "Application/JSON;charset=UTF-8",
        "application/vnd.api+json",
    ] {
        let (status, json) = raw_call(
            &router,
            "POST",
            "/api/sync/webdav/test",
            Some(ct),
            &[],
            &body,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "test {ct}: {json}");
        expected_requests += 1;
        assert_eq!(stub.propfind_auth().len(), expected_requests, "{ct}");

        let (status, json) = raw_call(
            &router,
            "PUT",
            "/api/sync/webdav/settings",
            Some(ct),
            &[],
            &body,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "settings {ct}: {json}");
    }
    assert_eq!(stored_base_url(&router).await, json!(stub.base_url()));
}
