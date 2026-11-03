//! WEB-26: `POST /api/restart` and the `instanceId` of `/api/health`, through the axum harness
//! with an injected `ShutdownHandle` (the process is never touched).
//!
//! The supervisor check reads `INVOCATION_ID` / `CC_SWITCH_WEB_SUPERVISED` when the request
//! arrives, and the agent's shell has `INVOCATION_ID` set: every test is `#[serial]` and starts
//! from `Fixture::new`, which removes both variables and puts them back on drop. The runtime is
//! paused, so the delay between the 202 and the signal is stepped, not slept.

#[allow(dead_code)]
mod support;

use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use cc_switch_lib::web_api::{
    instance_id, router_with_shutdown, ShutdownHandle, RESTART_SIGNAL_DELAY,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use serial_test::serial;
use support::{create_test_state, ensure_test_home, reset_test_fs, web_call, web_router};
use tower::ServiceExt;

const SYSTEMD_ENV: &str = "INVOCATION_ID";
const SUPERVISED_ENV: &str = "CC_SWITCH_WEB_SUPERVISED";
const MANUAL_TEXT: &str = "перезапустите сервер вручную";

struct Fixture {
    saved: Vec<(&'static str, Option<OsString>)>,
}

impl Fixture {
    fn new() -> Self {
        reset_test_fs();
        ensure_test_home();
        let saved = [SYSTEMD_ENV, SUPERVISED_ENV]
            .into_iter()
            .map(|name| (name, std::env::var_os(name)))
            .collect();
        std::env::remove_var(SYSTEMD_ENV);
        std::env::remove_var(SUPERVISED_ENV);
        Self { saved }
    }

    /// A router that can restart the process, and the handle it signals.
    fn router(&self) -> (axum::Router, ShutdownHandle) {
        let handle = ShutdownHandle::new();
        let state = Arc::new(create_test_state().expect("create test state"));
        (router_with_shutdown(state, Some(handle.clone())), handle)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for (name, value) in &self.saved {
            match value {
                Some(v) => std::env::set_var(name, v),
                None => std::env::remove_var(name),
            }
        }
    }
}

async fn restart(router: &axum::Router) -> (StatusCode, Value) {
    web_call(router, "POST", "/api/restart", Some(json!({}))).await
}

/// Steps the paused clock past the signal delay and lets the spawned task run.
async fn after_the_signal_delay() {
    tokio::time::advance(RESTART_SIGNAL_DELAY * 2).await;
    tokio::task::yield_now().await;
}

async fn health(router: &axum::Router) -> Value {
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router is infallible");
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).expect("json body")
}

#[tokio::test(start_paused = true)]
#[serial]
async fn without_a_supervisor_restart_is_409_and_the_signal_stays_down() {
    let fx = Fixture::new();
    let (router, handle) = fx.router();

    let (status, body) = restart(&router).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert!(
        body["error"].as_str().unwrap().contains(MANUAL_TEXT),
        "{body}"
    );
    after_the_signal_delay().await;
    assert!(!handle.is_requested(), "no supervisor: no shutdown signal");
}

#[tokio::test(start_paused = true)]
#[serial]
async fn supervised_by_the_env_flag_restart_is_202_and_the_signal_follows_the_response() {
    let fx = Fixture::new();
    std::env::set_var(SUPERVISED_ENV, "1");
    let (router, handle) = fx.router();

    let (status, body) = restart(&router).await;

    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["data"]["instanceId"], json!(instance_id()), "{body}");
    assert!(
        !handle.is_requested(),
        "the signal must not be raised before the response is out"
    );
    tokio::time::timeout(Duration::from_secs(5), handle.requested())
        .await
        .expect("shutdown signal after the 202");
    assert!(handle.is_requested());
}

#[tokio::test(start_paused = true)]
#[serial]
async fn systemd_invocation_id_counts_as_a_supervisor_and_an_empty_one_does_not() {
    let fx = Fixture::new();
    let (router, handle) = fx.router();

    std::env::set_var(SYSTEMD_ENV, "");
    let (status, body) = restart(&router).await;
    assert_eq!(status, StatusCode::CONFLICT, "empty INVOCATION_ID: {body}");

    std::env::set_var(SYSTEMD_ENV, "0f3c9a5e1d2b4c6f8a7b9c0d1e2f3a4b");
    let (status, body) = restart(&router).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    tokio::time::timeout(Duration::from_secs(5), handle.requested())
        .await
        .expect("shutdown signal");
}

#[tokio::test(start_paused = true)]
#[serial]
async fn only_the_exact_flag_value_one_counts_as_a_supervisor() {
    let fx = Fixture::new();
    let (router, handle) = fx.router();

    for value in ["0", "", "true", "yes"] {
        std::env::set_var(SUPERVISED_ENV, value);
        let (status, body) = restart(&router).await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "CC_SWITCH_WEB_SUPERVISED={value:?}: {body}"
        );
    }
    after_the_signal_delay().await;
    assert!(!handle.is_requested());
}

#[tokio::test(start_paused = true)]
#[serial]
async fn a_router_built_without_a_handle_answers_409_even_under_a_supervisor() {
    let _fx = Fixture::new();
    std::env::set_var(SUPERVISED_ENV, "1");
    let router = web_router();

    let (status, body) = restart(&router).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["error"].as_str().unwrap().contains(MANUAL_TEXT),
        "{body}"
    );
}

#[tokio::test(start_paused = true)]
#[serial]
async fn restart_needs_a_json_content_type_so_a_cross_site_post_cannot_restart_the_server() {
    let fx = Fixture::new();
    std::env::set_var(SUPERVISED_ENV, "1");
    let (router, handle) = fx.router();

    for content_type in [
        None,
        Some("text/plain"),
        Some("application/x-www-form-urlencoded"),
    ] {
        let mut req = Request::builder().method("POST").uri("/api/restart");
        if let Some(ct) = content_type {
            req = req.header(header::CONTENT_TYPE, ct);
        }
        let resp = router
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .expect("router is infallible");
        assert_eq!(
            resp.status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "content type {content_type:?}"
        );
    }
    after_the_signal_delay().await;
    assert!(!handle.is_requested(), "a refused request must not restart");
}

#[tokio::test(start_paused = true)]
#[serial]
async fn the_content_type_check_comes_before_the_supervisor_check() {
    let fx = Fixture::new();
    let (router, _handle) = fx.router();

    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/restart")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router is infallible");

    // No supervisor here: a 409 would mean the supervisor was checked first.
    assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test(start_paused = true)]
#[serial]
async fn a_second_restart_during_the_shutdown_is_202_again_and_does_not_panic() {
    let fx = Fixture::new();
    std::env::set_var(SUPERVISED_ENV, "1");
    let (router, handle) = fx.router();

    let (first, _) = restart(&router).await;
    tokio::time::timeout(Duration::from_secs(5), handle.requested())
        .await
        .expect("first signal");
    // The shutdown is under way: the latch is already up.
    let (second, body) = restart(&router).await;
    let (third, _) = restart(&router).await;
    after_the_signal_delay().await;

    assert_eq!(first, StatusCode::ACCEPTED);
    assert_eq!(second, StatusCode::ACCEPTED, "{body}");
    assert_eq!(third, StatusCode::ACCEPTED);
    assert_eq!(body["data"]["instanceId"], json!(instance_id()));
    assert!(handle.is_requested());
}

#[tokio::test(start_paused = true)]
#[serial]
async fn health_has_no_envelope_and_shares_a_stable_instance_id_with_the_202() {
    let fx = Fixture::new();
    std::env::set_var(SUPERVISED_ENV, "1");
    let (router, _handle) = fx.router();

    let before = health(&router).await;
    let (_, accepted) = restart(&router).await;
    let after = health(&router).await;

    assert_eq!(
        before,
        json!({ "status": "ok", "instanceId": instance_id() }),
        "health stays {{status, instanceId}} with no `ok` envelope"
    );
    assert_eq!(before, after, "the id is stable inside the process");
    assert_eq!(accepted["data"]["instanceId"], before["instanceId"]);
    assert!(!instance_id().is_empty());
}

#[tokio::test(start_paused = true)]
#[serial]
async fn a_raised_handle_resolves_requested_at_once_for_every_clone() {
    let _fx = Fixture::new();
    let handle = ShutdownHandle::new();
    let clone = handle.clone();
    assert!(!clone.is_requested());

    handle.request();
    handle.request();

    tokio::time::timeout(Duration::from_secs(1), clone.requested())
        .await
        .expect("already raised: resolves immediately");
    assert!(clone.is_requested());
}
