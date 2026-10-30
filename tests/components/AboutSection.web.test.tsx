import { render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AboutSection } from "@/components/settings/AboutSection";
import { UpdateProvider } from "@/contexts/UpdateContext";
import { setWeb } from "../helpers/web";

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  checkUpdates: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en" },
  }),
}));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: mocks.check }));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("3.0.0"),
}));
vi.mock("@/lib/api", () => ({
  settingsApi: {
    checkUpdates: mocks.checkUpdates,
    openExternal: vi.fn(),
  },
}));

function renderAbout() {
  return render(
    <QueryClientProvider client={new QueryClient()}>
      <UpdateProvider>
        <AboutSection isPortable={false} />
      </UpdateProvider>
    </QueryClientProvider>,
  );
}

const fetchMock = vi.fn();

describe("AboutSection · update controls", () => {
  beforeEach(() => {
    mocks.check.mockReset().mockResolvedValue(null);
    mocks.checkUpdates.mockReset();
    fetchMock.mockReset().mockResolvedValue({
      ok: true,
      json: async () => ({
        mode: "web",
        version: "9.9.9",
        runtime_boundaries: "server host only",
      }),
    });
    vi.stubGlobal("fetch", fetchMock);
  });
  afterEach(() => vi.unstubAllGlobals());

  it("desktop: shows the check-for-updates button", async () => {
    renderAbout();
    expect(
      await screen.findByText("settings.checkForUpdates"),
    ).toBeInTheDocument();
    expect(screen.queryByText("web.update.unsupported")).toBeNull();
  });

  it("web: shows the unsupported notice, no check button, no up-to-date", async () => {
    setWeb(true);
    renderAbout();
    expect(
      await screen.findByText("web.update.unsupported"),
    ).toBeInTheDocument();
    expect(screen.queryByText("settings.checkForUpdates")).toBeNull();
    expect(screen.queryByText("settings.upToDate")).toBeNull();
    expect(mocks.check).not.toHaveBeenCalled();
    expect(mocks.checkUpdates).not.toHaveBeenCalled();
  });

  it("desktop: no web block and no capabilities request", async () => {
    renderAbout();
    await screen.findByText("settings.checkForUpdates");
    expect(screen.queryByText("web.mode.title")).toBeNull();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("web: shows statement, server version and runtime_boundaries", async () => {
    setWeb(true);
    renderAbout();
    expect(screen.getByText("web.mode.title")).toBeInTheDocument();
    expect(screen.getByText("web.hostNotice")).toBeInTheDocument();
    expect(
      await screen.findByText("web.mode.serverVersion v9.9.9"),
    ).toBeInTheDocument();
    expect(screen.getByText("server host only")).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledWith("/api/capabilities");
  });

  it("web: capabilities failure keeps the statement, drops version", async () => {
    setWeb(true);
    fetchMock.mockRejectedValue(new Error("down"));
    renderAbout();
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    expect(screen.getByText("web.hostNotice")).toBeInTheDocument();
    expect(screen.queryByText(/web\.mode\.serverVersion/)).toBeNull();
  });

  it.each([
    ["HTTP 500", { ok: false, status: 500, json: async () => ({}) }],
    ["payload without version", { ok: true, json: async () => ({}) }],
  ])(
    "web: %s keeps the statement, shows no server version",
    async (_n, res) => {
      setWeb(true);
      fetchMock.mockResolvedValue(res);
      renderAbout();
      await waitFor(() => expect(fetchMock).toHaveBeenCalled());
      await new Promise((r) => setTimeout(r, 0));
      expect(screen.getByText("web.hostNotice")).toBeInTheDocument();
      expect(screen.queryByText(/web\.mode\.serverVersion/)).toBeNull();
    },
  );

  it("web: blank runtime_boundaries renders no empty paragraph", async () => {
    setWeb(true);
    fetchMock.mockResolvedValue({
      ok: true,
      json: async () => ({
        mode: "web",
        version: "9.9.9",
        runtime_boundaries: "  ",
      }),
    });
    renderAbout();
    const ver = await screen.findByText("web.mode.serverVersion v9.9.9");
    expect(ver.parentElement?.querySelectorAll("p")).toHaveLength(2);
  });
});
