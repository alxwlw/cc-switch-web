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

mod sse {
    use super::support::create_test_state;
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Request};
    use futures::StreamExt;
    use std::time::Duration;
    use tower::ServiceExt;

    async fn open(router: &axum::Router) -> axum::http::Response<Body> {
        router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/events")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    /// Reads body chunks until `needle` count of "\n\n" frame ends is reached.
    async fn read_frames(
        stream: &mut (impl futures::Stream<Item = Result<bytes::Bytes, axum::Error>> + Unpin),
        frames: usize,
    ) -> String {
        let mut buf = String::new();
        while buf.matches("\n\n").count() < frames {
            let chunk = tokio::time::timeout(Duration::from_secs(5), stream.next())
                .await
                .expect("sse stream hung")
                .expect("stream ended early")
                .unwrap();
            buf.push_str(std::str::from_utf8(&chunk).unwrap());
        }
        buf
    }

    #[tokio::test]
    async fn events_endpoint_streams_hub_events_in_emit_order_as_named_sse_events() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_test_fs();
        ensure_test_home();
        let state = Arc::new(create_test_state().unwrap());
        let router = cc_switch_lib::web_api::router(state.clone());

        let resp = open(&router).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/event-stream"
        );
        let mut body = resp.into_body().into_data_stream();

        state
            .events
            .emit("first-event", json!({ "a": 1, "b": "x" }));
        state.events.emit("second-event", ());
        let text = read_frames(&mut body, 2).await;
        assert_eq!(
            text,
            "event: first-event\ndata: {\"a\":1,\"b\":\"x\"}\n\nevent: second-event\ndata: null\n\n"
        );
    }

    #[tokio::test]
    async fn events_endpoint_survives_lag_and_ends_when_the_hub_is_gone() {
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_test_fs();
        ensure_test_home();
        let state = Arc::new(create_test_state().unwrap());
        let router = cc_switch_lib::web_api::router(state.clone());
        let resp = open(&router).await;
        let mut body = resp.into_body().into_data_stream();

        // Overflow the 256-slot buffer before the stream is polled once.
        for i in 0..400 {
            state.events.emit("burst", json!(i));
        }
        let first = read_frames(&mut body, 1).await;
        // Lagged was skipped, not fatal: the oldest events are gone, newer ones arrive.
        assert!(first.starts_with("event: burst\ndata: "), "{first}");
        assert!(!first.starts_with("event: burst\ndata: 0\n"), "{first}");
        // ... and the stream keeps working afterwards.
        state.events.emit("after", json!("ok"));
        let rest = read_frames(&mut body, 1).await;
        assert!(rest.contains("event:"), "{rest}");

        // Hub gone (state and router dropped) -> Closed -> stream ends.
        drop(router);
        drop(state);
        let end = tokio::time::timeout(Duration::from_secs(5), async {
            while body.next().await.is_some() {}
        })
        .await;
        assert!(end.is_ok(), "stream did not end after the hub was dropped");
    }

    #[tokio::test]
    async fn init_error_router_answers_events_with_503_json_not_a_stream() {
        let router = cc_switch_lib::web_api::init_error_router();
        let resp = open(&router).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }
}
