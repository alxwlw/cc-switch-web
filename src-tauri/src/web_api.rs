//! Headless HTTP API for the cc-switch-web fork (Tailscale-only, no in-app auth).
//!
//! M1 scope: `/api/health` + `/api/capabilities` only. M2 adds provider / MCP /
//! prompts / skills / sessions / usage endpoints on top of the existing
//! handle-free `*_internal(&AppState, …)` service functions.

use std::sync::Arc;

use axum::{routing::get, Json, Router};
use serde_json::{json, Value};

use crate::store::AppState;

pub type SharedState = Arc<AppState>;

pub fn router(_state: SharedState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/capabilities", get(capabilities))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn capabilities() -> Json<Value> {
    Json(json!({
        "mode": "web",
        "auth": "none (tailscale-only; TRUSTED_TAILNET=1 required for non-loopback bind)",
        "version": env!("CARGO_PKG_VERSION"),
        "mvp_apps": ["claude", "codex", "gemini", "opencode"],
        "endpoints": ["/api/health", "/api/capabilities"],
        "runtime_boundaries": "manages files on the server host only; no tray, no native file pickers, no auto-update",
    }))
}
