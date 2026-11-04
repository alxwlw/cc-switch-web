import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// In web builds `@tauri-apps/api/core` is aliased to the HTTP adapter; mirror that here.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});

import { invoke } from "@tauri-apps/api/core";

const STATUS = {
  effective: "/srv/.cc-switch-web",
  source: "override",
  override: "/srv/next",
  envLocked: false,
  default: "/home/srv/.cc-switch",
};

describe("data directory and restart commands · web adapter (WEB-26)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => vi.unstubAllGlobals());

  const reply = (status: number, body: unknown) =>
    ({ status, json: async () => body }) as Response;

  it("get_app_config_dir_status returns the whole status", async () => {
    fetchSpy.mockResolvedValue(reply(200, { ok: true, data: STATUS }));
    expect(await invoke("get_app_config_dir_status")).toEqual(STATUS);
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/app-config-dir");
    expect(init.method).toBe("GET");
  });

  it("get_app_config_dir_override picks the stored override, null included", async () => {
    fetchSpy.mockResolvedValueOnce(reply(200, { ok: true, data: STATUS }));
    expect(await invoke("get_app_config_dir_override")).toBe("/srv/next");
    fetchSpy.mockResolvedValueOnce(
      reply(200, { ok: true, data: { ...STATUS, override: null } }),
    );
    expect(await invoke("get_app_config_dir_override")).toBeNull();
  });

  it("set_app_config_dir_override PUTs {path} and resolves true, as the desktop command does", async () => {
    fetchSpy.mockResolvedValue(reply(200, { ok: true, data: STATUS }));
    expect(
      await invoke("set_app_config_dir_override", { path: "/srv/next" }),
    ).toBe(true);
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/app-config-dir");
    expect(init.method).toBe("PUT");
    expect(JSON.parse(init.body)).toEqual({ path: "/srv/next" });

    await invoke("set_app_config_dir_override", { path: null });
    expect(JSON.parse(fetchSpy.mock.calls[1][1].body)).toEqual({ path: null });
  });

  it("set_app_config_dir_override rejects with the server's text on 400 and 409", async () => {
    fetchSpy.mockResolvedValueOnce(
      reply(400, { ok: false, error: "путь не абсолютный" }),
    );
    await expect(
      invoke("set_app_config_dir_override", { path: "x" }),
    ).rejects.toThrow("путь не абсолютный");
    fetchSpy.mockResolvedValueOnce(
      reply(409, { ok: false, error: "каталог задан CC_SWITCH_CONFIG_DIR" }),
    );
    await expect(
      invoke("set_app_config_dir_override", { path: "/a" }),
    ).rejects.toThrow("каталог задан CC_SWITCH_CONFIG_DIR");
  });

  it("restart_server POSTs /api/restart with a JSON content type and resolves the instance id", async () => {
    fetchSpy.mockResolvedValue(
      reply(202, { ok: true, data: { instanceId: "abc" } }),
    );
    expect(await invoke("restart_server")).toEqual({ instanceId: "abc" });
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/restart");
    expect(init.method).toBe("POST");
    expect(init.headers["Content-Type"]).toBe("application/json");
  });

  it("restart_app hits the same route and resolves true; a 409 surfaces its text", async () => {
    fetchSpy.mockResolvedValueOnce(
      reply(202, { ok: true, data: { instanceId: "abc" } }),
    );
    expect(await invoke("restart_app")).toBe(true);
    expect(fetchSpy.mock.calls[0][0]).toBe("/api/restart");

    fetchSpy.mockResolvedValueOnce(
      reply(409, { ok: false, error: "перезапустите сервер вручную" }),
    );
    await expect(invoke("restart_app")).rejects.toThrow(
      "перезапустите сервер вручную",
    );
  });
});
