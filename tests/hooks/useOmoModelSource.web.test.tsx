import type { ReactNode } from "react";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { providersApi } from "@/lib/api";
import * as modelFetch from "@/lib/api/model-fetch";
import { useOmoModelSource } from "@/components/providers/forms/hooks/useOmoModelSource";
import { setProviders } from "../msw/state";
import { createTestQueryClient } from "../utils/testQueryClient";
import { setWeb } from "../helpers/web";

const toastWarning = vi.hoisted(() => vi.fn());
vi.mock("@/lib/toast", () => ({
  toast: { warning: toastWarning, error: vi.fn(), success: vi.fn() },
}));

const wrapper =
  (client = createTestQueryClient()) =>
  ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );

describe("useOmoModelSource — web", () => {
  beforeEach(() => {
    toastWarning.mockClear();
    setProviders("opencode", {
      custom: {
        id: "custom",
        name: "Custom",
        settingsConfig: {
          npm: "@ai-sdk/openai-compatible",
          options: { baseURL: "https://x" },
          models: { "m-1": { name: "M1" } },
        },
        category: "custom",
      },
    });
  });

  it("offers configured models without the live-state calls or failure toasts", async () => {
    setWeb(true);
    const liveIds = vi.spyOn(providersApi, "getOpenCodeLiveProviderIds");
    const runtime = vi.spyOn(modelFetch, "getOpenCodeModels");

    const { result } = renderHook(
      () => useOmoModelSource({ isOmoCategory: true }),
      { wrapper: wrapper() },
    );

    await waitFor(() =>
      expect(result.current.omoModelOptions.map((o) => o.value)).toEqual([
        "custom/m-1",
      ]),
    );
    expect(liveIds).not.toHaveBeenCalled();
    expect(runtime).not.toHaveBeenCalled();
    expect(toastWarning).not.toHaveBeenCalled();
  });

  it("still asks for the live state on desktop", async () => {
    const liveIds = vi
      .spyOn(providersApi, "getOpenCodeLiveProviderIds")
      .mockResolvedValue(["custom"]);
    vi.spyOn(modelFetch, "getOpenCodeModels").mockResolvedValue([]);

    const { result } = renderHook(
      () => useOmoModelSource({ isOmoCategory: true }),
      { wrapper: wrapper() },
    );

    await waitFor(() =>
      expect(result.current.omoModelOptions.map((o) => o.value)).toEqual([
        "custom/m-1",
      ]),
    );
    expect(liveIds).toHaveBeenCalledTimes(1);
  });
});
