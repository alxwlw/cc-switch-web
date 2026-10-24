//! Headless HTTP API for the cc-switch-web fork (Tailscale-only, no in-app auth).
//!
//! Thin Axum handlers over the existing handle-free service layer — the same
//! `*Service::*(state: &AppState, …)` functions the Tauri commands wrap.
//! Blocking ops run in `spawn_blocking`, mirroring `commands::*`.

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
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::provider::Provider;
use crate::services::prompt::PromptService;
use crate::services::provider::{EditorSave, ProviderService, ProviderSortUpdate};
use crate::services::skill::SkillService;
use crate::services::usage_stats::LogFilters;
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
        .route("/favicon.ico", get(favicon))
        .route("/api/capabilities", get(capabilities))
        // providers (per app)
        .route(
            "/api/:app/providers",
            get(list_providers).post(add_provider).put(update_provider),
        )
        .route("/api/:app/providers/current", get(current_provider))
        .route("/api/:app/providers/switch", post(switch_provider))
        .route("/api/:app/providers/sort", post(sort_providers))
        .route(
            "/api/:app/providers/import-default",
            post(import_default_providers),
        )
        .route("/api/:app/providers/live-add", post(live_add_provider))
        .route(
            "/api/:app/providers/live-remove",
            post(live_remove_provider),
        )
        .route("/api/:app/providers/:id", delete(delete_provider))
        // prompts (per app)
        .route("/api/:app/prompts", get(list_prompts))
        .route(
            "/api/:app/prompts/:id",
            put(upsert_prompt).delete(delete_prompt),
        )
        .route("/api/:app/prompts/:id/enable", post(enable_prompt))
        .route("/api/:app/prompt-content", get(prompt_content))
        .route("/api/:app/prompt-location", get(prompt_location))
        // mcp (unified store)
        .route(
            "/api/mcp/servers",
            get(list_mcp_servers).put(upsert_mcp_server),
        )
        .route("/api/mcp/servers/:id", delete(delete_mcp_server))
        .route("/api/mcp/servers/:id/toggle", post(toggle_mcp_server))
        .route("/api/mcp/config", get(mcp_config))
        .route("/api/mcp/validate", post(validate_mcp_command))
        .route("/api/mcp/import", post(import_mcp))
        .route("/api/mcp/resync", post(resync_mcp))
        // skills (unified store)
        .route("/api/skills/installed", get(list_skills))
        .route("/api/skills/toggle", post(toggle_skill))
        .route("/api/skills/resync", post(resync_skills))
        .route("/api/skills/uninstall", post(uninstall_skill))
        // sessions
        .route(
            "/api/sessions",
            get(list_sessions).delete(delete_sessions),
        )
        .route("/api/sessions/one", delete(delete_session))
        .route("/api/sessions/messages", get(session_messages))
        .route("/api/sessions/block", post(session_block))
        // usage
        .route("/api/usage/summary", get(usage_summary))
        .route("/api/usage/by-app", get(usage_by_app))
        .route("/api/usage/session", get(session_usage_summary))
        .route("/api/usage/trends", get(usage_trends))
        .route("/api/usage/first-date", get(usage_first_date))
        .route("/api/usage/provider-stats", get(provider_stats))
        .route("/api/usage/model-stats", get(model_stats))
        .route("/api/usage/request-logs", post(request_logs))
        .route("/api/usage/request-detail/:id", get(request_detail))
        .route("/api/usage/model-pricing", get(model_pricing))
        .route("/api/usage/sync", post(sync_session_usage))
        // settings & environment
        .route(
            "/api/settings",
            get(get_settings).put(save_settings),
        )
        .route("/api/paths", get(paths))
        .route("/api/env/tools", post(tool_versions))
        .route("/api/env/conflicts", get(env_conflicts))
        .route("/api/migrations/status", get(migrations_status))
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
        "runtime_boundaries": "manages files on the server host only; no tray, no native file pickers, no auto-update, no file dialogs, no terminal launch",
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
        ProviderService::add_from_editor(
            &state,
            app_type,
            body.provider,
            add_to_live,
            body.editor_save,
        )
    })
    .await
    {
        Ok(Ok(v)) => ok(v),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("add task failed: {e}")),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProviderReq {
    provider: Provider,
    original_id: Option<String>,
    editor_save: Option<EditorSave>,
}

async fn update_provider(
    State(state): State<SharedState>,
    Path(app): Path<String>,
    Json(body): Json<UpdateProviderReq>,
) -> ApiResult {
    let app_type = app_type!(&app);
    match tokio::task::spawn_blocking(move || {
        ProviderService::update_from_editor(
            &state,
            app_type,
            body.original_id.as_deref(),
            body.provider,
            body.editor_save,
        )
    })
    .await
    {
        Ok(Ok(v)) => ok(v),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("update task failed: {e}")),
    }
}

#[derive(Deserialize)]
struct SortProvidersReq {
    updates: Vec<ProviderSortUpdate>,
}

async fn sort_providers(
    State(state): State<SharedState>,
    Path(app): Path<String>,
    Json(body): Json<SortProvidersReq>,
) -> ApiResult {
    let app_type = app_type!(&app);
    match tokio::task::spawn_blocking(move || {
        ProviderService::update_sort_order(&state, app_type, body.updates)
    })
    .await
    {
        Ok(Ok(v)) => ok(v),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("sort task failed: {e}")),
    }
}

async fn import_default_providers(
    State(state): State<SharedState>,
    Path(app): Path<String>,
) -> ApiResult {
    let app_type = app_type!(&app);
    match tokio::task::spawn_blocking(move || {
        ProviderService::import_default_config(&state, app_type)
    })
    .await
    {
        Ok(Ok(v)) => ok(v),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("import task failed: {e}")),
    }
}

#[derive(Deserialize)]
struct LiveProviderReq {
    id: String,
}

async fn live_add_provider(
    State(state): State<SharedState>,
    Path(app): Path<String>,
    Json(body): Json<LiveProviderReq>,
) -> ApiResult {
    let app_type = app_type!(&app);
    match ProviderService::add_to_live_config(&state, app_type, &body.id) {
        Ok(()) => ok(true),
        Err(e) => fail(e.to_string()),
    }
}

async fn live_remove_provider(
    State(state): State<SharedState>,
    Path(app): Path<String>,
    Json(body): Json<LiveProviderReq>,
) -> ApiResult {
    let app_type = app_type!(&app);
    match ProviderService::remove_from_live_config(&state, app_type, &body.id) {
        Ok(()) => ok(true),
        Err(e) => fail(e.to_string()),
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

async fn prompt_location(State(_state): State<SharedState>, Path(app): Path<String>) -> ApiResult {
    let app_type = app_type!(&app);
    match crate::prompt_files::prompt_file_path(&app_type) {
        Ok(path) => ok(json!({
            "path": path.to_string_lossy(),
            "displayPath": crate::prompt_files::display_path(&path),
        })),
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

#[derive(Deserialize)]
struct McpConfigQuery {
    app: String,
}

async fn mcp_config(State(state): State<SharedState>, Query(q): Query<McpConfigQuery>) -> ApiResult {
    let app_type = app_type!(&q.app);
    let config_path = crate::config::get_app_config_path()
        .to_string_lossy()
        .to_string();
    // NB: get_servers is deprecated upstream in favor of get_all_servers,
    // but it is the only per-app view; kept like the compat Tauri command.
    #[allow(deprecated)]
    let servers = McpService::get_servers(&state, app_type);
    match servers {
        Ok(servers) => ok(json!({ "configPath": config_path, "servers": servers })),
        Err(e) => fail(e.to_string()),
    }
}

#[derive(Deserialize)]
struct ValidateMcpReq {
    cmd: String,
}

async fn validate_mcp_command(Json(body): Json<ValidateMcpReq>) -> ApiResult {
    match crate::claude_mcp::validate_command_in_path(&body.cmd) {
        Ok(v) => ok(v),
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

#[derive(Deserialize)]
struct UninstallSkillReq {
    id: String,
}

async fn uninstall_skill(
    State(state): State<SharedState>,
    Json(body): Json<UninstallSkillReq>,
) -> ApiResult {
    match SkillService::uninstall(&state.db, &body.id) {
        Ok(result) => ok(result),
        Err(e) => fail(e.to_string()),
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
struct DeleteOneSessionReq {
    provider_id: String,
    session_id: String,
    source_path: String,
}

async fn delete_session(Json(body): Json<DeleteOneSessionReq>) -> ApiResult {
    let provider_id = body.provider_id;
    let session_id = body.session_id;
    let source_path = body.source_path;
    match tokio::task::spawn_blocking(move || {
        session_manager::delete_session(&provider_id, &session_id, &source_path)
    })
    .await
    {
        Ok(Ok(v)) => ok(v),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("delete session failed: {e}")),
    }
}

#[derive(Deserialize)]
struct DeleteSessionsReq {
    items: Vec<session_manager::DeleteSessionRequest>,
}

async fn delete_sessions(Json(body): Json<DeleteSessionsReq>) -> ApiResult {
    let items = body.items;
    match tokio::task::spawn_blocking(move || session_manager::delete_sessions(&items)).await {
        Ok(outcomes) => ok(outcomes),
        Err(e) => fail(format!("delete sessions failed: {e}")),
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionBlockReq {
    provider_id: String,
    source_path: String,
    content_ref: session_manager::model::ContentRef,
    offset: Option<u32>,
    limit: Option<u32>,
}

async fn session_block(Json(body): Json<SessionBlockReq>) -> ApiResult {
    let SessionBlockReq {
        provider_id,
        source_path,
        content_ref,
        offset,
        limit,
    } = body;
    match tokio::task::spawn_blocking(move || {
        let source =
            session_manager::content::validate_source(&provider_id, &source_path)?;
        let full = session_manager::content::resolve_content_ref(&source, &content_ref)?;
        Ok::<_, String>(session_manager::content::paginate(&full, offset, limit))
    })
    .await
    {
        Ok(Ok(block)) => ok(block),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("load block failed: {e}")),
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionUsageQuery {
    app_type: String,
    session_id: String,
}

async fn session_usage_summary(
    State(state): State<SharedState>,
    Query(q): Query<SessionUsageQuery>,
) -> ApiResult {
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || {
        db.get_session_usage_summary(&q.app_type, &q.session_id)
    })
    .await
    {
        Ok(Ok(summary)) => ok(summary),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("usage query failed: {e}")),
    }
}

async fn usage_trends(State(state): State<SharedState>, Query(q): Query<UsageQuery>) -> ApiResult {
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || {
        db.get_daily_trends(
            q.start,
            q.end,
            q.app.as_deref(),
            q.provider.as_deref(),
            q.model.as_deref(),
        )
    })
    .await
    {
        Ok(Ok(rows)) => ok(rows),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("usage query failed: {e}")),
    }
}

async fn usage_first_date(
    State(state): State<SharedState>,
    Query(q): Query<UsageQuery>,
) -> ApiResult {
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || {
        db.get_first_usage_date(q.app.as_deref(), q.provider.as_deref(), q.model.as_deref())
    })
    .await
    {
        Ok(Ok(date)) => ok(date),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("usage query failed: {e}")),
    }
}

async fn provider_stats(
    State(state): State<SharedState>,
    Query(q): Query<UsageQuery>,
) -> ApiResult {
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || {
        db.get_provider_stats(
            q.start,
            q.end,
            q.app.as_deref(),
            q.provider.as_deref(),
            q.model.as_deref(),
        )
    })
    .await
    {
        Ok(Ok(rows)) => ok(rows),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("usage query failed: {e}")),
    }
}

async fn model_stats(State(state): State<SharedState>, Query(q): Query<UsageQuery>) -> ApiResult {
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || {
        db.get_model_stats(
            q.start,
            q.end,
            q.app.as_deref(),
            q.provider.as_deref(),
            q.model.as_deref(),
        )
    })
    .await
    {
        Ok(Ok(rows)) => ok(rows),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("usage query failed: {e}")),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestLogsReq {
    filters: LogFilters,
    page: u32,
    page_size: u32,
}

async fn request_logs(State(state): State<SharedState>, Json(body): Json<RequestLogsReq>) -> ApiResult {
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || {
        db.get_request_logs(&body.filters, body.page, body.page_size)
    })
    .await
    {
        Ok(Ok(logs)) => ok(logs),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("usage query failed: {e}")),
    }
}

async fn request_detail(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> ApiResult {
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || db.get_request_detail(&id)).await {
        Ok(Ok(detail)) => ok(detail),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("usage query failed: {e}")),
    }
}

async fn sync_session_usage(State(state): State<SharedState>) -> ApiResult {
    let db = state.db.clone();
    let _guard = crate::services::session_usage::session_sync_mutex()
        .lock()
        .await;
    match tokio::task::spawn_blocking(move || {
        crate::services::session_usage::sync_all_unlocked(&db)
    })
    .await
    {
        Ok(result) => ok(result),
        Err(e) => fail(format!("sync task failed: {e}")),
    }
}

async fn model_pricing(State(state): State<SharedState>) -> ApiResult {    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || {
        db.ensure_model_pricing_seeded()?;
        crate::services::model_pricing::sync_local_model_pricing(&db)?;
        let conn = crate::database::lock_conn!(db.conn);
        let table_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='model_pricing'",
                [],
                |row| row.get::<_, i64>(0).map(|count| count > 0),
            )
            .unwrap_or(false);
        if !table_exists {
            return Ok(Vec::new());
        }
        let mut stmt = conn.prepare(
            "SELECT model_id, display_name, input_cost_per_million, output_cost_per_million,
                    cache_read_cost_per_million, cache_creation_cost_per_million
             FROM model_pricing
             ORDER BY display_name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(crate::services::model_pricing::ModelPricingInfo {
                model_id: row.get(0)?,
                display_name: row.get(1)?,
                input_cost_per_million: row.get(2)?,
                output_cost_per_million: row.get(3)?,
                cache_read_cost_per_million: row.get(4)?,
                cache_creation_cost_per_million: row.get(5)?,
            })
        })?;
        let mut pricing = Vec::new();
        for row in rows {
            pricing.push(row?);
        }
        Ok::<_, crate::error::AppError>(pricing)
    })
    .await
    {
        Ok(Ok(list)) => ok(list),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("pricing query failed: {e}")),
    }
}

// ---------- settings & environment ----------

async fn get_settings() -> ApiResult {
    ok(crate::settings::get_settings_for_frontend())
}

async fn save_settings(
    State(state): State<SharedState>,
    Json(body): Json<crate::settings::AppSettings>,
) -> ApiResult {
    let existing = crate::settings::get_settings();
    let merged = crate::commands::settings::merge_settings_for_save(body, &existing);
    let stack_changed = merged.codex_stack_classic_tools != existing.codex_stack_classic_tools
        || merged.codex_stack_subagent_model != existing.codex_stack_subagent_model
        || merged.codex_stack_subagent_reasoning_effort
            != existing.codex_stack_subagent_reasoning_effort
        || merged.codex_stack_auto_review_model != existing.codex_stack_auto_review_model;
    let unify_changed =
        merged.unify_codex_session_history != existing.unify_codex_session_history;
    if let Err(e) = crate::settings::update_settings(merged) {
        return fail(e.to_string());
    }
    // NOTE: desktop tray restyle skipped headless (no AppHandle).
    if stack_changed {
        if let Err(err) =
            crate::mode::controller::resync_codex_stack_catalog(&state).await
        {
            log::warn!("Codex stack resync failed after settings save: {err}");
            return fail(format!("settings saved but Codex resync failed: {err}"));
        }
    }
    if unify_changed {
        if let Err(err) =
            crate::services::provider::reapply_current_codex_official_live(&state)
        {
            log::warn!("Codex live reapply failed after settings save: {err}");
            return fail(format!("settings saved but Codex live reapply failed: {err}"));
        }
    }
    ok(true)
}

async fn paths() -> ApiResult {
    ok(json!({
        "configDir": crate::config::get_app_config_dir().to_string_lossy(),
        "configPath": crate::config::get_app_config_path().to_string_lossy(),
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolVersionsReq {
    tools: Option<Vec<String>>,
    wsl_shell_by_tool: Option<
        std::collections::HashMap<String, crate::commands::misc::WslShellPreferenceInput>,
    >,
}

async fn tool_versions(Json(body): Json<ToolVersionsReq>) -> ApiResult {
    match crate::commands::misc::get_tool_versions(body.tools, body.wsl_shell_by_tool).await {
        Ok(versions) => ok(versions),
        Err(e) => fail(e.to_string()),
    }
}

async fn favicon() -> impl axum::response::IntoResponse {
    (StatusCode::NO_CONTENT, "")
}

/// One-shot startup migration flags (same semantics as the desktop commands:
/// each flag is reported once, then cleared).
async fn migrations_status() -> ApiResult {
    ok(json!({
        "migration": crate::init_status::take_migration_success(),
        "skills": crate::init_status::take_skills_migration_result(),
    }))
}

#[derive(Deserialize)]
struct EnvConflictsQuery {
    app: String,
}

async fn env_conflicts(Query(q): Query<EnvConflictsQuery>) -> ApiResult {
    match crate::services::env_checker::check_env_conflicts(&q.app) {
        Ok(conflicts) => ok(conflicts),
        Err(e) => fail(e.to_string()),
    }
}
