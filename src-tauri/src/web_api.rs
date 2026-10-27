//! Headless HTTP API for the cc-switch-web fork (Tailscale-only, no in-app auth).
//!
//! Thin Axum handlers over the existing handle-free service layer — the same
//! `*Service::*(state: &AppState, …)` functions the Tauri commands wrap.
//! Blocking ops run in `spawn_blocking`, mirroring `commands::*`.

use std::str::FromStr;
use std::sync::Arc;

use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    routing::{any, delete, get, patch, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::app_config::{AppType, McpServer};
use crate::commands as universal_provider_emit_cmds;
use crate::database::Database;
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::provider::{Provider, UniversalProvider};
use crate::services::profile::{ProfileDto, ProfileScope, ProfileService};
use crate::services::prompt::PromptService;
use crate::services::provider::icon;
use crate::services::provider::{EditorSave, ProviderService, ProviderSortUpdate};
use crate::services::skill::SkillService;
use crate::services::usage_stats::LogFilters;
use crate::services::{McpService, OmoService};
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

/// Like `blocking_api` for services returning `AppError`: `Conflict` (a file
/// changed outside CC Switch) maps to 409, everything else to 500. The body is
/// `e.to_string()`, the same text the desktop command returns.
async fn blocking_api_conflict<T, F>(what: &'static str, f: F) -> ApiResult
where
    T: serde::Serialize + Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(v)) => ok(v),
        Ok(Err(e @ AppError::Conflict(_))) => (
            StatusCode::CONFLICT,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("{what} task failed: {e}")),
    }
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
        .route("/api/events", get(events_stream))
        .route("/favicon.ico", get(favicon))
        .route("/api/capabilities", get(capabilities))
        .route("/api/init-status", get(init_status_route))
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
        .route(
            "/api/:app/providers/editor-view",
            post(provider_editor_view),
        )
        .route("/api/:app/providers/live-add", post(live_add_provider))
        .route(
            "/api/:app/providers/live-remove",
            post(live_remove_provider),
        )
        .route("/api/:app/providers/:id", delete(delete_provider))
        // endpoint speed test, custom endpoints, live settings
        .route("/api/:app/live-settings", get(live_settings))
        .route("/api/endpoints/speedtest", post(speedtest_endpoints))
        .route(
            "/api/:app/providers/:id/endpoints",
            get(list_endpoints)
                .post(add_endpoint)
                .delete(remove_endpoint),
        )
        .route(
            "/api/:app/providers/:id/endpoints/last-used",
            post(endpoint_last_used),
        )
        // proxy rectifier / optimizer / log settings, server log file
        .route(
            "/api/settings/rectifier",
            get(get_rectifier_config).put(set_rectifier_config),
        )
        .route(
            "/api/settings/optimizer",
            get(get_optimizer_config).put(set_optimizer_config),
        )
        .route("/api/settings/log", get(get_log_config).put(set_log_config))
        .route("/api/logs/tail", get(logs_tail))
        .route("/api/logs/download", get(logs_download))
        // universal providers (cross-app)
        .route(
            "/api/universal-providers",
            get(list_universal_providers).put(upsert_universal_provider),
        )
        .route(
            "/api/universal-providers/:id",
            get(get_universal_provider).delete(delete_universal_provider),
        )
        .route(
            "/api/universal-providers/:id/sync",
            post(sync_universal_provider),
        )
        // project profiles (cross-app; `clear-current` is a static segment and wins over `:id`)
        .route("/api/profiles", get(list_profiles).post(create_profile))
        .route("/api/profiles/clear-current", post(clear_current_profile))
        .route(
            "/api/profiles/:id",
            axum::routing::patch(update_profile).delete(delete_profile),
        )
        .route("/api/profiles/:id/apply", post(apply_profile))
        // prompts (per app)
        .route("/api/:app/prompts", get(list_prompts))
        .route(
            "/api/:app/prompts/:id",
            put(upsert_prompt).delete(delete_prompt),
        )
        .route("/api/:app/prompts/:id/enable", post(enable_prompt))
        .route(
            "/api/:app/prompts/import-from-file",
            post(import_prompt_from_file),
        )
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
        .route("/api/sessions", get(list_sessions).delete(delete_sessions))
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
        .route("/api/usage/last-sync", get(session_usage_last_sync))
        .route("/api/usage/rebuild-codex", post(rebuild_codex_usage))
        .route("/api/usage/data-sources", get(usage_data_sources))
        .route(
            "/api/usage/model-pricing/batch",
            post(update_model_pricing_batch),
        )
        .route(
            "/api/usage/model-pricing/:model_id",
            put(update_model_pricing).delete(delete_model_pricing),
        )
        .route(
            "/api/usage/models-dev/config",
            get(models_dev_config).put(save_models_dev_config),
        )
        .route(
            "/api/usage/models-dev/result",
            post(record_models_dev_result),
        )
        .route(
            "/api/:app/pricing-model-source",
            get(get_pricing_model_source).put(set_pricing_model_source),
        )
        // settings & environment
        .route("/api/settings", get(get_settings).put(save_settings))
        .route("/api/paths", get(paths))
        .route("/api/env/tools", post(tool_versions))
        .route("/api/env/conflicts", get(env_conflicts))
        .route("/api/env/conflicts/cleanup", post(env_cleanup))
        .route("/api/env/restore", post(env_restore))
        // OMO / OMO Slim live config
        .route("/api/omo/:variant/local-file", get(omo_local_file))
        .route(
            "/api/omo/:variant/current-provider",
            get(omo_current_provider),
        )
        .route("/api/omo/:variant/disable", post(omo_disable))
        .route("/api/migrations/status", get(migrations_status))
        // custom icon library
        .route(
            "/api/custom-icons",
            get(list_custom_icons).post(add_custom_icon),
        )
        .route(
            "/api/custom-icons/:id",
            put(rename_custom_icon).delete(delete_custom_icon),
        )
        // app-specific settings (WEB-20)
        .route(
            "/api/codex/unify-history-backup",
            get(codex_unify_history_backup),
        )
        .route(
            "/api/codex/forces-multi-agent-v2",
            get(codex_forces_multi_agent_v2),
        )
        .route(
            "/api/codex/session-compression",
            get(codex_session_compression_get).put(codex_session_compression_set),
        )
        .route(
            "/api/codex/sessions-disk-usage",
            get(codex_sessions_disk_usage),
        )
        .route(
            "/api/codex/restore-unified-history",
            post(codex_restore_unified_history),
        )
        .route("/api/claude/plugin-config", post(claude_plugin_config))
        .route(
            "/api/claude/onboarding-skip",
            post(claude_onboarding_skip_set).delete(claude_onboarding_skip_clear),
        )
        // Hermes (WEB-16)
        .route("/api/hermes/import-live", post(hermes_import_live))
        .route(
            "/api/hermes/live-provider-ids",
            get(hermes_live_provider_ids),
        )
        .route("/api/hermes/model-config", get(hermes_model_config))
        .route(
            "/api/hermes/memory/:kind",
            get(hermes_memory_get).put(hermes_memory_set),
        )
        .route("/api/hermes/memory-limits", get(hermes_memory_limits))
        .route(
            "/api/hermes/memory/:kind/enabled",
            put(hermes_memory_enabled),
        )
        .route("/api/hermes/web-ui", get(hermes_web_ui))
        // OpenClaw (WEB-15)
        .route("/api/openclaw/import-live", post(openclaw_import_live))
        .route(
            "/api/openclaw/live-provider-ids",
            get(openclaw_live_provider_ids),
        )
        .route(
            "/api/openclaw/live-provider/:id",
            get(openclaw_live_provider),
        )
        .route("/api/openclaw/health", get(openclaw_health))
        .route(
            "/api/openclaw/default-model",
            get(openclaw_default_model_get).put(openclaw_default_model_set),
        )
        .route(
            "/api/openclaw/model-catalog",
            get(openclaw_model_catalog_get).put(openclaw_model_catalog_set),
        )
        .route(
            "/api/openclaw/agents-defaults",
            get(openclaw_agents_defaults_get).put(openclaw_agents_defaults_set),
        )
        .route(
            "/api/openclaw/env",
            get(openclaw_env_get).put(openclaw_env_set),
        )
        .route(
            "/api/openclaw/tools",
            get(openclaw_tools_get).put(openclaw_tools_set),
        )
        .route(
            "/api/openclaw/workspace/:filename",
            get(openclaw_workspace_get).put(openclaw_workspace_set),
        )
        .route(
            "/api/openclaw/workspace-paths",
            get(openclaw_workspace_paths),
        )
        .route(
            "/api/openclaw/daily-memory",
            get(openclaw_daily_memory_list),
        )
        .route(
            "/api/openclaw/daily-memory/search",
            get(openclaw_daily_memory_search),
        )
        .route(
            "/api/openclaw/daily-memory/:filename",
            get(openclaw_daily_memory_get)
                .put(openclaw_daily_memory_set)
                .delete(openclaw_daily_memory_delete),
        )
        // Pi / Oh My Pi (WEB-17)
        .route("/api/:app/current-state", get(pi_current_state))
        .route(
            "/api/:app/providers/:id/usage-script",
            put(pi_usage_script_set),
        )
        .route("/api/pi/session-discovery", get(pi_session_discovery))
        .route(
            "/api/pi/prompt-files/:kind",
            get(pi_prompt_file_get)
                .put(pi_prompt_file_replace)
                .delete(pi_prompt_file_delete),
        )
        .route("/api/pi/prompt-templates", get(pi_prompt_templates_list))
        .route(
            "/api/pi/prompt-templates/:slug",
            put(pi_prompt_template_upsert).delete(pi_prompt_template_delete),
        )
        // DB backups (WEB-10)
        .route("/api/backups", get(backups_list).post(backups_create))
        .route(
            "/api/backups/:filename",
            patch(backup_rename).delete(backup_delete),
        )
        .route("/api/backups/:filename/restore", post(backup_restore))
        .route("/api/backup-locations", get(backup_locations_list))
        .route("/api/backup-locations/:id", delete(backup_location_delete))
        // Config import/export + manual live sync (WEB-09)
        .route("/api/config/export", get(config_export))
        .route(
            "/api/config/import",
            post(config_import).layer(DefaultBodyLimit::max(CONFIG_IMPORT_BODY_LIMIT)),
        )
        .route("/api/providers/sync-live", post(providers_sync_live))
        .with_state(state)
}

/// Router served when startup bootstrap failed and there is no `AppState`
/// (WEB-32): `/api/init-status` reports the error, `/api/capabilities` keeps
/// working, every other `/api/*` answers 503 with the init error text. The
/// caller layers the static console on top so the frontend can render the
/// upgrade / config-error screen.
pub fn init_error_router() -> Router {
    Router::new()
        .route("/api/capabilities", get(capabilities))
        .route("/api/init-status", get(init_status_route))
        .route("/api/*rest", any(init_failed))
}

/// The desktop `get_init_error` payload (`InitErrorPayload`) or `null`.
async fn init_status_route() -> ApiResult {
    ok(crate::init_status::get_init_error())
}

async fn init_failed() -> ApiResult {
    let msg = crate::init_status::get_init_error()
        .map(|e| e.error)
        .unwrap_or_else(|| "startup initialization failed".to_string());
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "ok": false, "error": msg })),
    )
}

/// Keep-alive comment frame (`: keep-alive`) cadence: frequent enough that an
/// idle connection is never silent for long (intermediaries commonly drop idle
/// connections after tens of seconds), rare enough to be negligible traffic.
pub const SSE_KEEP_ALIVE: std::time::Duration = std::time::Duration::from_secs(15);

/// `GET /api/events` (WEB-29): Server-Sent Events view of the event hub.
///
/// Wire format: one *named* SSE event per bus event — `event: <name>` and a
/// single `data:` line holding the JSON payload (`null` for payload-less
/// events). No `id:` / `retry:` fields; nothing is replayed on reconnect
/// (events are notifications to refetch, not state). A subscriber that falls
/// behind the hub's buffer gets a `resync` frame (`data: null`) in place of
/// the lost events (client must invalidate everything) and keeps streaming;
/// the stream ends only when the hub is gone. Dropping the response (client
/// disconnect) drops the receiver, so nothing leaks.
async fn events_stream(
    State(state): State<SharedState>,
) -> axum::response::sse::Sse<
    impl futures::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
> {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use tokio::sync::broadcast::error::RecvError;

    // Subscribe before the response is returned so nothing emitted after the
    // request was accepted is missed.
    let mut rx = state.events.subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(ev) => match Event::default().event(ev.name.as_str()).json_data(&ev.payload) {
                    Ok(frame) => yield Ok(frame),
                    Err(e) => log::warn!("sse: dropping `{}`: {e}", ev.name),
                },
                Err(RecvError::Lagged(n)) => {
                    log::warn!("sse: subscriber lagged, skipped {n} events");
                    yield Ok(Event::default().event("resync").data("null"));
                }
                Err(RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::new().interval(SSE_KEEP_ALIVE).text("keep-alive"))
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

async fn current_provider(State(state): State<SharedState>, Path(app): Path<String>) -> ApiResult {
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
    match tokio::task::spawn_blocking(move || {
        let result = ProviderService::switch(&state, app_type.clone(), &id)?;
        let payload = crate::events::provider_switched_payload(&state, &app_type, &id);
        state.events.emit("provider-switched", payload);
        Ok::<_, crate::error::AppError>(result)
    })
    .await
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EditorViewReq {
    settings_config: Value,
    category: Option<String>,
    provider_id: Option<String>,
    meta: Option<crate::provider::ProviderMeta>,
}

async fn provider_editor_view(
    State(state): State<SharedState>,
    Path(app): Path<String>,
    Json(body): Json<EditorViewReq>,
) -> ApiResult {
    let app_type = app_type!(&app);
    match ProviderService::editor_view_with_meta(
        &state,
        app_type,
        &body.settings_config,
        body.category.as_deref(),
        body.provider_id.as_deref(),
        body.meta.as_ref(),
    ) {
        Ok(view) => ok(view),
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

// ---------- rectifier / optimizer / log settings, server log file ----------

#[derive(Deserialize)]
struct ConfigReq<T> {
    config: T,
}

macro_rules! db_call {
    ($state:expr, $what:literal, |$db:ident| $body:expr) => {
        match tokio::task::spawn_blocking(move || {
            let $db = &$state.db;
            $body
        })
        .await
        {
            Ok(Ok(v)) => ok(v),
            Ok(Err(e)) => fail(e.to_string()),
            Err(e) => fail(format!("{} task failed: {e}", $what)),
        }
    };
}

async fn get_rectifier_config(State(state): State<SharedState>) -> ApiResult {
    db_call!(state, "get rectifier config", |db| db
        .get_rectifier_config())
}

async fn set_rectifier_config(
    State(state): State<SharedState>,
    Json(body): Json<ConfigReq<crate::proxy::types::RectifierConfig>>,
) -> ApiResult {
    db_call!(state, "set rectifier config", |db| db
        .set_rectifier_config(&body.config)
        .map(|()| true))
}

async fn get_optimizer_config(State(state): State<SharedState>) -> ApiResult {
    db_call!(state, "get optimizer config", |db| db
        .get_optimizer_config())
}

async fn set_optimizer_config(
    State(state): State<SharedState>,
    Json(body): Json<ConfigReq<crate::proxy::types::OptimizerConfig>>,
) -> ApiResult {
    db_call!(state, "set optimizer config", |db| db
        .set_optimizer_config(&body.config)
        .map(|()| true))
}

async fn get_log_config(State(state): State<SharedState>) -> ApiResult {
    db_call!(state, "get log config", |db| db.get_log_config())
}

/// Same effect as the desktop command: saves the config and applies the level
/// to the running logger (`log::set_max_level`).
async fn set_log_config(
    State(state): State<SharedState>,
    Json(body): Json<ConfigReq<crate::proxy::types::LogConfig>>,
) -> ApiResult {
    db_call!(state, "set log config", |db| {
        crate::services::ConfigService::set_log_config(db, &body.config).map(|()| true)
    })
}

fn log_not_found(path: &std::path::Path) -> ApiResult {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "ok": false,
            "error": format!("log file not found: {}", path.display()),
            "path": path.display().to_string(),
        })),
    )
}

#[derive(Deserialize)]
struct TailQuery {
    lines: Option<String>,
}

/// `lines`: absent -> 500, above 5000 -> 5000, `0` or not a number -> 400.
/// A missing log file is 404 `{ok:false, error, path}`.
async fn logs_tail(Query(q): Query<TailQuery>) -> ApiResult {
    use crate::server_log::{log_file_path, tail_lines, TAIL_DEFAULT_LINES, TAIL_MAX_LINES};
    let n = match q.lines.as_deref() {
        None => TAIL_DEFAULT_LINES,
        Some(raw) => match raw.trim().parse::<usize>() {
            Ok(0) | Err(_) => {
                return bad_request(format!("lines must be a positive integer, got '{raw}'"))
            }
            Ok(n) => n.min(TAIL_MAX_LINES),
        },
    };
    let path = log_file_path();
    let p = path.clone();
    match tokio::task::spawn_blocking(move || tail_lines(&p, n)).await {
        Ok(Ok(lines)) => ok(json!({ "path": path.display().to_string(), "lines": lines })),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => log_not_found(&path),
        Ok(Err(e)) => fail(format!("read {}: {e}", path.display())),
        Err(e) => fail(format!("log tail task failed: {e}")),
    }
}

/// Streams the file (no full buffering) as an attachment.
async fn logs_download() -> axum::response::Response {
    use axum::response::IntoResponse;
    use tokio::io::AsyncReadExt;
    let path = crate::server_log::log_file_path();
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return log_not_found(&path).into_response()
        }
        Err(e) => return fail(format!("read {}: {e}", path.display())).into_response(),
    };
    let stream = futures::stream::unfold(file, |mut f| async move {
        let mut buf = vec![0u8; 64 * 1024];
        match f.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok::<_, std::io::Error>(axum::body::Bytes::from(buf)), f))
            }
            Err(e) => Some((Err(e), f)),
        }
    });
    (
        [
            (
                axum::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8",
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"cc-switch-web.log\"",
            ),
        ],
        axum::body::Body::from_stream(stream),
    )
        .into_response()
}

// ---------- endpoint speed test / custom endpoints ----------

async fn live_settings(State(_state): State<SharedState>, Path(app): Path<String>) -> ApiResult {
    let app_type = app_type!(&app);
    match tokio::task::spawn_blocking(move || ProviderService::read_live_settings(app_type)).await {
        Ok(Ok(v)) => ok(v),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("live settings task failed: {e}")),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpeedtestReq {
    urls: Vec<String>,
    timeout_secs: Option<u64>,
}

/// Latency is measured from the server, not the browser. The service probes all
/// URLs concurrently (`join_all`), so the call is bounded by the timeout, not their sum.
async fn speedtest_endpoints(Json(body): Json<SpeedtestReq>) -> ApiResult {
    match crate::services::SpeedtestService::test_endpoints(body.urls, body.timeout_secs).await {
        Ok(rows) => ok(rows),
        Err(e) => fail(e.to_string()),
    }
}

#[derive(Deserialize)]
struct UrlReq {
    url: String,
}

async fn list_endpoints(
    State(state): State<SharedState>,
    Path((app, id)): Path<(String, String)>,
) -> ApiResult {
    let app_type = app_type!(&app);
    match tokio::task::spawn_blocking(move || {
        ProviderService::get_custom_endpoints(&state, app_type, &id)
    })
    .await
    {
        Ok(Ok(v)) => ok(v),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("list endpoints task failed: {e}")),
    }
}

async fn add_endpoint(
    State(state): State<SharedState>,
    Path((app, id)): Path<(String, String)>,
    Json(body): Json<UrlReq>,
) -> ApiResult {
    let app_type = app_type!(&app);
    match tokio::task::spawn_blocking(move || {
        ProviderService::add_custom_endpoint(&state, app_type, &id, body.url)
    })
    .await
    {
        Ok(Ok(())) => ok(true),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("add endpoint task failed: {e}")),
    }
}

async fn remove_endpoint(
    State(state): State<SharedState>,
    Path((app, id)): Path<(String, String)>,
    Json(body): Json<UrlReq>,
) -> ApiResult {
    let app_type = app_type!(&app);
    match tokio::task::spawn_blocking(move || {
        ProviderService::remove_custom_endpoint(&state, app_type, &id, body.url)
    })
    .await
    {
        Ok(Ok(())) => ok(true),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("remove endpoint task failed: {e}")),
    }
}

async fn endpoint_last_used(
    State(state): State<SharedState>,
    Path((app, id)): Path<(String, String)>,
    Json(body): Json<UrlReq>,
) -> ApiResult {
    let app_type = app_type!(&app);
    match tokio::task::spawn_blocking(move || {
        ProviderService::update_endpoint_last_used(&state, app_type, &id, body.url)
    })
    .await
    {
        Ok(Ok(())) => ok(true),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("endpoint last-used task failed: {e}")),
    }
}

// ---------- universal providers ----------

async fn list_universal_providers(State(state): State<SharedState>) -> ApiResult {
    match tokio::task::spawn_blocking(move || ProviderService::list_universal(&state)).await {
        Ok(Ok(map)) => ok(map),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("list universal providers task failed: {e}")),
    }
}

async fn get_universal_provider(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> ApiResult {
    match tokio::task::spawn_blocking(move || ProviderService::get_universal(&state, &id)).await {
        Ok(Ok(p)) => ok(p),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("get universal provider task failed: {e}")),
    }
}

#[derive(Deserialize)]
struct UpsertUniversalReq {
    provider: UniversalProvider,
}

async fn upsert_universal_provider(
    State(state): State<SharedState>,
    Json(body): Json<UpsertUniversalReq>,
) -> ApiResult {
    match tokio::task::spawn_blocking(move || {
        universal_provider_emit_cmds::upsert_universal_provider_emit(&state, body.provider)
    })
    .await
    {
        Ok(Ok(v)) => ok(v),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("upsert universal provider task failed: {e}")),
    }
}

async fn delete_universal_provider(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> ApiResult {
    match tokio::task::spawn_blocking(move || {
        universal_provider_emit_cmds::delete_universal_provider_emit(&state, &id)
    })
    .await
    {
        Ok(Ok(v)) => ok(v),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("delete universal provider task failed: {e}")),
    }
}

/// A partial failure (DB saved, a live file not written) comes back as
/// `{ok:false,error}` with the service text verbatim; the child providers stay in the DB.
async fn sync_universal_provider(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> ApiResult {
    match tokio::task::spawn_blocking(move || {
        universal_provider_emit_cmds::sync_universal_provider_emit(&state, &id)
    })
    .await
    {
        Ok(Ok(v)) => ok(v),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("sync universal provider task failed: {e}")),
    }
}

// ---------- project profiles ----------

fn parse_scope(scope: &str) -> Result<ProfileScope, ApiResult> {
    ProfileScope::parse(scope).map_err(|e| bad_request(e.to_string()))
}

async fn list_profiles(State(state): State<SharedState>) -> ApiResult {
    match tokio::task::spawn_blocking(move || ProfileService::list_response(&state)).await {
        Ok(Ok(v)) => ok(v),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("list profiles task failed: {e}")),
    }
}

#[derive(Deserialize)]
struct CreateProfileReq {
    name: String,
    scope: String,
}

async fn create_profile(
    State(state): State<SharedState>,
    Json(body): Json<CreateProfileReq>,
) -> ApiResult {
    let scope = match parse_scope(&body.scope) {
        Ok(s) => s,
        Err(r) => return r,
    };
    match tokio::task::spawn_blocking(move || ProfileService::create(&state, &body.name, scope))
        .await
    {
        Ok(Ok(p)) => ok(ProfileDto::from(p)),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("create profile task failed: {e}")),
    }
}

#[derive(Deserialize)]
struct UpdateProfileReq {
    name: Option<String>,
    resnapshot: Option<bool>,
    scope: Option<String>,
}

async fn update_profile(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateProfileReq>,
) -> ApiResult {
    let scope = match body.scope.as_deref().map(parse_scope).transpose() {
        Ok(s) => s,
        Err(r) => return r,
    };
    let resnapshot = body.resnapshot.unwrap_or(false);
    match tokio::task::spawn_blocking(move || {
        ProfileService::update(&state, &id, body.name, resnapshot, scope)
    })
    .await
    {
        Ok(Ok(p)) => ok(ProfileDto::from(p)),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("update profile task failed: {e}")),
    }
}

async fn delete_profile(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult {
    match tokio::task::spawn_blocking(move || ProfileService::delete(&state, &id)).await {
        Ok(Ok(())) => ok(true),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("delete profile task failed: {e}")),
    }
}

#[derive(Deserialize)]
struct ScopeReq {
    scope: String,
}

async fn clear_current_profile(
    State(state): State<SharedState>,
    Json(body): Json<ScopeReq>,
) -> ApiResult {
    let scope = match parse_scope(&body.scope) {
        Ok(s) => s,
        Err(r) => return r,
    };
    match tokio::task::spawn_blocking(move || state.db.set_current_profile_id(scope.as_str(), None))
        .await
    {
        Ok(Ok(())) => ok(true),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("clear current profile task failed: {e}")),
    }
}

/// `ProviderService::switch` uses `block_on` internally, so apply must run on the
/// blocking pool. Events go out only after a successful apply, in desktop order.
async fn apply_profile(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<ScopeReq>,
) -> ApiResult {
    let scope = match parse_scope(&body.scope) {
        Ok(s) => s,
        Err(r) => return r,
    };
    match tokio::task::spawn_blocking(move || {
        let warnings = ProfileService::apply(&state, &id, scope)?;
        ProfileService::emit_apply_events(&state, &id, scope);
        Ok::<_, AppError>(warnings)
    })
    .await
    {
        Ok(Ok(w)) => ok(w),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("apply profile task failed: {e}")),
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

/// Imports the app's live prompt file (on the server host) as a disabled prompt;
/// returns the new prompt id, same as the desktop command.
async fn import_prompt_from_file(
    State(state): State<SharedState>,
    Path(app): Path<String>,
) -> ApiResult {
    let app_type = app_type!(&app);
    match tokio::task::spawn_blocking(move || PromptService::import_from_file(&state, app_type))
        .await
    {
        Ok(Ok(id)) => ok(id),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("import prompt task failed: {e}")),
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

async fn delete_mcp_server(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult {
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

async fn mcp_config(
    State(state): State<SharedState>,
    Query(q): Query<McpConfigQuery>,
) -> ApiResult {
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
        let source = session_manager::content::validate_source(&provider_id, &source_path)?;
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

async fn usage_summary(State(state): State<SharedState>, Query(q): Query<UsageQuery>) -> ApiResult {
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

async fn usage_by_app(State(state): State<SharedState>, Query(q): Query<UsageQuery>) -> ApiResult {
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

async fn request_logs(
    State(state): State<SharedState>,
    Json(body): Json<RequestLogsReq>,
) -> ApiResult {
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

async fn request_detail(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult {
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

/// Test seam: hold the session-sync mutex from an integration test (the `services`
/// module is private) to prove that usage routes queue behind it.
#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub async fn session_sync_lock_test_hook() -> tokio::sync::MutexGuard<'static, ()> {
    crate::services::session_usage::session_sync_mutex()
        .lock()
        .await
}

async fn session_usage_last_sync() -> ApiResult {
    ok(crate::services::session_usage::last_sync_completed_at())
}

/// Same order as the desktop command: mutex → backup → reset → sync → notify.
async fn rebuild_codex_usage(State(state): State<SharedState>) -> ApiResult {
    let db = state.db.clone();
    let _guard = crate::services::session_usage::session_sync_mutex()
        .lock()
        .await;
    match tokio::task::spawn_blocking(move || {
        crate::services::session_usage::rebuild_codex_usage_unlocked(&db)
    })
    .await
    {
        Ok(Ok(result)) => ok(result),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("Codex 用量重建任务失败: {e}")),
    }
}

async fn usage_data_sources(State(state): State<SharedState>) -> ApiResult {
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || {
        crate::services::session_usage::get_data_source_breakdown(&db)
    })
    .await
    {
        Ok(Ok(sources)) => ok(sources),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("usage query failed: {e}")),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelPricingReq {
    display_name: String,
    input_cost: String,
    output_cost: String,
    cache_read_cost: String,
    cache_creation_cost: String,
}

async fn update_model_pricing(
    State(state): State<SharedState>,
    Path(model_id): Path<String>,
    Json(body): Json<ModelPricingReq>,
) -> ApiResult {
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || {
        crate::services::model_pricing::update_model_pricing(
            &db,
            crate::services::model_pricing::ModelPricingInfo {
                model_id,
                display_name: body.display_name,
                input_cost_per_million: body.input_cost,
                output_cost_per_million: body.output_cost,
                cache_read_cost_per_million: body.cache_read_cost,
                cache_creation_cost_per_million: body.cache_creation_cost,
            },
        )
    })
    .await
    {
        Ok(Ok(_)) => ok(Value::Null),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("update pricing failed: {e}")),
    }
}

#[derive(Deserialize)]
struct ModelPricingBatchReq {
    entries: Vec<crate::services::model_pricing::ModelPricingInfo>,
}

async fn update_model_pricing_batch(
    State(state): State<SharedState>,
    Json(body): Json<ModelPricingBatchReq>,
) -> ApiResult {
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || {
        crate::services::model_pricing::update_model_pricing_batch(&db, body.entries)
    })
    .await
    {
        Ok(Ok(changed)) => ok(changed),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("update pricing failed: {e}")),
    }
}

async fn delete_model_pricing(
    State(state): State<SharedState>,
    Path(model_id): Path<String>,
) -> ApiResult {
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || {
        crate::services::model_pricing::delete_model_pricing(&db, &model_id)
    })
    .await
    {
        Ok(Ok(())) => ok(Value::Null),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("delete pricing failed: {e}")),
    }
}

async fn models_dev_config(State(state): State<SharedState>) -> ApiResult {
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || {
        crate::services::model_pricing::get_models_dev_sync_state(&db)
    })
    .await
    {
        Ok(Ok(sync_state)) => ok(sync_state),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("models.dev config failed: {e}")),
    }
}

async fn save_models_dev_config(
    State(state): State<SharedState>,
    Json(config): Json<crate::services::model_pricing::ModelsDevSyncConfig>,
) -> ApiResult {
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || {
        crate::services::model_pricing::save_models_dev_sync_config(&db, config)
    })
    .await
    {
        Ok(Ok(())) => ok(Value::Null),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("models.dev config failed: {e}")),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelsDevResultReq {
    synced_at: Option<i64>,
    error: Option<String>,
}

async fn record_models_dev_result(
    State(state): State<SharedState>,
    Json(body): Json<ModelsDevResultReq>,
) -> ApiResult {
    let db = state.db.clone();
    match tokio::task::spawn_blocking(move || {
        crate::services::model_pricing::record_models_dev_sync_result(
            &db,
            body.synced_at,
            body.error,
        )
    })
    .await
    {
        Ok(Ok(())) => ok(Value::Null),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("models.dev result failed: {e}")),
    }
}

async fn get_pricing_model_source(
    State(state): State<SharedState>,
    Path(app): Path<String>,
) -> ApiResult {
    let app_type = app_type!(&app);
    match state.db.get_pricing_model_source(app_type.as_str()).await {
        Ok(source) => ok(source),
        Err(e) => fail(e.to_string()),
    }
}

#[derive(Deserialize)]
struct PricingModelSourceReq {
    value: String,
}

async fn set_pricing_model_source(
    State(state): State<SharedState>,
    Path(app): Path<String>,
    Json(body): Json<PricingModelSourceReq>,
) -> ApiResult {
    let app_type = app_type!(&app);
    match state
        .db
        .set_pricing_model_source(app_type.as_str(), &body.value)
        .await
    {
        Ok(()) => ok(Value::Null),
        // The only client error the DAO raises: a value other than request/response.
        Err(
            e @ AppError::Localized {
                key: "error.invalidPricingMode",
                ..
            },
        ) => bad_request(e.to_string()),
        Err(e) => fail(e.to_string()),
    }
}

async fn model_pricing(State(state): State<SharedState>) -> ApiResult {
    let db = state.db.clone();
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

#[derive(Deserialize)]
struct SaveSettingsReq {
    settings: crate::settings::AppSettings,
}

async fn save_settings(
    State(state): State<SharedState>,
    Json(body): Json<SaveSettingsReq>,
) -> ApiResult {
    // NOTE: `{ settings: … }` wrapper mirrors the Tauri `save_settings` arg
    // shape the console sends. A bare AppSettings here would silently
    // deserialize to defaults (serde ignores unknown fields) and the save
    // would look successful while changing nothing.
    let body = body.settings;
    let existing = crate::settings::get_settings();
    let merged = crate::commands::settings::merge_settings_for_save(body, &existing);
    let stack_changed = merged.codex_stack_classic_tools != existing.codex_stack_classic_tools
        || merged.codex_stack_subagent_model != existing.codex_stack_subagent_model
        || merged.codex_stack_subagent_reasoning_effort
            != existing.codex_stack_subagent_reasoning_effort
        || merged.codex_stack_auto_review_model != existing.codex_stack_auto_review_model;
    let unify_changed = merged.unify_codex_session_history != existing.unify_codex_session_history;
    if let Err(e) = crate::settings::update_settings(merged) {
        return fail(e.to_string());
    }
    // NOTE: desktop tray restyle skipped headless (no AppHandle).
    if stack_changed {
        if let Err(err) = crate::mode::controller::resync_codex_stack_catalog(&state).await {
            log::warn!("Codex stack resync failed after settings save: {err}");
            return fail(format!("settings saved but Codex resync failed: {err}"));
        }
    }
    if unify_changed {
        if let Err(err) = crate::services::provider::reapply_current_codex_official_live(&state) {
            log::warn!("Codex live reapply failed after settings save: {err}");
            return fail(format!(
                "settings saved but Codex live reapply failed: {err}"
            ));
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
    wsl_shell_by_tool:
        Option<std::collections::HashMap<String, crate::commands::misc::WslShellPreferenceInput>>,
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

#[derive(Deserialize)]
struct EnvCleanupReq {
    conflicts: Vec<crate::services::env_checker::EnvConflict>,
}

/// Deletes the listed conflicts (after writing a backup) on the SERVER host.
async fn env_cleanup(Json(body): Json<EnvCleanupReq>) -> ApiResult {
    match tokio::task::spawn_blocking(move || {
        crate::services::env_manager::delete_env_vars(body.conflicts)
    })
    .await
    {
        Ok(Ok(info)) => ok(info),
        Ok(Err(e)) => fail(e),
        Err(e) => fail(format!("env cleanup task failed: {e}")),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvRestoreReq {
    backup_path: String,
}

async fn env_restore(Json(body): Json<EnvRestoreReq>) -> ApiResult {
    match tokio::task::spawn_blocking(move || {
        crate::services::env_manager::restore_from_backup(body.backup_path)
    })
    .await
    {
        Ok(Ok(())) => ok(serde_json::Value::Null),
        Ok(Err(e)) => fail(e),
        Err(e) => fail(format!("env restore task failed: {e}")),
    }
}

// ---------- OMO / OMO Slim live config ----------

fn omo_variant(name: &str) -> Result<&'static crate::services::omo::OmoVariant, ApiResult> {
    match name {
        "omo" => Ok(&crate::services::omo::STANDARD),
        "omo-slim" => Ok(&crate::services::omo::SLIM),
        other => Err(bad_request(format!("unknown OMO variant: {other}"))),
    }
}

async fn omo_local_file(Path(variant): Path<String>) -> ApiResult {
    let v = match omo_variant(&variant) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match tokio::task::spawn_blocking(move || OmoService::read_local_file(v)).await {
        Ok(Ok(data)) => ok(data),
        Ok(Err(e)) => fail(e),
        Err(e) => fail(format!("omo read task failed: {e}")),
    }
}

async fn omo_current_provider(
    State(state): State<SharedState>,
    Path(variant): Path<String>,
) -> ApiResult {
    let v = match omo_variant(&variant) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match tokio::task::spawn_blocking(move || OmoService::current_provider_id(&state.db, v)).await {
        Ok(Ok(id)) => ok(id),
        Ok(Err(e)) => fail(e),
        Err(e) => fail(format!("omo current-provider task failed: {e}")),
    }
}

async fn omo_disable(State(state): State<SharedState>, Path(variant): Path<String>) -> ApiResult {
    let v = match omo_variant(&variant) {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    match tokio::task::spawn_blocking(move || OmoService::disable_current(&state.db, v)).await {
        Ok(Ok(())) => ok(Value::Null),
        Ok(Err(e)) => fail(e),
        Err(e) => fail(format!("omo disable task failed: {e}")),
    }
}

// ---------- custom icon library ----------

async fn list_custom_icons(State(state): State<SharedState>) -> ApiResult {
    match tokio::task::spawn_blocking(move || icon::list_library_icons(&state.db)).await {
        Ok(Ok(icons)) => ok(icons),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("list custom icons task failed: {e}")),
    }
}

#[derive(Deserialize)]
struct AddCustomIconReq {
    name: String,
    data: String,
}

async fn add_custom_icon(
    State(state): State<SharedState>,
    Json(body): Json<AddCustomIconReq>,
) -> ApiResult {
    match tokio::task::spawn_blocking(move || {
        icon::add_library_icon(&state.db, &body.name, &body.data)
    })
    .await
    {
        Ok(Ok(result)) => ok(result),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("add custom icon task failed: {e}")),
    }
}

#[derive(Deserialize)]
struct RenameCustomIconReq {
    name: String,
}

async fn rename_custom_icon(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<RenameCustomIconReq>,
) -> ApiResult {
    match tokio::task::spawn_blocking(move || icon::rename_library_icon(&state.db, &id, &body.name))
        .await
    {
        Ok(Ok(entry)) => ok(entry),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("rename custom icon task failed: {e}")),
    }
}

async fn delete_custom_icon(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult {
    match tokio::task::spawn_blocking(move || icon::delete_library_icon(&state.db, &id)).await {
        Ok(Ok(())) => ok(true),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("delete custom icon task failed: {e}")),
    }
}

// ---------- app-specific settings: Codex / Claude (WEB-20) ----------

/// Runs a blocking, fallible closure and maps both failure layers to `fail`.
async fn blocking_api<T, E, F>(what: &'static str, f: F) -> ApiResult
where
    T: serde::Serialize + Send + 'static,
    E: ToString + Send + 'static,
    F: FnOnce() -> Result<T, E> + Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(v)) => ok(v),
        Ok(Err(e)) => fail(e.to_string()),
        Err(e) => fail(format!("{what} task failed: {e}")),
    }
}

async fn codex_unify_history_backup() -> ApiResult {
    blocking_api("codex unify backup probe", || {
        Ok::<_, String>(crate::codex_history_migration::has_codex_official_history_unify_backup())
    })
    .await
}

async fn codex_forces_multi_agent_v2() -> ApiResult {
    blocking_api("codex multi-agent probe", || {
        crate::codex_config::read_codex_config_text()
            .map(|text| crate::codex_config::codex_config_forces_multi_agent_v2(&text))
    })
    .await
}

async fn codex_session_compression_get() -> ApiResult {
    blocking_api(
        "codex session compression read",
        crate::services::codex_session_compression::is_enabled,
    )
    .await
}

#[derive(Deserialize)]
struct CodexSessionCompressionReq {
    enabled: bool,
}

async fn codex_session_compression_set(
    State(state): State<SharedState>,
    Json(body): Json<CodexSessionCompressionReq>,
) -> ApiResult {
    blocking_api("codex session compression write", move || {
        crate::services::codex_session_compression::set_enabled(&state.db, body.enabled)
    })
    .await
}

async fn codex_sessions_disk_usage() -> ApiResult {
    blocking_api("codex disk usage", || {
        Ok::<_, String>(crate::services::codex_session_compression::sessions_disk_usage())
    })
    .await
}

async fn codex_restore_unified_history() -> ApiResult {
    blocking_api("codex history restore", || {
        crate::codex_history_migration::restore_codex_official_history_from_backups().map(|o| {
            crate::commands::CodexUnifyHistoryRestoreResult {
                restored_jsonl_files: o.restored_jsonl_files,
                restored_state_rows: o.restored_state_rows,
                skipped_reason: o.skipped_reason,
            }
        })
    })
    .await
}

#[derive(Deserialize)]
struct ClaudePluginConfigReq {
    official: bool,
}

async fn claude_plugin_config(Json(body): Json<ClaudePluginConfigReq>) -> ApiResult {
    blocking_api("claude plugin config", move || {
        if body.official {
            crate::claude_plugin::clear_claude_config()
        } else {
            crate::claude_plugin::write_claude_config()
        }
    })
    .await
}

async fn claude_onboarding_skip_set() -> ApiResult {
    blocking_api(
        "claude onboarding skip",
        crate::claude_mcp::set_has_completed_onboarding,
    )
    .await
}

async fn claude_onboarding_skip_clear() -> ApiResult {
    blocking_api(
        "claude onboarding restore",
        crate::claude_mcp::clear_has_completed_onboarding,
    )
    .await
}

// ---------- Hermes (WEB-16) ----------

fn parse_memory_kind(raw: &str) -> Result<crate::hermes_config::MemoryKind, ApiResult> {
    serde_json::from_value(Value::String(raw.to_string()))
        .map_err(|_| bad_request(format!("unknown memory kind: {raw}")))
}

async fn hermes_import_live(State(state): State<SharedState>) -> ApiResult {
    blocking_api("hermes import", move || {
        crate::services::provider::import_hermes_providers_from_live(&state)
    })
    .await
}

async fn hermes_live_provider_ids() -> ApiResult {
    blocking_api("hermes live provider ids", || {
        crate::hermes_config::get_providers().map(|p| p.keys().cloned().collect::<Vec<_>>())
    })
    .await
}

async fn hermes_model_config() -> ApiResult {
    blocking_api(
        "hermes model config",
        crate::hermes_config::get_model_config,
    )
    .await
}

async fn hermes_memory_get(Path(kind): Path<String>) -> ApiResult {
    let kind = match parse_memory_kind(&kind) {
        Ok(k) => k,
        Err(resp) => return resp,
    };
    blocking_api("hermes memory read", move || {
        crate::hermes_config::read_memory(kind)
    })
    .await
}

#[derive(Deserialize)]
struct HermesMemoryReq {
    content: String,
}

async fn hermes_memory_set(
    Path(kind): Path<String>,
    Json(body): Json<HermesMemoryReq>,
) -> ApiResult {
    let kind = match parse_memory_kind(&kind) {
        Ok(k) => k,
        Err(resp) => return resp,
    };
    blocking_api("hermes memory write", move || {
        crate::hermes_config::write_memory(kind, &body.content)
    })
    .await
}

async fn hermes_memory_limits() -> ApiResult {
    blocking_api(
        "hermes memory limits",
        crate::hermes_config::read_memory_limits,
    )
    .await
}

#[derive(Deserialize)]
struct HermesMemoryEnabledReq {
    enabled: bool,
}

async fn hermes_memory_enabled(
    Path(kind): Path<String>,
    Json(body): Json<HermesMemoryEnabledReq>,
) -> ApiResult {
    let kind = match parse_memory_kind(&kind) {
        Ok(k) => k,
        Err(resp) => return resp,
    };
    blocking_api("hermes memory toggle", move || {
        crate::hermes_config::set_memory_enabled(kind, body.enabled)
    })
    .await
}

// ---------- Pi / Oh My Pi (WEB-17) ----------

fn parse_pi_app(raw: &str) -> Result<AppType, ApiResult> {
    match AppType::from_str(raw) {
        Ok(t @ (AppType::Pi | AppType::Omp)) => Ok(t),
        _ => Err(bad_request(format!("unsupported app: {raw}"))),
    }
}

fn parse_pi_prompt_kind(
    raw: &str,
) -> Result<crate::services::pi_prompt_files::PiPromptFileKind, ApiResult> {
    serde_json::from_value(Value::String(raw.to_string()))
        .map_err(|_| bad_request(format!("unknown prompt file kind: {raw}")))
}

async fn pi_current_state(State(state): State<SharedState>, Path(app): Path<String>) -> ApiResult {
    let app_type = match parse_pi_app(&app) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    blocking_api("pi current state", move || match app_type {
        AppType::Omp => crate::commands::omp::current_state(&state),
        _ => crate::services::pi_state::PiStateService::current(&state),
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PiUsageScriptReq {
    usage_script: crate::provider::UsageScript,
}

async fn pi_usage_script_set(
    State(state): State<SharedState>,
    Path((app, id)): Path<(String, String)>,
    Json(body): Json<PiUsageScriptReq>,
) -> ApiResult {
    let app_type = match parse_pi_app(&app) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    blocking_api("pi usage script", move || {
        ProviderService::update_pi_usage_script(&state, app_type, &id, body.usage_script)
    })
    .await
}

async fn pi_session_discovery() -> ApiResult {
    blocking_api("pi session discovery", || {
        Ok::<_, String>(crate::session_manager::providers::pi::session_discovery())
    })
    .await
}

async fn pi_prompt_file_get(Path(kind): Path<String>) -> ApiResult {
    let kind = match parse_pi_prompt_kind(&kind) {
        Ok(k) => k,
        Err(resp) => return resp,
    };
    blocking_api_conflict("pi prompt file read", move || {
        crate::services::pi_prompt_files::PiPromptFileService::read(kind)
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PiPromptReplaceReq {
    expected_revision: String,
    content: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PiPromptDeleteReq {
    expected_revision: String,
}

async fn pi_prompt_file_replace(
    Path(kind): Path<String>,
    Json(body): Json<PiPromptReplaceReq>,
) -> ApiResult {
    let kind = match parse_pi_prompt_kind(&kind) {
        Ok(k) => k,
        Err(resp) => return resp,
    };
    blocking_api_conflict("pi prompt file replace", move || {
        crate::services::pi_prompt_files::PiPromptFileService::replace(
            kind,
            &body.expected_revision,
            &body.content,
        )
    })
    .await
}

async fn pi_prompt_file_delete(
    Path(kind): Path<String>,
    Json(body): Json<PiPromptDeleteReq>,
) -> ApiResult {
    let kind = match parse_pi_prompt_kind(&kind) {
        Ok(k) => k,
        Err(resp) => return resp,
    };
    blocking_api_conflict("pi prompt file delete", move || {
        crate::services::pi_prompt_files::PiPromptFileService::delete(kind, &body.expected_revision)
    })
    .await
}

async fn pi_prompt_templates_list() -> ApiResult {
    blocking_api_conflict("pi prompt templates list", || {
        crate::services::pi_prompt_files::PiPromptTemplateService::list()
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PiTemplateUpsertReq {
    original_slug: Option<String>,
    expected_revision: String,
    content: String,
}

async fn pi_prompt_template_upsert(
    Path(slug): Path<String>,
    Json(body): Json<PiTemplateUpsertReq>,
) -> ApiResult {
    blocking_api_conflict("pi prompt template upsert", move || {
        crate::services::pi_prompt_files::PiPromptTemplateService::upsert(
            &slug,
            body.original_slug.as_deref(),
            &body.expected_revision,
            &body.content,
        )
    })
    .await
}

async fn pi_prompt_template_delete(
    Path(slug): Path<String>,
    Json(body): Json<PiPromptDeleteReq>,
) -> ApiResult {
    blocking_api_conflict("pi prompt template delete", move || {
        crate::services::pi_prompt_files::PiPromptTemplateService::delete(
            &slug,
            &body.expected_revision,
        )
    })
    .await
}

// ---------- OpenClaw (WEB-15) ----------

async fn openclaw_import_live(State(state): State<SharedState>) -> ApiResult {
    blocking_api("openclaw import", move || {
        crate::services::provider::import_openclaw_providers_from_live(&state)
    })
    .await
}

async fn openclaw_live_provider_ids() -> ApiResult {
    blocking_api("openclaw live provider ids", || {
        crate::openclaw_config::get_providers().map(|p| p.keys().cloned().collect::<Vec<_>>())
    })
    .await
}

async fn openclaw_live_provider(Path(id): Path<String>) -> ApiResult {
    blocking_api("openclaw live provider", move || {
        crate::openclaw_config::get_provider(&id)
    })
    .await
}

async fn openclaw_health() -> ApiResult {
    blocking_api(
        "openclaw health",
        crate::openclaw_config::scan_openclaw_config_health,
    )
    .await
}

#[derive(Deserialize)]
struct OpenClawDefaultModelReq {
    model: crate::openclaw_config::OpenClawDefaultModel,
}

async fn openclaw_default_model_get() -> ApiResult {
    blocking_api(
        "openclaw default model",
        crate::openclaw_config::get_default_model,
    )
    .await
}

async fn openclaw_default_model_set(Json(body): Json<OpenClawDefaultModelReq>) -> ApiResult {
    blocking_api("openclaw default model write", move || {
        crate::openclaw_config::set_default_model(&body.model)
    })
    .await
}

#[derive(Deserialize)]
struct OpenClawModelCatalogReq {
    catalog: std::collections::HashMap<String, crate::openclaw_config::OpenClawModelCatalogEntry>,
}

async fn openclaw_model_catalog_get() -> ApiResult {
    blocking_api(
        "openclaw model catalog",
        crate::openclaw_config::get_model_catalog,
    )
    .await
}

async fn openclaw_model_catalog_set(Json(body): Json<OpenClawModelCatalogReq>) -> ApiResult {
    blocking_api("openclaw model catalog write", move || {
        crate::openclaw_config::set_model_catalog(&body.catalog)
    })
    .await
}

#[derive(Deserialize)]
struct OpenClawAgentsDefaultsReq {
    defaults: crate::openclaw_config::OpenClawAgentsDefaults,
}

async fn openclaw_agents_defaults_get() -> ApiResult {
    blocking_api(
        "openclaw agents defaults",
        crate::openclaw_config::get_agents_defaults,
    )
    .await
}

async fn openclaw_agents_defaults_set(Json(body): Json<OpenClawAgentsDefaultsReq>) -> ApiResult {
    blocking_api("openclaw agents defaults write", move || {
        crate::openclaw_config::set_agents_defaults(&body.defaults)
    })
    .await
}

#[derive(Deserialize)]
struct OpenClawEnvReq {
    env: crate::openclaw_config::OpenClawEnvConfig,
}

async fn openclaw_env_get() -> ApiResult {
    blocking_api("openclaw env", crate::openclaw_config::get_env_config).await
}

async fn openclaw_env_set(Json(body): Json<OpenClawEnvReq>) -> ApiResult {
    blocking_api("openclaw env write", move || {
        crate::openclaw_config::set_env_config(&body.env)
    })
    .await
}

#[derive(Deserialize)]
struct OpenClawToolsReq {
    tools: crate::openclaw_config::OpenClawToolsConfig,
}

async fn openclaw_tools_get() -> ApiResult {
    blocking_api("openclaw tools", crate::openclaw_config::get_tools_config).await
}

async fn openclaw_tools_set(Json(body): Json<OpenClawToolsReq>) -> ApiResult {
    blocking_api("openclaw tools write", move || {
        crate::openclaw_config::set_tools_config(&body.tools)
    })
    .await
}

#[derive(Deserialize)]
struct HermesWebUiQuery {
    path: Option<String>,
}

/// Host part of a `Host` header without the port; `None` when it is not a plain
/// hostname / IPv4 / bracketed IPv6 literal (anything else could smuggle a URL).
fn host_without_port(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let host = if raw.starts_with('[') {
        let end = raw.find(']')?;
        &raw[..=end]
    } else {
        raw.rsplit_once(':').map_or(raw, |(h, _)| h)
    };
    let bracketed = host.starts_with('[');
    let inner = host.trim_start_matches('[').trim_end_matches(']');
    // ':' is only legal inside an IPv6 literal, which must be bracketed
    let valid = !inner.is_empty()
        && inner.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '-') || (bracketed && c == ':')
        });
    valid.then(|| host.to_string())
}

/// Forces a single leading `/` and drops control characters, so the result can
/// only be a path on the Hermes origin (`//evil.example` -> `/evil.example`).
fn normalize_web_path(raw: Option<&str>) -> String {
    let cleaned: String = raw
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    format!("/{}", cleaned.trim_start_matches(['/', '\\']))
}

/// Probes Hermes Web UI on the server's loopback and returns the URL a browser
/// reaches it at: the request's own host (the client got here through it) plus
/// the Hermes port. Plain `http`: the service has no TLS of its own.
async fn hermes_web_ui(headers: HeaderMap, Query(q): Query<HermesWebUiQuery>) -> ApiResult {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .and_then(host_without_port)
    else {
        return bad_request("missing or invalid Host header");
    };
    let port = crate::commands::hermes_web_port();
    let reachable = match crate::commands::probe_hermes_web(port).await {
        Ok(r) => r,
        Err(e) => return fail(e),
    };
    let url = format!(
        "http://{host}:{port}{}",
        normalize_web_path(q.path.as_deref())
    );
    ok(json!({ "reachable": reachable, "port": port, "url": url }))
}

// ---------- OpenClaw workspace + daily memory (WEB-15) ----------
//
// Filename validation lives in `commands::workspace`; the handlers call the
// same validators first only so a bad name answers 400 (errors that come out
// of `blocking_api`-style paths are 500).

fn cmd_result<T: serde::Serialize>(r: Result<T, String>) -> ApiResult {
    match r {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}

#[derive(Deserialize)]
struct OpenClawContentReq {
    content: String,
}

#[derive(Deserialize)]
struct OpenClawMemorySearchQuery {
    #[serde(default)]
    q: String,
}

async fn openclaw_workspace_get(Path(filename): Path<String>) -> ApiResult {
    if let Err(e) = crate::commands::validate_filename(&filename) {
        return bad_request(e);
    }
    cmd_result(crate::commands::read_workspace_file(filename).await)
}

async fn openclaw_workspace_set(
    Path(filename): Path<String>,
    Json(body): Json<OpenClawContentReq>,
) -> ApiResult {
    if let Err(e) = crate::commands::validate_filename(&filename) {
        return bad_request(e);
    }
    cmd_result(crate::commands::write_workspace_file(filename, body.content).await)
}

async fn openclaw_workspace_paths() -> ApiResult {
    ok(json!({
        "workspace": crate::commands::workspace_subdir("workspace"),
        "memory": crate::commands::workspace_subdir("memory"),
    }))
}

async fn openclaw_daily_memory_list() -> ApiResult {
    cmd_result(crate::commands::list_daily_memory_files().await)
}

async fn openclaw_daily_memory_search(Query(q): Query<OpenClawMemorySearchQuery>) -> ApiResult {
    cmd_result(crate::commands::search_daily_memory_files(q.q).await)
}

async fn openclaw_daily_memory_get(Path(filename): Path<String>) -> ApiResult {
    if let Err(e) = crate::commands::validate_daily_memory_filename(&filename) {
        return bad_request(e);
    }
    cmd_result(crate::commands::read_daily_memory_file(filename).await)
}

async fn openclaw_daily_memory_set(
    Path(filename): Path<String>,
    Json(body): Json<OpenClawContentReq>,
) -> ApiResult {
    if let Err(e) = crate::commands::validate_daily_memory_filename(&filename) {
        return bad_request(e);
    }
    cmd_result(crate::commands::write_daily_memory_file(filename, body.content).await)
}

async fn openclaw_daily_memory_delete(Path(filename): Path<String>) -> ApiResult {
    if let Err(e) = crate::commands::validate_daily_memory_filename(&filename) {
        return bad_request(e);
    }
    cmd_result(crate::commands::delete_daily_memory_file(filename).await)
}

// ─── DB backups (WEB-10) ─────────────────────────────────────

/// `AppError` -> response: `InvalidInput` is the caller's fault (400), the rest 500.
fn backup_result<T: serde::Serialize>(r: Result<T, AppError>) -> ApiResult {
    match r {
        Ok(v) => ok(v),
        Err(AppError::InvalidInput(m)) => bad_request(m),
        Err(e) => fail(e.to_string()),
    }
}

async fn backup_blocking<T, F>(what: &'static str, f: F) -> ApiResult
where
    T: serde::Serialize + Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(r) => backup_result(r),
        Err(e) => fail(format!("{what} task failed: {e}")),
    }
}

async fn backups_create(State(state): State<SharedState>) -> ApiResult {
    backup_result(crate::commands::import_export::create_db_backup_file(state.db.clone()).await)
}

async fn backups_list() -> ApiResult {
    backup_blocking("backup list", Database::list_backups).await
}

async fn backup_restore(
    State(state): State<SharedState>,
    Path(filename): Path<String>,
) -> ApiResult {
    backup_result(
        crate::commands::import_export::restore_db_backup_file((*state).clone(), filename).await,
    )
}

async fn backup_rename(Path(filename): Path<String>, Json(body): Json<Value>) -> ApiResult {
    let Some(new_name) = body
        .get("newName")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return bad_request("newName is required");
    };
    backup_blocking("backup rename", move || {
        Database::rename_backup(&filename, &new_name)
    })
    .await
}

async fn backup_delete(Path(filename): Path<String>) -> ApiResult {
    backup_blocking("backup delete", move || Database::delete_backup(&filename)).await
}

async fn backup_locations_list() -> ApiResult {
    backup_blocking("backup locations", || {
        Ok(crate::services::backup_storage::list_locations())
    })
    .await
}

async fn backup_location_delete(Path(id): Path<String>) -> ApiResult {
    backup_blocking("backup location delete", move || {
        crate::services::backup_storage::delete_location(&id)
    })
    .await
}

// ─── Config import/export (WEB-09) ───────────────────────────

/// SQL dumps of big installs (usage logs) exceed axum's 2 MiB default.
const CONFIG_IMPORT_BODY_LIMIT: usize = 64 * 1024 * 1024;

/// Full SQL dump as text (the browser saves it); `dump_sql` yields valid UTF-8.
async fn config_export(State(state): State<SharedState>) -> ApiResult {
    let db = state.db.clone();
    backup_blocking("config export", move || {
        let content = db.export_sql_string()?;
        let filename = format!(
            "cc-switch-{}.sql",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        );
        Ok(json!({ "filename": filename, "content": content }))
    })
    .await
}

async fn config_import(State(state): State<SharedState>, Json(body): Json<Value>) -> ApiResult {
    let Some(content) = body
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return bad_request("content is required");
    };
    backup_result(
        crate::commands::import_export::import_config_with((*state).clone(), move |db| {
            db.import_sql_string(&content)
        })
        .await,
    )
}

async fn providers_sync_live(State(state): State<SharedState>) -> ApiResult {
    backup_result(
        crate::commands::import_export::sync_current_providers_live_core(state.db.clone()).await,
    )
}
