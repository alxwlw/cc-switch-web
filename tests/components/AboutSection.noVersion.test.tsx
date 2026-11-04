import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AboutSection } from "@/components/settings/AboutSection";
import { UpdateProvider } from "@/contexts/UpdateContext";
import { setWeb } from "../helpers/web";

// The version cache in AboutSection is module-level and filled by the first successful
// getVersion: this file never lets one succeed, so every test starts with it empty.
const mocks = vi.hoisted(() => ({
  getVersion: vi.fn(),
  check: vi.fn(),
  toastError: vi.fn(),
  openExternal: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en" },
  }),
}));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: mocks.check }));
vi.mock("@tauri-apps/api/core", async () => {
  const adapter =
    await vi.importActual<typeof import("@/lib/api/adapter")>(
      "@/lib/api/adapter",
    );
  return { invoke: adapter.invoke };
});
vi.mock("@/lib/clipboard", () => ({ copyText: vi.fn() }));
vi.mock("@/lib/toast", () => ({
  toast: { success: vi.fn(), error: mocks.toastError },
}));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: mocks.getVersion }));
vi.mock("@/lib/api", () => ({
  settingsApi: {
    checkUpdates: vi.fn(),
    installUpdateAndRestart: vi.fn(),
    openExternal: mocks.openExternal,
  },
}));

const RELEASES = "https://github.com/farion1231/cc-switch/releases";

describe("AboutSection · Release notes without a known version", () => {
  let consoleError: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    mocks.getVersion.mockReset().mockRejectedValue(new Error("no app api"));
    mocks.check.mockReset().mockResolvedValue(null);
    mocks.toastError.mockReset();
    mocks.openExternal.mockReset().mockResolvedValue(undefined);
    consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({
        ok: true,
        json: async () => ({ mode: "web", version: "9.9.9" }),
      })),
    );
  });
  afterEach(() => {
    consoleError.mockRestore();
    vi.unstubAllGlobals();
  });

  async function openReleaseNotes() {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <UpdateProvider>
          <AboutSection isPortable={false} />
        </UpdateProvider>
      </QueryClientProvider>,
    );
    // The failed load is logged before the button can be told apart from "still loading".
    await waitFor(() =>
      expect(consoleError).toHaveBeenCalledWith(
        "[AboutSection] Failed to load app version",
        expect.any(Error),
      ),
    );
    fireEvent.click(await screen.findByText("settings.releaseNotes"));
    await waitFor(() => expect(mocks.openExternal).toHaveBeenCalledTimes(1));
  }

  it("desktop: opens the releases list, no error toast", async () => {
    await openReleaseNotes();
    expect(mocks.openExternal).toHaveBeenCalledWith(RELEASES);
    expect(mocks.toastError).not.toHaveBeenCalled();
  });

  it("web: opens the releases list, no error toast", async () => {
    setWeb(true);
    await openReleaseNotes();
    expect(mocks.openExternal).toHaveBeenCalledWith(RELEASES);
    expect(mocks.toastError).not.toHaveBeenCalled();
  });
});
