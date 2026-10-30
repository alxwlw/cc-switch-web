//! `/api/*usage*`, `/api/subscription/*`, `/api/balance`, stream-check (WEB-04 T2).
//!
//! The handlers are thin wrappers over the same handle-free inners the
//! desktop commands use, so these tests pin the HTTP contract: route paths,
//! camelCase bodies, verbatim service errors, the 400-vs-422 boundary, and
//! the cache/emit tails. Stubs are loopback-only (the QuickJS sandbox allows
//! them); the sandbox itself is covered by asserting a non-loopback `http:`
//! script is refused without any request leaving the process.
#![cfg(not(target_os = "windows"))]

mod support;

use axum::http::StatusCode;
use cc_switch_lib::{AppState, AppType, Provider, ProviderMeta};
use serde_json::{json, Value};
use serial_test::serial;
use std::ffi::OsString;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use support::{
    create_test_state, ensure_test_home, reset_test_fs, test_mutex, web_call, web_router_with,
};

/// Save/restore one process-global env var (PATH/SHELL control must not leak
/// into other tests in this binary). Precedent: web_api_models.rs.
struct EnvGuard {
    key: &'static str,
    prev: Option<OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, val: &str) -> Self {
        let prev = std::env::var_os(key);
        std::env::set_var(key, val);
        Self { key, prev }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

/// Router over a fresh state the test keeps (to reach `state.db` /
/// `state.usage_cache` / `state.events`).
fn fixture() -> (Arc<AppState>, axum::Router) {
    reset_test_fs();
    ensure_test_home();
    let state = Arc::new(create_test_state().expect("state"));
    let router = web_router_with(state.clone());
    (state, router)
}

async fn ok_data(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, resp) = web_call(router, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(true), "{method} {uri}: {resp}");
    resp["data"].clone()
}

async fn err_text(
    router: &axum::Router,
    expected: StatusCode,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> String {
    let (status, resp) = web_call(router, method, uri, body).await;
    assert_eq!(status, expected, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(false), "{method} {uri}: {resp}");
    resp["error"].as_str().unwrap_or_default().to_string()
}

/// Axum stub answering one expected request; counts ONLY that request
/// (method + path). Anything else falls through to the 404 fallback
/// uncounted, and `axum::serve` keeps accepting connections in a loop.
struct Stub {
    url: String,
    hits: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Stub {
    async fn start(
        path: &str,
        respond: impl Fn() -> (StatusCode, &'static str, String) + Clone + Send + 'static,
    ) -> Self {
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let app = axum::Router::new()
            .route(
                path,
                axum::routing::any(move || {
                    let respond = respond.clone();
                    let counter = counter.clone();
                    async move {
                        // Count only the expected request; anything else
                        // (including a loopback port prober's `GET /`) falls
                        // through to the 404 fallback uncounted.
                        counter.fetch_add(1, Ordering::SeqCst);
                        let (status, content_type, body) = respond();
                        (status, [("content-type", content_type)], body)
                    }
                }),
            )
            .fallback(|| async { StatusCode::NOT_FOUND });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { url, hits, task }
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// A port nothing listens on (bind then release): connection refused.
async fn dead_port() -> u16 {
    tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Wait for the next bus event with `name`, ignoring everything else.
async fn next_event(
    rx: &mut tokio::sync::broadcast::Receiver<cc_switch_lib::events::Event>,
    name: &str,
) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = rx.recv().await.expect("event bus closed");
            if event.name == name {
                return event.payload;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no {name} event within 10 s"))
}

/// Saved-script provider: `meta.usage_script` (snake_case — `ProviderMeta`
/// carries no rename) via `ProviderMeta` JSON so the test never names the
/// non-exported `UsageScript` type.
fn script_provider(id: &str, code: &str, base_url: &str) -> Provider {
    let mut provider = Provider::with_id(
        id.to_string(),
        id.to_uppercase(),
        json!({"env": {
            "ANTHROPIC_BASE_URL": "https://unused.example",
            "ANTHROPIC_AUTH_TOKEN": "sk-test",
        }}),
        None,
    );
    let meta: ProviderMeta = serde_json::from_value(json!({
        "usage_script": {
            "enabled": true,
            "language": "javascript",
            "code": code,
            "timeout": 10,
            "baseUrl": base_url,
        }
    }))
    .expect("usage_script meta");
    provider.meta = Some(meta);
    provider
}

const USAGE_EXTRACTOR: &str =
    "(r) => [{planName: r.plan, remaining: r.left, total: 100, used: 100 - r.left, unit: '%'}]";

fn usage_code(url: &str) -> String {
    format!("({{request: {{url: {url:?}, method: 'GET'}}, extractor: {USAGE_EXTRACTOR}}})")
}

#[tokio::test]
#[serial]
async fn usage_query_hits_stub_caches_and_emits() {
    let _lock = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let (state, router) = fixture();
    let stub = Stub::start("/usage", || {
        (
            StatusCode::OK,
            "application/json",
            r#"{"plan":"pro","left":80}"#.to_string(),
        )
    })
    .await;
    let code = usage_code(&format!("{}/usage", stub.url));
    state
        .db
        .save_provider("claude", &script_provider("usage-stub", &code, &stub.url))
        .unwrap();
    let mut rx = state.events.subscribe();

    let data = ok_data(
        &router,
        "POST",
        "/api/claude/providers/usage-stub/usage",
        None,
    )
    .await;
    assert_eq!(data["success"], json!(true), "{data}");
    assert_eq!(data["data"][0]["planName"], json!("pro"), "{data}");
    assert_eq!(data["data"][0]["remaining"], json!(80.0), "{data}");
    assert_eq!(stub.hits(), 1);

    // Cache write-through …
    let cached = state
        .usage_cache
        .with_script(&AppType::Claude, "usage-stub", |r| r.clone())
        .expect("script cache entry");
    assert!(cached.success);
    assert_eq!(
        cached
            .data
            .as_ref()
            .and_then(|d| d.first())
            .and_then(|d| d.plan_name.clone()),
        Some("pro".to_string()),
    );

    // … and the desktop-identical bus payload.
    let payload = next_event(&mut rx, "usage-cache-updated").await;
    assert_eq!(payload["kind"], json!("script"), "{payload}");
    assert_eq!(payload["appType"], json!("claude"), "{payload}");
    assert_eq!(payload["providerId"], json!("usage-stub"), "{payload}");
    assert_eq!(payload["data"]["success"], json!(true), "{payload}");
}

#[tokio::test]
#[serial]
async fn usage_query_transport_failure_is_err_and_leaves_cache_untouched() {
    let _lock = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let (state, router) = fixture();
    let port = dead_port().await;
    let canary = "usage-canary-9f3a-key";
    let code = format!(
        "({{request: {{url: 'http://127.0.0.1:{port}/dead', method: 'GET', headers: {{'x-canary': '{canary}'}}}}, extractor: {USAGE_EXTRACTOR}}})"
    );
    state
        .db
        .save_provider(
            "claude",
            &script_provider("usage-dead", &code, &format!("http://127.0.0.1:{port}")),
        )
        .unwrap();
    let mut rx = state.events.subscribe();

    let (status, resp) = web_call(
        &router,
        "POST",
        "/api/claude/providers/usage-dead/usage",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{resp}");
    assert_eq!(resp["ok"], json!(false), "{resp}");
    assert!(resp["error"].is_string(), "{resp}");
    assert!(
        !resp.to_string().contains(canary),
        "apiKey material leaked into the error: {resp}"
    );

    assert!(
        state
            .usage_cache
            .with_script(&AppType::Claude, "usage-dead", |_| ())
            .is_none(),
        "transient failure must not write the cache"
    );
    assert!(rx.try_recv().is_err(), "no event on Err");
}

#[tokio::test]
#[serial]
async fn usage_script_test_rejects_non_loopback_http() {
    let _lock = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let (state, router) = fixture();
    // No such provider is needed for the refusal: validation fires before any
    // request, but the row must exist (desktop parity: unknown id is an Err).
    // Seed a bare row without any script — the body carries the script.
    state
        .db
        .save_provider(
            "claude",
            &Provider::with_id(
                "plain".to_string(),
                "PLAIN".to_string(),
                json!({"env": {}}),
                None,
            ),
        )
        .unwrap();

    // TEST-NET-1: never dialed — `validate_base_url` / `validate_request_url`
    // refuse first (plan review risk #1).
    let body = json!({
        "scriptCode": usage_code("http://93.184.216.0/usage"),
        "timeout": 10,
        "apiKey": "k",
        "baseUrl": "http://93.184.216.0/",
    });
    let data = ok_data(
        &router,
        "POST",
        "/api/claude/providers/plain/usage-script/test",
        Some(body),
    )
    .await;
    assert_eq!(data["success"], json!(false), "{data}");
    let error = data["error"].as_str().unwrap_or_default();
    assert!(
        error.to_lowercase().contains("https"),
        "expected the https refusal, got: {error}"
    );
}

/// The suite's only 5 s test: an infinite loop trips the QuickJS deadline.
/// The desktop folds it into `success:false` (not a transport `Err`), and the
/// test route never touches cache/emit.
#[tokio::test]
#[serial]
async fn usage_script_test_infinite_loop_hits_deadline() {
    let _lock = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let (state, router) = fixture();
    state
        .db
        .save_provider(
            "claude",
            &Provider::with_id(
                "loopy".to_string(),
                "LOOPY".to_string(),
                json!({"env": {}}),
                None,
            ),
        )
        .unwrap();
    let mut rx = state.events.subscribe();

    let started = std::time::Instant::now();
    let (status, resp) = web_call(
        &router,
        "POST",
        "/api/claude/providers/loopy/usage-script/test",
        Some(json!({"scriptCode": "for(;;);", "timeout": 10})),
    )
    .await;
    let elapsed = started.elapsed();
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(resp["ok"], json!(true), "{resp}");
    assert_eq!(resp["data"]["success"], json!(false), "{resp}");
    assert!(resp["data"]["error"].is_string(), "{resp}");
    assert!(
        elapsed >= Duration::from_secs(4),
        "deadline is 5 s, finished far too fast: {elapsed:?}"
    );

    assert!(rx.try_recv().is_err(), "test route never emits");
}

#[tokio::test]
#[serial]
async fn balance_unknown_provider_is_deterministic_and_key_free() {
    let _lock = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let (_state, router) = fixture();
    // The baseUrl points at a stub that must never be hit: an unknown vendor
    // short-circuits before any request.
    let stub = Stub::start("/", || {
        (StatusCode::OK, "application/json", "{}".to_string())
    })
    .await;
    let canary = "balance-canary-7c1e-key";

    for (body, want) in [
        (
            json!({"baseUrl": stub.url, "apiKey": canary}),
            "Unknown balance provider",
        ),
        (
            json!({"baseUrl": "https://api.deepseek.com", "apiKey": ""}),
            "API key is empty",
        ),
    ] {
        let (status, resp) = web_call(&router, "POST", "/api/balance", Some(body)).await;
        assert_eq!(status, StatusCode::OK, "{resp}");
        assert_eq!(resp["ok"], json!(true), "{resp}");
        assert_eq!(resp["data"]["success"], json!(false), "{resp}");
        // F2: pin the routing branch — the unknown baseUrl must reach
        // `detect_provider` (not a default vendor), and the empty key must hit
        // the empty-key guard. This proves `baseUrl` was forwarded.
        let error = resp["data"]["error"].as_str().unwrap_or_default();
        assert!(error.contains(want), "expected {want:?}, got: {error}");
        assert!(
            !resp.to_string().contains(canary),
            "apiKey material leaked into the response: {resp}"
        );
    }
    assert_eq!(stub.hits(), 0, "no request must escape to a vendor");
}

#[tokio::test]
#[serial]
async fn coding_plan_zenmux_against_stub() {
    let _lock = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let (_state, router) = fixture();
    // ZenMux is the only coding-plan vendor queried at `baseUrl` itself, so
    // it is the only hermetic success path; detection keys off "zenmux".
    let stub = Stub::start("/zenmux/quota", || {
        (
            StatusCode::OK,
            "application/json",
            json!({
                "success": true,
                "data": {
                    "quota_5_hour": {
                        "usage_percentage": 0.25,
                        "resets_at": "2030-01-01T00:00:00Z",
                        "used_value_usd": 1.5,
                        "max_value_usd": 6.0,
                    },
                    "quota_7_day": {"usage_percentage": 0.1},
                    "plan": {"tier": "pro"},
                    "account_status": "active",
                }
            })
            .to_string(),
        )
    })
    .await;

    let canary = "coding-plan-canary-4d2b-key";
    let data = ok_data(
        &router,
        "POST",
        "/api/subscription/coding-plan",
        Some(json!({
            "baseUrl": format!("{}/zenmux/quota", stub.url),
            "apiKey": canary,
        })),
    )
    .await;
    assert_eq!(data["success"], json!(true), "{data}");
    assert_eq!(data["tool"], json!("coding_plan"), "{data}");
    assert_eq!(data["tiers"].as_array().map(Vec::len), Some(2), "{data}");
    assert_eq!(data["tiers"][0]["name"], json!("five_hour"), "{data}");
    assert_eq!(data["tiers"][0]["utilization"], json!(25.0), "{data}");
    assert_eq!(stub.hits(), 1);
}

#[tokio::test]
#[serial]
async fn provider_limits_reflect_seeded_rows() {
    let _lock = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let (state, router) = fixture();
    let mut provider = Provider::with_id(
        "lim".to_string(),
        "LIM".to_string(),
        json!({"env": {}}),
        None,
    );
    let meta: ProviderMeta = serde_json::from_value(json!({
        "limitDailyUsd": "10.00",
        "limitMonthlyUsd": "100.00",
    }))
    .expect("limit meta");
    provider.meta = Some(meta);
    state.db.save_provider("claude", &provider).unwrap();

    // Seed one $2.50 row dated today through a second connection to the same
    // file DB (precedent: provider_service.rs opens the file directly).
    let home = ensure_test_home();
    let now = chrono::Utc::now().timestamp();
    {
        let conn =
            rusqlite::Connection::open(home.join(".cc-switch/cc-switch.db")).expect("open db");
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model,
                input_tokens, output_tokens, total_cost_usd,
                latency_ms, status_code, created_at
            ) VALUES ('lim-1', 'lim', 'claude', 'claude-3', 100, 50, '2.5', 100, 200, ?)",
            [now],
        )
        .expect("seed usage row");
    }

    let data = ok_data(&router, "GET", "/api/claude/providers/lim/limits", None).await;
    assert_eq!(data["providerId"], json!("lim"), "{data}");
    assert_eq!(data["dailyUsage"], json!("2.500000"), "{data}");
    assert_eq!(data["dailyLimit"], json!("10.00"), "{data}");
    assert_eq!(data["dailyExceeded"], json!(false), "{data}");
    assert_eq!(data["monthlyUsage"], json!("2.500000"), "{data}");
    assert_eq!(data["monthlyLimit"], json!("100.00"), "{data}");
    assert_eq!(data["monthlyExceeded"], json!(false), "{data}");
}

#[tokio::test]
#[serial]
async fn stream_check_against_sse_stub_is_ok() {
    let _lock = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let (state, router) = fixture();
    // The probe GETs the base URL and returns at the response headers; the
    // SSE content-type mirrors what a real streaming endpoint serves.
    let stub = Stub::start("/", || {
        (
            StatusCode::OK,
            "text/event-stream",
            "data: {\"ok\":true}\n\n".to_string(),
        )
    })
    .await;
    state
        .db
        .save_provider(
            "claude",
            &Provider::with_id(
                "streamy".to_string(),
                "STREAMY".to_string(),
                json!({"env": {
                    "ANTHROPIC_BASE_URL": stub.url,
                    "ANTHROPIC_AUTH_TOKEN": "sk-test",
                    "ANTHROPIC_MODEL": "claude-sonnet-4-6",
                }}),
                None,
            ),
        )
        .unwrap();

    let data = ok_data(
        &router,
        "POST",
        "/api/claude/providers/streamy/stream-check",
        None,
    )
    .await;
    assert_eq!(data["success"], json!(true), "{data}");
    assert_eq!(data["status"], json!("operational"), "{data}");
    assert!(data["httpStatus"].as_u64().is_some(), "{data}");
    assert_eq!(stub.hits(), 1);
}

#[tokio::test]
#[serial]
async fn subscription_quota_with_credential_file() {
    let _lock = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let (state, router) = fixture();
    // A claude credentials file that fails to parse: deterministic, no
    // network, and proves the route actually read the file (NotFound would
    // mean it did not). Desktop parity: even `success:false` publishes.
    let home = ensure_test_home();
    let dir = home.join(".claude");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(".credentials.json"), "{not json").unwrap();
    let mut rx = state.events.subscribe();

    let data = ok_data(&router, "GET", "/api/subscription/claude/quota", None).await;
    assert_eq!(data["tool"], json!("claude"), "{data}");
    assert_eq!(data["success"], json!(false), "{data}");
    assert_eq!(data["credentialStatus"], json!("parse_error"), "{data}");

    let cached = state
        .usage_cache
        .with_subscription(&AppType::Claude, |q| q.clone())
        .expect("subscription cache entry");
    assert!(!cached.success);
    let payload = next_event(&mut rx, "usage-cache-updated").await;
    assert_eq!(payload["kind"], json!("subscription"), "{payload}");
    assert_eq!(payload["appType"], json!("claude"), "{payload}");
}

#[tokio::test]
#[serial]
async fn subscription_agy_without_cli_is_intelligible_error() {
    let _lock = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    // Hermetic "no agy" (F1 [DP-014/DP-017]): empty PATH defeats the PATH
    // scan, SHELL="" defeats the login-shell fallback, the seam below
    // redirects the hardcoded system dirs to this empty dir, and the test
    // HOME has no ~/.local/bin/agy. The outcome is controlled by the test,
    // not by host state — a fake `agy` on the outer PATH must not flip this
    // to 200. Precedent: web_api_models.rs `models_agy_without_cli_*`.
    let empty = tempfile::tempdir().unwrap();
    let _path = EnvGuard::set("PATH", empty.path().to_str().unwrap());
    let _shell = EnvGuard::set("SHELL", "");
    let _sys = EnvGuard::set("CC_SWITCH_AGY_SYSTEM_DIRS", empty.path().to_str().unwrap());
    let (_state, router) = fixture();
    // Belt-and-braces: a stale `~/.local/bin/agy` from another test binary
    // sharing this HOME slot would otherwise be found via the home probe.
    let stray = ensure_test_home().join(".local").join("bin").join("agy");
    if stray.exists() {
        std::fs::remove_file(&stray).unwrap();
    }

    // No agy binary is discoverable under the scrubbed env (same as the T1
    // agy unit test) must be an intelligible error, not an empty card.
    for uri in [
        "/api/subscription/agy/some-account/quota",
        "/api/subscription/agy/quota",
    ] {
        let err = err_text(&router, StatusCode::INTERNAL_SERVER_ERROR, "GET", uri, None).await;
        assert!(
            err.to_lowercase().contains("antigravity") || err.to_lowercase().contains("agy"),
            "expected the agy fallback error, got: {err}"
        );
    }
}

#[tokio::test]
#[serial]
async fn malformed_bodies_and_apps_are_400() {
    let _lock = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let (_state, router) = fixture();

    // Missing required fields (never axum's 422).
    for (method, uri, body) in [
        ("POST", "/api/balance", json!({})),
        (
            "POST",
            "/api/balance",
            json!({"baseUrl": "https://x.example"}),
        ),
        (
            "POST",
            "/api/subscription/coding-plan",
            json!({"apiKey": "k"}),
        ),
        (
            "POST",
            "/api/claude/providers/x/usage-script/test",
            json!({"timeout": 10}),
        ),
        (
            "POST",
            "/api/claude/providers/x/usage-script/test",
            json!({"scriptCode": "1", "timeout": -1}),
        ),
    ] {
        let (status, resp) = web_call(&router, method, uri, Some(body)).await;
        assert_eq!(
            (status, &resp["ok"]),
            (StatusCode::BAD_REQUEST, &json!(false)),
            "{method} {uri}: {resp}"
        );
    }

    // Unknown app segment.
    let (status, resp) = web_call(&router, "POST", "/api/nope/providers/x/usage", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{resp}");
    assert_eq!(resp["ok"], json!(false), "{resp}");
}
