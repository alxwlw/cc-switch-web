import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { proxyApi } from "@/lib/api/proxy";

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

describe("proxy API over the web adapter (WEB-01 stage 1)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    fetchSpy = vi.fn().mockResolvedValue(ok(null));
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

  const appConfig = { appType: "claude", enabled: true };
  const globalConfig = { listenAddress: "127.0.0.1", listenPort: 15721 };

  // [name, call, method, url, body]
  const CASES: Array<
    [string, () => Promise<unknown>, string, string, unknown]
  > = [
    [
      "start",
      () => proxyApi.startProxyServer(),
      "POST",
      "/api/proxy/start",
      undefined,
    ],
    [
      "stop",
      () => proxyApi.stopProxyServer(),
      "POST",
      "/api/proxy/stop",
      undefined,
    ],
    [
      "stop with restore",
      () => proxyApi.stopProxyWithRestore(),
      "POST",
      "/api/proxy/stop-with-restore",
      undefined,
    ],
    [
      "status",
      () => proxyApi.getProxyStatus(),
      "GET",
      "/api/proxy/status",
      undefined,
    ],
    [
      "takeover status",
      () => proxyApi.getProxyTakeoverStatus(),
      "GET",
      "/api/proxy/takeover",
      undefined,
    ],
    [
      "set takeover (route)",
      () => proxyApi.setProxyTakeoverForApp("claude", true, false, "p1"),
      "PUT",
      "/api/proxy/takeover/claude",
      { enabled: true, stack: false, route: "p1" },
    ],
    [
      "set takeover (stack, no route -> null)",
      () => proxyApi.setProxyTakeoverForApp("codex", false, true),
      "PUT",
      "/api/proxy/takeover/codex",
      { enabled: false, stack: true, route: null },
    ],
    [
      "set route",
      () => proxyApi.setProxyRoute("gemini", "p/2"),
      "PUT",
      "/api/proxy/route/gemini",
      { providerId: "p/2" },
    ],
    [
      "app mode (claude-desktop)",
      () => proxyApi.getAppMode("claude-desktop"),
      "GET",
      "/api/proxy/mode/claude-desktop",
      undefined,
    ],
    [
      "take attach failures",
      () => proxyApi.takeStartupAttachFailures(),
      "POST",
      "/api/proxy/startup-attach-failures/take",
      undefined,
    ],
    [
      "exit apps in mode",
      () => proxyApi.exitProxyAppsInMode(true),
      "POST",
      "/api/proxy/exit-apps",
      { stack: true },
    ],
    [
      "direct provider",
      () => proxyApi.getDirectProvider("claude"),
      "GET",
      "/api/proxy/direct/claude",
      undefined,
    ],
    [
      "stack",
      () => proxyApi.getProxyStack("codex"),
      "GET",
      "/api/proxy/stack/codex",
      undefined,
    ],
    [
      "stack member (id is percent-encoded)",
      () => proxyApi.setProxyStackMember("codex", "a b/c", false),
      "PUT",
      "/api/proxy/stack/codex/a%20b%2Fc",
      { enabled: false },
    ],
    [
      "adopt codex catalog",
      () => proxyApi.adoptCodexStackCatalog(),
      "POST",
      "/api/proxy/codex/adopt-stack-catalog",
      undefined,
    ],
    [
      "restart codex daemon",
      () => proxyApi.restartCodexAppServerDaemon(),
      "POST",
      "/api/proxy/codex/restart-daemon",
      undefined,
    ],
    [
      "ack stale clients",
      () => proxyApi.acknowledgeCodexStaleClients(),
      "POST",
      "/api/proxy/codex/ack-stale-clients",
      undefined,
    ],
    [
      "get global config",
      () => proxyApi.getGlobalProxyConfig(),
      "GET",
      "/api/proxy/config",
      undefined,
    ],
    [
      "update global config (body is the config itself)",
      () => proxyApi.updateGlobalProxyConfig(globalConfig as never),
      "PUT",
      "/api/proxy/config",
      globalConfig,
    ],
    [
      "get client host",
      () => proxyApi.getProxyClientHost(),
      "GET",
      "/api/proxy/client-host",
      undefined,
    ],
    [
      "set client host",
      () => proxyApi.setProxyClientHost("100.64.0.1"),
      "PUT",
      "/api/proxy/client-host",
      { host: "100.64.0.1" },
    ],
    [
      "get app config",
      () => proxyApi.getProxyConfigForApp("grokbuild"),
      "GET",
      "/api/proxy/config/grokbuild",
      undefined,
    ],
    [
      "update app config (path from config.appType)",
      () => proxyApi.updateProxyConfigForApp(appConfig as never),
      "PUT",
      "/api/proxy/config/claude",
      appConfig,
    ],
  ];

  it.each(CASES)("%s", async (_name, call, method, url, body) => {
    await call();
    expect(lastCall()).toEqual({ url, method, body });
  });

  it("returns the unwrapped data", async () => {
    fetchSpy.mockResolvedValue(ok({ running: true, port: 15721 }));
    await expect(proxyApi.getProxyStatus()).resolves.toEqual({
      running: true,
      port: 15721,
    });
  });

  it("throws an object error as is (StackWriteError keeps partial and message)", async () => {
    const error = { partial: true, message: "wrote 1 of 2" };
    fetchSpy.mockResolvedValue({
      ok: false,
      status: 500,
      json: async () => ({ ok: false, error }),
    });
    const thrown = await proxyApi
      .setProxyStackMember("codex", "p1", true)
      .catch((e) => e);
    expect(thrown).toEqual(error);
    expect(thrown).not.toBeInstanceOf(Error);
  });

  it("keeps a string error as an Error with that message", async () => {
    fetchSpy.mockResolvedValue({
      ok: false,
      status: 500,
      json: async () => ({ ok: false, error: "proxy is busy" }),
    });
    const thrown = await proxyApi.stopProxyServer().catch((e) => e);
    expect(thrown).toBeInstanceOf(Error);
    expect(thrown.message).toBe("proxy is busy");
  });
});
