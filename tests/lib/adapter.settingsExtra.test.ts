import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@/lib/api/adapter";

describe("adapter invoke · privacy filter, config status, sync clear-settings", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;

  const respond = (data: unknown) =>
    fetchSpy.mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({ ok: true, data }),
    });

  const lastCall = () => {
    const [url, init] = fetchSpy.mock.calls[0] as [string, RequestInit];
    return { url, init };
  };

  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("get_privacy_filter_config: GET /api/settings/privacy-filter", async () => {
    respond({ enabled: false });
    await expect(invoke("get_privacy_filter_config")).resolves.toEqual({
      enabled: false,
    });
    const { url, init } = lastCall();
    expect(url).toBe("/api/settings/privacy-filter");
    expect(init.method).toBe("GET");
  });

  it("set_privacy_filter_config: PUT { config }", async () => {
    respond(true);
    const config = { enabled: true };
    await expect(invoke("set_privacy_filter_config", { config })).resolves.toBe(
      true,
    );
    const { url, init } = lastCall();
    expect(url).toBe("/api/settings/privacy-filter");
    expect(init.method).toBe("PUT");
    expect(JSON.parse(init.body as string)).toEqual({ config });
  });

  it("get_config_status: GET /api/config-status/:app", async () => {
    respond({ exists: false, path: "/x" });
    await expect(
      invoke("get_config_status", { app: "workbuddy" }),
    ).resolves.toEqual({ exists: false, path: "/x" });
    const { url, init } = lastCall();
    expect(url).toBe("/api/config-status/workbuddy");
    expect(init.method).toBe("GET");
  });

  it.each([
    ["webdav_sync_clear_settings", "/api/sync/webdav/settings"],
    ["s3_sync_clear_settings", "/api/sync/s3/settings"],
  ])("%s: DELETE %s without a body", async (cmd, expectedUrl) => {
    respond({ success: true });
    await expect(invoke(cmd)).resolves.toEqual({ success: true });
    const { url, init } = lastCall();
    expect(url).toBe(expectedUrl);
    expect(init.method).toBe("DELETE");
    expect(init.body).toBeUndefined();
  });
});
