import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { kimiOAuthApi } from "@/lib/api/kimiOAuth";

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

describe("Kimi OAuth API over the web adapter (WEB-02 stage 2)", () => {
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

  it("status -> GET /api/kimi/oauth/status", async () => {
    fetchSpy.mockResolvedValue(
      ok({ accounts: [], currentAccountId: null, primaryHidden: true }),
    );
    await expect(kimiOAuthApi.status()).resolves.toEqual({
      accounts: [],
      currentAccountId: null,
      primaryHidden: true,
    });
    expect(lastCall()).toEqual({
      url: "/api/kimi/oauth/status",
      method: "GET",
      body: undefined,
    });
  });

  it("start -> POST /api/kimi/oauth/start with region and accountId", async () => {
    fetchSpy.mockResolvedValue(ok({ flowId: "f1" }));
    await kimiOAuthApi.start("global", "managed:kimi-code");
    expect(lastCall()).toEqual({
      url: "/api/kimi/oauth/start",
      method: "POST",
      body: { region: "global", accountId: "managed:kimi-code" },
    });
  });

  it("start without an account sends accountId null", async () => {
    fetchSpy.mockResolvedValue(ok({ flowId: "f1" }));
    await kimiOAuthApi.start("mainland-cn");
    expect(lastCall().body).toEqual({
      region: "mainland-cn",
      accountId: null,
    });
  });

  it("poll -> POST /api/kimi/oauth/poll with flowId", async () => {
    fetchSpy.mockResolvedValue(
      ok({ authenticated: false, interval: 5, ended: null }),
    );
    await kimiOAuthApi.poll("flow-7");
    expect(lastCall()).toEqual({
      url: "/api/kimi/oauth/poll",
      method: "POST",
      body: { flowId: "flow-7" },
    });
  });

  it("cancel -> POST /api/kimi/oauth/cancel, unknown flow resolves false", async () => {
    fetchSpy.mockResolvedValue(ok(false));
    await expect(kimiOAuthApi.cancel("flow-8")).resolves.toBe(false);
    expect(lastCall()).toEqual({
      url: "/api/kimi/oauth/cancel",
      method: "POST",
      body: { flowId: "flow-8" },
    });
  });

  it("remove -> DELETE with the encoded account id", async () => {
    fetchSpy.mockResolvedValue(ok(null));
    await kimiOAuthApi.remove("managed:kimi-code");
    expect(lastCall()).toEqual({
      url: "/api/kimi/oauth/accounts/managed%3Akimi-code",
      method: "DELETE",
      body: undefined,
    });
  });

  it("quota -> GET .../accounts/:id/quota with the encoded id", async () => {
    fetchSpy.mockResolvedValue(ok({ success: true }));
    await kimiOAuthApi.quota("managed:kimi-code");
    expect(lastCall()).toEqual({
      url: "/api/kimi/oauth/accounts/managed%3Akimi-code/quota",
      method: "GET",
      body: undefined,
    });
  });

  it("models -> GET .../accounts/:id/models", async () => {
    fetchSpy.mockResolvedValue(ok({ "kimi-k2": {} }));
    await expect(kimiOAuthApi.models("kimi-b")).resolves.toEqual({
      "kimi-k2": {},
    });
    expect(lastCall()).toEqual({
      url: "/api/kimi/oauth/accounts/kimi-b/models",
      method: "GET",
      body: undefined,
    });
  });

  it("useAccount -> POST .../accounts/:id/use with the encoded id", async () => {
    fetchSpy.mockResolvedValue(ok(null));
    await kimiOAuthApi.useAccount("managed:kimi-code");
    expect(lastCall()).toEqual({
      url: "/api/kimi/oauth/accounts/managed%3Akimi-code/use",
      method: "POST",
      body: undefined,
    });
  });
});
