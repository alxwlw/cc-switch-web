import { afterEach, describe, expect, it, vi } from "vitest";
import { fetchCapabilities } from "@/lib/api/capabilities";

afterEach(() => vi.unstubAllGlobals());

describe("fetchCapabilities", () => {
  it("returns the bare payload", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        json: async () => ({ mode: "web", version: "1.2.3" }),
      }),
    );
    await expect(fetchCapabilities()).resolves.toEqual({
      mode: "web",
      version: "1.2.3",
      runtime_boundaries: "",
    });
  });

  it("rejects on a non-2xx response", async () => {
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValue({
          ok: false,
          status: 500,
          json: async () => ({ version: "1" }),
        }),
    );
    await expect(fetchCapabilities()).rejects.toThrow("HTTP 500");
  });

  it("rejects a payload without a string version", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: true, json: async () => ({}) }),
    );
    await expect(fetchCapabilities()).rejects.toThrow("unexpected payload");
  });
});
