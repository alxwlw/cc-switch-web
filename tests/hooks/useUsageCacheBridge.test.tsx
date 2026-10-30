import { QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { usageKeys } from "@/lib/query/usage";
import { subscriptionKeys } from "@/lib/query/subscription";
import { createTestQueryClient } from "../utils/testQueryClient";

// In the web build `@tauri-apps/api/event` is aliased to the SSE shim.
vi.mock("@tauri-apps/api/event", async () => {
  const shim = await vi.importActual<typeof import("@/lib/web-shims/event")>(
    "@/lib/web-shims/event",
  );
  return { listen: shim.listen };
});

class FakeES {
  static instance: FakeES | null = null;
  readyState = 1;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  listeners = new Map<string, Array<(e: { data: string }) => void>>();
  constructor(public url: string) {
    FakeES.instance = this;
  }
  addEventListener(name: string, fn: (e: { data: string }) => void) {
    this.listeners.set(name, [...(this.listeners.get(name) ?? []), fn]);
  }
  close() {}
  emit(name: string, data: string) {
    for (const fn of this.listeners.get(name) ?? []) fn({ data });
  }
}

describe("useUsageCacheBridge over the web SSE shim (WEB-04)", () => {
  beforeEach(() => {
    FakeES.instance = null;
    vi.stubGlobal("EventSource", FakeES);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("writes script payload into the useUsageQuery cache without refetch", async () => {
    const fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
    const { useUsageCacheBridge } = await import("@/hooks/useUsageCacheBridge");
    const client = createTestQueryClient();
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    renderHook(() => useUsageCacheBridge(), { wrapper });
    await vi.waitFor(() => expect(FakeES.instance).not.toBeNull());
    await vi.waitFor(() =>
      expect(FakeES.instance!.listeners.has("usage-cache-updated")).toBe(true),
    );

    const data = { success: true, data: [] };
    FakeES.instance!.emit(
      "usage-cache-updated",
      JSON.stringify({
        kind: "script",
        appType: "claude",
        providerId: "p1",
        data,
      }),
    );

    expect(client.getQueryData(usageKeys.script("p1", "claude"))).toEqual(data);
    // setQueryData, not invalidateQueries: no network fetch is triggered.
    expect(fetchSpy).not.toHaveBeenCalled();
    expect(FakeES.instance!.url).toBe("/api/events");
  });

  it("writes subscription payload into the subscription quota cache", async () => {
    const { useUsageCacheBridge } = await import("@/hooks/useUsageCacheBridge");
    const client = createTestQueryClient();
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    renderHook(() => useUsageCacheBridge(), { wrapper });
    await vi.waitFor(() => expect(FakeES.instance).not.toBeNull());
    await vi.waitFor(() =>
      expect(FakeES.instance!.listeners.has("usage-cache-updated")).toBe(true),
    );

    const data = {
      tool: "claude",
      credentialStatus: "valid",
      credentialMessage: null,
      success: true,
      tiers: [],
      extraUsage: null,
      error: null,
      queriedAt: 1,
    };
    FakeES.instance!.emit(
      "usage-cache-updated",
      JSON.stringify({ kind: "subscription", appType: "claude", data }),
    );

    expect(client.getQueryData(subscriptionKeys.quota("claude"))).toEqual(data);
  });
});
