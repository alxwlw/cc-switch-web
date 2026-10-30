//! `/api/models/*` (WEB-03 T2): the seven model-list routes through the HTTP layer.
//!
//! The handlers are thin wrappers over the same inners the desktop commands
//! use, so these tests pin the HTTP contract: route paths, camelCase bodies,
//! verbatim error texts the UI matches on, and DTO shapes passed through
//! unchanged.
#![cfg(not(target_os = "windows"))]

mod support;

use axum::http::StatusCode;
use cc_switch_lib::AppState;
use serde_json::{json, Value};
use serial_test::serial;
use std::ffi::OsString;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use support::{
    create_test_state, ensure_test_home, reset_test_fs, test_mutex, web_call, web_router_with,
};

/// Save/restore one process-global env var (PATH/SHELL control must not leak
/// into other tests in this binary).
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

    fn remove(key: &'static str) -> Self {
        let prev = std::env::var_os(key);
        std::env::remove_var(key);
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

fn fixture() -> (Arc<AppState>, axum::Router) {
    reset_test_fs();
    ensure_test_home();
    let state = Arc::new(create_test_state().unwrap());
    let router = web_router_with(state.clone());
    (state, router)
}

async fn ok_data(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, resp) = web_call(router, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(true), "{method} {uri}: {resp}");
    resp["data"].clone()
}

/// Error text of a failing route call; asserts the `{ok: false, error}` shape.
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

/// Axum stub answering `GET /v1/models` with `respond`; counts ONLY that
/// expected request (method + path). Anything else — including the loopback
/// port prober's `GET /` — falls through to the 404 fallback uncounted, and
/// `axum::serve` keeps accepting connections in a loop.
struct Stub {
    url: String,
    hits: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Stub {
    async fn start(
        respond: impl Fn() -> (StatusCode, &'static str, &'static str) + Clone + Send + 'static,
    ) -> Self {
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let app = axum::Router::new()
            .route(
                "/v1/models",
                axum::routing::get(move || {
                    let respond = respond.clone();
                    let counter = counter.clone();
                    async move {
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

    /// A stub that accepts the request and never responds: the client's own
    /// timeout drives completion, so the test needs no wall-clock sleeps.
    async fn hang() -> Self {
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let app = axum::Router::new()
            .route(
                "/v1/models",
                axum::routing::get(move || {
                    let counter = counter.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        std::future::pending::<()>().await;
                        StatusCode::OK
                    }
                }),
            )
            .fallback(|| async { StatusCode::NOT_FOUND });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { url, hits, task }
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
#[serial]
async fn models_fetch_lists_through_and_sorts() {
    let _lock = test_mutex().lock().unwrap();
    let (_state, router) = fixture();
    let stub = Stub::start(|| {
        (
            StatusCode::OK,
            "application/json",
            r#"{"data":[{"id":"b"},{"id":"a","owned_by":"acme"}]}"#,
        )
    })
    .await;
    let data = ok_data(
        &router,
        "POST",
        "/api/models/fetch",
        Some(json!({"baseUrl": stub.url, "apiKey": "k"})),
    )
    .await;
    assert_eq!(
        data,
        json!([
            {"id": "a", "ownedBy": "acme"},
            {"id": "b", "ownedBy": null},
        ])
    );
    assert_eq!(stub.hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
#[serial]
async fn models_fetch_401_surfaces_verbatim_without_key_leak() {
    let _lock = test_mutex().lock().unwrap();
    let (_state, router) = fixture();
    let stub = Stub::start(|| {
        (
            StatusCode::UNAUTHORIZED,
            "application/json",
            r#"{"error":"bad key"}"#,
        )
    })
    .await;
    let err = err_text(
        &router,
        StatusCode::INTERNAL_SERVER_ERROR,
        "POST",
        "/api/models/fetch",
        Some(json!({"baseUrl": stub.url, "apiKey": "sk-test-secret-ABC123"})),
    )
    .await;
    assert!(err.contains("HTTP 401"), "verbatim service text: {err}");
    assert!(
        !err.contains("sk-test-secret-ABC123"),
        "apiKey must not land in error output: {err}"
    );
    assert_eq!(stub.hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
#[serial]
async fn models_fetch_slow_stub_reports_timeout() {
    let _lock = test_mutex().lock().unwrap();
    let _timeout = EnvGuard::set("CC_SWITCH_MODEL_FETCH_TIMEOUT_SECS", "1");
    let (_state, router) = fixture();
    let stub = Stub::hang().await;
    let started = std::time::Instant::now();
    let err = err_text(
        &router,
        StatusCode::INTERNAL_SERVER_ERROR,
        "POST",
        "/api/models/fetch",
        Some(json!({"baseUrl": stub.url, "apiKey": "k"})),
    )
    .await;
    assert!(err.contains("timed out"), "UI timeout match: {err}");
    assert_eq!(stub.hits.load(Ordering::SeqCst), 1);
    // Pins the env seam: without it the client timeout is 15 s, not ~1 s.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "timeout seam engaged"
    );
}

#[tokio::test]
#[serial]
async fn models_fetch_invalid_json_surfaces_parse_error() {
    let _lock = test_mutex().lock().unwrap();
    let (_state, router) = fixture();
    let stub = Stub::start(|| (StatusCode::OK, "text/plain", "this is not json{{{")).await;
    let err = err_text(
        &router,
        StatusCode::INTERNAL_SERVER_ERROR,
        "POST",
        "/api/models/fetch",
        Some(json!({"baseUrl": stub.url, "apiKey": "k"})),
    )
    .await;
    assert!(
        err.contains("Failed to parse"),
        "verbatim service text: {err}"
    );
    assert_eq!(stub.hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
#[serial]
async fn models_fetch_bad_body_is_400_not_422() {
    let _lock = test_mutex().lock().unwrap();
    let (_state, router) = fixture();
    // Missing baseUrl.
    let err = err_text(
        &router,
        StatusCode::BAD_REQUEST,
        "POST",
        "/api/models/fetch",
        Some(json!({"apiKey": "k"})),
    )
    .await;
    assert!(err.contains("baseUrl"), "{err}");
    // Wrong-typed optional field.
    let err = err_text(
        &router,
        StatusCode::BAD_REQUEST,
        "POST",
        "/api/models/fetch",
        Some(json!({"baseUrl": "http://x", "apiKey": "k", "isFullUrl": "yes"})),
    )
    .await;
    assert!(err.contains("isFullUrl"), "{err}");
}

#[tokio::test]
#[serial]
async fn models_agy_without_cli_surfaces_install_hint() {
    let _lock = test_mutex().lock().unwrap();
    // Hermetic "no agy": empty PATH defeats the PATH scan, SHELL="" defeats
    // the login-shell fallback (`sh -l -c 'command -v agy'`), the seam below
    // redirects the hardcoded system dirs (`/opt/homebrew/bin`,
    // `/usr/local/bin`) to this empty dir, and the fresh test HOME has no
    // ~/.local/bin/agy and no agy settings.json.
    let empty = tempfile::tempdir().unwrap();
    let _path = EnvGuard::set("PATH", empty.path().to_str().unwrap());
    let _shell = EnvGuard::set("SHELL", "");
    let _sys = EnvGuard::set("CC_SWITCH_AGY_SYSTEM_DIRS", empty.path().to_str().unwrap());
    let (_state, router) = fixture();
    let err = err_text(
        &router,
        StatusCode::INTERNAL_SERVER_ERROR,
        "POST",
        "/api/models/agy",
        Some(json!({})),
    )
    .await;
    assert_eq!(err, "Antigravity CLI not found; install agy first");
}

#[tokio::test]
#[serial]
async fn models_agy_finds_cli_through_seam_dirs() {
    let _lock = test_mutex().lock().unwrap();
    // Reviewer simulation: PATH is empty and the login-shell probe is off, so
    // the fake CLI is visible ONLY through the seam — without the seam this
    // would be "Antigravity CLI not found". The test outcome is controlled by
    // the seam, not by host state.
    let dir = tempfile::tempdir().unwrap();
    let fake = dir.path().join("agy");
    std::fs::write(&fake, "#!/bin/sh\nprintf 'g1\tGemini One\n'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let empty = tempfile::tempdir().unwrap();
    let _path = EnvGuard::set("PATH", empty.path().to_str().unwrap());
    let _shell = EnvGuard::set("SHELL", "");
    let _sys = EnvGuard::set("CC_SWITCH_AGY_SYSTEM_DIRS", dir.path().to_str().unwrap());
    let (_state, router) = fixture();
    let data = ok_data(&router, "POST", "/api/models/agy", Some(json!({}))).await;
    assert_eq!(data, json!([{"id": "Gemini One", "ownedBy": null}]));
}

#[tokio::test]
#[serial]
async fn models_opencode_without_tool_surfaces_detection_error() {
    let _lock = test_mutex().lock().unwrap();
    // Hermetic "no opencode": empty PATH + plain `sh -c` (no rc files) defeat
    // `command -v`; the seam below redirects the hardcoded system dirs
    // (`/usr/local/bin`, `/usr/bin`) to this empty dir; the fresh test HOME
    // plus removed install-dir envs leave no other candidate behind.
    let empty = tempfile::tempdir().unwrap();
    let _path = EnvGuard::set("PATH", empty.path().to_str().unwrap());
    let _shell = EnvGuard::set("SHELL", "/bin/sh");
    let _sys = EnvGuard::set("CC_SWITCH_TOOL_SYSTEM_DIRS", empty.path().to_str().unwrap());
    let _install = EnvGuard::remove("OPENCODE_INSTALL_DIR");
    let _xdg = EnvGuard::remove("XDG_BIN_DIR");
    let _gopath = EnvGuard::remove("GOPATH");
    let (_state, router) = fixture();
    let err = err_text(
        &router,
        StatusCode::INTERNAL_SERVER_ERROR,
        "GET",
        "/api/models/opencode",
        None,
    )
    .await;
    assert!(err.contains("opencode"), "detection error: {err}");
}

#[tokio::test]
#[serial]
async fn models_opencode_lists_two_refs_through_fake_tool() {
    let _lock = test_mutex().lock().unwrap();
    // Fake `opencode` first on PATH; SHELL=/bin/sh keeps `command -v` to the
    // plain inherited PATH (no rc prepends), so the fake always wins.
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let fake = bin.join("opencode");
    std::fs::write(&fake, "#!/bin/sh\necho 'zeta/z2'\necho 'acme/a1'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let _path = EnvGuard::set("PATH", bin.to_str().unwrap());
    let _shell = EnvGuard::set("SHELL", "/bin/sh");
    let _install = EnvGuard::remove("OPENCODE_INSTALL_DIR");
    let _xdg = EnvGuard::remove("XDG_BIN_DIR");
    let _gopath = EnvGuard::remove("GOPATH");
    let (_state, router) = fixture();
    // The runtime executes with the opencode config dir
    // (`<home>/.config/opencode`) as cwd; it must exist.
    let config_dir = ensure_test_home().join(".config").join("opencode");
    std::fs::create_dir_all(&config_dir).unwrap();
    let data = ok_data(&router, "GET", "/api/models/opencode", None).await;
    assert_eq!(
        data,
        json!([
            {"providerId": "acme", "modelId": "a1"},
            {"providerId": "zeta", "modelId": "z2"},
        ])
    );
}

#[tokio::test]
#[serial]
async fn models_opencode_finds_tool_through_seam_dirs() {
    let _lock = test_mutex().lock().unwrap();
    // Same reviewer simulation for opencode: PATH is empty, so the fake tool is
    // visible ONLY through the seam — without the seam
    // `locate_default_tool` would report "not installed".
    let dir = tempfile::tempdir().unwrap();
    let fake = dir.path().join("opencode");
    std::fs::write(&fake, "#!/bin/sh\necho 'zeta/z2'\necho 'acme/a1'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let empty = tempfile::tempdir().unwrap();
    let _path = EnvGuard::set("PATH", empty.path().to_str().unwrap());
    let _shell = EnvGuard::set("SHELL", "/bin/sh");
    let _sys = EnvGuard::set("CC_SWITCH_TOOL_SYSTEM_DIRS", dir.path().to_str().unwrap());
    let _install = EnvGuard::remove("OPENCODE_INSTALL_DIR");
    let _xdg = EnvGuard::remove("XDG_BIN_DIR");
    let _gopath = EnvGuard::remove("GOPATH");
    let (_state, router) = fixture();
    let config_dir = ensure_test_home().join(".config").join("opencode");
    std::fs::create_dir_all(&config_dir).unwrap();
    let data = ok_data(&router, "GET", "/api/models/opencode", None).await;
    assert_eq!(
        data,
        json!([
            {"providerId": "acme", "modelId": "a1"},
            {"providerId": "zeta", "modelId": "z2"},
        ])
    );
}

#[tokio::test]
#[serial]
async fn models_codex_oauth_without_account_surfaces_manager_error() {
    let _lock = test_mutex().lock().unwrap();
    let (_state, router) = fixture();
    for uri in [
        "/api/models/codex-oauth",
        "/api/models/codex-oauth?accountId=",
    ] {
        let err = err_text(&router, StatusCode::INTERNAL_SERVER_ERROR, "GET", uri, None).await;
        assert_eq!(err, "No ChatGPT account available", "{uri}: {err}");
    }
    // A concrete id flows into the manager instead of the default lookup.
    let err = err_text(
        &router,
        StatusCode::INTERNAL_SERVER_ERROR,
        "GET",
        "/api/models/codex-oauth?accountId=foo",
        None,
    )
    .await;
    assert!(
        err.contains("Codex OAuth token unavailable"),
        "accountId plumbing: {err}"
    );
}

#[tokio::test]
#[serial]
async fn models_xai_oauth_without_account_surfaces_manager_error() {
    let _lock = test_mutex().lock().unwrap();
    let (_state, router) = fixture();
    let err = err_text(
        &router,
        StatusCode::INTERNAL_SERVER_ERROR,
        "GET",
        "/api/models/xai-oauth",
        None,
    )
    .await;
    assert_eq!(err, "No usable xAI account available");
}

#[tokio::test]
#[serial]
async fn models_copilot_without_account_surfaces_manager_error() {
    let _lock = test_mutex().lock().unwrap();
    let (_state, router) = fixture();
    let err = err_text(
        &router,
        StatusCode::INTERNAL_SERVER_ERROR,
        "GET",
        "/api/models/copilot",
        None,
    )
    .await;
    assert_eq!(err, "GitHub 令牌无效或已过期");
    let err = err_text(
        &router,
        StatusCode::INTERNAL_SERVER_ERROR,
        "GET",
        "/api/models/copilot/missing",
        None,
    )
    .await;
    assert_eq!(err, "账号不存在: missing");
}
