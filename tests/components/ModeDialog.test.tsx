import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ComponentProps } from "react";
import type { Provider } from "@/types";
import { ModeDialog } from "@/components/providers/mode/ModeDialog";
import { setWeb } from "../helpers/web";

vi.mock("@/lib/frontendLogger", () => ({ logFrontendInfo: vi.fn() }));

const provider = (id: string, name: string) =>
  ({ id, name, settingsConfig: {} }) as Provider;

const renderExit = (
  overrides: Partial<ComponentProps<typeof ModeDialog>> = {},
) => {
  const props: ComponentProps<typeof ModeDialog> = {
    app: "claude",
    state: { kind: "exit" },
    active: "route",
    providers: [provider("kimi", "Kimi"), provider("deepseek", "DeepSeek")],
    directId: "kimi",
    eligibleIds: ["kimi", "deepseek"],
    defaultPick: "deepseek",
    stackMembers: [],
    onClose: vi.fn(),
    onEnter: vi.fn(),
    onExit: vi.fn().mockResolvedValue(undefined),
    onSwitchDirect: vi.fn(),
    ...overrides,
  };
  render(<ModeDialog {...props} />);
  return props;
};

describe("ModeDialog exit", () => {
  it("asks before going back to direct and closes after confirming", async () => {
    const props = renderExit();

    expect(screen.getByText("mode.dialog.exitTitle")).toBeInTheDocument();
    expect(props.onExit).not.toHaveBeenCalled();

    fireEvent.click(
      screen.getByRole("button", { name: "mode.dialog.confirmDirect" }),
    );

    await waitFor(() => expect(props.onClose).toHaveBeenCalled());
    expect(props.onExit).toHaveBeenCalledWith(undefined);
  });

  it("passes the provider to use after going back to direct", async () => {
    const props = renderExit({
      state: { kind: "exit", useProviderId: "deepseek" },
    });

    fireEvent.click(
      screen.getByRole("button", { name: "mode.dialog.confirmDirect" }),
    );

    await waitFor(() => expect(props.onExit).toHaveBeenCalledWith("deepseek"));
  });

  it("keeps the dialog open and shows the error when leaving fails", async () => {
    const props = renderExit({
      onExit: vi.fn().mockRejectedValue(new Error("write failed")),
    });

    fireEvent.click(
      screen.getByRole("button", { name: "mode.dialog.confirmDirect" }),
    );

    expect(await screen.findByRole("alert")).toHaveTextContent("write failed");
    expect(props.onClose).not.toHaveBeenCalled();
  });

  it("gives Claude Desktop its own notes: restart, no file, no failover", () => {
    renderExit({ app: "claude-desktop" });

    expect(
      screen.getByText("mode.dialog.desktopRouteNoteRestart"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("mode.dialog.desktopExitNoteRouteKept"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("mode.dialog.exitNoteFile"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("mode.dialog.exitNoteRouteKept"),
    ).not.toBeInTheDocument();
  });

  it("tells Claude Desktop where the aggregated models go when leaving it", () => {
    renderExit({ app: "claude-desktop", active: "stack" });

    expect(
      screen.getByText("mode.dialog.desktopLeaveStackNote"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("mode.dialog.leaveStackNote"),
    ).not.toBeInTheDocument();
  });

  it("does nothing on cancel", () => {
    const props = renderExit();

    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));

    expect(props.onClose).toHaveBeenCalled();
    expect(props.onExit).not.toHaveBeenCalled();
  });
});

describe("ModeDialog enter — Claude Desktop reaches the server's proxy (WEB-19)", () => {
  const renderEnter = (
    app: ComponentProps<typeof ModeDialog>["app"],
    target: "route" | "stack" = "route",
  ) =>
    render(
      <ModeDialog
        app={app}
        state={{ kind: "enter", target }}
        active="direct"
        providers={[provider("kimi", "Kimi")]}
        directId="kimi"
        eligibleIds={["kimi"]}
        defaultPick="kimi"
        stackMembers={[]}
        onClose={vi.fn()}
        onEnter={vi.fn()}
        onExit={vi.fn()}
        onSwitchDirect={vi.fn()}
      />,
    );

  it.each(["route", "stack"] as const)(
    "web: %s mode for Claude Desktop says the client host is set in Settings → Local routing",
    (target) => {
      setWeb(true);
      renderEnter("claude-desktop", target);
      expect(screen.getByText("web.desktopRouteHost")).toBeInTheDocument();
    },
  );

  it("desktop: no server-host note", () => {
    renderEnter("claude-desktop");
    expect(screen.queryByText("web.desktopRouteHost")).toBeNull();
  });

  it("web: other apps do not get the Claude Desktop note", () => {
    setWeb(true);
    renderEnter("claude");
    expect(screen.queryByText("web.desktopRouteHost")).toBeNull();
  });
});
