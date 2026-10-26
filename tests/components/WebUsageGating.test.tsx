import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import "@testing-library/jest-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageSettingsSheet } from "@/components/usage/UsageSettingsSheet";
import { PricingConfigPanel } from "@/components/usage/PricingConfigPanel";
import { setWeb } from "../helpers/web";

const m = vi.hoisted(() => ({
  getPricingModelSource: vi.fn(),
  onRebuild: vi.fn(),
  onRouting: vi.fn(),
  pricing: [
    {
      modelId: "gpt-x",
      displayName: "GPT X",
      inputCostPerMillion: "1",
      outputCostPerMillion: "2",
      cacheReadCostPerMillion: "0.5",
      cacheCreationCostPerMillion: "0.25",
    },
  ],
}));

// t стабильна между рендерами: useBillingSources держит её в deps эффекта
const i18nStub = vi.hoisted(() => {
  const t = (key: string) => key;
  return { t, i18n: { resolvedLanguage: "en", language: "en" } };
});
vi.mock("react-i18next", () => ({ useTranslation: () => i18nStub }));
vi.mock("@/lib/query/proxy", () => ({
  useGlobalProxyConfig: () => ({
    data: { enableLogging: true },
    isLoading: false,
  }),
}));
vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    getPricingModelSource: m.getPricingModelSource,
    setPricingModelSource: vi.fn(),
  },
}));
vi.mock("@/lib/query/usage", () => ({
  useModelPricing: () => ({ data: m.pricing, isLoading: false, error: null }),
  useDeleteModelPricing: () => ({ mutate: vi.fn(), isPending: false }),
}));
vi.mock("@/components/usage/ModelsDevAutoSyncPanel", () => ({
  ModelsDevAutoSyncPanel: () => <div data-testid="models-dev-sync" />,
}));
vi.mock("@/components/usage/ModelsDevPickerDialog", () => ({
  ModelsDevPickerDialog: () => <div data-testid="models-dev-picker" />,
}));
vi.mock("@/components/usage/PricingEditModal", () => ({
  PricingEditModal: () => <div data-testid="pricing-edit" />,
}));

const renderSheet = () =>
  render(
    <UsageSettingsSheet
      open
      onOpenChange={vi.fn()}
      sessionAutoSyncEnabled
      syncing={false}
      onSyncNow={vi.fn()}
      onOpenRoutingSettings={m.onRouting}
      rebuildingCodex={false}
      onRebuildCodex={m.onRebuild}
      refreshIntervalMs={30000}
      refreshIntervalOptions={[30000]}
      onRefreshIntervalChange={vi.fn()}
      onOpenPricing={vi.fn()}
    />,
  );

const renderPricing = () =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <PricingConfigPanel />
    </QueryClientProvider>,
  );

beforeEach(() => {
  m.getPricingModelSource.mockReset().mockResolvedValue("response");
  m.onRebuild.mockReset();
  m.onRouting.mockReset();
});

describe.each([true, false])("UsageSettingsSheet (web=%s)", (web) => {
  beforeEach(() => setWeb(web));

  it("rebuild Codex usage works in web and on desktop", async () => {
    renderSheet();
    const button = screen.getByRole("button", {
      name: "usage.rebuildCodex.actionEllipsis",
    });
    expect(button).toBeEnabled();
    await userEvent.click(button);
    expect(m.onRebuild).toHaveBeenCalledTimes(1);
  });

  it("the logging status row and its link to routing settings exist only on desktop", () => {
    renderSheet();
    expect(screen.queryByText("routingSettings.logging") === null).toBe(web);
    expect(
      screen.queryByRole("button", { name: "usage.sources.editLogging" }) ===
        null,
    ).toBe(web);
  });

  it("session sync stays available", () => {
    renderSheet();
    expect(
      screen.getByRole("button", { name: "usage.sessionSync.syncNow" }),
    ).toBeEnabled();
  });
});

describe.each([true, false])("PricingConfigPanel (web=%s)", (web) => {
  beforeEach(() => setWeb(web));

  it("keeps reading the pricing table", () => {
    renderPricing();
    expect(screen.getByText("GPT X")).toBeInTheDocument();
  });

  it.each([
    ["usage.importFromModelsDev", "models-dev-picker"],
    ["usage.addPricing", "pricing-edit"],
    ["usage.pricing.editAria", "pricing-edit"],
    ["usage.pricing.deleteAria", null],
  ])("%s is enabled in web and on desktop", async (name, opens) => {
    renderPricing();
    const button = screen.getByRole("button", { name });
    expect(button).toBeEnabled();
    await userEvent.click(button);
    if (opens) {
      expect(screen.queryByTestId(opens)).not.toBeNull();
    }
  });

  it("models.dev sync panel and the pricing model source row render in web and on desktop", async () => {
    renderPricing();
    expect(screen.queryByTestId("models-dev-sync")).not.toBeNull();
    expect(
      screen.queryByText("settings.globalProxy.pricingDefaultsTitle"),
    ).not.toBeNull();
    await vi.waitFor(() => expect(m.getPricingModelSource).toHaveBeenCalled());
  });
});
