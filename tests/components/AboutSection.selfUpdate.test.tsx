import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AboutSection } from "@/components/settings/AboutSection";
import { UpdateProvider } from "@/contexts/UpdateContext";
import { setWeb } from "../helpers/web";

const mocks = vi.hoisted(() => ({ reload: vi.fn(), check: vi.fn() }));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    // Options are part of the text, so a sha or a count put into a string is asserted too.
    t: (key: string, options?: Record<string, unknown>) =>
      options ? `${key} ${JSON.stringify(options)}` : key,
    i18n: { language: "en" },
  }),
}));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: mocks.check }));
// Web: `invoke` is the real adapter (the command -> /api/update/* mapping runs); only `fetch`
// is replaced.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter =
    await vi.importActual<typeof import("@/lib/api/adapter")>(
      "@/lib/api/adapter",
    );
  return { invoke: adapter.invoke };
});
vi.mock("@/lib/pageReload", () => ({ reloadPage: mocks.reload }));
vi.mock("@/lib/clipboard", () => ({ copyText: vi.fn() }));
vi.mock("@/lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("3.0.0"),
}));
vi.mock("@/lib/api", () => ({
  settingsApi: {
    checkUpdates: vi.fn(),
    installUpdateAndRestart: vi.fn(),
    openExternal: vi.fn(),
  },
}));

const SHA = "0123456789abcdef0123456789abcdef01234567";
const LATEST = "fedcba9876543210fedcba9876543210fedcba98";

function updateData(over: Record<string, unknown> = {}) {
  return {
    status: "behind",
    currentVersion: "4.0.20",
    currentSha: SHA,
    latestSha: LATEST,
    behindBy: 2,
    commits: [
      { sha: LATEST, message: "feat: newest", date: null },
      { sha: "bbbbbbb2222222", message: "fix: middle", date: null },
    ],
    checkedAt: "2026-11-04T10:30:00Z",
    branch: "web-parity",
    instructions: { systemd: "SYSTEMD CMD", docker: "DOCKER CMD" },
    ...over,
  };
}

function runStatus(stage: string, over: Record<string, unknown> = {}) {
  return {
    stage,
    ok: !["failed", "rolled-back", "rollback-failed"].includes(stage),
    message: `server says: ${stage}`,
    fromSha: SHA,
    toSha: LATEST,
    startedAt: "2026-11-04T11:00:00Z",
    finishedAt: null,
    ...over,
  };
}

const json = (status: number, body: unknown) => ({
  ok: status >= 200 && status < 300,
  status,
  json: async () => body,
});

describe("AboutSection · self-update (WEB-28)", () => {
  const fetchMock = vi.fn();
  let selfUpdate: boolean;
  let check: unknown;
  let install: () => unknown;
  let runs: unknown;

  const calls = (url: string) =>
    fetchMock.mock.calls.filter(([u]) => u === url);
  const checkUrls = () =>
    fetchMock.mock.calls
      .map(([u]) => u as string)
      .filter((u) => u.startsWith("/api/update/check"));

  beforeEach(() => {
    vi.useFakeTimers();
    setWeb(true);
    mocks.reload.mockReset();
    mocks.check.mockReset().mockResolvedValue(null);
    selfUpdate = true;
    check = updateData();
    runs = null;
    install = () =>
      json(202, {
        ok: true,
        data: { unit: "cc-switch-web-update", branch: "web-parity" },
      });
    fetchMock.mockReset().mockImplementation(async (url: string) => {
      if (url === "/api/capabilities") {
        return json(200, {
          mode: "web",
          version: "9.9.9",
          runtime_boundaries: "server host only",
          selfUpdate,
        });
      }
      if (url.startsWith("/api/update/check")) {
        return json(200, { ok: true, data: check });
      }
      if (url === "/api/update/install") return install();
      if (url === "/api/update/status") {
        return json(200, { ok: true, data: runs });
      }
      throw new Error(`unexpected request ${url}`);
    });
    vi.stubGlobal("fetch", fetchMock);
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  const tick = (ms: number) =>
    act(async () => {
      await vi.advanceTimersByTimeAsync(ms);
    });

  /** Mounts the section and lets the automatic check (1 s after mount) and capabilities answer. */
  async function open() {
    const view = render(
      <QueryClientProvider client={new QueryClient()}>
        <UpdateProvider>
          <AboutSection isPortable={false} />
        </UpdateProvider>
      </QueryClientProvider>,
    );
    await tick(1_500);
    return view;
  }

  const installButton = () => screen.queryByText("web.update.install.button");

  async function startUpdate() {
    fireEvent.click(screen.getByText("web.update.install.button"));
    const dialog = screen.getByRole("dialog");
    fireEvent.click(
      within(dialog).getByText("web.update.install.confirmButton"),
    );
    await tick(0);
  }

  it("the server does not offer self-update: no button, the manual instructions stay", async () => {
    selfUpdate = false;
    await open();
    expect(screen.getByText('web.update.behindTitle {"count":2}')).toBeTruthy();
    expect(installButton()).toBeNull();
    expect(
      Array.from(document.querySelectorAll("pre")).map((p) => p.textContent),
    ).toEqual(["SYSTEMD CMD", "DOCKER CMD"]);
    expect(calls("/api/update/status")).toHaveLength(0);
  });

  it.each([
    ["up-to-date", { status: "up-to-date", behindBy: 0, commits: [] }],
    [
      "unknown",
      {
        status: "unknown",
        currentSha: null,
        latestSha: null,
        behindBy: 0,
        commits: [],
      },
    ],
  ])("self-update on, but the build is %s: no button", async (_n, over) => {
    check = updateData(over);
    await open();
    expect(installButton()).toBeNull();
  });

  it("self-update on and behind: the button shows next to the manual instructions", async () => {
    await open();
    expect(installButton()).not.toBeNull();
    expect(
      Array.from(document.querySelectorAll("pre")).map((p) => p.textContent),
    ).toEqual(["SYSTEMD CMD", "DOCKER CMD"]);
  });

  it("the confirmation names the rebuild, the target, the commits and the database risk; cancel starts nothing", async () => {
    await open();
    fireEvent.click(screen.getByText("web.update.install.button"));
    const dialog = screen.getByRole("dialog");
    expect(
      within(dialog).getByText("web.update.install.confirmTitle"),
    ).toBeTruthy();
    expect(
      within(dialog).getByText(
        'web.update.install.confirmMessage {"branch":"web-parity","sha":"fedcba9","behind":2}',
      ),
    ).toBeTruthy();
    // the same commit list as the page's, not another copy of its markup
    expect(within(dialog).getByText("feat: newest")).toBeTruthy();
    expect(within(dialog).getByText("fix: middle")).toBeTruthy();
    expect(within(dialog).getByText("web.update.commitsTitle")).toBeTruthy();
    expect(
      within(dialog).getByText("web.update.install.confirmDbWarning"),
    ).toBeTruthy();

    fireEvent.click(within(dialog).getByText("common.cancel"));
    await tick(0);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(calls("/api/update/install")).toHaveLength(0);
    expect(installButton()).not.toBeNull();
  });

  it("confirm starts the update, shows each stage, drops the button and reloads on done", async () => {
    await open();
    await startUpdate();
    expect(calls("/api/update/install")).toHaveLength(1);
    expect(installButton()).toBeNull();
    expect(screen.getByText("web.update.install.starting")).toBeTruthy();
    expect(
      screen.getByText('web.update.install.branch {"branch":"web-parity"}'),
    ).toBeTruthy();

    for (const stage of [
      "checking",
      "fetching",
      "merging",
      "building",
      "installing",
      "restarting",
      "rolling-back",
    ]) {
      runs = runStatus(stage);
      await tick(2_000);
      expect(
        screen.getByText(`web.update.install.stage.${stage}`),
      ).toBeTruthy();
      expect(installButton()).toBeNull();
    }
    expect(mocks.reload).not.toHaveBeenCalled();

    runs = runStatus("done", { finishedAt: "2026-11-04T11:05:00Z" });
    await tick(2_000);
    expect(mocks.reload).toHaveBeenCalledTimes(1);
    expect(screen.getByText("web.update.install.reloading")).toBeTruthy();
  });

  it("a stage the script may gain later is shown by its own name", async () => {
    await open();
    await startUpdate();
    runs = runStatus("signing");
    await tick(2_000);
    expect(screen.getByText("signing")).toBeTruthy();
  });

  it("desktop: a failed check shows no web alert and no self-update", async () => {
    setWeb(false);
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.check.mockRejectedValue(new Error("updater down"));
    await open();
    expect(mocks.check).toHaveBeenCalled();
    errorSpy.mockRestore();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(installButton()).toBeNull();
  });

  it("done at the checkout's own commit, no rebuild: nothing reloads, and the check is asked again past the server cache", async () => {
    await open();
    await startUpdate();
    runs = runStatus("done", {
      fromSha: SHA,
      toSha: SHA,
      message: "already up to date",
      finishedAt: "2026-11-04T11:05:00Z",
    });
    const before = checkUrls().length;
    await tick(2_000);
    expect(mocks.reload).not.toHaveBeenCalled();
    expect(screen.getByText("web.update.install.unchangedTitle")).toBeTruthy();
    expect(checkUrls().slice(before)).toEqual(["/api/update/check?force=1"]);
  });

  it("the same done after a rebuild reloads and asks nothing", async () => {
    await open();
    await startUpdate();
    runs = runStatus("building", { fromSha: SHA, toSha: SHA });
    await tick(2_000);
    runs = runStatus("done", {
      fromSha: SHA,
      toSha: SHA,
      finishedAt: "2026-11-04T11:05:00Z",
    });
    const before = checkUrls().length;
    await tick(2_000);
    expect(mocks.reload).toHaveBeenCalledTimes(1);
    expect(checkUrls()).toHaveLength(before);
  });

  it("a 409 shows the server's reason as it came, and the button is back", async () => {
    install = () =>
      json(409, { ok: false, error: "an update is already running" });
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    await open();
    await startUpdate();
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("web.update.install.refusedTitle");
    expect(alert).toHaveTextContent("an update is already running");
    expect(installButton()).not.toBeNull();
    errorSpy.mockRestore();
  });

  it("rolled-back shows the server's message, keeps the behind block and offers the button again", async () => {
    await open();
    await startUpdate();
    runs = runStatus("rolled-back", {
      message: "health check failed; the previous build is back",
    });
    await tick(2_000);
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("web.update.install.rolledBackTitle");
    expect(alert.querySelector(".text-destructive")).toBeNull();
    expect(alert).toHaveTextContent(
      "health check failed; the previous build is back",
    );
    expect(mocks.reload).not.toHaveBeenCalled();
    expect(screen.getByText('web.update.behindTitle {"count":2}')).toBeTruthy();
    expect(installButton()).not.toBeNull();
  });

  it("failed shows the server's message and offers the button again", async () => {
    await open();
    await startUpdate();
    runs = runStatus("failed", { message: "the checkout has local changes" });
    await tick(2_000);
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("web.update.install.failedTitle");
    expect(alert).toHaveTextContent("the checkout has local changes");
    expect(installButton()).not.toBeNull();
  });

  it("rollback-failed is a separate, stronger message with the server's paths, and no button", async () => {
    await open();
    await startUpdate();
    runs = runStatus("rollback-failed", {
      message: "ROLLBACK FAILED. Previous files kept at: /x/cc-switch-web.prev",
    });
    await tick(2_000);
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("web.update.install.rollbackFailedTitle");
    expect(alert).toHaveTextContent("web.update.install.rollbackFailedHint");
    expect(alert).toHaveTextContent("/x/cc-switch-web.prev");
    expect(alert).not.toHaveTextContent("web.update.install.rolledBackTitle");
    // stronger than a rollback: the title is in the destructive colour
    expect(alert.querySelector(".text-destructive")).not.toBeNull();
    expect(installButton()).toBeNull();
  });

  it("a run that never writes a status ends with the journal command", async () => {
    await open();
    await startUpdate();
    await tick(60_000);
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("web.update.install.stuckNoStatus");
    expect(alert).toHaveTextContent(
      "journalctl --user -u cc-switch-web-update",
    );
    expect(installButton()).not.toBeNull();
  });

  it("an old done on disk when About opens: no reload, nothing shown, the button is there", async () => {
    runs = runStatus("done", { finishedAt: "2026-11-01T11:05:00Z" });
    await open();
    await tick(10_000);
    expect(mocks.reload).not.toHaveBeenCalled();
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(installButton()).not.toBeNull();
  });

  it.each([
    ["failed", "web.update.install.failedTitle", true],
    ["rolled-back", "web.update.install.rolledBackTitle", true],
    ["rollback-failed", "web.update.install.rollbackFailedTitle", false],
  ])(
    "an old %s on disk when About opens is shown with its message and time, without a reload or a poll",
    async (stage, title, button) => {
      runs = runStatus(stage, {
        message: `left from the earlier run: ${stage}`,
        finishedAt: "2026-11-01T11:05:00Z",
      });
      await open();
      const alert = screen.getByRole("alert");
      expect(alert).toHaveTextContent(title);
      expect(alert).toHaveTextContent(`left from the earlier run: ${stage}`);
      const local = new Date("2026-11-01T11:05:00Z").toLocaleString();
      expect(alert).toHaveTextContent(
        `web.update.install.finishedAt {"time":"${local}"}`,
      );
      expect(installButton() !== null).toBe(button);
      const probes = calls("/api/update/status").length;
      await tick(30_000);
      expect(calls("/api/update/status")).toHaveLength(probes);
      expect(mocks.reload).not.toHaveBeenCalled();
    },
  );

  it("a new launch replaces the outcome of the earlier run", async () => {
    runs = runStatus("rolled-back", { message: "the earlier rollback" });
    await open();
    expect(screen.getByRole("alert")).toHaveTextContent("the earlier rollback");
    runs = null;
    await startUpdate();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByText("web.update.install.starting")).toBeTruthy();
  });

  it("opened during a run: shows its progress without the button, and reloads when it is done", async () => {
    runs = runStatus("building");
    await open();
    expect(screen.getByText("web.update.install.stage.building")).toBeTruthy();
    expect(installButton()).toBeNull();
    expect(calls("/api/update/install")).toHaveLength(0);

    runs = runStatus("done", { finishedAt: "2026-11-04T11:05:00Z" });
    await tick(2_000);
    expect(mocks.reload).toHaveBeenCalledTimes(1);
  });

  it("leaving About stops the poll", async () => {
    const view = await open();
    await startUpdate();
    runs = runStatus("building");
    await tick(2_000);
    const probes = calls("/api/update/status").length;
    view.unmount();
    runs = runStatus("done");
    await tick(30_000);
    expect(calls("/api/update/status")).toHaveLength(probes);
    expect(mocks.reload).not.toHaveBeenCalled();
  });
});
