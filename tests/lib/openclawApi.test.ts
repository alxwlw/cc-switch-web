import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// In web builds `@tauri-apps/api/core` is aliased to the HTTP adapter; mirror that here.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});

import { openclawApi } from "@/lib/api/openclaw";
import { providersApi } from "@/lib/api/providers";
import { setWeb } from "../helpers/web";

describe("OpenClaw API · web adapter (WEB-15)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    setWeb(true);
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  const reply = (data: unknown) =>
    fetchSpy.mockResolvedValue({
      status: 200,
      json: async () => ({ ok: true, data }),
    } as Response);

  const lastCall = () => {
    const [url, init] = fetchSpy.mock.calls.at(-1)!;
    return {
      url: String(url),
      method: init?.method as string,
      body: init?.body ? JSON.parse(init.body as string) : undefined,
    };
  };

  it("importOpenClawFromLive: POST", async () => {
    reply(2);
    expect(await providersApi.importOpenClawFromLive()).toBe(2);
    expect(lastCall()).toEqual({
      url: "/api/openclaw/import-live",
      method: "POST",
      body: undefined,
    });
  });

  it("getOpenClawLiveProviderIds: GET", async () => {
    reply(["a", "b"]);
    expect(await providersApi.getOpenClawLiveProviderIds()).toEqual(["a", "b"]);
    expect(lastCall()).toEqual({
      url: "/api/openclaw/live-provider-ids",
      method: "GET",
      body: undefined,
    });
  });

  it("getLiveProvider: id is URL-encoded in the path; null is kept", async () => {
    reply(null);
    expect(await openclawApi.getLiveProvider("a/b c")).toBeNull();
    expect(lastCall()).toEqual({
      url: "/api/openclaw/live-provider/a%2Fb%20c",
      method: "GET",
      body: undefined,
    });
  });

  it("scanHealth: GET", async () => {
    reply([{ code: "x", message: "m" }]);
    expect(await openclawApi.scanHealth()).toEqual([
      { code: "x", message: "m" },
    ]);
    expect(lastCall()).toEqual({
      url: "/api/openclaw/health",
      method: "GET",
      body: undefined,
    });
  });

  it.each([
    [
      "getDefaultModel",
      "setDefaultModel",
      "default-model",
      "model",
      { primary: "p/m" },
    ],
    [
      "getModelCatalog",
      "setModelCatalog",
      "model-catalog",
      "catalog",
      { "p/m": { alias: "m" } },
    ],
    [
      "getAgentsDefaults",
      "setAgentsDefaults",
      "agents-defaults",
      "defaults",
      { workspace: "w" },
    ],
    ["getEnv", "setEnv", "env", "env", { vars: { A: "1" } }],
    ["getTools", "setTools", "tools", "tools", { profile: "full" }],
  ] as const)(
    "%s / %s: GET and PUT with the value under `%s.%s`",
    async (getter, setter, path, key, value) => {
      const api = openclawApi as unknown as Record<
        string,
        (...a: unknown[]) => Promise<unknown>
      >;
      reply(value);
      expect(await api[getter]()).toEqual(value);
      expect(lastCall()).toEqual({
        url: `/api/openclaw/${path}`,
        method: "GET",
        body: undefined,
      });
      reply({ backupPath: "/b" });
      expect(await api[setter](value)).toEqual({ backupPath: "/b" });
      expect(lastCall()).toEqual({
        url: `/api/openclaw/${path}`,
        method: "PUT",
        body: { [key]: value },
      });
    },
  );
});
