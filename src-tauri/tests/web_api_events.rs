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
            "autoFailoverEnabled": false,
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
