import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// In web builds `@tauri-apps/api/core` is aliased to the HTTP adapter; mirror that here.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});

import { vscodeApi } from "@/lib/api/vscode";

function okResponse(data: unknown) {
  return { status: 200, json: async () => ({ ok: true, data }) };
}

describe("vscodeApi endpoints через adapter (WEB-21)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => vi.unstubAllGlobals());

  it("getLiveProviderSettings: GET /api/:app/live-settings", async () => {
    fetchSpy.mockResolvedValue(okResponse({ env: {} }));
    await expect(vscodeApi.getLiveProviderSettings("claude")).resolves.toEqual({
      env: {},
    });
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/claude/live-settings");
    expect(init.method).toBe("GET");
    expect(init.body).toBeUndefined();
  });

  it("testApiEndpoints: POST /api/endpoints/speedtest с {urls, timeoutSecs}", async () => {
    const rows = [{ url: "https://a.example", latency: 42, status: 200 }];
    fetchSpy.mockResolvedValue(okResponse(rows));
    await expect(
      vscodeApi.testApiEndpoints(["https://a.example", "https://b.example"], {
        timeoutSecs: 7,
      }),
    ).resolves.toEqual(rows);
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/endpoints/speedtest");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body)).toEqual({
      urls: ["https://a.example", "https://b.example"],
      timeoutSecs: 7,
    });
  });

  it("getCustomEndpoints: GET с url-encoded id провайдера", async () => {
    fetchSpy.mockResolvedValue(okResponse([]));
    await vscodeApi.getCustomEndpoints("codex", "p/1 x");
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/codex/providers/p%2F1%20x/endpoints");
    expect(init.method).toBe("GET");
  });

  it("addCustomEndpoint: POST с {url} в теле", async () => {
    fetchSpy.mockResolvedValue(okResponse(true));
    await vscodeApi.addCustomEndpoint("claude", "p1", "https://x.example");
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/claude/providers/p1/endpoints");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body)).toEqual({ url: "https://x.example" });
  });

  it("removeCustomEndpoint: DELETE несёт {url} в JSON-теле", async () => {
    fetchSpy.mockResolvedValue(okResponse(true));
    await vscodeApi.removeCustomEndpoint("claude", "p1", "https://x.example");
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/claude/providers/p1/endpoints");
    expect(init.method).toBe("DELETE");
    expect(init.headers).toMatchObject({ "Content-Type": "application/json" });
    expect(JSON.parse(init.body)).toEqual({ url: "https://x.example" });
  });

  it("updateEndpointLastUsed: POST /last-used с {url}", async () => {
    fetchSpy.mockResolvedValue(okResponse(true));
    await vscodeApi.updateEndpointLastUsed("gemini", "p1", "https://x.example");
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/gemini/providers/p1/endpoints/last-used");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body)).toEqual({ url: "https://x.example" });
  });

  it("ошибка сервиса (HTTP 500 {ok:false,error}) становится rejected promise", async () => {
    fetchSpy.mockResolvedValue({
      status: 500,
      json: async () => ({ ok: false, error: "invalid endpoint url" }),
    });
    await expect(
      vscodeApi.addCustomEndpoint("claude", "p1", "ftp://x"),
    ).rejects.toThrow("invalid endpoint url");
  });
});
