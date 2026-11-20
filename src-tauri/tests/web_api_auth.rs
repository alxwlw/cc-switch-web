//! `/api/auth/:provider/*` (WEB-02 stage 1). Token files are seeded in the test config
//! dir BEFORE the router builds `AppState` (the managers load them eagerly). No test
//! reaches the network: device flows are not started here.
#![cfg(not(target_os = "windows"))]

mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};
use serial_test::serial;
use support::{ensure_test_home, reset_test_fs, test_mutex, web_call, web_router};

const GH_TOKEN_A: &str = "gho_SECRET_copilot_token_AAAA";
const GH_TOKEN_B: &str = "gho_SECRET_copilot_token_BBBB";
const XAI_REFRESH: &str = "xai-SECRET-refresh-token";
const CODEX_REFRESH: &str = "codex-SECRET-refresh-token";
const CODEX_ID_TOKEN: &str = "codex-SECRET-id-token";
const SECRETS: [&str; 5] = [
    GH_TOKEN_A,
    GH_TOKEN_B,
    XAI_REFRESH,
    CODEX_REFRESH,
    CODEX_ID_TOKEN,
];

fn config_dir() -> std::path::PathBuf {
    ensure_test_home().join(".cc-switch")
}

fn seed() {
    let dir = config_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("copilot_auth.json"),
        json!({
            "version": 3,
            "default_account_id": "1001",
            "accounts": {
                "1001": {"github_token": GH_TOKEN_A, "user": {"login": "octo-a", "id": 1001, "avatar_url": null}, "authenticated_at": 1},
                "1002": {"github_token": GH_TOKEN_B, "user": {"login": "octo-b", "id": 1002, "avatar_url": null}, "authenticated_at": 2}
            }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        dir.join("xai_oauth_auth.json"),
        json!({
            "version": 1,
            "default_account_id": "xai-acc-7",
            "accounts": {"xai-acc-7": {"account_id": "xai-acc-7", "login": "grok-user", "refresh_token": XAI_REFRESH, "authenticated_at": 3}}
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        dir.join("codex_oauth_auth.json"),
        json!({
            "version": 2,
            "default_account_id": "codex-1",
            "accounts": {"codex-1": {"account_id": "codex-1", "chatgpt_account_id": "ws-1", "email": "dev@example.com", "refresh_token": CODEX_REFRESH, "id_token": CODEX_ID_TOKEN, "authenticated_at": 4}}
        })
        .to_string(),
    )
    .unwrap();
}

async fn ok_data(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, resp) = web_call(router, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(true), "{method} {uri}: {resp}");
    resp["data"].clone()
}

fn ids(accounts: &Value) -> Vec<String> {
    let mut v: Vec<String> = accounts
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap().to_string())
        .collect();
    v.sort();
    v
}

fn file_json(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(config_dir().join(name)).unwrap()).unwrap()
}

/// No key containing "token" anywhere, and no seeded secret as a value.
fn assert_no_tokens(v: &Value) {
    let text = v.to_string();
    for secret in SECRETS {
        assert!(!text.contains(secret), "secret {secret} leaked: {text}");
    }
    fn keys(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    out.push(k.clone());
                    keys(x, out);
                }
            }
            Value::Array(a) => a.iter().for_each(|x| keys(x, out)),
            _ => {}
        }
    }
    let mut all = Vec::new();
    keys(v, &mut all);
    assert!(
        all.iter().all(|k| !k.to_lowercase().contains("token")),
        "token key in {text}"
    );
}

#[tokio::test]
#[serial]
async fn accounts_and_status_list_seeded_accounts_with_default() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    seed();
    let router = web_router();

    let accounts = ok_data(&router, "GET", "/api/auth/github_copilot/accounts", None).await;
    assert_eq!(ids(&accounts), ["1001", "1002"]);
    let default: Vec<&Value> = accounts
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["is_default"] == json!(true))
        .collect();
    assert_eq!(default.len(), 1);
    assert_eq!(default[0]["id"], json!("1001"));
    assert_eq!(default[0]["login"], json!("octo-a"));
    assert_no_tokens(&accounts);

    let status = ok_data(&router, "GET", "/api/auth/github_copilot/status", None).await;
    assert_eq!(status["authenticated"], json!(true));
    assert_eq!(status["default_account_id"], json!("1001"));
    assert_eq!(status["provider"], json!("github_copilot"));
    assert_no_tokens(&status);

    let xai = ok_data(&router, "GET", "/api/auth/xai_oauth/status", None).await;
    assert_eq!(xai["default_account_id"], json!("xai-acc-7"));
    assert_eq!(ids(&xai["accounts"]), ["xai-acc-7"]);
    assert_eq!(xai["accounts"][0]["login"], json!("grok-user"));
    assert_no_tokens(&xai);
    assert_no_tokens(&ok_data(&router, "GET", "/api/auth/xai_oauth/accounts", None).await);

    let codex = ok_data(&router, "GET", "/api/auth/codex_oauth/status", None).await;
    assert_eq!(codex["default_account_id"], json!("codex-1"));
    assert_eq!(codex["accounts"][0]["login"], json!("dev@example.com"));
    assert_no_tokens(&codex);
    let codex_accounts = ok_data(&router, "GET", "/api/auth/codex_oauth/accounts", None).await;
    assert_eq!(ids(&codex_accounts), ["codex-1"]);
    assert_no_tokens(&codex_accounts);
}

#[tokio::test]
#[serial]
async fn set_default_switches_response_and_file() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    seed();
    let router = web_router();

    let data = ok_data(
        &router,
        "PUT",
        "/api/auth/github_copilot/default",
        Some(json!({ "accountId": "1002" })),
    )
    .await;
    assert_eq!(data, Value::Null);
    let status = ok_data(&router, "GET", "/api/auth/github_copilot/status", None).await;
    assert_eq!(status["default_account_id"], json!("1002"));
    assert_eq!(
        file_json("copilot_auth.json")["default_account_id"],
        json!("1002")
    );

    // unknown account id surfaces the manager's error as 500
    let (status, body) = web_call(
        &router,
        "PUT",
        "/api/auth/github_copilot/default",
        Some(json!({ "accountId": "nope" })),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["ok"], json!(false));
}

#[tokio::test]
#[serial]
async fn delete_account_updates_response_and_file() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    seed();
    let router = web_router();

    ok_data(
        &router,
        "DELETE",
        "/api/auth/github_copilot/accounts/1002",
        None,
    )
    .await;
    let accounts = ok_data(&router, "GET", "/api/auth/github_copilot/accounts", None).await;
    assert_eq!(ids(&accounts), ["1001"]);
    let file = file_json("copilot_auth.json");
    assert!(file["accounts"].get("1002").is_none(), "{file}");
    assert!(file["accounts"].get("1001").is_some(), "{file}");

    ok_data(
        &router,
        "DELETE",
        "/api/auth/codex_oauth/accounts/codex-1",
        None,
    )
    .await;
    let codex = ok_data(&router, "GET", "/api/auth/codex_oauth/accounts", None).await;
    assert!(codex.as_array().unwrap().is_empty(), "{codex}");
    assert!(file_json("codex_oauth_auth.json")["accounts"]
        .as_object()
        .unwrap()
        .is_empty());
}

#[tokio::test]
#[serial]
async fn logout_empties_accounts() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    seed();
    let router = web_router();

    for provider in ["github_copilot", "xai_oauth", "codex_oauth"] {
        ok_data(
            &router,
            "POST",
            &format!("/api/auth/{provider}/logout"),
            None,
        )
        .await;
        let status = ok_data(
            &router,
            "GET",
            &format!("/api/auth/{provider}/status"),
            None,
        )
        .await;
        assert_eq!(
            status["authenticated"],
            json!(false),
            "{provider}: {status}"
        );
        assert!(
            status["accounts"].as_array().unwrap().is_empty(),
            "{provider}: {status}"
        );
        assert_eq!(status["default_account_id"], Value::Null, "{provider}");
    }
}

#[tokio::test]
#[serial]
async fn login_rejections_and_unknown_provider_are_400() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();

    let (status, body) = web_call(
        &router,
        "POST",
        "/api/auth/codex_oauth/login",
        Some(json!({ "loginMethod": "browser" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(
        body["error"],
        json!("browser login is unavailable in web mode: use device code")
    );

    let (status, body) = web_call(
        &router,
        "POST",
        "/api/auth/agy_oauth/login",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["ok"], json!(false));

    let (status, body) = web_call(&router, "GET", "/api/auth/bogus/status", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], json!("Unsupported auth provider: bogus"));
    let (status, body) = web_call(
        &router,
        "POST",
        "/api/auth/bogus/poll",
        Some(json!({ "deviceCode": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
#[serial]
async fn cancel_is_400_for_providers_without_cancellable_flow() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();

    let (status, body) = web_call(
        &router,
        "POST",
        "/api/auth/github_copilot/cancel",
        Some(json!({ "deviceCode": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(
        body["error"],
        json!("Login cancellation is only supported for Codex OAuth")
    );

    // Codex answers with the bool (no such flow -> false)
    let data = ok_data(
        &router,
        "POST",
        "/api/auth/codex_oauth/cancel",
        Some(json!({ "deviceCode": "x" })),
    )
    .await;
    assert_eq!(data, json!(false));
}

#[tokio::test]
#[serial]
async fn capabilities_report_server_platform_and_agy_route_is_reachable() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();

    let (status, body) = web_call(&router, "GET", "/api/capabilities", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["platform"], json!(std::env::consts::OS));

    // static `agy` segment beside `:provider`: must reach the running probe, not a 400
    let data = ok_data(&router, "GET", "/api/auth/agy/running", None).await;
    assert!(data.is_boolean(), "{data}");
}
