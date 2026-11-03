import { renderHook, act } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useSettings } from "@/hooks/useSettings";
import type { Settings } from "@/types";
import { setWeb } from "../helpers/web";

const mutateAsyncMock = vi.fn();
const useSettingsQueryMock = vi.fn();
const setAppConfigDirOverrideMock = vi.fn();
const setAutoLaunchMock = vi.fn();
const applyClaudePluginConfigMock = vi.fn();
const applyClaudeOnboardingSkipMock = vi.fn();
const clearClaudeOnboardingSkipMock = vi.fn();
const syncCurrentProvidersLiveMock = vi.fn();
const updateTrayMenuMock = vi.fn();
const getCurrentMock = vi.fn();
const getAllMock = vi.fn();
const getQueryDataMock = vi.fn();
const invalidatePiDirectoryCachesMock = vi.fn();
const toastErrorMock = vi.fn();
const toastSuccessMock = vi.fn();

let settingsFormMock: any;
let directorySettingsMock: any;
let metadataMock: any;
let serverSettings: Settings;

vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => toastErrorMock(...args),
    success: (...args: unknown[]) => toastSuccessMock(...args),
  },
}));

vi.mock("@/hooks/useSettingsForm", () => ({
  useSettingsForm: () => settingsFormMock,
}));

vi.mock("@/hooks/useDirectorySettings", () => ({
  useDirectorySettings: () => directorySettingsMock,
}));

vi.mock("@/hooks/useSettingsMetadata", () => ({
  useSettingsMetadata: () => metadataMock,
}));

vi.mock("@/lib/query", () => ({
  invalidatePiDirectoryCaches: (...args: unknown[]) =>
    invalidatePiDirectoryCachesMock(...args),
  useSettingsQuery: (...args: unknown[]) => useSettingsQueryMock(...args),
  useSaveSettingsMutation: () => ({
    mutateAsync: mutateAsyncMock,
    isPending: false,
  }),
}));

vi.mock("@tanstack/react-query", async () => {
  const actual = await vi.importActual<typeof import("@tanstack/react-query")>(
    "@tanstack/react-query",
  );
  return {
    ...actual,
    useQueryClient: () => ({
      getQueryData: (...args: unknown[]) => getQueryDataMock(...args),
    }),
  };
});

vi.mock("@/lib/api", () => ({
  settingsApi: {
    setAppConfigDirOverride: (...args: unknown[]) =>
      setAppConfigDirOverrideMock(...args),
    setAutoLaunch: (...args: unknown[]) => setAutoLaunchMock(...args),
    applyClaudePluginConfig: (...args: unknown[]) =>
      applyClaudePluginConfigMock(...args),
    applyClaudeOnboardingSkip: (...args: unknown[]) =>
      applyClaudeOnboardingSkipMock(...args),
    clearClaudeOnboardingSkip: (...args: unknown[]) =>
      clearClaudeOnboardingSkipMock(...args),
    syncCurrentProvidersLive: (...args: unknown[]) =>
      syncCurrentProvidersLiveMock(...args),
  },
  providersApi: {
    updateTrayMenu: (...args: unknown[]) => updateTrayMenuMock(...args),
    getCurrent: (...args: unknown[]) => getCurrentMock(...args),
    getAll: (...args: unknown[]) => getAllMock(...args),
  },
}));

const createSettingsFormMock = (overrides: Record<string, unknown> = {}) => ({
  settings: {
    showInTray: true,
    minimizeToTrayOnClose: true,
    enableClaudePluginIntegration: false,
    skipClaudeOnboarding: true,
    claudeConfigDir: "/claude",
    codexConfigDir: "/codex",
    geminiConfigDir: "/gemini",
    opencodeConfigDir: "/opencode",
    openclawConfigDir: "/openclaw",
    hermesConfigDir: "/hermes",
    piConfigDir: "/pi",
    language: "zh",
  },
  isLoading: false,
  initialLanguage: "zh",
  updateSettings: vi.fn(),
  resetSettings: vi.fn(),
  syncLanguage: vi.fn(),
  ...overrides,
});

const createDirectorySettingsMock = (
  overrides: Record<string, unknown> = {},
) => ({
  appConfigDir: undefined,
  resolvedDirs: {
    appConfig: "/home/mock/.cc-switch",
    claude: "/default/claude",
    codex: "/default/codex",
    gemini: "/default/gemini",
    opencode: "/default/opencode",
    openclaw: "/default/openclaw",
    hermes: "/default/hermes",
    pi: "/default/pi",
  },
  isLoading: false,
  initialAppConfigDir: undefined,
  commitAppConfigDir: vi.fn(),
  updateDirectory: vi.fn(),
  updateAppConfigDir: vi.fn(),
  browseDirectory: vi.fn(),
  browseAppConfigDir: vi.fn(),
  resetDirectory: vi.fn(),
  resetAppConfigDir: vi.fn(),
  resetAllDirectories: vi.fn(),
  ...overrides,
});

const createMetadataMock = (overrides: Record<string, unknown> = {}) => ({
  isPortable: false,
  requiresRestart: false,
  isLoading: false,
  acknowledgeRestart: vi.fn(),
  setRequiresRestart: vi.fn(),
  ...overrides,
});

// Не обслуживается в web (desktop-only); data dir закрыт WEB-26, plugin/onboarding — WEB-20.
const DESKTOP_ONLY = () => [setAutoLaunchMock];
const SIDE_EFFECTS = () => [
  setAppConfigDirOverrideMock,
  ...DESKTOP_ONLY(),
  applyClaudePluginConfigMock,
  applyClaudeOnboardingSkipMock,
  clearClaudeOnboardingSkipMock,
];

// Форма и сервер расходятся по всем полям, у которых в full save есть desktop-only side effect.
function arrange() {
  serverSettings = {
    ...serverSettings,
    launchOnStartup: false,
    skipClaudeOnboarding: false,
    enableClaudePluginIntegration: false,
    claudeConfigDir: "/server/claude",
  };
  useSettingsQueryMock.mockReturnValue({
    data: serverSettings,
    isLoading: false,
  });
  settingsFormMock = createSettingsFormMock({
    settings: {
      ...serverSettings,
      launchOnStartup: true,
      skipClaudeOnboarding: true,
      enableClaudePluginIntegration: true,
      claudeConfigDir: "/changed/claude",
    },
  });
  directorySettingsMock = createDirectorySettingsMock({
    appConfigDir: "/override/app",
    initialAppConfigDir: "/previous/app",
  });
}

describe("useSettings hook · WEB-00 desktop-only side effects", () => {
  beforeEach(() => {
    for (const m of [
      mutateAsyncMock,
      useSettingsQueryMock,
      getCurrentMock,
      getAllMock,
      getQueryDataMock,
      toastErrorMock,
      toastSuccessMock,
      ...SIDE_EFFECTS(),
    ]) {
      m.mockReset();
    }
    window.localStorage.clear();
    serverSettings = {
      showInTray: true,
      minimizeToTrayOnClose: true,
      enableClaudePluginIntegration: false,
      skipClaudeOnboarding: false,
      claudeConfigDir: "/server/claude",
      language: "zh",
    } as Settings;
    metadataMock = createMetadataMock();
    mutateAsyncMock.mockResolvedValue(true);
    for (const m of SIDE_EFFECTS()) m.mockResolvedValue(true);
    syncCurrentProvidersLiveMock.mockResolvedValue({ ok: true });
    getCurrentMock.mockResolvedValue(null);
    getAllMock.mockResolvedValue({});
    getQueryDataMock.mockImplementation(() => serverSettings);
    arrange();
  });

  it("web: full save sends the changed data dir before save_settings, runs the served side effects and asks for a restart", async () => {
    setWeb(true);
    const { result } = renderHook(() => useSettings());

    let saveResult: { requiresRestart: boolean } | null = null;
    await act(async () => {
      saveResult = await result.current.saveSettings(undefined, {
        silent: true,
      });
    });

    expect(mutateAsyncMock).toHaveBeenCalledTimes(1);
    expect((mutateAsyncMock.mock.calls[0][0] as Settings).claudeConfigDir).toBe(
      "/changed/claude",
    );
    expect(saveResult).toEqual({ requiresRestart: true });
    expect(metadataMock.setRequiresRestart).toHaveBeenCalledWith(true);
    expect(setAppConfigDirOverrideMock).toHaveBeenCalledTimes(1);
    expect(setAppConfigDirOverrideMock).toHaveBeenCalledWith("/override/app");
    // a refusal of the directory must leave the rest unsaved, so the PUT goes first
    expect(
      setAppConfigDirOverrideMock.mock.invocationCallOrder[0],
    ).toBeLessThan(mutateAsyncMock.mock.invocationCallOrder[0]);
    expect(directorySettingsMock.commitAppConfigDir).toHaveBeenCalledWith(
      "/override/app",
    );
    for (const m of DESKTOP_ONLY()) expect(m).not.toHaveBeenCalled();
    expect(applyClaudeOnboardingSkipMock).toHaveBeenCalledTimes(1);
    expect(applyClaudePluginConfigMock).toHaveBeenCalledWith({
      official: false,
    });
    // WEB-09: sync_current_providers_live is served, the web save runs it like desktop.
    expect(syncCurrentProvidersLiveMock).toHaveBeenCalledTimes(1);
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("web: a refused data dir (400) shows the server's text, saves nothing else and opens no restart", async () => {
    setWeb(true);
    setAppConfigDirOverrideMock.mockRejectedValue(
      new Error("путь «x» не абсолютный"),
    );
    const { result } = renderHook(() => useSettings());
    await act(async () => {
      await expect(
        result.current.saveSettings(undefined, { silent: true }),
      ).rejects.toThrow("путь «x» не абсолютный");
    });
    expect(mutateAsyncMock).not.toHaveBeenCalled();
    expect(directorySettingsMock.commitAppConfigDir).not.toHaveBeenCalled();
    expect(metadataMock.setRequiresRestart).not.toHaveBeenCalled();
    expect(applyClaudeOnboardingSkipMock).not.toHaveBeenCalled();
    expect(applyClaudePluginConfigMock).not.toHaveBeenCalled();
    expect(toastErrorMock).toHaveBeenCalledTimes(1);
    expect(toastErrorMock.mock.calls[0][0]).toContain("путь «x» не абсолютный");
  });

  it("web: a directory fixed by the env is never sent, whatever the draft says", async () => {
    setWeb(true);
    directorySettingsMock = createDirectorySettingsMock({
      appConfigDir: "/override/app",
      initialAppConfigDir: "/previous/app",
      appConfigDirStatus: { envLocked: true },
    });
    const { result } = renderHook(() => useSettings());
    let saveResult: { requiresRestart: boolean } | null = null;
    await act(async () => {
      saveResult = await result.current.saveSettings(undefined, {
        silent: true,
      });
    });
    expect(mutateAsyncMock).toHaveBeenCalledTimes(1);
    expect(setAppConfigDirOverrideMock).not.toHaveBeenCalled();
    expect(saveResult).toEqual({ requiresRestart: false });
    // the page reads the lock from useSettings
    expect(result.current.appConfigDirStatus).toEqual({ envLocked: true });
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("web: an unchanged data dir is not sent, so an unrelated save cannot fail on it", async () => {
    setWeb(true);
    directorySettingsMock = createDirectorySettingsMock({
      appConfigDir: "/same/app",
      initialAppConfigDir: "/same/app",
    });
    setAppConfigDirOverrideMock.mockRejectedValue(new Error("400"));
    const { result } = renderHook(() => useSettings());
    await act(async () => {
      await expect(
        result.current.saveSettings(undefined, { silent: true }),
      ).resolves.toEqual({ requiresRestart: false });
    });
    expect(setAppConfigDirOverrideMock).not.toHaveBeenCalled();
  });

  it("web: going back to the default sends null", async () => {
    setWeb(true);
    directorySettingsMock = createDirectorySettingsMock({
      appConfigDir: undefined,
      initialAppConfigDir: "/previous/app",
    });
    const { result } = renderHook(() => useSettings());
    await act(async () => {
      await expect(
        result.current.saveSettings(undefined, { silent: true }),
      ).resolves.toEqual({ requiresRestart: true });
    });
    expect(setAppConfigDirOverrideMock).toHaveBeenCalledWith(null);
  });

  it("web: autosave applies plugin/onboarding and live sync, skips auto-launch", async () => {
    setWeb(true);
    const { result } = renderHook(() => useSettings());
    await act(async () => {
      await result.current.autoSaveSettings({
        launchOnStartup: true,
        skipClaudeOnboarding: true,
        enableClaudePluginIntegration: true,
      });
    });
    expect(mutateAsyncMock).toHaveBeenCalledTimes(1);
    for (const m of [...DESKTOP_ONLY(), setAppConfigDirOverrideMock])
      expect(m).not.toHaveBeenCalled();
    expect(applyClaudeOnboardingSkipMock).toHaveBeenCalledTimes(1);
    expect(applyClaudePluginConfigMock).toHaveBeenCalledTimes(1);
    expect(syncCurrentProvidersLiveMock).toHaveBeenCalledTimes(1);
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("desktop: the same full save still runs every side effect and flags restart", async () => {
    const { result } = renderHook(() => useSettings());
    let saveResult: { requiresRestart: boolean } | null = null;
    await act(async () => {
      saveResult = await result.current.saveSettings(undefined, {
        silent: true,
      });
    });
    expect(saveResult).toEqual({ requiresRestart: true });
    expect(setAppConfigDirOverrideMock).toHaveBeenCalledWith("/override/app");
    // once, after save_settings: the web pre-save call is web only
    expect(setAppConfigDirOverrideMock).toHaveBeenCalledTimes(1);
    expect(
      setAppConfigDirOverrideMock.mock.invocationCallOrder[0],
    ).toBeGreaterThan(mutateAsyncMock.mock.invocationCallOrder[0]);
    expect(setAutoLaunchMock).toHaveBeenCalledWith(true);
    expect(applyClaudeOnboardingSkipMock).toHaveBeenCalledTimes(1);
    expect(applyClaudePluginConfigMock).toHaveBeenCalledWith({
      official: false,
    });
    expect(syncCurrentProvidersLiveMock).toHaveBeenCalledTimes(1);
  });
});
