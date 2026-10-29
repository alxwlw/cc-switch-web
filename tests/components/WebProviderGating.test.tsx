import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import { authApi } from "@/lib/api";
import userEvent from "@testing-library/user-event";
import { FormProvider, useForm } from "react-hook-form";
import "@testing-library/jest-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";
import { ProviderCardActions } from "@/components/providers/ProviderCardActions";
import { ProviderEmptyState } from "@/components/providers/ProviderEmptyState";
import { SwitchModePanel } from "@/components/providers/mode/SwitchModePanel";
import { ModelInputWithFetch } from "@/components/providers/forms/shared/ModelInputWithFetch";
import { EndpointField } from "@/components/providers/forms/shared/EndpointField";
import { FetchedModelPicker } from "@/components/providers/forms/FetchedModelPicker";
import { CopilotAuthSection } from "@/components/providers/forms/CopilotAuthSection";
import { XaiOAuthSection } from "@/components/providers/forms/XaiOAuthSection";
import { CodexOAuthSection } from "@/components/providers/forms/CodexOAuthSection";
import {
  buildAdditiveSections,
  buildLaunchSections,
  type AdditiveInput,
  type CardPresentation,
} from "@/components/providers/presentation";
import { isViewAvailable, readStoredView } from "@/lib/navigation";
import { createTestQueryClient } from "../utils/testQueryClient";
import { setWeb } from "../helpers/web";

// Карточка рисуется кнопками по presentation: проверяем, что SwitchModePanel отдаёт в web
vi.mock("@/components/providers/ProviderCard", () => ({
  ProviderCard: ({ provider, presentation }: any) => (
    <div data-testid={`card-${provider.id}`}>
      {presentation.status && (
        <span data-testid={`status-${provider.id}`}>
          {presentation.status.label}
        </span>
      )}
      {presentation.buttons.map((button: any) => (
        <button
          key={button.key}
          data-testid={`${button.key}-${provider.id}`}
          data-reason={button.disabledReason ?? ""}
          disabled={Boolean(button.disabledReason)}
          onClick={button.onClick}
        >
          {button.label}
        </button>
      ))}
    </div>
  ),
}));
vi.mock("@/hooks/useStreamCheck", () => ({
  useStreamCheck: () => ({ checkProvider: vi.fn(), isChecking: () => false }),
}));

const provider = (id: string, extra: Partial<Provider> = {}): Provider => ({
  id,
  name: id,
  settingsConfig: {},
  ...extra,
});

function WithForm({ children }: { children: React.ReactNode }) {
  const form = useForm();
  return <FormProvider {...form}>{children}</FormProvider>;
}

const baseCard: CardPresentation = { buttons: [], chips: [] };

async function openMenu(props: {
  onTest?: () => void;
  onOpenTerminal?: () => void;
}) {
  const user = userEvent.setup();
  render(
    <ProviderCardActions
      providerName="P"
      presentation={baseCard}
      onEdit={vi.fn()}
      onDelete={vi.fn()}
      {...props}
    />,
  );
  await user.click(
    screen.getByRole("button", { name: "providerCard.action.more" }),
  );
  return { user, menu: await screen.findByRole("menu") };
}

describe("ProviderCardActions — web", () => {
  it("disables connectivity check and open terminal with the web reason", async () => {
    setWeb(true);
    const onTest = vi.fn();
    const onOpenTerminal = vi.fn();
    const { user, menu } = await openMenu({ onTest, onOpenTerminal });

    const test = within(menu).getByRole("menuitem", {
      name: /provider\.connectivityCheck/,
    });
    const terminal = within(menu).getByRole("menuitem", {
      name: /provider\.openTerminal/,
    });
    expect(test).toHaveAttribute("aria-disabled", "true");
    expect(test).toHaveTextContent("web.unavailable");
    expect(terminal).toHaveAttribute("aria-disabled", "true");
    expect(terminal).toHaveTextContent("web.unavailable");
    await user.click(test);
    await user.click(terminal);
    expect(onTest).not.toHaveBeenCalled();
    expect(onOpenTerminal).not.toHaveBeenCalled();
  });

  it("keeps both enabled on desktop", async () => {
    const onTest = vi.fn();
    const { user, menu } = await openMenu({
      onTest,
      onOpenTerminal: vi.fn(),
    });

    const test = within(menu).getByRole("menuitem", {
      name: "provider.connectivityCheck",
    });
    expect(test).not.toHaveAttribute("aria-disabled", "true");
    expect(
      within(menu).getByRole("menuitem", { name: "provider.openTerminal" }),
    ).not.toHaveAttribute("aria-disabled", "true");
    await user.click(test);
    expect(onTest).toHaveBeenCalledTimes(1);
  });
});

describe("ProviderEmptyState — import from live config", () => {
  it("disables the import button when the import reads the live config", () => {
    setWeb(true);
    const onImport = vi.fn();
    render(
      <ProviderEmptyState
        appId="opencode"
        onImport={onImport}
        importUnavailable
      />,
    );
    expect(
      screen.getByRole("button", { name: "provider.importCurrent" }),
    ).toBeDisabled();
  });

  it("leaves import enabled when it is available", () => {
    render(<ProviderEmptyState appId="claude" onImport={vi.fn()} />);
    expect(
      screen.getByRole("button", { name: "provider.importCurrent" }),
    ).toBeEnabled();
  });
});

describe("buildAdditiveSections — live membership unknown (web)", () => {
  const t = ((key: string) => key) as AdditiveInput["t"];
  const input = (over: Partial<AdditiveInput> = {}): AdditiveInput => ({
    app: "opencode",
    t,
    providers: [provider("a"), provider("b")],
    isInConfig: () => false,
    actions: {
      add: vi.fn(),
      remove: vi.fn(),
      disableOmo: vi.fn(),
      setDefault: vi.fn(),
    },
    ...over,
  });

  it("puts every card in one neutral section with both add and remove", () => {
    const add = vi.fn();
    const remove = vi.fn();
    const sections = buildAdditiveSections(
      input({
        membershipUnknown: true,
        actions: { add, remove, disableOmo: vi.fn(), setDefault: vi.fn() },
      }),
    );

    expect(sections.map((s) => s.key)).toEqual(["unknownMembership"]);
    expect(sections[0].title).toBe("web.liveUnknownTitle");
    const [first] = sections[0].items;
    expect(first.presentation.buttons.map((b) => b.key)).toEqual([
      "add",
      "remove",
    ]);
    first.presentation.buttons[0].onClick();
    first.presentation.buttons[1].onClick();
    expect(add).toHaveBeenCalledWith(first.provider);
    expect(remove).toHaveBeenCalledWith(first.provider);
  });

  it.each([
    ["openclaw", undefined],
    ["hermes", undefined],
    ["opencode", undefined],
  ] as const)(
    "%s: Remove in the unknown-membership section has reason %s",
    (app, reason) => {
      const sections = buildAdditiveSections(
        input({ app, membershipUnknown: true }),
      );
      const remove = sections[0].items[0].presentation.buttons.find(
        (b) => b.key === "remove",
      );
      expect(remove?.disabledReason).toBe(reason);
    },
  );

  it("keeps the added/available split when membership is known", () => {
    const sections = buildAdditiveSections(input());
    expect(sections.map((s) => s.key)).toEqual(["added", "available"]);
    expect(sections[1].items).toHaveLength(2);
  });
});

describe("buildLaunchSections — web", () => {
  const t = ((key: string) => key) as AdditiveInput["t"];
  const build = (terminalUnavailableReason?: string) =>
    buildLaunchSections({
      app: "agy",
      t,
      providers: [provider("a")],
      openTerminal: vi.fn(),
      terminalUnavailableReason,
    })[0].items[0].presentation.buttons[0];

  it("disables the open-terminal button with the reason", () => {
    expect(build("web.unavailable").disabledReason).toBe("web.unavailable");
  });

  it("leaves it enabled without a reason", () => {
    expect(build().disabledReason).toBeUndefined();
  });
});

describe("shared form controls — web", () => {
  it("disables the fetch-models button", () => {
    setWeb(true);
    render(
      <ModelInputWithFetch
        id="m"
        value=""
        onChange={vi.fn()}
        fetchedModels={[]}
        isLoading={false}
        onFetch={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    ).toBeDisabled();
  });

  it("keeps the fetch-models button enabled on desktop", () => {
    render(
      <ModelInputWithFetch
        id="m"
        value=""
        onChange={vi.fn()}
        fetchedModels={[]}
        isLoading={false}
        onFetch={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    ).toBeEnabled();
  });

  it("shows the endpoint speed-test entry in web and desktop (WEB-21)", () => {
    const props = {
      id: "u",
      label: "url",
      value: "",
      onChange: vi.fn(),
      placeholder: "p",
      onManageClick: vi.fn(),
      manageButtonLabel: "manage-endpoints",
    };
    for (const web of [true, false]) {
      setWeb(web);
      const { unmount } = render(
        <WithForm>
          <EndpointField {...props} />
        </WithForm>,
      );
      expect(
        screen.getByRole("button", { name: "manage-endpoints" }),
      ).toBeInTheDocument();
      unmount();
    }
  });

  it("Codex select mode loads accounts in web instead of showing a notice (WEB-02)", async () => {
    setWeb(true);
    const getStatus = vi
      .spyOn(authApi, "authGetStatus")
      .mockReturnValue(new Promise(() => {}));
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <CodexOAuthSection mode="select" onAccountSelect={vi.fn()} />
      </QueryClientProvider>,
    );
    expect(screen.queryByText("web.accountLogin")).toBeNull();
    await waitFor(() => expect(getStatus).toHaveBeenCalledWith("codex_oauth"));
    getStatus.mockRestore();
  });
});

describe("FetchedModelPicker — web", () => {
  const render_ = (models: { id: string; ownedBy: string | null }[]) =>
    render(
      <FetchedModelPicker
        models={models}
        configuredModelIds={[]}
        onApply={vi.fn()}
        onFetch={vi.fn()}
        isFetching={false}
      />,
    );

  it("disables fetching while there is no list", () => {
    setWeb(true);
    render_([]);
    expect(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    ).toBeDisabled();
  });

  it("keeps an already loaded list openable", () => {
    setWeb(true);
    render_([{ id: "m", ownedBy: null }]);
    expect(
      screen.getByRole("button", { name: "providerForm.modelPicker.title" }),
    ).toBeEnabled();
  });

  it("allows fetching on desktop", () => {
    render_([]);
    expect(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    ).toBeEnabled();
  });
});

describe("account sections in provider forms — web", () => {
  it.each([
    ["Copilot", <CopilotAuthSection mode="select" key="c" />],
    [
      "xAI",
      <XaiOAuthSection mode="select" onAccountSelect={vi.fn()} key="x" />,
    ],
  ])("%s select mode has no web notice (WEB-02)", (_label, element) => {
    setWeb(true);
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        {element}
      </QueryClientProvider>,
    );
    expect(screen.queryByText("web.accountLogin")).toBeNull();
  });
});

describe("app views — web", () => {
  beforeEach(() => localStorage.clear());

  it("serves the openclaw sub-views (WEB-15), hermesMemory (WEB-16) and auth (WEB-02)", () => {
    setWeb(true);
    for (const view of [
      "workspace",
      "openclawConfig",
      "hermesMemory",
      "providers",
      "auth",
    ] as const) {
      expect(isViewAvailable(view)).toBe(true);
    }
    localStorage.setItem("cc-switch-last-view", "openclawConfig");
    expect(readStoredView()).toBe("openclawConfig");
    localStorage.setItem("cc-switch-last-view", "auth");
    expect(readStoredView()).toBe("auth");
  });

  it("keeps them on desktop", () => {
    localStorage.setItem("cc-switch-last-view", "hermesMemory");
    expect(isViewAvailable("workspace")).toBe(true);
    expect(readStoredView()).toBe("hermesMemory");
  });
});

describe("SwitchModePanel — web", () => {
  beforeEach(() => {
    Element.prototype.scrollIntoView = vi.fn();
  });

  function renderPanel(providers: Record<string, Provider>, current: string) {
    const onSwitch = vi.fn();
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <SwitchModePanel
          app="claude"
          providers={providers}
          currentProviderId={current}
          isLoading={false}
          onSwitch={onSwitch}
          onOpenRoutingSettings={vi.fn()}
          onEdit={vi.fn()}
          onDelete={vi.fn()}
          onDuplicate={vi.fn()}
          onOpenWebsite={vi.fn()}
        />
      </QueryClientProvider>,
    );
    return onSwitch;
  }

  it("renders no mode tabs and marks the current provider as in use", () => {
    setWeb(true);
    const onSwitch = renderPanel({ a: provider("a"), b: provider("b") }, "b");

    expect(
      screen.queryByRole("group", { name: "mode.tabsLabel" }),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("status-b")).toHaveTextContent(
      "providerCard.status.inUse",
    );
    expect(screen.queryByTestId("switch-b")).not.toBeInTheDocument();
    screen.getByTestId("switch-a").click();
    expect(onSwitch).toHaveBeenCalledTimes(1);
    expect(onSwitch.mock.calls[0][0].id).toBe("a");
  });

  it("blocks a provider that needs routing with the web reason", () => {
    setWeb(true);
    renderPanel(
      {
        a: provider("a"),
        b: provider("b"),
        needsRoute: provider("needsRoute", {
          meta: { apiFormat: "openai_chat" },
        }),
      },
      "a",
    );

    const button = screen.getByTestId("switch-needsRoute");
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("data-reason", "web.needsRouting");
    expect(screen.getByTestId("switch-b")).toBeEnabled();
  });

  it("still shows the mode tabs on desktop", async () => {
    renderPanel({ a: provider("a") }, "a");
    expect(
      await screen.findByRole("group", { name: "mode.tabsLabel" }),
    ).toBeInTheDocument();
  });
});
