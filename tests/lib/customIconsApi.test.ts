import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// In web builds `@tauri-apps/api/core` is aliased to the HTTP adapter; mirror that here.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});

import { customIconsApi } from "@/lib/api/customIcons";

const entry = {
  id: "icon-1",
  name: "Logo",
  data: "data:image/png;base64,AAAA",
  createdAt: 1761350400,
};

describe("customIconsApi · web adapter (WEB-08)", () => {
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

  it("list: GET /api/custom-icons", async () => {
    fetchSpy.mockResolvedValue(jsonResponse({ ok: true, data: [entry] }));
    expect(await customIconsApi.list()).toEqual([entry]);
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/custom-icons");
    expect(init.method).toBe("GET");
  });

  it("add: POST /api/custom-icons с {name,data}, existed пробрасывается", async () => {
    fetchSpy.mockResolvedValue(
      jsonResponse({ ok: true, data: { icon: entry, existed: true } }),
    );
    const res = await customIconsApi.add("New name", entry.data);
    expect(res).toEqual({ icon: entry, existed: true });
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/custom-icons");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body)).toEqual({
      name: "New name",
      data: entry.data,
    });
  });

  it("rename: PUT /api/custom-icons/:id с {name}, id экранируется", async () => {
    fetchSpy.mockResolvedValue(
      jsonResponse({ ok: true, data: { ...entry, name: "Renamed" } }),
    );
    const res = await customIconsApi.rename("a/b c", "Renamed");
    expect(res.name).toBe("Renamed");
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/custom-icons/a%2Fb%20c");
    expect(init.method).toBe("PUT");
    expect(JSON.parse(init.body)).toEqual({ name: "Renamed" });
  });

  it("remove: DELETE /api/custom-icons/:id без тела", async () => {
    fetchSpy.mockResolvedValue(jsonResponse({ ok: true, data: true }));
    await customIconsApi.remove("icon-1");
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/custom-icons/icon-1");
    expect(init.method).toBe("DELETE");
    expect(init.body).toBeUndefined();
  });

  it("ошибка валидации {ok:false,error} (HTTP 500) пробрасывается дословно", async () => {
    fetchSpy.mockResolvedValue(
      jsonResponse({ ok: false, error: "Icon name must not be empty" }, 500),
    );
    await expect(customIconsApi.add("", entry.data)).rejects.toThrow(
      "Icon name must not be empty",
    );
  });
});
