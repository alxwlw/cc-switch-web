mod support;

use axum::http::StatusCode;
use cc_switch_lib::{AppType, MultiAppConfig, Provider};
use serde_json::json;
use std::sync::Arc;
use support::{
    create_test_state_with_config, ensure_test_home, reset_test_fs, test_mutex, web_call,
};
use tokio::sync::broadcast::error::TryRecvError;

fn config_with_two_claude_providers() -> MultiAppConfig {
    let mut config = MultiAppConfig::default();
    let manager = config.get_manager_mut(&AppType::Claude).unwrap();
    manager.current = "old-provider".to_string();
    for (id, key) in [("old-provider", "k-old"), ("new-provider", "k-new")] {
        manager.providers.insert(
            id.to_string(),
            Provider::with_id(
                id.to_string(),
                id.to_string(),
                json!({ "env": { "ANTHROPIC_API_KEY": key } }),
                None,
            ),
        );
    }
    config
}

#[tokio::test]
async fn switch_provider_emits_provider_switched_with_the_desktop_payload_shape() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let state =
        Arc::new(create_test_state_with_config(&config_with_two_claude_providers()).unwrap());
    state
        .db
        .set_proxy_flags_sync("claude", false, true)
        .unwrap();
    let mut rx = state.events.subscribe();
    let router = cc_switch_lib::web_api::router(state.clone());

    let (status, body) = web_call(
        &router,
        "POST",
        "/api/claude/providers/switch",
        Some(json!({ "id": "new-provider" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);

    let event = rx.try_recv().expect("provider-switched was emitted");
    assert_eq!(event.name, "provider-switched");
    assert_eq!(
        event.payload,
        json!({
            "appType": "claude",
            "proxyEnabled": false,
            "autoFailoverEnabled": true,
            "providerId": "new-provider",
        })
    );
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
}

#[tokio::test]
async fn failed_switch_emits_nothing() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let state =
        Arc::new(create_test_state_with_config(&config_with_two_claude_providers()).unwrap());
    let mut rx = state.events.subscribe();
    let router = cc_switch_lib::web_api::router(state.clone());

    let (_, body) = web_call(
        &router,
        "POST",
        "/api/claude/providers/switch",
        Some(json!({ "id": "does-not-exist" })),
    )
    .await;
    assert_eq!(body["ok"], false);
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
}

#[test]
fn provider_switched_payload_reads_both_flags_from_their_sources() {
    use cc_switch_lib::live::engine::DeviceStore;
    use cc_switch_lib::mode::state::{self, Mode};
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let app_state = create_test_state_with_config(&config_with_two_claude_providers()).unwrap();
    app_state
        .db
        .set_proxy_flags_sync("claude", true, true)
        .unwrap();
    state::update(&DeviceStore::for_device(), |live| {
        let app = live.apps.entry("claude".to_string()).or_default();
        let mut mode = app.mode_state();
        mode.mode = Some(Mode::Proxy);
        app.set_mode_state(mode);
    })
    .unwrap();

    let payload =
        cc_switch_lib::events::provider_switched_payload(&app_state, &AppType::Claude, "p1");
    assert_eq!(
        payload,
        json!({
            "appType": "claude",
            "proxyEnabled": true,
            "autoFailoverEnabled": true,
            "providerId": "p1",
        })
    );
    // other apps keep their own (default) flags
    let other = cc_switch_lib::events::provider_switched_payload(&app_state, &AppType::Codex, "p2");
    assert_eq!(other["proxyEnabled"], false);
    assert_eq!(other["autoFailoverEnabled"], false);
}
