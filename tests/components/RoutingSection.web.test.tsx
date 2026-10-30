import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RoutingSection } from "@/components/settings/sections/RoutingSection";
import { createTestQueryClient } from "../utils/testQueryClient";
import { setWeb } from "../helpers/web";

const config = vi.hoisted(() => ({
  value: {
    listenAddress: "127.0.0.1",
    listenPort: 15721,
    enableLogging: false,
  },
}));

vi.mock("@/lib/query/proxy", () => ({
  useGlobalProxyConfig: () => ({ data: config.value }),
  useProxyClientHost: () => ({ data: "" }),
  useProxyStatusQuery: () => ({ data: { running: false } }),
  useSetProxyClientHost: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useUpdateGlobalProxyConfig: () => ({
    mutateAsync: vi.fn(),
    isPending: false,
  }),
}));
vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({
    startProxyServer: vi.fn(),
    stopProxyServer: vi.fn(),
    stopWithRestore: vi.fn(),
    isPending: false,
  }),
}));
vi.mock("@/lib/query", () => ({
  useProvidersQuery: () => ({ data: { providers: {} } }),
}));
vi.mock("@/lib/api/proxy", () => ({
  proxyApi: { getAppMode: vi.fn().mockResolvedValue(null) },
}));
vi.mock("@/components/proxy/AutoFailoverConfigPanel", () => ({
  AutoFailoverConfigPanel: () => <div>failover-panel</div>,
}));
vi.mock("@/components/settings/RectifierConfigPanel", () => ({
  RectifierConfigPanel: () => <div>rectifier-panel</div>,
}));

function renderSection() {
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <RoutingSection onOpenApp={vi.fn()} />
    </QueryClientProvider>,
  );
}

const addressInput = () => screen.getAllByRole("textbox")[0];

describe("RoutingSection — web (WEB-01)", () => {
  beforeEach(() => {
    config.value = {
      listenAddress: "127.0.0.1",
      listenPort: 15721,
      enableLogging: false,
    };
  });

  it("shows the server caption, rectifier, and the per-app config panel", () => {
    setWeb(true);
    renderSection();
    expect(screen.getByTestId("web-proxy-caption")).toHaveTextContent(
      "web.proxyCaption",
    );
    expect(screen.getByText("rectifier-panel")).toBeInTheDocument();
    expect(screen.getByText("failover-panel")).toBeInTheDocument();
  });

  it("no listen warning on loopback", () => {
    setWeb(true);
    renderSection();
    expect(screen.queryByTestId("web-listen-warning")).toBeNull();
  });

  it.each(["0.0.0.0", "::"])(
    "warns when the saved listen address is %s",
    (address) => {
      config.value = { ...config.value, listenAddress: address };
      setWeb(true);
      renderSection();
      expect(screen.getByTestId("web-listen-warning")).toHaveTextContent(
        `web.proxyListenWarning`,
      );
      expect(screen.getByRole("alert")).toBeInTheDocument();
    },
  );

  it("warns as soon as the user types a wildcard address, and clears it after", () => {
    setWeb(true);
    renderSection();
    fireEvent.change(addressInput(), { target: { value: "0.0.0.0" } });
    expect(screen.getByTestId("web-listen-warning")).toBeInTheDocument();
    fireEvent.change(addressInput(), { target: { value: "127.0.0.1" } });
    expect(screen.queryByTestId("web-listen-warning")).toBeNull();
  });
});

describe("RoutingSection — desktop", () => {
  it("has no web caption, no listen warning, and keeps the per-app config panel", () => {
    config.value = { ...config.value, listenAddress: "0.0.0.0" };
    renderSection();
    expect(screen.queryByTestId("web-proxy-caption")).toBeNull();
    expect(screen.queryByTestId("web-listen-warning")).toBeNull();
    expect(screen.getByText("failover-panel")).toBeInTheDocument();
  });
});
