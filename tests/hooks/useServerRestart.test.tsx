import { StrictMode } from "react";
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  RESTART_POLL_INTERVAL_MS,
  RESTART_WAIT_MS,
  useServerRestart,
} from "@/hooks/useServerRestart";
import { setWeb } from "../helpers/web";

// In the web build `@tauri-apps/api/core` is aliased to the adapter; route invoke through it.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter =
    await vi.importActual<typeof import("@/lib/api/adapter")>(
      "@/lib/api/adapter",
    );
  return { invoke: adapter.invoke };
});

const reloadMock = vi.hoisted(() => vi.fn());
vi.mock("@/lib/pageReload", () => ({ reloadPage: reloadMock }));

const toastErrorMock = vi.hoisted(() => vi.fn());
vi.mock("@/lib/toast", () => ({
  toast: { error: toastErrorMock, success: vi.fn() },
}));

const OLD = "old-process-id";
const NEW = "new-process-id";

const json = (status: number, body: unknown) => ({
  ok: status >= 200 && status < 300,
  status,
  json: async () => body,
});

type Health = () => Promise<unknown> | unknown;

describe("useServerRestart (WEB-26)", () => {
  let health: Health;
  let fetchSpy: ReturnType<typeof vi.fn>;
  let restartResponse: () => unknown;

  const calls = (url: string) => fetchSpy.mock.calls.filter(([u]) => u === url);

  beforeEach(() => {
    vi.useFakeTimers();
    setWeb(true);
    reloadMock.mockReset();
    toastErrorMock.mockReset();
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

  const tick = (ms: number) =>
    act(async () => {
      await vi.advanceTimersByTimeAsync(ms);
    });

  const start = async () => {
    const hook = renderHook(() => useServerRestart());
    let done!: Promise<boolean>;
    await act(async () => {
      done = hook.result.current.restart();
      await vi.advanceTimersByTimeAsync(0);
    });
    return { ...hook, done };
  };

  it("polls every second and gives up after a minute, as the console promises", () => {
    expect(RESTART_POLL_INTERVAL_MS).toBe(1000);
    expect(RESTART_WAIT_MS).toBe(60000);
  });

  it("posts JSON to /api/restart, keeps polling on the same id and on network errors, reloads once the id changes", async () => {
    const { result, done } = await start();

    const [, init] = calls("/api/restart")[0];
    expect(init.method).toBe("POST");
    expect(init.headers["Content-Type"]).toBe("application/json");
    expect(result.current.restarting).toBe(true);
    expect(calls("/api/health")).toHaveLength(0);

    // the old process still answers until its cleanup is done: same id
    await tick(RESTART_POLL_INTERVAL_MS);
    expect(calls("/api/health")).toHaveLength(1);
    // then the connection is refused
    health = () => {
      throw new TypeError("Failed to fetch");
    };
    await tick(RESTART_POLL_INTERVAL_MS);
    await tick(RESTART_POLL_INTERVAL_MS);
    expect(calls("/api/health")).toHaveLength(3);
    expect(reloadMock).not.toHaveBeenCalled();

    health = () => json(200, { status: "ok", instanceId: NEW });
    await tick(RESTART_POLL_INTERVAL_MS);
    await done;

    expect(reloadMock).toHaveBeenCalledTimes(1);
    expect(toastErrorMock).not.toHaveBeenCalled();
    expect(result.current.restarting).toBe(false);
    // no timer of the flow outlives it (the 60 s deadline, the poll, the probe limit)
    expect(vi.getTimerCount()).toBe(0);
    // the poll is over: no probe after the reload
    const probes = calls("/api/health").length;
    await tick(5 * RESTART_POLL_INTERVAL_MS);
    expect(calls("/api/health")).toHaveLength(probes);
  });

  it("reloads on a 503 (the new process came up in its init-error screen)", async () => {
    const { done } = await start();
    health = () => json(503, { error: "init failed" });
    await tick(RESTART_POLL_INTERVAL_MS);
    await done;
    expect(reloadMock).toHaveBeenCalledTimes(1);
  });

  it("does not reload on other statuses or a 200 without an instanceId", async () => {
    const { done } = await start();
    health = () => json(502, {});
    await tick(RESTART_POLL_INTERVAL_MS);
    health = () => json(200, { status: "ok" });
    await tick(RESTART_POLL_INTERVAL_MS);
    expect(reloadMock).not.toHaveBeenCalled();
    await tick(RESTART_WAIT_MS);
    await done;
    expect(reloadMock).not.toHaveBeenCalled();
  });

  it("gives up after 60 s with a toast and no reload, and the button frees up", async () => {
    const { result, done } = await start();
    await tick(RESTART_WAIT_MS - 1);
    expect(toastErrorMock).not.toHaveBeenCalled();
    expect(result.current.restarting).toBe(true);
    await tick(1);
    await done;

    expect(toastErrorMock).toHaveBeenCalledTimes(1);
    expect(toastErrorMock.mock.calls[0][0]).toBe("web.dataDir.restartTimeout");
    expect(reloadMock).not.toHaveBeenCalled();
    expect(result.current.restarting).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
    const probes = calls("/api/health").length;
    await tick(5 * RESTART_POLL_INTERVAL_MS);
    expect(calls("/api/health")).toHaveLength(probes);
  });

  it("a hung probe does not stall the poll or the 60 s limit", async () => {
    fetchSpy.mockImplementation(
      (url: string, init: { signal?: AbortSignal }) =>
        url === "/api/restart"
          ? Promise.resolve(restartResponse())
          : new Promise((_resolve, reject) => {
              init.signal?.addEventListener("abort", () =>
                reject(new DOMException("aborted", "AbortError")),
              );
            }),
    );
    const { done } = await start();
    await tick(RESTART_WAIT_MS);
    await done;
    expect(calls("/api/health").length).toBeGreaterThan(3);
    expect(toastErrorMock).toHaveBeenCalledTimes(1);
  });

  it("a second click during the flow starts no second request or poll", async () => {
    const { result, done } = await start();
    await act(async () => {
      // the second call does nothing, and says so: its caller must not close the dialog
      await expect(result.current.restart()).resolves.toBe(false);
    });
    expect(calls("/api/restart")).toHaveLength(1);
    await tick(RESTART_POLL_INTERVAL_MS);
    expect(calls("/api/health")).toHaveLength(1);

    health = () => json(200, { status: "ok", instanceId: NEW });
    await tick(RESTART_POLL_INTERVAL_MS);
    await expect(done).resolves.toBe(true);
    expect(reloadMock).toHaveBeenCalledTimes(1);
  });

  it("can restart again after the flow ended", async () => {
    const first = await start();
    await tick(RESTART_WAIT_MS);
    await first.done;
    await act(async () => {
      void first.result.current.restart();
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(calls("/api/restart")).toHaveLength(2);
  });

  it("409 shows the server's text, polls nothing and frees the button", async () => {
    restartResponse = () =>
      json(409, { ok: false, error: "перезапустите сервер вручную" });
    const { result, done } = await start();
    await done;

    expect(toastErrorMock).toHaveBeenCalledWith("перезапустите сервер вручную");
    expect(reloadMock).not.toHaveBeenCalled();
    await tick(3 * RESTART_POLL_INTERVAL_MS);
    expect(calls("/api/health")).toHaveLength(0);
    expect(result.current.restarting).toBe(false);
  });

  it("a 202 without an instanceId is not taken for a restart that can be followed", async () => {
    restartResponse = () => json(202, { ok: true, data: {} });
    const { done } = await start();
    await done;
    expect(calls("/api/health")).toHaveLength(0);
    expect(reloadMock).not.toHaveBeenCalled();
    expect(toastErrorMock).toHaveBeenCalledWith("web.dataDir.restartTimeout", {
      duration: 10_000,
    });
  });

  it("leaving the page stops the poll and the reload", async () => {
    const { unmount, done } = await start();
    await tick(RESTART_POLL_INTERVAL_MS);
    const probes = calls("/api/health").length;
    unmount();
    health = () => json(200, { status: "ok", instanceId: NEW });
    await tick(RESTART_WAIT_MS);
    await done;

    expect(calls("/api/health")).toHaveLength(probes);
    expect(reloadMock).not.toHaveBeenCalled();
    expect(toastErrorMock).not.toHaveBeenCalled();
  });
  it("leaving the page aborts the probe in flight and leaves no timer", async () => {
    let probeSignal: AbortSignal | undefined;
    fetchSpy.mockImplementation(
      (url: string, init: { signal?: AbortSignal }) => {
        if (url === "/api/restart") return Promise.resolve(restartResponse());
        probeSignal = init.signal;
        return new Promise((_resolve, reject) => {
          init.signal?.addEventListener("abort", () =>
            reject(new DOMException("aborted", "AbortError")),
          );
        });
      },
    );
    const { unmount, done } = await start();
    await tick(RESTART_POLL_INTERVAL_MS);
    expect(probeSignal?.aborted).toBe(false);
    unmount();
    await done;
    expect(probeSignal?.aborted).toBe(true);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("leaving the page while the POST is in flight starts no poll", async () => {
    let answer!: (r: unknown) => void;
    fetchSpy.mockImplementation((url: string) =>
      url === "/api/restart"
        ? new Promise((resolve) => {
            answer = resolve;
          })
        : Promise.resolve(health()),
    );
    const { unmount, done } = await start();
    unmount();
    await act(async () => {
      answer(restartResponse());
      await vi.advanceTimersByTimeAsync(0);
    });
    await done;
    await tick(3 * RESTART_POLL_INTERVAL_MS);
    expect(calls("/api/health")).toHaveLength(0);
    expect(vi.getTimerCount()).toBe(0);
    expect(reloadMock).not.toHaveBeenCalled();
  });

  it("survives the double effect of StrictMode (dev builds)", async () => {
    const hook = renderHook(() => useServerRestart(), { wrapper: StrictMode });
    let done!: Promise<boolean>;
    await act(async () => {
      done = hook.result.current.restart();
      await vi.advanceTimersByTimeAsync(0);
    });
    health = () => json(200, { status: "ok", instanceId: NEW });
    await tick(RESTART_POLL_INTERVAL_MS);
    await done;
    expect(reloadMock).toHaveBeenCalledTimes(1);
  });

  it.each([
    ["a non-Error rejection", { message: "inner" }],
    ["an empty message", ""],
  ])("falls back to the generic text on %s", async (_name, error) => {
    restartResponse = () => json(500, { ok: false, error });
    const { done } = await start();
    await done;
    expect(toastErrorMock).toHaveBeenCalledWith("settings.restartFailed");
    expect(calls("/api/health")).toHaveLength(0);
  });
});
