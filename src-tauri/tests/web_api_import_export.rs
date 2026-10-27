#[allow(dead_code)]
mod support;

use axum::http::StatusCode;
use cc_switch_lib::{AppType, MultiAppConfig, Provider};
use serde_json::{json, Value};
use serial_test::serial;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use support::{
    create_test_state, create_test_state_with_config, ensure_test_home, reset_test_fs, web_call,
    web_router_with,
};

struct Fixture {
    router: axum::Router,
    state: Arc<cc_switch_lib::AppState>,
    backups: PathBuf,
}

fn backups_dir() -> PathBuf {
    let backups = ensure_test_home().join(".cc-switch").join("backups");
    let _ = fs::remove_dir_all(&backups);
    backups
}

/// Fresh HOME with two Claude providers (`old-provider` current).
fn fresh() -> Fixture {
    reset_test_fs();
    let backups = backups_dir();
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
        backups,
    }
}

/// Fresh HOME and an empty DB (no providers at all).
fn empty() -> Fixture {
    reset_test_fs();
    let backups = backups_dir();
    let state = Arc::new(create_test_state().expect("state"));
    Fixture {
        router: web_router_with(state.clone()),
        state,
        backups,
    }
}

async fn data(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, resp) = web_call(router, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(true), "{method} {uri}: {resp}");
    resp["data"].clone()
}

async fn import_failure(router: &axum::Router, content: &str) -> String {
    let (status, resp) = web_call(
        router,
        "POST",
        "/api/config/import",
        Some(json!({ "content": content })),
    )
    .await;
    assert!(
        status.is_client_error() || status.is_server_error(),
        "{resp}"
    );
    assert_eq!(resp["ok"], json!(false), "{resp}");
    resp["error"].as_str().expect("error string").to_string()
}

async fn export(router: &axum::Router) -> Value {
    data(router, "GET", "/api/config/export", None).await
}

fn key_of(fx: &Fixture, id: &str) -> Option<String> {
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

fn live_claude_key() -> String {
    let path = ensure_test_home().join(".claude").join("settings.json");
    let v: Value = serde_json::from_str(&fs::read_to_string(path).expect("live settings"))
        .expect("live settings json");
    v["env"]["ANTHROPIC_API_KEY"]
        .as_str()
        .expect("live key")
        .to_string()
}

#[tokio::test]
#[serial]
async fn export_returns_timestamped_filename_and_cc_switch_sql() {
    let fx = fresh();
    let out = export(&fx.router).await;
    let filename = out["filename"].as_str().expect("filename");
    let stamp = filename
        .strip_prefix("cc-switch-")
        .and_then(|s| s.strip_suffix(".sql"))
        .unwrap_or_else(|| panic!("bad filename {filename}"));
    assert_eq!(stamp.len(), "YYYYMMDD-HHMMSS".len(), "{filename}");
    assert!(
        stamp
            .chars()
            .enumerate()
            .all(|(i, c)| if i == 8 { c == '-' } else { c.is_ascii_digit() }),
        "{filename}"
    );
    let content = out["content"].as_str().expect("content");
    assert!(content.starts_with("-- CC Switch SQLite"), "{content:.80}");
    assert!(content.contains("old-provider"));
}

#[tokio::test]
#[serial]
async fn export_then_import_into_clean_db_restores_providers_with_backup() {
    let src = fresh();
    let dump = export(&src.router).await["content"]
        .as_str()
        .unwrap()
        .to_string();

    let dst = empty();
    assert_eq!(key_of(&dst, "old-provider"), None);
    let resp = data(
        &dst.router,
        "POST",
        "/api/config/import",
        Some(json!({ "content": dump })),
    )
    .await;
    assert_eq!(resp["success"], json!(true), "{resp}");
    assert_eq!(resp["message"], json!("SQL imported successfully"));
    let backup_id = resp["backupId"].as_str().expect("backupId");
    assert!(!backup_id.is_empty(), "{resp}");
    assert!(
        dst.backups.join(format!("{backup_id}.db")).exists(),
        "safety backup {backup_id} must exist in {:?}",
        dst.backups
    );
    assert_eq!(key_of(&dst, "old-provider").as_deref(), Some("k-old"));
    assert_eq!(key_of(&dst, "new-provider").as_deref(), Some("k-new"));
    assert_eq!(
        dst.state
            .db
            .get_current_provider("claude")
            .unwrap()
            .as_deref(),
        Some("old-provider")
    );
    // Post-import sync projected the current provider into the live file.
    assert_eq!(live_claude_key(), "k-old");
}

#[tokio::test]
#[serial]
async fn import_of_garbage_fails_and_leaves_db_untouched() {
    let fx = fresh();
    let error = import_failure(&fx.router, "this is not a cc-switch dump").await;
    assert!(
        error.contains("Only SQL backups exported by CC Switch")
            || error.contains("仅支持导入由 CC Switch 导出的 SQL 备份文件"),
        "{error}"
    );
    assert_eq!(key_of(&fx, "old-provider").as_deref(), Some("k-old"));
    assert_eq!(key_of(&fx, "new-provider").as_deref(), Some("k-new"));
    assert!(!fx.backups.exists(), "a rejected import takes no backup");

    // A body without `content` is a 400, not a panic.
    let (status, resp) = web_call(&fx.router, "POST", "/api/config/import", Some(json!({}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{resp}");
    assert_eq!(resp["ok"], json!(false));
}

#[tokio::test]
#[serial]
async fn import_of_newer_schema_dump_is_refused_with_the_desktop_message() {
    let fx = fresh();
    let dump = export(&fx.router).await["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(dump.contains("PRAGMA user_version="));
    let newer = dump
        .lines()
        .map(|l| {
            if l.starts_with("PRAGMA user_version=") {
                "PRAGMA user_version=9999;".to_string()
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    // Edit the live DB so a wrongly accepted import would be visible.
    fx.state
        .db
        .delete_provider("claude", "new-provider")
        .unwrap();

    let error = import_failure(&fx.router, &newer).await;
    // Same text the desktop command returns for the same SQL.
    let desktop = fx
        .state
        .db
        .import_sql_string(&newer)
        .expect_err("direct import refuses too")
        .to_string();
    assert_eq!(error, desktop);
    assert_eq!(key_of(&fx, "new-provider"), None, "DB must stay as it was");
    assert_eq!(key_of(&fx, "old-provider").as_deref(), Some("k-old"));
}

#[tokio::test]
#[serial]
async fn dump_over_two_mib_passes_the_body_limit_and_round_trips() {
    let fx = fresh();
    // ~3 MB of multibyte text in one provider note: the dump exceeds axum's 2 MiB default.
    let big = "ж😀".repeat(500_000);
    {
        let conn = rusqlite::Connection::open(ensure_test_home().join(".cc-switch/cc-switch.db"))
            .expect("open db file");
        conn.execute(
            "UPDATE providers SET notes = ?1 WHERE id = 'old-provider'",
            [&big],
        )
        .expect("set notes");
    }
    let dump = export(&fx.router).await["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(dump.len() > 2 * 1024 * 1024, "dump is {} bytes", dump.len());

    let resp = data(
        &fx.router,
        "POST",
        "/api/config/import",
        Some(json!({ "content": dump })),
    )
    .await;
    assert_eq!(resp["success"], json!(true), "{resp}");
    let p = fx
        .state
        .db
        .get_provider_by_id("old-provider", "claude")
        .unwrap()
        .unwrap();
    assert_eq!(p.notes.as_deref(), Some(big.as_str()));
}

#[tokio::test]
#[serial]
async fn body_over_64_mib_is_rejected_by_the_route_limit() {
    let fx = fresh();
    let huge = "a".repeat(65 * 1024 * 1024);
    let (status, _) = {
        use axum::body::Body;
        use axum::http::{header, Request};
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let req = Request::builder()
            .method("POST")
            .uri("/api/config/import")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(format!("{{\"content\":\"{huge}\"}}")))
            .unwrap();
        let resp = fx.router.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let _ = resp.into_body().collect().await;
        (status, ())
    };
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(key_of(&fx, "old-provider").as_deref(), Some("k-old"));
}

#[tokio::test]
#[serial]
async fn dump_with_blob_and_invalid_text_exports_as_utf8_and_reimports() {
    let fx = fresh();
    {
        let conn = rusqlite::Connection::open(ensure_test_home().join(".cc-switch/cc-switch.db"))
            .expect("open db file");
        // Invalid UTF-8 bytes in a TEXT cell, and a BLOB with NUL.
        conn.execute(
            "UPDATE providers SET notes = CAST(X'80FF61' AS TEXT) WHERE id = 'new-provider'",
            [],
        )
        .expect("bad text");
        conn.execute(
            "UPDATE providers SET settings_config = CAST(X'00FF' AS BLOB) WHERE id = 'old-provider'",
            [],
        )
        .expect("blob");
    }
    // The route itself proves UTF-8: the body is a JSON string.
    let dump = export(&fx.router).await["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        dump.contains("X'80FF61'") || dump.contains("X'00FF'"),
        "no hex escape in dump"
    );
    let resp = data(
        &fx.router,
        "POST",
        "/api/config/import",
        Some(json!({ "content": dump })),
    )
    .await;
    assert_eq!(resp["success"], json!(true), "{resp}");
}

#[tokio::test]
#[serial]
async fn sync_live_rewrites_the_live_file_of_the_current_provider() {
    let fx = fresh();
    assert!(
        !ensure_test_home().join(".claude/settings.json").exists(),
        "no live file yet"
    );
    let resp = data(&fx.router, "POST", "/api/providers/sync-live", None).await;
    assert_eq!(
        resp,
        json!({ "success": true, "message": "Live configuration synchronized" })
    );
    assert_eq!(live_claude_key(), "k-old");

    // Corrupt the live file; the manual sync puts the DB provider back.
    fs::write(
        ensure_test_home().join(".claude/settings.json"),
        r#"{"env":{"ANTHROPIC_API_KEY":"tampered"}}"#,
    )
    .unwrap();
    data(&fx.router, "POST", "/api/providers/sync-live", None).await;
    assert_eq!(live_claude_key(), "k-old");
}

#[tokio::test]
#[serial]
async fn concurrent_imports_are_serialised_and_leave_a_sound_db() {
    let fx = fresh();
    let dump = export(&fx.router).await["content"]
        .as_str()
        .unwrap()
        .to_string();
    let body = || Some(json!({ "content": dump }));
    let ((s1, r1), (s2, r2), (s3, r3)) = tokio::join!(
        web_call(&fx.router, "POST", "/api/config/import", body()),
        web_call(&fx.router, "POST", "/api/config/import", body()),
        web_call(&fx.router, "POST", "/api/config/import", body()),
    );
    let ids: Vec<String> = [(s1, &r1), (s2, &r2), (s3, &r3)]
        .into_iter()
        .map(|(s, r)| {
            assert_eq!(s, StatusCode::OK, "{r}");
            assert_eq!(r["ok"], json!(true), "{r}");
            r["data"]["backupId"]
                .as_str()
                .expect("backupId")
                .to_string()
        })
        .collect();
    // Each import took its own safety backup (not interleaved into one).
    let mut uniq = ids.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), 3, "{ids:?}");
    assert_eq!(key_of(&fx, "old-provider").as_deref(), Some("k-old"));
    assert_eq!(key_of(&fx, "new-provider").as_deref(), Some("k-new"));
}
