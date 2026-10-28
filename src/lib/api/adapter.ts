/**
 * Web transport for the cc-switch-web fork (Tailscale-only, no in-app auth).
 *
 * In the web build, `@tauri-apps/api/core` is aliased to `web-shims/core`,
 * which re-exports `invoke` from here. Every `src/lib/api/*` module keeps
 * calling `invoke("<tauri-command>", args)` unchanged — this file translates
 * the MVP command subset to the Axum HTTP API (`src-tauri/src/web_api.rs`).
 * Anything else throws a clear "unsupported in web mode" error instead of
 * hanging or crashing the console.
 */

import { downloadText } from "@/lib/download";

export const isWeb =
  typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);

type Args = Record<string, any>;

interface Endpoint {
  method: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
  url: string;
  body?: unknown;
  /** When set, resolve with `data[pick]` instead of the whole payload. */
  pick?: string;
  /** `arrayBuffer`: success is a raw body (no `{ok,data}` envelope); errors stay JSON. */
  responseType?: "json" | "arrayBuffer";
}

const enc = encodeURIComponent;

function query(params: Record<string, unknown>): string {
  const parts: string[] = [];
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined || value === null) continue;
    parts.push(`${enc(key)}=${enc(String(value))}`);
  }
  return parts.length > 0 ? `?${parts.join("&")}` : "";
}

function appBase(app: string): string {
  return `/api/${enc(app)}`;
}

/**
 * Translate one Tauri command + args to an HTTP endpoint.
 * Returns `null` when the command has no web equivalent.
 */
function mapCommand(cmd: string, a: Args): Endpoint | null {
  switch (cmd) {
    // ---------- providers ----------
    case "get_providers":
      return { method: "GET", url: `${appBase(a.app)}/providers` };
    case "get_current_provider":
      return { method: "GET", url: `${appBase(a.app)}/providers/current` };
    case "switch_provider":
      return {
        method: "POST",
        url: `${appBase(a.app)}/providers/switch`,
        body: { id: a.id },
      };
    case "add_provider":
      return {
        method: "POST",
        url: `${appBase(a.app)}/providers`,
        body: {
          provider: a.provider,
          addToLive: a.addToLive,
          editorSave: a.editorSave,
        },
      };
    case "update_provider":
      return {
        method: "PUT",
        url: `${appBase(a.app)}/providers`,
        body: {
          provider: a.provider,
          originalId: a.originalId,
          editorSave: a.editorSave,
        },
      };
    case "delete_provider":
      return {
        method: "DELETE",
        url: `${appBase(a.app)}/providers/${enc(a.id)}`,
      };
    case "update_providers_sort_order":
      return {
        method: "POST",
        url: `${appBase(a.app)}/providers/sort`,
        body: { updates: a.updates },
      };
    case "import_default_config":
      return {
        method: "POST",
        url: `${appBase(a.app)}/providers/import-default`,
      };
    case "add_provider_to_live_config":
      return {
        method: "POST",
        url: `${appBase(a.app)}/providers/live-add`,
        body: { id: a.id },
      };
    case "remove_provider_from_live_config":
      return {
        method: "POST",
        url: `${appBase(a.app)}/providers/live-remove`,
        body: { id: a.id },
      };
    case "get_provider_editor_view":
      return {
        method: "POST",
        url: `${appBase(a.app)}/providers/editor-view`,
        body: {
          settingsConfig: a.settingsConfig,
          category: a.category ?? null,
          providerId: a.providerId ?? null,
          meta: a.meta ?? null,
        },
      };

    // ---------- rectifier / optimizer / log config (reveal_app_log_file stays unmapped) ----------
    case "get_rectifier_config":
      return { method: "GET", url: "/api/settings/rectifier" };
    case "set_rectifier_config":
      return {
        method: "PUT",
        url: "/api/settings/rectifier",
        body: { config: a.config },
      };
    case "get_optimizer_config":
      return { method: "GET", url: "/api/settings/optimizer" };
    case "set_optimizer_config":
      return {
        method: "PUT",
        url: "/api/settings/optimizer",
        body: { config: a.config },
      };
    case "get_log_config":
      return { method: "GET", url: "/api/settings/log" };
    case "set_log_config":
      return {
        method: "PUT",
        url: "/api/settings/log",
        body: { config: a.config },
      };

    // ---------- server log (web-only, no Tauri command) ----------
    case "get_server_log_tail":
      return {
        method: "GET",
        url: `/api/logs/tail${query({ lines: a.lines })}`,
      };

    // ---------- endpoint speed test / custom endpoints ----------
    case "read_live_provider_settings":
      return { method: "GET", url: `${appBase(a.app)}/live-settings` };
    case "test_api_endpoints":
      return {
        method: "POST",
        url: "/api/endpoints/speedtest",
        body: { urls: a.urls, timeoutSecs: a.timeoutSecs },
      };
    case "get_custom_endpoints":
      return {
        method: "GET",
        url: `${appBase(a.app)}/providers/${enc(a.providerId)}/endpoints`,
      };
    case "add_custom_endpoint":
      return {
        method: "POST",
        url: `${appBase(a.app)}/providers/${enc(a.providerId)}/endpoints`,
        body: { url: a.url },
      };
    case "remove_custom_endpoint":
      return {
        method: "DELETE",
        url: `${appBase(a.app)}/providers/${enc(a.providerId)}/endpoints`,
        body: { url: a.url },
      };
    case "update_endpoint_last_used":
      return {
        method: "POST",
        url: `${appBase(a.app)}/providers/${enc(a.providerId)}/endpoints/last-used`,
        body: { url: a.url },
      };

    // ---------- prompts ----------
    case "get_prompts":
      return { method: "GET", url: `${appBase(a.app)}/prompts` };
    case "upsert_prompt":
      return {
        method: "PUT",
        url: `${appBase(a.app)}/prompts/${enc(a.id)}`,
        body: { prompt: a.prompt },
      };
    case "delete_prompt":
      return {
        method: "DELETE",
        url: `${appBase(a.app)}/prompts/${enc(a.id)}`,
      };
    case "enable_prompt":
      return {
        method: "POST",
        url: `${appBase(a.app)}/prompts/${enc(a.id)}/enable`,
      };
    case "import_prompt_from_file":
      return {
        method: "POST",
        url: `${appBase(a.app)}/prompts/import-from-file`,
      };
    case "get_current_prompt_file_content":
      return { method: "GET", url: `${appBase(a.app)}/prompt-content` };
    case "get_prompt_file_location":
      return { method: "GET", url: `${appBase(a.app)}/prompt-location` };

    // ---------- custom icon library ----------
    case "get_custom_icons":
      return { method: "GET", url: "/api/custom-icons" };
    case "add_custom_icon":
      return {
        method: "POST",
        url: "/api/custom-icons",
        body: { name: a.name, data: a.data },
      };
    case "rename_custom_icon":
      return {
        method: "PUT",
        url: `/api/custom-icons/${enc(a.id)}`,
        body: { name: a.name },
      };
    case "delete_custom_icon":
      return { method: "DELETE", url: `/api/custom-icons/${enc(a.id)}` };

    // ---------- mcp ----------
    case "get_mcp_servers":
      return { method: "GET", url: "/api/mcp/servers" };
    case "upsert_mcp_server":
      return {
        method: "PUT",
        url: "/api/mcp/servers",
        body: { server: a.server },
      };
    case "delete_mcp_server":
      return { method: "DELETE", url: `/api/mcp/servers/${enc(a.id)}` };
    case "toggle_mcp_app":
    case "set_mcp_enabled":
      return {
        method: "POST",
        url: `/api/mcp/servers/${enc(a.serverId ?? a.id)}/toggle`,
        body: { app: a.app, enabled: a.enabled },
      };
    case "get_mcp_config":
      return { method: "GET", url: `/api/mcp/config${query({ app: a.app })}` };
    case "validate_mcp_command":
      return { method: "POST", url: "/api/mcp/validate", body: { cmd: a.cmd } };
    case "import_mcp_from_apps":
      return { method: "POST", url: "/api/mcp/import" };
    case "resync_mcp_to_apps":
      return {
        method: "POST",
        url: "/api/mcp/resync",
        body: { apps: a.apps ?? null },
      };

    // ---------- skills ----------
    case "get_installed_skills":
      return { method: "GET", url: "/api/skills/installed" };
    case "toggle_skill_app":
      return {
        method: "POST",
        url: "/api/skills/toggle",
        body: { id: a.id, app: a.app, enabled: a.enabled },
      };
    case "resync_skills_to_apps":
      return { method: "POST", url: "/api/skills/resync" };
    case "uninstall_skill_unified":
      return {
        method: "POST",
        url: "/api/skills/uninstall",
        body: { id: a.id },
      };

    // ---------- sessions ----------
    case "list_sessions":
      return { method: "GET", url: "/api/sessions" };
    case "get_session_messages":
      return {
        method: "GET",
        url: `/api/sessions/messages${query({ providerId: a.providerId, sourcePath: a.sourcePath })}`,
      };
    case "get_session_image":
      return {
        method: "POST",
        url: "/api/sessions/image",
        body: {
          providerId: a.providerId,
          sourcePath: a.sourcePath,
          image: a.image,
        },
        responseType: "arrayBuffer",
      };
    case "get_session_block_content":
      return {
        method: "POST",
        url: "/api/sessions/block",
        body: {
          providerId: a.providerId,
          sourcePath: a.sourcePath,
          contentRef: a.contentRef,
          offset: a.offset,
          limit: a.limit,
        },
      };
    case "delete_session":
      return {
        method: "DELETE",
        url: "/api/sessions/one",
        body: {
          providerId: a.providerId,
          sessionId: a.sessionId,
          sourcePath: a.sourcePath,
        },
      };
    case "delete_sessions":
      return {
        method: "DELETE",
        url: "/api/sessions",
        body: { items: a.items },
      };

    // ---------- usage ----------
    case "get_usage_summary":
      return {
        method: "GET",
        url: `/api/usage/summary${query({
          start: a.startDate,
          end: a.endDate,
          app: a.appType,
          provider: a.providerName,
          model: a.model,
        })}`,
      };
    case "get_session_usage_summary":
      return {
        method: "GET",
        url: `/api/usage/session${query({ appType: a.appType, sessionId: a.sessionId })}`,
      };
    case "get_usage_summary_by_app":
      return {
        method: "GET",
        url: `/api/usage/by-app${query({
          start: a.startDate,
          end: a.endDate,
          provider: a.providerName,
          model: a.model,
        })}`,
      };
    case "get_usage_trends":
      return {
        method: "GET",
        url: `/api/usage/trends${query({
          start: a.startDate,
          end: a.endDate,
          app: a.appType,
          provider: a.providerName,
          model: a.model,
        })}`,
      };
    case "get_usage_first_date":
      return {
        method: "GET",
        url: `/api/usage/first-date${query({
          app: a.appType,
          provider: a.providerName,
          model: a.model,
        })}`,
      };
    case "get_provider_stats":
      return {
        method: "GET",
        url: `/api/usage/provider-stats${query({
          start: a.startDate,
          end: a.endDate,
          app: a.appType,
          provider: a.providerName,
          model: a.model,
        })}`,
      };
    case "get_model_stats":
      return {
        method: "GET",
        url: `/api/usage/model-stats${query({
          start: a.startDate,
          end: a.endDate,
          app: a.appType,
          provider: a.providerName,
          model: a.model,
        })}`,
      };
    case "get_request_logs":
      return {
        method: "POST",
        url: "/api/usage/request-logs",
        body: { filters: a.filters, page: a.page, pageSize: a.pageSize },
      };
    case "get_request_detail":
      return {
        method: "GET",
        url: `/api/usage/request-detail/${enc(a.requestId)}`,
      };
    case "get_model_pricing":
      return { method: "GET", url: "/api/usage/model-pricing" };
    case "sync_session_usage":
      return { method: "POST", url: "/api/usage/sync" };
    case "update_model_pricing":
      return {
        method: "PUT",
        url: `/api/usage/model-pricing/${enc(a.modelId)}`,
        body: {
          displayName: a.displayName,
          inputCost: a.inputCost,
          outputCost: a.outputCost,
          cacheReadCost: a.cacheReadCost,
          cacheCreationCost: a.cacheCreationCost,
        },
      };
    case "update_model_pricing_batch":
      return {
        method: "POST",
        url: "/api/usage/model-pricing/batch",
        body: { entries: a.entries },
      };
    case "delete_model_pricing":
      return {
        method: "DELETE",
        url: `/api/usage/model-pricing/${enc(a.modelId)}`,
      };
    case "get_models_dev_sync_config":
      return { method: "GET", url: "/api/usage/models-dev/config" };
    case "save_models_dev_sync_config":
      // The route takes the bare config object, not the {config} wrapper.
      return {
        method: "PUT",
        url: "/api/usage/models-dev/config",
        body: a.config,
      };
    case "record_models_dev_sync_result":
      return {
        method: "POST",
        url: "/api/usage/models-dev/result",
        body: { syncedAt: a.syncedAt, error: a.error },
      };
    case "get_session_usage_last_sync":
      return { method: "GET", url: "/api/usage/last-sync" };
    case "rebuild_codex_usage":
      return { method: "POST", url: "/api/usage/rebuild-codex" };
    case "get_usage_data_sources":
      return { method: "GET", url: "/api/usage/data-sources" };
    case "get_pricing_model_source":
      return {
        method: "GET",
        url: `/api/${enc(a.appType)}/pricing-model-source`,
      };
    case "set_pricing_model_source":
      return {
        method: "PUT",
        url: `/api/${enc(a.appType)}/pricing-model-source`,
        body: { value: a.value },
      };

    // ---------- app-specific settings (WEB-20) ----------
    case "has_codex_unify_history_backup":
      return { method: "GET", url: "/api/codex/unify-history-backup" };
    case "codex_forces_multi_agent_v2":
      return { method: "GET", url: "/api/codex/forces-multi-agent-v2" };
    case "get_codex_session_compression":
      return { method: "GET", url: "/api/codex/session-compression" };
    case "set_codex_session_compression":
      return {
        method: "PUT",
        url: "/api/codex/session-compression",
        body: { enabled: a.enabled },
      };
    case "get_codex_sessions_disk_usage":
      return { method: "GET", url: "/api/codex/sessions-disk-usage" };
    case "restore_codex_unified_history":
      return { method: "POST", url: "/api/codex/restore-unified-history" };
    case "apply_claude_plugin_config":
      // `official: true` clears the plugin config (same as the desktop command).
      return {
        method: "POST",
        url: "/api/claude/plugin-config",
        body: { official: a.official },
      };
    case "apply_claude_onboarding_skip":
      return { method: "POST", url: "/api/claude/onboarding-skip" };
    case "clear_claude_onboarding_skip":
      return { method: "DELETE", url: "/api/claude/onboarding-skip" };

    // ---------- Hermes config & memory (WEB-16) ----------
    // `launch_hermes_dashboard` (visible terminal on the server) stays desktop-only: unmapped.
    case "import_hermes_providers_from_live":
      return { method: "POST", url: "/api/hermes/import-live" };
    case "get_hermes_live_provider_ids":
      return { method: "GET", url: "/api/hermes/live-provider-ids" };
    case "get_hermes_model_config":
      return { method: "GET", url: "/api/hermes/model-config" };
    case "get_hermes_memory":
      return { method: "GET", url: `/api/hermes/memory/${enc(a.kind)}` };
    case "set_hermes_memory":
      return {
        method: "PUT",
        url: `/api/hermes/memory/${enc(a.kind)}`,
        body: { content: a.content },
      };
    case "get_hermes_memory_limits":
      return { method: "GET", url: "/api/hermes/memory-limits" };
    case "set_hermes_memory_enabled":
      return {
        method: "PUT",
        url: `/api/hermes/memory/${enc(a.kind)}/enabled`,
        body: { enabled: a.enabled },
      };
    case "open_hermes_web_ui":
      // Probe only: the server answers {reachable, port, url}; hermesApi.openWebUI opens the url.
      return {
        method: "GET",
        url: `/api/hermes/web-ui${query({ path: a.path })}`,
      };

    // ---------- Pi / Oh My Pi state, usage script, sessions, prompts (WEB-17) ----------
    case "get_pi_current_state":
      return { method: "GET", url: "/api/pi/current-state" };
    case "get_omp_current_state":
      return { method: "GET", url: "/api/omp/current-state" };
    case "update_pi_provider_usage_script":
      return {
        method: "PUT",
        url: `${appBase(a.app)}/providers/${enc(a.id)}/usage-script`,
        body: { usageScript: a.usageScript },
      };
    case "get_pi_session_discovery":
      return { method: "GET", url: "/api/pi/session-discovery" };
    case "get_pi_prompt_file":
      return { method: "GET", url: `/api/pi/prompt-files/${enc(a.kind)}` };
    case "replace_pi_prompt_file":
      return {
        method: "PUT",
        url: `/api/pi/prompt-files/${enc(a.kind)}`,
        body: { expectedRevision: a.expectedRevision, content: a.content },
      };
    case "delete_pi_prompt_file":
      return {
        method: "DELETE",
        url: `/api/pi/prompt-files/${enc(a.kind)}`,
        body: { expectedRevision: a.expectedRevision },
      };
    case "list_pi_prompt_templates":
      return { method: "GET", url: "/api/pi/prompt-templates" };
    case "upsert_pi_prompt_template":
      return {
        method: "PUT",
        url: `/api/pi/prompt-templates/${enc(a.slug)}`,
        body: {
          originalSlug: a.originalSlug ?? null,
          expectedRevision: a.expectedRevision,
          content: a.content,
        },
      };
    case "delete_pi_prompt_template":
      return {
        method: "DELETE",
        url: `/api/pi/prompt-templates/${enc(a.slug)}`,
        body: { expectedRevision: a.expectedRevision },
      };

    // ---------- DB backups & backup locations (WEB-10) ----------
    // `reveal_backup_location` (file manager on the server) stays desktop-only: unmapped.
    case "create_db_backup":
      return { method: "POST", url: "/api/backups" };
    case "list_db_backups":
      return { method: "GET", url: "/api/backups" };
    case "restore_db_backup":
      return {
        method: "POST",
        url: `/api/backups/${enc(a.filename)}/restore`,
      };
    case "rename_db_backup":
      return {
        method: "PATCH",
        url: `/api/backups/${enc(a.oldFilename)}`,
        body: { newName: a.newName },
      };
    case "delete_db_backup":
      return { method: "DELETE", url: `/api/backups/${enc(a.filename)}` };
    case "list_backup_locations":
      return { method: "GET", url: "/api/backup-locations" };
    case "delete_backup_location":
      return { method: "DELETE", url: `/api/backup-locations/${enc(a.id)}` };

    // ---------- Config import/export & manual live sync (WEB-09) ----------
    // The native file dialogs and *_to_file/_from_file commands stay desktop-only: unmapped.
    case "export_config_to_string":
      return { method: "GET", url: "/api/config/export" };
    case "import_config_from_string":
      return {
        method: "POST",
        url: "/api/config/import",
        body: { content: a.content },
      };
    case "sync_current_providers_live":
      return { method: "POST", url: "/api/providers/sync-live" };

    // ---------- Deep-link import via paste/upload (WEB-27) ----------
    // OS registration of ccswitch:// and the pending queue stay desktop-only (peek/ack are silent no-ops below).
    case "parse_deeplink":
      return {
        method: "POST",
        url: "/api/deeplink/parse",
        body: { url: a.url },
      };
    case "merge_deeplink_config":
      return {
        method: "POST",
        url: "/api/deeplink/merge",
        body: { request: a.request },
      };
    case "fetch_deeplink_icon":
      return {
        method: "POST",
        url: "/api/deeplink/icon",
        body: { url: a.url },
      };
    case "import_from_deeplink_unified":
      return {
        method: "POST",
        url: "/api/deeplink/import",
        body: { request: a.request },
      };

    // ---------- OpenClaw config, workspace & daily memory (WEB-15) ----------
    // `open_workspace_directory` (file manager on the server) stays desktop-only: unmapped.
    case "import_openclaw_providers_from_live":
      return { method: "POST", url: "/api/openclaw/import-live" };
    case "get_openclaw_live_provider_ids":
      return { method: "GET", url: "/api/openclaw/live-provider-ids" };
    case "get_openclaw_live_provider":
      return {
        method: "GET",
        url: `/api/openclaw/live-provider/${enc(a.providerId)}`,
      };
    case "scan_openclaw_config_health":
      return { method: "GET", url: "/api/openclaw/health" };
    case "get_openclaw_default_model":
      return { method: "GET", url: "/api/openclaw/default-model" };
    case "set_openclaw_default_model":
      return {
        method: "PUT",
        url: "/api/openclaw/default-model",
        body: { model: a.model },
      };
    case "get_openclaw_model_catalog":
      return { method: "GET", url: "/api/openclaw/model-catalog" };
    case "set_openclaw_model_catalog":
      return {
        method: "PUT",
        url: "/api/openclaw/model-catalog",
        body: { catalog: a.catalog },
      };
    case "get_openclaw_agents_defaults":
      return { method: "GET", url: "/api/openclaw/agents-defaults" };
    case "set_openclaw_agents_defaults":
      return {
        method: "PUT",
        url: "/api/openclaw/agents-defaults",
        body: { defaults: a.defaults },
      };
    case "get_openclaw_env":
      return { method: "GET", url: "/api/openclaw/env" };
    case "set_openclaw_env":
      return {
        method: "PUT",
        url: "/api/openclaw/env",
        body: { env: a.env },
      };
    case "get_openclaw_tools":
      return { method: "GET", url: "/api/openclaw/tools" };
    case "set_openclaw_tools":
      return {
        method: "PUT",
        url: "/api/openclaw/tools",
        body: { tools: a.tools },
      };
    case "read_workspace_file":
      return {
        method: "GET",
        url: `/api/openclaw/workspace/${enc(a.filename)}`,
      };
    case "write_workspace_file":
      return {
        method: "PUT",
        url: `/api/openclaw/workspace/${enc(a.filename)}`,
        body: { content: a.content },
      };
    case "list_daily_memory_files":
      return { method: "GET", url: "/api/openclaw/daily-memory" };
    case "search_daily_memory_files":
      return {
        method: "GET",
        url: `/api/openclaw/daily-memory/search${query({ q: a.query })}`,
      };
    case "read_daily_memory_file":
      return {
        method: "GET",
        url: `/api/openclaw/daily-memory/${enc(a.filename)}`,
      };
    case "write_daily_memory_file":
      return {
        method: "PUT",
        url: `/api/openclaw/daily-memory/${enc(a.filename)}`,
        body: { content: a.content },
      };
    case "delete_daily_memory_file":
      return {
        method: "DELETE",
        url: `/api/openclaw/daily-memory/${enc(a.filename)}`,
      };
    // Web-only (no desktop command): server-side absolute paths of the workspace dirs.
    case "get_openclaw_workspace_paths":
      return { method: "GET", url: "/api/openclaw/workspace-paths" };

    // ---------- settings & environment ----------
    case "get_settings":
      return { method: "GET", url: "/api/settings" };
    case "save_settings":
      return {
        method: "PUT",
        url: "/api/settings",
        body: { settings: a.settings },
      };
    case "get_app_config_path":
      return { method: "GET", url: "/api/paths", pick: "configPath" };
    case "get_config_dir":
      return { method: "GET", url: "/api/paths", pick: "configDir" };
    case "get_tool_versions":
      return {
        method: "POST",
        url: "/api/env/tools",
        body: {
          tools: a.tools ?? null,
          wslShellByTool: a.wslShellByTool ?? null,
        },
      };
    case "run_tool_lifecycle_action":
      return {
        method: "POST",
        url: "/api/tools/lifecycle",
        body: {
          tools: a.tools,
          action: a.action,
          wslShellByTool: a.wslShellByTool ?? null,
          uninstallPlan: a.uninstallPlan ?? null,
        },
      };
    case "prepare_tool_uninstall":
      return {
        method: "POST",
        url: "/api/tools/uninstall-preview",
        body: { tool: a.tool, wslShellByTool: a.wslShellByTool ?? null },
      };
    case "probe_tool_installations":
      return {
        method: "POST",
        url: "/api/tools/probe",
        body: { tools: a.tools },
      };
    case "list_tool_installations":
      return {
        method: "POST",
        url: "/api/tools/installations",
        body: { tools: a.tools },
      };
    case "check_env_conflicts":
      return {
        method: "GET",
        url: `/api/env/conflicts${query({ app: a.app })}`,
      };
    case "delete_env_vars":
      return {
        method: "POST",
        url: "/api/env/conflicts/cleanup",
        body: { conflicts: a.conflicts },
      };
    case "restore_env_backup":
      return {
        method: "POST",
        url: "/api/env/restore",
        body: { backupPath: a.backupPath },
      };

    // ---------- OMO / OMO Slim live config ----------
    case "read_omo_local_file":
      return { method: "GET", url: "/api/omo/omo/local-file" };
    case "get_current_omo_provider_id":
      return { method: "GET", url: "/api/omo/omo/current-provider" };
    case "disable_current_omo":
      return { method: "POST", url: "/api/omo/omo/disable" };
    case "read_omo_slim_local_file":
      return { method: "GET", url: "/api/omo/omo-slim/local-file" };
    case "get_current_omo_slim_provider_id":
      return { method: "GET", url: "/api/omo/omo-slim/current-provider" };
    case "disable_current_omo_slim":
      return { method: "POST", url: "/api/omo/omo-slim/disable" };

    // ---------- startup one-shots ----------
    // InitErrorPayload | null (WEB-32); main.tsx shows DatabaseUpgrade / the error screen
    case "get_init_error":
      return { method: "GET", url: "/api/init-status" };
    case "get_migration_result":
      return {
        method: "GET",
        url: "/api/migrations/status",
        pick: "migration",
      };
    case "get_skills_migration_result":
      return { method: "GET", url: "/api/migrations/status", pick: "skills" };

    // ---------- universal providers ----------
    case "get_universal_providers":
      return { method: "GET", url: "/api/universal-providers" };
    case "get_universal_provider":
      return { method: "GET", url: `/api/universal-providers/${enc(a.id)}` };
    case "upsert_universal_provider":
      return {
        method: "PUT",
        url: "/api/universal-providers",
        body: { provider: a.provider },
      };
    case "delete_universal_provider":
      return { method: "DELETE", url: `/api/universal-providers/${enc(a.id)}` };
    case "sync_universal_provider":
      return {
        method: "POST",
        url: `/api/universal-providers/${enc(a.id)}/sync`,
      };

    // ---------- project profiles ----------
    case "list_profiles":
      return { method: "GET", url: "/api/profiles" };
    case "create_profile":
      return {
        method: "POST",
        url: "/api/profiles",
        body: { name: a.name, scope: a.scope },
      };
    case "update_profile":
      return {
        method: "PATCH",
        url: `/api/profiles/${enc(a.id)}`,
        body: { name: a.name, resnapshot: a.resnapshot, scope: a.scope },
      };
    case "delete_profile":
      return { method: "DELETE", url: `/api/profiles/${enc(a.id)}` };
    case "apply_profile":
      return {
        method: "POST",
        url: `/api/profiles/${enc(a.id)}/apply`,
        body: { scope: a.scope },
      };
    case "clear_current_profile":
      return {
        method: "POST",
        url: "/api/profiles/clear-current",
        body: { scope: a.scope },
      };

    default:
      return null;
  }
}

export class WebUnsupportedError extends Error {
  readonly command: string;
  constructor(command: string) {
    super(
      `«${command}» недоступно в web-режиме (desktop-only: трей, диалоги, терминал, OAuth, обновления)`,
    );
    this.name = "WebUnsupportedError";
    this.command = command;
  }
}

/** WEB-30: external HTTP(S) links open in the browser — no backend needed.
 *  Scheme validation mirrors the desktop command (misc.rs validate_external_url),
 *  restricted to http/https like settingsApi.openExternal. */
function openExternalInBrowser(url: unknown): boolean {
  if (typeof url !== "string" || !url.trim()) throw new Error("Invalid URL");
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    throw new Error("Invalid URL");
  }
  const scheme = parsed.protocol.replace(":", "").toLowerCase();
  if (scheme !== "http" && scheme !== "https") throw new Error("Invalid URL");
  if (typeof window === "undefined") throw new Error("Invalid URL");
  const opened = window.open(
    parsed.toString(),
    "_blank",
    "noopener,noreferrer",
  );
  if (!opened)
    throw new Error("Браузер заблокировал открытие ссылки (popup blocker)");
  return true;
}

/**
 * `stream_session_messages` over `GET /api/sessions/stream` (NDJSON, one
 * TranscriptChunk per line). Failing before the first chunk throws so the
 * caller falls back to `get_session_messages`; an error chunk is delivered to
 * `onChunk` and then thrown, as the desktop command does.
 */
async function streamSessionMessages(args: Args): Promise<void> {
  const url = `/api/sessions/stream${query({ providerId: args.providerId, sourcePath: args.sourcePath })}`;
  let res: Response;
  try {
    res = await fetch(url, { method: "GET" });
  } catch (e) {
    throw new Error(`Web API недоступно (GET ${url}): ${String(e)}`);
  }
  if (!res.ok || !res.body) {
    throw new Error(`Web API ошибка (stream_session_messages): ${res.status}`);
  }
  const deliver = (line: string) => {
    if (!line.trim()) return;
    const chunk = JSON.parse(line);
    args.onChunk?.onmessage?.(chunk);
    if (chunk?.type === "error") throw new Error(String(chunk.message));
  };
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let tail = "";
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    tail += decoder.decode(value, { stream: true });
    let nl = tail.indexOf("\n");
    while (nl >= 0) {
      const line = tail.slice(0, nl);
      tail = tail.slice(nl + 1);
      deliver(line);
      nl = tail.indexOf("\n");
    }
  }
  tail += decoder.decode();
  deliver(tail);
}

/** Drop-in replacement for Tauri's `invoke` used by every `src/lib/api/*` module. */
export async function invoke<T>(cmd: string, args: Args = {}): Promise<T> {
  // Silent local noops: desktop-only fire-and-forget calls that must not
  // spam the console in web mode (tray, window theme, deep links, plugin internals).
  if (
    cmd === "tray_app_page_seen" ||
    cmd === "take_tray_navigation" ||
    cmd === "peek_pending_deeplink" ||
    cmd === "ack_pending_deeplink" ||
    cmd === "check_for_updates" ||
    cmd === "update_tray_menu" ||
    cmd === "set_window_theme" ||
    cmd.startsWith("plugin:")
  ) {
    return null as T;
  }
  // Browser-side commands with no HTTP endpoint.
  if (cmd === "open_external") {
    return openExternalInBrowser(args.url) as T;
  }
  if (cmd === "stream_session_messages") {
    return (await streamSessionMessages(args)) as T;
  }
  if (cmd === "export_session_markdown") {
    downloadText(args.defaultName, args.content, "text/markdown;charset=utf-8");
    return args.defaultName as T;
  }
  const mapped = mapCommand(cmd, args);
  if (!mapped) throw new WebUnsupportedError(cmd);
  const init: RequestInit = {
    method: mapped.method,
    headers: { "Content-Type": "application/json" },
  };
  if (mapped.body !== undefined && mapped.method !== "GET") {
    init.body = JSON.stringify(mapped.body);
  }
  let res: Response;
  try {
    res = await fetch(mapped.url, init);
  } catch (e) {
    throw new Error(
      `Web API недоступно (${mapped.method} ${mapped.url}): ${String(e)}`,
    );
  }
  if (mapped.responseType === "arrayBuffer" && res.ok) {
    return (await res.arrayBuffer()) as T;
  }
  let payload: any = null;
  try {
    payload = await res.json();
  } catch {
    throw new Error(`Web API вернул не-JSON (${res.status} ${mapped.url})`);
  }
  if (!payload || payload.ok !== true) {
    throw new Error(payload?.error ?? `Web API ошибка (${cmd})`);
  }
  const data = payload.data;
  if (mapped.pick && data && typeof data === "object")
    return data[mapped.pick] as T;
  return data as T;
}
