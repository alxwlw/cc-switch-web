#[allow(dead_code)]
mod support;

use axum::http::StatusCode;
use cc_switch_lib::{update_settings, AppSettings};
use serde_json::{json, Value};
use serial_test::serial;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use support::{create_test_state, ensure_test_home, reset_test_fs, web_call, web_router_with};

/// Router over a fresh state with `opencode_config_dir` pointing at a temp dir.
fn fresh() -> (axum::Router, PathBuf) {
    reset_test_fs();
    let home = ensure_test_home();
    let dir = home.join(".opencode-web-api");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("opencode dir");
    update_settings(AppSettings {
        opencode_config_dir: Some(dir.to_string_lossy().into_owned()),
        ..AppSettings::default()
    })
    .expect("opencode dir override");
    (
        web_router_with(Arc::new(create_test_state().expect("state"))),
        dir,
    )
}

async fn data(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, resp) = web_call(router, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(true), "{method} {uri}: {resp}");
    resp["data"].clone()
}

fn config_path(dir: &std::path::Path) -> PathBuf {
    dir.join("opencode.json")
}

/// Two legacy providers, mirroring the `opencode_tests.rs` fixture shape.
const TWO_PROVIDERS: &str = r#"{
  "provider": {
    "alpha": {"options": {"apiKey": "sk-alpha"}},
    "beta": {"options": {"apiKey": "sk-beta"}}
  }
}"#;

#[tokio::test]
#[serial]
async fn live_provider_ids_lists_both_providers() {
    let (router, dir) = fresh();
    fs::write(config_path(&dir), TWO_PROVIDERS).expect("seed");
    let mut ids: Vec<String> =
        serde_json::from_value(data(&router, "GET", "/api/opencode/live-provider-ids", None).await)
            .expect("string ids");
    ids.sort();
    assert_eq!(ids, vec!["alpha".to_string(), "beta".to_string()]);
}

#[tokio::test]
#[serial]
async fn import_live_imports_two_and_repeat_adds_no_duplicates() {
    let (router, dir) = fresh();
    fs::write(config_path(&dir), TWO_PROVIDERS).expect("seed");

    let (status, resp) = web_call(&router, "POST", "/api/opencode/import-live", None).await;
    assert_eq!(status, StatusCode::OK, "POST import-live: {resp}");
    assert_eq!(resp, json!({"ok": true, "data": 2}), "import-live: {resp}");

    // Live ids are unchanged by the import.
    let mut ids: Vec<String> =
        serde_json::from_value(data(&router, "GET", "/api/opencode/live-provider-ids", None).await)
            .expect("string ids");
    ids.sort();
    assert_eq!(ids, vec!["alpha".to_string(), "beta".to_string()]);

    // Repeat import is a no-op: unchanged entries count as neither
    // imported nor updated (live.rs), so the count is 0 and nothing duplicates.
    let (status, resp) = web_call(&router, "POST", "/api/opencode/import-live", None).await;
    assert_eq!(status, StatusCode::OK, "repeat POST import-live: {resp}");
    assert_eq!(
        resp,
        json!({"ok": true, "data": 0}),
        "repeat import-live: {resp}"
    );
}

#[tokio::test]
#[serial]
async fn default_model_round_trip() {
    let (router, dir) = fresh();
    fs::write(
        config_path(&dir),
        "{\"model\": \"alpha/gpt-4\", \"small_model\": \"beta/mini\"}",
    )
    .expect("seed");

    assert_eq!(
        data(&router, "GET", "/api/opencode/default-model", None).await,
        json!("alpha/gpt-4")
    );
    data(
        &router,
        "PUT",
        "/api/opencode/default-model",
        Some(json!({"model": "beta/mini"})),
    )
    .await;
    assert_eq!(
        data(&router, "GET", "/api/opencode/default-model", None).await,
        json!("beta/mini")
    );
}

#[tokio::test]
#[serial]
async fn default_model_missing_is_null() {
    let (router, dir) = fresh();
    fs::write(config_path(&dir), TWO_PROVIDERS).expect("seed");
    assert_eq!(
        data(&router, "GET", "/api/opencode/default-model", None).await,
        Value::Null
    );
}

#[tokio::test]
#[serial]
async fn default_model_without_slash_surfaces_validation_text() {
    let (router, dir) = fresh();
    fs::write(config_path(&dir), TWO_PROVIDERS).expect("seed");

    let (status, resp) = web_call(
        &router,
        "PUT",
        "/api/opencode/default-model",
        Some(json!({"model": "без-слеша"})),
    )
    .await;
    assert_eq!(resp["ok"], json!(false), "PUT invalid model: {resp}");
    assert!(
        resp["error"]
            .as_str()
            .unwrap_or("")
            .contains("OpenCode model must be \"<provider>/<model>\": без-слеша"),
        "validation text verbatim: {resp}"
    );
    // The mapped AppError surfaces via `fail`.
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{resp}");

    // Missing/non-string bodies are 400, never axum's 422.
    for body in [json!({}), json!({"model": 42}), json!({"model": null})] {
        let (status, resp) = web_call(
            &router,
            "PUT",
            "/api/opencode/default-model",
            Some(body.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "PUT {body}: {resp}");
        assert_eq!(resp["ok"], json!(false), "PUT {body}: {resp}");
    }
}

#[tokio::test]
#[serial]
async fn default_model_write_preserves_comments_and_other_content() {
    let (router, dir) = fresh();
    let path = config_path(&dir);
    fs::write(
        &path,
        "{\n  // keep me\n  \"model\": \"old/a\",\n  \"small_model\": \"old/b\",\n  \"provider\": {\n    \"alpha\": {\"options\": {\"apiKey\": \"sk-alpha\"}}\n  },\n  \"mcp\": {}\n}\n",
    )
    .expect("seed");
    let before = fs::read_to_string(&path).expect("read before");

    data(
        &router,
        "PUT",
        "/api/opencode/default-model",
        Some(json!({"model": "relay/vendor/model-x"})),
    )
    .await;

    let after = fs::read_to_string(&path).expect("read after");
    assert!(after.contains("// keep me"), "comment survives: {after}");
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();
    assert_eq!(
        before_lines.len(),
        after_lines.len(),
        "no lines added or removed:\n{after}"
    );
    let diffs: Vec<(usize, &str, &str)> = before_lines
        .iter()
        .zip(after_lines.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, (a, b))| (i, *a, *b))
        .collect();
    assert_eq!(diffs.len(), 1, "only the model line changes: {diffs:?}");
    assert!(diffs[0].1.contains("\"old/a\""), "{diffs:?}");
    assert!(diffs[0].2.contains("relay/vendor/model-x"), "{diffs:?}");
}
