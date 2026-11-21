#[allow(dead_code)]
mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};
use serial_test::serial;
use std::fs;
use std::sync::Arc;
use support::{create_test_state, ensure_test_home, reset_test_fs, web_call, web_router_with};

fn fresh() -> axum::Router {
    reset_test_fs();
    ensure_test_home();
    web_router_with(Arc::new(create_test_state().expect("state")))
}

fn read_json(path: &std::path::Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).expect("read json")).expect("parse json")
}

async fn data(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, resp) = web_call(router, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(true), "{method} {uri}: {resp}");
    resp["data"].clone()
}

#[tokio::test]
#[serial]
async fn plugin_config_official_toggles_primary_api_key() {
    let router = fresh();
    let path = ensure_test_home().join(".claude/config.json");

    let changed = data(
        &router,
        "POST",
        "/api/claude/plugin-config",
        Some(json!({"official": false})),
    )
    .await;
    assert_eq!(changed, json!(true));
    assert_eq!(read_json(&path)["primaryApiKey"], json!("any"));

    let changed = data(
        &router,
        "POST",
        "/api/claude/plugin-config",
        Some(json!({"official": true})),
    )
    .await;
    assert_eq!(changed, json!(true));
    assert!(read_json(&path).get("primaryApiKey").is_none());
}

#[tokio::test]
#[serial]
async fn onboarding_skip_sets_and_clears_flag() {
    let router = fresh();
    let path = ensure_test_home().join(".claude.json");
    fs::write(&path, r#"{"keep":1}"#).expect("seed");

    assert_eq!(
        data(&router, "POST", "/api/claude/onboarding-skip", None).await,
        json!(true)
    );
    let v = read_json(&path);
    assert_eq!(v["hasCompletedOnboarding"], json!(true));
    assert_eq!(v["keep"], json!(1));

    assert_eq!(
        data(&router, "DELETE", "/api/claude/onboarding-skip", None).await,
        json!(true)
    );
    let v = read_json(&path);
    assert!(v.get("hasCompletedOnboarding").is_none());
    assert_eq!(v["keep"], json!(1));
}

#[tokio::test]
#[serial]
async fn session_compression_round_trips() {
    let router = fresh();
    let uri = "/api/codex/session-compression";
    assert_eq!(data(&router, "GET", uri, None).await, json!(false));
    assert_eq!(
        data(&router, "PUT", uri, Some(json!({"enabled": true}))).await,
        json!(true)
    );
    assert_eq!(data(&router, "GET", uri, None).await, json!(true));
    assert_eq!(
        data(&router, "PUT", uri, Some(json!({"enabled": false}))).await,
        json!(false)
    );
    assert_eq!(data(&router, "GET", uri, None).await, json!(false));
}

#[tokio::test]
#[serial]
async fn multi_agent_probe_follows_config_toml() {
    let router = fresh();
    let uri = "/api/codex/forces-multi-agent-v2";
    assert_eq!(data(&router, "GET", uri, None).await, json!(false));
    let dir = ensure_test_home().join(".codex");
    fs::create_dir_all(&dir).expect("codex dir");
    fs::write(
        dir.join("config.toml"),
        "[features]\nmulti_agent_v2 = true\n",
    )
    .expect("config");
    assert_eq!(data(&router, "GET", uri, None).await, json!(true));
}

#[tokio::test]
#[serial]
async fn disk_usage_sums_session_files() {
    let router = fresh();
    let uri = "/api/codex/sessions-disk-usage";
    assert_eq!(data(&router, "GET", uri, None).await, json!(0));
    let root = ensure_test_home().join(".codex");
    fs::create_dir_all(root.join("sessions/2026/06")).expect("sessions");
    fs::create_dir_all(root.join("archived_sessions")).expect("archived");
    fs::write(root.join("sessions/2026/06/a.jsonl"), vec![b'x'; 100]).expect("a");
    fs::write(root.join("archived_sessions/b.jsonl"), vec![b'y'; 23]).expect("b");
    assert_eq!(data(&router, "GET", uri, None).await, json!(123));
}

#[tokio::test]
#[serial]
async fn restore_without_backup_reports_skipped_reason() {
    let router = fresh();
    assert_eq!(
        data(&router, "GET", "/api/codex/unify-history-backup", None).await,
        json!(false)
    );
    let r = data(&router, "POST", "/api/codex/restore-unified-history", None).await;
    assert_eq!(r["skippedReason"], json!("no_backup_ledger"));
    assert_eq!(r["restoredJsonlFiles"], json!(0));
    assert_eq!(r["restoredStateRows"], json!(0));
}

#[tokio::test]
#[serial]
async fn restore_with_backup_rewrites_ledgered_sessions() {
    let router = fresh();
    let home = ensure_test_home();
    let gen = home.join(".cc-switch/backups/codex-official-history-unify-v1/20260612_010101");
    let backup_dir = gen.join("jsonl/sessions/2026/06/01");
    fs::create_dir_all(&backup_dir).expect("backup dir");
    fs::write(
        backup_dir.join("official.jsonl"),
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"openai\"}}\n",
    )
    .expect("backup");
    let live_dir = home.join(".codex/sessions/2026/06/01");
    fs::create_dir_all(&live_dir).expect("live dir");
    let live = live_dir.join("official.jsonl");
    fs::write(
        &live,
        "{\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"model_provider\":\"custom\"}}\n",
    )
    .expect("live");

    assert_eq!(
        data(&router, "GET", "/api/codex/unify-history-backup", None).await,
        json!(true)
    );
    let r = data(&router, "POST", "/api/codex/restore-unified-history", None).await;
    assert_eq!(r["restoredJsonlFiles"], json!(1));
    assert!(r.get("skippedReason").is_none(), "{r}");
    assert!(fs::read_to_string(&live)
        .expect("read live")
        .contains("\"model_provider\":\"openai\""));
}
