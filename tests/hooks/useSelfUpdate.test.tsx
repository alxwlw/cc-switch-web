import { StrictMode } from "react";
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  SELF_UPDATE_FIRST_STATUS_WAIT_MS,
  SELF_UPDATE_POLL_INTERVAL_MS,
  SELF_UPDATE_PROBE_TIMEOUT_MS,
  SELF_UPDATE_STAGE_WAIT_MS,
  useSelfUpdate,
} from "@/hooks/useSelfUpdate";
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

const FROM = "0123456789abcdef0123456789abcdef01234567";
const TO = "fedcba9876543210fedcba9876543210fedcba98";

const json = (status: number, body: unknown) => ({
  ok: status >= 200 && status < 300,
  status,
  json: async () => body,
});

/** The status file as `scripts/self-update.sh` writes it, over the given fields. */
function status(stage: string, over: Record<string, unknown> = {}) {
  const final = ["done", "failed", "rolled-back", "rollback-failed"].includes(
    stage,
  );
  return {
    stage,
    ok:
      stage !== "failed" &&
      stage !== "rolled-back" &&
      stage !== "rollback-failed",
    message: `message of ${stage}`,
    fromSha: FROM,
    toSha: TO,
    startedAt: "2026-11-04T10:00:00Z",
    finishedAt: final ? "2026-11-04T10:05:00Z" : null,
    ...over,
  };
}

type Probe = (signal: AbortSignal | undefined) => Promise<unknown> | unknown;

describe("useSelfUpdate (WEB-28)", () => {
  let statusAnswer: Probe;
  let installAnswer: () => unknown;
  let fetchSpy: ReturnType<typeof vi.fn>;

  const calls = (url: string) => fetchSpy.mock.calls.filter(([u]) => u === url);
  const statusCalls = () => calls("/api/update/status");

  const answerData = (data: unknown) => {
    statusAnswer = () => json(200, { ok: true, data });
  };

  beforeEach(() => {
    vi.useFakeTimers();
    setWeb(true);
    reloadMock.mockReset();
    answerData(null);
    installAnswer = () =>
      json(202, {
        ok: true,
        data: { unit: "cc-switch-web-update", branch: "web-parity" },
      });
    fetchSpy = vi.fn(async (url: string, init?: RequestInit) =>
      url === "/api/update/install"
        ? installAnswer()
        : statusAnswer(init?.signal ?? undefined),
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
  const poll = () => tick(SELF_UPDATE_POLL_INTERVAL_MS);

  const setup = async (enabled = true) => {
    const hook = renderHook(() => useSelfUpdate(enabled));
    await tick(0);
    return hook;
  };
  const start = async (hook: Awaited<ReturnType<typeof setup>>) => {
    await act(async () => {
      void hook.result.current.start();
      await vi.advanceTimersByTimeAsync(0);
    });
  };

  it("polls every 2 s, the probe limit is 5 s, a stage may last 15 min, the first status 1 min", () => {
    expect(SELF_UPDATE_POLL_INTERVAL_MS).toBe(2_000);
    expect(SELF_UPDATE_PROBE_TIMEOUT_MS).toBe(5_000);
    expect(SELF_UPDATE_STAGE_WAIT_MS).toBe(15 * 60_000);
    expect(SELF_UPDATE_FIRST_STATUS_WAIT_MS).toBe(60_000);
  });

  it("posts JSON to /api/update/install, follows the real stages and reloads once on done", async () => {
    const hook = await setup();
    await start(hook);

    const [, init] = calls("/api/update/install")[0];
    expect(init.method).toBe("POST");
    expect(init.headers["Content-Type"]).toBe("application/json");
    expect(init.body).toBeUndefined();
    expect(hook.result.current.state).toEqual({
      kind: "running",
      stage: null,
      branch: "web-parity",
      waiting: false,
    });

    for (const stage of ["checking", "building", "installing", "restarting"]) {
      answerData(status(stage));
      await poll();
      expect(hook.result.current.state).toMatchObject({
        kind: "running",
        stage,
      });
    }
    expect(reloadMock).not.toHaveBeenCalled();

    answerData(status("done"));
    await poll();
    expect(hook.result.current.state).toEqual({ kind: "reloading" });
    expect(reloadMock).toHaveBeenCalledTimes(1);
    // the poll is over: no timer, no further probe
    expect(vi.getTimerCount()).toBe(0);
    const probes = statusCalls().length;
    await tick(5 * SELF_UPDATE_POLL_INTERVAL_MS);
    expect(statusCalls()).toHaveLength(probes);
    expect(reloadMock).toHaveBeenCalledTimes(1);
  });

  it("a network error or a 5xx in the middle of the poll does not end it", async () => {
    const hook = await setup();
    await start(hook);
    answerData(status("restarting"));
    await poll();

    statusAnswer = () => {
      throw new TypeError("Failed to fetch");
    };
    await poll();
    expect(hook.result.current.state).toMatchObject({
      kind: "running",
      stage: "restarting",
      waiting: true,
    });
    statusAnswer = () => json(502, { ok: false, error: "bad gateway" });
    await poll();
    statusAnswer = () => json(200, "not an envelope");
    await poll();
    expect(hook.result.current.state).toMatchObject({
      kind: "running",
      waiting: true,
    });
    const probes = statusCalls().length;

    answerData(status("done"));
    await poll();
    expect(statusCalls()).toHaveLength(probes + 1);
    expect(reloadMock).toHaveBeenCalledTimes(1);
  });

  it("a probe that hangs is cut after 5 s and the poll goes on", async () => {
    const hook = await setup();
    await start(hook);
    let aborted = 0;
    statusAnswer = (signal) =>
      new Promise((_resolve, reject) => {
        signal?.addEventListener("abort", () => {
          aborted += 1;
          reject(new DOMException("aborted", "AbortError"));
        });
      });
    await poll(); // probe 1 starts
    expect(aborted).toBe(0);
    await tick(SELF_UPDATE_PROBE_TIMEOUT_MS);
    expect(aborted).toBe(1);
    expect(hook.result.current.state).toMatchObject({ waiting: true });

    answerData(status("done"));
    await poll();
    expect(reloadMock).toHaveBeenCalledTimes(1);
  });

  it("gives up 15 min after the last change of stage and names the log; errors do not extend it", async () => {
    const hook = await setup();
    await start(hook);
    answerData(status("building"));
    await poll();

    // The server goes away: errors only, no stage change.
    statusAnswer = () => {
      throw new TypeError("Failed to fetch");
    };
    await tick(SELF_UPDATE_STAGE_WAIT_MS - 1);
    expect(hook.result.current.state.kind).toBe("running");
    await tick(2);
    expect(hook.result.current.state).toEqual({
      kind: "stuck",
      reason: "stalled",
    });
    expect(vi.getTimerCount()).toBe(0);
    expect(reloadMock).not.toHaveBeenCalled();
  });

  it("a stage the server keeps reporting does not keep the wait alive: a hung unit ends it", async () => {
    const hook = await setup();
    await start(hook);
    answerData(status("building"));
    await poll();
    // the same stage on every probe for 15 minutes
    await tick(SELF_UPDATE_STAGE_WAIT_MS - 1);
    expect(hook.result.current.state).toMatchObject({
      kind: "running",
      stage: "building",
    });
    await tick(2);
    expect(hook.result.current.state).toEqual({
      kind: "stuck",
      reason: "stalled",
    });
  });

  it("a new stage restarts the 15 min wait", async () => {
    const hook = await setup();
    await start(hook);
    answerData(status("building"));
    await poll();
    await tick(SELF_UPDATE_STAGE_WAIT_MS - 2 * SELF_UPDATE_POLL_INTERVAL_MS);
    answerData(status("installing"));
    await poll();
    // 14 min later still waiting (the old deadline has long passed)
    await tick(SELF_UPDATE_STAGE_WAIT_MS - 2 * SELF_UPDATE_POLL_INTERVAL_MS);
    expect(hook.result.current.state).toMatchObject({
      kind: "running",
      stage: "installing",
    });
  });

  it("a status that stays null after the launch ends the wait after 1 min", async () => {
    const hook = await setup();
    await start(hook);
    await tick(SELF_UPDATE_FIRST_STATUS_WAIT_MS - 1_000);
    expect(hook.result.current.state).toMatchObject({
      kind: "running",
      stage: null,
    });
    await tick(1_000);
    expect(hook.result.current.state).toEqual({
      kind: "stuck",
      reason: "no-status",
    });
    expect(vi.getTimerCount()).toBe(0);
    expect(reloadMock).not.toHaveBeenCalled();
  });

  it("a first stage that arrives in time cancels the null deadline", async () => {
    const hook = await setup();
    await start(hook);
    await tick(SELF_UPDATE_FIRST_STATUS_WAIT_MS - 4_000);
    answerData(status("fetching"));
    await poll();
    await tick(SELF_UPDATE_FIRST_STATUS_WAIT_MS);
    expect(hook.result.current.state).toMatchObject({
      kind: "running",
      stage: "fetching",
    });
  });

  it("an old done on disk is not an event: no reload, no poll", async () => {
    answerData(status("done"));
    const hook = await setup();
    expect(statusCalls()).toHaveLength(1);
    expect(hook.result.current.state).toEqual({ kind: "idle" });
    await tick(10 * SELF_UPDATE_POLL_INTERVAL_MS);
    expect(statusCalls()).toHaveLength(1);
    expect(reloadMock).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });

  it.each(["failed", "rolled-back", "rollback-failed"])(
    "an old %s on disk is not shown either",
    async (stage) => {
      answerData(status(stage));
      const hook = await setup();
      expect(hook.result.current.state).toEqual({ kind: "idle" });
      expect(vi.getTimerCount()).toBe(0);
    },
  );

  it("a page opened during a run picks the progress up and reloads when it is done", async () => {
    answerData(status("building"));
    const hook = await setup();
    expect(hook.result.current.state).toEqual({
      kind: "running",
      stage: "building",
      branch: null,
      waiting: false,
    });
    expect(calls("/api/update/install")).toHaveLength(0);
    // it cannot be started a second time from here
    await start(hook);
    expect(calls("/api/update/install")).toHaveLength(0);

    answerData(status("done"));
    await poll();
    expect(reloadMock).toHaveBeenCalledTimes(1);
  });

  it("does not look at the status when self-update is off", async () => {
    answerData(status("building"));
    const hook = await setup(false);
    await tick(3 * SELF_UPDATE_POLL_INTERVAL_MS);
    expect(fetchSpy).not.toHaveBeenCalled();
    expect(hook.result.current.state).toEqual({ kind: "idle" });
  });

  it("shows the server's reason of a 409 as it came and polls nothing", async () => {
    installAnswer = () =>
      json(409, { ok: false, error: "an update is already running" });
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const hook = await setup();
    await start(hook);
    expect(hook.result.current.state).toEqual({
      kind: "refused",
      message: "an update is already running",
    });
    await tick(5 * SELF_UPDATE_POLL_INTERVAL_MS);
    // only the probe of the mount
    expect(statusCalls()).toHaveLength(1);
    expect(vi.getTimerCount()).toBe(0);
    errorSpy.mockRestore();
  });

  it.each([
    ["failed", "failed"],
    ["rolled-back", "rolled-back"],
    ["rollback-failed", "rollback-failed"],
  ] as const)(
    "%s ends the run with the server's message and no reload",
    async (stage, outcome) => {
      const hook = await setup();
      await start(hook);
      answerData(status("building"));
      await poll();
      answerData(status(stage, { message: `why ${stage} happened` }));
      await poll();
      expect(hook.result.current.state).toEqual({
        kind: "ended",
        outcome,
        message: `why ${stage} happened`,
      });
      expect(reloadMock).not.toHaveBeenCalled();
      expect(vi.getTimerCount()).toBe(0);
    },
  );

  it("done at the commit it started from has nothing new to load: no reload", async () => {
    const hook = await setup();
    await start(hook);
    answerData(
      status("done", {
        fromSha: FROM,
        toSha: FROM,
        message: "already up to date",
      }),
    );
    await poll();
    expect(hook.result.current.state).toEqual({
      kind: "ended",
      outcome: "unchanged",
      message: "already up to date",
    });
    expect(reloadMock).not.toHaveBeenCalled();
  });

  it("a second start while a run is followed does nothing", async () => {
    const hook = await setup();
    await start(hook);
    await start(hook);
    expect(calls("/api/update/install")).toHaveLength(1);
  });

  it("can be started again after a rollback", async () => {
    const hook = await setup();
    await start(hook);
    answerData(status("rolled-back"));
    await poll();
    expect(hook.result.current.state.kind).toBe("ended");
    answerData(null);
    await start(hook);
    expect(calls("/api/update/install")).toHaveLength(2);
    expect(hook.result.current.state).toMatchObject({
      kind: "running",
      stage: null,
    });
  });

  it("leaving the page stops the poll: no timer, no probe in flight, no reload", async () => {
    const hook = await setup();
    await start(hook);
    answerData(status("building"));
    await poll();
    let aborted = false;
    statusAnswer = (signal) =>
      new Promise((_resolve, reject) => {
        signal?.addEventListener("abort", () => {
          aborted = true;
          reject(new DOMException("aborted", "AbortError"));
        });
      });
    await poll(); // a probe is now in flight
    expect(aborted).toBe(false);

    hook.unmount();
    expect(aborted).toBe(true);
    expect(vi.getTimerCount()).toBe(0);
    const probes = statusCalls().length;
    answerData(status("done"));
    await tick(5 * SELF_UPDATE_POLL_INTERVAL_MS);
    expect(statusCalls()).toHaveLength(probes);
    expect(reloadMock).not.toHaveBeenCalled();
  });

  it("leaving the page while the install request is in flight starts no poll", async () => {
    let release!: () => void;
    installAnswer = () =>
      new Promise((resolve) => {
        release = () =>
          resolve(json(202, { ok: true, data: { unit: "u", branch: "b" } }));
      }) as unknown as ReturnType<typeof json>;
    const hook = await setup();
    await start(hook);
    hook.unmount();
    await act(async () => {
      release();
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(vi.getTimerCount()).toBe(0);
  });

  it("under StrictMode the probe of the mount is not doubled into two polls", async () => {
    answerData(status("building"));
    const hook = renderHook(() => useSelfUpdate(true), { wrapper: StrictMode });
    await tick(0);
    expect(hook.result.current.state).toMatchObject({
      kind: "running",
      stage: "building",
    });
    const before = statusCalls().length;
    await poll();
    // one timer chain: one probe per interval
    expect(statusCalls()).toHaveLength(before + 1);
  });
});
