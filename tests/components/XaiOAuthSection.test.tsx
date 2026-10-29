import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { XaiOAuthSection } from "@/components/providers/forms/XaiOAuthSection";

const mockUseXaiOauth = vi.hoisted(() => vi.fn());
const mockCopyText = vi.hoisted(() => vi.fn());

vi.mock("@/lib/clipboard", () => ({ copyText: mockCopyText }));

vi.mock("@/components/providers/forms/hooks/useXaiOauth", () => ({
  useXaiOauth: mockUseXaiOauth,
}));

// 「N 个供应商在用」要读供应商列表（React Query）；这里不关心，给空
vi.mock("@/components/providers/forms/hooks/useManagedAccountUsers", () => ({
  useManagedAccountUsers: () => () => [],
}));

describe("XaiOAuthSection", () => {
  beforeEach(() => {
    mockUseXaiOauth.mockReturnValue({
      accounts: [
        {
          id: "expired-account",
          login: "expired@example.com",
          avatar_url: null,
          authenticated_at: 1,
          github_domain: "x.ai",
          requires_reauth: true,
        },
        {
          id: "usable-account",
          login: "usable@example.com",
          avatar_url: null,
          authenticated_at: 2,
          github_domain: "x.ai",
          requires_reauth: false,
        },
      ],
      defaultAccountId: "usable-account",
      hasAnyAccount: true,
      isAuthenticated: true,
      pollingState: "idle",
      deviceCode: null,
      error: null,
      isPolling: false,
      isAddingAccount: false,
      isRemovingAccount: false,
      isSettingDefaultAccount: false,
      addAccount: vi.fn(),
      removeAccount: vi.fn(),
      setDefaultAccount: vi.fn(),
      cancelAuth: vi.fn(),
      logout: vi.fn(),
    });
  });

  it("keeps a selected account visible when it requires reauthentication", () => {
    render(
      <XaiOAuthSection
        selectedAccountId="expired-account"
        onAccountSelect={vi.fn()}
      />,
    );

    expect(screen.getByRole("combobox")).toHaveTextContent(
      "expired@example.com",
    );
    expect(screen.getByRole("combobox")).toHaveTextContent("凭据已失效");
  });

  it("keeps the device code visible when copying it is rejected", async () => {
    mockCopyText.mockRejectedValueOnce(new Error("clipboard denied"));
    const base = mockUseXaiOauth();
    mockUseXaiOauth.mockReturnValue({
      ...base,
      pollingState: "polling",
      isPolling: true,
      deviceCode: {
        device_code: "dev-1",
        user_code: "ABCD-1234",
        verification_uri: "https://x.ai/device",
        expires_in: 600,
        interval: 5,
      },
    });
    const unhandled = vi.fn();
    process.on("unhandledRejection", unhandled);
    render(<XaiOAuthSection onAccountSelect={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "common.copy" }));

    await waitFor(() => expect(mockCopyText).toHaveBeenCalledWith("ABCD-1234"));
    await new Promise((r) => setTimeout(r, 0));
    process.off("unhandledRejection", unhandled);
    expect(unhandled).not.toHaveBeenCalled();
    expect(screen.getByText("ABCD-1234")).toBeInTheDocument();
  });
});
