mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use support::{
    create_test_state, ensure_test_home, reset_test_fs, test_mutex, web_call, web_router_with,
};

fn fresh() -> axum::Router {
    reset_test_fs();
    ensure_test_home();
    web_router_with(Arc::new(create_test_state().expect("state")))
}

fn pricing_body(name: &str, input: &str) -> Value {
    json!({
        "displayName": name,
        "inputCost": input,
        "outputCost": "2.5",
        "cacheReadCost": "0.1",
        "cacheCreationCost": "0.2",
    })
}

fn entry(id: &str, name: &str) -> Value {
    json!({
        "modelId": id,
        "displayName": name,
        "inputCostPerMillion": "1",
        "outputCostPerMillion": "2",
        "cacheReadCostPerMillion": "0.1",
        "cacheCreationCostPerMillion": "0.2",
    })
}

async fn pricing_list(router: &axum::Router) -> Vec<Value> {
    let (status, body) = web_call(router, "GET", "/api/usage/model-pricing", None).await;
    assert_eq!(status, StatusCode::OK);
    body["data"].as_array().expect("pricing array").clone()
}

fn find<'a>(list: &'a [Value], id: &str) -> Option<&'a Value> {
    list.iter().find(|e| e["modelId"] == id)
}

#[tokio::test]
async fn update_pricing_is_reflected_in_the_list() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fresh();

    let (status, body) = web_call(
        &router,
        "PUT",
        "/api/usage/model-pricing/acme-model-1",
        Some(pricing_body("Acme One", "3.25")),
    )
    .await;
    assert_eq!((status, &body["ok"]), (StatusCode::OK, &json!(true)));

    let list = pricing_list(&router).await;
    let e = find(&list, "acme-model-1").expect("entry listed");
    assert_eq!(e["displayName"], "Acme One");
    assert_eq!(e["inputCostPerMillion"], "3.25");
    assert_eq!(e["outputCostPerMillion"], "2.5");
    assert_eq!(e["cacheReadCostPerMillion"], "0.1");
    assert_eq!(e["cacheCreationCostPerMillion"], "0.2");

    // The id comes from the path: a slash-bearing id survives percent-encoding.
    let (status, _) = web_call(
        &router,
        "PUT",
        "/api/usage/model-pricing/vendor%2Facme-2",
        Some(pricing_body("Acme Two", "1")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(find(&pricing_list(&router).await, "vendor/acme-2").is_some());
}

#[tokio::test]
async fn invalid_update_returns_the_service_error_verbatim() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fresh();

    let (status, body) = web_call(
        &router,
        "PUT",
        "/api/usage/model-pricing/acme-model-1",
        Some(pricing_body("  ", "1")),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "显示名称不能为空 (Display name is required)");
}

#[tokio::test]
async fn batch_of_three_returns_three_and_lists_all() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fresh();

    let (status, body) = web_call(
        &router,
        "POST",
        "/api/usage/model-pricing/batch",
        Some(json!({"entries": [entry("b-1", "B One"), entry("b-2", "B Two"), entry("b-3", "B Three")]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"], 3);

    let list = pricing_list(&router).await;
    for (id, name) in [("b-1", "B One"), ("b-2", "B Two"), ("b-3", "B Three")] {
        assert_eq!(find(&list, id).expect(id)["displayName"], name);
    }
}

// Desktop contract (services::model_pricing::update_model_pricing_batch): every entry is
// normalised before the first write, so one invalid entry rejects the whole batch.
#[tokio::test]
async fn batch_with_invalid_middle_entry_is_all_or_nothing() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fresh();

    let (status, body) = web_call(
        &router,
        "POST",
        "/api/usage/model-pricing/batch",
        Some(
            json!({"entries": [entry("c-1", "C One"), entry("c-2", ""), entry("c-3", "C Three")]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"], "显示名称不能为空 (Display name is required)");

    let list = pricing_list(&router).await;
    for id in ["c-1", "c-2", "c-3"] {
        assert!(find(&list, id).is_none(), "{id} must not be written");
    }
}

#[tokio::test]
async fn delete_removes_the_entry_and_blank_id_errors() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fresh();
    web_call(
        &router,
        "PUT",
        "/api/usage/model-pricing/d-1",
        Some(pricing_body("D One", "1")),
    )
    .await;
    assert!(find(&pricing_list(&router).await, "d-1").is_some());

    let (status, body) = web_call(&router, "DELETE", "/api/usage/model-pricing/d-1", None).await;
    assert_eq!((status, &body["ok"]), (StatusCode::OK, &json!(true)));
    assert!(find(&pricing_list(&router).await, "d-1").is_none());

    let (status, body) = web_call(&router, "DELETE", "/api/usage/model-pricing/%20", None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"], "模型 ID 不能为空 (Model ID is required)");
}

#[tokio::test]
async fn models_dev_config_round_trips_and_result_updates_state() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fresh();

    let (status, body) = web_call(
        &router,
        "PUT",
        "/api/usage/models-dev/config",
        Some(json!({
            "autoSyncEnabled": false,
            "includeCommonModels": false,
            "selectedModelKeys": ["anthropic/claude-x"],
            "excludedCommonModelKeys": ["openai/gpt-y"],
        })),
    )
    .await;
    assert_eq!((status, &body["ok"]), (StatusCode::OK, &json!(true)));

    let (_, body) = web_call(&router, "GET", "/api/usage/models-dev/config", None).await;
    let cfg = &body["data"]["config"];
    assert_eq!(cfg["autoSyncEnabled"], false);
    assert_eq!(cfg["includeCommonModels"], false);
    assert_eq!(cfg["selectedModelKeys"], json!(["anthropic/claude-x"]));
    assert_eq!(cfg["excludedCommonModelKeys"], json!(["openai/gpt-y"]));
    assert!(body["data"]["configPath"]
        .as_str()
        .is_some_and(|p| !p.is_empty()));

    let (status, _) = web_call(
        &router,
        "POST",
        "/api/usage/models-dev/result",
        Some(json!({"syncedAt": 1_700_000_000_123_i64, "error": "HTTP 503"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = web_call(&router, "GET", "/api/usage/models-dev/config", None).await;
    let cfg = &body["data"]["config"];
    assert_eq!(cfg["lastSyncAt"], 1_700_000_000_123_i64);
    assert_eq!(cfg["lastSyncError"], "HTTP 503");
    // result recording leaves the user's switches alone
    assert_eq!(cfg["autoSyncEnabled"], false);

    // a later success without syncedAt keeps the timestamp and clears the error
    web_call(
        &router,
        "POST",
        "/api/usage/models-dev/result",
        Some(json!({"syncedAt": null, "error": null})),
    )
    .await;
    let (_, body) = web_call(&router, "GET", "/api/usage/models-dev/config", None).await;
    assert_eq!(body["data"]["config"]["lastSyncAt"], 1_700_000_000_123_i64);
    assert_eq!(body["data"]["config"]["lastSyncError"], Value::Null);
}

#[tokio::test]
async fn pricing_model_source_round_trips_and_rejects_bad_input() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fresh();

    let (status, body) = web_call(&router, "GET", "/api/claude/pricing-model-source", None).await;
    assert_eq!(status, StatusCode::OK);
    let default = body["data"].as_str().unwrap().to_string();
    let other = if default == "request" {
        "response"
    } else {
        "request"
    };

    let (status, _) = web_call(
        &router,
        "PUT",
        "/api/claude/pricing-model-source",
        Some(json!({"value": other})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = web_call(&router, "GET", "/api/claude/pricing-model-source", None).await;
    assert_eq!(body["data"], other);
    // other apps are independent
    let (_, body) = web_call(&router, "GET", "/api/codex/pricing-model-source", None).await;
    assert_eq!(body["data"], default);

    let (status, body) = web_call(
        &router,
        "PUT",
        "/api/claude/pricing-model-source",
        Some(json!({"value": "bogus"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body["error"],
        "无效计费模式: bogus (Invalid pricing mode: bogus)"
    );
    let (_, body) = web_call(&router, "GET", "/api/claude/pricing-model-source", None).await;
    assert_eq!(body["data"], other, "rejected value must not be stored");

    let (status, _) = web_call(&router, "GET", "/api/nope/pricing-model-source", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

fn seed_codex_session(home: &Path) -> std::path::PathBuf {
    let dir = home.join(".codex/sessions/2026/07/10");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rollout-2026-07-10T03-00-00-019f0000-0000-7000-8000-000000000001.jsonl");
    let lines = [
        json!({"timestamp":"2026-07-10T03:00:00Z","type":"session_meta","payload":{"id":"019f0000-0000-7000-8000-000000000001","forked_from_id":null,"source":"cli"}}),
        json!({"timestamp":"2026-07-10T03:00:01Z","type":"turn_context","payload":{"model":"gpt-5.6-sol"}}),
        json!({"timestamp":"2026-07-10T03:00:02Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{
            "input_tokens":1000,"cached_input_tokens":900,"output_tokens":100,"reasoning_output_tokens":0,"total_tokens":1100}}}}),
    ];
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    std::fs::write(&path, text).unwrap();
    path
}

fn backups(home: &Path) -> Vec<String> {
    let dir = home.join(".cc-switch/backups");
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    rd.filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("db_backup_"))
        .collect()
}

async fn codex_rows(router: &axum::Router) -> u64 {
    let (status, body) = web_call(router, "GET", "/api/usage/data-sources", None).await;
    assert_eq!(status, StatusCode::OK);
    body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["dataSource"] == "codex_session")
        .map(|s| s["requestCount"].as_u64().unwrap())
        .sum()
}

#[tokio::test]
async fn rebuild_codex_backs_up_and_reimports_from_session_files() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fresh();
    let home = ensure_test_home();
    let session = seed_codex_session(home);
    assert!(backups(home).is_empty());

    let (status, body) = web_call(&router, "POST", "/api/usage/sync", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(codex_rows(&router).await, 1);

    let (status, body) = web_call(&router, "POST", "/api/usage/rebuild-codex", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["imported"], 1);
    assert_eq!(codex_rows(&router).await, 1, "rebuilt, not duplicated");
    assert_eq!(backups(home).len(), 1, "one safety backup per rebuild");

    // with the session file gone the reset sticks: proves rows were rebuilt from files
    std::fs::remove_file(&session).unwrap();
    let (status, body) = web_call(&router, "POST", "/api/usage/rebuild-codex", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["imported"], 0);
    assert_eq!(codex_rows(&router).await, 0);
}

#[tokio::test]
async fn last_sync_is_a_timestamp_at_or_after_a_sync() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fresh();
    let before = chrono::Utc::now().timestamp_millis();
    web_call(&router, "POST", "/api/usage/sync", None).await;
    let (status, body) = web_call(&router, "GET", "/api/usage/last-sync", None).await;
    assert_eq!(status, StatusCode::OK);
    let at = body["data"].as_i64().expect("timestamp after a sync");
    assert!(at >= before, "{at} < {before}");
}

// Rebuild and manual sync must both queue behind session_sync_mutex; rebuild takes its
// backup only once it holds it, and neither interleaves with the other's writes.
#[tokio::test]
async fn rebuild_and_sync_wait_for_the_session_sync_mutex() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fresh();
    let home = ensure_test_home();
    seed_codex_session(home);
    web_call(&router, "POST", "/api/usage/sync", None).await;
    assert_eq!(codex_rows(&router).await, 1);

    let held = cc_switch_lib::web_api::session_sync_lock_test_hook().await;
    let r1 = router.clone();
    let mut rebuild =
        tokio::spawn(async move { web_call(&r1, "POST", "/api/usage/rebuild-codex", None).await });
    let r2 = router.clone();
    let mut sync =
        tokio::spawn(async move { web_call(&r2, "POST", "/api/usage/sync", None).await });

    assert!(
        tokio::time::timeout(Duration::from_millis(400), &mut rebuild)
            .await
            .is_err()
    );
    assert!(tokio::time::timeout(Duration::from_millis(100), &mut sync)
        .await
        .is_err());
    assert!(backups(home).is_empty(), "backup must wait for the mutex");
    assert_eq!(
        codex_rows(&router).await,
        1,
        "reset must wait for the mutex"
    );

    drop(held);
    let (s1, b1) = rebuild.await.unwrap();
    let (s2, b2) = sync.await.unwrap();
    assert_eq!((s1, s2), (StatusCode::OK, StatusCode::OK), "{b1} {b2}");
    assert_eq!(codex_rows(&router).await, 1);
    assert_eq!(backups(home).len(), 1);
}

#[tokio::test]
async fn two_concurrent_rebuilds_serialize() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let router = fresh();
    let home = ensure_test_home();
    seed_codex_session(home);

    let held = cc_switch_lib::web_api::session_sync_lock_test_hook().await;
    let mut tasks = Vec::new();
    for _ in 0..2 {
        let r = router.clone();
        tasks.push(tokio::spawn(async move {
            web_call(&r, "POST", "/api/usage/rebuild-codex", None).await
        }));
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(300), &mut tasks[0])
            .await
            .is_err()
    );
    assert!(backups(home).is_empty());
    drop(held);
    for t in tasks {
        let (status, body) = t.await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    assert_eq!(
        codex_rows(&router).await,
        1,
        "second rebuild reset the first's rows, not doubled"
    );
    assert_eq!(backups(home).len(), 2, "each rebuild takes its own backup");
}

// The headless worker the server starts: one startup round, so last-sync moves
// without any request. Polls for the effect (no fixed sleep).
#[tokio::test]
async fn background_worker_runs_a_startup_sync() {
    let _g = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let state = Arc::new(create_test_state().expect("state"));
    let router = web_router_with(state.clone());
    seed_codex_session(ensure_test_home());
    let before = chrono::Utc::now().timestamp_millis();

    let worker = cc_switch_lib::bootstrap::spawn_session_usage_worker(state.db.clone());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let (_, body) = web_call(&router, "GET", "/api/usage/last-sync", None).await;
        if body["data"].as_i64().is_some_and(|at| at >= before) {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "worker never synced"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    worker.abort();
    assert_eq!(
        codex_rows(&router).await,
        1,
        "startup round imported the session"
    );
}
