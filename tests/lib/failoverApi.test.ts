import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { failoverApi } from "@/lib/api/failover";

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

describe("failover API over the web adapter (WEB-01 stage 2)", () => {
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

  const breakerConfig = {
    failureThreshold: 4,
    successThreshold: 2,
    timeoutSeconds: 60,
    errorRateThreshold: 0.5,
    minRequests: 10,
  };

  // [name, call, method, url, body]
  const CASES: Array<
    [string, () => Promise<unknown>, string, string, unknown]
  > = [
    [
      "provider health (ids are percent-encoded)",
      () => failoverApi.getProviderHealth("a b/c", "claude"),
      "GET",
      "/api/failover/health/claude/a%20b%2Fc",
      undefined,
    ],
    [
      "reset circuit breaker",
      () => failoverApi.resetCircuitBreaker("p1", "codex"),
      "POST",
      "/api/failover/breaker/codex/p1/reset",
      undefined,
    ],
    [
      "get breaker config",
      () => failoverApi.getCircuitBreakerConfig(),
      "GET",
      "/api/failover/breaker-config",
      undefined,
    ],
    [
      "update breaker config (body is the config itself, not wrapped)",
      () =>
        failoverApi.updateCircuitBreakerConfig(
          breakerConfig as unknown as Parameters<
            typeof failoverApi.updateCircuitBreakerConfig
          >[0],
        ),
      "PUT",
      "/api/failover/breaker-config",
      breakerConfig,
    ],
    [
      "breaker stats",
      () => failoverApi.getCircuitBreakerStats("p1", "gemini"),
      "GET",
      "/api/failover/breaker-stats/gemini/p1",
      undefined,
    ],
    [
      "failover queue",
      () => failoverApi.getFailoverQueue("claude"),
      "GET",
      "/api/failover/queue/claude",
      undefined,
    ],
    [
      "available providers",
      () => failoverApi.getAvailableProvidersForFailover("codex"),
      "GET",
      "/api/failover/available/codex",
      undefined,
    ],
    [
      "add to queue",
      () => failoverApi.addToFailoverQueue("claude", "p1"),
      "POST",
      "/api/failover/queue/claude/p1",
      undefined,
    ],
    [
      "remove from queue",
      () => failoverApi.removeFromFailoverQueue("claude", "p1"),
      "DELETE",
      "/api/failover/queue/claude/p1",
      undefined,
    ],
    [
      "get auto failover",
      () => failoverApi.getAutoFailoverEnabled("claude"),
      "GET",
      "/api/failover/auto/claude",
      undefined,
    ],
    [
      "set auto failover",
      () => failoverApi.setAutoFailoverEnabled("claude", true),
      "PUT",
      "/api/failover/auto/claude",
      { enabled: true },
    ],
  ];

  it.each(CASES)("%s", async (_name, call, method, url, body) => {
    await call();
    expect(lastCall()).toEqual({ url, method, body });
  });

  it("returns the unwrapped data", async () => {
    fetchSpy.mockResolvedValue(ok([{ providerId: "p1" }]));
    await expect(failoverApi.getFailoverQueue("claude")).resolves.toEqual([
      { providerId: "p1" },
    ]);
  });

  it("keeps a string error from the envelope as an Error with that message", async () => {
    fetchSpy.mockResolvedValue({
      ok: false,
      status: 400,
      json: async () => ({ ok: false, error: "queue is full" }),
    });
    const thrown = await failoverApi
      .addToFailoverQueue("claude", "p1")
      .catch((e) => e);
    expect(thrown).toBeInstanceOf(Error);
    expect(thrown.message).toBe("queue is full");
  });
});
