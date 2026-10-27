#[allow(dead_code)]
mod support;

use axum::http::StatusCode;
use cc_switch_lib::{update_settings, AppSettings, AppState, Provider};
use serde_json::{json, Value};
use serial_test::serial;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use support::{create_test_state, ensure_test_home, reset_test_fs, web_call, web_router_with};

/// Router over a fresh state with `pi_config_dir` / `omp_config_dir` pointing at temp dirs.
fn fresh() -> (axum::Router, Arc<AppState>, PathBuf) {
    reset_test_fs();
    let home = ensure_test_home();
    let pi = home.join(".pi-web-api");
    let omp = home.join(".omp-web-api");
    for dir in [&pi, &omp] {
        let _ = fs::remove_dir_all(dir);
        fs::create_dir_all(dir).expect("agent dir");
    }
    update_settings(AppSettings {
        pi_config_dir: Some(pi.to_string_lossy().into_owned()),
        omp_config_dir: Some(omp.to_string_lossy().into_owned()),
        ..AppSettings::default()
    })
    .expect("agent dir override");
    let state = Arc::new(create_test_state().expect("state"));
    (web_router_with(state.clone()), state, pi)
}

async fn data(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, resp) = web_call(router, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(true), "{method} {uri}: {resp}");
    resp["data"].clone()
}

async fn conflict(router: &axum::Router, method: &str, uri: &str, body: Value) -> Value {
    let (status, resp) = web_call(router, method, uri, Some(body)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(false), "{resp}");
    resp
}

#[tokio::test]
#[serial]
async fn current_state_routes_reach_app_route() {
    let (router, _state, _pi) = fresh();
    for app in ["pi", "omp"] {
        let state = data(&router, "GET", &format!("/api/{app}/current-state"), None).await;
        assert_eq!(state["enabledProviderIds"], json!([]), "{app}: {state}");
        assert_eq!(state["defaultProviderId"], Value::Null, "{app}: {state}");
    }
    let (status, resp) = web_call(&router, "GET", "/api/claude/current-state", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{resp}");
    assert_eq!(resp["ok"], json!(false));
}

#[tokio::test]
#[serial]
async fn session_discovery_responds() {
    let (router, _state, _pi) = fresh();
    let d = data(&router, "GET", "/api/pi/session-discovery", None).await;
    assert!(d["status"].is_string(), "{d}");
}

#[tokio::test]
#[serial]
async fn prompt_file_read_replace_and_stale_revision_conflicts() {
    let (router, _state, pi) = fresh();
    let uri = "/api/pi/prompt-files/system_override";

    let snap = data(&router, "GET", uri, None).await;
    assert_eq!(snap["exists"], json!(false));
    assert_eq!(snap["revision"], json!("missing"));

    let first = data(
        &router,
        "PUT",
        uri,
        Some(json!({ "expectedRevision": "missing", "content": "one" })),
    )
    .await;
    assert_eq!(first["content"], json!("one"));
    let rev1 = first["revision"].as_str().unwrap().to_string();
    assert_ne!(rev1, "missing");
    assert_eq!(fs::read_to_string(pi.join("SYSTEM.md")).unwrap(), "one");

    // Second tab with the same (now stale) revision must not overwrite.
    let second = data(
        &router,
        "PUT",
        uri,
        Some(json!({ "expectedRevision": rev1, "content": "two" })),
    )
    .await;
    assert_ne!(second["revision"], json!(rev1));
    let resp = conflict(
        &router,
        "PUT",
        uri,
        json!({ "expectedRevision": rev1, "content": "three" }),
    )
    .await;
    let err = resp["error"].as_str().unwrap();
    assert!(err.contains("changed outside CC Switch"), "{err}");
    assert!(err.starts_with("并发冲突: "), "{err}");
    assert_eq!(fs::read_to_string(pi.join("SYSTEM.md")).unwrap(), "two");

    // External edit, then replace with the revision we last saw.
    let rev2 = second["revision"].as_str().unwrap().to_string();
    fs::write(pi.join("SYSTEM.md"), "external").unwrap();
    conflict(
        &router,
        "PUT",
        uri,
        json!({ "expectedRevision": rev2, "content": "mine" }),
    )
    .await;
    conflict(&router, "DELETE", uri, json!({ "expectedRevision": rev2 })).await;
    assert_eq!(
        fs::read_to_string(pi.join("SYSTEM.md")).unwrap(),
        "external"
    );

    let cur = data(&router, "GET", uri, None).await;
    let deleted = data(
        &router,
        "DELETE",
        uri,
        Some(json!({ "expectedRevision": cur["revision"] })),
    )
    .await;
    assert_eq!(deleted, json!(true));
    assert!(!pi.join("SYSTEM.md").exists());
}

#[tokio::test]
#[serial]
async fn prompt_file_kind_is_validated_as_json_400() {
    let (router, _state, _pi) = fresh();
    let (status, resp) = web_call(&router, "GET", "/api/pi/prompt-files/bogus", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{resp}");
    assert_eq!(resp["ok"], json!(false));
    let append = data(&router, "GET", "/api/pi/prompt-files/system_append", None).await;
    assert_eq!(append["revision"], json!("missing"));
}

#[tokio::test]
#[serial]
async fn prompt_templates_upsert_rename_and_conflicts() {
    let (router, _state, pi) = fresh();
    assert_eq!(
        data(&router, "GET", "/api/pi/prompt-templates", None).await,
        json!([])
    );

    let alpha = data(
        &router,
        "PUT",
        "/api/pi/prompt-templates/alpha",
        Some(json!({ "originalSlug": null, "expectedRevision": "missing", "content": "A" })),
    )
    .await;
    assert_eq!(alpha["slug"], json!("alpha"));
    assert_eq!(
        fs::read_to_string(pi.join("prompts/alpha.md")).unwrap(),
        "A"
    );
    let alpha_rev = alpha["revision"].as_str().unwrap().to_string();

    // rename alpha -> beta
    let beta = data(
        &router,
        "PUT",
        "/api/pi/prompt-templates/beta",
        Some(json!({ "originalSlug": "alpha", "expectedRevision": alpha_rev, "content": "B" })),
    )
    .await;
    assert_eq!(beta["slug"], json!("beta"));
    assert!(!pi.join("prompts/alpha.md").exists());
    assert_eq!(fs::read_to_string(pi.join("prompts/beta.md")).unwrap(), "B");
    let list = data(&router, "GET", "/api/pi/prompt-templates", None).await;
    assert_eq!(list.as_array().unwrap().len(), 1, "{list}");
    assert_eq!(list[0]["slug"], json!("beta"));

    // rename onto an existing slug conflicts and leaves the original alone
    let gamma = data(
        &router,
        "PUT",
        "/api/pi/prompt-templates/gamma",
        Some(json!({ "originalSlug": null, "expectedRevision": "missing", "content": "G" })),
    )
    .await;
    let resp = conflict(
        &router,
        "PUT",
        "/api/pi/prompt-templates/beta",
        json!({ "originalSlug": "gamma", "expectedRevision": gamma["revision"], "content": "X" }),
    )
    .await;
    assert!(resp["error"]
        .as_str()
        .unwrap()
        .contains("changed outside CC Switch"));
    assert_eq!(
        fs::read_to_string(pi.join("prompts/gamma.md")).unwrap(),
        "G"
    );
    assert_eq!(fs::read_to_string(pi.join("prompts/beta.md")).unwrap(), "B");

    // delete with a wrong revision conflicts; with the right one succeeds
    conflict(
        &router,
        "DELETE",
        "/api/pi/prompt-templates/beta",
        json!({ "expectedRevision": "deadbeef" }),
    )
    .await;
    assert!(pi.join("prompts/beta.md").exists());
    let deleted = data(
        &router,
        "DELETE",
        "/api/pi/prompt-templates/beta",
        Some(json!({ "expectedRevision": beta["revision"] })),
    )
    .await;
    assert_eq!(deleted, json!(true));
    assert!(!pi.join("prompts/beta.md").exists());
}

#[tokio::test]
#[serial]
async fn usage_script_is_saved_on_provider_and_app_is_checked() {
    let (router, state, _pi) = fresh();
    let provider = Provider::with_id(
        "pi-web".into(),
        "Pi Web".into(),
        json!({ "baseUrl": "https://example.com/v1", "api": "openai-completions" }),
        None,
    );
    state.db.save_provider("pi", &provider).expect("seed");

    let script = json!({ "enabled": true, "language": "javascript", "code": "return 1;" });
    let saved = data(
        &router,
        "PUT",
        "/api/pi/providers/pi-web/usage-script",
        Some(json!({ "usageScript": script })),
    )
    .await;
    assert_eq!(saved, json!(true));
    let stored = state
        .db
        .get_provider_by_id("pi-web", "pi")
        .unwrap()
        .unwrap()
        .meta
        .unwrap()
        .usage_script
        .unwrap();
    assert_eq!(stored.code, "return 1;");
    assert!(stored.enabled);

    let (status, resp) = web_call(
        &router,
        "PUT",
        "/api/claude/providers/pi-web/usage-script",
        Some(json!({ "usageScript": script })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{resp}");
}
