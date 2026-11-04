import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UpdateProvider, useUpdate } from "@/contexts/UpdateContext";
import { checkForUpdate } from "@/lib/updater";
import { setWeb } from "../helpers/web";

const mocks = vi.hoisted(() => ({ check: vi.fn() }));

vi.mock("@tauri-apps/plugin-updater", () => ({ check: mocks.check }));
// Web: `invoke` is the real adapter, so the command -> URL mapping is under test; only
// `fetch` is replaced.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter =
    await vi.importActual<typeof import("@/lib/api/adapter")>(
      "@/lib/api/adapter",
    );
  return { invoke: adapter.invoke };
});
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("3.0.0"),
}));

const SHA = "0123456789abcdef0123456789abcdef01234567";
const LATEST = "fedcba9876543210fedcba9876543210fedcba98";

const instructions = {
  systemd:
    "cd <checkout> && git pull --ff-only && scripts/install-server.sh && systemctl --user daemon-reload && systemctl --user restart cc-switch-web",
  docker: "docker build -f Dockerfile.web -t cc-switch-web .",
};

function webCheck(over: Record<string, unknown> = {}) {
  return {
    status: "behind",
    currentVersion: "4.0.20",
    currentSha: SHA,
    latestSha: LATEST,
    behindBy: 2,
    commits: [
      { sha: LATEST, message: "newest", date: "2026-11-04T10:00:00Z" },
      { sha: "aaaaaaa1111111", message: "older", date: null },
    ],
    checkedAt: "2026-11-04T10:30:00Z",
    instructions,
    ...over,
  };
}

const fetchMock = vi.fn();

function answer(data: unknown) {
  fetchMock.mockResolvedValue({
    ok: true,
    status: 200,
    json: async () => ({ ok: true, data }),
  });
}

/** URLs of the update-check requests made so far. */
const checkUrls = () =>
  fetchMock.mock.calls
    .map((c) => c[0] as string)
    .filter((u) => u.startsWith("/api/update/check"));

const wrapper = ({ children }: { children: ReactNode }) => (
  <UpdateProvider>{children}</UpdateProvider>
);

describe("updater · checkForUpdate (desktop)", () => {
  beforeEach(() => {
    mocks.check.mockReset().mockResolvedValue(null);
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });
  afterEach(() => vi.unstubAllGlobals());

  it("no update from the plugin is up-to-date", async () => {
    await expect(checkForUpdate()).resolves.toEqual({ status: "up-to-date" });
    expect(mocks.check).toHaveBeenCalledTimes(1);
  });

  it("an update from the plugin is available with its version, and the server is not asked", async () => {
    mocks.check.mockResolvedValue({
      version: "9.9.9",
      notes: "n",
      date: "2026-01-01",
    });
    await expect(checkForUpdate({ force: true })).resolves.toEqual({
      status: "available",
      info: {
        currentVersion: "3.0.0",
        availableVersion: "9.9.9",
        notes: "n",
        pubDate: "2026-01-01",
      },
    });
    expect(fetchMock).not.toHaveBeenCalled();
  });
});

describe("updater · checkForUpdate (web)", () => {
  beforeEach(() => {
    setWeb(true);
    mocks.check.mockReset().mockResolvedValue(null);
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });
  afterEach(() => vi.unstubAllGlobals());

  it("behind: available, version is <version>+<7 chars of latestSha>, the plugin is not called", async () => {
    answer(webCheck());
    const result = await checkForUpdate();
    expect(result).toMatchObject({
      status: "available",
      info: {
        currentVersion: "4.0.20",
        availableVersion: `4.0.20+${LATEST.slice(0, 7)}`,
        pubDate: "2026-11-04T10:00:00Z",
      },
      web: { status: "behind", behindBy: 2, instructions },
    });
    expect(mocks.check).not.toHaveBeenCalled();
  });

  it("up-to-date and unknown keep the server's data and never become an update", async () => {
    answer(webCheck({ status: "up-to-date", behindBy: 0, commits: [] }));
    await expect(checkForUpdate()).resolves.toMatchObject({
      status: "up-to-date",
      web: { status: "up-to-date" },
    });
    answer(
      webCheck({
        status: "unknown",
        currentSha: null,
        latestSha: null,
        behindBy: 0,
        commits: [],
      }),
    );
    await expect(checkForUpdate()).resolves.toMatchObject({
      status: "unknown",
      web: { status: "unknown", currentSha: null },
    });
  });

  it("asks without force by default and with ?force=1 only when told to", async () => {
    answer(webCheck());
    await checkForUpdate();
    await checkForUpdate({ force: false });
    await checkForUpdate({ force: true });
    expect(checkUrls()).toEqual([
      "/api/update/check",
      "/api/update/check",
      "/api/update/check?force=1",
    ]);
  });

  it("a server error rejects with the server's text, unchanged", async () => {
    fetchMock.mockResolvedValue({
      ok: false,
      status: 500,
      json: async () => ({
        ok: false,
        error: "GitHub rate limit reached (HTTP 403); try again after 11:42",
      }),
    });
    await expect(checkForUpdate()).rejects.toThrow(
      "GitHub rate limit reached (HTTP 403); try again after 11:42",
    );
  });

  it.each([
    ["an unknown status", webCheck({ status: "newer" })],
    ["a missing behindBy", webCheck({ behindBy: undefined })],
    ["commits that are not a list", webCheck({ commits: "none" })],
    ["behind without latestSha", webCheck({ latestSha: null })],
    ["no instructions", webCheck({ instructions: null })],
    ["a negative behindBy", webCheck({ behindBy: -1 })],
    ["a fractional behindBy", webCheck({ behindBy: 1.5 })],
  ])("rejects a payload with %s instead of guessing", async (_n, payload) => {
    answer(payload);
    await expect(checkForUpdate()).rejects.toThrow("unexpected payload");
  });

  it("a server that never answers rejects after opts.timeout, with a text of its own", async () => {
    vi.useFakeTimers();
    try {
      fetchMock.mockReturnValue(new Promise(() => {}));
      const outcome = checkForUpdate({ timeout: 5000 }).then(
        () => null,
        (e: Error) => e.message,
      );
      await vi.advanceTimersByTimeAsync(4999);
      let settled = false;
      void outcome.then(() => (settled = true));
      await vi.advanceTimersByTimeAsync(0);
      expect(settled).toBe(false);
      await vi.advanceTimersByTimeAsync(1);
      expect(await outcome).toBe(
        "update check: no answer from the server in 5 s",
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it("without opts.timeout the wait is 30 s", async () => {
    vi.useFakeTimers();
    try {
      fetchMock.mockReturnValue(new Promise(() => {}));
      let settled = false;
      const outcome = checkForUpdate().then(
        () => ((settled = true), null),
        (e: Error) => ((settled = true), e.message),
      );
      await vi.advanceTimersByTimeAsync(29999);
      expect(settled).toBe(false);
      await vi.advanceTimersByTimeAsync(1);
      expect(await outcome).toBe(
        "update check: no answer from the server in 30 s",
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it("an answer, or an error, before the deadline leaves no timer behind", async () => {
    vi.useFakeTimers();
    try {
      answer(webCheck());
      await checkForUpdate();
      expect(vi.getTimerCount()).toBe(0);
      fetchMock.mockResolvedValue({
        ok: false,
        status: 500,
        json: async () => ({ ok: false, error: "boom" }),
      });
      await expect(checkForUpdate()).rejects.toThrow("boom");
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("UpdateContext", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mocks.check.mockReset().mockResolvedValue(null);
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("desktop: auto-checks on mount, webUpdate stays null", async () => {
    const { result } = renderHook(() => useUpdate(), { wrapper });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1100);
    });
    expect(mocks.check).toHaveBeenCalledTimes(1);
    expect(result.current.webUpdate).toBeNull();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("desktop: an available update sets hasUpdate", async () => {
    mocks.check.mockResolvedValue({ version: "9.9.9", notes: "n" });
    const { result } = renderHook(() => useUpdate(), { wrapper });
    let available = false;
    await act(async () => {
      available = await result.current.checkUpdate();
    });
    expect(available).toBe(true);
    expect(result.current.hasUpdate).toBe(true);
    expect(result.current.updateInfo?.availableVersion).toBe("9.9.9");
  });

  it("web: the automatic check runs after a second, without force, and behind sets hasUpdate", async () => {
    setWeb(true);
    answer(webCheck());
    const { result } = renderHook(() => useUpdate(), { wrapper });
    expect(checkUrls()).toEqual([]);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1100);
    });
    expect(checkUrls()).toEqual(["/api/update/check"]);
    expect(result.current.hasUpdate).toBe(true);
    expect(result.current.updateInfo?.availableVersion).toBe(
      `4.0.20+${LATEST.slice(0, 7)}`,
    );
    expect(result.current.webUpdate).toMatchObject({
      status: "behind",
      behindBy: 2,
    });
    expect(mocks.check).not.toHaveBeenCalled();
  });

  it("web: a re-render and a re-mount never force; only the manual call does", async () => {
    setWeb(true);
    answer(webCheck({ status: "up-to-date", behindBy: 0, commits: [] }));
    const first = renderHook(() => useUpdate(), { wrapper });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1100);
    });
    first.rerender();
    first.rerender();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(checkUrls()).toEqual(["/api/update/check"]);

    first.unmount();
    renderHook(() => useUpdate(), { wrapper });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1100);
    });
    expect(checkUrls()).toEqual(["/api/update/check", "/api/update/check"]);

    const manual = renderHook(() => useUpdate(), { wrapper });
    await act(async () => {
      await manual.result.current.checkUpdate({ force: true });
    });
    expect(checkUrls().at(-1)).toBe("/api/update/check?force=1");
    expect(checkUrls().filter((u) => u.includes("force"))).toHaveLength(1);
  });

  it("web: up-to-date and unknown leave hasUpdate false but keep webUpdate", async () => {
    setWeb(true);
    answer(webCheck({ status: "unknown", latestSha: null, commits: [] }));
    const { result } = renderHook(() => useUpdate(), { wrapper });
    let available = true;
    await act(async () => {
      available = await result.current.checkUpdate();
    });
    expect(available).toBe(false);
    expect(result.current.hasUpdate).toBe(false);
    expect(result.current.webUpdate?.status).toBe("unknown");
  });

  it("web: a failed check keeps the server's text in error, hasUpdate false, no webUpdate", async () => {
    setWeb(true);
    fetchMock.mockResolvedValue({
      ok: false,
      status: 500,
      json: async () => ({ ok: false, error: "rate limit text from server" }),
    });
    const { result } = renderHook(() => useUpdate(), { wrapper });
    await act(async () => {
      await expect(result.current.checkUpdate()).rejects.toThrow(
        "rate limit text from server",
      );
    });
    expect(result.current.error).toBe("rate limit text from server");
    expect(result.current.hasUpdate).toBe(false);
    expect(result.current.webUpdate).toBeNull();
  });

  it("web: a failed re-check after a behind answer drops the behind state", async () => {
    setWeb(true);
    answer(webCheck());
    const { result } = renderHook(() => useUpdate(), { wrapper });
    await act(async () => {
      await result.current.checkUpdate();
    });
    expect(result.current.hasUpdate).toBe(true);
    expect(result.current.webUpdate?.status).toBe("behind");

    fetchMock.mockResolvedValue({
      ok: false,
      status: 500,
      json: async () => ({ ok: false, error: "rate limit text from server" }),
    });
    await act(async () => {
      await expect(result.current.checkUpdate({ force: true })).rejects.toThrow(
        "rate limit text from server",
      );
    });
    expect(result.current.error).toBe("rate limit text from server");
    expect(result.current.hasUpdate).toBe(false);
    expect(result.current.webUpdate).toBeNull();
  });

  it("web: a server that never answers ends the check after 30 s and frees the button", async () => {
    setWeb(true);
    fetchMock.mockReturnValue(new Promise(() => {}));
    const { result } = renderHook(() => useUpdate(), { wrapper });
    let outcome!: Promise<string | null>;
    await act(async () => {
      outcome = result.current.checkUpdate({ force: true }).then(
        () => null,
        (e: Error) => e.message,
      );
      await vi.advanceTimersByTimeAsync(29999);
    });
    expect(result.current.isChecking).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(await outcome).toBe(
      "update check: no answer from the server in 30 s",
    );
    expect(result.current.isChecking).toBe(false);
    expect(result.current.error).toBe(
      "update check: no answer from the server in 30 s",
    );
    expect(result.current.hasUpdate).toBe(false);
  });
});
