#[allow(dead_code)]
mod support;

use axum::http::StatusCode;
use base64::prelude::{Engine as _, BASE64_URL_SAFE_NO_PAD};
use serde_json::{json, Value};
use serial_test::serial;
use std::sync::Arc;
use support::{create_test_state, reset_test_fs, web_call, web_router_with};

const PROVIDER_LINK: &str = "ccswitch://v1/import?resource=provider&app=claude&name=Web%20Paste&homepage=https%3A%2F%2Fexample.com&endpoint=https%3A%2F%2Fapi.example.com%2Fv1&apiKey=sk-test-web-paste&model=claude-sonnet-4";

struct Fixture {
    router: axum::Router,
    state: Arc<cc_switch_lib::AppState>,
}

fn fresh() -> Fixture {
    reset_test_fs();
    let state = Arc::new(create_test_state().expect("state"));
    Fixture {
        router: web_router_with(state.clone()),
        state,
    }
}

async fn ok_data(router: &axum::Router, uri: &str, body: Value) -> Value {
    let (status, resp) = web_call(router, "POST", uri, Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{uri}: {resp}");
    assert_eq!(resp["ok"], json!(true), "{uri}: {resp}");
    resp["data"].clone()
}

#[tokio::test]
#[serial]
async fn parse_valid_provider_link_returns_request() {
    let f = fresh();
    let req = ok_data(
        &f.router,
        "/api/deeplink/parse",
        json!({ "url": PROVIDER_LINK }),
    )
    .await;
    assert_eq!(req["resource"], "provider");
    assert_eq!(req["app"], "claude");
    assert_eq!(req["name"], "Web Paste");
    assert_eq!(req["endpoint"], "https://api.example.com/v1");
    assert_eq!(req["apiKey"], "sk-test-web-paste");
    assert_eq!(req["model"], "claude-sonnet-4");
}

#[tokio::test]
#[serial]
async fn parse_garbage_returns_parser_text() {
    let f = fresh();
    let expected = cc_switch_lib::parse_deeplink_url("not a link")
        .unwrap_err()
        .to_string();
    assert!(!expected.is_empty());
    let (status, resp) = web_call(
        &f.router,
        "POST",
        "/api/deeplink/parse",
        Some(json!({ "url": "not a link" })),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(resp["ok"], json!(false));
    assert_eq!(resp["error"], json!(expected));
}

#[tokio::test]
#[serial]
async fn merge_returns_request_with_config_filled_in() {
    let f = fresh();
    let config = BASE64_URL_SAFE_NO_PAD.encode(
        r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-from-config","ANTHROPIC_BASE_URL":"https://cfg.example.com"}}"#,
    );
    let link = format!(
        "ccswitch://v1/import?resource=provider&app=claude&name=Merged&config={config}&configFormat=json"
    );
    let parsed = ok_data(&f.router, "/api/deeplink/parse", json!({ "url": link })).await;
    let merged = ok_data(
        &f.router,
        "/api/deeplink/merge",
        json!({ "request": parsed }),
    )
    .await;
    assert_eq!(merged["apiKey"], "sk-from-config");
    assert_eq!(merged["endpoint"], "https://cfg.example.com");
}

#[tokio::test]
#[serial]
async fn missing_fields_are_400() {
    let f = fresh();
    for (uri, body) in [
        ("/api/deeplink/parse", json!({})),
        ("/api/deeplink/icon", json!({})),
        ("/api/deeplink/merge", json!({})),
        ("/api/deeplink/import", json!({})),
        ("/api/deeplink/import", json!({ "request": 5 })),
    ] {
        let (status, resp) = web_call(&f.router, "POST", uri, Some(body.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri} {body}: {resp}");
        assert_eq!(resp["ok"], json!(false));
    }
}

#[tokio::test]
#[serial]
async fn import_provider_lands_in_db() {
    let f = fresh();
    let request = ok_data(
        &f.router,
        "/api/deeplink/parse",
        json!({ "url": PROVIDER_LINK }),
    )
    .await;
    let out = ok_data(
        &f.router,
        "/api/deeplink/import",
        json!({ "request": request }),
    )
    .await;
    assert_eq!(out["type"], "provider");
    let id = out["id"].as_str().expect("provider id").to_string();
    let providers = f.state.db.get_all_providers("claude").unwrap();
    let p = providers.get(&id).expect("provider stored");
    assert_eq!(p.name, "Web Paste");
}

#[tokio::test]
#[serial]
async fn import_mcp_reports_imported_and_failed() {
    let f = fresh();
    let config = BASE64_URL_SAFE_NO_PAD.encode(
        r#"{"mcpServers":{"good":{"command":"echo"},"broken":{"type":"stdio","command":""}}}"#,
    );
    let link = format!("ccswitch://v1/import?resource=mcp&apps=kimi&config={config}");
    let request = ok_data(&f.router, "/api/deeplink/parse", json!({ "url": link })).await;
    let out = ok_data(
        &f.router,
        "/api/deeplink/import",
        json!({ "request": request }),
    )
    .await;
    assert_eq!(out["type"], "mcp");
    assert_eq!(out["importedCount"], 1);
    assert_eq!(out["importedIds"], json!(["good"]));
    let failed = out["failed"].as_array().expect("failed list");
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0]["id"], "broken");
    assert!(!failed[0]["error"].as_str().unwrap().is_empty());
    let stored = f.state.db.get_all_mcp_servers().unwrap();
    assert!(stored.contains_key("good") && !stored.contains_key("broken"));
}

#[tokio::test]
#[serial]
async fn import_unsupported_resource_returns_its_error_text() {
    let f = fresh();
    let (status, resp) = web_call(
        &f.router,
        "POST",
        "/api/deeplink/import",
        Some(json!({ "request": { "version": "v1", "resource": "widget" } })),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(resp["ok"], json!(false));
    assert_eq!(resp["error"], "Unsupported resource type: widget");
}

/// The icon route must never connect to a loopback listener: plain http and https
/// to 127.0.0.1 are both refused before any socket is opened.
#[tokio::test]
#[serial]
async fn icon_route_never_connects_to_loopback() {
    let f = fresh();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    for scheme in ["http", "https"] {
        let data = ok_data(
            &f.router,
            "/api/deeplink/icon",
            json!({ "url": format!("{scheme}://127.0.0.1:{port}/i.png") }),
        )
        .await;
        assert!(data.is_null(), "{scheme}: {data}");
    }
    match listener.accept() {
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
        other => panic!("loopback listener got a connection: {other:?}"),
    }
}

#[tokio::test]
#[serial]
async fn icon_route_refuses_localhost_and_private_hosts() {
    let f = fresh();
    for url in ["https://localhost/i.png", "https://10.0.0.1/i.png"] {
        let data = ok_data(&f.router, "/api/deeplink/icon", json!({ "url": url })).await;
        assert!(data.is_null(), "{url}: {data}");
    }
}
