import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// In web builds `@tauri-apps/api/core` is aliased to the HTTP adapter; mirror that here.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});

import { invoke } from "@tauri-apps/api/core";

describe("get_init_error · web adapter (WEB-32)", () => {
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

  it("GET /api/init-status and returns the payload untouched", async () => {
    const payload = {
      path: "/srv/.cc-switch/cc-switch.db",
      error: "db too new",
      kind: "db_version_too_new",
      db_version: 9999,
      supported_version: 23,
    };
    fetchSpy.mockResolvedValue(jsonResponse({ ok: true, data: payload }));

    expect(await invoke("get_init_error")).toEqual(payload);

    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/init-status");
    expect(init.method).toBe("GET");
  });

  it("passes null through when there is no init error", async () => {
    fetchSpy.mockResolvedValue(jsonResponse({ ok: true, data: null }));

    expect(await invoke("get_init_error")).toBeNull();
  });
});
