import { afterEach, describe, expect, it, vi } from "vitest";
import { fetchCapabilities } from "@/lib/api/capabilities";

afterEach(() => vi.unstubAllGlobals());

describe("fetchCapabilities", () => {
  it("returns the bare payload", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        json: async () => ({
          mode: "web",
          version: "1.2.3",
          platform: "macos",
        }),
      }),
    );
    await expect(fetchCapabilities()).resolves.toEqual({
      mode: "web",
      version: "1.2.3",
      buildSha: null,
      runtime_boundaries: "",
      platform: "macos",
      selfUpdate: false,
      webVersion: null,
      projectUrl: null,
    });
  });

  it("projectUrl is passed through and anything but a non-empty string is null", async () => {
    const answer = (projectUrl: unknown) =>
      vi.stubGlobal(
        "fetch",
        vi.fn().mockResolvedValue({
          ok: true,
          json: async () => ({ mode: "web", version: "1.2.3", projectUrl }),
        }),
      );
    answer("https://github.com/o/r");
    await expect(fetchCapabilities()).resolves.toMatchObject({
      projectUrl: "https://github.com/o/r",
    });
    for (const loose of [undefined, 1, null, ""]) {
      answer(loose);
      await expect(fetchCapabilities()).resolves.toMatchObject({
        projectUrl: null,
      });
    }
  });

  it("webVersion is passed through and anything but a string is null", async () => {
    const answer = (webVersion: unknown) =>
      vi.stubGlobal(
        "fetch",
        vi.fn().mockResolvedValue({
          ok: true,
          json: async () => ({ mode: "web", version: "1.2.3", webVersion }),
        }),
      );
    answer("4.0.35-web.1");
    await expect(fetchCapabilities()).resolves.toMatchObject({
      webVersion: "4.0.35-web.1",
    });
    for (const loose of [undefined, 1, null]) {
      answer(loose);
      await expect(fetchCapabilities()).resolves.toMatchObject({
        webVersion: null,
      });
    }
  });

  it("selfUpdate is true only for the boolean true", async () => {
    const answer = (selfUpdate: unknown) =>
      vi.stubGlobal(
        "fetch",
        vi.fn().mockResolvedValue({
          ok: true,
          json: async () => ({ mode: "web", version: "1.2.3", selfUpdate }),
        }),
      );
    answer(true);
    await expect(fetchCapabilities()).resolves.toMatchObject({
      selfUpdate: true,
    });
    for (const loose of ["true", 1, "1", null]) {
      answer(loose);
      await expect(fetchCapabilities()).resolves.toMatchObject({
        selfUpdate: false,
      });
    }
  });

  it("passes the build sha through", async () => {
    const sha = "0123456789abcdef0123456789abcdef01234567";
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        json: async () => ({ mode: "web", version: "1.2.3", buildSha: sha }),
      }),
    );
    await expect(fetchCapabilities()).resolves.toMatchObject({ buildSha: sha });
  });

  it.each([
    { buildSha: "", what: "an empty string" },
    { buildSha: null, what: "null" },
    { buildSha: 42, what: "a non-string" },
  ])("treats $what as no build sha", async ({ buildSha }) => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        ok: true,
        json: async () => ({ mode: "web", version: "1.2.3", buildSha }),
      }),
    );
    await expect(fetchCapabilities()).resolves.toMatchObject({
      buildSha: null,
    });
  });

  it("rejects on a non-2xx response", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
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
