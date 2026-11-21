import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useWorkBuddySwitch } from "@/components/providers/useWorkBuddyOfficial";
import { setWeb } from "../helpers/web";

const mocks = vi.hoisted(() => ({
  liveState: vi.fn(),
  setDefault: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("@/lib/toast", () => ({
  toast: { success: mocks.toastSuccess, error: mocks.toastError },
}));
vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    authApi: {
      ...actual.authApi,
      workbuddyAccountLiveState: mocks.liveState,
      authSetDefaultAccount: mocks.setDefault,
    },
  };
});

const wrapper = ({ children }: { children: ReactNode }) => (
  <QueryClientProvider client={new QueryClient()}>
    {children}
  </QueryClientProvider>
);

const switchOnce = async () => {
  const { result } = renderHook(() => useWorkBuddySwitch(), { wrapper });
  await act(async () => {
    await result.current.switchToAccount("u-1", "Alice");
  });
  expect(mocks.toastSuccess).toHaveBeenCalledOnce();
  return mocks.toastSuccess.mock.calls[0][1] as Record<string, unknown>;
};

describe("useWorkBuddySwitch success toast", () => {
  beforeEach(() => {
    for (const mock of Object.values(mocks)) mock.mockReset();
    mocks.liveState.mockResolvedValue({ running: false });
    mocks.setDefault.mockResolvedValue(undefined);
  });

  it("offers no open-app action in the web build", async () => {
    setWeb(true);
    expect(await switchOnce()).not.toHaveProperty("action");
  });

  it("offers the open-app action on desktop", async () => {
    setWeb(false);
    expect(await switchOnce()).toHaveProperty("action.onClick");
  });
});
