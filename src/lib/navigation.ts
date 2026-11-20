import type { AppId } from "@/lib/api";
import { isWeb } from "@/lib/api/adapter";

/**
 * 主窗口的导航模型（v7）：侧栏选一个应用或一个全局页；设置单独占一屏。
 *
 * - 应用页：点侧栏应用行进入供应商页；OpenClaw、Hermes 在页头下用分段控件切到自己的专属页。
 * - 全局页：用量统计、授权中心、MCP、Skills、提示词、会话、应用（安装与显示），每样只出现一次。
 * - 设置：侧栏换成设置目录，6 个分组。
 */

export type AppPage =
  | "providers"
  | "workspace"
  | "openclawConfig"
  | "hermesMemory";

export type GlobalPage =
  | "usage"
  | "auth"
  | "mcp"
  | "skills"
  | "skillsDiscovery"
  | "prompts"
  | "sessions"
  | "apps";

export type View = AppPage | GlobalPage | "settings";

export type SettingsSection =
  | "general"
  | "appConfig"
  | "routing"
  | "network"
  | "data"
  | "about";

export const SETTINGS_SECTIONS: SettingsSection[] = [
  "general",
  "appConfig",
  "routing",
  "network",
  "data",
  "about",
];

// WEB-00: sections made entirely of desktop-only commands are hidden in web. network was opened by WEB-23 (the list is empty,
// the gate point stays here). routing was opened by WEB-01 (the failover block is hidden until phase 28), data was opened by
// WEB-24, its blocks were lifted by WEB-26/09/10/11. isWeb is read at call time.
const WEB_HIDDEN_SETTINGS_SECTIONS: SettingsSection[] = [];

export function isSettingsSectionAvailable(section: SettingsSection): boolean {
  return !(isWeb && WEB_HIDDEN_SETTINGS_SECTIONS.includes(section));
}

/** A section hidden in web (requested, saved, from a link) opens as `general`. */
export function resolveSettingsSection(
  section: SettingsSection,
): SettingsSection {
  return isSettingsSectionAvailable(section) ? section : "general";
}

// WEB-00: screens made entirely of desktop-only commands are hidden in web through this list.
// Empty now: auth was opened by WEB-02 (Copilot/Codex/xAI in stage 1, Kimi in stage 2),
// skillsDiscovery — WEB-05, workspace/openclawConfig — WEB-15, hermesMemory — WEB-16.
const WEB_HIDDEN_VIEWS: View[] = [];

export function isViewAvailable(view: View): boolean {
  return !(isWeb && WEB_HIDDEN_VIEWS.includes(view));
}

const APP_PAGES: AppPage[] = [
  "providers",
  "workspace",
  "openclawConfig",
  "hermesMemory",
];

const GLOBAL_PAGES: GlobalPage[] = [
  "usage",
  "auth",
  "mcp",
  "skills",
  "skillsDiscovery",
  "prompts",
  "sessions",
  "apps",
];

export const VIEW_STORAGE_KEY = "cc-switch-last-view";
export const APP_STORAGE_KEY = "cc-switch-last-app";

/** 旧版视图名 → 新视图（OpenClaw 的三个配置页合成了一个）。 */
const LEGACY_VIEWS: Record<string, View> = {
  openclawEnv: "openclawConfig",
  openclawTools: "openclawConfig",
  openclawAgents: "openclawConfig",
};

export function isAppPage(view: View): view is AppPage {
  return (APP_PAGES as string[]).includes(view);
}

export function parseView(value: string | null | undefined): View | null {
  if (!value) return null;
  if (value in LEGACY_VIEWS) return LEGACY_VIEWS[value];
  if (value === "settings") return "settings";
  if ((APP_PAGES as string[]).includes(value)) return value as AppPage;
  if ((GLOBAL_PAGES as string[]).includes(value)) return value as GlobalPage;
  return null;
}

/** 应用专属页只对它所属的应用成立；换了应用就回到供应商页。 */
export function appPageBelongsTo(page: AppPage, app: AppId): boolean {
  switch (page) {
    case "providers":
      return true;
    case "workspace":
    case "openclawConfig":
      return app === "openclaw";
    case "hermesMemory":
      return app === "hermes";
  }
}

export function readStoredView(): View {
  try {
    const stored =
      parseView(localStorage.getItem(VIEW_STORAGE_KEY)) ?? "providers";
    // WEB-00: a saved view from WEB_HIDDEN_VIEWS is not restored in web (the list is empty now, auth was opened by WEB-02, phase 25)
    return isViewAvailable(stored) ? stored : "providers";
  } catch {
    return "providers";
  }
}

export function storeView(view: View) {
  try {
    localStorage.setItem(VIEW_STORAGE_KEY, view);
  } catch {
    // 存不了就算了：下次启动回到供应商页
  }
}

/** 提示词、会话页在 Claude Desktop 上看的是 Claude Code 的内容（两者共用）。 */
export function sharedFeatureAppOf(app: AppId): AppId {
  return app === "claude-desktop" ? "claude" : app;
}
