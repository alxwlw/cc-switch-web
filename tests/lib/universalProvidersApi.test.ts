import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// In web builds `@tauri-apps/api/core` is aliased to the HTTP adapter; mirror that here.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});

import { universalProvidersApi } from "@/lib/api";
import type { UniversalProvider } from "@/types";

const provider = {
  id: "u/1 x",
  name: "Univ",
  providerType: "newapi",
  apps: { claude: true, codex: false, gemini: true },
  baseUrl: "https://example.com",
  apiKey: "sk-1",
  createdAt: 1,
} as unknown as UniversalProvider;

function okResponse(data: unknown, status = 200) {
  return { status, json: async () => ({ ok: true, data }) };
}

describe("universalProvidersApi через adapter (WEB-07)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => vi.unstubAllGlobals());

  it("getAll: GET /api/universal-providers, возвращает карту", async () => {
    fetchSpy.mockResolvedValue(okResponse({ a: provider }));
    await expect(universalProvidersApi.getAll()).resolves.toEqual({
      a: provider,
    });
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/universal-providers");
    expect(init.method).toBe("GET");
    expect(init.body).toBeUndefined();
  });

  it("get: GET c url-encoded id, null проходит как есть", async () => {
    fetchSpy.mockResolvedValue(okResponse(null));
    await expect(universalProvidersApi.get("u/1 x")).resolves.toBeNull();
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/universal-providers/u%2F1%20x");
    expect(init.method).toBe("GET");
  });

  it("upsert: PUT с body {provider}", async () => {
    fetchSpy.mockResolvedValue(okResponse(true));
    await expect(universalProvidersApi.upsert(provider)).resolves.toBe(true);
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/universal-providers");
    expect(init.method).toBe("PUT");
    expect(JSON.parse(init.body)).toEqual({ provider });
  });

  it("delete: DELETE c url-encoded id, без body", async () => {
    fetchSpy.mockResolvedValue(okResponse(true));
    await expect(universalProvidersApi.delete("u/1 x")).resolves.toBe(true);
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/universal-providers/u%2F1%20x");
    expect(init.method).toBe("DELETE");
    expect(init.body).toBeUndefined();
  });

  it("sync: POST /:id/sync", async () => {
    fetchSpy.mockResolvedValue(okResponse(true));
    await expect(universalProvidersApi.sync("u/1 x")).resolves.toBe(true);
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/universal-providers/u%2F1%20x/sync");
    expect(init.method).toBe("POST");
  });

  it("sync: HTTP 500 {ok:false,error} отклоняется дословным текстом сервера", async () => {
    const text =
      "统一供应商已保存到数据库，但以下应用的配置文件未能写入：codex: disk full";
    fetchSpy.mockResolvedValue({
      status: 500,
      json: async () => ({ ok: false, error: text }),
    });
    await expect(universalProvidersApi.sync("a")).rejects.toThrow(
      new Error(text),
    );
  });
});
