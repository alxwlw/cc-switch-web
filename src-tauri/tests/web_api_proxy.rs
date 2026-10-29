//! `/api/proxy/*` (WEB-01 stage 1): lifecycle, takeover, routing / Stack and configs through
//! the HTTP layer. Startup after a crash and `shutdown_cleanup` are covered in
//! `mode::controller` (`mode_tests`). Every test starts a real proxy on a loopback ephemeral
//! port (`listenPort` 0) and stops it before it ends.
#![cfg(not(target_os = "windows"))]

mod support;

use axum::http::StatusCode;
use cc_switch_lib::{get_claude_settings_path, AppState, Provider};
use serde_json::{json, Value};
use serial_test::serial;
use std::sync::Arc;
use support::{
    create_test_state, ensure_test_home, reset_test_fs, test_mutex, web_call, web_router_with,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const USER_SETTINGS: &str = r#"{
  "hooks": {
    "Stop": []
  },
  "env": {
    "ANTHROPIC_BASE_URL": "https://a.example",
    "ANTHROPIC_AUTH_TOKEN": "sk-a",
    "ANTHROPIC_MODEL": "claude-sonnet-4-6",
    "DISABLE_TELEMETRY": "1"
  },
  "permissions": {
    "allow": [
      "Bash"
    ]
  }
}
"#;

fn claude(id: &str, url: &str, key: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        id.to_uppercase(),
        json!({"env": {
            "ANTHROPIC_BASE_URL": url,
            "ANTHROPIC_AUTH_TOKEN": key,
            "ANTHROPIC_MODEL": "claude-sonnet-4-6"
        }}),
        None,
    )
}

fn live_settings() -> Value {
    serde_json::from_slice(&std::fs::read(get_claude_settings_path()).unwrap()).unwrap()
}

struct Fixture {
    state: Arc<AppState>,
    router: axum::Router,
}

/// Temp HOME with two Claude providers (`a` current, matching the user's file; `b` on
/// `upstream`) and the user's `settings.json`; proxy
/// port set to 0 through the config route.
async fn fixture(upstream: &str) -> Fixture {
    reset_test_fs();
    ensure_test_home();
    let state = Arc::new(create_test_state().unwrap());
    state
        .db
        .save_provider("claude", &claude("a", "https://a.example", "sk-a"))
        .unwrap();
    state
        .db
        .save_provider("claude", &claude("b", upstream, "sk-plain-b"))
        .unwrap();
    state.db.set_current_provider("claude", "a").unwrap();
    let router = web_router_with(state.clone());
    let settings = get_claude_settings_path();
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(&settings, USER_SETTINGS).unwrap();
    let config = ok_data(&router, "GET", "/api/proxy/config", None).await;
    let mut config = config;
    config["listenAddress"] = json!("127.0.0.1");
    config["listenPort"] = json!(0);
    ok_data(&router, "PUT", "/api/proxy/config", Some(config)).await;
    Fixture { state, router }
}

async fn ok_data(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, resp) = web_call(router, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(true), "{method} {uri}: {resp}");
    resp["data"].clone()
}

async fn status_of(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> u16 {
    web_call(router, method, uri, body).await.0.as_u16()
}

/// One-shot stub upstream: answers every request with a minimal Anthropic message and
/// records the raw request head + body.
async fn stub_upstream() -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let log = log.clone();
            tokio::spawn(async move {
                let request = read_request(&mut socket).await;
                log.lock().unwrap().push(request);
                let body = json!({
                    "id": "msg_stub", "type": "message", "role": "assistant",
                    "model": "claude-sonnet-4-6",
                    "content": [{"type": "text", "text": "hi"}],
                    "stop_reason": "end_turn", "stop_sequence": null,
                    "usage": {"input_tokens": 1, "output_tokens": 1}
                })
                .to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (url, seen)
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> String {
    let mut data = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = socket.read(&mut chunk).await.unwrap_or(0);
        if read == 0 {
            break;
        }
        data.extend_from_slice(&chunk[..read]);
        let text = String::from_utf8_lossy(&data).to_string();
        if let Some(end) = text.find("\r\n\r\n") {
            let length = text[..end]
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                })
                .unwrap_or(0);
            if data.len() >= end + 4 + length {
                break;
            }
        }
    }
    String::from_utf8_lossy(&data).to_string()
}

#[tokio::test]
#[serial]
async fn start_reports_running_and_stop_ends_it() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture("https://upstream.invalid").await;

    assert_eq!(
        ok_data(&f.router, "GET", "/api/proxy/status", None).await["running"],
        json!(false)
    );
    let info = ok_data(&f.router, "POST", "/api/proxy/start", None).await;
    let port = info["port"].as_u64().unwrap();
    assert!(port > 0, "ephemeral port was bound: {info}");

    let status = ok_data(&f.router, "GET", "/api/proxy/status", None).await;
    assert_eq!(status["running"], json!(true));
    assert_eq!(status["port"].as_u64().unwrap(), port);
    assert_eq!(status["address"], json!("127.0.0.1"));

    ok_data(&f.router, "POST", "/api/proxy/stop", None).await;
    assert_eq!(
        ok_data(&f.router, "GET", "/api/proxy/status", None).await["running"],
        json!(false)
    );
}

#[tokio::test]
#[serial]
async fn takeover_points_claude_at_the_proxy_and_stop_is_refused_until_it_is_off() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture("https://upstream.invalid").await;
    ok_data(&f.router, "POST", "/api/proxy/start", None).await;
    let port = ok_data(&f.router, "GET", "/api/proxy/status", None).await["port"]
        .as_u64()
        .unwrap();
    let before = live_settings();

    ok_data(
        &f.router,
        "PUT",
        "/api/proxy/takeover/claude",
        Some(json!({"enabled": true, "stack": false})),
    )
    .await;

    let takeover = ok_data(&f.router, "GET", "/api/proxy/takeover", None).await;
    assert_eq!(takeover["claude"], json!(true));
    assert_eq!(takeover["codex"], json!(false));
    let live = live_settings();
    assert_eq!(
        live["env"]["ANTHROPIC_BASE_URL"],
        json!(format!("http://127.0.0.1:{port}"))
    );
    assert_ne!(live, before);
    let mode = ok_data(&f.router, "GET", "/api/proxy/mode/claude", None).await;
    assert_eq!(mode["mode"], json!("route"));

    // stop while claude is taken over: refused, proxy keeps running, file untouched
    let (status, resp) = web_call(&f.router, "POST", "/api/proxy/stop", None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{resp}");
    assert_eq!(resp["ok"], json!(false));
    assert!(resp["error"].as_str().unwrap().contains("接管"), "{resp}");
    assert_eq!(
        ok_data(&f.router, "GET", "/api/proxy/status", None).await["running"],
        json!(true)
    );
    assert_eq!(live_settings(), live);

    ok_data(
        &f.router,
        "PUT",
        "/api/proxy/takeover/claude",
        Some(json!({"enabled": false})),
    )
    .await;
    assert_eq!(
        ok_data(&f.router, "GET", "/api/proxy/takeover", None).await["claude"],
        json!(false)
    );
    // values back to the user's file; key order of the proxy keys is not pinned (see T2 notes)
    assert_eq!(live_settings(), before);
    // the last app left the proxy, so the server stopped by itself
    assert_eq!(
        ok_data(&f.router, "GET", "/api/proxy/status", None).await["running"],
        json!(false)
    );
}

#[tokio::test]
#[serial]
async fn stop_with_restore_exits_every_app_and_stops_the_proxy() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture("https://upstream.invalid").await;
    let before = live_settings();
    // entering route mode starts the proxy by itself
    ok_data(
        &f.router,
        "PUT",
        "/api/proxy/takeover/claude",
        Some(json!({"enabled": true})),
    )
    .await;
    assert_eq!(
        ok_data(&f.router, "GET", "/api/proxy/status", None).await["running"],
        json!(true)
    );

    ok_data(&f.router, "POST", "/api/proxy/stop-with-restore", None).await;

    assert_eq!(
        ok_data(&f.router, "GET", "/api/proxy/takeover", None).await["claude"],
        json!(false)
    );
    assert_eq!(
        ok_data(&f.router, "GET", "/api/proxy/status", None).await["running"],
        json!(false)
    );
    assert_eq!(live_settings(), before);
}

#[tokio::test]
#[serial]
async fn route_switch_changes_the_routed_provider() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture("https://upstream.invalid").await;
    ok_data(
        &f.router,
        "PUT",
        "/api/proxy/takeover/claude",
        Some(json!({"enabled": true, "route": "a"})),
    )
    .await;
    let mode = ok_data(&f.router, "GET", "/api/proxy/mode/claude", None).await;
    assert_eq!(mode["routeProviderId"], json!("a"));

    let result = ok_data(
        &f.router,
        "PUT",
        "/api/proxy/route/claude",
        Some(json!({"providerId": "b"})),
    )
    .await;
    assert_eq!(result["kind"], json!("route"));

    let mode = ok_data(&f.router, "GET", "/api/proxy/mode/claude", None).await;
    assert_eq!(mode["routeProviderId"], json!("b"));
    assert_eq!(mode["mode"], json!("route"));
    // the direct pointer is independent of the route
    let direct = ok_data(&f.router, "GET", "/api/proxy/direct/claude", None).await;
    assert_eq!(direct, json!("a"));

    ok_data(&f.router, "POST", "/api/proxy/stop-with-restore", None).await;
}

#[tokio::test]
#[serial]
async fn stack_member_add_and_remove_and_a_failure_is_an_object() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture("https://upstream.invalid").await;

    ok_data(
        &f.router,
        "PUT",
        "/api/proxy/stack/claude/b",
        Some(json!({"enabled": true})),
    )
    .await;
    let stack = ok_data(&f.router, "GET", "/api/proxy/stack/claude", None).await;
    let ids = |stack: &Value| -> Vec<String> {
        stack["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["providerId"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(ids(&stack), vec!["b".to_string()], "{stack}");

    ok_data(
        &f.router,
        "PUT",
        "/api/proxy/stack/claude/b",
        Some(json!({"enabled": false})),
    )
    .await;
    let stack = ok_data(&f.router, "GET", "/api/proxy/stack/claude", None).await;
    assert!(ids(&stack).is_empty(), "{stack}");

    // unknown provider: 500 with the serialized StackWriteError, nothing written
    let (status, resp) = web_call(
        &f.router,
        "PUT",
        "/api/proxy/stack/claude/ghost",
        Some(json!({"enabled": true})),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{resp}");
    assert_eq!(resp["ok"], json!(false));
    assert_eq!(resp["error"]["partial"], json!(false), "{resp}");
    assert!(
        resp["error"]["message"].as_str().unwrap().contains("ghost"),
        "{resp}"
    );
}

#[tokio::test]
#[serial]
async fn a_request_through_the_proxy_reaches_the_upstream_with_the_plain_key() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let (upstream, seen) = stub_upstream().await;
    let f = fixture(&upstream).await;
    ok_data(
        &f.router,
        "PUT",
        "/api/proxy/takeover/claude",
        Some(json!({"enabled": true, "route": "b"})),
    )
    .await;
    let port = ok_data(&f.router, "GET", "/api/proxy/status", None).await["port"]
        .as_u64()
        .unwrap();

    let body = json!({"model": "claude-sonnet-4-6", "max_tokens": 8,
        "messages": [{"role": "user", "content": "hi"}]})
    .to_string();
    let mut client = tokio::net::TcpStream::connect(("127.0.0.1", port as u16))
        .await
        .unwrap();
    let request = format!(
        "POST /v1/messages HTTP/1.1\r\nhost: 127.0.0.1:{port}\r\ncontent-type: application/json\r\nx-api-key: placeholder\r\nanthropic-version: 2023-06-01\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    client.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("msg_stub"), "{response}");

    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "{seen:?}");
    let upstream_request = seen[0].to_ascii_lowercase();
    assert!(
        upstream_request.starts_with("post /v1/messages"),
        "{upstream_request}"
    );
    assert!(
        upstream_request.contains("sk-plain-b"),
        "{upstream_request}"
    );
    assert!(
        !upstream_request.contains("placeholder"),
        "{upstream_request}"
    );

    ok_data(&f.router, "POST", "/api/proxy/stop-with-restore", None).await;
}

#[tokio::test]
#[serial]
async fn client_host_round_trips_and_rewrites_the_taken_over_client() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture("https://upstream.invalid").await;
    assert_eq!(
        ok_data(&f.router, "GET", "/api/proxy/client-host", None).await,
        json!("")
    );
    ok_data(
        &f.router,
        "PUT",
        "/api/proxy/takeover/claude",
        Some(json!({"enabled": true})),
    )
    .await;
    let port = ok_data(&f.router, "GET", "/api/proxy/status", None).await["port"]
        .as_u64()
        .unwrap();

    ok_data(
        &f.router,
        "PUT",
        "/api/proxy/client-host",
        Some(json!({"host": "localhost"})),
    )
    .await;

    assert_eq!(
        ok_data(&f.router, "GET", "/api/proxy/client-host", None).await,
        json!("localhost")
    );
    assert_eq!(
        live_settings()["env"]["ANTHROPIC_BASE_URL"],
        json!(format!("http://localhost:{port}"))
    );

    ok_data(
        &f.router,
        "PUT",
        "/api/proxy/client-host",
        Some(json!({"host": ""})),
    )
    .await;
    ok_data(&f.router, "POST", "/api/proxy/stop-with-restore", None).await;
}

#[tokio::test]
#[serial]
async fn global_and_per_app_config_round_trip() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture("https://upstream.invalid").await;

    let mut global = ok_data(&f.router, "GET", "/api/proxy/config", None).await;
    assert_eq!(global["listenPort"], json!(0));
    global["enableLogging"] = json!(!global["enableLogging"].as_bool().unwrap());
    ok_data(&f.router, "PUT", "/api/proxy/config", Some(global.clone())).await;
    assert_eq!(
        ok_data(&f.router, "GET", "/api/proxy/config", None).await,
        global
    );

    let mut app = ok_data(&f.router, "GET", "/api/proxy/config/claude", None).await;
    assert_eq!(app["appType"], json!("claude"));
    app["maxRetries"] = json!(7);
    // `enabled` mirrors the mode, so a client cannot turn it on this way
    app["enabled"] = json!(true);
    ok_data(&f.router, "PUT", "/api/proxy/config/claude", Some(app)).await;
    let app = ok_data(&f.router, "GET", "/api/proxy/config/claude", None).await;
    assert_eq!(app["maxRetries"], json!(7));
    assert_eq!(app["enabled"], json!(false));
    // the other apps' rows are untouched
    let codex = ok_data(&f.router, "GET", "/api/proxy/config/codex", None).await;
    assert_ne!(codex["maxRetries"], json!(7));
    let _ = &f.state;
}

#[tokio::test]
#[serial]
async fn bad_input_is_a_400_not_a_422_or_a_500() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture("https://upstream.invalid").await;
    let r = &f.router;

    // unknown app / app without a mode
    assert_eq!(status_of(r, "GET", "/api/proxy/mode/nope", None).await, 400);
    assert_eq!(
        status_of(r, "GET", "/api/proxy/mode/opencode", None).await,
        400
    );
    assert_eq!(
        status_of(r, "GET", "/api/proxy/config/nope", None).await,
        400
    );
    assert_eq!(
        status_of(r, "GET", "/api/proxy/direct/nope", None).await,
        400
    );
    assert_eq!(
        status_of(r, "GET", "/api/proxy/stack/nope", None).await,
        400
    );
    // missing or mistyped fields
    for (method, uri, body) in [
        ("PUT", "/api/proxy/takeover/claude", json!({})),
        (
            "PUT",
            "/api/proxy/takeover/claude",
            json!({"enabled": "yes"}),
        ),
        (
            "PUT",
            "/api/proxy/takeover/claude",
            json!({"enabled": true, "stack": 1}),
        ),
        (
            "PUT",
            "/api/proxy/takeover/claude",
            json!({"enabled": true, "route": 3}),
        ),
        ("PUT", "/api/proxy/takeover/nope", json!({"enabled": true})),
        ("PUT", "/api/proxy/route/claude", json!({})),
        ("PUT", "/api/proxy/route/claude", json!({"providerId": 1})),
        ("PUT", "/api/proxy/stack/claude/a", json!({})),
        ("PUT", "/api/proxy/stack/nope/a", json!({"enabled": true})),
        ("POST", "/api/proxy/exit-apps", json!({})),
        ("PUT", "/api/proxy/client-host", json!({})),
        ("PUT", "/api/proxy/config", json!({"listenPort": "x"})),
        (
            "PUT",
            "/api/proxy/config/claude",
            json!({"appType": "claude"}),
        ),
    ] {
        assert_eq!(
            status_of(r, method, uri, Some(body.clone())).await,
            400,
            "{method} {uri} {body}"
        );
    }
    // body app does not match the path
    let mut codex = ok_data(r, "GET", "/api/proxy/config/codex", None).await;
    codex["maxRetries"] = json!(9);
    assert_eq!(
        status_of(r, "PUT", "/api/proxy/config/claude", Some(codex)).await,
        400
    );
    // nothing was written by the rejected calls
    let claude = ok_data(r, "GET", "/api/proxy/config/claude", None).await;
    assert_ne!(claude["maxRetries"], json!(9));
}

#[tokio::test]
#[serial]
async fn plain_routes_answer_without_a_running_proxy() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture("https://upstream.invalid").await;
    let r = &f.router;

    assert_eq!(
        ok_data(r, "POST", "/api/proxy/startup-attach-failures/take", None).await,
        json!([])
    );
    assert_eq!(
        ok_data(
            r,
            "POST",
            "/api/proxy/exit-apps",
            Some(json!({"stack": false}))
        )
        .await,
        json!([])
    );
    assert_eq!(
        ok_data(r, "GET", "/api/proxy/mode/claude", None).await["mode"],
        json!("direct")
    );
    ok_data(r, "POST", "/api/proxy/codex/ack-stale-clients", None).await;
    // restart-daemon would restart the developer's real Codex daemon, so it is not called:
    // a GET proves the path is registered (405, not 404)
    let (status, _, _) =
        support::web_call_raw(r, "GET", "/api/proxy/codex/restart-daemon", None).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}
