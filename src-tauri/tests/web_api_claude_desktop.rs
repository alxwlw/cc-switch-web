//! `/api/claude-desktop/*` and `/api/official-seeds/:app/ensure` (WEB-19): Claude Desktop status,
//! default routes, import from Claude, official seed cards and Claude Desktop route mode through
//! the HTTP layer. The Claude Desktop files are on the server host: every path comes from the
//! status response, under a temp HOME with `XDG_CONFIG_HOME` pointed at it.
#![cfg(not(target_os = "windows"))]

mod support;

use axum::http::StatusCode;
use cc_switch_lib::{AppState, Provider};
use serde_json::{json, Value};
use serial_test::serial;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use support::{
    create_test_state, ensure_test_home, reset_test_fs, test_mutex, web_call, web_router_with,
};

const PROFILE_FILE: &str = "00000000-0000-4000-8000-000000157210.json";

/// Points `XDG_CONFIG_HOME` at the temp HOME for the test and puts the old value back, since
/// other tests in the binary may rely on it.
struct XdgGuard(Option<std::ffi::OsString>);

impl XdgGuard {
    fn under(home: &Path) -> Self {
        let old = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
        Self(old)
    }
}

impl Drop for XdgGuard {
    fn drop(&mut self) {
        match self.0.take() {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }
}

struct Fixture {
    state: Arc<AppState>,
    router: axum::Router,
    _xdg: XdgGuard,
}

/// Temp HOME, empty DB, proxy listen port set to 0 (an ephemeral port once it runs).
async fn fixture() -> Fixture {
    reset_test_fs();
    let home = ensure_test_home();
    let xdg = XdgGuard::under(home);
    let state = Arc::new(create_test_state().unwrap());
    let router = web_router_with(state.clone());
    let mut config = ok_data(&router, "GET", "/api/proxy/config", None).await;
    config["listenAddress"] = json!("127.0.0.1");
    config["listenPort"] = json!(0);
    ok_data(&router, "PUT", "/api/proxy/config", Some(config)).await;
    Fixture {
        state,
        router,
        _xdg: xdg,
    }
}

async fn ok_data(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, resp) = web_call(router, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(true), "{method} {uri}: {resp}");
    resp["data"].clone()
}

async fn status(router: &axum::Router) -> Value {
    ok_data(router, "GET", "/api/claude-desktop/status", None).await
}

/// Where Claude Desktop keeps its files on this host, taken from the status response.
struct DesktopDirs {
    /// `<config>/Claude` (the regular install; its existence is what `installed` reads)
    normal: PathBuf,
    /// `<config>/Claude-3p`
    threep: PathBuf,
}

impl DesktopDirs {
    fn from_status(status: &Value) -> Self {
        let library = PathBuf::from(status["configLibraryPath"].as_str().unwrap());
        let threep = library.parent().unwrap().to_path_buf();
        let normal = threep.parent().unwrap().join("Claude");
        Self { normal, threep }
    }
    fn normal_config(&self) -> PathBuf {
        self.normal.join("claude_desktop_config.json")
    }
    fn threep_config(&self) -> PathBuf {
        self.threep.join("claude_desktop_config.json")
    }
    fn profile(&self) -> PathBuf {
        self.threep.join("configLibrary").join(PROFILE_FILE)
    }
    fn meta(&self) -> PathBuf {
        self.threep.join("configLibrary").join("_meta.json")
    }
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap_or_else(|e| panic!("{path:?}: {e}")))
        .unwrap()
}

fn provider_from(value: Value) -> Provider {
    serde_json::from_value(value).unwrap()
}

/// A Claude card; `models` are written into `env` as given.
fn claude_card(id: &str, models: Value) -> Provider {
    let mut env = json!({
        "ANTHROPIC_BASE_URL": format!("https://{id}.example"),
        "ANTHROPIC_AUTH_TOKEN": format!("sk-{id}"),
    });
    for (key, value) in models.as_object().unwrap() {
        env[key] = value.clone();
    }
    provider_from(json!({
        "id": id,
        "name": id.to_uppercase(),
        "settingsConfig": { "env": env },
    }))
}

#[tokio::test]
#[serial]
async fn status_is_not_installed_without_the_claude_dir_and_installed_with_it() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;

    let before = status(&f.router).await;
    assert_eq!(before["supported"], json!(true));
    // macOS also looks for /Applications/Claude.app, which a dev machine may have
    #[cfg(not(target_os = "macos"))]
    assert_eq!(before["installed"], json!(false));
    assert_eq!(before["configured"], json!(false));
    assert_eq!(before["proxyRunning"], json!(false));
    assert_eq!(before["mode"], Value::Null);
    assert_eq!(before["gatewayTokenConfigured"], json!(false));
    let dirs = DesktopDirs::from_status(&before);
    assert_eq!(
        before["profilePath"],
        json!(dirs.profile().display().to_string())
    );
    assert!(
        dirs.normal.starts_with(ensure_test_home()),
        "the Claude dir is under the temp HOME, not the real one: {:?}",
        dirs.normal
    );

    std::fs::create_dir_all(&dirs.normal).unwrap();
    let after = status(&f.router).await;
    assert_eq!(after["supported"], json!(true));
    assert_eq!(after["installed"], json!(true));
    // a bare data dir is not a configured CC Switch profile
    assert_eq!(after["configured"], json!(false));
}

#[tokio::test]
#[serial]
async fn status_follows_the_proxy_running_flag() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    assert_eq!(status(&f.router).await["proxyRunning"], json!(false));

    ok_data(&f.router, "POST", "/api/proxy/start", None).await;
    assert_eq!(status(&f.router).await["proxyRunning"], json!(true));

    ok_data(&f.router, "POST", "/api/proxy/stop", None).await;
    assert_eq!(status(&f.router).await["proxyRunning"], json!(false));
}

#[tokio::test]
#[serial]
async fn default_routes_are_the_camel_case_role_table() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;

    let routes = ok_data(&f.router, "GET", "/api/claude-desktop/default-routes", None).await;
    let routes = routes.as_array().unwrap();
    assert_eq!(routes.len(), 4, "{routes:?}");
    assert_eq!(
        routes[0],
        json!({
            "routeId": "claude-sonnet-5",
            "envKey": "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "supports1m": true
        })
    );
    let ids: Vec<&str> = routes
        .iter()
        .map(|r| r["routeId"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "claude-sonnet-5",
            "claude-opus-5",
            "claude-haiku-4-5",
            "claude-fable-5"
        ]
    );
}

#[tokio::test]
#[serial]
async fn import_copies_claude_cards_and_adds_the_official_seed() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let db = &f.state.db;
    // goes over as a direct card: a Claude model id on a plain Anthropic endpoint
    db.save_provider(
        "claude",
        &claude_card("direct", json!({"ANTHROPIC_MODEL": "claude-sonnet-4-6"})),
    )
    .unwrap();
    // goes over as a route card: the upstream model is not a Claude id
    db.save_provider(
        "claude",
        &claude_card(
            "routed",
            json!({"ANTHROPIC_DEFAULT_SONNET_MODEL": "kimi-k3"}),
        ),
    )
    .unwrap();
    // neither: not the Anthropic wire format and no model to route
    let mut skipped = claude_card("skipped", json!({}));
    skipped.meta = Some(
        serde_json::from_value(json!({"apiFormat": "openai_chat"})).expect("meta with apiFormat"),
    );
    db.save_provider("claude", &skipped).unwrap();
    // already in the Claude Desktop table: left as the user has it
    db.save_provider(
        "claude",
        &claude_card("kept", json!({"ANTHROPIC_MODEL": "claude-sonnet-4-6"})),
    )
    .unwrap();
    let mut mine = claude_card("kept", json!({"ANTHROPIC_MODEL": "claude-sonnet-4-6"}));
    mine.name = "My own".to_string();
    db.save_provider("claude-desktop", &mine).unwrap();

    let imported = ok_data(
        &f.router,
        "POST",
        "/api/claude-desktop/import-from-claude",
        None,
    )
    .await;
    assert_eq!(imported, json!(2));

    let desktop = db.get_all_providers("claude-desktop").unwrap();
    let mut ids: Vec<&str> = desktop.keys().map(String::as_str).collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        ["claude-desktop-official", "direct", "kept", "routed"],
        "the skipped card is not copied, the official seed is added"
    );
    assert_eq!(desktop["kept"].name, "My own");
    let meta = |id: &str| serde_json::to_value(desktop[id].meta.as_ref().unwrap()).unwrap();
    assert_eq!(meta("direct")["claudeDesktopMode"], json!("direct"));
    assert_eq!(meta("routed")["claudeDesktopMode"], json!("proxy"));
    assert_eq!(
        meta("routed")["claudeDesktopModelRoutes"]["claude-sonnet-5"]["model"],
        json!("kimi-k3")
    );
    // the Claude table itself is untouched
    assert_eq!(db.get_all_providers("claude").unwrap().len(), 4);

    // a second run has nothing left to copy
    let again = ok_data(
        &f.router,
        "POST",
        "/api/claude-desktop/import-from-claude",
        None,
    )
    .await;
    assert_eq!(again, json!(0));
}

#[tokio::test]
#[serial]
async fn ensure_official_seed_is_idempotent_for_each_app() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;

    for (app, table, id) in [
        (
            "claude-desktop",
            "claude-desktop",
            "claude-desktop-official",
        ),
        ("codex", "codex", "codex-official"),
        ("grokbuild", "grokbuild", "grokbuild-official"),
    ] {
        let uri = format!("/api/official-seeds/{app}/ensure");
        assert!(
            f.state.db.get_provider_by_id(id, table).unwrap().is_none(),
            "{app}: no seed before"
        );
        assert_eq!(ok_data(&f.router, "POST", &uri, None).await, json!(true));
        assert!(
            f.state.db.get_provider_by_id(id, table).unwrap().is_some(),
            "{app}: seed row {id} in table {table}"
        );
        assert_eq!(ok_data(&f.router, "POST", &uri, None).await, json!(false));
    }
}

#[tokio::test]
#[serial]
async fn ensure_official_seed_rejects_other_apps_with_400() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;

    for app in ["claude", "gemini", "claude_desktop", "nope"] {
        let (status, resp) = web_call(
            &f.router,
            "POST",
            &format!("/api/official-seeds/{app}/ensure"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{app}: {resp}");
        assert_eq!(resp["ok"], json!(false));
        assert!(resp["error"].as_str().unwrap().contains(app), "{resp}");
    }
    // a refused call writes nothing
    assert!(f.state.db.get_all_providers("claude").unwrap().is_empty());
}

#[tokio::test]
#[serial]
async fn route_mode_writes_the_profile_from_the_client_host_and_exit_restores_the_files() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture().await;
    let dirs = DesktopDirs::from_status(&status(&f.router).await);

    // Claude Desktop is installed on this host and the user already has a config file
    std::fs::create_dir_all(&dirs.normal).unwrap();
    std::fs::write(
        dirs.normal_config(),
        r#"{"theme": "dark", "deploymentMode": "1p"}"#,
    )
    .unwrap();

    // a route card, and a client host other than loopback
    let routed = provider_from(json!({
        "id": "map",
        "name": "MAP",
        "settingsConfig": { "env": {
            "ANTHROPIC_BASE_URL": "https://map.example",
            "ANTHROPIC_AUTH_TOKEN": "sk-map",
        }},
        "meta": {
            "claudeDesktopMode": "proxy",
            "claudeDesktopModelRoutes": {
                "claude-sonnet-5": { "model": "kimi-k3" }
            }
        }
    }));
    f.state.db.save_provider("claude-desktop", &routed).unwrap();
    ok_data(
        &f.router,
        "PUT",
        "/api/proxy/client-host",
        Some(json!({"host": "cc-switch.example.test"})),
    )
    .await;

    // entering route mode starts the proxy by itself
    ok_data(
        &f.router,
        "PUT",
        "/api/proxy/takeover/claude-desktop",
        Some(json!({"enabled": true, "route": "map"})),
    )
    .await;
    let port = ok_data(&f.router, "GET", "/api/proxy/status", None).await["port"]
        .as_u64()
        .unwrap();
    assert!(port > 0);

    let expected = format!("http://cc-switch.example.test:{port}/claude-desktop");
    let profile = read_json(&dirs.profile());
    assert_eq!(
        profile["inferenceGatewayBaseUrl"],
        json!(expected),
        "the profile URL is built from the configured client host, not a fixed 127.0.0.1"
    );
    assert_eq!(profile["inferenceProvider"], json!("gateway"));
    assert_eq!(profile["inferenceModels"], json!(["claude-sonnet-5"]));
    assert_eq!(
        read_json(&dirs.normal_config())["deploymentMode"],
        json!("3p")
    );
    assert_eq!(
        read_json(&dirs.threep_config())["deploymentMode"],
        json!("3p")
    );
    let meta = read_json(&dirs.meta());
    assert_eq!(
        meta["appliedId"],
        json!(PROFILE_FILE.trim_end_matches(".json"))
    );
    assert_eq!(meta["entries"][0]["name"], json!("CC Switch"));

    let live = status(&f.router).await;
    assert_eq!(live["mode"], json!("proxy"));
    assert_eq!(live["configured"], json!(true));
    assert_eq!(live["installed"], json!(true));
    assert_eq!(live["proxyRunning"], json!(true));
    assert_eq!(live["expectedBaseUrl"], json!(expected));
    assert_eq!(live["actualBaseUrl"], json!(expected));
    assert_eq!(live["gatewayTokenConfigured"], json!(true));

    // changing the client host rewrites the profile the route points at
    ok_data(
        &f.router,
        "PUT",
        "/api/proxy/client-host",
        Some(json!({"host": "10.9.8.7"})),
    )
    .await;
    assert_eq!(
        read_json(&dirs.profile())["inferenceGatewayBaseUrl"],
        json!(format!("http://10.9.8.7:{port}/claude-desktop"))
    );
    ok_data(
        &f.router,
        "PUT",
        "/api/proxy/client-host",
        Some(json!({"host": ""})),
    )
    .await;
    assert_eq!(
        read_json(&dirs.profile())["inferenceGatewayBaseUrl"],
        json!(format!("http://127.0.0.1:{port}/claude-desktop")),
        "with no client host the listen address is the fallback"
    );

    // leaving route mode: what restore really does is the official (1p) setup
    ok_data(
        &f.router,
        "PUT",
        "/api/proxy/takeover/claude-desktop",
        Some(json!({"enabled": false})),
    )
    .await;
    let normal = read_json(&dirs.normal_config());
    assert_eq!(normal["deploymentMode"], json!("1p"));
    assert_eq!(normal["theme"], json!("dark"), "the user's own keys stay");
    assert_eq!(
        read_json(&dirs.threep_config())["deploymentMode"],
        json!("1p")
    );
    // the profile file stays (user edits survive), its connection fields are cleared
    let profile = read_json(&dirs.profile());
    for key in [
        "inferenceProvider",
        "inferenceGatewayBaseUrl",
        "inferenceGatewayApiKey",
        "inferenceGatewayAuthScheme",
        "inferenceModels",
    ] {
        assert!(profile.get(key).is_none(), "{key} is cleared: {profile}");
    }
    // and Desktop no longer lists or applies it
    let meta = read_json(&dirs.meta());
    assert!(meta.get("appliedId").is_none(), "{meta}");
    assert_eq!(meta["entries"], json!([]));
    let after = status(&f.router).await;
    assert_eq!(after["mode"], Value::Null);
    assert_eq!(after["configured"], json!(false));
    assert_eq!(after["actualBaseUrl"], Value::Null);
    // the last app left route mode, so the proxy stopped by itself
    assert_eq!(after["proxyRunning"], json!(false));
}
