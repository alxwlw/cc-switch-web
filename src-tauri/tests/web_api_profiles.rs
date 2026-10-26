mod support;

use axum::http::StatusCode;
use cc_switch_lib::{AppState, AppType, Provider, ProviderService};
use serde_json::{json, Value};
use std::sync::Arc;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex, web_call};
use tokio::sync::broadcast::{error::TryRecvError, Receiver};

const LIST: &str = "/api/profiles";

fn claude_provider(id: &str, token: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        id.to_uppercase(),
        json!({ "env": {
            "ANTHROPIC_AUTH_TOKEN": token,
            "ANTHROPIC_BASE_URL": "https://api.test"
        }}),
        None,
    )
}

struct Fixture {
    state: Arc<AppState>,
    router: axum::Router,
    home: std::path::PathBuf,
}

/// Two claude providers (p1 current, live settings.json = p1) in a temp HOME.
fn fixture() -> Fixture {
    reset_test_fs();
    let home = ensure_test_home();
    let state = Arc::new(create_test_state().expect("test state"));
    for (id, key) in [("p1", "key-1"), ("p2", "key-2")] {
        state
            .db
            .save_provider("claude", &claude_provider(id, key))
            .unwrap();
    }
    state.db.set_current_provider("claude", "p1").unwrap();
    let dir = home.join(".claude");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("settings.json"),
        serde_json::to_string_pretty(&claude_provider("p1", "key-1").settings_config).unwrap(),
    )
    .unwrap();
    let router = cc_switch_lib::web_api::router(state.clone());
    Fixture {
        state,
        router,
        home: home.to_path_buf(),
    }
}

fn live_token(f: &Fixture) -> String {
    let raw = std::fs::read_to_string(f.home.join(".claude/settings.json")).unwrap();
    let v: Value = serde_json::from_str(&raw).unwrap();
    v["env"]["ANTHROPIC_AUTH_TOKEN"].as_str().unwrap().into()
}

async fn create(f: &Fixture, name: &str, scope: &str) -> Value {
    let (status, body) = web_call(
        &f.router,
        "POST",
        LIST,
        Some(json!({ "name": name, "scope": scope })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["data"].clone()
}

fn drain(rx: &mut Receiver<cc_switch_lib::events::Event>) -> Vec<(String, Value)> {
    let mut out = vec![];
    loop {
        match rx.try_recv() {
            Ok(e) => out.push((e.name, e.payload)),
            Err(TryRecvError::Empty) => return out,
            Err(e) => panic!("{e:?}"),
        }
    }
}

#[tokio::test]
async fn create_then_list_returns_dto_with_empty_current_ids() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture();

    let p = create(&f, "  Proj A ", "claude").await;
    assert_eq!(p["name"], "Proj A");
    assert_eq!(p["payload"]["providers"]["claude"], "p1");
    assert_eq!(p["payload"]["providers"]["codex"], Value::Null);

    let (status, list) = web_call(&f.router, "GET", LIST, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["data"]["profiles"].as_array().unwrap().len(), 1);
    assert_eq!(list["data"]["profiles"][0]["id"], p["id"]);
    assert_eq!(
        list["data"]["currentIds"],
        json!({ "claude": null, "claudeDesktop": null, "codex": null })
    );
}

#[tokio::test]
async fn apply_switches_provider_writes_live_and_emits_in_desktop_order() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture();
    let p = create(&f, "A", "claude").await;
    let id = p["id"].as_str().unwrap();

    ProviderService::switch(&f.state, AppType::Claude, "p2").unwrap();
    assert_eq!(live_token(&f), "key-2");
    let mut rx = f.state.events.subscribe();

    let (status, body) = web_call(
        &f.router,
        "POST",
        &format!("{LIST}/{id}/apply"),
        Some(json!({ "scope": "claude" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true);
    assert!(body["data"].is_array(), "warnings array: {body}");

    assert_eq!(
        f.state
            .db
            .get_current_provider("claude")
            .unwrap()
            .as_deref(),
        Some("p1")
    );
    assert_eq!(live_token(&f), "key-1");

    let events = drain(&mut rx);
    assert_eq!(
        events,
        vec![
            (
                "provider-switched".to_string(),
                json!({
                    "appType": "claude",
                    "proxyEnabled": false,
                    "autoFailoverEnabled": false,
                    "providerId": "p1",
                })
            ),
            (
                "profile-applied".to_string(),
                json!({ "profileId": id, "scope": "claude" })
            ),
        ]
    );

    let (_, list) = web_call(&f.router, "GET", LIST, None).await;
    assert_eq!(list["data"]["currentIds"]["claude"], id);
}

#[tokio::test]
async fn apply_under_proxy_takeover_reports_proxy_enabled() {
    use cc_switch_lib::live::engine::DeviceStore;
    use cc_switch_lib::mode::state::{self, Mode};
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture();
    let p = create(&f, "A", "claude").await;
    let id = p["id"].as_str().unwrap();
    state::update(&DeviceStore::for_device(), |live| {
        let app = live.apps.entry("claude".to_string()).or_default();
        let mut mode = app.mode_state();
        mode.mode = Some(Mode::Proxy);
        app.set_mode_state(mode);
    })
    .unwrap();
    let mut rx = f.state.events.subscribe();

    let (status, body) = web_call(
        &f.router,
        "POST",
        &format!("{LIST}/{id}/apply"),
        Some(json!({ "scope": "claude" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let events = drain(&mut rx);
    assert_eq!(events.len(), 2, "{events:?}");
    assert_eq!(events[0].0, "provider-switched");
    assert_eq!(events[0].1["appType"], "claude");
    assert_eq!(events[0].1["proxyEnabled"], true);
    assert_eq!(events[1].0, "profile-applied");
}

#[tokio::test]
async fn failed_apply_emits_nothing_and_unknown_profile_is_an_error() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture();
    let mut rx = f.state.events.subscribe();

    let (status, body) = web_call(
        &f.router,
        "POST",
        &format!("{LIST}/nope/apply"),
        Some(json!({ "scope": "claude" })),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("Profile not found"));
    assert!(drain(&mut rx).is_empty());
}

#[tokio::test]
async fn update_renames_and_resnapshot_overwrites_scope_snapshot() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture();
    let p = create(&f, "A", "claude").await;
    let id = p["id"].as_str().unwrap();
    ProviderService::switch(&f.state, AppType::Claude, "p2").unwrap();

    let (status, body) = web_call(
        &f.router,
        "PATCH",
        &format!("{LIST}/{id}"),
        Some(json!({ "name": "Renamed", "resnapshot": true, "scope": "claude" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["name"], "Renamed");
    assert_eq!(body["data"]["payload"]["providers"]["claude"], "p2");

    // name only: snapshot untouched
    let (_, body) = web_call(
        &f.router,
        "PATCH",
        &format!("{LIST}/{id}"),
        Some(json!({ "name": "Again" })),
    )
    .await;
    assert_eq!(body["data"]["name"], "Again");
    assert_eq!(body["data"]["payload"]["providers"]["claude"], "p2");

    // resnapshot without scope is rejected by the service
    let (status, body) = web_call(
        &f.router,
        "PATCH",
        &format!("{LIST}/{id}"),
        Some(json!({ "resnapshot": true })),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["ok"], false);
}

#[tokio::test]
async fn clear_current_nulls_only_that_scope_and_delete_removes_profile() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture();
    let p = create(&f, "A", "claude").await;
    let id = p["id"].as_str().unwrap();
    f.state
        .db
        .set_current_profile_id("claude", Some(id))
        .unwrap();
    f.state
        .db
        .set_current_profile_id("codex", Some(id))
        .unwrap();

    let (status, body) = web_call(
        &f.router,
        "POST",
        "/api/profiles/clear-current",
        Some(json!({ "scope": "claude" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "ok": true, "data": true }));
    let (_, list) = web_call(&f.router, "GET", LIST, None).await;
    assert_eq!(list["data"]["currentIds"]["claude"], Value::Null);
    assert_eq!(list["data"]["currentIds"]["codex"], id);

    let (status, body) = web_call(&f.router, "DELETE", &format!("{LIST}/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "ok": true, "data": true }));
    let (_, list) = web_call(&f.router, "GET", LIST, None).await;
    assert_eq!(list["data"]["profiles"], json!([]));
    assert_eq!(list["data"]["currentIds"]["codex"], Value::Null);
}

#[tokio::test]
async fn unknown_scope_is_400_on_every_scoped_route() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture();
    let p = create(&f, "A", "claude").await;
    let id = p["id"].as_str().unwrap();
    let mut rx = f.state.events.subscribe();
    let bad = json!({ "scope": "gemini" });

    for (method, uri, body) in [
        (
            "POST",
            LIST.to_string(),
            json!({ "name": "x", "scope": "gemini" }),
        ),
        (
            "PATCH",
            format!("{LIST}/{id}"),
            json!({ "scope": "gemini" }),
        ),
        ("POST", format!("{LIST}/{id}/apply"), bad.clone()),
        ("POST", format!("{LIST}/clear-current"), bad.clone()),
    ] {
        let (status, resp) = web_call(&f.router, method, &uri, Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {uri}: {resp}");
        assert_eq!(resp["ok"], false);
        assert!(resp["error"].as_str().unwrap().contains("gemini"));
    }
    assert!(drain(&mut rx).is_empty());
    let (_, list) = web_call(&f.router, "GET", LIST, None).await;
    assert_eq!(list["data"]["profiles"].as_array().unwrap().len(), 1);
}
