import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usageApi } from "@/lib/api/usage";
import { subscriptionApi } from "@/lib/api/subscription";
import { streamCheckProvider } from "@/lib/api/connectivity-check";

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

describe("usage/subscription/balance/stream-check over the web adapter (WEB-04)", () => {
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
      "query provider usage (no body, ids are percent-encoded)",
      () => usageApi.query("a b/c", "claude"),
      "POST",
      "/api/claude/providers/a%20b%2Fc/usage",
      undefined,
    ],
    [
      "test usage script (body mirrors invoke args, path wins)",
      () =>
        usageApi.testScript(
          "p1",
          "claude",
          "code",
          10,
          "k",
          "https://x",
          "tok",
          "u1",
          "custom",
        ),
      "POST",
      "/api/claude/providers/p1/usage-script/test",
      {
        scriptCode: "code",
        timeout: 10,
        apiKey: "k",
        baseUrl: "https://x",
        accessToken: "tok",
        userId: "u1",
        templateType: "custom",
      },
    ],
    [
      "check provider limits",
      () => usageApi.checkProviderLimits("p1", "claude"),
      "GET",
      "/api/claude/providers/p1/limits",
      undefined,
    ],
    [
      "subscription quota (CLI tool)",
      () => subscriptionApi.getQuota("claude"),
      "GET",
      "/api/subscription/claude/quota",
      undefined,
    ],
    [
      "codex oauth quota",
      () => subscriptionApi.getCodexOauthQuota("a1"),
      "GET",
      "/api/subscription/codex-oauth/a1/quota",
      undefined,
    ],
    [
      "codex oauth quota (null falls back to default placeholder)",
      () => subscriptionApi.getCodexOauthQuota(null),
      "GET",
      "/api/subscription/codex-oauth/default/quota",
      undefined,
    ],
    [
      "xai oauth quota",
      () => subscriptionApi.getXaiOauthQuota("a1"),
      "GET",
      "/api/subscription/xai-oauth/a1/quota",
      undefined,
    ],
    [
      "agy account quota",
      () => subscriptionApi.getAgyAccountQuota("a1"),
      "GET",
      "/api/subscription/agy/a1/quota",
      undefined,
    ],
    [
      "coding plan quota (full camelCase body)",
      () =>
        subscriptionApi.getCodingPlanQuota(
          "https://x",
          "k",
          "ak",
          "sk",
          "volcengine",
          "org1",
          "proj1",
        ),
      "POST",
      "/api/subscription/coding-plan",
      {
        baseUrl: "https://x",
        apiKey: "k",
        accessKeyId: "ak",
        secretAccessKey: "sk",
        codingPlanProvider: "volcengine",
        teamOrganizationId: "org1",
        teamProjectId: "proj1",
      },
    ],
    [
      "balance ({baseUrl, apiKey})",
      () => subscriptionApi.getBalance("https://x", "k"),
      "POST",
      "/api/balance",
      { baseUrl: "https://x", apiKey: "k" },
    ],
    [
      "stream check (no body)",
      () => streamCheckProvider("claude", "p1"),
      "POST",
      "/api/claude/providers/p1/stream-check",
      undefined,
    ],
  ];

  it.each(CASES)("%s", async (_name, call, method, url, body) => {
    await call();
    expect(lastCall()).toEqual({ url, method, body });
  });

  it("returns the unwrapped data", async () => {
    fetchSpy.mockResolvedValue(ok({ success: true }));
    await expect(usageApi.query("p1", "claude")).resolves.toEqual({
      success: true,
    });
  });

  it("keeps a string error from the envelope as an Error with that message", async () => {
    fetchSpy.mockResolvedValue({
      ok: false,
      status: 400,
      json: async () => ({ ok: false, error: "quota is full" }),
    });
    const thrown = await subscriptionApi
      .getBalance("https://x", "k")
      .catch((e) => e);
    expect(thrown).toBeInstanceOf(Error);
    expect(thrown.message).toBe("quota is full");
  });
});
