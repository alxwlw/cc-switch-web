import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import WorkspaceDirLink from "@/components/workspace/WorkspaceDirLink";
import { workspaceApi } from "@/lib/api/workspace";
import { setWeb } from "../helpers/web";

const copyTextMock = vi.hoisted(() => vi.fn());
const toastMock = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }));

vi.mock("@/lib/api/workspace", () => ({
  workspaceApi: { getPaths: vi.fn(), openDirectory: vi.fn() },
}));
vi.mock("@/lib/clipboard", () => ({ copyText: copyTextMock }));
vi.mock("@/lib/toast", () => ({ toast: toastMock }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

beforeEach(() => {
  vi.clearAllMocks();
});

describe("WorkspaceDirLink", () => {
  it("desktop: the path opens the directory, nothing is fetched", () => {
    setWeb(false);
    render(<WorkspaceDirLink subdir="memory" label="~/.openclaw/memory/" />);
    fireEvent.click(screen.getByText("~/.openclaw/memory/"));
    expect(workspaceApi.openDirectory).toHaveBeenCalledWith("memory");
    expect(workspaceApi.getPaths).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("button", { name: "workspace.copyPath" }),
    ).toBeNull();
  });

  it("web: shows the server path for its subdir and copies it", async () => {
    setWeb(true);
    vi.mocked(workspaceApi.getPaths).mockResolvedValue({
      workspace: "/srv/oc/workspace",
      memory: "/srv/oc/workspace/memory",
    });
    copyTextMock.mockResolvedValue(undefined);
    render(<WorkspaceDirLink subdir="memory" label="~/.openclaw/memory/" />);
    expect(
      await screen.findByText("/srv/oc/workspace/memory"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "workspace.copyPath" }));
    await waitFor(() =>
      expect(copyTextMock).toHaveBeenCalledWith("/srv/oc/workspace/memory"),
    );
    expect(toastMock.success).toHaveBeenCalledWith("workspace.pathCopied");
    expect(workspaceApi.openDirectory).not.toHaveBeenCalled();
  });

  it("web: a failed copy shows an error toast", async () => {
    setWeb(true);
    vi.mocked(workspaceApi.getPaths).mockResolvedValue({
      workspace: "/w",
      memory: "/m",
    });
    copyTextMock.mockRejectedValue(new Error("denied"));
    render(<WorkspaceDirLink subdir="workspace" label="~/w/" />);
    await screen.findByText("/w");
    fireEvent.click(screen.getByRole("button", { name: "workspace.copyPath" }));
    await waitFor(() =>
      expect(toastMock.error).toHaveBeenCalledWith("workspace.pathCopyFailed"),
    );
  });

  it("web: keeps the label when the paths request fails", async () => {
    setWeb(true);
    vi.mocked(workspaceApi.getPaths).mockRejectedValue(new Error("boom"));
    render(<WorkspaceDirLink subdir="workspace" label="~/w/" />);
    await waitFor(() => expect(workspaceApi.getPaths).toHaveBeenCalled());
    expect(screen.getByText("~/w/")).toBeInTheDocument();
  });
});
