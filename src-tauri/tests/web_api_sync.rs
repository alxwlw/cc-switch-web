//! `/api/sync/:backend/*` (WEB-11): manual WebDAV / S3 sync through the HTTP layer. Route-shape
//! tests (backend dispatch, 400 for an unknown backend with no side effect, errors passed
//! through verbatim, the Content-Type guard) sit beside a stub-backed matrix: an in-memory WebDAV
//! server on loopback stands in for the remote. S3 is covered on its error path and on how
//! `test` resolves the secret; the SigV4 happy path is a manual check (phase 33 Task 4).
//! Settings live in the temp HOME's `.cc-switch/settings.json`.
#![cfg(not(target_os = "windows"))]

mod support;

use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, Response, StatusCode};
use cc_switch_lib::{AppType, MultiAppConfig, Provider};
use serde_json::{json, Value};
use serial_test::serial;
use support::{
    create_test_state, create_test_state_with_config, ensure_test_home, reset_test_fs, web_call,
    web_router_with,
};

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
async fn test_against_a_closed_port_answers_ok_false_with_the_services_own_text() {
    let router = router();
    let port = closed_port();
    let base_url = format!("http://127.0.0.1:{port}/dav");
    let started = Instant::now();
    let (status, json) = web_call(
        &router,
        "POST",
        "/api/sync/webdav/test",
        Some(json!({ "settings": webdav_settings(&base_url) })),
    )
    .await;
    let elapsed = started.elapsed();
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    // The text `services/webdav.rs` builds for a refused connection: only a real PROPFIND
    // against the closed port can produce it, a canned failure cannot.
    let target = format!("http://127.0.0.1:{port}/dav");
    assert_eq!(
        json,
        json!({
            "ok": false,
            "error": format!(
                "WebDAV 连接失败（连接失败）: {target} (WebDAV connection failed (connection failed): {target})"
            ),
        })
    );
    // A refused connection answers at once; the service's request timeout is 30 s.
    assert!(
        elapsed < Duration::from_secs(10),
        "closed port took {elapsed:?}"
    );
}

#[tokio::test]
#[serial]
async fn s3_test_against_a_closed_port_answers_ok_false_with_the_services_own_text() {
    let router = router();
    let mut settings = s3_settings();
    settings["endpoint"] = json!(format!("http://127.0.0.1:{}", closed_port()));
    let started = Instant::now();
    let (status, json) = web_call(
        &router,
        "POST",
        "/api/sync/s3/test",
        Some(json!({ "settings": settings })),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json,
        json!({
            "ok": false,
            "error": "S3 连接失败（连接失败） (S3 connection failed (connection failed))",
        })
    );
    assert!(started.elapsed() < Duration::from_secs(10));
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

/// One request the stub received.
#[derive(Clone, Debug)]
struct Seen {
    method: String,
    path: String,
    /// `Authorization` header, empty when absent.
    auth: String,
    /// `x-amz-date` header (S3), empty when absent.
    amz_date: String,
}

struct StubState {
    /// In-memory remote: request path -> stored body (PUT writes it, GET / HEAD read it).
    files: Mutex<BTreeMap<String, Vec<u8>>>,
    seen: Mutex<Vec<Seen>>,
    /// Held by every PUT / GET / HEAD before it answers; widens the window in which two
    /// unserialised operations would overlap. No assertion depends on it elapsing.
    transfer_delay: Duration,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
}

struct InFlight<'a>(&'a StubState);

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Path-style S3 bucket URL of `s3_settings()` against a custom endpoint: `HEAD /bucket/`.
const S3_BUCKET_PATH: &str = "/bucket/";

/// The moshi-hook probes loopback listeners with `GET /` about every 16 s: that request is
/// answered 404 and never recorded. Everything else is, and the assertions filter by method
/// and path.
async fn stub_handler(State(st): State<Arc<StubState>>, req: Request<Body>) -> Response<Body> {
    let (parts, body) = req.into_parts();
    let method = parts.method.as_str().to_string();
    let path = parts.uri.path().to_string();
    let reply = |status: StatusCode| Response::builder().status(status);
    if method == "GET" && path == "/" {
        return reply(StatusCode::NOT_FOUND).body(Body::empty()).unwrap();
    }
    let header_of = |name: &str| {
        parts
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string()
    };
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .unwrap_or_default();
    st.seen.lock().unwrap().push(Seen {
        method: method.clone(),
        path: path.clone(),
        auth: header_of("authorization"),
        amz_date: header_of("x-amz-date"),
    });

    let now = st.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
    st.max_in_flight.fetch_max(now, Ordering::SeqCst);
    let _in_flight = InFlight(&st);
    if matches!(method.as_str(), "PUT" | "GET" | "HEAD") && !st.transfer_delay.is_zero() {
        tokio::time::sleep(st.transfer_delay).await;
    }

    let stored = st.files.lock().unwrap().get(&path).cloned();
    match method.as_str() {
        "PROPFIND" => reply(StatusCode::MULTI_STATUS).body(Body::empty()),
        "MKCOL" => reply(StatusCode::CREATED).body(Body::empty()),
        "PUT" => {
            st.files.lock().unwrap().insert(path, bytes.to_vec());
            reply(StatusCode::CREATED).body(Body::empty())
        }
        "GET" => match stored {
            Some(b) => reply(StatusCode::OK)
                .header("etag", "\"stub-etag\"")
                .body(Body::from(b)),
            None => reply(StatusCode::NOT_FOUND).body(Body::empty()),
        },
        "HEAD" if stored.is_some() || path == S3_BUCKET_PATH => reply(StatusCode::OK)
            .header("etag", "\"stub-etag\"")
            .body(Body::empty()),
        "HEAD" => reply(StatusCode::NOT_FOUND).body(Body::empty()),
        _ => reply(StatusCode::METHOD_NOT_ALLOWED).body(Body::empty()),
    }
    .unwrap()
}

/// In-memory WebDAV server on loopback (also answers an S3 `HEAD /bucket/`), speaking the
/// methods `services/webdav.rs` uses: PROPFIND, MKCOL, PUT, GET, HEAD.
struct DavStub {
    port: u16,
    state: Arc<StubState>,
    task: tokio::task::JoinHandle<()>,
}

impl DavStub {
    async fn start() -> Self {
        Self::start_with_delay(Duration::ZERO).await
    }

    async fn start_with_delay(transfer_delay: Duration) -> Self {
        let state = Arc::new(StubState {
            files: Mutex::new(BTreeMap::new()),
            seen: Mutex::new(Vec::new()),
            transfer_delay,
            in_flight: AtomicUsize::new(0),
            max_in_flight: AtomicUsize::new(0),
        });
        let app = axum::Router::new()
            .fallback(stub_handler)
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { port, state, task }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/dav", self.port)
    }

    /// S3 `endpoint` value pointing at the stub (path style, plain http).
    fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn requests(&self, method: &str) -> Vec<Seen> {
        self.state
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.method == method)
            .cloned()
            .collect()
    }

    /// Authorization headers of the `PROPFIND /dav` requests received so far.
    fn propfind_auth(&self) -> Vec<String> {
        self.requests("PROPFIND")
            .into_iter()
            .filter(|r| r.path == "/dav")
            .map(|r| r.auth)
            .collect()
    }

    /// `(method, path)` of everything under `/dav`, in arrival order.
    fn dav_log(&self) -> Vec<(String, String)> {
        self.state
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path == "/dav" || r.path.starts_with("/dav/"))
            .map(|r| (r.method.clone(), r.path.clone()))
            .collect()
    }

    fn file(&self, path: &str) -> Option<Vec<u8>> {
        self.state.files.lock().unwrap().get(path).cloned()
    }

    fn forget_requests(&self) {
        self.state.seen.lock().unwrap().clear();
        self.state.max_in_flight.store(0, Ordering::SeqCst);
    }

    fn max_in_flight(&self) -> usize {
        self.state.max_in_flight.load(Ordering::SeqCst)
    }
}

impl Drop for DavStub {
    fn drop(&mut self) {
        self.task.abort();
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
    let attacker = DavStub::start().await;
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
    let attacker = DavStub::start().await;
    let body = json!({ "settings": webdav_settings(&attacker.base_url()) });

    let bad: [Option<&str>; 8] = [
        Some("text/plain;charset=UTF-8"),
        Some("application/x-www-form-urlencoded"),
        // The `json` / `+json` subtype alone is not enough: the type must be `application`.
        Some("text/json"),
        Some("text/x+json"),
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
    let stub = DavStub::start().await;
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

// ---- Stub-backed matrix (WEB-11 Task 2) ---------------------------------------------------------

/// `Basic base64("alice:")`: the credential when the password is empty and not filled in.
const EMPTY_PASSWORD_BASIC: &str = "Basic YWxpY2U6";

/// Where `webdav_settings` (default root and profile) keeps its files on the stub.
const REMOTE_DIR: &str = "/dav/cc-switch-sync/v2/db-v6/default";

/// The request settled with `200 {ok:true,data}`; returns `data`.
async fn data(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, json) = web_call(router, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {json}");
    assert_eq!(json["ok"], json!(true), "{method} {uri}: {json}");
    json["data"].clone()
}

/// What the save route persisted, unmasked (`GET /api/settings` clears the secrets).
fn stored_settings() -> Value {
    let path = ensure_test_home().join(".cc-switch").join("settings.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("settings.json")).expect("json")
}

struct Fixture {
    router: axum::Router,
    state: Arc<cc_switch_lib::AppState>,
}

/// Fresh HOME with two Claude providers, `old-provider` current.
fn fixture_with_providers() -> Fixture {
    reset_test_fs();
    ensure_test_home();
    let mut config = MultiAppConfig::default();
    let manager = config.get_manager_mut(&AppType::Claude).unwrap();
    manager.current = "old-provider".to_string();
    for (id, key) in [("old-provider", "k-old"), ("new-provider", "k-new")] {
        manager.providers.insert(
            id.to_string(),
            Provider::with_id(
                id.to_string(),
                id.to_string(),
                json!({ "env": { "ANTHROPIC_API_KEY": key } }),
                None,
            ),
        );
    }
    let state = Arc::new(create_test_state_with_config(&config).expect("state"));
    Fixture {
        router: web_router_with(state.clone()),
        state,
    }
}

fn db_key(fx: &Fixture, id: &str) -> Option<String> {
    fx.state
        .db
        .get_provider_by_id(id, "claude")
        .expect("db read")
        .map(|p| {
            p.settings_config["env"]["ANTHROPIC_API_KEY"]
                .as_str()
                .expect("key")
                .to_string()
        })
}

fn live_claude_path() -> std::path::PathBuf {
    ensure_test_home().join(".claude").join("settings.json")
}

fn write_live_claude_key(key: &str) {
    let path = live_claude_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        json!({ "env": { "ANTHROPIC_API_KEY": key } }).to_string(),
    )
    .unwrap();
}

fn live_claude_key() -> String {
    let v: Value =
        serde_json::from_str(&std::fs::read_to_string(live_claude_path()).expect("live settings"))
            .expect("live settings json");
    v["env"]["ANTHROPIC_API_KEY"]
        .as_str()
        .expect("live key")
        .to_string()
}

fn s3_settings_for(stub: &DavStub) -> Value {
    let mut settings = s3_settings();
    settings["endpoint"] = json!(stub.endpoint());
    settings
}

#[tokio::test]
#[serial]
async fn test_against_the_stub_is_ok_and_creates_the_remote_directories() {
    let router = router();
    let stub = DavStub::start().await;
    let (status, json) = web_call(
        &router,
        "POST",
        "/api/sync/webdav/test",
        Some(json!({ "settings": webdav_settings(&stub.base_url()) })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(
        json,
        json!({ "ok": true, "data": { "success": true, "message": "WebDAV connection ok" } })
    );
    // Nothing was stored, so the password in the body is the one that was sent.
    assert_eq!(stub.propfind_auth(), vec![STORED_BASIC.to_string()]);
    let mkcol: Vec<String> = stub
        .dav_log()
        .into_iter()
        .filter(|(m, _)| m == "MKCOL")
        .map(|(_, p)| p)
        .collect();
    assert_eq!(
        mkcol,
        [
            "/dav/cc-switch-sync/",
            "/dav/cc-switch-sync/v2/",
            "/dav/cc-switch-sync/v2/db-v6/",
            "/dav/cc-switch-sync/v2/db-v6/default/",
        ]
    );
}

#[tokio::test]
#[serial]
async fn webdav_test_fills_an_empty_password_only_when_preserve_empty_password_is_not_false() {
    let router = router();
    let stub = DavStub::start().await;
    store_webdav(&router, &stub.base_url()).await;

    let mut empty = webdav_settings(&stub.base_url());
    empty["password"] = json!("");
    let mut typed = webdav_settings(&stub.base_url());
    typed["password"] = json!("typed-secret-2");

    // (settings, preserveEmptyPassword, Authorization the stub must see)
    let cases = [
        (empty.clone(), None, STORED_BASIC.to_string()),
        (empty.clone(), Some(true), STORED_BASIC.to_string()),
        (empty.clone(), Some(false), EMPTY_PASSWORD_BASIC.to_string()),
        // A typed password is never replaced by the stored one.
        (
            typed.clone(),
            Some(true),
            // base64("alice:typed-secret-2")
            "Basic YWxpY2U6dHlwZWQtc2VjcmV0LTI=".to_string(),
        ),
    ];
    for (settings, flag, expected) in cases {
        stub.forget_requests();
        let mut body = json!({ "settings": settings });
        if let Some(flag) = flag {
            body["preserveEmptyPassword"] = json!(flag);
        }
        let (status, json) = web_call(&router, "POST", "/api/sync/webdav/test", Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{flag:?}: {json}");
        assert_eq!(stub.propfind_auth(), vec![expected], "{flag:?}");
    }
}

#[tokio::test]
#[serial]
async fn webdav_save_masks_the_password_and_password_touched_controls_the_overwrite() {
    let router = router();
    let url = "http://127.0.0.1:1/dav";
    store_webdav(&router, url).await;

    let masked = data(&router, "GET", "/api/settings", None).await;
    assert_eq!(masked["webdavSync"]["baseUrl"], json!(url));
    assert_eq!(masked["webdavSync"]["username"], json!("alice"));
    assert_eq!(masked["webdavSync"]["enabled"], json!(true));
    assert_eq!(masked["webdavSync"]["password"], json!(""), "{masked}");
    assert_eq!(
        stored_settings()["webdavSync"]["password"],
        json!("webdav-secret-1")
    );

    // An empty password with `passwordTouched` false or absent keeps the stored one while the
    // rest of the settings are saved.
    for touched in [Some(false), None] {
        let mut settings = webdav_settings(url);
        settings["password"] = json!("");
        settings["username"] = json!(format!("user-{touched:?}"));
        let mut body = json!({ "settings": settings });
        if let Some(touched) = touched {
            body["passwordTouched"] = json!(touched);
        }
        let saved = data(&router, "PUT", "/api/sync/webdav/settings", Some(body)).await;
        assert_eq!(saved, json!({ "success": true }));
        let on_disk = &stored_settings()["webdavSync"];
        assert_eq!(on_disk["password"], json!("webdav-secret-1"), "{touched:?}");
        assert_eq!(on_disk["username"], json!(format!("user-{touched:?}")));
    }

    // Touched: the typed value replaces it, and so does an explicit empty one.
    for (typed, expected) in [("new-secret-2", "new-secret-2"), ("", "")] {
        let mut settings = webdav_settings(url);
        settings["password"] = json!(typed);
        data(
            &router,
            "PUT",
            "/api/sync/webdav/settings",
            Some(json!({ "settings": settings, "passwordTouched": true })),
        )
        .await;
        assert_eq!(stored_settings()["webdavSync"]["password"], json!(expected));
        let masked = data(&router, "GET", "/api/settings", None).await;
        assert_eq!(masked["webdavSync"]["password"], json!(""), "{masked}");
    }
}

#[tokio::test]
#[serial]
async fn s3_save_masks_the_secret_and_password_touched_controls_the_overwrite() {
    let router = router();
    let save = |settings: Value, touched: Option<bool>| {
        let mut body = json!({ "settings": settings });
        if let Some(touched) = touched {
            body["passwordTouched"] = json!(touched);
        }
        data(&router, "PUT", "/api/sync/s3/settings", Some(body))
    };

    assert_eq!(
        save(s3_settings(), Some(true)).await,
        json!({ "success": true })
    );
    let masked = data(&router, "GET", "/api/settings", None).await;
    assert_eq!(masked["s3Sync"]["bucket"], json!("bucket"));
    assert_eq!(masked["s3Sync"]["region"], json!("us-east-1"));
    assert_eq!(masked["s3Sync"]["accessKeyId"], json!("AKIAEXAMPLE"));
    assert_eq!(masked["s3Sync"]["secretAccessKey"], json!(""), "{masked}");
    assert_eq!(
        stored_settings()["s3Sync"]["secretAccessKey"],
        json!("s3-secret-1")
    );

    for touched in [Some(false), None] {
        let mut settings = s3_settings();
        settings["secretAccessKey"] = json!("");
        settings["bucket"] = json!(format!("bucket-{touched:?}").to_lowercase());
        save(settings, touched).await;
        let on_disk = &stored_settings()["s3Sync"];
        assert_eq!(
            on_disk["secretAccessKey"],
            json!("s3-secret-1"),
            "{touched:?}"
        );
        assert_eq!(
            on_disk["bucket"],
            json!(format!("bucket-{touched:?}").to_lowercase())
        );
    }

    // Touched with an empty secret is refused by the validation, not stored.
    let mut empty = s3_settings();
    empty["secretAccessKey"] = json!("");
    let (status, json) = web_call(
        &router,
        "PUT",
        "/api/sync/s3/settings",
        Some(json!({ "settings": empty, "passwordTouched": true })),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json["error"],
        json!("S3 Secret Access Key 不能为空 (S3 Secret Access Key is required.)")
    );
    assert_eq!(
        stored_settings()["s3Sync"]["secretAccessKey"],
        json!("s3-secret-1")
    );

    let mut typed = s3_settings();
    typed["secretAccessKey"] = json!("s3-secret-2");
    save(typed, Some(true)).await;
    assert_eq!(
        stored_settings()["s3Sync"]["secretAccessKey"],
        json!("s3-secret-2")
    );
}

/// One `POST /api/sync/s3/test` against the stub; returns the `(x-amz-date, Authorization)`
/// of the single `HEAD /bucket/` it sent.
async fn s3_test_signature(
    router: &axum::Router,
    stub: &DavStub,
    secret: &str,
    preserve: Option<bool>,
) -> (String, String) {
    let mut settings = s3_settings_for(stub);
    settings["secretAccessKey"] = json!(secret);
    let mut body = json!({ "settings": settings });
    if let Some(preserve) = preserve {
        body["preserveEmptyPassword"] = json!(preserve);
    }
    stub.forget_requests();
    let resp = data(router, "POST", "/api/sync/s3/test", Some(body)).await;
    assert_eq!(
        resp,
        json!({ "success": true, "message": "S3 connection ok" })
    );
    let heads = stub.requests("HEAD");
    assert_eq!(heads.len(), 1, "{heads:?}");
    assert_eq!(heads[0].path, S3_BUCKET_PATH);
    (heads[0].amz_date.clone(), heads[0].auth.clone())
}

#[tokio::test]
#[serial]
async fn s3_test_fills_an_empty_secret_only_when_preserve_empty_password_is_not_false() {
    let router = router();
    let stub = DavStub::start().await;
    data(
        &router,
        "PUT",
        "/api/sync/s3/settings",
        Some(json!({ "settings": s3_settings_for(&stub), "passwordTouched": true })),
    )
    .await;

    // Flag false and an empty secret: the validation refuses it and nothing is sent.
    stub.forget_requests();
    let mut empty = s3_settings_for(&stub);
    empty["secretAccessKey"] = json!("");
    let (status, json) = web_call(
        &router,
        "POST",
        "/api/sync/s3/test",
        Some(json!({ "settings": empty, "preserveEmptyPassword": false })),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{json}");
    assert_eq!(
        json,
        json!({
            "ok": false,
            "error": "S3 Secret Access Key 不能为空 (S3 Secret Access Key is required.)"
        })
    );
    assert!(stub.requests("HEAD").is_empty());

    // Flag true or absent: the stored secret is used. The SigV4 signature depends on the
    // secret and on the second-resolution `x-amz-date`, so the calls are compared when they
    // share a date (a second boundary between them only means another attempt).
    for _ in 0..10 {
        let filled = s3_test_signature(&router, &stub, "", Some(true)).await;
        let defaulted = s3_test_signature(&router, &stub, "", None).await;
        let typed_stored = s3_test_signature(&router, &stub, "s3-secret-1", Some(false)).await;
        let typed_other = s3_test_signature(&router, &stub, "other-secret-3", Some(true)).await;
        if [&defaulted, &typed_stored, &typed_other]
            .iter()
            .any(|s| s.0 != filled.0)
        {
            continue;
        }
        assert_eq!(
            filled.1, typed_stored.1,
            "preserve true did not use the stored secret"
        );
        assert_eq!(
            defaulted.1, typed_stored.1,
            "absent flag did not default to preserve"
        );
        // A typed secret is never replaced by the stored one.
        assert_ne!(typed_other.1, typed_stored.1);
        return;
    }
    panic!("four consecutive test calls never shared one x-amz-date");
}

#[tokio::test]
#[serial]
async fn upload_puts_the_snapshot_archive_to_the_stub() {
    use sha2::{Digest, Sha256};

    let fx = fixture_with_providers();
    let stub = DavStub::start().await;
    store_webdav(&fx.router, &stub.base_url()).await;

    let resp = data(&fx.router, "POST", "/api/sync/webdav/upload", None).await;
    assert_eq!(resp, json!({ "status": "uploaded" }));

    let db_sql = stub.file(&format!("{REMOTE_DIR}/db.sql")).expect("db.sql");
    let db_sql_text = String::from_utf8(db_sql.clone()).expect("utf-8 dump");
    for needle in ["old-provider", "new-provider", "k-old", "k-new"] {
        assert!(db_sql_text.contains(needle), "db.sql lacks {needle}");
    }
    let skills = stub
        .file(&format!("{REMOTE_DIR}/skills.zip"))
        .expect("skills.zip");
    assert_eq!(&skills[..2], b"PK", "skills.zip is a zip");
    let manifest: Value = serde_json::from_slice(
        &stub
            .file(&format!("{REMOTE_DIR}/manifest.json"))
            .expect("manifest"),
    )
    .expect("manifest json");
    let artifacts = manifest["artifacts"].as_object().expect("artifacts");
    assert_eq!(
        artifacts.keys().map(String::as_str).collect::<Vec<_>>(),
        ["db.sql", "skills.zip"]
    );
    assert_eq!(
        artifacts["db.sql"]["sha256"],
        json!(format!("{:x}", Sha256::digest(&db_sql)))
    );
    assert_eq!(artifacts["db.sql"]["size"], json!(db_sql.len()));
    assert_eq!(
        artifacts["skills.zip"]["sha256"],
        json!(format!("{:x}", Sha256::digest(&skills)))
    );

    // Artifacts first, manifest last; every request carried the stored credential.
    let puts: Vec<String> = stub
        .dav_log()
        .into_iter()
        .filter(|(m, _)| m == "PUT")
        .map(|(_, p)| p)
        .collect();
    assert_eq!(
        puts,
        [
            format!("{REMOTE_DIR}/db.sql"),
            format!("{REMOTE_DIR}/skills.zip"),
            format!("{REMOTE_DIR}/manifest.json"),
        ]
    );
    let puts = stub.requests("PUT");
    assert!(puts.iter().all(|r| r.auth == STORED_BASIC), "{puts:?}");

    // The success is recorded in the (unmasked) settings, the secret untouched.
    let on_disk = &stored_settings()["webdavSync"];
    assert!(on_disk["status"]["lastSyncAt"].is_i64(), "{on_disk}");
    assert_eq!(on_disk["password"], json!("webdav-secret-1"));
}

#[tokio::test]
#[serial]
async fn download_restores_the_providers_and_projects_them_into_the_live_file() {
    let fx = fixture_with_providers();
    let stub = DavStub::start().await;
    store_webdav(&fx.router, &stub.base_url()).await;
    data(&fx.router, "POST", "/api/sync/webdav/upload", None).await;

    // Local drift after the upload: one provider gone, one changed, a stale live file.
    fx.state
        .db
        .delete_provider("claude", "new-provider")
        .unwrap();
    let mut drifted = fx
        .state
        .db
        .get_provider_by_id("old-provider", "claude")
        .unwrap()
        .unwrap();
    drifted.settings_config = json!({ "env": { "ANTHROPIC_API_KEY": "k-drift" } });
    fx.state.db.save_provider("claude", &drifted).unwrap();
    write_live_claude_key("k-stale");
    assert_eq!(db_key(&fx, "new-provider"), None);
    assert_eq!(db_key(&fx, "old-provider").as_deref(), Some("k-drift"));

    let resp = data(&fx.router, "POST", "/api/sync/webdav/download", None).await;
    assert_eq!(
        resp,
        json!({
            "status": "downloaded",
            "sourceLayout": "current",
            "sourcePath": "/cc-switch-sync/v2/db-v6/default",
        }),
        "no `warning`: the post-import sync succeeded"
    );

    assert_eq!(db_key(&fx, "new-provider").as_deref(), Some("k-new"));
    assert_eq!(db_key(&fx, "old-provider").as_deref(), Some("k-old"));
    assert_eq!(
        fx.state
            .db
            .get_current_provider("claude")
            .unwrap()
            .as_deref(),
        Some("old-provider")
    );
    // `run_post_import_sync` rewrote the live file from the restored current provider.
    assert_eq!(live_claude_key(), "k-old");
    // The settings (with the password) are not part of the snapshot and survive it.
    assert_eq!(
        stored_settings()["webdavSync"]["password"],
        json!("webdav-secret-1")
    );
}

#[tokio::test]
#[serial]
async fn download_and_remote_info_on_an_empty_remote() {
    let fx = fixture_with_providers();
    let stub = DavStub::start().await;
    store_webdav(&fx.router, &stub.base_url()).await;

    let info = data(&fx.router, "GET", "/api/sync/webdav/remote-info", None).await;
    assert_eq!(info, json!({ "empty": true }));

    let (status, json) = web_call(&fx.router, "POST", "/api/sync/webdav/download", None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        json,
        json!({
            "ok": false,
            "error": "远端没有可下载的同步数据 (No downloadable sync data found on the remote.)"
        })
    );
    assert_eq!(db_key(&fx, "old-provider").as_deref(), Some("k-old"));
}

#[tokio::test]
#[serial]
async fn remote_info_describes_the_uploaded_snapshot() {
    let fx = fixture_with_providers();
    let stub = DavStub::start().await;
    store_webdav(&fx.router, &stub.base_url()).await;
    data(&fx.router, "POST", "/api/sync/webdav/upload", None).await;

    let info = data(&fx.router, "GET", "/api/sync/webdav/remote-info", None).await;
    assert_eq!(info["compatible"], json!(true), "{info}");
    assert_eq!(info["layout"], json!("current"));
    assert_eq!(
        info["remotePath"],
        json!("/cc-switch-sync/v2/db-v6/default")
    );
    assert_eq!(info["artifacts"], json!(["db.sql", "skills.zip"]));
    let manifest: Value =
        serde_json::from_slice(&stub.file(&format!("{REMOTE_DIR}/manifest.json")).unwrap())
            .unwrap();
    assert_eq!(info["snapshotId"], manifest["snapshotId"]);
    assert!(!info["deviceName"].as_str().unwrap_or_default().is_empty());
}

/// Collapses a request log to its transfer direction: `W` for what an upload sends (MKCOL, PUT,
/// HEAD), `R` for what a download sends (GET), runs merged, PROPFIND ignored.
fn directions(log: &[(String, String)]) -> String {
    let mut out = String::new();
    for (method, _) in log {
        let d = match method.as_str() {
            "MKCOL" | "PUT" | "HEAD" => 'W',
            "GET" => 'R',
            _ => continue,
        };
        if !out.ends_with(d) {
            out.push(d);
        }
    }
    out
}

#[tokio::test]
#[serial]
async fn concurrent_upload_and_download_are_serialised_by_the_sync_lock() {
    let fx = fixture_with_providers();
    // Every PUT / GET / HEAD is held for 100 ms: without the lock the download's GETs would
    // land inside the upload's PUTs. The assertions below are about overlap, not elapsed time.
    let stub = DavStub::start_with_delay(Duration::from_millis(100)).await;
    store_webdav(&fx.router, &stub.base_url()).await;
    // Seed the remote, so the download has a snapshot to restore.
    data(&fx.router, "POST", "/api/sync/webdav/upload", None).await;
    stub.forget_requests();

    let (up, down) = tokio::join!(
        web_call(&fx.router, "POST", "/api/sync/webdav/upload", None),
        web_call(&fx.router, "POST", "/api/sync/webdav/download", None),
    );
    assert_eq!(up.0, StatusCode::OK, "{}", up.1);
    assert_eq!(up.1["data"], json!({ "status": "uploaded" }));
    assert_eq!(down.0, StatusCode::OK, "{}", down.1);
    assert_eq!(down.1["data"]["status"], json!("downloaded"));

    assert_eq!(
        stub.max_in_flight(),
        1,
        "two sync operations talked to the remote at once: {:?}",
        stub.dav_log()
    );
    let order = directions(&stub.dav_log());
    assert!(
        order == "WR" || order == "RW",
        "the operations' requests interleaved: {order} in {:?}",
        stub.dav_log()
    );
}
