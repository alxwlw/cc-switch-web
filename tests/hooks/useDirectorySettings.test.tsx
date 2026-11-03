import { renderHook, act, waitFor } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useDirectorySettings } from "@/hooks/useDirectorySettings";
import type { SettingsFormState } from "@/hooks/useSettingsForm";
import type { AppConfigDirStatus } from "@/lib/api/settings";
import { setWeb } from "../helpers/web";

const getAppConfigDirOverrideMock = vi.hoisted(() => vi.fn());
const getAppConfigDirStatusMock = vi.hoisted(() => vi.fn());
const getConfigDirMock = vi.hoisted(() => vi.fn());
const selectConfigDirectoryMock = vi.hoisted(() => vi.fn());
const setAppConfigDirOverrideMock = vi.hoisted(() => vi.fn());
const homeDirMock = vi.hoisted(() => vi.fn<() => Promise<string>>());
const joinMock = vi.hoisted(() =>
  vi.fn(async (...segments: string[]) => segments.join("/")),
);
const toastErrorMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api", () => ({
  settingsApi: {
    getAppConfigDirOverride: getAppConfigDirOverrideMock,
    getAppConfigDirStatus: getAppConfigDirStatusMock,
    getConfigDir: getConfigDirMock,
    selectConfigDirectory: selectConfigDirectoryMock,
    setAppConfigDirOverride: setAppConfigDirOverrideMock,
  },
}));

vi.mock("@tauri-apps/api/path", () => ({
  homeDir: homeDirMock,
  join: joinMock,
}));

vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      (options?.defaultValue as string) ?? key,
  }),
}));

const createSettings = (
  overrides: Partial<SettingsFormState> = {},
): SettingsFormState => ({
  showInTray: true,
  minimizeToTrayOnClose: true,
  enableClaudePluginIntegration: false,
  claudeConfigDir: "/claude/custom",
  codexConfigDir: "/codex/custom",
  grokConfigDir: "/grok/custom",
  language: "zh",
  ...overrides,
});

describe("useDirectorySettings", () => {
  const onUpdateSettings = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();

    homeDirMock.mockResolvedValue("/home/mock");
    joinMock.mockImplementation(async (...segments: string[]) =>
      segments.join("/"),
    );

    getAppConfigDirOverrideMock.mockResolvedValue(null);
    getConfigDirMock.mockImplementation(async (app: string) => {
      if (app === "claude") return "/remote/claude";
      if (app === "codex") return "/remote/codex";
      if (app === "gemini") return "/remote/gemini";
      if (app === "grokbuild") return "/remote/grok";
      if (app === "opencode") return "/remote/opencode";
      if (app === "openclaw") return "/remote/openclaw";
      if (app === "pi") return "/remote/pi";
      if (app === "omp") return "/remote/omp";
      return "/remote/hermes";
    });
    selectConfigDirectoryMock.mockReset();
  });

  it("initializes directories using overrides and remote defaults", async () => {
    getAppConfigDirOverrideMock.mockResolvedValue("  /override/app  ");

    const { result } = renderHook(() =>
      useDirectorySettings({ settings: createSettings(), onUpdateSettings }),
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(result.current.appConfigDir).toBe("/override/app");
    expect(result.current.resolvedDirs).toEqual({
      appConfig: "/override/app",
      claude: "/remote/claude",
      codex: "/remote/codex",
      gemini: "/remote/gemini",
      grokbuild: "/remote/grok",
      opencode: "/remote/opencode",
      openclaw: "/remote/openclaw",
      hermes: "/remote/hermes",
      pi: "/remote/pi",
      omp: "/remote/omp",
    });
  });

  it.each([
    ["claude", "claudeConfigDir"],
    ["codex", "codexConfigDir"],
    ["gemini", "geminiConfigDir"],
    ["grokbuild", "grokConfigDir"],
    ["opencode", "opencodeConfigDir"],
    ["openclaw", "openclawConfigDir"],
    ["hermes", "hermesConfigDir"],
    ["pi", "piConfigDir"],
  ] as const)(
    "syncs the %s display when form drafts are discarded",
    async (app, field) => {
      const initialSettings = createSettings({ [field]: undefined });
      const { result, rerender } = renderHook(
        ({ settings }) => useDirectorySettings({ settings, onUpdateSettings }),
        { initialProps: { settings: initialSettings } },
      );
      await waitFor(() => expect(result.current.isLoading).toBe(false));
      const backendDirectory = result.current.resolvedDirs[app];
      rerender({
        settings: { ...initialSettings, skipClaudeOnboarding: true },
      });
      expect(result.current.resolvedDirs[app]).toBe(backendDirectory);

      act(() => result.current.updateAppConfigDir("/unsaved/app"));
      rerender({
        settings: { ...initialSettings, [field]: "/unsaved/config" },
      });
      expect(result.current.resolvedDirs[app]).toBe("/unsaved/config");

      rerender({ settings: initialSettings });
      await waitFor(() =>
        expect(result.current.resolvedDirs[app]).toBe(backendDirectory),
      );
      expect(result.current.appConfigDir).toBe("/unsaved/app");
      expect(result.current.resolvedDirs.appConfig).toBe("/unsaved/app");

      rerender({ settings: { ...initialSettings, [field]: "/saved/config" } });
      expect(result.current.resolvedDirs[app]).toBe("/saved/config");
    },
  );

  it.each(["edit", "reset", "resetAll"] as const)(
    "ignores an older backend resolution after a newer %s",
    async (action) => {
      const initialSettings = createSettings({ claudeConfigDir: undefined });
      const { result, rerender } = renderHook(
        ({ settings }) => useDirectorySettings({ settings, onUpdateSettings }),
        { initialProps: { settings: initialSettings } },
      );
      await waitFor(() => expect(result.current.isLoading).toBe(false));
      rerender({
        settings: { ...initialSettings, claudeConfigDir: "/discarded" },
      });
      let finishResolution!: (directory: string) => void;
      getConfigDirMock.mockImplementationOnce(
        () =>
          new Promise<string>((resolve) => {
            finishResolution = resolve;
          }),
      );
      rerender({ settings: initialSettings });

      if (action === "resetAll") {
        act(() =>
          result.current.resetAllDirectories({ claude: "/reset/claude" }),
        );
      } else {
        act(() => result.current.updateDirectory("claude", "/newer/draft"));
        rerender({
          settings: { ...initialSettings, claudeConfigDir: "/newer/draft" },
        });
        if (action === "reset") {
          await act(() => result.current.resetDirectory("claude"));
          rerender({ settings: initialSettings });
        }
      }
      await act(async () => finishResolution("/older/backend"));
      expect(result.current.resolvedDirs.claude).toBe(
        action === "edit"
          ? "/newer/draft"
          : action === "reset"
            ? "/home/mock/.claude"
            : "/reset/claude",
      );
    },
  );

  it("finishes a backend resolution across unrelated settings and directory edits", async () => {
    const initialSettings = createSettings({ claudeConfigDir: undefined });
    const { result, rerender } = renderHook(
      ({ settings }) => useDirectorySettings({ settings, onUpdateSettings }),
      { initialProps: { settings: initialSettings } },
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    rerender({
      settings: { ...initialSettings, claudeConfigDir: "/discarded" },
    });
    let finishResolution!: (directory: string) => void;
    getConfigDirMock.mockImplementationOnce(
      () =>
        new Promise<string>((resolve) => {
          finishResolution = resolve;
        }),
    );
    rerender({ settings: initialSettings });
    rerender({ settings: { ...initialSettings, skipClaudeOnboarding: true } });
    act(() => result.current.updateDirectory("codex", "/newer/codex"));
    rerender({
      settings: {
        ...initialSettings,
        skipClaudeOnboarding: true,
        codexConfigDir: "/newer/codex",
      },
    });
    await act(async () => finishResolution("/backend/default"));
    expect(result.current.resolvedDirs.claude).toBe("/backend/default");
    expect(result.current.resolvedDirs.codex).toBe("/newer/codex");
  });

  it("does not resolve a local reset against the persisted custom directory", async () => {
    const initialSettings = createSettings();
    const { result, rerender } = renderHook(
      ({ settings }) => useDirectorySettings({ settings, onUpdateSettings }),
      { initialProps: { settings: initialSettings } },
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    getConfigDirMock.mockClear();
    await act(() => result.current.resetDirectory("claude"));
    rerender({ settings: { ...initialSettings, claudeConfigDir: undefined } });
    expect(result.current.resolvedDirs.claude).toBe("/home/mock/.claude");
    expect(getConfigDirMock).not.toHaveBeenCalled();
  });

  it("updates claude directory when browsing succeeds", async () => {
    selectConfigDirectoryMock.mockResolvedValue("/picked/claude");

    const { result } = renderHook(() =>
      useDirectorySettings({
        settings: createSettings({ claudeConfigDir: undefined }),
        onUpdateSettings,
      }),
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));

    await act(async () => {
      await result.current.browseDirectory("claude");
    });

    expect(selectConfigDirectoryMock).toHaveBeenCalledWith("/remote/claude");
    expect(onUpdateSettings).toHaveBeenCalledWith({
      claudeConfigDir: "/picked/claude",
    });
    expect(result.current.resolvedDirs.claude).toBe("/picked/claude");
  });

  it("reports error when directory selection fails", async () => {
    selectConfigDirectoryMock.mockResolvedValue(null);

    const { result } = renderHook(() =>
      useDirectorySettings({ settings: createSettings(), onUpdateSettings }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    await act(async () => {
      await result.current.browseDirectory("codex");
    });

    expect(result.current.resolvedDirs.codex).toBe("/remote/codex");
    expect(onUpdateSettings).not.toHaveBeenCalledWith({
      codexConfigDir: expect.anything(),
    });
    expect(selectConfigDirectoryMock).toHaveBeenCalled();

    selectConfigDirectoryMock.mockRejectedValue(new Error("dialog failed"));
    toastErrorMock.mockClear();

    await act(async () => {
      await result.current.browseDirectory("codex");
    });

    expect(toastErrorMock).toHaveBeenCalled();
  });

  it("warns when directory selection promise rejects", async () => {
    selectConfigDirectoryMock.mockRejectedValue(new Error("dialog failed"));

    const { result } = renderHook(() =>
      useDirectorySettings({ settings: createSettings(), onUpdateSettings }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    await act(async () => {
      await result.current.browseDirectory("codex");
    });

    expect(toastErrorMock).toHaveBeenCalled();
    expect(onUpdateSettings).not.toHaveBeenCalledWith({
      codexConfigDir: expect.anything(),
    });
  });

  it("moves the saved baseline when a new app config dir is committed", async () => {
    getAppConfigDirOverrideMock.mockResolvedValue("/override/app");
    const { result } = renderHook(() =>
      useDirectorySettings({
        settings: createSettings(),
        onUpdateSettings: vi.fn(),
      }),
    );
    await waitFor(() =>
      expect(result.current.initialAppConfigDir).toBe("/override/app"),
    );

    act(() => result.current.commitAppConfigDir("/saved/app"));
    expect(result.current.initialAppConfigDir).toBe("/saved/app");

    // 重置回的也是新基准
    act(() => result.current.updateAppConfigDir("/typed/app"));
    act(() => result.current.resetAllDirectories());
    expect(result.current.appConfigDir).toBe("/saved/app");
  });

  it("updates app config directory via browseAppConfigDir", async () => {
    selectConfigDirectoryMock.mockResolvedValue("  /new/app  ");

    const { result } = renderHook(() =>
      useDirectorySettings({
        settings: createSettings(),
        onUpdateSettings,
      }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    await act(async () => {
      await result.current.browseAppConfigDir();
    });

    expect(result.current.appConfigDir).toBe("/new/app");
    expect(selectConfigDirectoryMock).toHaveBeenCalledWith(
      "/home/mock/.cc-switch",
    );
  });

  it("resets directories to computed defaults", async () => {
    const { result } = renderHook(() =>
      useDirectorySettings({
        settings: createSettings({
          claudeConfigDir: "/custom/claude",
          codexConfigDir: "/custom/codex",
        }),
        onUpdateSettings,
      }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    await act(async () => {
      await result.current.resetDirectory("claude");
      await result.current.resetDirectory("codex");
      await result.current.resetAppConfigDir();
    });

    expect(onUpdateSettings).toHaveBeenCalledWith({
      claudeConfigDir: undefined,
    });
    expect(onUpdateSettings).toHaveBeenCalledWith({
      codexConfigDir: undefined,
    });
    expect(result.current.resolvedDirs.claude).toBe("/home/mock/.claude");
    expect(result.current.resolvedDirs.codex).toBe("/home/mock/.codex");
    expect(result.current.resolvedDirs.appConfig).toBe("/home/mock/.cc-switch");
  });

  it("updates openclaw directory when browsing succeeds", async () => {
    selectConfigDirectoryMock.mockResolvedValue("/picked/openclaw");

    const { result } = renderHook(() =>
      useDirectorySettings({
        settings: createSettings({ openclawConfigDir: undefined }),
        onUpdateSettings,
      }),
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));

    await act(async () => {
      await result.current.browseDirectory("openclaw");
    });

    expect(selectConfigDirectoryMock).toHaveBeenCalledWith("/remote/openclaw");
    expect(onUpdateSettings).toHaveBeenCalledWith({
      openclawConfigDir: "/picked/openclaw",
    });
    expect(result.current.resolvedDirs.openclaw).toBe("/picked/openclaw");
  });

  it("resetAllDirectories applies provided resolved values", async () => {
    const { result } = renderHook(() =>
      useDirectorySettings({ settings: createSettings(), onUpdateSettings }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    act(() => {
      result.current.resetAllDirectories({
        claude: "/server/claude",
        codex: "/server/codex",
        gemini: "/server/gemini",
        grokbuild: "/server/grok",
        opencode: "/server/opencode",
        openclaw: "/server/openclaw",
      });
    });

    expect(result.current.resolvedDirs.claude).toBe("/server/claude");
    expect(result.current.resolvedDirs.codex).toBe("/server/codex");
    expect(result.current.resolvedDirs.gemini).toBe("/server/gemini");
    expect(result.current.resolvedDirs.grokbuild).toBe("/server/grok");
    expect(result.current.resolvedDirs.opencode).toBe("/server/opencode");
    expect(result.current.resolvedDirs.openclaw).toBe("/server/openclaw");
  });

  const serverStatus = (
    overrides: Partial<AppConfigDirStatus> = {},
  ): AppConfigDirStatus => ({
    effective: "/srv/data/.cc-switch",
    source: "default",
    override: null,
    envLocked: false,
    default: "/srv/data/.cc-switch",
    ...overrides,
  });

  it("web: reads the status from the server, not get_app_config_dir_override, and shows its stored override", async () => {
    setWeb(true);
    getAppConfigDirOverrideMock.mockRejectedValue(new Error("unsupported"));
    getAppConfigDirStatusMock.mockResolvedValue(
      serverStatus({
        source: "override",
        effective: "/old/dir",
        override: "  /new/dir  ",
      }),
    );

    const { result } = renderHook(() =>
      useDirectorySettings({ settings: createSettings(), onUpdateSettings }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(getAppConfigDirOverrideMock).not.toHaveBeenCalled();
    // what applies after the restart is the stored value, the status line says what runs now
    expect(result.current.appConfigDir).toBe("/new/dir");
    expect(result.current.initialAppConfigDir).toBe("/new/dir");
    expect(result.current.resolvedDirs.appConfig).toBe("/new/dir");
    expect(result.current.appConfigDirStatus?.effective).toBe("/old/dir");
    expect(result.current.resolvedDirs.claude).toBe("/remote/claude");
  });

  it("web: the default and the reset target are the server's, not the shim's homeDir", async () => {
    setWeb(true);
    homeDirMock.mockResolvedValue("/browser/home");
    getAppConfigDirStatusMock.mockResolvedValue(
      serverStatus({ default: "/srv/home/.cc-switch" }),
    );

    const { result } = renderHook(() =>
      useDirectorySettings({ settings: createSettings(), onUpdateSettings }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.appConfigDir).toBeUndefined();
    expect(result.current.resolvedDirs.appConfig).toBe("/srv/home/.cc-switch");

    act(() => result.current.updateAppConfigDir("/somewhere/else"));
    expect(result.current.resolvedDirs.appConfig).toBe("/somewhere/else");
    await act(async () => {
      await result.current.resetAppConfigDir();
    });
    expect(result.current.appConfigDir).toBeUndefined();
    expect(result.current.resolvedDirs.appConfig).toBe("/srv/home/.cc-switch");
  });

  it("web: a directory fixed by the env shows the env value and has no override or draft", async () => {
    setWeb(true);
    getAppConfigDirStatusMock.mockResolvedValue(
      serverStatus({
        source: "env",
        envLocked: true,
        effective: "/srv/.cc-switch-web",
        override: "/stale/file/value",
      }),
    );

    const { result } = renderHook(() =>
      useDirectorySettings({ settings: createSettings(), onUpdateSettings }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(result.current.appConfigDirStatus?.envLocked).toBe(true);
    expect(result.current.appConfigDir).toBeUndefined();
    expect(result.current.initialAppConfigDir).toBeUndefined();
    expect(result.current.resolvedDirs.appConfig).toBe("/srv/.cc-switch-web");
    // discarding drafts keeps showing the env value
    act(() => result.current.resetAllDirectories());
    expect(result.current.resolvedDirs.appConfig).toBe("/srv/.cc-switch-web");
  });

  it("web: a failed status read still resolves the other directories, shows no status and no client-side default", async () => {
    setWeb(true);
    getAppConfigDirStatusMock.mockRejectedValue(new Error("404"));
    const consoleError = vi
      .spyOn(console, "error")
      .mockImplementation(() => undefined);

    const { result } = renderHook(() =>
      useDirectorySettings({ settings: createSettings(), onUpdateSettings }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(getAppConfigDirOverrideMock).not.toHaveBeenCalled();
    expect(result.current.appConfigDirStatus).toBeNull();
    expect(result.current.appConfigDir).toBeUndefined();
    expect(result.current.resolvedDirs.appConfig).toBe("");
    expect(result.current.resolvedDirs.claude).toBe("/remote/claude");
    expect(result.current.resolvedDirs.codex).toBe("/remote/codex");
    expect(consoleError).toHaveBeenCalled();
    consoleError.mockRestore();
  });

  it("desktop: never asks for the web status and keeps the client-side default", async () => {
    getAppConfigDirOverrideMock.mockResolvedValue(null);
    const { result } = renderHook(() =>
      useDirectorySettings({ settings: createSettings(), onUpdateSettings }),
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(getAppConfigDirStatusMock).not.toHaveBeenCalled();
    expect(result.current.appConfigDirStatus).toBeNull();
    expect(result.current.resolvedDirs.appConfig).toBe("/home/mock/.cc-switch");
  });
});
