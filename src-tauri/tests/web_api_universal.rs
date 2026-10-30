mod support;

use axum::http::StatusCode;
use cc_switch_lib::AppState;
use serde_json::{json, Value};
use std::sync::Arc;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex, web_call};
use tokio::sync::broadcast::{error::TryRecvError, Receiver};

const LIST: &str = "/api/universal-providers";

fn provider_json(id: &str, claude: bool, codex: bool) -> Value {
    json!({
        "id": id,
        "name": format!("Universal {id}"),
        "providerType": "newapi",
        "apps": { "claude": claude, "codex": codex, "gemini": false },
        "baseUrl": "https://api.example.com",
        "apiKey": "sk-universal",
        "models": {
            "claude": { "model": "claude-x" },
            "codex": { "model": "gpt-x" }
        }
    })
}

struct Fixture {
    state: Arc<AppState>,
    router: axum::Router,
    rx: Receiver<cc_switch_lib::events::Event>,
}

fn fixture() -> Fixture {
    reset_test_fs();
    ensure_test_home();
    let state = Arc::new(create_test_state().expect("test state"));
    let rx = state.events.subscribe();
    let router = cc_switch_lib::web_api::router(state.clone());
    Fixture { state, router, rx }
}

async fn put(f: &Fixture, id: &str, claude: bool, codex: bool) -> (StatusCode, Value) {
    support::web_call(
        &f.router,
        "PUT",
        LIST,
        Some(json!({ "provider": provider_json(id, claude, codex) })),
    )
    .await
}

fn child_ids(state: &AppState, app: &str) -> Vec<String> {
    let mut ids: Vec<String> = state
        .db
        .get_all_providers(app)
        .unwrap()
        .keys()
        .cloned()
        .collect();
    ids.sort();
    ids
}

fn assert_no_event(rx: &mut Receiver<cc_switch_lib::events::Event>) {
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
}

fn assert_synced_event(rx: &mut Receiver<cc_switch_lib::events::Event>, action: &str, id: &str) {
    let ev = rx.try_recv().expect("universal-provider-synced emitted");
    assert_eq!(ev.name, "universal-provider-synced");
    assert_eq!(ev.payload, json!({ "action": action, "id": id }));
    assert_no_event(rx);
}

#[tokio::test]
async fn upsert_then_get_and_list_return_the_desktop_dto_and_emit_upsert() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();

    let (status, body) = put(&f, "u1", true, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "ok": true, "data": true }));
    assert_synced_event(&mut f.rx, "upsert", "u1");

    let (_, list) = web_call(&f.router, "GET", LIST, None).await;
    let entry = &list["data"]["u1"];
    assert_eq!(entry["providerType"], "newapi");
    assert_eq!(entry["baseUrl"], "https://api.example.com");
    assert_eq!(entry["apiKey"], "sk-universal");
    assert_eq!(
        entry["apps"],
        json!({"claude": true, "codex": true, "gemini": false})
    );

    let (_, one) = web_call(&f.router, "GET", &format!("{LIST}/u1"), None).await;
    assert_eq!(one["data"]["name"], "Universal u1");
    let (status, none) = web_call(&f.router, "GET", &format!("{LIST}/missing"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(none, json!({ "ok": true, "data": null }));
    assert_no_event(&mut f.rx);
}

#[tokio::test]
async fn failed_upsert_reports_service_error_and_emits_nothing() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();

    let mut p = provider_json("bad", true, false);
    p["icon"] = json!("data:image/png;base64,AAAA");
    let (status, body) = web_call(&f.router, "PUT", LIST, Some(json!({ "provider": p }))).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["ok"], false);
    assert!(
        body["error"].as_str().unwrap().contains("自定义图标无效"),
        "{body}"
    );
    assert_no_event(&mut f.rx);
    let (_, list) = web_call(&f.router, "GET", LIST, None).await;
    assert_eq!(list["data"], json!({}));
}

#[tokio::test]
async fn sync_creates_children_for_enabled_apps_only_and_emits_sync() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();
    put(&f, "u1", true, false).await;
    assert_synced_event(&mut f.rx, "upsert", "u1");

    let (status, body) = web_call(&f.router, "POST", &format!("{LIST}/u1/sync"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "ok": true, "data": true }));
    assert_synced_event(&mut f.rx, "sync", "u1");

    assert_eq!(child_ids(&f.state, "claude"), vec!["universal-claude-u1"]);
    assert!(child_ids(&f.state, "codex").is_empty());
    assert!(child_ids(&f.state, "gemini").is_empty());
}

#[tokio::test]
async fn sync_rewrites_live_file_of_the_current_child() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();
    let home = ensure_test_home();
    put(&f, "u1", true, false).await;
    web_call(&f.router, "POST", &format!("{LIST}/u1/sync"), None).await;
    f.state
        .db
        .set_current_provider("claude", "universal-claude-u1")
        .unwrap();
    while f.rx.try_recv().is_ok() {}

    // change the key, re-sync: the current child's live file must follow
    let mut p = provider_json("u1", true, false);
    p["apiKey"] = json!("sk-rotated");
    web_call(&f.router, "PUT", LIST, Some(json!({ "provider": p }))).await;
    let (_, body) = web_call(&f.router, "POST", &format!("{LIST}/u1/sync"), None).await;
    assert_eq!(body["ok"], true, "{body}");

    let live = std::fs::read_to_string(home.join(".claude/settings.json")).expect("live file");
    let live: Value = serde_json::from_str(&live).unwrap();
    assert_eq!(live["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-rotated");
}

#[tokio::test]
async fn partial_sync_failure_is_ok_false_with_service_text_keeps_children_and_emits_nothing() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();
    let home = ensure_test_home();
    put(&f, "u1", true, true).await;
    web_call(&f.router, "POST", &format!("{LIST}/u1/sync"), None).await;
    for app in ["claude", "codex"] {
        f.state
            .db
            .set_current_provider(app, &format!("universal-{app}-u1"))
            .unwrap();
    }
    // Claude's live file cannot be written (a directory sits where the file goes);
    // Codex stays writable.
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    let _ = std::fs::remove_file(home.join(".claude/settings.json"));
    std::fs::create_dir_all(home.join(".claude/settings.json")).unwrap();
    while f.rx.try_recv().is_ok() {}

    let mut p = provider_json("u1", true, true);
    p["apiKey"] = json!("sk-rotated");
    web_call(&f.router, "PUT", LIST, Some(json!({ "provider": p }))).await;
    assert_synced_event(&mut f.rx, "upsert", "u1");

    let (status, body) = web_call(&f.router, "POST", &format!("{LIST}/u1/sync"), None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body,
        json!({
            "ok": false,
            "error": "统一供应商已保存到数据库，但以下应用的配置文件未能写入，仍是旧内容：claude。请重试同步，或切换一次该应用的供应商。"
        })
    );
    assert_no_event(&mut f.rx);

    // DB has both children, with the new key; the healthy app's live file was updated.
    assert_eq!(child_ids(&f.state, "claude"), vec!["universal-claude-u1"]);
    assert_eq!(child_ids(&f.state, "codex"), vec!["universal-codex-u1"]);
    let claude_child = f
        .state
        .db
        .get_provider_by_id("universal-claude-u1", "claude")
        .unwrap()
        .unwrap();
    assert_eq!(
        claude_child.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
        "sk-rotated"
    );
    // Codex was not written before this sync (its child was never current-projected),
    // so a config.toml carrying the child's model proves the healthy app was updated.
    let codex_cfg = std::fs::read_to_string(home.join(".codex/config.toml")).expect("codex live");
    assert!(codex_cfg.contains("gpt-x"), "{codex_cfg}");
}

#[tokio::test]
async fn sync_of_unknown_id_is_ok_false_and_emits_nothing() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();

    let (status, body) = web_call(&f.router, "POST", &format!("{LIST}/nope/sync"), None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body,
        json!({ "ok": false, "error": "统一供应商 nope 不存在" })
    );
    assert_no_event(&mut f.rx);
}

#[tokio::test]
async fn delete_removes_provider_and_children_and_emits_delete() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();
    put(&f, "u1", true, true).await;
    web_call(&f.router, "POST", &format!("{LIST}/u1/sync"), None).await;
    while f.rx.try_recv().is_ok() {}

    let (status, body) = web_call(&f.router, "DELETE", &format!("{LIST}/u1"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "ok": true, "data": true }));
    assert_synced_event(&mut f.rx, "delete", "u1");

    let (_, list) = web_call(&f.router, "GET", LIST, None).await;
    assert_eq!(list["data"], json!({}));
    assert!(child_ids(&f.state, "claude").is_empty());
    assert!(child_ids(&f.state, "codex").is_empty());
}

// The service has no "not found" branch on delete (always Ok(true)), so the only
// reachable failure is a DB write error; force it with a trigger on the settings row
// that holds the universal providers (src/database/dao/universal_providers.rs:62).
#[tokio::test]
async fn failed_delete_reports_error_and_emits_nothing() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let mut f = fixture();
    let home = ensure_test_home();
    put(&f, "u1", true, false).await;
    while f.rx.try_recv().is_ok() {}

    let conn = rusqlite::Connection::open(home.join(".cc-switch/cc-switch.db")).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER block_universal BEFORE INSERT ON settings \
         WHEN NEW.key = 'universal_providers' \
         BEGIN SELECT RAISE(ABORT, 'blocked-by-test'); END;",
    )
    .unwrap();

    let (status, body) = web_call(&f.router, "DELETE", &format!("{LIST}/u1"), None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["ok"], false);
    assert!(
        body["error"].as_str().unwrap().contains("blocked-by-test"),
        "{body}"
    );
    assert_no_event(&mut f.rx);
}

/// Resulting state of deleting a universal provider whose child is the current
/// provider of an app: (current provider id, child rows left).
async fn delete_with_current_child_via_web() -> (Option<String>, Vec<String>) {
    let f = fixture();
    put(&f, "u1", true, false).await;
    web_call(&f.router, "POST", &format!("{LIST}/u1/sync"), None).await;
    f.state
        .db
        .set_current_provider("claude", "universal-claude-u1")
        .unwrap();
    let (_, body) = web_call(&f.router, "DELETE", &format!("{LIST}/u1"), None).await;
    assert_eq!(body["ok"], true);
    (
        f.state.db.get_current_provider("claude").unwrap(),
        child_ids(&f.state, "claude"),
    )
}

#[tokio::test]
async fn delete_of_universal_provider_with_current_child_matches_desktop_command() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());

    let web = delete_with_current_child_via_web().await;

    // Desktop path: the Tauri command is a one-line wrapper over this same helper.
    reset_test_fs();
    ensure_test_home();
    let state = create_test_state().unwrap();
    let provider: cc_switch_lib::UniversalProvider =
        serde_json::from_value(provider_json("u1", true, false)).unwrap();
    cc_switch_lib::upsert_universal_provider_emit(&state, provider).unwrap();
    cc_switch_lib::sync_universal_provider_emit(&state, "u1").unwrap();
    state
        .db
        .set_current_provider("claude", "universal-claude-u1")
        .unwrap();
    cc_switch_lib::delete_universal_provider_emit(&state, "u1").unwrap();
    let desktop = (
        state.db.get_current_provider("claude").unwrap(),
        child_ids(&state, "claude"),
    );

    assert_eq!(web, desktop);
    // Actual state: the active child row is deleted and nothing replaces it, so the
    // app is left with no current provider (live file untouched).
    assert_eq!(web, (None, Vec::<String>::new()));
}
