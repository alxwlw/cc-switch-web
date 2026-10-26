import {
  beforeEach,
  describe,
  expect,
  it,
  vi,
  type MockInstance,
} from "vitest";
import { QueryClient, QueryObserver } from "@tanstack/react-query";

let failListen = false;
const handlers = new Map<string, Set<(e: { payload: unknown }) => void>>();
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, fn: (e: { payload: unknown }) => void) => {
    if (failListen) throw new Error("boom");
    if (!handlers.has(name)) handlers.set(name, new Set());
    handlers.get(name)!.add(fn);
    return () => handlers.get(name)!.delete(fn);
  },
}));

import {
  EVENT_INVALIDATIONS,
  startEventInvalidation,
  subscribeEventInvalidation,
} from "../../src/lib/query/eventInvalidation";

const fire = (name: string, payload: unknown = null) =>
  handlers.get(name)?.forEach((fn) => fn({ payload }));

describe("subscribeEventInvalidation", () => {
  let qc: QueryClient;
  let spy: MockInstance<QueryClient["invalidateQueries"]>;
  beforeEach(() => {
    handlers.clear();
    qc = new QueryClient();
    spy = vi.spyOn(qc, "invalidateQueries");
  });

  const keys = () => spy.mock.calls.map((c) => (c[0] as any)?.queryKey);

  it("provider-switched invalidates that app's failover + proxy keys", async () => {
    await subscribeEventInvalidation(qc);
    fire("provider-switched", { appType: "codex", providerId: "p1" });
    expect(keys()).toEqual([
      ["autoFailoverEnabled", "codex"],
      ["failoverQueue", "codex"],
      ["availableProvidersForFailover", "codex"],
      ["subscription", "quota", "codex"],
      ["proxyStatus"],
      ["proxyTakeoverStatus"],
    ]);
  });

  it("provider-switched without appType invalidates the app-agnostic prefixes", async () => {
    await subscribeEventInvalidation(qc);
    fire("provider-switched", {});
    expect(keys()).toEqual([
      ["autoFailoverEnabled"],
      ["failoverQueue"],
      ["proxyStatus"],
      ["proxyTakeoverStatus"],
    ]);
  });

  it("proxy-flags-changed invalidates proxy status, takeover and failover flags", async () => {
    await subscribeEventInvalidation(qc);
    fire("proxy-flags-changed");
    expect(keys()).toEqual([
      ["proxyStatus"],
      ["proxyTakeoverStatus"],
      ["autoFailoverEnabled"],
    ]);
  });

  it("usage-log-recorded invalidates the usage namespace only", async () => {
    await subscribeEventInvalidation(qc);
    fire("usage-log-recorded");
    expect(keys()).toEqual([["usage"]]);
  });

  it("resync invalidates everything (no filter)", async () => {
    await subscribeEventInvalidation(qc);
    fire("resync");
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy.mock.calls[0]).toEqual([]);
  });

  it("does not map events owned elsewhere", async () => {
    await subscribeEventInvalidation(qc);
    for (const n of ["profile-applied", "webdav-sync-status-updated"]) {
      expect(handlers.has(n)).toBe(false);
    }
  });

  it("an event during an in-flight fetch restarts it and ends with new data", async () => {
    let server = "old";
    const resolvers: Array<() => void> = [];
    const fn = vi.fn(() => {
      const snap = server; // value as of when the request started
      return new Promise<string>((r) => resolvers.push(() => r(snap)));
    });
    qc.setQueryData(["usage", "x"], "old");
    const observer = new QueryObserver(qc, {
      queryKey: ["usage", "x"],
      queryFn: fn,
    });
    const unobserve = observer.subscribe(() => {});
    expect(fn).toHaveBeenCalledTimes(1); // started before the change
    await subscribeEventInvalidation(qc);
    server = "new";
    fire("usage-log-recorded");
    expect(fn).toHaveBeenCalledTimes(2);
    resolvers.forEach((r) => r());
    await vi.waitFor(() =>
      expect(observer.getCurrentResult().data).toBe("new"),
    );
    unobserve();
  });

  it("startEventInvalidation registers nothing when disabled", async () => {
    startEventInvalidation(qc, false, () => {});
    await Promise.resolve();
    expect(handlers.size).toBe(0);
  });

  it("startEventInvalidation listens to every mapped name plus resync", async () => {
    startEventInvalidation(qc, true, () => {});
    await vi.waitFor(() => expect(handlers.has("resync")).toBe(true));
    expect([...handlers.keys()].sort()).toEqual(
      [...Object.keys(EVENT_INVALIDATIONS), "resync"].sort(),
    );
  });

  it("startEventInvalidation reports subscribe failures", async () => {
    const onError = vi.fn();
    const bad = { invalidateQueries: vi.fn() } as unknown as QueryClient;
    handlers.clear();
    failListen = true;
    startEventInvalidation(bad, true, onError);
    await vi.waitFor(() => expect(onError).toHaveBeenCalledTimes(1));
    failListen = false;
  });

  it("unsubscribe detaches every handler", async () => {
    const off = await subscribeEventInvalidation(qc);
    off();
    fire("provider-switched", { appType: "codex" });
    fire("resync");
    expect(spy).not.toHaveBeenCalled();
  });
});
