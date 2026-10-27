import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { BackupStorageSection } from "@/components/settings/BackupStorageSection";
import { setWeb } from "../helpers/web";

const mocks = vi.hoisted(() => ({
  copyText: vi.fn(),
  reveal: vi.fn(),
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("@/lib/clipboard", () => ({ copyText: mocks.copyText }));
vi.mock("@/lib/toast", () => ({
  toast: { success: mocks.toastSuccess, error: mocks.toastError },
}));
vi.mock("@/lib/api/settings", () => ({
  backupsApi: { revealBackupLocation: mocks.reveal },
}));
vi.mock("@/hooks/useBackupManager", () => ({
  useBackupLocations: () => ({
    locations: [
      {
        id: "database",
        path: "/srv/cc-switch/backups",
        sizeBytes: 10,
        itemCount: 1,
        lastModified: null,
        deletable: false,
      },
      {
        id: "skills",
        path: "/srv/cc-switch/skill-backups",
        sizeBytes: 5,
        itemCount: 2,
        lastModified: null,
        deletable: true,
      },
    ],
    isLoading: false,
    isError: false,
    remove: vi.fn(),
    isDeleting: false,
  }),
}));

async function renderOpen() {
  render(<BackupStorageSection />);
  await userEvent.click(screen.getByText("settings.backupStorage.title"));
}

describe("BackupStorageSection", () => {
  it("web: shows the server path with a copy button per location and no reveal", async () => {
    setWeb(true);
    await renderOpen();
    expect(screen.getByText("/srv/cc-switch/backups")).toBeInTheDocument();
    expect(
      screen.getByText("/srv/cc-switch/skill-backups"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "settings.backupStorage.reveal" }),
    ).toBeNull();
    const copyButtons = screen.getAllByRole("button", {
      name: "workspace.copyPath",
    });
    expect(copyButtons).toHaveLength(2);

    await userEvent.click(copyButtons[1]);
    expect(mocks.copyText).toHaveBeenCalledWith("/srv/cc-switch/skill-backups");
    expect(mocks.toastSuccess).toHaveBeenCalledWith("workspace.pathCopied");
  });

  it("web: a failed copy shows an error toast", async () => {
    setWeb(true);
    mocks.copyText.mockRejectedValueOnce(new Error("denied"));
    await renderOpen();
    await userEvent.click(
      screen.getAllByRole("button", { name: "workspace.copyPath" })[0],
    );
    expect(mocks.toastError).toHaveBeenCalledWith("workspace.pathCopyFailed");
    expect(mocks.toastSuccess).not.toHaveBeenCalled();
  });

  it("desktop: keeps the reveal button and has no copy button", async () => {
    await renderOpen();
    expect(
      screen.queryByRole("button", { name: "workspace.copyPath" }),
    ).toBeNull();
    const reveal = screen.getAllByRole("button", {
      name: "settings.backupStorage.reveal",
    });
    expect(reveal).toHaveLength(2);
    await userEvent.click(reveal[1]);
    expect(mocks.reveal).toHaveBeenCalledWith("skills");
    expect(mocks.copyText).not.toHaveBeenCalled();
  });
});
