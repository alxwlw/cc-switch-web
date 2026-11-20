import path from "node:path";
import ts from "typescript";
import { describe, expect, it } from "vitest";
import { isWebHandled } from "@/lib/api/adapter";
import { frontendFiles, walk } from "./support/frontendSources";

const ROOT = path.resolve(__dirname, "../..");

/** Commands that never get a web route: the desktop shell owns them. */
const DESKTOP_ONLY = new Set([
  // tray
  "tray_panel_action",
  "tray_panel_hide",
  "tray_panel_layout",
  "tray_panel_placement",
  "tray_panel_state",
  "set_auto_launch",
  // native dialogs and file paths (the web build uploads and downloads instead)
  "open_file_dialog",
  "open_zip_file_dialog",
  "save_file_dialog",
  "pick_directory",
  "export_config_to_file",
  "import_config_from_file",
  "install_skills_from_zip",
  // file manager on the server host
  "open_app_config_folder",
  "open_cc_switch_skills_dir",
  "open_config_folder",
  "open_workspace_directory",
  "open_skill_dir",
  "reveal_app_log_file",
  "reveal_backup_location",
  "reveal_session_path",
  // terminals and other apps on the server host
  "launch_hermes_dashboard",
  "launch_session_terminal",
  "open_provider_terminal",
  "workbuddy_account_open_app",
  "workbuddy_account_prepare_new_login",
  // desktop app updates (the web build updates itself through /api/update/*)
  "check_app_update_available",
  "install_update_and_restart",
  "open_portable_update_download",
  "get_auto_update_status",
  "download_app_update",
  // gated by isWeb in the UI, out of scope of the parity plan
  "copilot_get_usage",
  "copilot_get_usage_for_account",
]);

/** v4.0.35 commands still to be mapped; each task removes the names it maps. */
const PENDING = new Set([
  // Task 2
  "get_claude_account_quota",
  "get_claude_default_plan",
  "get_claude_login_switch",
  "claude_account_submit_paste",
  // Task 3
  "workbuddy_account_import_current",
  "workbuddy_account_live_state",
  "workbuddy_account_quota",
  "workbuddy_model_owners",
  // Task 4
  "dsh_account_login_start",
  "dsh_account_login_poll",
  "dsh_account_login_cancel",
  "dsh_account_sign_out",
  "dsh_account_status",
  "copilot_cli_signed_in_account",
  // Task 5
  "get_privacy_filter_config",
  "set_privacy_filter_config",
  "get_config_status",
  "webdav_sync_clear_settings",
  "s3_sync_clear_settings",
  // Task 6
  "sync_session_index",
  "search_session_content",
  "clear_session_index",
  // Task 7
  "get_skill_repo_tokens",
  "set_skill_repo_token",
  "delete_skill_repo_token",
  "discover_repo_skills",
  "refresh_local_skill_metadata",
  // Task 8
  "start_mcp_connectivity_checks",
  "cancel_mcp_connectivity_checks",
  // Task 9
  "diagnose_ip",
  "fetch_exchange_rates",
  "fetch_models_dev",
]);

/** Every `invoke("name")` / `x.invoke<T>("name")` under src/, minus the adapter itself. */
function invokedCommands(): Set<string> {
  const adapter = path.join(ROOT, "src", "lib", "api", "adapter.ts");
  const names = new Set<string>();
  for (const file of frontendFiles(path.join(ROOT, "src"))) {
    if (path.resolve(file.fileName) === adapter) continue;
    walk(file, (node) => {
      if (!ts.isCallExpression(node)) return;
      const callee = node.expression;
      const isInvoke =
        (ts.isIdentifier(callee) && callee.text === "invoke") ||
        (ts.isPropertyAccessExpression(callee) &&
          callee.name.text === "invoke");
      const first = node.arguments[0];
      if (isInvoke && first && ts.isStringLiteralLike(first)) {
        names.add(first.text);
      }
    });
  }
  return names;
}

const invoked = invokedCommands();
const listed = [...DESKTOP_ONLY, ...PENDING];

describe("web coverage of invoked commands", () => {
  it("finds the invoked commands", () => {
    expect(invoked.size).toBeGreaterThan(300);
    expect(isWebHandled("get_providers")).toBe(true);
    expect(isWebHandled("tray_panel_state")).toBe(false);
  });

  it("every invoked command works in web mode or is listed", () => {
    const unlisted = [...invoked].filter(
      (c) => !isWebHandled(c) && !DESKTOP_ONLY.has(c) && !PENDING.has(c),
    );
    expect(unlisted).toEqual([]);
  });

  it("listed commands are not mapped", () => {
    expect(listed.filter(isWebHandled)).toEqual([]);
  });

  it("listed commands are still invoked", () => {
    expect(listed.filter((c) => !invoked.has(c))).toEqual([]);
  });
});
