import { render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AuthCenterPanel } from "@/components/settings/AuthCenterPanel";
import { createTestQueryClient } from "../utils/testQueryClient";
import { setWeb } from "../helpers/web";

vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => <div>copilot-section</div>,
}));
vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: () => <div>codex-section</div>,
}));
vi.mock("@/components/providers/forms/XaiOAuthSection", () => ({
  XaiOAuthSection: () => <div>xai-section</div>,
}));
vi.mock("@/components/providers/KimiAccountLogin", () => ({
  KimiAccountLogin: () => <div>kimi-login</div>,
}));
// Real AgyOAuthSection and ManagedAccountsGroup; only data hooks are stubbed.
vi.mock("@/components/providers/forms/hooks/useManagedAuth", () => ({
  useManagedAuth: () => ({
    accounts: [],
    defaultAccountId: null,
    isStatusSuccess: true,
    isStatusError: false,
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
    refetchStatus: vi.fn(),
  }),
}));
vi.mock("@/components/providers/forms/hooks/useManagedAccountUsers", () => ({
  useManagedAccountUsers: () => () => [],
}));

function stubCapabilities(platform: string) {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        mode: "web",
        version: "1.0.0",
        runtime_boundaries: "",
        platform,
      }),
    }),
  );
}

const renderPanel = () =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <AuthCenterPanel />
    </QueryClientProvider>,
  );

describe("AuthCenterPanel in web (WEB-02)", () => {
  beforeEach(() => setWeb(true));
  afterEach(() => vi.unstubAllGlobals());

  it("linux server: no Antigravity card, Kimi login rendered", async () => {
    stubCapabilities("linux");
    renderPanel();
    await waitFor(() =>
      expect(fetch).toHaveBeenCalledWith("/api/capabilities"),
    );
    expect(screen.getByText("copilot-section")).toBeInTheDocument();
    expect(screen.getByText("codex-section")).toBeInTheDocument();
    expect(screen.getByText("xai-section")).toBeInTheDocument();
    expect(screen.getByText("kimi-login")).toBeInTheDocument();
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByRole("region", { name: "Antigravity" })).toBeNull();
  });

  it("macos server: Antigravity card with the loopback line and no sign-in button", async () => {
    stubCapabilities("macos");
    renderPanel();
    const card = await screen.findByRole("region", { name: "Antigravity" });
    expect(card).toHaveTextContent(
      "Antigravity 登录通过服务器上的本地回环回调完成",
    );
    expect(card.querySelector("#auth-login-agy")).toBeNull();
    expect(card.querySelector("#auth-add-agy")).toBeNull();
    expect(screen.getByText("kimi-login")).toBeInTheDocument();
  });

  it("desktop: Kimi login is rendered, no capabilities request", () => {
    setWeb(false);
    stubCapabilities("linux");
    renderPanel();
    expect(screen.getByText("kimi-login")).toBeInTheDocument();
    expect(fetch).not.toHaveBeenCalled();
  });
});
