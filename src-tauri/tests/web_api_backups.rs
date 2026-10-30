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
    create_test_state_with_config, ensure_test_home, reset_test_fs, web_call, web_router_with,
};

struct Fixture {
    router: axum::Router,
    state: Arc<cc_switch_lib::AppState>,
    backups: PathBuf,
}

/// Fresh HOME with two Claude providers (`old-provider` current), router over its state.
fn fresh() -> Fixture {
    reset_test_fs();
    let home = ensure_test_home();
    let backups = home.join(".cc-switch").join("backups");
    let _ = fs::remove_dir_all(&backups);
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

async fn data(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, resp) = web_call(router, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(true), "{method} {uri}: {resp}");
    resp["data"].clone()
}

/// Expects `{ok:false, error:<non-empty string>}` with exactly `status`.
async fn failure(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    status: StatusCode,
) -> String {
    let (got, resp) = web_call(router, method, uri, body).await;
    assert_eq!(got, status, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(false), "{method} {uri}: {resp}");
    let error = resp["error"].as_str().expect("error string").to_string();
    assert!(!error.is_empty(), "{resp}");
    error
}

async fn switch(router: &axum::Router, id: &str) {
    data(
        router,
        "POST",
        "/api/claude/providers/switch",
        Some(json!({ "id": id })),
    )
    .await;
}

async fn create(router: &axum::Router) -> String {
    data(router, "POST", "/api/backups", None)
        .await
        .as_str()
        .expect("filename")
        .to_string()
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

fn db_key(fx: &Fixture) -> String {
    let p = fx
        .state
        .db
        .get_provider_by_id("old-provider", "claude")
        .expect("db read")
        .expect("provider row");
    p.settings_config["env"]["ANTHROPIC_API_KEY"]
        .as_str()
        .expect("key")
        .to_string()
}

fn current(fx: &Fixture) -> Option<String> {
    fx.state.db.get_current_provider("claude").expect("current")
}

#[tokio::test]
#[serial]
async fn create_then_list_contains_the_backup() {
    let fx = fresh();
    let filename = create(&fx.router).await;
    assert!(filename.ends_with(".db"), "{filename}");
    assert!(fx.backups.join(&filename).exists());

    let list = data(&fx.router, "GET", "/api/backups", None).await;
    let entry = list
        .as_array()
        .expect("array")
        .iter()
        .find(|e| e["filename"] == json!(filename))
        .unwrap_or_else(|| panic!("{filename} not in {list}"));
    assert!(
        entry["sizeBytes"].as_u64().expect("sizeBytes") > 0,
        "{entry}"
    );
    assert!(entry["createdAt"].as_str().expect("createdAt").len() > 8);
}

#[tokio::test]
#[serial]
async fn restore_brings_back_provider_and_resyncs_live_file() {
    let fx = fresh();
    switch(&fx.router, "old-provider").await;
    assert_eq!(live_claude_key(), "k-old");
    let filename = create(&fx.router).await;

    // Edit the current provider: DB row and live file both change.
    data(
        &fx.router,
        "PUT",
        "/api/claude/providers",
        Some(json!({ "provider": {
            "id": "old-provider",
            "name": "old-provider",
            "settingsConfig": { "env": { "ANTHROPIC_API_KEY": "k-edited" } },
        }})),
    )
    .await;
    assert_eq!(live_claude_key(), "k-edited");
    assert_eq!(db_key(&fx), "k-edited");

    let restored = data(
        &fx.router,
        "POST",
        &format!("/api/backups/{filename}/restore"),
        None,
    )
    .await;
    // Like the desktop command: the id of the safety backup taken before the restore.
    let safety = restored.as_str().expect("safety id");
    assert!(!safety.is_empty() && safety != filename, "{restored}");
    assert!(
        fx.backups.join(format!("{safety}.db")).exists(),
        "safety backup {safety} must exist"
    );
    assert_eq!(db_key(&fx), "k-old", "DB provider is back");
    assert_eq!(live_claude_key(), "k-old", "live file must be re-synced");
}

#[tokio::test]
#[serial]
async fn restore_of_corrupt_backup_fails_and_leaves_db_untouched() {
    let fx = fresh();
    fs::create_dir_all(&fx.backups).unwrap();
    fs::write(fx.backups.join("corrupt.db"), b"not a sqlite database").unwrap();

    failure(
        &fx.router,
        "POST",
        "/api/backups/corrupt.db/restore",
        None,
        StatusCode::INTERNAL_SERVER_ERROR,
    )
    .await;
    assert_eq!(current(&fx).as_deref(), Some("old-provider"));
    let providers = data(&fx.router, "GET", "/api/claude/providers", None).await;
    assert!(providers.get("new-provider").is_some(), "{providers}");
}

#[tokio::test]
#[serial]
async fn restore_of_future_schema_backup_fails() {
    let fx = fresh();
    let filename = create(&fx.router).await;
    {
        let conn = rusqlite::Connection::open(fx.backups.join(&filename)).unwrap();
        conn.execute_batch("PRAGMA user_version = 9999;").unwrap();
    }
    switch(&fx.router, "new-provider").await;

    failure(
        &fx.router,
        "POST",
        &format!("/api/backups/{filename}/restore"),
        None,
        StatusCode::INTERNAL_SERVER_ERROR,
    )
    .await;
    assert_eq!(current(&fx).as_deref(), Some("new-provider"));
}

#[tokio::test]
#[serial]
async fn traversal_in_filename_is_rejected_with_400_on_every_route() {
    let fx = fresh();
    // `%2F` stays inside one path segment for routing, then Path decodes it to `/`.
    let bad = "..%2F..%2Fevil.db";
    for (method, uri, body) in [
        ("POST", format!("/api/backups/{bad}/restore"), None),
        (
            "PATCH",
            format!("/api/backups/{bad}"),
            Some(json!({ "newName": "x" })),
        ),
        ("DELETE", format!("/api/backups/{bad}"), None),
    ] {
        let error = failure(&fx.router, method, &uri, body, StatusCode::BAD_REQUEST).await;
        assert!(error.contains("Invalid backup filename"), "{uri}: {error}");
    }
}

#[tokio::test]
#[serial]
async fn rename_returns_new_filename_and_refuses_an_occupied_name() {
    let fx = fresh();
    let first = create(&fx.router).await;
    fs::copy(fx.backups.join(&first), fx.backups.join("taken.db")).unwrap();

    let renamed = data(
        &fx.router,
        "PATCH",
        &format!("/api/backups/{first}"),
        Some(json!({ "newName": "mine" })),
    )
    .await;
    assert_eq!(renamed, json!("mine.db"));
    assert!(fx.backups.join("mine.db").exists());
    assert!(!fx.backups.join(&first).exists());

    failure(
        &fx.router,
        "PATCH",
        "/api/backups/mine.db",
        Some(json!({ "newName": "taken" })),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert!(fx.backups.join("mine.db").exists(), "source must survive");

    failure(
        &fx.router,
        "PATCH",
        "/api/backups/mine.db",
        Some(json!({})),
        StatusCode::BAD_REQUEST,
    )
    .await;
}

#[tokio::test]
#[serial]
async fn delete_removes_the_backup_and_a_missing_one_is_400() {
    let fx = fresh();
    let filename = create(&fx.router).await;
    let deleted = data(
        &fx.router,
        "DELETE",
        &format!("/api/backups/{filename}"),
        None,
    )
    .await;
    assert_eq!(deleted, Value::Null);
    assert!(!fx.backups.join(&filename).exists());
    failure(
        &fx.router,
        "DELETE",
        &format!("/api/backups/{filename}"),
        None,
        StatusCode::BAD_REQUEST,
    )
    .await;
}

#[tokio::test]
#[serial]
async fn concurrent_restores_and_delete_give_meaningful_answers_and_a_sound_db() {
    // Pins soundness only: no join error, meaningful answers, a consistent DB.
    // Lock ordering is pinned elsewhere (import_export.rs manual_restore_... test
    // and backup.rs delete-during-restore).
    let fx = fresh();
    switch(&fx.router, "old-provider").await;
    let filename = create(&fx.router).await;
    switch(&fx.router, "new-provider").await;

    let restore_uri = format!("/api/backups/{filename}/restore");
    let delete_uri = format!("/api/backups/{filename}");
    let ((s1, r1), (s2, r2), (s3, r3)) = tokio::join!(
        web_call(&fx.router, "POST", &restore_uri, None),
        web_call(&fx.router, "POST", &restore_uri, None),
        web_call(&fx.router, "DELETE", &delete_uri, None),
    );

    let mut restored = 0;
    for (status, resp) in [(s1, &r1), (s2, &r2)] {
        match resp["ok"].as_bool() {
            Some(true) => {
                assert_eq!(status, StatusCode::OK, "{resp}");
                assert!(
                    resp["data"].as_str().is_some_and(|s| !s.is_empty()),
                    "{resp}"
                );
                restored += 1;
            }
            // Delete may win the race: the file is then gone for a later restore.
            Some(false) => {
                assert!(
                    status.is_client_error() || status.is_server_error(),
                    "{resp}"
                );
                let error = resp["error"].as_str().unwrap_or_default();
                assert!(
                    !error.is_empty() && !error.contains("task failed"),
                    "{resp}"
                );
            }
            None => panic!("no ok flag: {resp}"),
        }
    }
    match r3["ok"].as_bool() {
        Some(true) => {
            assert_eq!(s3, StatusCode::OK, "{r3}");
            assert!(!fx.backups.join(&filename).exists());
        }
        // Nothing else removes the file, so delete never sees "not found".
        _ => panic!("delete must succeed, it only waits for restore: {s3} {r3}"),
    }

    // The DB is sound: the shared handle answers and a fresh open parses the file.
    let expected = if restored > 0 {
        "old-provider"
    } else {
        "new-provider"
    };
    assert_eq!(current(&fx).as_deref(), Some(expected));
    let providers = data(&fx.router, "GET", "/api/claude/providers", None).await;
    assert!(providers.get("old-provider").is_some(), "{providers}");
    assert!(providers.get("new-provider").is_some(), "{providers}");
    let reopened = cc_switch_lib::Database::init().expect("DB opens after the race");
    assert_eq!(
        reopened.get_current_provider("claude").unwrap().as_deref(),
        Some(expected)
    );
}

#[tokio::test]
#[serial]
async fn backup_locations_list_and_delete() {
    let fx = fresh();
    let skills = ensure_test_home().join(".cc-switch").join("skill-backups");
    let _ = fs::remove_dir_all(&skills);
    fs::create_dir_all(skills.join("gen1")).unwrap();
    fs::write(skills.join("gen1").join("x.txt"), b"1234567").unwrap();
    let locations = data(&fx.router, "GET", "/api/backup-locations", None).await;
    let locations = locations.as_array().expect("array");
    assert!(!locations.is_empty());
    for loc in locations {
        assert!(loc["id"].as_str().is_some_and(|s| !s.is_empty()), "{loc}");
        assert!(loc["path"].as_str().is_some_and(|s| !s.is_empty()), "{loc}");
        assert!(loc["sizeBytes"].is_u64(), "{loc}");
        assert!(loc["itemCount"].is_u64(), "{loc}");
        assert!(loc["deletable"].is_boolean(), "{loc}");
    }

    failure(
        &fx.router,
        "DELETE",
        "/api/backup-locations/no-such-location",
        None,
        StatusCode::BAD_REQUEST,
    )
    .await;

    let skills_loc = locations
        .iter()
        .find(|l| l["id"] == json!("skills"))
        .expect("seeded skills location");
    assert_eq!(skills_loc["deletable"], json!(true));
    assert_eq!(skills_loc["itemCount"], json!(1));
    assert_eq!(skills_loc["sizeBytes"], json!(7));
    assert_eq!(skills_loc["path"], json!(skills.to_string_lossy()));
    let freed = data(&fx.router, "DELETE", "/api/backup-locations/skills", None).await;
    assert_eq!(freed, json!(7));
    assert!(!skills.exists());

    // The DB-backups category is managed one by one: whole-class delete is refused.
    failure(
        &fx.router,
        "DELETE",
        "/api/backup-locations/database",
        None,
        StatusCode::BAD_REQUEST,
    )
    .await;
}
