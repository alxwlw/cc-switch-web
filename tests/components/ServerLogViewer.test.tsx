import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import "@testing-library/jest-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// In web builds `@tauri-apps/api/core` is aliased to the HTTP adapter; mirror that here.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

import { ServerLogViewer } from "@/components/settings/ServerLogViewer";

const ok = (data: unknown) => ({
  status: 200,
  json: async () => ({ ok: true, data }),
});

describe("ServerLogViewer (WEB-24)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => vi.unstubAllGlobals());

  it("shows the server path and the tail lines, with a download link", async () => {
    fetchSpy.mockResolvedValue(
      ok({
        path: "/srv/cc/logs/cc-switch-web.log",
        lines: ["line one", "line two"],
      }),
    );
    render(<ServerLogViewer open onClose={vi.fn()} />);
    const log = await screen.findByLabelText(
      "settings.advanced.logConfig.viewer.title",
      {
        selector: "pre",
      },
    );
    expect(log).toHaveTextContent("line one line two");
    expect(log.textContent).toBe("line one\nline two");
    expect(
      screen.getByText(/\/srv\/cc\/logs\/cc-switch-web\.log/),
    ).toBeInTheDocument();
    expect(fetchSpy.mock.calls[0][0]).toBe("/api/logs/tail?lines=500");
    const link = screen.getByRole("link", {
      name: /settings\.advanced\.logConfig\.viewer\.download/,
    });
    expect(link).toHaveAttribute("href", "/api/logs/download");
    expect(link).toHaveAttribute("download");
  });

  it("refresh fetches the tail again and shows the new lines", async () => {
    fetchSpy
      .mockResolvedValueOnce(ok({ path: "/p", lines: ["old"] }))
      .mockResolvedValueOnce(ok({ path: "/p", lines: ["old", "new"] }));
    render(<ServerLogViewer open onClose={vi.fn()} />);
    await screen.findByText("old");
    await userEvent.click(
      screen.getByRole("button", {
        name: /settings\.advanced\.logConfig\.viewer\.refresh/,
      }),
    );
    await waitFor(() =>
      expect(
        screen.getByLabelText("settings.advanced.logConfig.viewer.title", {
          selector: "pre",
        }).textContent,
      ).toBe("old\nnew"),
    );
    expect(fetchSpy).toHaveBeenCalledTimes(2);
  });

  it("a missing log file (404 + error with the path) is a normal empty state", async () => {
    fetchSpy.mockResolvedValue({
      status: 404,
      json: async () => ({
        ok: false,
        error: "log file not found: /srv/cc/logs/cc-switch-web.log",
        path: "/srv/cc/logs/cc-switch-web.log",
      }),
    });
    render(<ServerLogViewer open onClose={vi.fn()} />);
    expect(
      await screen.findByText("settings.advanced.logConfig.viewer.unavailable"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("log file not found: /srv/cc/logs/cc-switch-web.log"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("link", { name: /viewer\.download/ }),
    ).toBeInTheDocument();
  });

  it("refresh after a failure replaces the error with the lines, and the reverse", async () => {
    fetchSpy
      .mockResolvedValueOnce({
        status: 404,
        json: async () => ({ ok: false, error: "log file not found: /p" }),
      })
      .mockResolvedValueOnce(ok({ path: "/p", lines: ["back"] }))
      .mockResolvedValueOnce({
        status: 404,
        json: async () => ({ ok: false, error: "log file not found: /p" }),
      });
    render(<ServerLogViewer open onClose={vi.fn()} />);
    await screen.findByText("log file not found: /p");
    const refresh = screen.getByRole("button", { name: /viewer\.refresh/ });
    await userEvent.click(refresh);
    await screen.findByText("back");
    expect(screen.queryByText("log file not found: /p")).toBeNull();
    await userEvent.click(refresh);
    await screen.findByText("log file not found: /p");
    expect(screen.queryByText("back")).toBeNull();
    expect(screen.queryByText(/viewer\.path/)).toBeNull();
  });

  it("Escape closes the dialog through onClose", async () => {
    fetchSpy.mockResolvedValue(ok({ path: "/p", lines: [] }));
    const onClose = vi.fn();
    render(<ServerLogViewer open onClose={onClose} />);
    await screen.findByText("settings.advanced.logConfig.viewer.empty");
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does not fetch while closed", () => {
    render(<ServerLogViewer open={false} onClose={vi.fn()} />);
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});
