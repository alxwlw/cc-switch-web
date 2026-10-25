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

describe("usage configuration for pi/omp cards — web", () => {
  const renderCard = (appId: "pi" | "claude") =>
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
          onConfigureUsage={vi.fn()}
          onOpenWebsite={vi.fn()}
        />
      </QueryClientProvider>,
    );

  it.each([
    ["pi", true],
    ["claude", false],
  ] as const)(
    "configure-usage on a %s card is disabled=%s",
    async (appId, disabled) => {
      setWeb(true);
      const user = userEvent.setup();
      renderCard(appId);
      await user.click(
        screen.getByRole("button", { name: "providerCard.action.more" }),
      );
      const item = within(await screen.findByRole("menu")).getByRole(
        "menuitem",
        { name: /provider\.configureUsage/ },
      );
      if (disabled) {
        expect(item).toHaveAttribute("aria-disabled", "true");
        expect(item).toHaveTextContent("web.unavailable");
      } else {
        expect(item).not.toHaveAttribute("aria-disabled", "true");
      }
    },
  );
});
