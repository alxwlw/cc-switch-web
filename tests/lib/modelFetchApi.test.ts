import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  fetchCodexOauthModels,
  fetchModelsForConfig,
  fetchXaiOauthModels,
  getAgyModels,
  getOpenCodeModels,
  showFetchModelsError,
} from "@/lib/api/model-fetch";
import {
  copilotGetModels,
  copilotGetModelsForAccount,
} from "@/lib/api/copilot";
import { toast } from "@/lib/toast";

// In the web build `@tauri-apps/api/core` is aliased to the adapter; route invoke through it.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter =
    await vi.importActual<typeof import("@/lib/api/adapter")>(
      "@/lib/api/adapter",
    );
  return { invoke: adapter.invoke };
});

vi.mock("@/lib/toast", () => ({
  toast: { error: vi.fn() },
}));

const ok = (data: unknown) => ({
  ok: true,
  status: 200,
  json: async () => ({ ok: true, data }),
});

describe("model list APIs over the web adapter (WEB-03)", () => {
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
      "fetch models for config (camelCase body as invoke sends)",
      () =>
        fetchModelsForConfig(
          "https://api.example.com/v1",
          "sk-test",
          true,
          "https://api.example.com/v1/models",
          "cc-switch/1.0",
          {
            apiFormat: "openai_chat",
            requestHeaders: { "X-Custom": "1" },
          },
        ),
      "POST",
      "/api/models/fetch",
      {
        baseUrl: "https://api.example.com/v1",
        apiKey: "sk-test",
        isFullUrl: true,
        modelsUrl: "https://api.example.com/v1/models",
        customUserAgent: "cc-switch/1.0",
        apiFormat: "openai_chat",
        requestHeaders: { "X-Custom": "1" },
      },
    ],
    [
      "agy models",
      () =>
        getAgyModels({
          apiKey: "fixture-key",
          baseUrl: "https://relay.example/gemini",
        }),
      "POST",
      "/api/models/agy",
      { apiKey: "fixture-key", baseUrl: "https://relay.example/gemini" },
    ],
    [
      "opencode models",
      () => getOpenCodeModels(),
      "GET",
      "/api/models/opencode",
      undefined,
    ],
    [
      "codex oauth models with account",
      () => fetchCodexOauthModels("chatgpt-1"),
      "GET",
      "/api/models/codex-oauth?accountId=chatgpt-1",
      undefined,
    ],
    [
      "codex oauth models without account (no query)",
      () => fetchCodexOauthModels(null),
      "GET",
      "/api/models/codex-oauth",
      undefined,
    ],
    [
      "xai oauth models with account",
      () => fetchXaiOauthModels("xai-account"),
      "GET",
      "/api/models/xai-oauth?accountId=xai-account",
      undefined,
    ],
    [
      "xai oauth models without account (no query)",
      () => fetchXaiOauthModels(null),
      "GET",
      "/api/models/xai-oauth",
      undefined,
    ],
    [
      "copilot models",
      () => copilotGetModels(),
      "GET",
      "/api/models/copilot",
      undefined,
    ],
    [
      "copilot models for account (segment is percent-encoded)",
      () => copilotGetModelsForAccount("gh 1/2"),
      "GET",
      "/api/models/copilot/gh%201%2F2",
      undefined,
    ],
  ];

  it.each(CASES)("%s", async (_name, call, method, url, body) => {
    await call();
    expect(lastCall()).toEqual({ url, method, body });
  });

  it("returns the unwrapped data (CopilotModel stays snake_case)", async () => {
    fetchSpy.mockResolvedValue(
      ok([
        {
          id: "gpt-5.6",
          name: "GPT-5.6",
          vendor: "openai",
          model_picker_enabled: true,
        },
      ]),
    );
    await expect(copilotGetModels()).resolves.toEqual([
      {
        id: "gpt-5.6",
        name: "GPT-5.6",
        vendor: "openai",
        model_picker_enabled: true,
      },
    ]);
  });

  it("keeps a string error from the envelope as an Error with that message", async () => {
    fetchSpy.mockResolvedValue({
      ok: false,
      status: 400,
      json: async () => ({ ok: false, error: "HTTP 401 Unauthorized" }),
    });
    const thrown = await fetchModelsForConfig(
      "https://api.example.com/v1",
      "bad-key",
      false,
    ).catch((e) => e);
    expect(thrown).toBeInstanceOf(Error);
    expect(thrown.message).toBe("HTTP 401 Unauthorized");
  });

  describe("showFetchModelsError on adapter-shaped errors", () => {
    const t = ((key: string) => key) as unknown as Parameters<
      typeof showFetchModelsError
    >[1];

    beforeEach(() => {
      vi.mocked(toast.error).mockClear();
    });

    it("401 from the adapter picks fetchModelsAuthFailed", () => {
      showFetchModelsError(new Error("HTTP 401 Unauthorized"), t);
      expect(toast.error).toHaveBeenCalledWith(
        "providerForm.fetchModelsAuthFailed",
      );
    });

    it("timeout from the adapter picks fetchModelsTimeout", () => {
      showFetchModelsError(new Error("request timed out after 15s"), t);
      expect(toast.error).toHaveBeenCalledWith(
        "providerForm.fetchModelsTimeout",
      );
    });
  });
});
