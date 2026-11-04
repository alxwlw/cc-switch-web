import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AboutSection } from "@/components/settings/AboutSection";
import { UpdateProvider } from "@/contexts/UpdateContext";
import { setWeb } from "../helpers/web";

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  checkUpdates: vi.fn(),
  install: vi.fn(),
  copyText: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
  openExternal: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    // Options are part of the text, so a count or a sha put into a string is asserted too.
    t: (key: string, options?: Record<string, unknown>) =>
      options ? `${key} ${JSON.stringify(options)}` : key,
    i18n: { language: "en" },
  }),
}));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: mocks.check }));
// Web: `invoke` is the real adapter (the command -> /api/update/check mapping runs);
// only `fetch` is replaced.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter =
    await vi.importActual<typeof import("@/lib/api/adapter")>(
      "@/lib/api/adapter",
    );
  return { invoke: adapter.invoke };
});
vi.mock("@/lib/clipboard", () => ({ copyText: mocks.copyText }));
vi.mock("@/lib/toast", () => ({
  toast: { success: mocks.toastSuccess, error: mocks.toastError },
}));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("3.0.0"),
}));
vi.mock("@/lib/api", () => ({
  settingsApi: {
    checkUpdates: mocks.checkUpdates,
    installUpdateAndRestart: mocks.install,
    openExternal: mocks.openExternal,
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

const SHA = "0123456789abcdef0123456789abcdef01234567";
const LATEST = "fedcba9876543210fedcba9876543210fedcba98";
const SYSTEMD =
  "cd <checkout> && git pull --ff-only && scripts/install-server.sh";
const DOCKER =
  "cd <checkout> && git pull --ff-only\ndocker build -f Dockerfile.web -t cc-switch-web .";

/** The server's `/api/update/check` data (`UpdateCheck`), over the given fields. */
function updateData(over: Record<string, unknown> = {}) {
  return {
    status: "behind",
    currentVersion: "4.0.20",
    currentSha: SHA,
    latestSha: LATEST,
    behindBy: 3,
    commits: [
      { sha: LATEST, message: "feat: newest", date: "2026-11-04T10:00:00Z" },
      { sha: "bbbbbbb2222222", message: "fix: middle", date: null },
      { sha: "ccccccc3333333", message: "docs: oldest", date: null },
    ],
    checkedAt: "2026-11-04T10:30:00Z",
    instructions: { systemd: SYSTEMD, docker: DOCKER },
    ...over,
  };
}

/** Answers `/api/capabilities` and `/api/update/check*` like the server does. */
function serve(check: { ok: boolean; status?: number; body: unknown }) {
  fetchMock.mockImplementation(async (url: string) => {
    if (url === "/api/capabilities") {
      return {
        ok: true,
        json: async () => ({
          mode: "web",
          version: "9.9.9",
          runtime_boundaries: "server host only",
        }),
      };
    }
    if (url.startsWith("/api/update/check")) {
      return {
        ok: check.ok,
        status: check.status ?? 200,
        json: async () => check.body,
      };
    }
    throw new Error(`unexpected request ${url}`);
  });
}

const serveCheck = (data: unknown) =>
  serve({ ok: true, body: { ok: true, data } });

const checkUrls = () =>
  fetchMock.mock.calls
    .map((c) => c[0] as string)
    .filter((u) => u.startsWith("/api/update/check"));

async function clickCheck() {
  fireEvent.click(await screen.findByText("settings.checkForUpdates"));
}

describe("AboutSection · update controls", () => {
  beforeEach(() => {
    mocks.check.mockReset().mockResolvedValue(null);
    mocks.checkUpdates.mockReset();
    mocks.install.mockReset().mockResolvedValue(true);
    mocks.copyText.mockReset().mockResolvedValue(undefined);
    mocks.toastSuccess.mockReset();
    mocks.toastError.mockReset();
    mocks.openExternal.mockReset().mockResolvedValue(undefined);
    fetchMock.mockReset();
    serveCheck(updateData({ status: "up-to-date", behindBy: 0, commits: [] }));
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

  it("desktop: an available update turns the button into install, which installs", async () => {
    mocks.check.mockResolvedValue({ version: "9.9.9", notes: "n" });
    renderAbout();
    await clickCheck();
    const install = await screen.findByText(
      'settings.updateTo {"version":"9.9.9"}',
    );
    fireEvent.click(install);
    await waitFor(() => expect(mocks.install).toHaveBeenCalledTimes(1));
    expect(screen.queryByText(/web\.update\.behindTitle/)).toBeNull();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("web: the check button is there and the old unsupported notice is gone", async () => {
    setWeb(true);
    renderAbout();
    expect(
      await screen.findByText("settings.checkForUpdates"),
    ).toBeInTheDocument();
    expect(screen.queryByText("web.update.unsupported")).toBeNull();
    expect(mocks.check).not.toHaveBeenCalled();
  });

  it("web, behind: shows the lag, the commits and both instructions with copy buttons; no install", async () => {
    setWeb(true);
    serveCheck(updateData());
    renderAbout();
    await clickCheck();

    expect(
      await screen.findByText('web.update.behindTitle {"count":3}'),
    ).toBeInTheDocument();
    expect(screen.getByText("feat: newest")).toBeInTheDocument();
    expect(screen.getByText("fix: middle")).toBeInTheDocument();
    expect(screen.getByText("docs: oldest")).toBeInTheDocument();
    expect(screen.getByText(LATEST.slice(0, 7))).toBeInTheDocument();
    // The running build, cut to 7 characters, sits in the footer with the check time.
    expect(
      screen.getByText(/web\.update\.buildSha \{"sha":"0123456"\}/),
    ).toBeInTheDocument();
    expect(screen.queryByText(new RegExp(SHA))).toBeNull();
    // All three commits are listed: no "showing N of M".
    expect(screen.queryByText(/web\.update\.shown/)).toBeNull();

    const pres = Array.from(document.querySelectorAll("pre")).map(
      (p) => p.textContent,
    );
    expect(pres).toEqual([SYSTEMD, DOCKER]);

    fireEvent.click(
      screen.getByRole("button", {
        name: "web.update.copy: web.update.systemd",
      }),
    );
    await waitFor(() => expect(mocks.copyText).toHaveBeenCalledWith(SYSTEMD));
    fireEvent.click(
      screen.getByRole("button", {
        name: "web.update.copy: web.update.docker",
      }),
    );
    await waitFor(() => expect(mocks.copyText).toHaveBeenCalledWith(DOCKER));

    expect(screen.queryByText(/settings\.updateTo/)).toBeNull();
    expect(screen.queryByText("settings.updating")).toBeNull();
    expect(mocks.install).not.toHaveBeenCalled();
    expect(screen.queryByText("web.update.upToDateTitle")).toBeNull();
  });

  it("web, behind: a copied command is confirmed, a failed copy says why", async () => {
    setWeb(true);
    serveCheck(updateData());
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    renderAbout();
    await clickCheck();
    const copySystemd = await screen.findByRole("button", {
      name: "web.update.copy: web.update.systemd",
    });
    fireEvent.click(copySystemd);
    await waitFor(() =>
      expect(mocks.toastSuccess).toHaveBeenCalledWith("web.update.copied", {
        closeButton: true,
      }),
    );
    expect(mocks.toastError).not.toHaveBeenCalled();

    mocks.copyText.mockRejectedValue(new Error("clipboard denied"));
    fireEvent.click(copySystemd);
    await waitFor(() =>
      expect(mocks.toastError).toHaveBeenCalledWith("web.update.copyFailed", {
        description: "clipboard denied",
        closeButton: true,
      }),
    );
    expect(errorSpy).toHaveBeenCalledWith(
      "[AboutSection] Failed to copy instructions",
      expect.objectContaining({ message: "clipboard denied" }),
    );
    errorSpy.mockRestore();
  });

  it("web: the check time is shown in the viewer's locale, and kept as sent when it is not a date", async () => {
    setWeb(true);
    serveCheck(updateData({ checkedAt: "2026-11-04T10:30:00Z" }));
    renderAbout();
    await clickCheck();
    const local = new Date("2026-11-04T10:30:00Z").toLocaleString();
    expect(
      await screen.findByText(
        new RegExp(`web\\.update\\.checkedAt \\{"time":"${local}"\\}`),
      ),
    ).toBeInTheDocument();
  });

  it("web: an unreadable check time is shown as sent", async () => {
    setWeb(true);
    serveCheck(updateData({ checkedAt: "yesterday-ish" }));
    renderAbout();
    await clickCheck();
    expect(
      await screen.findByText(/"time":"yesterday-ish"/),
    ).toBeInTheDocument();
  });

  it("web, behind with a long list: says how many of how many are shown", async () => {
    setWeb(true);
    serveCheck(updateData({ behindBy: 47 }));
    renderAbout();
    await clickCheck();
    expect(
      await screen.findByText(/web\.update\.shown \{"shown":3,"total":47\}/),
    ).toBeInTheDocument();
  });

  it("web, up-to-date: says so, no lag, no instructions", async () => {
    setWeb(true);
    serveCheck(updateData({ status: "up-to-date", behindBy: 0, commits: [] }));
    renderAbout();
    await clickCheck();
    expect(
      await screen.findByText("web.update.upToDateTitle"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/web\.update\.behindTitle/)).toBeNull();
    expect(screen.queryByText("web.update.unknownTitle")).toBeNull();
    expect(document.querySelector("pre")).toBeNull();
  });

  it("web, unknown: says the build version cannot be determined, not up to date", async () => {
    setWeb(true);
    serveCheck(
      updateData({
        status: "unknown",
        currentSha: null,
        latestSha: null,
        behindBy: 0,
        commits: [],
      }),
    );
    renderAbout();
    await clickCheck();
    expect(
      await screen.findByText("web.update.unknownTitle"),
    ).toBeInTheDocument();
    expect(screen.getByText("web.update.unknownHint")).toBeInTheDocument();
    expect(screen.queryByText("web.update.upToDateTitle")).toBeNull();
    expect(screen.queryByText(/web\.update\.behindTitle/)).toBeNull();
    // No build commit is known: no build line, only the check time.
    expect(screen.queryByText(/web\.update\.buildSha/)).toBeNull();
    expect(screen.getByText(/web\.update\.checkedAt/)).toBeInTheDocument();
  });

  it("web, failed check: shows the server's text as it came, not an up-to-date state", async () => {
    setWeb(true);
    const text = "GitHub rate limit reached (HTTP 403); try again after 11:42";
    serve({ ok: false, status: 500, body: { ok: false, error: text } });
    renderAbout();
    await clickCheck();
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("web.update.checkFailedTitle");
    expect(alert).toHaveTextContent(text);
    expect(screen.queryByText("web.update.upToDateTitle")).toBeNull();
    expect(screen.queryByText("settings.upToDate")).toBeNull();
  });

  it("web, behind: after a rate-limited re-check the alert replaces the behind block", async () => {
    setWeb(true);
    serveCheck(updateData());
    renderAbout();
    await clickCheck();
    await screen.findByText('web.update.behindTitle {"count":3}');

    const text = "GitHub rate limit reached (HTTP 403); try again after 11:42";
    serve({ ok: false, status: 500, body: { ok: false, error: text } });
    await clickCheck();
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(text);
    expect(screen.queryByText(/web\.update\.behindTitle/)).toBeNull();
    expect(document.querySelector("pre")).toBeNull();
  });

  it("web, behind: Release notes opens the running version's page, not <version>+<sha>", async () => {
    setWeb(true);
    serveCheck(updateData());
    renderAbout();
    await clickCheck();
    await screen.findByText('web.update.behindTitle {"count":3}');
    fireEvent.click(screen.getByText("settings.releaseNotes"));
    await waitFor(() => expect(mocks.openExternal).toHaveBeenCalledTimes(1));
    const url = mocks.openExternal.mock.calls[0][0] as string;
    expect(url).toBe(
      "https://github.com/farion1231/cc-switch/releases/tag/v3.0.0",
    );
    expect(url).not.toContain("+");
  });

  it("web, up to date: Release notes opens the running version's page", async () => {
    setWeb(true);
    renderAbout();
    await clickCheck();
    await screen.findByText("web.update.upToDateTitle");
    fireEvent.click(screen.getByText("settings.releaseNotes"));
    await waitFor(() =>
      expect(mocks.openExternal).toHaveBeenCalledWith(
        "https://github.com/farion1231/cc-switch/releases/tag/v3.0.0",
      ),
    );
    expect(mocks.openExternal).toHaveBeenCalledTimes(1);
  });

  it("desktop, no update: Release notes opens the running version's page", async () => {
    renderAbout();
    await clickCheck();
    await waitFor(() =>
      expect(mocks.toastSuccess).toHaveBeenCalledWith("settings.upToDate", {
        closeButton: true,
      }),
    );
    fireEvent.click(screen.getByText("settings.releaseNotes"));
    await waitFor(() =>
      expect(mocks.openExternal).toHaveBeenCalledWith(
        "https://github.com/farion1231/cc-switch/releases/tag/v3.0.0",
      ),
    );
    expect(mocks.openExternal).toHaveBeenCalledTimes(1);
  });

  it("desktop: Release notes opens the available version's page", async () => {
    mocks.check.mockResolvedValue({ version: "9.9.9", notes: "n" });
    renderAbout();
    await clickCheck();
    await screen.findByText('settings.updateTo {"version":"9.9.9"}');
    fireEvent.click(screen.getByText("settings.releaseNotes"));
    await waitFor(() =>
      expect(mocks.openExternal).toHaveBeenCalledWith(
        "https://github.com/farion1231/cc-switch/releases/tag/v9.9.9",
      ),
    );
  });

  it("web: the manual button is the one that forces past the server cache", async () => {
    setWeb(true);
    serveCheck(updateData());
    renderAbout();
    await clickCheck();
    await screen.findByText('web.update.behindTitle {"count":3}');
    expect(checkUrls()).toEqual(["/api/update/check?force=1"]);
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
