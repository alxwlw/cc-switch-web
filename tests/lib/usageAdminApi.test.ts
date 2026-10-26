import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// In web builds `@tauri-apps/api/core` is aliased to the HTTP adapter; mirror that here.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});

import { usageApi } from "@/lib/api/usage";
import { proxyApi } from "@/lib/api/proxy";

describe("usage admin API · web adapter (WEB-12)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  const reply = (data: unknown) =>
    fetchSpy.mockResolvedValue({
      status: 200,
      json: async () => ({ ok: true, data }),
    } as Response);

  const lastCall = () => {
    const [url, init] = fetchSpy.mock.calls.at(-1)!;
    return {
      url: String(url),
      method: init?.method as string,
      body: init?.body ? JSON.parse(init.body as string) : undefined,
    };
  };

  it("updateModelPricing: PUT with the encoded model id and the cost fields", async () => {
    reply(null);
    await usageApi.updateModelPricing("vendor/m 1", "M", "1", "2", "3", "4");
    expect(lastCall()).toEqual({
      url: "/api/usage/model-pricing/vendor%2Fm%201",
      method: "PUT",
      body: {
        displayName: "M",
        inputCost: "1",
        outputCost: "2",
        cacheReadCost: "3",
        cacheCreationCost: "4",
      },
    });
  });

  it("updateModelPricingBatch: POST {entries}, returns the changed count", async () => {
    reply(2);
    const entries = [
      {
        modelId: "a",
        displayName: "A",
        inputCostPerMillion: "1",
        outputCostPerMillion: "2",
        cacheReadCostPerMillion: "3",
        cacheCreationCostPerMillion: "4",
      },
    ];
    expect(await usageApi.updateModelPricingBatch(entries)).toBe(2);
    expect(lastCall()).toEqual({
      url: "/api/usage/model-pricing/batch",
      method: "POST",
      body: { entries },
    });
  });

  it("updateModelPricingBatch: a server error rejects instead of returning a count", async () => {
    fetchSpy.mockResolvedValue({
      status: 200,
      json: async () => ({ ok: false, error: "bad entry" }),
    } as Response);
    await expect(usageApi.updateModelPricingBatch([])).rejects.toThrow(
      "bad entry",
    );
  });

  it("deleteModelPricing: DELETE with the encoded model id", async () => {
    reply(null);
    await usageApi.deleteModelPricing("vendor/m");
    expect(lastCall()).toEqual({
      url: "/api/usage/model-pricing/vendor%2Fm",
      method: "DELETE",
      body: undefined,
    });
  });

  it("getModelsDevSyncConfig: GET config", async () => {
    reply({ configPath: "p", config: {} });
    await usageApi.getModelsDevSyncConfig();
    expect(lastCall()).toEqual({
      url: "/api/usage/models-dev/config",
      method: "GET",
      body: undefined,
    });
  });

  it("saveModelsDevSyncConfig: PUT the bare config object, not {config}", async () => {
    reply(null);
    const config = {
      autoSyncEnabled: true,
      includeCommonModels: false,
      selectedModelKeys: ["a/b"],
      excludedCommonModelKeys: [],
      lastSyncAt: null,
      lastSyncError: null,
    };
    await usageApi.saveModelsDevSyncConfig(config);
    expect(lastCall()).toEqual({
      url: "/api/usage/models-dev/config",
      method: "PUT",
      body: config,
    });
  });

  it("recordModelsDevSyncResult: POST {syncedAt, error}", async () => {
    reply(null);
    await usageApi.recordModelsDevSyncResult(1234, null);
    expect(lastCall()).toEqual({
      url: "/api/usage/models-dev/result",
      method: "POST",
      body: { syncedAt: 1234, error: null },
    });
  });

  it("getSessionUsageLastSync: GET last-sync", async () => {
    reply(99);
    expect(await usageApi.getSessionUsageLastSync()).toBe(99);
    expect(lastCall()).toEqual({
      url: "/api/usage/last-sync",
      method: "GET",
      body: undefined,
    });
  });

  it("rebuildCodexUsage: POST rebuild-codex", async () => {
    reply({ imported: 1 });
    await usageApi.rebuildCodexUsage();
    expect(lastCall()).toEqual({
      url: "/api/usage/rebuild-codex",
      method: "POST",
      body: undefined,
    });
  });

  it("getDataSourceBreakdown: GET data-sources", async () => {
    reply([]);
    await usageApi.getDataSourceBreakdown();
    expect(lastCall()).toEqual({
      url: "/api/usage/data-sources",
      method: "GET",
      body: undefined,
    });
  });

  it("getPricingModelSource: GET /api/:app/pricing-model-source", async () => {
    reply("response");
    expect(await proxyApi.getPricingModelSource("claude")).toBe("response");
    expect(lastCall()).toEqual({
      url: "/api/claude/pricing-model-source",
      method: "GET",
      body: undefined,
    });
  });

  it("setPricingModelSource: PUT {value}", async () => {
    reply(null);
    await proxyApi.setPricingModelSource("codex", "request");
    expect(lastCall()).toEqual({
      url: "/api/codex/pricing-model-source",
      method: "PUT",
      body: { value: "request" },
    });
  });
});
