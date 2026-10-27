import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import "@testing-library/jest-dom";
import { describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";
import { setWeb } from "../helpers/web";

const provider = (id: string, extra: Partial<Provider> = {}): Provider => ({
  id,
  name: id,
  settingsConfig: {},
  ...extra,
});
const baseCard = { buttons: [], chips: [] };

describe("usage configuration for pi/omp cards — web (WEB-17)", () => {
  const renderCard = (
    appId: "pi" | "omp" | "claude",
    onConfigureUsage: () => void,
  ) =>
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ProviderCard
          provider={provider("p", {
            settingsConfig: { env: { ANTHROPIC_BASE_URL: "https://x" } },
          })}
          appId={appId}
          isCurrent={false}
          presentation={baseCard}
          onEdit={vi.fn()}
          onDelete={vi.fn()}
          onConfigureUsage={onConfigureUsage}
          onOpenWebsite={vi.fn()}
        />
      </QueryClientProvider>,
    );

  it.each(["pi", "omp", "claude"] as const)(
    "configure-usage on a %s card is enabled and fires",
    async (appId) => {
      setWeb(true);
      const onConfigureUsage = vi.fn();
      const user = userEvent.setup();
      renderCard(appId, onConfigureUsage);
      await user.click(
        screen.getByRole("button", { name: "providerCard.action.more" }),
      );
      const item = within(await screen.findByRole("menu")).getByRole(
        "menuitem",
        { name: /provider\.configureUsage/ },
      );
      expect(item).not.toHaveAttribute("aria-disabled", "true");
      expect(item).not.toHaveTextContent("web.unavailable");
      await user.click(item);
      expect(onConfigureUsage).toHaveBeenCalledTimes(1);
    },
  );
});
