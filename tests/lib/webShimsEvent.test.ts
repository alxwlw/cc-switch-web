import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

class FakeES {
  static CLOSED = 2;
  static instances: FakeES[] = [];
  readyState = 0;
  closed = false;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  listeners = new Map<string, Array<(e: { data: string }) => void>>();
  constructor(public url: string) {
    FakeES.instances.push(this);
  }
  addEventListener(name: string, fn: (e: { data: string }) => void) {
    this.listeners.set(name, [...(this.listeners.get(name) ?? []), fn]);
  }
  close() {
    this.closed = true;
    this.readyState = 2;
  }
  open() {
    this.readyState = 1;
    this.onopen?.();
  }
  emit(name: string, data: string) {
    for (const fn of this.listeners.get(name) ?? []) fn({ data });
  }
  fail(closed: boolean) {
    this.readyState = closed ? 2 : 0;
    this.onerror?.();
  }
}

type Shim = typeof import("../../src/lib/web-shims/event");
let shim: Shim;
const last = () => FakeES.instances[FakeES.instances.length - 1];

beforeEach(async () => {
  FakeES.instances = [];
  vi.useFakeTimers();
  vi.stubGlobal("EventSource", FakeES);
  vi.resetModules();
  shim = await import("../../src/lib/web-shims/event");
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("web-shims/event", () => {
  it("is lazy: no EventSource until the first listen, then one shared", async () => {
    expect(FakeES.instances).toHaveLength(0);
    await shim.listen("a", () => {});
    await shim.listen("b", () => {});
    expect(FakeES.instances).toHaveLength(1);
    expect(last().url).toBe("/api/events");
  });

  it("delivers a Tauri-shaped event with the parsed payload", async () => {
    const h = vi.fn();
    await shim.listen("provider-switched", h);
    last().emit("provider-switched", '{"appType":"claude"}');
    expect(h).toHaveBeenCalledTimes(1);
    const evt = h.mock.calls[0][0];
    expect(evt.event).toBe("provider-switched");
    expect(evt.payload).toEqual({ appType: "claude" });
    expect(typeof evt.id).toBe("number");
  });

  it("delivers null payload for data: null and does not fire other names", async () => {
    const h = vi.fn();
    const other = vi.fn();
    await shim.listen("x", h);
    await shim.listen("y", other);
    last().emit("x", "null");
    expect(h.mock.calls[0][0].payload).toBeNull();
    expect(other).not.toHaveBeenCalled();
  });

  it("unlisten stops delivery and is idempotent; closes the source at zero handlers", async () => {
    const h1 = vi.fn();
    const h2 = vi.fn();
    const un1 = await shim.listen("x", h1);
    const un2 = await shim.listen("x", h2);
    const es = last();
    un1();
    un1();
    es.emit("x", "1");
    expect(h1).not.toHaveBeenCalled();
    expect(h2).toHaveBeenCalledTimes(1);
    expect(es.closed).toBe(false);
    un2();
    expect(es.closed).toBe(true);
  });

  it("once fires a single time", async () => {
    const h = vi.fn();
    await shim.once("x", h);
    last().emit("x", "1");
    last().emit("x", "2");
    expect(h).toHaveBeenCalledTimes(1);
    expect(h.mock.calls[0][0].payload).toBe(1);
  });

  it("one throwing handler does not starve the others", async () => {
    const err = vi.spyOn(console, "error").mockImplementation(() => {});
    const h = vi.fn();
    await shim.listen("x", () => {
      throw new Error("boom");
    });
    await shim.listen("x", h);
    last().emit("x", "1");
    expect(h).toHaveBeenCalledTimes(1);
    err.mockRestore();
  });

  it("server resync frame reaches resync listeners", async () => {
    const r = vi.fn();
    await shim.listen("resync", r);
    last().emit("resync", "null");
    expect(r).toHaveBeenCalledTimes(1);
  });

  it("first open does not resync; reopen after a drop does, once", async () => {
    const r = vi.fn();
    await shim.listen("resync", r);
    last().open();
    expect(r).not.toHaveBeenCalled();
    last().fail(false); // browser auto-retry
    last().open();
    expect(r).toHaveBeenCalledTimes(1);
    last().open();
    expect(r).toHaveBeenCalledTimes(1);
  });

  it("CLOSED (503) reconnects with doubling backoff, rebinds handlers once, then resyncs", async () => {
    const h = vi.fn();
    const r = vi.fn();
    await shim.listen("x", h);
    await shim.listen("resync", r);
    const first = last();
    first.fail(true);
    expect(first.closed).toBe(true);
    vi.advanceTimersByTime(999);
    expect(FakeES.instances).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(FakeES.instances).toHaveLength(2);
    const second = last();
    second.fail(true);
    vi.advanceTimersByTime(1_999);
    expect(FakeES.instances).toHaveLength(2);
    vi.advanceTimersByTime(1);
    expect(FakeES.instances).toHaveLength(3);
    const third = last();
    third.open();
    expect(r).toHaveBeenCalledTimes(1);
    third.emit("x", "7");
    expect(h).toHaveBeenCalledTimes(1);
    expect(third.listeners.get("x")).toHaveLength(1);
    // backoff resets after a successful open
    third.fail(true);
    vi.advanceTimersByTime(1_000);
    expect(FakeES.instances).toHaveLength(4);
  });

  it("backoff is capped at 30s", async () => {
    await shim.listen("x", () => {});
    for (let i = 0; i < 8; i++) {
      last().fail(true);
      vi.advanceTimersByTime(30_000);
    }
    const n = FakeES.instances.length;
    last().fail(true);
    vi.advanceTimersByTime(29_999);
    expect(FakeES.instances).toHaveLength(n);
    vi.advanceTimersByTime(1);
    expect(FakeES.instances).toHaveLength(n + 1);
  });

  it("unlistening the last handler during backoff cancels the reconnect", async () => {
    const un = await shim.listen("x", () => {});
    last().fail(true);
    un();
    vi.advanceTimersByTime(60_000);
    expect(FakeES.instances).toHaveLength(1);
  });

  it("re-listening after the last unlisten during backoff reconnects at once", async () => {
    const un = await shim.listen("x", () => {});
    last().fail(true);
    un();
    await shim.listen("x", () => {});
    expect(FakeES.instances).toHaveLength(2);
    expect(last().closed).toBe(false);
  });

  it("ignores late frames from a replaced connection", async () => {
    const h = vi.fn();
    await shim.listen("x", h);
    const old = last();
    old.fail(true);
    vi.advanceTimersByTime(1_000);
    old.emit("x", "1");
    expect(h).not.toHaveBeenCalled();
    last().emit("x", "2");
    expect(h).toHaveBeenCalledTimes(1);
  });

  it("a never-emitted name (configLoadError) stays silent", async () => {
    const h = vi.fn();
    await shim.listen("configLoadError", h);
    last().open();
    last().emit("provider-switched", "{}");
    expect(h).not.toHaveBeenCalled();
  });
});
