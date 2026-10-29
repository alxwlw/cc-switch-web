import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  getGlobalProxyUrl,
  getUpstreamProxyStatus,
  scanLocalProxies,
  setGlobalProxyUrl,
  testProxyUrl,
} from "@/lib/api/globalProxy";

// In the web build `@tauri-apps/api/core` is aliased to the adapter; route invoke through it.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter =
    await vi.importActual<typeof import("@/lib/api/adapter")>(
      "@/lib/api/adapter",
    );
  return { invoke: adapter.invoke };
});

const ok = (data: unknown) => ({
  ok: true,
  status: 200,
  json: async () => ({ ok: true, data }),
});
const fail = (error: string, status = 400) => ({
  ok: false,
  status,
  json: async () => ({ ok: false, error }),
});

describe("globalProxy API over the web adapter (WEB-23)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  const lastCall = () => {
    const [url, init] = fetchSpy.mock.calls.at(-1)!;
    return {
      url,
      method: init.method,
      body: init.body === undefined ? undefined : JSON.parse(init.body),
    };
  };

  it("getGlobalProxyUrl -> GET /api/global-proxy", async () => {
    fetchSpy.mockResolvedValue(ok("http://127.0.0.1:7890"));
    expect(await getGlobalProxyUrl()).toBe("http://127.0.0.1:7890");
    expect(lastCall()).toEqual({
      url: "/api/global-proxy",
      method: "GET",
      body: undefined,
    });
  });

  it("setGlobalProxyUrl -> PUT /api/global-proxy {url}", async () => {
    fetchSpy.mockResolvedValue(ok(null));
    await setGlobalProxyUrl("socks5://10.0.0.1:1080");
    expect(lastCall()).toEqual({
      url: "/api/global-proxy",
      method: "PUT",
      body: { url: "socks5://10.0.0.1:1080" },
    });
  });

  it("testProxyUrl -> POST /api/global-proxy/test {url}", async () => {
    const result = { success: true, latencyMs: 42, error: null };
    fetchSpy.mockResolvedValue(ok(result));
    expect(await testProxyUrl("http://10.0.0.2:8080")).toEqual(result);
    expect(lastCall()).toEqual({
      url: "/api/global-proxy/test",
      method: "POST",
      body: { url: "http://10.0.0.2:8080" },
    });
  });

  it("getUpstreamProxyStatus -> GET /api/global-proxy/status", async () => {
    const status = { enabled: true, proxyUrl: "http://***@10.0.0.3:3128" };
    fetchSpy.mockResolvedValue(ok(status));
    expect(await getUpstreamProxyStatus()).toEqual(status);
    expect(lastCall()).toEqual({
      url: "/api/global-proxy/status",
      method: "GET",
      body: undefined,
    });
  });

  it("scanLocalProxies -> POST /api/global-proxy/scan", async () => {
    const found = [
      { url: "http://127.0.0.1:7890", proxyType: "http", port: 7890 },
    ];
    fetchSpy.mockResolvedValue(ok(found));
    expect(await scanLocalProxies()).toEqual(found);
    expect(lastCall()).toMatchObject({
      url: "/api/global-proxy/scan",
      method: "POST",
    });
  });

  it("setGlobalProxyUrl rejects with the server message verbatim", async () => {
    fetchSpy.mockResolvedValue(fail("invalid proxy url: nope"));
    const err = await setGlobalProxyUrl("nope").catch((e) => e);
    expect(err).toBeInstanceOf(Error);
    expect(err.message).toBe("invalid proxy url: nope");
  });
});
