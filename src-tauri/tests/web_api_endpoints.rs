mod support;

use axum::http::StatusCode;
use cc_switch_lib::{AppState, Provider};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex, web_call};
use tokio::net::TcpListener;
use tokio::sync::Barrier;

fn claude_provider(id: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        id.to_uppercase(),
        json!({ "env": { "ANTHROPIC_AUTH_TOKEN": "k", "ANTHROPIC_BASE_URL": "https://api.test" }}),
        None,
    )
}

struct Fixture {
    router: axum::Router,
    home: std::path::PathBuf,
}

fn fixture() -> Fixture {
    reset_test_fs();
    let home = ensure_test_home();
    let state = Arc::new(create_test_state().expect("test state"));
    state
        .db
        .save_provider("claude", &claude_provider("p1"))
        .unwrap();
    let router = cc_switch_lib::web_api::router(state as Arc<AppState>);
    Fixture {
        router,
        home: home.to_path_buf(),
    }
}

/// Local stub; every request waits at `barrier` (bounded) and counts a miss when
/// the barrier is not reached, i.e. when requests were not concurrent.
async fn stub(barrier: Arc<Barrier>, misses: Arc<AtomicUsize>) -> String {
    let app = axum::Router::new().route(
        "/",
        axum::routing::get(move || {
            let barrier = barrier.clone();
            let misses = misses.clone();
            async move {
                if tokio::time::timeout(Duration::from_secs(3), barrier.wait())
                    .await
                    .is_err()
                {
                    misses.fetch_add(1, Ordering::SeqCst);
                }
                "ok"
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

async fn dead_url() -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    drop(l);
    url
}

#[tokio::test]
async fn speedtest_probes_urls_concurrently_and_reports_latency() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture();
    // Barrier(3): warmup and timed round each release only if all 3 are in flight.
    let barrier = Arc::new(Barrier::new(3));
    let misses = Arc::new(AtomicUsize::new(0));
    let urls = [
        stub(barrier.clone(), misses.clone()).await,
        stub(barrier.clone(), misses.clone()).await,
        stub(barrier.clone(), misses.clone()).await,
    ];
    let (status, body) = web_call(
        &f.router,
        "POST",
        "/api/endpoints/speedtest",
        Some(json!({ "urls": urls, "timeoutSecs": 10 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rows = body["data"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    for (row, url) in rows.iter().zip(urls.iter()) {
        assert_eq!(row["url"], json!(url));
        assert_eq!(row["status"], json!(200));
        assert!(row["latency"].is_u64(), "{row}");
        assert!(row["error"].is_null(), "{row}");
    }
    assert_eq!(misses.load(Ordering::SeqCst), 0, "requests were serialized");
}

/// Pins the `timeoutSecs` wire name: the service default is 8 s per request
/// (warmup + timed = 16 s), `timeoutSecs: 2` must cut that to ~4 s.
#[tokio::test]
async fn speedtest_honours_timeout_secs_from_the_request() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture();
    let app = axum::Router::new().route(
        "/",
        axum::routing::get(|| async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            "late"
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let started = std::time::Instant::now();
    let (status, body) = web_call(
        &f.router,
        "POST",
        "/api/endpoints/speedtest",
        Some(json!({ "urls": [url], "timeoutSecs": 2 })),
    )
    .await;
    let elapsed = started.elapsed();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"][0]["error"], json!("请求超时"));
    assert!(body["data"][0]["latency"].is_null(), "{body}");
    assert!(elapsed < Duration::from_secs(10), "took {elapsed:?}");
}

#[tokio::test]
async fn speedtest_unreachable_url_is_a_row_with_error_not_a_500() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture();
    let dead = dead_url().await;
    let (status, body) = web_call(
        &f.router,
        "POST",
        "/api/endpoints/speedtest",
        Some(json!({ "urls": [dead] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let row = &body["data"][0];
    assert_eq!(row["url"], json!(dead));
    assert!(row["latency"].is_null(), "{row}");
    assert_eq!(row["error"], json!("连接失败"));
}

#[tokio::test]
async fn endpoints_add_get_dedup_remove() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture();
    let uri = "/api/claude/providers/p1/endpoints";

    for _ in 0..2 {
        let (status, body) = web_call(
            &f.router,
            "POST",
            uri,
            Some(json!({ "url": "https://a.example.com/" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["data"], json!(true));
    }
    web_call(
        &f.router,
        "POST",
        uri,
        Some(json!({ "url": "https://b.example.com" })),
    )
    .await;

    let (_, body) = web_call(&f.router, "GET", uri, None).await;
    let mut urls: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["url"].as_str().unwrap())
        .collect();
    urls.sort();
    // trailing slash normalised away; repeated add does not duplicate
    assert_eq!(urls, vec!["https://a.example.com", "https://b.example.com"]);
    assert!(body["data"][0]["addedAt"].is_i64(), "{body}");

    let (status, body) = web_call(
        &f.router,
        "DELETE",
        uri,
        Some(json!({ "url": "https://a.example.com" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, body) = web_call(&f.router, "GET", uri, None).await;
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"][0]["url"], json!("https://b.example.com"));
}

/// Desktop parity: `provider_endpoints` has no last_used column, so the call is
/// accepted but persists nothing; the endpoint list must stay intact.
#[tokio::test]
async fn endpoint_last_used_is_accepted_and_keeps_list_intact() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture();
    let uri = "/api/claude/providers/p1/endpoints";
    web_call(
        &f.router,
        "POST",
        uri,
        Some(json!({ "url": "https://a.example.com" })),
    )
    .await;
    let (status, body) = web_call(
        &f.router,
        "POST",
        &format!("{uri}/last-used"),
        Some(json!({ "url": "https://a.example.com/" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"], json!(true));
    let (_, after) = web_call(&f.router, "GET", uri, None).await;
    assert_eq!(after["data"].as_array().unwrap().len(), 1);
    assert_eq!(after["data"][0]["url"], json!("https://a.example.com"));
}

#[tokio::test]
async fn unknown_provider_add_reports_ok_false() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture();
    let (status, body) = web_call(
        &f.router,
        "POST",
        "/api/claude/providers/nope/endpoints",
        Some(json!({ "url": "https://a.example.com" })),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert!(body["error"].is_string(), "{body}");
    // reading endpoints of an unknown provider is an empty list, as on desktop
    let (status, body) = web_call(
        &f.router,
        "GET",
        "/api/claude/providers/nope/endpoints",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"], json!([]));
}

#[tokio::test]
async fn live_settings_reads_prepared_file_from_temp_home() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture();
    let dir = f.home.join(".claude");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("settings.json"),
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://live.example.com"}}"#,
    )
    .unwrap();
    let (status, body) = web_call(&f.router, "GET", "/api/claude/live-settings", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["data"]["env"]["ANTHROPIC_BASE_URL"],
        Value::from("https://live.example.com")
    );
    let (status, _) = web_call(&f.router, "GET", "/api/bogus/live-settings", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
