import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import "@testing-library/jest-dom";
import i18n from "i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import EndpointSpeedTest from "@/components/providers/forms/EndpointSpeedTest";
import { createTestQueryClient } from "../utils/testQueryClient";
import { setWeb } from "../helpers/web";

const api = vi.hoisted(() => ({
  getCustomEndpoints: vi.fn(),
  addCustomEndpoint: vi.fn(),
  removeCustomEndpoint: vi.fn(),
  testApiEndpoints: vi.fn(),
  updateEndpointLastUsed: vi.fn(),
}));
vi.mock("@/lib/api/vscode", () => ({ vscodeApi: api }));

// Панель тянет шапку приложения; тут важны только children и footer.
vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({ children, footer }: any) => (
    <div>
      {children}
      {footer}
    </div>
  ),
}));

const SAVED = [{ url: "https://old.example", addedAt: 1 }];

async function renderPanel(client = createTestQueryClient()) {
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={client}>
      <EndpointSpeedTest
        appId="claude"
        providerId="p1"
        value="https://old.example"
        onChange={vi.fn()}
        initialEndpoints={[]}
        autoSelect={false}
        onAutoSelectChange={vi.fn()}
        onClose={onClose}
      />
    </QueryClientProvider>,
  );
  await screen.findAllByText("https://old.example");
  return { client, onClose };
}

async function addUrl(url: string) {
  const input = screen.getByPlaceholderText(
    "endpointTest.addEndpointPlaceholder",
  );
  await userEvent.type(input, `${url}{Enter}`);
}

describe("EndpointSpeedTest (WEB-21)", () => {
  beforeEach(() => {
    api.getCustomEndpoints.mockResolvedValue(SAVED);
    api.addCustomEndpoint.mockResolvedValue(undefined);
    api.removeCustomEndpoint.mockResolvedValue(undefined);
    i18n.addResourceBundle(
      "zh",
      "translation",
      {
        endpointTest: {
          savePartial: "partial {{saved}} / {{error}}",
          serverSideNote: "server-side-note",
        },
      },
      true,
      true,
    );
  });

  it("показывает подпись про замер с сервера в web, но не на desktop", async () => {
    setWeb(true);
    await renderPanel();
    expect(screen.getByTestId("endpoint-server-side-note")).toHaveTextContent(
      "server-side-note",
    );
  });

  it("на desktop подписи нет", async () => {
    setWeb(false);
    await renderPanel();
    expect(
      screen.queryByTestId("endpoint-server-side-note"),
    ).not.toBeInTheDocument();
  });

  it("ошибка на втором add: providers-query инвалидируется, показаны сохранённые URL", async () => {
    const { client, onClose } = await renderPanel();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    api.addCustomEndpoint
      .mockResolvedValueOnce(undefined)
      .mockRejectedValueOnce(new Error("boom"));

    await addUrl("https://a.example");
    await addUrl("https://b.example");
    await userEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() =>
      expect(screen.getByText(/partial .*a\.example.* \/ boom/)).toBeVisible(),
    );
    expect(screen.queryByText(/b\.example.* \/ boom/)).not.toBeInTheDocument();
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["providers", "claude"],
    });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("успешное сохранение тоже инвалидирует providers и закрывает панель", async () => {
    const { client, onClose } = await renderPanel();
    const invalidate = vi.spyOn(client, "invalidateQueries");

    await addUrl("https://a.example");
    await userEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(api.addCustomEndpoint).toHaveBeenCalledWith(
      "claude",
      "p1",
      "https://a.example",
    );
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["providers", "claude"],
    });
  });
});
