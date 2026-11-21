import path from "node:path";
import ts from "typescript";
import { describe, expect, it } from "vitest";
import { APP_IDS_WITH } from "@/config/appSpecs";
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

/**
 * `invoke` calls whose first argument is not a plain string literal, so the
 * walker cannot read the command name. Each entry names the file and the exact
 * expression, and lists the concrete commands that site can produce; they are
 * checked like the literal names. A new computed call fails the guard until it
 * is added here (or rewritten with a literal).
 */
const COMPUTED_INVOKES: {
  file: string;
  expression: string;
  /** Where the values come from, then the names they give. */
  names: () => string[];
}[] = [
  {
    file: "src/lib/api/pi.ts",
    expression: "`get_${app}_current_state`",
    // `app` is an AppIdWith<"piFamily">: every app with the piFamily capability
    names: () => APP_IDS_WITH.piFamily.map((app) => `get_${app}_current_state`),
  },
];

interface Invocations {
  literals: Set<string>;
  /** "<repo-relative file>: <first argument source text>" */
  computed: Set<string>;
}

/** Every `invoke(...)` / `x.invoke<T>(...)` under src/, minus the adapter itself. */
function invocations(): Invocations {
  const adapter = path.join(ROOT, "src", "lib", "api", "adapter.ts");
  const literals = new Set<string>();
  const computed = new Set<string>();
  for (const file of frontendFiles(path.join(ROOT, "src"))) {
    if (path.resolve(file.fileName) === adapter) continue;
    const rel = path.relative(ROOT, file.fileName).split(path.sep).join("/");
    walk(file, (node) => {
      if (!ts.isCallExpression(node)) return;
      const callee = node.expression;
      const isInvoke =
        (ts.isIdentifier(callee) && callee.text === "invoke") ||
        (ts.isPropertyAccessExpression(callee) &&
          callee.name.text === "invoke");
      const first = node.arguments[0];
      if (!isInvoke || !first) return;
      if (ts.isStringLiteralLike(first)) literals.add(first.text);
      else computed.add(`${rel}: ${first.getText(file)}`);
    });
  }
  return { literals, computed };
}

const found = invocations();
const allowedComputed = COMPUTED_INVOKES.map(
  (c) => `${c.file}: ${c.expression}`,
);
const invoked = new Set([
  ...found.literals,
  ...COMPUTED_INVOKES.flatMap((c) => c.names()),
]);
const listed = [...DESKTOP_ONLY];

describe("web coverage of invoked commands", () => {
  it("finds the invoked commands", () => {
    expect(invoked.size).toBeGreaterThan(300);
    expect(isWebHandled("get_providers")).toBe(true);
    expect(isWebHandled("tray_panel_state")).toBe(false);
  });

  it("every computed invoke call is allowlisted, and every allowlist entry exists", () => {
    expect([...found.computed].sort()).toEqual([...allowedComputed].sort());
  });

  it("expands the allowlisted call to every piFamily app", () => {
    expect(COMPUTED_INVOKES[0].names()).toContain("get_omp_current_state");
    expect(invoked.has("get_pi_current_state")).toBe(true);
  });

  it("every invoked command works in web mode or is listed", () => {
    const unlisted = [...invoked].filter(
      (c) => !isWebHandled(c) && !DESKTOP_ONLY.has(c),
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
