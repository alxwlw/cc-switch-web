mod support;

use axum::http::StatusCode;
use std::collections::HashSet;
use support::{ensure_test_home, reset_test_fs, test_mutex, web_call, web_router};

const URI: &str = "/api/claude/prompts/import-from-file";

#[tokio::test]
async fn import_from_file_returns_new_prompt_id_and_stores_disabled_prompt() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home();
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(home.join(".claude/CLAUDE.md"), "live instructions").unwrap();
    let router = web_router();

    let (status, body) = web_call(&router, "POST", URI, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    let id = body["data"].as_str().expect("data is the prompt id");
    assert!(id.starts_with("imported-"), "id: {id}");

    let (status, list) = web_call(&router, "GET", "/api/claude/prompts", None).await;
    assert_eq!(status, StatusCode::OK);
    let prompt = &list["data"][id];
    assert_eq!(prompt["content"], "live instructions");
    assert_eq!(prompt["enabled"], false);
}

// The service ids imports by unix-second timestamp: two imports in the same second
// overwrite one record, in different seconds create two. Either way no id is duplicated.
#[tokio::test]
async fn import_from_file_twice_never_duplicates_an_id() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home();
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(home.join(".claude/CLAUDE.md"), "live instructions").unwrap();
    let router = web_router();

    let mut ids = HashSet::new();
    for _ in 0..2 {
        let (status, body) = web_call(&router, "POST", URI, None).await;
        assert_eq!(status, StatusCode::OK);
        ids.insert(body["data"].as_str().unwrap().to_string());
    }
    let (_, list) = web_call(&router, "GET", "/api/claude/prompts", None).await;
    let stored: HashSet<String> = list["data"].as_object().unwrap().keys().cloned().collect();
    assert_eq!(stored, ids);
}

#[tokio::test]
async fn import_from_file_without_live_file_passes_service_error_verbatim() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) = web_call(&router, "POST", URI, None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "提示词文件不存在");
}

#[tokio::test]
async fn import_from_file_unknown_app_is_bad_request() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) =
        web_call(&router, "POST", "/api/nope/prompts/import-from-file", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["ok"], false);
    assert!(body["error"].is_string());
}
