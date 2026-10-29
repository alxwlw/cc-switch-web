import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import "@testing-library/jest-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Sidebar } from "@/components/shell/Sidebar";
import { SettingsPage } from "@/components/settings/SettingsPage";
import { AuthSettingsPanel } from "@/components/providers/AuthSettingsPanel";
import {
  VIEW_STORAGE_KEY,
  isViewAvailable,
  readStoredView,
  resolveSettingsSection,
  type SettingsSection,
} from "@/lib/navigation";
import type { VisibleApps } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";
import { setWeb } from "../helpers/web";

const settingsState = vi.hoisted(() => ({
  value: {} as Record<string, unknown>,
  saveSettings: vi.fn(),
  browseAppConfigDir: vi.fn(),
  browseDirectory: vi.fn(),
  updateDirectory: vi.fn(),
}));

const platform = vi.hoisted(() => ({ linux: false }));
vi.mock("@/lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/platform")>()),
  isLinux: () => platform.linux,
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { resolvedLanguage: "en" },
  }),
}));
vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({ hasUpdate: false, updateInfo: null }),
}));
vi.mock("@/hooks/useSidebarStatus", () => ({
  useSidebarStatus: () => ({
    appStatus: () => ({ mode: "direct", alert: false }),
    todayCost: 0,
    todayTokens: 0,
    todayRequests: 0,
    todayHitRate: 0,
    authNeedsAttention: false,
  }),
}));
vi.mock("@/components/theme-provider", () => ({
  useTheme: () => ({ theme: "system", setTheme: vi.fn() }),
}));
vi.mock("@/hooks/useSettings", () => ({
  useSettings: () => ({
    settings: settingsState.value,
    isLoading: false,
    isSaving: false,
    isPortable: false,
    appConfigDir: "/app",
    initialAppConfigDir: "/app",
    resolvedDirs: { appConfig: "/app", claude: "/home/srv/.claude" },
    requiresRestart: false,
    updateSettings: vi.fn(),
    updateDirectory: settingsState.updateDirectory,
    updateAppConfigDir: vi.fn(),
    browseDirectory: settingsState.browseDirectory,
    browseAppConfigDir: settingsState.browseAppConfigDir,
    resetDirectory: vi.fn(),
    resetAppConfigDir: vi.fn(),
    saveSettings: settingsState.saveSettings,
    autoSaveSettings: vi.fn(),
    resetSettings: vi.fn(),
    acknowledgeRestart: vi.fn(),
  }),
}));
vi.mock("@/hooks/useImportExport", () => ({
  useImportExport: () => ({
    selectedFile: null,
    status: "idle",
    errorMessage: null,
    backupId: null,
    isImporting: false,
    selectImportFile: vi.fn(),
    importConfig: vi.fn(),
    exportConfig: vi.fn(),
    clearSelection: vi.fn(),
    resetStatus: vi.fn(),
  }),
}));
vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({ data: settingsState.value }),
}));
const revealAppLogFile = vi.hoisted(() => vi.fn());
vi.mock("@/lib/api", () => ({
  settingsApi: { restart: vi.fn(), revealAppLogFile },
}));
vi.mock("@/components/settings/sections/RoutingSection", () => ({
  RoutingSection: () => <div>routing-section</div>,
  RectifierBlock: () => <div>rectifier-block</div>,
}));
vi.mock("@/components/settings/ServerLogViewer", () => ({
  ServerLogViewer: ({ open }: { open: boolean }) =>
    open ? <div>server-log-viewer</div> : null,
}));
vi.mock("@/components/settings/GlobalProxySettings", () => ({
  GlobalProxySettings: () => <div>global-proxy-settings</div>,
}));
vi.mock("@/components/settings/ImportExportSection", () => ({
  ImportExportSection: () => <div>import-export-section</div>,
}));
vi.mock("@/components/settings/BackupListSection", () => ({
  BackupListSection: () => <div>backup-list-section</div>,
}));
vi.mock("@/components/settings/WebdavSyncSection", () => ({
  WebdavSyncSection: () => <div>webdav-section</div>,
}));
vi.mock("@/components/settings/LogConfigPanel", () => ({
  LogConfigPanel: () => <div>log-config-panel</div>,
}));
vi.mock("@/components/settings/CodexAuthSettings", () => ({
  CodexAuthSettings: () => <div>codex-auth-settings</div>,
}));
vi.mock("@/components/settings/AuthCenterPanel", () => ({
  AuthCenterPanel: () => <div>auth-center-panel</div>,
}));
vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({ isOpen, children }: any) =>
    isOpen ? <div>{children}</div> : null,
}));
vi.mock("@/components/settings/AboutSection", () => ({
  AboutSection: () => <div>about-section</div>,
}));

const SECTIONS: SettingsSection[] = [
  "general",
  "appConfig",
  "routing",
  "network",
  "data",
  "about",
];
const WEB_HIDDEN: SettingsSection[] = ["network"];

beforeEach(() => {
  platform.linux = false;
  settingsState.value = {
    language: "en",
    minimizeToTrayOnClose: true,
    launchOnStartup: true,
    silentStartup: false,
    enableClaudePluginIntegration: false,
    skipClaudeOnboarding: false,
    claudeConfigDir: "/home/srv/.claude",
  };
  settingsState.saveSettings.mockReset();
  settingsState.browseAppConfigDir.mockReset();
  settingsState.browseDirectory.mockReset();
  revealAppLogFile.mockReset();
  localStorage.clear();
});

const renderPage = (section: SettingsSection) =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <SettingsPage
        section={section}
        onOpenApps={vi.fn()}
        onOpenApp={vi.fn()}
      />
    </QueryClientProvider>,
  );

const renderSidebar = (
  view: "usage" | "settings",
  settingsSection: SettingsSection = "general",
) =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <Sidebar
        activeApp="claude"
        view={view}
        visibleApps={{} as VisibleApps}
        settingsSection={settingsSection}
        onSelectApp={vi.fn()}
        onSelectPage={vi.fn()}
        onSelectSettingsSection={vi.fn()}
        onExitSettings={vi.fn()}
      />
    </QueryClientProvider>,
  );

describe("navigation helpers · WEB-00", () => {
  it.each(SECTIONS)("web: resolveSettingsSection(%s)", (section) => {
    setWeb(true);
    expect(resolveSettingsSection(section)).toBe(
      WEB_HIDDEN.includes(section) ? "general" : section,
    );
  });

  it.each(SECTIONS)("desktop: resolveSettingsSection(%s) is identity", (s) => {
    expect(resolveSettingsSection(s)).toBe(s);
  });

  it("web: a stored hidden auth view is not restored", () => {
    localStorage.setItem(VIEW_STORAGE_KEY, "auth");
    setWeb(true);
    expect(isViewAvailable("auth")).toBe(false);
    expect(readStoredView()).toBe("providers");
  });

  it.each([true, false])(
    "the skills discovery view is available and restored (web=%s)",
    (web) => {
      localStorage.setItem(VIEW_STORAGE_KEY, "skillsDiscovery");
      setWeb(web);
      expect(isViewAvailable("skillsDiscovery")).toBe(true);
      expect(readStoredView()).toBe("skillsDiscovery");
    },
  );

  it("desktop: a stored auth view is restored", () => {
    localStorage.setItem(VIEW_STORAGE_KEY, "auth");
    expect(isViewAvailable("auth")).toBe(true);
    expect(readStoredView()).toBe("auth");
  });
});

describe("Sidebar · WEB-00", () => {
  it("web: no auth entry in the global list", () => {
    setWeb(true);
    renderSidebar("usage");
    expect(screen.queryByRole("button", { name: /nav\.auth/ })).toBeNull();
    expect(screen.getByRole("button", { name: /nav\.usage/ })).toBeVisible();
  });

  it("desktop: auth entry is present", () => {
    renderSidebar("usage");
    expect(screen.getByRole("button", { name: /nav\.auth/ })).toBeVisible();
  });

  it("web: settings directory lists only working sections", () => {
    setWeb(true);
    renderSidebar("settings");
    const group = screen.getByRole("group", { name: "nav.settings" });
    const names = within(group)
      .getAllByRole("button")
      .map((b) => b.textContent);
    expect(names).toEqual([
      "settings.sections.general",
      "settings.sections.appConfig",
      "settings.sections.routing",
      "settings.sections.data",
      "settings.sections.about",
    ]);
  });

  it("desktop: settings directory lists all six sections", () => {
    renderSidebar("settings");
    const group = screen.getByRole("group", { name: "nav.settings" });
    expect(within(group).getAllByRole("button")).toHaveLength(6);
  });
});

describe("Sidebar settings highlight · WEB-00", () => {
  const current = () =>
    within(screen.getByRole("group", { name: "nav.settings" }))
      .getAllByRole("button")
      .filter((b) => b.getAttribute("aria-current") === "page")
      .map((b) => b.textContent);

  it("web: a requested hidden section highlights general", () => {
    setWeb(true);
    renderSidebar("settings", "network");
    expect(current()).toEqual(["settings.sections.general"]);
  });

  it("desktop: routing highlights routing", () => {
    renderSidebar("settings", "routing");
    expect(current()).toEqual(["settings.sections.routing"]);
  });
});

describe("SettingsPage · WEB-00", () => {
  it.each(WEB_HIDDEN)(
    "web: requested hidden section %s lands on general",
    (s) => {
      setWeb(true);
      renderPage(s);
      expect(
        screen.getByRole("heading", {
          level: 1,
          name: "settings.sections.general",
        }),
      ).toBeInTheDocument();
      expect(screen.getByText("settings.general.appearance")).toBeVisible();
      expect(screen.queryByText("global-proxy-settings")).toBeNull();
    },
  );

  it("web: routing shows only the rectifier block", () => {
    setWeb(true);
    renderPage("routing");
    expect(screen.getByText("rectifier-block")).toBeInTheDocument();
    expect(screen.queryByText("routing-section")).toBeNull();
  });

  it("desktop: routing shows the full routing section", () => {
    renderPage("routing");
    expect(screen.getByText("routing-section")).toBeInTheDocument();
    expect(screen.queryByText("rectifier-block")).toBeNull();
  });

  it("web: data shows backups, import/export and the log block; location and cloud sync stay hidden", async () => {
    setWeb(true);
    renderPage("data");
    expect(screen.getByText("log-config-panel")).toBeInTheDocument();
    expect(screen.getByText("backup-list-section")).toBeInTheDocument();
    expect(
      screen.getByText("settings.advanced.backup.title"),
    ).toBeInTheDocument();
    expect(screen.queryByText("settings.data.location")).toBeNull();
    expect(screen.getByText("import-export-section")).toBeInTheDocument();
    expect(screen.queryByText("webdav-section")).toBeNull();
    expect(screen.queryByText("server-log-viewer")).toBeNull();
    await userEvent.click(
      screen.getByRole("button", {
        name: "settings.advanced.logConfig.openLog",
      }),
    );
    expect(screen.getByText("server-log-viewer")).toBeInTheDocument();
  });

  it("desktop: data shows import/export and the log button reveals the file", async () => {
    renderPage("data");
    expect(screen.getByText("import-export-section")).toBeInTheDocument();
    expect(screen.getByText("backup-list-section")).toBeInTheDocument();
    expect(screen.getByText("webdav-section")).toBeInTheDocument();
    expect(screen.getByText("settings.data.location")).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", {
        name: "settings.advanced.logConfig.openLog",
      }),
    );
    expect(screen.queryByText("server-log-viewer")).toBeNull();
    expect(revealAppLogFile).toHaveBeenCalledTimes(1);
  });

  it("desktop: network section still renders", () => {
    renderPage("network");
    expect(screen.getByText("global-proxy-settings")).toBeInTheDocument();
  });

  it("web: launch-on-startup and silent-startup toggles are disabled", () => {
    setWeb(true);
    renderPage("general");
    expect(
      screen.getByRole("switch", { name: "settings.launchOnStartup" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("switch", { name: "settings.silentStartup" }),
    ).toBeDisabled();
    // рабочие тоглы остаются активными
    expect(
      screen.getByRole("switch", { name: "settings.general.checkToolUpdates" }),
    ).toBeEnabled();
  });

  it("desktop: launch-on-startup toggle is enabled", () => {
    renderPage("general");
    expect(
      screen.getByRole("switch", { name: "settings.launchOnStartup" }),
    ).toBeEnabled();
  });

  it("web: tray, window-control and terminal settings are disabled", () => {
    setWeb(true);
    renderPage("general");
    expect(
      screen.getByRole("switch", { name: "settings.minimizeToTray" }),
    ).toBeDisabled();
    for (const name of [
      "settings.general.trayStylePanel",
      "settings.general.trayStyleMenu",
    ]) {
      expect(screen.getByRole("button", { name })).toBeDisabled();
    }
    expect(
      screen.getByRole("combobox", { name: "settings.terminal.title" }),
    ).toBeDisabled();
  });

  it("desktop: tray and terminal settings are enabled", () => {
    renderPage("general");
    expect(
      screen.getByRole("switch", { name: "settings.minimizeToTray" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "settings.general.trayStyleMenu" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("combobox", { name: "settings.terminal.title" }),
    ).toBeEnabled();
  });

  it("web + Linux: the app window controls switch is disabled with the tip", async () => {
    setWeb(true);
    platform.linux = true;
    renderPage("general");
    const sw = screen.getByRole("switch", {
      name: "settings.useAppWindowControls",
    });
    expect(sw).toBeDisabled();
    await userEvent.hover(sw.parentElement as HTMLElement);
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "web.unavailable",
    );
  });

  it("desktop + Linux: the app window controls switch is enabled", () => {
    platform.linux = true;
    renderPage("general");
    expect(
      screen.getByRole("switch", { name: "settings.useAppWindowControls" }),
    ).toBeEnabled();
  });

  it("web: a disabled toggle explains itself on hover", async () => {
    setWeb(true);
    renderPage("general");
    await userEvent.hover(
      screen.getByRole("switch", { name: "settings.launchOnStartup" })
        .parentElement as HTMLElement,
    );
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "web.unavailable",
    );
  });

  it("web: appConfig keeps manual paths, disables Browse, shows Claude toggles and Codex block, says paths are on the server", async () => {
    setWeb(true);
    renderPage("appConfig");

    expect(
      screen.getByRole("switch", {
        name: "settings.enableClaudePluginIntegration",
      }),
    ).toBeEnabled();
    expect(
      screen.getByRole("switch", { name: "settings.skipClaudeOnboarding" }),
    ).toBeEnabled();
    expect(screen.getByText("codex-auth-settings")).toBeInTheDocument();
    expect(screen.getByRole("note")).toHaveTextContent("web.serverPath");

    const browse = screen.getAllByRole("button", {
      name: "settings.browseDirectory",
    });
    expect(browse.length).toBeGreaterThan(0);
    for (const button of browse) expect(button).toBeDisabled();

    // ручной ввод пути работает
    const input = screen.getByDisplayValue("/home/srv/.claude");
    expect(input).toBeEnabled();
    await userEvent.type(input, "x");
    expect(settingsState.updateDirectory).toHaveBeenCalled();
  });

  it("desktop: appConfig keeps Browse enabled, Codex block and no server hint", () => {
    renderPage("appConfig");
    expect(screen.getByText("codex-auth-settings")).toBeInTheDocument();
    expect(screen.queryByRole("note")).toBeNull();
    expect(
      screen.getByRole("switch", { name: "settings.skipClaudeOnboarding" }),
    ).toBeEnabled();
    for (const button of screen.getAllByRole("button", {
      name: "settings.browseDirectory",
    }))
      expect(button).toBeEnabled();
  });
});

describe("AuthSettingsPanel · WEB-00", () => {
  it("web: provider-form account links show a notice, not the auth center", () => {
    setWeb(true);
    render(<AuthSettingsPanel target="codex_oauth" onClose={vi.fn()} />);
    expect(screen.getByRole("status")).toHaveTextContent("web.unavailable");
    expect(screen.queryByText("auth-center-panel")).toBeNull();
  });

  it("desktop: renders the auth center", () => {
    render(<AuthSettingsPanel target="codex_oauth" onClose={vi.fn()} />);
    expect(screen.getByText("auth-center-panel")).toBeInTheDocument();
  });
});
