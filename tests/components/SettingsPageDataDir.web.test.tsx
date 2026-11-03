import { QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import "@testing-library/jest-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsPage } from "@/components/settings/SettingsPage";
import { DirectoryInput } from "@/components/settings/DirectorySettings";
import { RESTART_POLL_INTERVAL_MS } from "@/hooks/useServerRestart";
import type { AppConfigDirStatus } from "@/lib/api/settings";
import { createTestQueryClient } from "../utils/testQueryClient";
import { setWeb } from "../helpers/web";

// WEB-26 in the page: the data directory block and the restart flow. The hooks that talk to the
// server are replaced by state; the restart goes through the real adapter into a stubbed fetch.

vi.mock("@tauri-apps/api/core", async () => {
  const adapter =
    await vi.importActual<typeof import("@/lib/api/adapter")>(
      "@/lib/api/adapter",
    );
  return { invoke: adapter.invoke };
});

const reloadMock = vi.hoisted(() => vi.fn());
vi.mock("@/lib/pageReload", () => ({ reloadPage: reloadMock }));

const toastMock = vi.hoisted(() => ({
  error: vi.fn(),
  success: vi.fn(),
}));
vi.mock("@/lib/toast", () => ({ toast: toastMock }));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options?.path ? `${key}|${options.path}|${options.source}` : key,
    i18n: { resolvedLanguage: "en" },
  }),
}));

const page = vi.hoisted(() => ({
  status: null as unknown,
  appConfigDir: undefined as string | undefined,
  initialAppConfigDir: undefined as string | undefined,
  resolved: "/srv/home/.cc-switch",
  requiresRestart: false,
  updateAppConfigDir: vi.fn(),
  resetAppConfigDir: vi.fn(),
  browseAppConfigDir: vi.fn(),
  saveSettings: vi.fn(),
  acknowledgeRestart: vi.fn(),
}));

vi.mock("@/hooks/useSettings", () => ({
  useSettings: () => ({
    settings: { language: "en" },
    isLoading: false,
    isSaving: false,
    isPortable: false,
    appConfigDir: page.appConfigDir,
    initialAppConfigDir: page.initialAppConfigDir,
    appConfigDirStatus: page.status,
    resolvedDirs: { appConfig: page.resolved },
    requiresRestart: page.requiresRestart,
    updateSettings: vi.fn(),
    updateDirectory: vi.fn(),
    updateAppConfigDir: page.updateAppConfigDir,
    browseDirectory: vi.fn(),
    browseAppConfigDir: page.browseAppConfigDir,
    resetDirectory: vi.fn(),
    resetAppConfigDir: page.resetAppConfigDir,
    saveSettings: page.saveSettings,
    autoSaveSettings: vi.fn(),
    resetSettings: vi.fn(),
    acknowledgeRestart: page.acknowledgeRestart,
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
  useSettingsQuery: () => ({ data: { language: "en" } }),
}));
vi.mock("@/components/settings/ServerLogViewer", () => ({
  ServerLogViewer: () => null,
}));
vi.mock("@/components/settings/ImportExportSection", () => ({
  ImportExportSection: () => null,
}));
vi.mock("@/components/settings/BackupListSection", () => ({
  BackupListSection: () => null,
}));
vi.mock("@/components/settings/WebdavSyncSection", () => ({
  WebdavSyncSection: () => null,
}));
vi.mock("@/components/settings/LogConfigPanel", () => ({
  LogConfigPanel: () => null,
}));
vi.mock("@/components/deeplink/DeepLinkPasteEntry", () => ({
  DeepLinkPasteEntry: () => null,
}));

const OLD = "old-process";
const NEW = "new-process";

const status = (
  overrides: Partial<AppConfigDirStatus> = {},
): AppConfigDirStatus => ({
  effective: "/srv/home/.cc-switch",
  source: "default",
  override: null,
  envLocked: false,
  default: "/srv/home/.cc-switch",
  ...overrides,
});

const json = (code: number, body: unknown) => ({
  ok: code < 300,
  status: code,
  json: async () => body,
});

const renderPage = () =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <SettingsPage section="data" onOpenApps={vi.fn()} onOpenApp={vi.fn()} />
    </QueryClientProvider>,
  );

describe("SettingsPage data directory · WEB-26", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  let health: () => unknown;
  let restartResponse: () => unknown;

  const calls = (url: string) => fetchSpy.mock.calls.filter(([u]) => u === url);

  beforeEach(() => {
    setWeb(true);
    page.status = status();
    page.appConfigDir = undefined;
    page.initialAppConfigDir = undefined;
    page.requiresRestart = false;
    page.saveSettings.mockReset();
    page.updateAppConfigDir.mockReset();
    page.resetAppConfigDir.mockReset();
    page.browseAppConfigDir.mockReset();
    page.acknowledgeRestart.mockReset();
    reloadMock.mockReset();
    toastMock.error.mockReset();
    health = () => json(200, { status: "ok", instanceId: OLD });
    restartResponse = () => json(202, { ok: true, data: { instanceId: OLD } });
    fetchSpy = vi.fn(async (url: string) =>
      url === "/api/restart" ? restartResponse() : health(),
    );
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("shows an editable path with the server's source line and an honest scope note", async () => {
    page.status = status({
      source: "override",
      effective: "/srv/data",
      override: "/srv/data",
    });
    page.appConfigDir = "/srv/data";
    page.initialAppConfigDir = "/srv/data";
    renderPage();

    const input = screen.getByDisplayValue("/srv/data");
    expect(input).toBeEnabled();
    expect(input).not.toHaveAttribute("readonly");
    await userEvent.type(input, "x");
    expect(page.updateAppConfigDir).toHaveBeenCalled();

    expect(screen.getByTestId("app-config-dir-current")).toHaveTextContent(
      "web.dataDir.current|/srv/data|web.dataDir.source.override",
    );
    expect(screen.getByTestId("app-config-dir-scope")).toHaveTextContent(
      "web.dataDir.scope",
    );
    expect(screen.queryByTestId("app-config-dir-locked")).toBeNull();
    // no native dialog in a browser; the reset goes back to the server's default
    expect(
      screen.queryByRole("button", { name: "settings.browseDirectory" }),
    ).toBeNull();
    await userEvent.click(
      screen.getByRole("button", { name: "settings.resetDefault" }),
    );
    expect(page.resetAppConfigDir).toHaveBeenCalledTimes(1);
    // nothing changed: no save button in the block
    expect(screen.queryByRole("button", { name: "common.save" })).toBeNull();
  });

  it("desktop: the same block keeps Browse and shows no server notes", () => {
    setWeb(false);
    page.appConfigDir = "/mac/data";
    page.initialAppConfigDir = "/mac/data";
    page.status = null;
    renderPage();
    expect(
      screen.getByRole("button", { name: "settings.browseDirectory" }),
    ).toBeEnabled();
    expect(screen.queryByTestId("app-config-dir-scope")).toBeNull();
    expect(screen.queryByTestId("app-config-dir-current")).toBeNull();
  });

  it("a read-only DirectoryInput has neither Browse nor reset, on desktop too", () => {
    setWeb(false);
    render(
      <DirectoryInput
        label=""
        value="/fixed"
        resolvedValue="/fixed"
        readOnly
        onChange={vi.fn()}
        onBrowse={vi.fn()}
        onReset={vi.fn()}
      />,
    );
    expect(screen.getByDisplayValue("/fixed")).toHaveAttribute("readonly");
    expect(
      screen.queryByRole("button", { name: "settings.browseDirectory" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "settings.resetDefault" }),
    ).toBeNull();
  });

  it("offers save only for a changed directory", () => {
    page.appConfigDir = "/srv/elsewhere";
    renderPage();
    expect(
      screen.getByRole("button", { name: "common.save" }),
    ).toBeInTheDocument();
  });

  it("a directory fixed by the env is read-only: value, source and reason, no browse, reset or save", () => {
    page.status = status({
      source: "env",
      envLocked: true,
      effective: "/srv/.cc-switch-web",
    });
    page.resolved = "/srv/.cc-switch-web";
    // even a stale draft must not light up the block's save button
    page.appConfigDir = "/draft";
    page.initialAppConfigDir = undefined;
    renderPage();

    const input = screen.getByDisplayValue("/draft");
    expect(input).toHaveAttribute("readonly");
    expect(screen.getByTestId("app-config-dir-current")).toHaveTextContent(
      "web.dataDir.current|/srv/.cc-switch-web|web.dataDir.source.env",
    );
    expect(screen.getByTestId("app-config-dir-locked")).toHaveTextContent(
      "web.dataDir.envLocked",
    );
    expect(
      screen.queryByRole("button", { name: "settings.resetDefault" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "settings.browseDirectory" }),
    ).toBeNull();
    expect(screen.queryByRole("button", { name: "common.save" })).toBeNull();
    page.resolved = "/srv/home/.cc-switch";
  });

  describe("restart dialog", () => {
    beforeEach(() => {
      vi.useFakeTimers({ shouldAdvanceTime: false });
      page.requiresRestart = true;
    });

    const tick = (ms: number) =>
      act(async () => {
        await vi.advanceTimersByTimeAsync(ms);
      });

    it("restarts the server in a dev build too, waits for the new process, then reloads; the button stays blocked meanwhile", async () => {
      // vitest runs with import.meta.env.DEV === true: the desktop branch would only toast
      expect(import.meta.env.DEV).toBe(true);
      renderPage();
      await tick(0);
      expect(
        screen.getByText("web.dataDir.restartRequiredMessage"),
      ).toBeVisible();

      const now = screen.getByRole("button", { name: "settings.restartNow" });
      await act(async () => {
        now.click();
        now.click();
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(calls("/api/restart")).toHaveLength(1);
      const [, init] = calls("/api/restart")[0];
      expect(init.method).toBe("POST");
      expect(init.headers["Content-Type"]).toBe("application/json");

      const busy = screen.getByRole("button", {
        name: /web\.dataDir\.restarting/,
      });
      expect(busy).toBeDisabled();
      expect(
        screen.getByRole("button", { name: "settings.restartLater" }),
      ).toBeDisabled();
      expect(page.acknowledgeRestart).not.toHaveBeenCalled();
      // Escape does not dismiss the dialog while the server is going down
      fireEvent.keyDown(document.body, { key: "Escape" });
      await tick(0);
      expect(busy).toBeVisible();
      expect(page.acknowledgeRestart).not.toHaveBeenCalled();

      await tick(RESTART_POLL_INTERVAL_MS);
      expect(reloadMock).not.toHaveBeenCalled();
      health = () => json(200, { status: "ok", instanceId: NEW });
      await tick(RESTART_POLL_INTERVAL_MS);

      expect(reloadMock).toHaveBeenCalledTimes(1);
      expect(page.acknowledgeRestart).toHaveBeenCalled();
      expect(calls("/api/restart")).toHaveLength(1);
    });

    it("Escape closes the dialog when no restart is running", async () => {
      renderPage();
      await tick(0);
      expect(
        screen.getByText("web.dataDir.restartRequiredMessage"),
      ).toBeVisible();
      fireEvent.keyDown(document.body, { key: "Escape" });
      await tick(0);
      expect(
        screen.queryByText("web.dataDir.restartRequiredMessage"),
      ).toBeNull();
      expect(page.acknowledgeRestart).toHaveBeenCalledTimes(1);
      expect(calls("/api/restart")).toHaveLength(0);
    });

    it("409 shows the server's text and closes the dialog without a reload", async () => {
      restartResponse = () =>
        json(409, { ok: false, error: "перезапустите сервер вручную" });
      renderPage();
      await tick(0);
      await act(async () => {
        screen.getByRole("button", { name: "settings.restartNow" }).click();
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(toastMock.error).toHaveBeenCalledWith(
        "перезапустите сервер вручную",
      );
      expect(reloadMock).not.toHaveBeenCalled();
      expect(calls("/api/health")).toHaveLength(0);
      expect(page.acknowledgeRestart).toHaveBeenCalled();
      expect(
        screen.queryByText("web.dataDir.restartRequiredMessage"),
      ).toBeNull();
    });
  });
});
