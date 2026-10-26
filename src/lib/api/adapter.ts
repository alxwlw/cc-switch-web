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

export const isWeb =
  typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);

type Args = Record<string, any>;

interface Endpoint {
  method: "GET" | "POST" | "PUT" | "DELETE";
  url: string;
  body?: unknown;
  /** When set, resolve with `data[pick]` instead of the whole payload. */
  pick?: string;
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
