import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useProxyStatus } from "@/hooks/useProxyStatus";
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

const ok = (data: unknown) => ({
  ok: true,
  status: 200,
  json: async () => ({ ok: true, data }),
});

describe("useProxyStatus in web polls through the adapter (WEB-01)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    vi.useFakeTimers();
    setWeb(true);
    fetchSpy = vi.fn(async (url: string) =>
      url === "/api/proxy/status"
        ? ok({ running: true, address: "127.0.0.1", port: 15721 })
        : ok({ claude: false }),
    );
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  const count = (url: string) =>
    fetchSpy.mock.calls.filter(([u]) => u === url).length;

  it("requests status and takeover over HTTP and repeats the poll while running", async () => {
    const client = createTestQueryClient();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useProxyStatus(), { wrapper });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(result.current.isRunning).toBe(true);
    expect(result.current.isInitialStatusPending).toBe(false);
    expect(count("/api/proxy/status")).toBe(1);
    expect(count("/api/proxy/takeover")).toBe(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2100);
    });
    expect(count("/api/proxy/status")).toBe(2);
    for (const [, init] of fetchSpy.mock.calls) {
      expect(init.method).toBe("GET");
    }
  });
});
