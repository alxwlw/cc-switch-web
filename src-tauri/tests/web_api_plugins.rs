//! `/api/plugins/:app/*`: the Plugins page on the server. Every service call runs the app's
//! own CLI, and a CLI on the host PATH wins over a fake one, so these tests stop at the
//! service's input checks, which run before any CLI and echo the request's values back. That
//! is enough to show each route parses `:app` and hands the body fields to the right service;
//! the CLI side is covered by the lib unit tests of `services::plugin_manager`.

mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};
use serial_test::serial;
use support::{reset_test_fs, test_mutex, web_call, web_router};

async fn expect_error(
    method: &str,
    uri: &str,
    body: Option<Value>,
    status: StatusCode,
    error: &str,
) {
    let router = web_router();
    let (got, resp) = web_call(&router, method, uri, body).await;
    assert_eq!(got, status, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(false), "{method} {uri}: {resp}");
    assert_eq!(resp["error"], json!(error), "{method} {uri}: {resp}");
}

#[tokio::test]
#[serial]
async fn an_unknown_app_is_a_400_on_every_plugin_route() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let id = Some(json!({"id": "a@b"}));
    let cases: [(&str, &str, Option<Value>); 10] = [
        ("GET", "/api/plugins/nope/installed", None),
        ("GET", "/api/plugins/nope/catalog", None),
        ("GET", "/api/plugins/nope/marketplaces", None),
        (
            "POST",
            "/api/plugins/nope/marketplaces",
            Some(json!({"source": "o/r"})),
        ),
        (
            "POST",
            "/api/plugins/nope/marketplaces/remove",
            Some(json!({"name": "m"})),
        ),
        ("POST", "/api/plugins/nope/marketplaces/refresh", None),
        ("POST", "/api/plugins/nope/install", id.clone()),
        ("POST", "/api/plugins/nope/uninstall", id.clone()),
        (
            "POST",
            "/api/plugins/nope/enabled",
            Some(json!({"id": "a@b", "enabled": true})),
        ),
        ("POST", "/api/plugins/nope/update", id),
    ];
    for (method, uri, body) in cases {
        expect_error(
            method,
            uri,
            body,
            StatusCode::BAD_REQUEST,
            "unsupported plugin app: nope",
        )
        .await;
    }
}

/// `a@b` is a valid Claude Code id, so a refusal shows the route took `grokbuild` from the path
/// (Grok Build ids have no `@`) and the id from the body.
#[tokio::test]
#[serial]
async fn plugin_actions_hand_the_path_app_and_the_body_id_to_the_service() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let cases: [(&str, Value); 4] = [
        ("install", json!({"id": "a@b"})),
        ("uninstall", json!({"id": "a@b"})),
        ("enabled", json!({"id": "a@b", "enabled": false})),
        ("update", json!({"id": "a@b"})),
    ];
    for (action, body) in cases {
        expect_error(
            "POST",
            &format!("/api/plugins/grokbuild/{action}"),
            Some(body),
            StatusCode::INTERNAL_SERVER_ERROR,
            "Invalid plugin id: a@b",
        )
        .await;
    }
}

#[tokio::test]
#[serial]
async fn marketplace_routes_hand_their_values_to_the_service() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let error = StatusCode::INTERNAL_SERVER_ERROR;
    expect_error(
        "POST",
        "/api/plugins/claude/marketplaces",
        Some(json!({"source": "-x"})),
        error,
        "Invalid marketplace source: -x",
    )
    .await;
    expect_error(
        "POST",
        "/api/plugins/claude/marketplaces/remove",
        Some(json!({"name": "bad name"})),
        error,
        "Invalid marketplace name: bad name",
    )
    .await;
    // MiniMax Code has no marketplace commands: the service refuses before any CLI runs.
    expect_error(
        "GET",
        "/api/plugins/mcode/marketplaces",
        None,
        error,
        "mcode does not support marketplace management",
    )
    .await;
}
