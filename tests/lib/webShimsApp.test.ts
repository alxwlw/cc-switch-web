import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getName, getVersion } from "@/lib/web-shims/app";

const fetchMock = vi.fn();

describe("web shim · @tauri-apps/api/app", () => {
  beforeEach(() => {
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });
  afterEach(() => vi.unstubAllGlobals());

  it("getVersion is the version /api/capabilities reports, with nothing added", async () => {
    fetchMock.mockResolvedValue({
      ok: true,
      json: async () => ({ mode: "web", version: "4.0.20" }),
    });
    await expect(getVersion()).resolves.toBe("4.0.20");
    expect(fetchMock).toHaveBeenCalledWith("/api/capabilities");
  });

  it("getVersion rejects when the server cannot say, instead of inventing a number", async () => {
    fetchMock.mockResolvedValue({
      ok: false,
      status: 502,
      json: async () => ({}),
    });
    await expect(getVersion()).rejects.toThrow("HTTP 502");
  });

  it("getName stays", async () => {
    await expect(getName()).resolves.toBe("cc-switch-web");
  });
});
