import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { providersApi } from "@/lib/api/providers";

// In the web build `@tauri-apps/api/core` is aliased to the adapter; route invoke through it.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter =
    await vi.importActual<typeof import("@/lib/api/adapter")>(
      "@/lib/api/adapter",
    );
  return { invoke: adapter.invoke };
});

const ok = (data: unknown) => ({
  ok: true,
  status: 200,
  json: async () => ({ ok: true, data }),
});

describe("opencode API over the web adapter (WEB-14)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    fetchSpy = vi.fn().mockResolvedValue(ok(null));
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  const lastCall = () => {
    const [url, init] = fetchSpy.mock.calls.at(-1)!;
    return {
      url,
      method: init.method,
      body: init.body === undefined ? undefined : JSON.parse(init.body),
    };
  };

  // [name, call, method, url, body]
  const CASES: Array<
    [string, () => Promise<unknown>, string, string, unknown]
  > = [
    [
      "import from live",
      () => providersApi.importOpenCodeFromLive(),
      "POST",
      "/api/opencode/import-live",
      undefined,
    ],
    [
      "live provider ids",
      () => providersApi.getOpenCodeLiveProviderIds(),
      "GET",
      "/api/opencode/live-provider-ids",
      undefined,
    ],
    [
      "get default model",
      () => providersApi.getOpenCodeDefaultModel(),
      "GET",
      "/api/opencode/default-model",
      undefined,
    ],
    [
      "set default model (value under `model`)",
      () => providersApi.setOpenCodeDefaultModel("p/m"),
      "PUT",
      "/api/opencode/default-model",
      { model: "p/m" },
    ],
  ];

  it.each(CASES)("%s", async (_name, call, method, url, body) => {
    await call();
    expect(lastCall()).toEqual({ url, method, body });
  });

  it("returns the unwrapped data", async () => {
    fetchSpy.mockResolvedValue(ok(["a", "b"]));
    await expect(providersApi.getOpenCodeLiveProviderIds()).resolves.toEqual([
      "a",
      "b",
    ]);
    fetchSpy.mockResolvedValue(ok("p/m"));
    await expect(providersApi.getOpenCodeDefaultModel()).resolves.toBe("p/m");
    fetchSpy.mockResolvedValue(ok(null));
    await expect(providersApi.getOpenCodeDefaultModel()).resolves.toBeNull();
    fetchSpy.mockResolvedValue(ok(2));
    await expect(providersApi.importOpenCodeFromLive()).resolves.toBe(2);
  });

  it("keeps a string error from the envelope as an Error with that message", async () => {
    fetchSpy.mockResolvedValue({
      ok: false,
      status: 500,
      json: async () => ({ ok: false, error: "no live config" }),
    });
    const thrown = await providersApi
      .getOpenCodeLiveProviderIds()
      .catch((e) => e);
    expect(thrown).toBeInstanceOf(Error);
    expect(thrown.message).toBe("no live config");
  });
});
