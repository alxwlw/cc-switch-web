import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@/lib/api/adapter";

describe("adapter invoke · outbound fetchers", () => {
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

  it("diagnose_ip: POST /api/global-proxy/diagnose { url }", async () => {
    const report = { samples: [], location: null, locationError: null };
    respond(report);
    await expect(
      invoke("diagnose_ip", { url: "http://127.0.0.1:7890" }),
    ).resolves.toEqual(report);
    const { url, init } = lastCall();
    expect(url).toBe("/api/global-proxy/diagnose");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body as string)).toEqual({
      url: "http://127.0.0.1:7890",
    });
  });

  it("fetch_exchange_rates: GET /api/usage/exchange-rates", async () => {
    const rates = { rates: { CNY: 7.1 }, fetchedAt: 1, source: "er-api" };
    respond(rates);
    await expect(invoke("fetch_exchange_rates")).resolves.toEqual(rates);
    const { url, init } = lastCall();
    expect(url).toBe("/api/usage/exchange-rates");
    expect(init.method).toBe("GET");
    expect(init.body).toBeUndefined();
  });

  it("fetch_models_dev: GET /api/usage/models-dev, JSON text unchanged", async () => {
    const text = '{"anthropic":{"models":{}}}';
    respond(text);
    const result = await invoke<string>("fetch_models_dev");
    expect(result).toBe(text);
    expect(JSON.parse(result)).toEqual({ anthropic: { models: {} } });
    const { url, init } = lastCall();
    expect(url).toBe("/api/usage/models-dev");
    expect(init.method).toBe("GET");
  });
});
