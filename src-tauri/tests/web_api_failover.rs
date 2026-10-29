//! `/api/failover/*` (WEB-01 stage 2): queue, breaker, health and auto-failover through the
//! HTTP layer, plus the forwarder -> `try_switch` path through a real proxy on a loopback
//! port: a failing upstream makes the proxy switch to the next provider, the route is
//! recorded and `provider-switched` (source `failover`) reaches the event bus.
#![cfg(not(target_os = "windows"))]

mod support;

use axum::http::StatusCode;
use cc_switch_lib::{get_claude_settings_path, AppState, Provider};
use serde_json::{json, Value};
use serial_test::serial;
use std::sync::Arc;
use std::time::Duration;
use support::{
    create_test_state, ensure_test_home, reset_test_fs, test_mutex, web_call, web_router_with,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const USER_SETTINGS: &str = r#"{
  "env": {
    "ANTHROPIC_BASE_URL": "https://a.example",
    "ANTHROPIC_AUTH_TOKEN": "sk-a",
    "ANTHROPIC_MODEL": "claude-sonnet-4-6"
  }
}
"#;

fn claude(id: &str, url: &str, key: &str, sort_index: usize) -> Provider {
    let mut provider = Provider::with_id(
        id.to_string(),
        id.to_uppercase(),
        json!({"env": {
            "ANTHROPIC_BASE_URL": url,
            "ANTHROPIC_AUTH_TOKEN": key,
            "ANTHROPIC_MODEL": "claude-sonnet-4-6"
        }}),
        None,
    );
    provider.sort_index = Some(sort_index);
    provider
}

struct Fixture {
    state: Arc<AppState>,
    router: axum::Router,
}

/// Temp HOME with Claude providers `a` and `b` (upstreams given), `a` current, the user's
/// settings file, proxy port 0, and the state bound to the proxy service like the production
/// constructors do.
async fn fixture(a_url: &str, b_url: &str) -> Fixture {
    reset_test_fs();
    ensure_test_home();
    let state = Arc::new(create_test_state().unwrap());
    state.proxy_service.bind_app_state((*state).clone());
    state
        .db
        .save_provider("claude", &claude("a", a_url, "sk-plain-a", 0))
        .unwrap();
    state
        .db
        .save_provider("claude", &claude("b", b_url, "sk-plain-b", 1))
        .unwrap();
    state.db.set_current_provider("claude", "a").unwrap();
    let router = web_router_with(state.clone());
    let settings = get_claude_settings_path();
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(&settings, USER_SETTINGS).unwrap();
    let mut config = ok_data(&router, "GET", "/api/proxy/config", None).await;
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

/// Stub upstream answering every request with `status`; counts only the proxied
/// `POST /v1/messages` calls. Other peers probe fresh loopback ports on a dev machine
/// (`GET /` from a Go client, once per listener and per host name), and counting every
/// accepted connection made `b_seen` 2..3 instead of 1 in about half of the runs.
async fn stub_upstream(status: u16) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let counter = counter.clone();
            tokio::spawn(async move {
                if read_request(&mut socket)
                    .await
                    .starts_with("POST /v1/messages ")
                {
                    counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
                let body = if status == 200 {
                    json!({
                        "id": "msg_stub", "type": "message", "role": "assistant",
                        "model": "claude-sonnet-4-6",
                        "content": [{"type": "text", "text": "hi"}],
                        "stop_reason": "end_turn", "stop_sequence": null,
                        "usage": {"input_tokens": 1, "output_tokens": 1}
                    })
                } else {
                    json!({"type": "error", "error": {"type": "api_error", "message": "boom"}})
                }
                .to_string();
                let response = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (url, seen)
}

/// Reads one request off `socket`; returns its request line (empty if the peer sent nothing).
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
    String::from_utf8_lossy(&data)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string()
}

/// One `/v1/messages` request through the running proxy; returns the raw response.
async fn proxy_request(router: &axum::Router) -> String {
    let port = ok_data(router, "GET", "/api/proxy/status", None).await["port"]
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
    response
}

/// Next `provider-switched` payload on `rx` (other events are skipped); fails after 10 s.
async fn next_switch(
    rx: &mut tokio::sync::broadcast::Receiver<cc_switch_lib::events::Event>,
) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = rx.recv().await.expect("event bus closed");
            if event.name == "provider-switched" {
                return event.payload;
            }
        }
    })
    .await
    .expect("no provider-switched event within 10 s")
}

/// Queue `a` then `b` (a = P1), breaker opens after one failure, claude routed to `a`,
/// auto-failover on.
async fn arm_failover(f: &Fixture) {
    let mut config = ok_data(&f.router, "GET", "/api/proxy/config/claude", None).await;
    config["circuitFailureThreshold"] = json!(1);
    ok_data(&f.router, "PUT", "/api/proxy/config/claude", Some(config)).await;
    ok_data(&f.router, "POST", "/api/failover/queue/claude/a", None).await;
    ok_data(&f.router, "POST", "/api/failover/queue/claude/b", None).await;
    ok_data(
        &f.router,
        "PUT",
        "/api/proxy/takeover/claude",
        Some(json!({"enabled": true, "route": "a"})),
    )
    .await;
    ok_data(
        &f.router,
        "PUT",
        "/api/failover/auto/claude",
        Some(json!({"enabled": true})),
    )
    .await;
}

async fn route_of(f: &Fixture) -> Value {
    ok_data(&f.router, "GET", "/api/proxy/mode/claude", None).await["routeProviderId"].clone()
}

#[tokio::test]
#[serial]
async fn a_failing_provider_switches_the_route_and_emits_through_the_proxy() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let (a_url, a_seen) = stub_upstream(500).await;
    let (b_url, b_seen) = stub_upstream(200).await;
    let f = fixture(&a_url, &b_url).await;
    let mut rx = f.state.events.subscribe();

    arm_failover(&f).await;
    // enabling auto-failover switches to P1 and says so
    let enabled = next_switch(&mut rx).await;
    assert_eq!(
        enabled,
        json!({"appType": "claude", "providerId": "a", "source": "failoverEnabled"})
    );
    assert_eq!(route_of(&f).await, json!("a"));

    // a request: a answers 500, b answers 200 -> served by b, forwarder switches to b
    let response = proxy_request(&f.router).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("msg_stub"), "{response}");
    assert!(a_seen.load(std::sync::atomic::Ordering::SeqCst) >= 1);
    assert_eq!(b_seen.load(std::sync::atomic::Ordering::SeqCst), 1);

    let switched = next_switch(&mut rx).await;
    assert_eq!(
        switched,
        json!({"appType": "claude", "providerId": "b", "source": "failover"})
    );
    assert_eq!(route_of(&f).await, json!("b"));

    // a is past the threshold: unhealthy, snake_case DTO
    let health = ok_data(&f.router, "GET", "/api/failover/health/claude/a", None).await;
    assert_eq!(health["is_healthy"], json!(false), "{health}");
    assert_eq!(health["provider_id"], json!("a"));
    assert!(health["consecutive_failures"].as_u64().unwrap() >= 1);
    assert!(health["last_error"].is_string(), "{health}");

    // reset a: healthy again, and a (lower sort_index) takes the route back
    ok_data(
        &f.router,
        "POST",
        "/api/failover/breaker/claude/a/reset",
        None,
    )
    .await;
    let health = ok_data(&f.router, "GET", "/api/failover/health/claude/a", None).await;
    assert_eq!(health["is_healthy"], json!(true), "{health}");
    let back = next_switch(&mut rx).await;
    assert_eq!(
        back,
        json!({"appType": "claude", "providerId": "a", "source": "failover"})
    );
    assert_eq!(route_of(&f).await, json!("a"));

    ok_data(&f.router, "POST", "/api/proxy/stop-with-restore", None).await;
}

#[tokio::test]
#[serial]
async fn with_auto_failover_off_a_failing_provider_is_an_error_and_nothing_switches() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let (a_url, a_seen) = stub_upstream(500).await;
    let (b_url, b_seen) = stub_upstream(200).await;
    let f = fixture(&a_url, &b_url).await;
    arm_failover(&f).await;
    ok_data(
        &f.router,
        "PUT",
        "/api/failover/auto/claude",
        Some(json!({"enabled": false})),
    )
    .await;
    assert_eq!(
        ok_data(&f.router, "GET", "/api/failover/auto/claude", None).await,
        json!(false)
    );
    let mut rx = f.state.events.subscribe();

    let response = proxy_request(&f.router).await;
    assert!(!response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(a_seen.load(std::sync::atomic::Ordering::SeqCst) >= 1);
    assert_eq!(b_seen.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(route_of(&f).await, json!("a"));
    assert!(rx.try_recv().is_err(), "no event was emitted");

    ok_data(&f.router, "POST", "/api/proxy/stop-with-restore", None).await;
}

#[tokio::test]
#[serial]
async fn queue_add_remove_available_and_auto_round_trip() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture("https://a.example", "https://b.example").await;
    let ids = |v: &Value| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|i| {
                i.get("providerId")
                    .or_else(|| i.get("id"))
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect()
    };

    assert!(ids(&ok_data(&f.router, "GET", "/api/failover/queue/claude", None).await).is_empty());
    let mut available =
        ids(&ok_data(&f.router, "GET", "/api/failover/available/claude", None).await);
    available.sort();
    assert_eq!(available, vec!["a".to_string(), "b".to_string()]);

    ok_data(&f.router, "POST", "/api/failover/queue/claude/b", None).await;
    let queue = ok_data(&f.router, "GET", "/api/failover/queue/claude", None).await;
    assert_eq!(ids(&queue), vec!["b".to_string()]);
    assert!(queue[0]["providerName"].is_string(), "camelCase: {queue}");
    assert_eq!(
        ids(&ok_data(&f.router, "GET", "/api/failover/available/claude", None).await),
        vec!["a".to_string()]
    );

    ok_data(&f.router, "DELETE", "/api/failover/queue/claude/b", None).await;
    assert!(ids(&ok_data(&f.router, "GET", "/api/failover/queue/claude", None).await).is_empty());

    assert_eq!(
        ok_data(&f.router, "GET", "/api/failover/auto/claude", None).await,
        json!(false)
    );
}

#[tokio::test]
#[serial]
async fn breaker_config_round_trips_and_stats_are_null() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture("https://a.example", "https://b.example").await;

    let mut config = ok_data(&f.router, "GET", "/api/failover/breaker-config", None).await;
    for key in [
        "failureThreshold",
        "successThreshold",
        "timeoutSeconds",
        "errorRateThreshold",
        "minRequests",
    ] {
        assert!(config.get(key).is_some(), "camelCase {key}: {config}");
    }
    config["failureThreshold"] = json!(7);
    config["timeoutSeconds"] = json!(33);
    ok_data(
        &f.router,
        "PUT",
        "/api/failover/breaker-config",
        Some(config.clone()),
    )
    .await;
    let again = ok_data(&f.router, "GET", "/api/failover/breaker-config", None).await;
    assert_eq!(again["failureThreshold"], json!(7));
    assert_eq!(again["timeoutSeconds"], json!(33));
    assert_eq!(again, config);

    assert_eq!(
        ok_data(
            &f.router,
            "GET",
            "/api/failover/breaker-stats/claude/a",
            None
        )
        .await,
        Value::Null
    );
    // health of a provider that never failed: no row means healthy
    let health = ok_data(&f.router, "GET", "/api/failover/health/claude/a", None).await;
    assert_eq!(health["is_healthy"], json!(true), "{health}");
}

#[tokio::test]
#[serial]
async fn bad_apps_and_bad_bodies_are_400_never_422() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture("https://a.example", "https://b.example").await;

    // claude-desktop has no failover: the desktop message, 400
    let (status, resp) =
        web_call(&f.router, "GET", "/api/failover/queue/claude-desktop", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{resp}");
    assert!(
        resp["error"].as_str().unwrap().contains("不支持故障转移"),
        "{resp}"
    );
    for (method, uri) in [
        ("GET", "/api/failover/available/claude-desktop"),
        ("POST", "/api/failover/queue/claude-desktop/a"),
        ("DELETE", "/api/failover/queue/claude-desktop/a"),
        ("GET", "/api/failover/auto/claude-desktop"),
        ("POST", "/api/failover/breaker/claude-desktop/a/reset"),
        ("GET", "/api/failover/health/nope/a"),
        ("GET", "/api/failover/breaker-stats/nope/a"),
        ("GET", "/api/failover/queue/nope"),
    ] {
        assert_eq!(
            status_of(&f.router, method, uri, None).await,
            400,
            "{method} {uri}"
        );
    }

    // missing / wrong-typed fields
    for body in [json!({}), json!({"enabled": "yes"}), json!([1])] {
        assert_eq!(
            status_of(
                &f.router,
                "PUT",
                "/api/failover/auto/claude",
                Some(body.clone())
            )
            .await,
            400,
            "{body}"
        );
    }
    for body in [json!({}), json!({"failureThreshold": 3})] {
        assert_eq!(
            status_of(
                &f.router,
                "PUT",
                "/api/failover/breaker-config",
                Some(body.clone())
            )
            .await,
            400,
            "{body}"
        );
    }
    // an unknown provider in the queue route is an error, not a 2xx
    assert_ne!(
        status_of(&f.router, "POST", "/api/failover/queue/claude/ghost", None).await,
        200
    );
}
