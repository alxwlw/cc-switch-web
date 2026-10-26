import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// In web builds `@tauri-apps/api/core` is aliased to the HTTP adapter; mirror that here.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});

import { profilesApi } from "@/lib/api";

function okResponse(data: unknown, status = 200) {
  return { status, json: async () => ({ ok: true, data }) };
}

describe("profilesApi через adapter (WEB-06)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => vi.unstubAllGlobals());

  it("list: GET /api/profiles, возвращает ProfilesResponse", async () => {
    const data = {
      profiles: [],
      currentIds: { claude: "p1", claudeDesktop: null, codex: null },
    };
    fetchSpy.mockResolvedValue(okResponse(data));
    await expect(profilesApi.list()).resolves.toEqual(data);
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/profiles");
    expect(init.method).toBe("GET");
    expect(init.body).toBeUndefined();
  });

  it("create: POST /api/profiles с {name, scope}", async () => {
    fetchSpy.mockResolvedValue(okResponse({ id: "p1" }));
    await profilesApi.create("Work", "claude-desktop");
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/profiles");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body)).toEqual({
      name: "Work",
      scope: "claude-desktop",
    });
  });

  it("update: PATCH c url-encoded id и телом {name, resnapshot, scope}", async () => {
    fetchSpy.mockResolvedValue(okResponse({ id: "p/1 x" }));
    await profilesApi.update("p/1 x", {
      name: "New",
      resnapshot: true,
      scope: "codex",
    });
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/profiles/p%2F1%20x");
    expect(init.method).toBe("PATCH");
    expect(JSON.parse(init.body)).toEqual({
      name: "New",
      resnapshot: true,
      scope: "codex",
    });
  });

  it("update: только rename — resnapshot и scope не уходят в body", async () => {
    fetchSpy.mockResolvedValue(okResponse({ id: "p1" }));
    await profilesApi.update("p1", { name: "Only" });
    expect(JSON.parse(fetchSpy.mock.calls[0][1].body)).toEqual({
      name: "Only",
    });
  });

  it("delete: DELETE c url-encoded id, без body", async () => {
    fetchSpy.mockResolvedValue(okResponse(true));
    await profilesApi.delete("p/1 x");
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/profiles/p%2F1%20x");
    expect(init.method).toBe("DELETE");
    expect(init.body).toBeUndefined();
  });

  it("apply: POST /:id/apply с {scope}, возвращает warnings", async () => {
    fetchSpy.mockResolvedValue(okResponse(["warn-1"]));
    await expect(profilesApi.apply("p/1 x", "claude")).resolves.toEqual([
      "warn-1",
    ]);
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/profiles/p%2F1%20x/apply");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body)).toEqual({ scope: "claude" });
  });

  it("clearCurrent: POST /api/profiles/clear-current с {scope}", async () => {
    fetchSpy.mockResolvedValue(okResponse(true));
    await profilesApi.clearCurrent("codex");
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/profiles/clear-current");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body)).toEqual({ scope: "codex" });
  });

  it("apply: HTTP 400 {ok:false,error} отклоняется текстом сервера", async () => {
    fetchSpy.mockResolvedValue({
      status: 400,
      json: async () => ({ ok: false, error: "unknown scope: x" }),
    });
    await expect(profilesApi.apply("p1", "claude")).rejects.toThrow(
      new Error("unknown scope: x"),
    );
  });
});
