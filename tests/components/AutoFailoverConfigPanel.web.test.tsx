import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AutoFailoverConfigPanel } from "@/components/proxy/AutoFailoverConfigPanel";
import { createTestQueryClient } from "../utils/testQueryClient";
import { setWeb } from "../helpers/web";

// In the web build `@tauri-apps/api/core` is aliased to the adapter; route invoke through it.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter =
    await vi.importActual<typeof import("@/lib/api/adapter")>(
      "@/lib/api/adapter",
    );
  return { invoke: adapter.invoke };
});

const stored = {
  appType: "claude",
  enabled: true,
  autoFailoverEnabled: false,
  maxRetries: 3,
  streamingFirstByteTimeout: 60,
  streamingIdleTimeout: 120,
  nonStreamingTimeout: 600,
  circuitFailureThreshold: 5,
  circuitSuccessThreshold: 2,
  circuitTimeoutSeconds: 60,
  circuitErrorRateThreshold: 0.5,
  circuitMinRequests: 10,
};

const ok = (data: unknown) => ({
  ok: true,
  status: 200,
  json: async () => ({ ok: true, data }),
});

describe("AutoFailoverConfigPanel — per-app proxy config over web (WEB-01)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    setWeb(true);
    fetchSpy = vi.fn(async (_url: string, init: { method: string }) =>
      ok(init.method === "GET" ? stored : null),
    );
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("loads GET /api/proxy/config/claude and saves the edited retries by PUT", async () => {
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <AutoFailoverConfigPanel appType="claude" />
      </QueryClientProvider>,
    );

    const retries = (await waitFor(() => {
      const el = document.querySelector<HTMLInputElement>("#maxRetries-claude");
      expect(el).not.toBeNull();
      expect(el!.value).toBe("3");
      return el;
    })) as HTMLInputElement;
    expect(
      fetchSpy.mock.calls.some(
        ([url, init]) =>
          url === "/api/proxy/config/claude" && init.method === "GET",
      ),
    ).toBe(true);

    fireEvent.change(retries, { target: { value: "7" } });
    fireEvent.click(screen.getByRole("button", { name: /common\.save|保存/ }));

    await waitFor(() =>
      expect(
        fetchSpy.mock.calls.some(([, init]) => init.method === "PUT"),
      ).toBe(true),
    );
    const put = fetchSpy.mock.calls.find(([, init]) => init.method === "PUT")!;
    expect(put[0]).toBe("/api/proxy/config/claude");
    expect(JSON.parse(put[1].body)).toMatchObject({
      appType: "claude",
      maxRetries: 7,
      nonStreamingTimeout: 600,
    });
  });
});
