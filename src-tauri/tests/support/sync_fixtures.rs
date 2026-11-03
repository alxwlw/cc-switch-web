//! Helpers of the sync tests (`web_api_sync.rs`): settings bodies, request helpers, the provider
//! fixture and the stub-backed assertions' shared pieces.

use std::sync::Arc;

use axum::http::StatusCode;
use cc_switch_lib::{AppType, MultiAppConfig, Provider};
use serde_json::{json, Value};

use crate::dav_stub::{DavStub, S3_BUCKET_PATH};
use crate::support::{
    create_test_state, create_test_state_with_config, ensure_test_home, reset_test_fs, web_call,
    web_router_with,
};

pub fn router() -> axum::Router {
    reset_test_fs();
    ensure_test_home();
    web_router_with(std::sync::Arc::new(create_test_state().unwrap()))
}

/// A localhost port that nothing listens on.
pub fn closed_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

pub fn webdav_settings(base_url: &str) -> Value {
    json!({
        "enabled": true,
        "baseUrl": base_url,
        "username": "alice",
        "password": "webdav-secret-1",
    })
}

pub fn s3_settings() -> Value {
    json!({
        "enabled": true,
        "bucket": "bucket",
        "region": "us-east-1",
        "accessKeyId": "AKIAEXAMPLE",
        "secretAccessKey": "s3-secret-1",
    })
}

/// One request per route with its own verb, with a body that would be valid for a real backend.
pub fn routes(backend: &str) -> Vec<(&'static str, String, Option<Value>)> {
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

/// `web_call` always sends JSON; this one sends the given Content-Type (or none) and extra
/// headers, so a CORS-simple request can be replayed.
pub async fn raw_call(
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

/// `Basic base64("alice:webdav-secret-1")`: the stored credential from `webdav_settings`.
pub const STORED_BASIC: &str = "Basic YWxpY2U6d2ViZGF2LXNlY3JldC0x";

/// Stores WebDAV settings (with the password) through the save route.
pub async fn store_webdav(router: &axum::Router, base_url: &str) {
    let (status, json) = web_call(
        router,
        "PUT",
        "/api/sync/webdav/settings",
        Some(json!({ "settings": webdav_settings(base_url), "passwordTouched": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
}

pub async fn stored_base_url(router: &axum::Router) -> Value {
    let (status, json) = web_call(router, "GET", "/api/settings", None).await;
    assert_eq!(status, StatusCode::OK);
    json["data"]["webdavSync"]["baseUrl"].clone()
}

/// `Basic base64("alice:")`: the credential when the password is empty and not filled in.
pub const EMPTY_PASSWORD_BASIC: &str = "Basic YWxpY2U6";

/// Where `webdav_settings` (default root and profile) keeps its files on the stub.
pub const REMOTE_DIR: &str = "/dav/cc-switch-sync/v2/db-v6/default";

/// The request settled with `200 {ok:true,data}`; returns `data`.
pub async fn data(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, json) = web_call(router, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {json}");
    assert_eq!(json["ok"], json!(true), "{method} {uri}: {json}");
    json["data"].clone()
}

/// What the save route persisted, unmasked (`GET /api/settings` clears the secrets).
pub fn stored_settings() -> Value {
    let path = ensure_test_home().join(".cc-switch").join("settings.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("settings.json")).expect("json")
}

pub struct Fixture {
    pub router: axum::Router,
    pub state: Arc<cc_switch_lib::AppState>,
}

/// Fresh HOME with two Claude providers, `old-provider` current.
pub fn fixture_with_providers() -> Fixture {
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

pub fn db_key(fx: &Fixture, id: &str) -> Option<String> {
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

pub fn live_claude_path() -> std::path::PathBuf {
    ensure_test_home().join(".claude").join("settings.json")
}

pub fn write_live_claude_key(key: &str) {
    let path = live_claude_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        json!({ "env": { "ANTHROPIC_API_KEY": key } }).to_string(),
    )
    .unwrap();
}

pub fn live_claude_key() -> String {
    let v: Value =
        serde_json::from_str(&std::fs::read_to_string(live_claude_path()).expect("live settings"))
            .expect("live settings json");
    v["env"]["ANTHROPIC_API_KEY"]
        .as_str()
        .expect("live key")
        .to_string()
}

pub fn s3_settings_for(stub: &DavStub) -> Value {
    let mut settings = s3_settings();
    settings["endpoint"] = json!(stub.endpoint());
    settings
}

/// One `POST /api/sync/s3/test` against the stub; returns the `(x-amz-date, Authorization)`
/// of the single `HEAD /bucket/` it sent.
pub async fn s3_test_signature(
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

/// Collapses a request log to its transfer direction: `W` for what an upload sends (MKCOL, PUT,
/// HEAD), `R` for what a download sends (GET), runs merged, PROPFIND ignored.
pub fn directions(log: &[(String, String)]) -> String {
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
