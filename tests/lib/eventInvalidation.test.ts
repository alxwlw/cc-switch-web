import {
  beforeEach,
  describe,
  expect,
  it,
  vi,
  type MockInstance,
} from "vitest";
import { QueryClient, QueryObserver } from "@tanstack/react-query";

const handlers = new Map<string, Set<(e: { payload: unknown }) => void>>();
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, fn: (e: { payload: unknown }) => void) => {
    if (!handlers.has(name)) handlers.set(name, new Set());
    handlers.get(name)!.add(fn);
    return () => handlers.get(name)!.delete(fn);
  },
}));

import { subscribeEventInvalidation } from "../../src/lib/query/eventInvalidation";

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

  it("usage-log-recorded invalidates the usage namespace only", async () => {
    await subscribeEventInvalidation(qc);
    fire("usage-log-recorded");
    expect(keys()).toEqual([["usage"]]);
  });

  it("resync invalidates everything (no filter)", async () => {
    await subscribeEventInvalidation(qc);
    fire("resync");
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy.mock.calls[0][0]).toBeUndefined();
  });

  it("does not map events owned elsewhere", async () => {
    await subscribeEventInvalidation(qc);
    for (const n of ["profile-applied", "webdav-sync-status-updated"]) {
      expect(handlers.has(n)).toBe(false);
    }
  });

  it("two events during one in-flight refetch do not restart it", async () => {
    let release!: (v: number) => void;
    const fn = vi.fn(() => new Promise<number>((r) => (release = r)));
    qc.setQueryData(["usage", "x"], 0); // cancelRefetch only bites with data
    const observer = new QueryObserver(qc, {
      queryKey: ["usage", "x"],
      queryFn: fn,
    });
    const unobserve = observer.subscribe(() => {});
    expect(fn).toHaveBeenCalledTimes(1); // mount fetch, still pending
    await subscribeEventInvalidation(qc);
    fire("usage-log-recorded");
    fire("usage-log-recorded");
    expect(fn).toHaveBeenCalledTimes(1);
    release(1);
    await vi.waitFor(() => expect(observer.getCurrentResult().data).toBe(1));
    expect(fn).toHaveBeenCalledTimes(1);
    unobserve();
  });

  it("unsubscribe detaches every handler", async () => {
    const off = await subscribeEventInvalidation(qc);
    off();
    fire("provider-switched", { appType: "codex" });
    fire("resync");
    expect(spy).not.toHaveBeenCalled();
  });
});
