import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useDiscoverableSkills } from "@/hooks/useSkills";
import { createTestQueryClient } from "../utils/testQueryClient";
import { setWeb } from "../helpers/web";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

function wrapper({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider client={createTestQueryClient()}>
      {children}
    </QueryClientProvider>
  );
}

const discoverCalls = () =>
  invokeMock.mock.calls.filter(
    (call) => call[0] === "discover_available_skills",
  );

describe("WEB-05: discover skills работает в web", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue({ skills: [], failures: [] });
  });

  it("useDiscoverableSkills: web зовёт discover_available_skills", async () => {
    setWeb(true);
    renderHook(() => useDiscoverableSkills(), { wrapper });
    await act(async () => {
      await Promise.resolve();
    });
    expect(discoverCalls()).toHaveLength(1);
  });
});
