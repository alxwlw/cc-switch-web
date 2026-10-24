//! Headless HTTP API for the cc-switch-web fork (Tailscale-only, no in-app auth).
//!
//! Thin Axum handlers over the existing handle-free service layer — the same
//! `*Service::*(state: &AppState, …)` functions the Tauri commands wrap.
//! Blocking provider ops run in `spawn_blocking`, mirroring `commands::*`.

use std::str::FromStr;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::app_config::{AppType, McpServer};
use crate::prompt::Prompt;
use crate::provider::Provider;
use crate::services::prompt::PromptService;
use crate::services::provider::{EditorSave, ProviderService};
use crate::services::skill::SkillService;
use crate::services::McpService;
use crate::session_manager;
use crate::store::AppState;

pub type SharedState = Arc<AppState>;

type ApiResult = (StatusCode, Json<Value>);

fn ok(data: impl serde::Serialize) -> ApiResult {
    (StatusCode::OK, Json(json!({ "ok": true, "data": data })))
}

fn fail(msg: impl ToString) -> ApiResult {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "ok": false, "error": msg.to_string() })),
    )
}

fn bad_request(msg: impl ToString) -> ApiResult {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "ok": false, "error": msg.to_string() })),
    )
}

macro_rules! app_type {
    ($app:expr) => {
        match AppType::from_str($app) {
            Ok(t) => t,
            Err(e) => return bad_request(e.to_string()),
        }
    };
}

pub fn router(state: SharedState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/capabilities", get(capabilities))
        // providers (per app)
        .route("/api/:app/providers", get(list_providers).post(add_provider))
        .route("/api/:app/providers/current", get(current_provider))
        .route("/api/:app/providers/switch", post(switch_provider))
        .route("/api/:app/providers/:id", delete(delete_provider))
        // prompts (per app)
        .route("/api/:app/prompts", get(list_prompts))
        .route(
            "/api/:app/prompts/:id",
            put(upsert_prompt).delete(delete_prompt),
        )
        .route("/api/:app/prompts/:id/enable", post(enable_prompt))
        .route("/api/:app/prompt-content", get(prompt_content))
        // mcp (unified store)
        .route(
            "/api/mcp/servers",
            get(list_mcp_servers).put(upsert_mcp_server),
        )
        .route("/api/mcp/servers/:id", delete(delete_mcp_server))
        .route("/api/mcp/servers/:id/toggle", post(toggle_mcp_server))
        .route("/api/mcp/import", post(import_mcp))
        .route("/api/mcp/resync", post(resync_mcp))
        // skills (unified store)
        .route("/api/skills/installed", get(list_skills))
        .route("/api/skills/toggle", post(toggle_skill))
        .route("/api/skills/resync", post(resync_skills))
        // sessions
        .route("/api/sessions", get(list_sessions))
        .route("/api/sessions/messages", get(session_messages))
        // usage
        .route("/api/usage/summary", get(usage_summary))
        .route("/api/usage/by-app", get(usage_by_app))
        .with_state(state)
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
        "endpoints": [
            "/api/health", "/api/capabilities",
            "/api/:app/providers", "/api/:app/providers/current",
            "/api/:app/providers/switch", "/api/:app/providers/:id",
            "/api/:app/prompts", "/api/:app/prompts/:id",
            "/api/:app/prompts/:id/enable", "/api/:app/prompt-content",
            "/api/mcp/servers", "/api/mcp/servers/:id",
            "/api/mcp/servers/:id/toggle", "/api/mcp/import", "/api/mcp/resync",
            "/api/skills/installed", "/api/skills/toggle", "/api/skills/resync",
            "/api/sessions", "/api/sessions/messages",
            "/api/usage/summary", "/api/usage/by-app",
        ],
        "runtime_boundaries": "manages files on the server host only; no tray, no native file pickers, no auto-update",
    }))
}

// ---------- providers ----------

async fn list_providers(State(state): State<SharedState>, Path(app): Path<String>) -> ApiResult {
    let app_type = app_type!(&app);
    match ProviderService::list(&state, app_type) {
        Ok(map) => ok(map),
        Err(e) => fail(e.to_string()),
    }
}

async fn current_provider(
    State(state): State<SharedState>,
    Path(app): Path<String>,
) -> ApiResult {
    let app_type = app_type!(&app);
    match ProviderService::current(&state, app_type) {
        Ok(id) => ok(id),
        Err(e) => fail(e.to_string()),
    }
}

#[derive(Deserialize)]
struct SwitchReq {
    id: String,
}

async fn switch_provider(
    State(state): State<SharedState>,
    Path(app): Path<String>,
    Json(body): Json<SwitchReq>,
) -> ApiResult {
    let app_type = app_type!(&app);
    let id = body.id;
    match tokio::task::spawn_blocking(move || ProviderService::switch(&state, app_type, &id)).await
    {
        Ok(Ok(result)) => ok(result),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("switch task failed: {e}")),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddProviderReq {
    provider: Provider,
    add_to_live: Option<bool>,
    editor_save: Option<EditorSave>,
}

async fn add_provider(
    State(state): State<SharedState>,
    Path(app): Path<String>,
    Json(body): Json<AddProviderReq>,
) -> ApiResult {
    let app_type = app_type!(&app);
    let add_to_live = body.add_to_live.unwrap_or(true);
    match tokio::task::spawn_blocking(move || {
        ProviderService::add_from_editor(&state, app_type, body.provider, add_to_live, body.editor_save)
    })
    .await
    {
        Ok(Ok(v)) => ok(v),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("add task failed: {e}")),
    }
}

async fn delete_provider(
    State(state): State<SharedState>,
    Path((app, id)): Path<(String, String)>,
) -> ApiResult {
    let app_type = app_type!(&app);
    match tokio::task::spawn_blocking(move || ProviderService::delete(&state, app_type, &id)).await
    {
        Ok(Ok(())) => ok(true),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("delete task failed: {e}")),
    }
}

// ---------- prompts ----------

async fn list_prompts(State(state): State<SharedState>, Path(app): Path<String>) -> ApiResult {
    let app_type = app_type!(&app);
    match PromptService::get_prompts(&state, app_type) {
        Ok(map) => ok(map),
        Err(e) => fail(e.to_string()),
    }
}

#[derive(Deserialize)]
struct UpsertPromptReq {
    prompt: Prompt,
}

async fn upsert_prompt(
    State(state): State<SharedState>,
    Path((app, id)): Path<(String, String)>,
    Json(body): Json<UpsertPromptReq>,
) -> ApiResult {
    let app_type = app_type!(&app);
    match PromptService::upsert_prompt(&state, app_type, &id, body.prompt) {
        Ok(()) => ok(true),
        Err(e) => fail(e.to_string()),
    }
}

async fn delete_prompt(
    State(state): State<SharedState>,
    Path((app, id)): Path<(String, String)>,
) -> ApiResult {
    let app_type = app_type!(&app);
    match PromptService::delete_prompt(&state, app_type, &id) {
        Ok(()) => ok(true),
        Err(e) => fail(e.to_string()),
    }
}

async fn enable_prompt(
    State(state): State<SharedState>,
    Path((app, id)): Path<(String, String)>,
) -> ApiResult {
    let app_type = app_type!(&app);
    match PromptService::enable_prompt(&state, app_type, &id) {
        Ok(()) => ok(true),
        Err(e) => fail(e.to_string()),
    }
}

async fn prompt_content(State(_state): State<SharedState>, Path(app): Path<String>) -> ApiResult {
    let app_type = app_type!(&app);
    match PromptService::get_current_file_content(app_type) {
        Ok(content) => ok(content),
        Err(e) => fail(e.to_string()),
    }
}

// ---------- mcp ----------

async fn list_mcp_servers(State(state): State<SharedState>) -> ApiResult {
    match McpService::get_all_servers(&state) {
        Ok(map) => ok(map),
        Err(e) => fail(e.to_string()),
    }
}

#[derive(Deserialize)]
struct UpsertMcpReq {
    server: McpServer,
}

async fn upsert_mcp_server(
    State(state): State<SharedState>,
    Json(body): Json<UpsertMcpReq>,
) -> ApiResult {
    match McpService::upsert_server(&state, body.server) {
        Ok(()) => ok(true),
        Err(e) => fail(e.to_string()),
    }
}

async fn delete_mcp_server(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> ApiResult {
    match McpService::delete_server(&state, &id) {
        Ok(v) => ok(v),
        Err(e) => fail(e.to_string()),
    }
}

#[derive(Deserialize)]
struct ToggleMcpReq {
    app: String,
    enabled: bool,
}

async fn toggle_mcp_server(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<ToggleMcpReq>,
) -> ApiResult {
    let app_type = app_type!(&body.app);
    match McpService::toggle_app_locked(&state, &id, app_type, body.enabled).await {
        Ok(()) => ok(true),
        Err(e) => fail(e.to_string()),
    }
}

async fn import_mcp(State(state): State<SharedState>) -> ApiResult {
    match McpService::import_from_all_apps(&state) {
        Ok(n) => ok(n),
        Err(e) => fail(e.to_string()),
    }
}

#[derive(Deserialize)]
struct ResyncMcpReq {
    apps: Option<Vec<String>>,
}

async fn resync_mcp(State(state): State<SharedState>, Json(body): Json<ResyncMcpReq>) -> ApiResult {
    let targets = match McpService::resync_targets(body.apps.as_deref()) {
        Ok(t) => t,
        Err(e) => return fail(e.to_string()),
    };
    let mut outcomes = Vec::with_capacity(targets.len());
    for app in targets {
        let _guard = state.proxy_service.lock_switch_for_app(app.as_str()).await;
        outcomes.push(McpService::resync_app(&state, &app));
    }
    ok(outcomes)
}

// ---------- skills ----------

async fn list_skills(State(state): State<SharedState>) -> ApiResult {
    match SkillService::get_all_installed(&state.db) {
        Ok(list) => ok(list),
        Err(e) => fail(e.to_string()),
    }
}

#[derive(Deserialize)]
struct ToggleSkillReq {
    id: String,
    app: String,
    enabled: bool,
}

async fn toggle_skill(
    State(state): State<SharedState>,
    Json(body): Json<ToggleSkillReq>,
) -> ApiResult {
    let app_type = app_type!(&body.app);
    match SkillService::toggle_app(&state.db, &body.id, &app_type, body.enabled) {
        Ok(()) => ok(true),
        Err(e) => fail(e.to_string()),
    }
}

async fn resync_skills(State(state): State<SharedState>) -> ApiResult {
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || SkillService::resync_all_apps(&db)).await {
        Ok(outcomes) => ok(outcomes),
        Err(e) => fail(format!("resync task failed: {e}")),
    }
}

// ---------- sessions ----------

async fn list_sessions() -> ApiResult {
    match tokio::task::spawn_blocking(session_manager::scan_sessions).await {
        Ok(list) => ok(list),
        Err(e) => fail(format!("scan sessions failed: {e}")),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionMessagesQuery {
    provider_id: String,
    source_path: String,
}

async fn session_messages(Query(q): Query<SessionMessagesQuery>) -> ApiResult {
    let provider_id = q.provider_id;
    let source_path = q.source_path;
    match tokio::task::spawn_blocking(move || {
        session_manager::load_transcript(&provider_id, &source_path)
            .map(|loaded| loaded.transcript.messages.clone())
    })
    .await
    {
        Ok(Ok(messages)) => ok(messages),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("load transcript failed: {e}")),
    }
}

// ---------- usage ----------

#[derive(Deserialize)]
struct UsageQuery {
    start: Option<i64>,
    end: Option<i64>,
    app: Option<String>,
    provider: Option<String>,
    model: Option<String>,
}

async fn usage_summary(
    State(state): State<SharedState>,
    Query(q): Query<UsageQuery>,
) -> ApiResult {
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || {
        db.get_usage_summary(
            q.start,
            q.end,
            q.app.as_deref(),
            q.provider.as_deref(),
            q.model.as_deref(),
        )
    })
    .await
    {
        Ok(Ok(summary)) => ok(summary),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("usage query failed: {e}")),
    }
}

async fn usage_by_app(
    State(state): State<SharedState>,
    Query(q): Query<UsageQuery>,
) -> ApiResult {
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || {
        db.get_usage_summary_by_app(q.start, q.end, q.provider.as_deref(), q.model.as_deref())
    })
    .await
    {
        Ok(Ok(rows)) => ok(rows),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("usage query failed: {e}")),
    }
}
