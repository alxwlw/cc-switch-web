import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useServerCapabilities } from "@/hooks/useServerCapabilities";
import { setWeb } from "../helpers/web";

afterEach(() => vi.unstubAllGlobals());

describe("useServerCapabilities", () => {
  it("web: a failed request settles to error after one call, no retry", async () => {
    setWeb(true);
    const fetchMock = vi.fn().mockRejectedValue(new Error("down"));
    vi.stubGlobal("fetch", fetchMock);
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={new QueryClient()}>
        {children}
      </QueryClientProvider>
    );
    const { result } = renderHook(() => useServerCapabilities(), { wrapper });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});
