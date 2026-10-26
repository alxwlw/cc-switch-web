mod support;

use axum::http::{header, StatusCode};
use cc_switch_lib::server_log::log_file_path;
use cc_switch_lib::AppState;
use serde_json::{json, Value};
use std::sync::Arc;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex, web_call};

fn fixture() -> axum::Router {
    reset_test_fs();
    ensure_test_home();
    let state = Arc::new(create_test_state().expect("test state"));
    cc_switch_lib::web_api::router(state as Arc<AppState>)
}

fn write_log(lines: usize) -> std::path::PathBuf {
    let path = log_file_path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let body: String = (1..=lines).map(|i| format!("log line {i}\n")).collect();
    std::fs::write(&path, body).unwrap();
    path
}

fn lines_of(body: &Value) -> Vec<String> {
    body["data"]["lines"]
        .as_array()
        .expect("lines array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn rectifier_config_round_trips_through_put_and_get() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fixture();
    let (status, body) = web_call(&router, "GET", "/api/settings/rectifier", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["data"]["enabled"],
        json!(true),
        "default is on: {body}"
    );

    let config = json!({
        "enabled": false,
        "requestThinkingSignature": true,
        "requestThinkingBudget": false,
        "requestMediaFallback": true,
        "requestMediaHeuristic": false,
    });
    let (status, body) = web_call(
        &router,
        "PUT",
        "/api/settings/rectifier",
        Some(json!({ "config": config })),
    )
    .await;
    assert_eq!(
        (status, &body),
        (StatusCode::OK, &json!({ "ok": true, "data": true }))
    );
    let (_, body) = web_call(&router, "GET", "/api/settings/rectifier", None).await;
    assert_eq!(body["data"], config);
}

#[tokio::test]
async fn optimizer_config_round_trips_through_put_and_get() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fixture();
    let (_, body) = web_call(&router, "GET", "/api/settings/optimizer", None).await;
    assert_eq!(
        body["data"]["enabled"],
        json!(false),
        "default is off: {body}"
    );

    let config = json!({ "enabled": true, "thinkingOptimizer": false, "cacheInjection": true });
    let (status, body) = web_call(
        &router,
        "PUT",
        "/api/settings/optimizer",
        Some(json!({ "config": config })),
    )
    .await;
    assert_eq!(
        (status, &body),
        (StatusCode::OK, &json!({ "ok": true, "data": true }))
    );
    let (_, body) = web_call(&router, "GET", "/api/settings/optimizer", None).await;
    assert_eq!(body["data"], config);
}

#[tokio::test]
async fn log_config_round_trips_and_applies_the_level_to_the_logger() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fixture();
    let (_, body) = web_call(&router, "GET", "/api/settings/log", None).await;
    assert_eq!(body["data"], json!({ "enabled": true, "level": "info" }));

    log::set_max_level(log::LevelFilter::Error);
    let config = json!({ "enabled": true, "level": "debug" });
    let (status, body) = web_call(
        &router,
        "PUT",
        "/api/settings/log",
        Some(json!({ "config": config })),
    )
    .await;
    assert_eq!(
        (status, &body),
        (StatusCode::OK, &json!({ "ok": true, "data": true }))
    );
    assert_eq!(log::max_level(), log::LevelFilter::Debug);
    let (_, body) = web_call(&router, "GET", "/api/settings/log", None).await;
    assert_eq!(body["data"], config);

    // the master switch turns logging off entirely
    let off = json!({ "enabled": false, "level": "warn" });
    web_call(
        &router,
        "PUT",
        "/api/settings/log",
        Some(json!({ "config": off })),
    )
    .await;
    assert_eq!(log::max_level(), log::LevelFilter::Off);
    log::set_max_level(log::LevelFilter::Info);
}

#[tokio::test]
async fn put_with_a_malformed_body_is_rejected_and_keeps_the_stored_config() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fixture();
    // axum's Json extractor answers non-JSON text on a bad body; only the status matters here.
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;
    let req = Request::builder()
        .method("PUT")
        .uri("/api/settings/rectifier")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"config":{"enabled":"nope"}}"#))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    assert!(resp.status().is_client_error(), "got {}", resp.status());
    let (_, body) = web_call(&router, "GET", "/api/settings/rectifier", None).await;
    assert_eq!(body["data"]["enabled"], json!(true));
}

#[tokio::test]
async fn tail_returns_the_last_lines_of_the_prepared_file() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fixture();
    let path = write_log(10);
    let (status, body) = web_call(&router, "GET", "/api/logs/tail?lines=3", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(lines_of(&body), ["log line 8", "log line 9", "log line 10"]);
    assert_eq!(body["data"]["path"], json!(path.display().to_string()));
}

#[tokio::test]
async fn tail_defaults_to_500_lines_and_caps_at_5000() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fixture();
    write_log(6000);
    let (_, body) = web_call(&router, "GET", "/api/logs/tail", None).await;
    let got = lines_of(&body);
    assert_eq!(got.len(), 500);
    assert_eq!(got[0], "log line 5501");
    assert_eq!(got[499], "log line 6000");

    let (status, body) = web_call(&router, "GET", "/api/logs/tail?lines=100000", None).await;
    assert_eq!(status, StatusCode::OK);
    let got = lines_of(&body);
    assert_eq!(got.len(), 5000);
    assert_eq!(got[0], "log line 1001");
}

#[tokio::test]
async fn tail_rejects_zero_and_non_numeric_lines() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fixture();
    write_log(3);
    for q in ["0", "abc", "-5", ""] {
        let (status, body) =
            web_call(&router, "GET", &format!("/api/logs/tail?lines={q}"), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "lines={q:?}");
        assert_eq!(body["ok"], json!(false));
        assert!(
            body["error"].as_str().unwrap().contains(&format!("'{q}'")),
            "lines={q:?}: {body}"
        );
    }
}

#[tokio::test]
async fn missing_log_file_is_404_with_the_expected_path() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fixture();
    let expected = log_file_path().display().to_string();
    assert!(!std::path::Path::new(&expected).exists());
    let (status, body) = web_call(&router, "GET", "/api/logs/tail", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["path"], json!(expected));
    assert!(
        body["error"].as_str().unwrap().contains(&expected),
        "{body}"
    );

    let (status, body) = web_call(&router, "GET", "/api/logs/download", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["path"], json!(expected));
}

#[tokio::test]
async fn download_streams_the_whole_file_as_a_text_attachment() {
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fixture();
    // larger than one 64 KiB read, so the stream yields several chunks
    let path = write_log(20_000);
    let on_disk = std::fs::read(&path).unwrap();
    assert!(on_disk.len() > 128 * 1024);

    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/logs/download")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()[header::CONTENT_TYPE],
        "text/plain; charset=utf-8"
    );
    assert_eq!(
        resp.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename=\"cc-switch-web.log\""
    );
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(bytes.as_ref(), on_disk.as_slice());
}
