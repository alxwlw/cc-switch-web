//! `/api/omo/:variant/{local-file,current-provider,disable}` for both variants. Live files
//! live under `<test home>/.config/opencode`; providers are seeded straight into the DB the
//! router is built on.

mod support;

use axum::http::StatusCode;
use cc_switch_lib::{AppState, Database, Provider};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use support::{ensure_test_home, reset_test_fs, test_mutex, web_call};

struct Variant {
    slug: &'static str,
    category: &'static str,
    file: &'static str,
    provider_id: &'static str,
    other_provider_id: &'static str,
}

const OMO: Variant = Variant {
    slug: "omo",
    category: "omo",
    file: "oh-my-openagent.jsonc",
    provider_id: "omo-p1",
    other_provider_id: "omo-p2",
};
const SLIM: Variant = Variant {
    slug: "omo-slim",
    category: "omo-slim",
    file: "oh-my-opencode-slim.jsonc",
    provider_id: "slim-p1",
    other_provider_id: "slim-p2",
};

fn live_file(v: &Variant, content: &str) -> PathBuf {
    let dir = ensure_test_home().join(".config").join("opencode");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(v.file);
    std::fs::write(&path, content).unwrap();
    path
}

fn seed(db: &Database, id: &str, category: &str) {
    let mut p = Provider::with_id(id.into(), format!("name-{id}"), json!({}), None);
    p.category = Some(category.into());
    db.save_provider("opencode", &p).unwrap();
}

/// Fresh DB + router over it; hold `test_mutex()` and reset the fs before calling.
fn router_with_db() -> (axum::Router, Arc<Database>) {
    let db = Arc::new(Database::init().unwrap());
    let router = cc_switch_lib::web_api::router(Arc::new(AppState::new(db.clone())));
    (router, db)
}

fn url(v: &Variant, op: &str) -> String {
    format!("/api/omo/{}/{op}", v.slug)
}

async fn read_local_file_returns_the_live_file(v: &Variant, agent: &str) {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let path = live_file(
        v,
        &format!(
            r#"{{ // comment
            "agents": {{"{agent}": {{"model": "m-1"}}}},
            "theme": "dark" }}"#
        ),
    );
    let (router, _db) = router_with_db();

    let (status, body) = web_call(&router, "GET", &url(v, "local-file"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    let data = &body["data"];
    assert_eq!(data["agents"], json!({agent: {"model": "m-1"}}));
    assert_eq!(data["otherFields"], json!({"theme": "dark"}));
    assert_eq!(data["filePath"], path.to_str().unwrap());
    assert!(data["lastModified"].as_str().is_some_and(|s| !s.is_empty()));
}

#[tokio::test]
async fn omo_local_file_returns_agents_and_other_fields() {
    read_local_file_returns_the_live_file(&OMO, "sisyphus").await;
}

#[tokio::test]
async fn omo_slim_local_file_returns_agents_and_other_fields() {
    read_local_file_returns_the_live_file(&SLIM, "oracle").await;
}

#[tokio::test]
async fn local_file_without_a_file_is_a_500_with_the_service_error() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let (router, _db) = router_with_db();

    let (status, body) = web_call(&router, "GET", &url(&OMO, "local-file"), None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["ok"], false);
    assert_eq!(
        body["error"],
        cc_switch_lib::AppError::OmoConfigNotFound.to_string()
    );
}

async fn current_provider_comes_from_the_db(v: &Variant) {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let (router, db) = router_with_db();

    let (status, body) = web_call(&router, "GET", &url(v, "current-provider"), None).await;
    assert_eq!((status, &body["data"]), (StatusCode::OK, &json!("")));

    seed(&db, v.other_provider_id, v.category);
    seed(&db, v.provider_id, v.category);
    db.set_omo_provider_current("opencode", v.provider_id, v.category)
        .unwrap();
    let (status, body) = web_call(&router, "GET", &url(v, "current-provider"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"ok": true, "data": v.provider_id}));
}

#[tokio::test]
async fn omo_current_provider_comes_from_the_db() {
    current_provider_comes_from_the_db(&OMO).await;
}

#[tokio::test]
async fn omo_slim_current_provider_comes_from_the_db() {
    current_provider_comes_from_the_db(&SLIM).await;
}

async fn disable_clears_current_and_deletes_the_file_idempotently(v: &Variant, other: &Variant) {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let path = live_file(v, r#"{"agents": {"a": {}}}"#);
    let other_path = live_file(other, r#"{"agents": {"b": {}}}"#);
    let (router, db) = router_with_db();
    seed(&db, v.provider_id, v.category);
    seed(&db, other.provider_id, other.category);
    // set_omo_provider_current deactivates the opposite category, so `other` is never current.
    db.set_omo_provider_current("opencode", v.provider_id, v.category)
        .unwrap();
    assert!(db
        .is_omo_provider_current("opencode", v.provider_id, v.category)
        .unwrap());

    for _ in 0..2 {
        let (status, body) = web_call(&router, "POST", &url(v, "disable"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!({"ok": true, "data": Value::Null}));
        assert!(!path.exists(), "{} must be deleted", path.display());
        assert!(db
            .get_current_omo_provider("opencode", v.category)
            .unwrap()
            .is_none());
    }
    assert!(other_path.exists(), "the other variant's file is untouched");

    let (_, body) = web_call(&router, "GET", &url(v, "current-provider"), None).await;
    assert_eq!(body["data"], "");
}

#[tokio::test]
async fn omo_disable_clears_current_and_deletes_the_file_idempotently() {
    disable_clears_current_and_deletes_the_file_idempotently(&OMO, &SLIM).await;
}

#[tokio::test]
async fn omo_slim_disable_clears_current_and_deletes_the_file_idempotently() {
    disable_clears_current_and_deletes_the_file_idempotently(&SLIM, &OMO).await;
}

#[tokio::test]
async fn unknown_variant_is_a_400_on_every_route() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let (router, _db) = router_with_db();

    for (method, op) in [
        ("GET", "local-file"),
        ("GET", "current-provider"),
        ("POST", "disable"),
    ] {
        let (status, body) = web_call(&router, method, &format!("/api/omo/bogus/{op}"), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {op}");
        assert_eq!(
            body,
            json!({"ok": false, "error": "unknown OMO variant: bogus"})
        );
    }
}
