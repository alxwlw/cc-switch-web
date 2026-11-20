#[allow(dead_code)]
mod support;

use axum::http::StatusCode;
use cc_switch_lib::{update_settings, AppSettings};
use serde_json::{json, Value};
use serial_test::serial;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use support::{create_test_state, ensure_test_home, reset_test_fs, web_call, web_router_with};

/// Router over a fresh state with `openclaw_config_dir` pointing at a temp dir.
fn fresh() -> (axum::Router, PathBuf) {
    reset_test_fs();
    let home = ensure_test_home();
    let dir = home.join(".openclaw-web-api");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("openclaw dir");
    update_settings(AppSettings {
        openclaw_config_dir: Some(dir.to_string_lossy().into_owned()),
        ..AppSettings::default()
    })
    .expect("openclaw dir override");
    (
        web_router_with(Arc::new(create_test_state().expect("state"))),
        dir,
    )
}

async fn data(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, resp) = web_call(router, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(true), "{method} {uri}: {resp}");
    resp["data"].clone()
}

/// Config with unknown fields at every level the typed structs flatten.
const CONFIG: &str = r#"{
  "models": {
    "mode": "merge",
    "providers": {
      "alpha": {
        "baseUrl": "https://a.example.com/v1",
        "apiKey": "sk-a",
        "api": "openai-completions",
        "models": [{"id": "m-a", "name": "Model A"}]
      },
      "beta": {
        "baseUrl": "https://b.example.com/v1",
        "apiKey": "sk-b",
        "models": [{"id": "m-b"}]
      },
      "nomodels": {"baseUrl": "https://c.example.com/v1"}
    }
  },
  "agents": {
    "defaults": {
      "model": {"primary": "alpha/m-a", "fallbacks": ["beta/m-b"], "x-model-extra": 1},
      "models": {"alpha/m-a": {"alias": "A", "x-cat-extra": "keep"}},
      "workspace": "/srv/ws"
    }
  },
  "env": {"vars": {"K": "V"}, "unknownEnv": true},
  "tools": {"profile": "full", "allow": ["a"], "deny": ["d"], "x-tools-extra": [1, 2]},
  "gateway": {"port": 18789}
}"#;

fn seed(dir: &std::path::Path) {
    fs::write(dir.join("openclaw.json"), CONFIG).expect("seed");
}

fn live(dir: &std::path::Path) -> Value {
    json5::from_str(&fs::read_to_string(dir.join("openclaw.json")).unwrap()).expect("live json5")
}

#[tokio::test]
#[serial]
async fn default_model_round_trip_keeps_unknown_fields() {
    let (router, dir) = fresh();
    seed(&dir);
    let got = data(&router, "GET", "/api/openclaw/default-model", None).await;
    assert_eq!(
        got,
        json!({"primary": "alpha/m-a", "fallbacks": ["beta/m-b"], "x-model-extra": 1})
    );

    let new = json!({"primary": "beta/m-b", "fallbacks": [], "x-model-extra": 2, "added": "yes"});
    data(
        &router,
        "PUT",
        "/api/openclaw/default-model",
        Some(json!({"model": new})),
    )
    .await;
    assert_eq!(
        data(&router, "GET", "/api/openclaw/default-model", None).await,
        json!({"primary": "beta/m-b", "x-model-extra": 2, "added": "yes"})
    );
    let cfg = live(&dir);
    assert_eq!(
        cfg["agents"]["defaults"]["model"]["primary"],
        json!("beta/m-b")
    );
    // siblings of the written section survive
    assert_eq!(cfg["agents"]["defaults"]["workspace"], json!("/srv/ws"));
    assert_eq!(cfg["gateway"], json!({"port": 18789}));
}

#[tokio::test]
#[serial]
async fn model_catalog_round_trip_keeps_unknown_fields() {
    let (router, dir) = fresh();
    seed(&dir);
    assert_eq!(
        data(&router, "GET", "/api/openclaw/model-catalog", None).await,
        json!({"alpha/m-a": {"alias": "A", "x-cat-extra": "keep"}})
    );
    let catalog = json!({
        "beta/m-b": {"alias": "B", "x-new": [true]},
        "gamma/m-g": {"x-only": 7}
    });
    data(
        &router,
        "PUT",
        "/api/openclaw/model-catalog",
        Some(json!({"catalog": catalog.clone()})),
    )
    .await;
    assert_eq!(
        data(&router, "GET", "/api/openclaw/model-catalog", None).await,
        catalog
    );
    let cfg = live(&dir);
    assert_eq!(
        cfg["agents"]["defaults"]["model"]["primary"],
        json!("alpha/m-a")
    );
    assert_eq!(cfg["agents"]["defaults"]["workspace"], json!("/srv/ws"));
}

#[tokio::test]
#[serial]
async fn agents_defaults_round_trip_keeps_unknown_fields() {
    let (router, dir) = fresh();
    seed(&dir);
    let got = data(&router, "GET", "/api/openclaw/agents-defaults", None).await;
    assert_eq!(got["workspace"], json!("/srv/ws"));
    assert_eq!(got["model"]["primary"], json!("alpha/m-a"));

    let new = json!({
        "model": {"primary": "gamma/m-g"},
        "workspace": "/srv/other",
        "x-defaults-extra": {"deep": 1}
    });
    data(
        &router,
        "PUT",
        "/api/openclaw/agents-defaults",
        Some(json!({"defaults": new})),
    )
    .await;
    assert_eq!(
        data(&router, "GET", "/api/openclaw/agents-defaults", None).await,
        json!({
            "model": {"primary": "gamma/m-g"},
            "workspace": "/srv/other",
            "x-defaults-extra": {"deep": 1}
        })
    );
    assert_eq!(live(&dir)["gateway"], json!({"port": 18789}));
}

#[tokio::test]
#[serial]
async fn env_round_trip_keeps_unknown_fields() {
    let (router, dir) = fresh();
    seed(&dir);
    assert_eq!(
        data(&router, "GET", "/api/openclaw/env", None).await,
        json!({"vars": {"K": "V"}, "unknownEnv": true})
    );
    let env = json!({"vars": {"K2": "V2"}, "shellEnv": {"enabled": true}, "other": 5});
    data(
        &router,
        "PUT",
        "/api/openclaw/env",
        Some(json!({"env": env.clone()})),
    )
    .await;
    assert_eq!(data(&router, "GET", "/api/openclaw/env", None).await, env);
    assert_eq!(live(&dir)["tools"]["profile"], json!("full"));
}

#[tokio::test]
#[serial]
async fn tools_round_trip_keeps_unknown_fields() {
    let (router, dir) = fresh();
    seed(&dir);
    assert_eq!(
        data(&router, "GET", "/api/openclaw/tools", None).await,
        json!({"profile": "full", "allow": ["a"], "deny": ["d"], "x-tools-extra": [1, 2]})
    );
    let tools = json!({"profile": "coding", "allow": ["x", "y"], "x-tools-extra": {"n": 3}});
    data(
        &router,
        "PUT",
        "/api/openclaw/tools",
        Some(json!({"tools": tools.clone()})),
    )
    .await;
    assert_eq!(
        data(&router, "GET", "/api/openclaw/tools", None).await,
        tools
    );
    assert_eq!(live(&dir)["env"]["vars"], json!({"K": "V"}));
}

#[tokio::test]
#[serial]
async fn empty_config_reads_defaults() {
    let (router, _dir) = fresh();
    assert_eq!(
        data(&router, "GET", "/api/openclaw/default-model", None).await,
        Value::Null
    );
    assert_eq!(
        data(&router, "GET", "/api/openclaw/model-catalog", None).await,
        Value::Null
    );
    assert_eq!(
        data(&router, "GET", "/api/openclaw/agents-defaults", None).await,
        Value::Null
    );
    assert_eq!(
        data(&router, "GET", "/api/openclaw/env", None).await,
        json!({})
    );
    assert_eq!(
        data(&router, "GET", "/api/openclaw/tools", None).await,
        json!({})
    );
}

#[tokio::test]
#[serial]
async fn put_with_wrong_body_key_is_rejected() {
    let (router, dir) = fresh();
    seed(&dir);
    let before = fs::read_to_string(dir.join("openclaw.json")).unwrap();
    use axum::body::Body;
    use axum::http::{header, Request};
    use tower::ServiceExt;
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/openclaw/default-model")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"primary": "x"}).to_string()))
                .unwrap(),
        )
        .await
        .expect("infallible");
    let status = resp.status();
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{status}");
    assert_eq!(
        fs::read_to_string(dir.join("openclaw.json")).unwrap(),
        before
    );
}

#[tokio::test]
#[serial]
async fn health_reports_known_problem_and_clean_config_is_empty() {
    let (router, dir) = fresh();
    // no file: nothing to warn about
    assert_eq!(
        data(&router, "GET", "/api/openclaw/health", None).await,
        json!([])
    );
    seed(&dir);
    assert_eq!(
        data(&router, "GET", "/api/openclaw/health", None).await,
        json!([])
    );
    fs::write(
        dir.join("openclaw.json"),
        r#"{"tools": {"profile": "bogus"}, "agents": {"defaults": {"timeout": 5}}}"#,
    )
    .unwrap();
    assert_eq!(
        data(&router, "GET", "/api/openclaw/health", None).await,
        json!([
            {
                "code": "invalid_tools_profile",
                "message": "tools.profile uses unsupported value 'bogus'.",
                "path": "tools.profile"
            },
            {
                "code": "legacy_agents_timeout",
                "message": "agents.defaults.timeout is deprecated; use agents.defaults.timeoutSeconds.",
                "path": "agents.defaults.timeout"
            }
        ])
    );
}

#[tokio::test]
#[serial]
async fn live_provider_ids_and_single_provider() {
    let (router, dir) = fresh();
    seed(&dir);
    let mut ids: Vec<String> =
        serde_json::from_value(data(&router, "GET", "/api/openclaw/live-provider-ids", None).await)
            .unwrap();
    ids.sort();
    assert_eq!(ids, vec!["alpha", "beta", "nomodels"]);

    let alpha = data(&router, "GET", "/api/openclaw/live-provider/alpha", None).await;
    assert_eq!(alpha["baseUrl"], json!("https://a.example.com/v1"));
    assert_eq!(alpha["models"][0]["id"], json!("m-a"));
    assert_eq!(
        data(&router, "GET", "/api/openclaw/live-provider/missing", None).await,
        Value::Null
    );
}

#[tokio::test]
#[serial]
async fn import_live_imports_providers_with_models_only() {
    let (router, dir) = fresh();
    seed(&dir);
    // "nomodels" is skipped by the service, so 2 of 3 are imported
    assert_eq!(
        data(&router, "POST", "/api/openclaw/import-live", None).await,
        json!(2)
    );
    let providers = data(&router, "GET", "/api/openclaw/providers", None).await;
    let mut ids: Vec<String> = providers
        .as_object()
        .expect("map")
        .keys()
        .cloned()
        .collect();
    ids.sort();
    assert_eq!(ids, vec!["alpha", "beta"]);
}
