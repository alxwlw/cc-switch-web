import { render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import KimiAccountQuotaFooter from "@/components/KimiAccountQuotaFooter";
import { kimiOAuthApi } from "@/lib/api/kimiOAuth";
import { createTestQueryClient } from "../utils/testQueryClient";
import { setWeb } from "../helpers/web";

vi.mock("@/lib/api/kimiOAuth", () => ({
  kimiOAuthApi: { status: vi.fn(), quota: vi.fn() },
}));

const account = {
  id: "kimi-official-b",
  name: "Kimi Official · Nick",
  authenticated: true,
  region: "mainland-cn" as const,
  expiresAt: null,
  error: null,
  nickname: "Nick",
  avatarUrl: null,
  primary: false,
};

function renderFooter(
  name: string,
  inline = false,
  client = createTestQueryClient(),
) {
  const invalidate = vi.spyOn(client, "invalidateQueries");
  render(
    <QueryClientProvider client={client}>
      <KimiAccountQuotaFooter
        accountId={account.id}
        name={name}
        inline={inline}
      />
    </QueryClientProvider>,
  );
  return invalidate;
}

describe("KimiAccountQuotaFooter", () => {
  beforeEach(() => {
    vi.mocked(kimiOAuthApi.quota).mockResolvedValue({
      tool: "kimi",
      credentialStatus: "not_found",
      credentialMessage: null,
      success: false,
      tiers: [],
      extraUsage: null,
      error: null,
      queriedAt: null,
    });
    vi.mocked(kimiOAuthApi.status).mockResolvedValue({
      accounts: [account],
      currentAccountId: null,
      primaryHidden: false,
    });
  });

  it("reloads the card list once the account's nickname is known", async () => {
    const invalidate = renderFooter("Kimi Official (kimi-off)");
    await waitFor(() =>
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: ["providers", "kimi"],
      }),
    );
  });

  it("web: queries status and quota like on desktop (WEB-02 stage 2)", async () => {
    setWeb(true);
    vi.mocked(kimiOAuthApi.quota).mockResolvedValue({
      tool: "kimi_oauth",
      credentialStatus: "valid",
      credentialMessage: null,
      success: true,
      tiers: [
        {
          name: "five_hour",
          utilization: 37,
          resetsAt: null,
        },
      ],
      extraUsage: null,
      error: null,
      queriedAt: 0,
    });
    const invalidate = renderFooter("Kimi Official (kimi-off)", true);
    await waitFor(() => expect(kimiOAuthApi.status).toHaveBeenCalled());
    await waitFor(() =>
      expect(kimiOAuthApi.quota).toHaveBeenCalledWith(account.id),
    );
    await waitFor(() =>
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: ["providers", "kimi"],
      }),
    );
    // the test i18n returns keys: a rendered tier line is "quota.tierLeft"
    expect(await screen.findByText("quota.tierLeft")).toBeInTheDocument();
  });

  it("web: shows the server's error text when the quota request fails", async () => {
    setWeb(true);
    vi.mocked(kimiOAuthApi.quota).mockRejectedValue(
      new Error("kimi upstream said 503"),
    );
    // useKimiAccountQuota keeps retry: 1; retryDelay 0 on this test's client
    // skips the ~1 s backoff before the query counts as failed
    renderFooter(
      account.name,
      true,
      new QueryClient({
        defaultOptions: { queries: { retry: false, retryDelay: 0 } },
      }),
    );
    expect(
      await screen.findByText(/kimi upstream said 503/),
    ).toBeInTheDocument();
  });

  it("leaves the list alone when the card already shows the nickname", async () => {
    const invalidate = renderFooter(account.name);
    await waitFor(() => expect(kimiOAuthApi.status).toHaveBeenCalled());
    expect(invalidate).not.toHaveBeenCalledWith({
      queryKey: ["providers", "kimi"],
    });
  });

  it("says the account has no subscription when the quota comes back empty", async () => {
    vi.mocked(kimiOAuthApi.quota).mockResolvedValue({
      tool: "kimi_oauth",
      credentialStatus: "valid",
      credentialMessage: null,
      success: true,
      tiers: [],
      extraUsage: null,
      error: null,
      queriedAt: 0,
    });
    renderFooter(account.name, true);
    expect(
      await screen.findByText("kimi.oauth.noSubscription"),
    ).toBeInTheDocument();
  });

  it("stays silent when the account has no login on this device", async () => {
    renderFooter(account.name, true);
    await waitFor(() => expect(kimiOAuthApi.quota).toHaveBeenCalled());
    expect(
      screen.queryByText("kimi.oauth.noSubscription"),
    ).not.toBeInTheDocument();
  });
});
