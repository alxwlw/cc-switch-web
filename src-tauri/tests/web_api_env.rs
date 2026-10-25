//! `/api/env/conflicts/cleanup` and `/api/env/restore`. The unix branch of
//! `env_manager` edits shell rc files named by `sourcePath` ("path:line"), so every
//! fixture lives under the test home; the registry branch (windows) is never run here.
#![cfg(not(target_os = "windows"))]

mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};
use std::path::PathBuf;
use support::{ensure_test_home, reset_test_fs, test_mutex, web_call, web_router};

const CLEANUP: &str = "/api/env/conflicts/cleanup";
const RESTORE: &str = "/api/env/restore";

fn rc_file(name: &str, content: &str) -> PathBuf {
    let path = ensure_test_home().join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn conflict(var: &str, value: &str, source_type: &str, source_path: &str) -> Value {
    json!({"varName": var, "varValue": value, "sourceType": source_type, "sourcePath": source_path})
}

/// Raw body (no JSON content check) -> status and text, for extractor rejections.
async fn raw_post(router: &axum::Router, uri: &str, body: &str) -> (StatusCode, String) {
    use axum::body::Body;
    use axum::http::{header, Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

#[tokio::test]
async fn cleanup_backs_up_then_removes_only_the_named_variable_and_restore_puts_it_back() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let rc = rc_file(
        "env_test_rc",
        "export PATH=/bin\nexport ANTHROPIC_API_KEY=sk-secret\nexport KEEP_ME=1",
    );
    let src = format!("{}:2", rc.display());
    let router = web_router();

    let (status, body) = web_call(
        &router,
        "POST",
        CLEANUP,
        Some(json!({"conflicts": [conflict("ANTHROPIC_API_KEY", "sk-secret", "file", &src)]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    assert_eq!(
        std::fs::read_to_string(&rc).unwrap(),
        "export PATH=/bin\nexport KEEP_ME=1"
    );

    let backup_path = body["data"]["backupPath"].as_str().unwrap().to_string();
    assert!(
        backup_path.starts_with(ensure_test_home().to_str().unwrap()),
        "backup must land in the test home: {backup_path}"
    );
    assert!(backup_path.contains("env-backup-"));
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(&backup_path).unwrap()).unwrap();
    assert_eq!(saved["backupPath"], backup_path);
    assert_eq!(
        saved["conflicts"],
        json!([conflict("ANTHROPIC_API_KEY", "sk-secret", "file", &src)])
    );
    assert_eq!(body["data"]["conflicts"], saved["conflicts"]);

    let (status, body) = web_call(
        &router,
        "POST",
        RESTORE,
        Some(json!({"backupPath": backup_path})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"ok": true, "data": null}));
    assert_eq!(
        std::fs::read_to_string(&rc).unwrap(),
        "export PATH=/bin\nexport KEEP_ME=1\nexport ANTHROPIC_API_KEY=sk-secret"
    );
}

#[tokio::test]
async fn cleanup_of_a_missing_file_fails_verbatim_but_keeps_the_backup() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home();
    let missing = home.join("no_such_rc");
    let src = format!("{}:1", missing.display());
    let router = web_router();

    let (status, body) = web_call(
        &router,
        "POST",
        CLEANUP,
        Some(json!({"conflicts": [conflict("X_KEY", "v", "file", &src)]})),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["ok"], false);
    let err = body["error"].as_str().unwrap();
    assert!(err.starts_with("删除环境变量失败: 读取文件失败 "), "{err}");
    assert!(err.contains(&missing.display().to_string()), "{err}");
    let backups: Vec<_> = std::fs::read_dir(home.join(".cc-switch").join("backups"))
        .unwrap()
        .collect();
    assert_eq!(
        backups.len(),
        1,
        "backup is written before the delete fails"
    );
    assert!(err.contains(&backups[0].as_ref().unwrap().path().display().to_string()));
}

#[tokio::test]
async fn unix_system_source_is_a_noop_on_cleanup_and_unrestorable() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) = web_call(
        &router,
        "POST",
        CLEANUP,
        Some(json!({"conflicts": [conflict("SYS_VAR", "1", "system", "process env")]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);
    let backup_path = body["data"]["backupPath"].as_str().unwrap().to_string();

    let (status, body) = web_call(
        &router,
        "POST",
        RESTORE,
        Some(json!({"backupPath": backup_path})),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body,
        json!({"ok": false, "error": "无法恢复类型为 system 的环境变量"})
    );
}

#[tokio::test]
async fn unknown_source_type_is_rejected_without_touching_anything() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    let (status, body) = web_call(
        &router,
        "POST",
        CLEANUP,
        Some(json!({"conflicts": [conflict("A", "1", "registry", "HKEY")]})),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["ok"], false);
    assert!(body["error"]
        .as_str()
        .unwrap()
        .starts_with("删除环境变量失败: 未知的环境变量来源类型: registry. 备份已保存到: "));
}

#[tokio::test]
async fn restore_with_missing_or_corrupt_backup_fails_as_ok_false() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home();
    let router = web_router();

    let missing = home.join("nope.json");
    let (status, body) = web_call(
        &router,
        "POST",
        RESTORE,
        Some(json!({"backupPath": missing.to_str().unwrap()})),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["ok"], false);
    assert!(body["error"]
        .as_str()
        .unwrap()
        .starts_with("读取备份文件失败: "));

    let corrupt = rc_file("corrupt.json", "{not json");
    let (status, body) = web_call(
        &router,
        "POST",
        RESTORE,
        Some(json!({"backupPath": corrupt.to_str().unwrap()})),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body["error"]
        .as_str()
        .unwrap()
        .starts_with("解析备份文件失败: "));
}

#[tokio::test]
async fn malformed_bodies_are_rejected_by_the_json_extractor() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let router = web_router();

    // syntactically broken JSON
    let (status, _) = raw_post(&router, CLEANUP, "{not json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // valid JSON, wrong shape / missing field
    let (status, _) = raw_post(&router, CLEANUP, r#"{"conflicts": "x"}"#).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = raw_post(&router, RESTORE, "{}").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    // a conflict missing a required field
    let (status, _) = raw_post(&router, CLEANUP, r#"{"conflicts": [{"varName": "A"}]}"#).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}
