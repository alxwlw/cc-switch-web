//! `/api/kimi/oauth/*` (WEB-02 stage 2). Credentials are expired access tokens with a
//! refresh token: the account counts as signed in, and `status` skips the identity fetch
//! (it only runs for a still-valid access token), so no test touches the network.
//! start / poll-with-a-flow / quota / models need the Kimi auth servers; there is no base
//! URL seam outside `#[cfg(test)]`, so they are checked by hand (plan phase 26, Task 4).
#![cfg(not(target_os = "windows"))]

mod support;

use axum::http::StatusCode;
use cc_switch_lib::Provider;
use serde_json::{json, Value};
use serial_test::serial;
use std::ffi::OsString;
use std::path::PathBuf;
use support::{
    create_test_state, ensure_test_home, reset_test_fs, test_mutex, web_call, web_router_with,
};

const ACCESS: &str = "kimi-SECRET-access-token";
const REFRESH: &str = "kimi-SECRET-refresh-token";

struct KimiHome {
    directory: tempfile::TempDir,
    previous: Option<OsString>,
}
impl KimiHome {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("KIMI_CODE_HOME");
        std::env::set_var("KIMI_CODE_HOME", directory.path());
        Self {
            directory,
            previous,
        }
    }
    fn path(&self) -> PathBuf {
        self.directory.path().to_path_buf()
    }
}
impl Drop for KimiHome {
    fn drop(&mut self) {
        match &self.previous {
            Some(v) => std::env::set_var("KIMI_CODE_HOME", v),
            None => std::env::remove_var("KIMI_CODE_HOME"),
        }
    }
}

const SECOND: &str = "kimi-code-ccs-second";
const THIRD: &str = "kimi-code-ccs-third";
const PRIMARY: &str = "managed:kimi-code";

fn slot(id: &str) -> String {
    id.replace(':', "-")
}

/// Three official accounts, `managed:kimi-code` current; credentials expired long ago.
fn seed(home: &KimiHome) {
    let credentials = home.path().join("credentials");
    std::fs::create_dir_all(&credentials).unwrap();
    let mut config =
        json!({"default_model": format!("{PRIMARY}/k3"), "providers": {}, "models": {}});
    for id in [PRIMARY, SECOND, THIRD] {
        config["providers"][id] = json!({"type":"kimi","base_url":"https://api.kimi.com/coding/v1","oauth":{"storage":"file","key":format!("oauth/{}", slot(id))}});
        config["models"][format!("{id}/k3")] =
            json!({"provider":id,"model":"k3","max_context_size":1048576});
        std::fs::write(
            credentials.join(format!("{}.json", slot(id))),
            json!({"access_token": ACCESS, "refresh_token": REFRESH, "expires_at": 1}).to_string(),
        )
        .unwrap();
    }
    std::fs::write(
        home.path().join("config.toml"),
        toml::to_string(&config).unwrap(),
    )
    .unwrap();
}

fn assert_no_secrets(v: &Value) {
    let text = v.to_string();
    for secret in [ACCESS, REFRESH] {
        assert!(!text.contains(secret), "secret {secret} leaked: {text}");
    }
}

async fn ok_data(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, resp) = web_call(router, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(true), "{method} {uri}: {resp}");
    assert_no_secrets(&resp);
    resp["data"].clone()
}

#[tokio::test]
#[serial]
async fn status_lists_prepared_accounts_and_current() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let home = KimiHome::new();
    seed(&home);
    let router = web_router_with(std::sync::Arc::new(create_test_state().unwrap()));

    let status = ok_data(&router, "GET", "/api/kimi/oauth/status", None).await;
    let mut ids: Vec<&str> = status["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    ids.sort();
    let mut expected = vec![PRIMARY, SECOND, THIRD];
    expected.sort();
    assert_eq!(ids, expected);
    assert_eq!(status["currentAccountId"], json!(PRIMARY));
    let primary = status["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == json!(PRIMARY))
        .unwrap();
    assert_eq!(primary["authenticated"], json!(true));
    assert_eq!(primary["primary"], json!(true));
}

#[tokio::test]
#[serial]
async fn use_account_switches_current_and_unknown_account_is_500() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let home = KimiHome::new();
    seed(&home);
    let router = web_router_with(std::sync::Arc::new(create_test_state().unwrap()));

    let uri = format!("/api/kimi/oauth/accounts/{SECOND}/use");
    ok_data(&router, "POST", &uri, None).await;
    let status = ok_data(&router, "GET", "/api/kimi/oauth/status", None).await;
    assert_eq!(status["currentAccountId"], json!(SECOND));

    let (code, resp) = web_call(
        &router,
        "POST",
        "/api/kimi/oauth/accounts/no-such/use",
        None,
    )
    .await;
    assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR, "{resp}");
    assert_eq!(resp["ok"], json!(false));
    assert_eq!(resp["error"], json!("Kimi official account not found"));
}

#[tokio::test]
#[serial]
async fn remove_deletes_credential_provider_row_and_config_node() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let home = KimiHome::new();
    seed(&home);
    let state = std::sync::Arc::new(create_test_state().unwrap());
    state
        .db
        .save_provider(
            "kimi",
            &Provider::with_id(THIRD.into(), "third".into(), json!({}), None),
        )
        .unwrap();
    assert!(state
        .db
        .get_provider_by_id(THIRD, "kimi")
        .unwrap()
        .is_some());
    let router = web_router_with(state.clone());

    let credential = home
        .path()
        .join("credentials")
        .join(format!("{THIRD}.json"));
    assert!(credential.exists());
    let data = ok_data(
        &router,
        "DELETE",
        &format!("/api/kimi/oauth/accounts/{THIRD}"),
        None,
    )
    .await;
    assert_eq!(data, Value::Null);
    assert!(!credential.exists(), "credential file kept");
    assert!(
        state
            .db
            .get_provider_by_id(THIRD, "kimi")
            .unwrap()
            .is_none(),
        "provider row kept"
    );
    let config = std::fs::read_to_string(home.path().join("config.toml")).unwrap();
    assert!(!config.contains(THIRD), "config still names the account");
    // Siblings are untouched.
    assert!(home
        .path()
        .join("credentials")
        .join(format!("{SECOND}.json"))
        .exists());
    let status = ok_data(&router, "GET", "/api/kimi/oauth/status", None).await;
    assert_eq!(status["accounts"].as_array().unwrap().len(), 2);
}

#[tokio::test]
#[serial]
async fn removing_the_primary_account_only_hides_it() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let home = KimiHome::new();
    seed(&home);
    let router = web_router_with(std::sync::Arc::new(create_test_state().unwrap()));
    assert_eq!(
        ok_data(&router, "GET", "/api/kimi/oauth/status", None).await["primaryHidden"],
        json!(false)
    );

    // `:` in the id arrives percent-encoded, as the web adapter sends it
    ok_data(
        &router,
        "DELETE",
        "/api/kimi/oauth/accounts/managed%3Akimi-code",
        None,
    )
    .await;

    let status = ok_data(&router, "GET", "/api/kimi/oauth/status", None).await;
    assert_eq!(status["primaryHidden"], json!(true));
    let ids: Vec<&str> = status["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&PRIMARY), "{ids:?}");
    // Kimi's own login is hidden, not signed out: the credential file stays
    assert!(home
        .path()
        .join("credentials")
        .join(format!("{}.json", slot(PRIMARY)))
        .exists());
}

#[tokio::test]
#[serial]
async fn removing_the_current_separate_account_is_refused_and_keeps_it() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let home = KimiHome::new();
    seed(&home);
    let router = web_router_with(std::sync::Arc::new(create_test_state().unwrap()));
    ok_data(
        &router,
        "POST",
        &format!("/api/kimi/oauth/accounts/{SECOND}/use"),
        None,
    )
    .await;

    let (code, resp) = web_call(
        &router,
        "DELETE",
        &format!("/api/kimi/oauth/accounts/{SECOND}"),
        None,
    )
    .await;
    assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR, "{resp}");
    assert_eq!(resp["ok"], json!(false));
    assert!(home
        .path()
        .join("credentials")
        .join(format!("{SECOND}.json"))
        .exists());
}

#[tokio::test]
#[serial]
async fn poll_and_cancel_of_an_unknown_flow() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let _home = KimiHome::new();
    let router = web_router_with(std::sync::Arc::new(create_test_state().unwrap()));

    let poll = ok_data(
        &router,
        "POST",
        "/api/kimi/oauth/poll",
        Some(json!({"flowId": "nope"})),
    )
    .await;
    assert_eq!(poll["ended"], json!("cancelled"));
    assert_eq!(poll["authenticated"], json!(false));

    let cancel = ok_data(
        &router,
        "POST",
        "/api/kimi/oauth/cancel",
        Some(json!({"flowId": "nope"})),
    )
    .await;
    assert_eq!(cancel, json!(false));

    let (code, resp) = web_call(&router, "POST", "/api/kimi/oauth/poll", Some(json!({}))).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{resp}");
}

#[tokio::test]
#[serial]
async fn start_with_a_bad_region_is_400() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let _home = KimiHome::new();
    let router = web_router_with(std::sync::Arc::new(create_test_state().unwrap()));

    for body in [json!({"region": "mars"}), json!({}), json!({"region": 7})] {
        let (code, resp) =
            web_call(&router, "POST", "/api/kimi/oauth/start", Some(body.clone())).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{body}: {resp}");
        assert_eq!(resp["ok"], json!(false));
    }
}
