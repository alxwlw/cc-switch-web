import { QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Sidebar } from "@/components/shell/Sidebar";
import { UpdateProvider } from "@/contexts/UpdateContext";
import type { VisibleApps } from "@/types";
import { createTestQueryClient } from "../../utils/testQueryClient";
import { setWeb } from "../../helpers/web";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { resolvedLanguage: "en" },
  }),
}));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("4.0.20"),
}));
// `invoke` is the real adapter: the dot is driven by what /api/update/check says.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter =
    await vi.importActual<typeof import("@/lib/api/adapter")>(
      "@/lib/api/adapter",
    );
  return { invoke: adapter.invoke };
});
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

const LATEST = "fedcba9876543210fedcba9876543210fedcba98";
const fetchMock = vi.fn();

function serve(status: "behind" | "up-to-date") {
  fetchMock.mockResolvedValue({
    ok: true,
    status: 200,
    json: async () => ({
      ok: true,
      data: {
        status,
        currentVersion: "4.0.20",
        currentSha: "0123456789abcdef0123456789abcdef01234567",
        latestSha: status === "behind" ? LATEST : null,
        behindBy: status === "behind" ? 1 : 0,
        commits:
          status === "behind"
            ? [{ sha: LATEST, message: "newer", date: null }]
            : [],
        checkedAt: "2026-11-04T10:30:00Z",
        instructions: { systemd: "s", docker: "d" },
      },
    }),
  });
}

async function renderSidebarAndWaitForCheck() {
  const client = createTestQueryClient();
  client.setQueryDefaults(["settings"], { staleTime: Infinity });
  client.setQueryData(["settings"], {});
  render(
    <QueryClientProvider client={client}>
      <UpdateProvider>
        <Sidebar
          activeApp="claude"
          view="usage"
          visibleApps={{} as VisibleApps}
          settingsSection="general"
          onSelectApp={vi.fn()}
          onSelectPage={vi.fn()}
          onSelectSettingsSection={vi.fn()}
          onExitSettings={vi.fn()}
        />
      </UpdateProvider>
    </QueryClientProvider>,
  );
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1100);
  });
}

/** The green dot on the Settings entry of the sidebar. */
const settingsDot = () =>
  screen
    .getByRole("button", { name: "nav.settings" })
    .querySelector(".bg-success");

describe("sidebar · update dot in the web console", () => {
  beforeEach(() => {
    setWeb(true);
    vi.useFakeTimers();
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("lights up when the automatic check finds the build behind", async () => {
    serve("behind");
    await renderSidebarAndWaitForCheck();
    expect(settingsDot()).not.toBeNull();
  });

  it("stays dark when the build is up to date", async () => {
    serve("up-to-date");
    await renderSidebarAndWaitForCheck();
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/update/check",
      expect.anything(),
    );
    expect(settingsDot()).toBeNull();
  });
});
