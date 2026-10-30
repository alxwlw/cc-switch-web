import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// In web builds `@tauri-apps/api/core` is aliased to the HTTP adapter; mirror that here.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});

import { omoApi, omoSlimApi } from "@/lib/api/omo";

describe("omo API · web adapter (WEB-18)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  const jsonResponse = (body: unknown, status = 200) =>
    ({ status, json: async () => body }) as Response;

  const localFile = (filePath: string) => ({
    filePath,
    agents: { [`agent-${filePath}`]: { model: "m" } },
    lastModified: "2026-10-25T00:00:00Z",
  });

  it("omoApi.readLocalFile: GET /api/omo/omo/local-file, unwraps data", async () => {
    const data = localFile("/srv/omo.json");
    fetchSpy.mockResolvedValue(jsonResponse({ ok: true, data }));
    expect(await omoApi.readLocalFile()).toEqual(data);
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/omo/omo/local-file");
    expect(init.method).toBe("GET");
  });

  it("omoApi.getCurrentOmoProviderId: GET /api/omo/omo/current-provider", async () => {
    fetchSpy.mockResolvedValue(jsonResponse({ ok: true, data: "prov-omo" }));
    expect(await omoApi.getCurrentOmoProviderId()).toBe("prov-omo");
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/omo/omo/current-provider");
    expect(init.method).toBe("GET");
  });

  it("omoApi.disableCurrentOmo: POST /api/omo/omo/disable", async () => {
    fetchSpy.mockResolvedValue(jsonResponse({ ok: true, data: null }));
    await omoApi.disableCurrentOmo();
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/omo/omo/disable");
    expect(init.method).toBe("POST");
  });

  it("omoSlimApi.readLocalFile: GET /api/omo/omo-slim/local-file, unwraps data", async () => {
    const data = localFile("/srv/omo-slim.json");
    fetchSpy.mockResolvedValue(jsonResponse({ ok: true, data }));
    expect(await omoSlimApi.readLocalFile()).toEqual(data);
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/omo/omo-slim/local-file");
    expect(init.method).toBe("GET");
  });

  it("omoSlimApi.getCurrentProviderId: GET /api/omo/omo-slim/current-provider", async () => {
    fetchSpy.mockResolvedValue(jsonResponse({ ok: true, data: "prov-slim" }));
    expect(await omoSlimApi.getCurrentProviderId()).toBe("prov-slim");
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/omo/omo-slim/current-provider");
    expect(init.method).toBe("GET");
  });

  it("omoSlimApi.disableCurrent: POST /api/omo/omo-slim/disable", async () => {
    fetchSpy.mockResolvedValue(jsonResponse({ ok: true, data: null }));
    await omoSlimApi.disableCurrent();
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/omo/omo-slim/disable");
    expect(init.method).toBe("POST");
  });
});
