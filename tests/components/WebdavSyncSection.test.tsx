import { screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import "@testing-library/jest-dom";
import { QueryClientProvider } from "@tanstack/react-query";

import { WebdavSyncSection } from "@/components/settings/WebdavSyncSection";
import { setWeb } from "../helpers/web";
import {
  baseConfig,
  renderSection,
  s3Config,
} from "../helpers/webdavSyncSection";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const toastWarningMock = vi.fn();
const toastInfoMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    warning: (...args: unknown[]) => toastWarningMock(...args),
    info: (...args: unknown[]) => toastInfoMock(...args),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("@/components/ui/button", () => ({
  Button: ({ children, ...props }: any) => <button {...props}>{children}</button>,
}));

vi.mock("@/components/ui/input", () => ({
  Input: (props: any) => <input {...props} />,
}));

vi.mock("@/components/ui/switch", () => ({
  Switch: ({ checked, onCheckedChange, ...props }: any) => (
    <button
      role="switch"
      aria-checked={checked}
      onClick={() => onCheckedChange?.(!checked)}
      {...props}
    />
  ),
}));

vi.mock("@/components/ui/select", () => ({
  Select: ({ value, onValueChange, children }: any) => (
    <select
      data-testid="webdav-preset-select"
      value={value}
      onChange={(event) => onValueChange?.(event.target.value)}
    >
      {children}
    </select>
  ),
  SelectTrigger: ({ children }: any) => <>{children}</>,
  SelectValue: () => null,
  SelectContent: ({ children }: any) => <>{children}</>,
  SelectItem: ({ value, children }: any) => (
    <option value={value}>{children}</option>
  ),
}));

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ open, children }: any) => (open ? <div>{children}</div> : null),
  DialogContent: ({ children }: any) => <div>{children}</div>,
  DialogDescription: ({ children }: any) => <div>{children}</div>,
  DialogFooter: ({ children }: any) => <div>{children}</div>,
  DialogHeader: ({ children }: any) => <div>{children}</div>,
  DialogTitle: ({ children }: any) => <h2>{children}</h2>,
}));

const { settingsApiMock } = vi.hoisted(() => ({
  settingsApiMock: {
    webdavTestConnection: vi.fn(),
    webdavSyncSaveSettings: vi.fn(),
    webdavSyncFetchRemoteInfo: vi.fn(),
    webdavSyncUpload: vi.fn(),
    webdavSyncDownload: vi.fn(),
    s3SyncSaveSettings: vi.fn(),
    s3TestConnection: vi.fn(),
    s3SyncFetchRemoteInfo: vi.fn(),
    s3SyncDownload: vi.fn(),
  },
}));

vi.mock("@/lib/api", () => ({
  settingsApi: settingsApiMock,
}));

describe("WebdavSyncSection", () => {
  beforeEach(() => {
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    toastWarningMock.mockReset();
    toastInfoMock.mockReset();
    settingsApiMock.webdavTestConnection.mockReset();
    settingsApiMock.webdavSyncSaveSettings.mockReset();
    settingsApiMock.webdavSyncFetchRemoteInfo.mockReset();
    settingsApiMock.webdavSyncUpload.mockReset();
    settingsApiMock.webdavSyncDownload.mockReset();
    settingsApiMock.s3SyncSaveSettings.mockReset();
    settingsApiMock.s3TestConnection.mockReset();
    settingsApiMock.s3SyncFetchRemoteInfo.mockReset();
    settingsApiMock.s3SyncDownload.mockReset();

    settingsApiMock.s3SyncSaveSettings.mockResolvedValue({ success: true });
    settingsApiMock.s3TestConnection.mockResolvedValue({ success: true });
    settingsApiMock.webdavSyncSaveSettings.mockResolvedValue({ success: true });
    settingsApiMock.webdavTestConnection.mockResolvedValue({
      success: true,
      message: "ok",
    });
    settingsApiMock.webdavSyncFetchRemoteInfo.mockResolvedValue({
      deviceName: "My MacBook",
      createdAt: "2026-02-01T10:00:00Z",
      snapshotId: "snapshot-1",
      version: 2,
      compatible: true,
      artifacts: ["db.sql", "skills.zip"],
    });
    settingsApiMock.webdavSyncUpload.mockResolvedValue({ status: "uploaded" });
    settingsApiMock.webdavSyncDownload.mockResolvedValue({ status: "downloaded" });
    settingsApiMock.s3SyncFetchRemoteInfo.mockResolvedValue({
      deviceName: "My MacBook",
      createdAt: "2026-02-01T10:00:00Z",
      snapshotId: "snapshot-1",
      version: 2,
      compatible: true,
      artifacts: ["db.sql", "skills.zip"],
    });
    settingsApiMock.s3SyncDownload.mockResolvedValue({ status: "downloaded" });
  });

  it("shows auto sync error callout when last auto sync failed", () => {
    renderSection({
      ...baseConfig,
      status: {
        lastError: "network timeout",
        lastErrorSource: "auto",
      },
    });

    expect(
      screen.getByText("settings.webdavSync.autoSyncLastErrorTitle"),
    ).toBeInTheDocument();
    expect(screen.getByText("network timeout")).toBeInTheDocument();
  });

  it("does not show auto sync error callout for manual sync errors", () => {
    renderSection({
      ...baseConfig,
      status: {
        lastError: "manual upload failed",
        lastErrorSource: "manual",
      },
    });

    expect(
      screen.queryByText("settings.webdavSync.autoSyncLastErrorTitle"),
    ).not.toBeInTheDocument();
  });

  it("does not show auto sync error callout when source is missing", () => {
    renderSection({
      ...baseConfig,
      autoSync: true,
      status: {
        lastError: "legacy error without source",
      },
    });

    expect(
      screen.queryByText("settings.webdavSync.autoSyncLastErrorTitle"),
    ).not.toBeInTheDocument();
  });

  it("shows validation error when saving without base url", async () => {
    renderSection({ ...baseConfig, baseUrl: "" });

    fireEvent.click(screen.getByRole("button", { name: "settings.webdavSync.save" }));

    expect(toastErrorMock).toHaveBeenCalledWith("settings.webdavSync.missingUrl");
    expect(settingsApiMock.webdavSyncSaveSettings).not.toHaveBeenCalled();
  });

  it("saves settings and auto tests connection", async () => {
    renderSection(baseConfig);

    fireEvent.click(screen.getByRole("button", { name: "settings.webdavSync.save" }));

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledTimes(1);
    });
    expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledWith(
      expect.objectContaining({
        baseUrl: "https://dav.example.com/dav/",
        username: "alice",
        password: "",
        autoSync: false,
      }),
      false,
    );
    await waitFor(() => {
      expect(settingsApiMock.webdavTestConnection).toHaveBeenCalledWith(
        expect.objectContaining({
          baseUrl: "https://dav.example.com/dav/",
        }),
        true,
      );
    });
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "settings.webdavSync.saveAndTestSuccess",
    );
  });

  it("preserves password only for the single post-save refresh", async () => {
    const view = renderSection(baseConfig);

    fireEvent.click(screen.getByRole("button", { name: "settings.webdavSync.save" }));

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledTimes(1);
    });

    view.rerender(
      <QueryClientProvider client={view.client}>
        <WebdavSyncSection config={{ ...baseConfig, password: "" }} />
      </QueryClientProvider>,
    );

    expect(
      (
        screen.getByPlaceholderText(
          "settings.webdavSync.passwordPlaceholder",
        ) as HTMLInputElement
      ).value,
    ).toBe("secret");

    view.rerender(
      <QueryClientProvider client={view.client}>
        <WebdavSyncSection config={{ ...baseConfig, password: "" }} />
      </QueryClientProvider>,
    );

    expect(
      (
        screen.getByPlaceholderText(
          "settings.webdavSync.passwordPlaceholder",
        ) as HTMLInputElement
      ).value,
    ).toBe("");
  });

  it("does not preserve password after a later external config refresh", async () => {
    const view = renderSection(baseConfig);

    fireEvent.click(screen.getByRole("button", { name: "settings.webdavSync.save" }));

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledTimes(1);
    });

    view.rerender(
      <QueryClientProvider client={view.client}>
        <WebdavSyncSection config={{ ...baseConfig, password: "" }} />
      </QueryClientProvider>,
    );

    expect(
      (
        screen.getByPlaceholderText(
          "settings.webdavSync.passwordPlaceholder",
        ) as HTMLInputElement
      ).value,
    ).toBe("secret");

    view.rerender(
      <QueryClientProvider client={view.client}>
        <WebdavSyncSection
          config={{ ...baseConfig, username: "bob", password: "" }}
        />
      </QueryClientProvider>,
    );

    expect(
      (
        screen.getByPlaceholderText(
          "settings.webdavSync.passwordPlaceholder",
        ) as HTMLInputElement
      ).value,
    ).toBe("");
  });

  it("does not submit a preserved password again when testing without touching it", async () => {
    const view = renderSection(baseConfig);

    fireEvent.click(screen.getByRole("button", { name: "settings.webdavSync.save" }));

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledTimes(1);
    });

    view.rerender(
      <QueryClientProvider client={view.client}>
        <WebdavSyncSection config={{ ...baseConfig, password: "" }} />
      </QueryClientProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "settings.webdavSync.test" }));

    await waitFor(() => {
      expect(settingsApiMock.webdavTestConnection).toHaveBeenLastCalledWith(
        expect.objectContaining({
          password: "",
        }),
        true,
      );
    });
  });

  it("saves auto sync as true after toggle", async () => {
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("switch", { name: "settings.webdavSync.autoSync" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "confirm.autoSync.confirm" }),
    );
    await waitFor(() => {
      expect(
        screen.getByRole("switch", { name: "settings.webdavSync.autoSync" }),
      ).toHaveAttribute("aria-checked", "true");
    });
    fireEvent.click(screen.getByRole("button", { name: "settings.webdavSync.save" }));

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          autoSync: true,
        }),
        false,
      );
    });
  });

  it("blocks upload when there are unsaved changes", async () => {
    renderSection(baseConfig);

    fireEvent.change(
      screen.getByPlaceholderText("settings.webdavSync.usernamePlaceholder"),
      { target: { value: "bob" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.upload" }),
    );

    expect(toastErrorMock).toHaveBeenCalledWith(
      "settings.webdavSync.unsavedChanges",
    );
    expect(settingsApiMock.webdavSyncFetchRemoteInfo).not.toHaveBeenCalled();
  });

  it("disables sync buttons until config is saved", () => {
    renderSection(undefined);

    const uploadButton = screen.getByRole("button", {
      name: "settings.webdavSync.upload",
    });
    const downloadButton = screen.getByRole("button", {
      name: "settings.webdavSync.download",
    });

    expect(uploadButton).toBeDisabled();
    expect(downloadButton).toBeDisabled();
    expect(settingsApiMock.webdavSyncFetchRemoteInfo).not.toHaveBeenCalled();
  });

  it("fetches remote info and uploads after confirmation", async () => {
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.upload" }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncFetchRemoteInfo).toHaveBeenCalledTimes(1);
    });

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.webdavSync.confirmUpload.confirm",
      }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncUpload).toHaveBeenCalledTimes(1);
    });
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "settings.webdavSync.uploadSuccess",
    );
  });

  it("blocks upload confirmation if form changes after dialog opens", async () => {
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.upload" }),
    );
    await waitFor(() => {
      expect(settingsApiMock.webdavSyncFetchRemoteInfo).toHaveBeenCalledTimes(1);
    });

    fireEvent.change(screen.getByPlaceholderText("cc-switch-sync"), {
      target: { value: "new-root" },
    });
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.webdavSync.confirmUpload.confirm",
      }),
    );

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        "settings.webdavSync.unsavedChanges",
      );
    });
    expect(settingsApiMock.webdavSyncUpload).not.toHaveBeenCalled();
  });

  it("fetches remote info and downloads after confirmation", async () => {
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.download" }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncFetchRemoteInfo).toHaveBeenCalledTimes(1);
    });

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.webdavSync.confirmDownload.confirm",
      }),
    );

    await waitFor(() => {
      expect(settingsApiMock.webdavSyncDownload).toHaveBeenCalledTimes(1);
    });
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "settings.webdavSync.downloadSuccess",
    );
  });

  it("blocks download confirmation if form changes after dialog opens", async () => {
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.download" }),
    );
    await waitFor(() => {
      expect(settingsApiMock.webdavSyncFetchRemoteInfo).toHaveBeenCalledTimes(1);
    });

    fireEvent.change(screen.getByPlaceholderText("default"), {
      target: { value: "new-profile" },
    });
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.webdavSync.confirmDownload.confirm",
      }),
    );

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        "settings.webdavSync.unsavedChanges",
      );
    });
    expect(settingsApiMock.webdavSyncDownload).not.toHaveBeenCalled();
  });

  it("shows info when no remote snapshot is found for download", async () => {
    settingsApiMock.webdavSyncFetchRemoteInfo.mockResolvedValueOnce({ empty: true });
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.download" }),
    );

    await waitFor(() => {
      expect(toastInfoMock).toHaveBeenCalledWith("settings.webdavSync.noRemoteData");
    });
    expect(settingsApiMock.webdavSyncDownload).not.toHaveBeenCalled();
  });

  it("blocks download when remote snapshot is incompatible", async () => {
    settingsApiMock.webdavSyncFetchRemoteInfo.mockResolvedValueOnce({
      deviceName: "Legacy Machine",
      createdAt: "2025-01-01T00:00:00Z",
      snapshotId: "legacy-snapshot",
      version: 1,
      compatible: false,
      artifacts: ["db.sql"],
    });
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.download" }),
    );

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        "settings.webdavSync.incompatibleVersion",
      );
    });
    expect(settingsApiMock.webdavSyncDownload).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("button", {
        name: "settings.webdavSync.confirmDownload.confirm",
      }),
    ).not.toBeInTheDocument();
  });

  it("shows error when download fails after confirmation", async () => {
    settingsApiMock.webdavSyncDownload.mockRejectedValueOnce(new Error("boom"));
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.download" }),
    );
    await waitFor(() => {
      expect(settingsApiMock.webdavSyncFetchRemoteInfo).toHaveBeenCalledTimes(1);
    });

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.webdavSync.confirmDownload.confirm",
      }),
    );

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        "settings.webdavSync.downloadFailed",
      );
    });
  });

  describe("download confirm names the server in web (WEB-11)", () => {
    const DESKTOP_WEBDAV = "settings.webdavSync.confirmDownload.warning";
    const DESKTOP_S3 = "settings.s3Sync.confirmDownload.warning";
    const WEB = "web.syncDownloadWarning";

    it("web, WebDAV: the confirm names the server instead of 'local data'", async () => {
      setWeb(true);
      renderSection(baseConfig);

      fireEvent.click(
        screen.getByRole("button", { name: "settings.webdavSync.download" }),
      );
      await screen.findByText(WEB);
      expect(screen.queryByText(DESKTOP_WEBDAV)).toBeNull();
    });

    it("desktop, WebDAV: the confirm keeps the desktop text", async () => {
      renderSection(baseConfig);

      fireEvent.click(
        screen.getByRole("button", { name: "settings.webdavSync.download" }),
      );
      await screen.findByText(DESKTOP_WEBDAV);
      expect(screen.queryByText(WEB)).toBeNull();
    });

    it("web, S3: the confirm names the server instead of 'local data'", async () => {
      setWeb(true);
      renderSection(baseConfig, s3Config);

      fireEvent.click(
        screen.getByRole("button", { name: "settings.s3Sync.download" }),
      );
      await screen.findByText(WEB);
      expect(screen.queryByText(DESKTOP_S3)).toBeNull();
    });

    it("desktop, S3: the confirm keeps the desktop text", async () => {
      renderSection(baseConfig, s3Config);

      fireEvent.click(
        screen.getByRole("button", { name: "settings.s3Sync.download" }),
      );
      await screen.findByText(DESKTOP_S3);
      expect(screen.queryByText(WEB)).toBeNull();
    });
  });

  describe("form rows at phone width (WEB-11)", () => {
    // jsdom has no layout: this pins the classes, the widths were measured in a browser.
    const rowLabels = () =>
      screen
        .getAllByText(/^settings\.(syncType|webdavSync|s3Sync)\./)
        .filter((el) => el.tagName === "LABEL");

    // From sm up a row is the old one: a label beside the field, items-start only on the
    // three toggle rows (autoSync, S3 enabled), items-center on every other row.
    const TOP_ROW =
      /^settings\.(webdavSync\.autoSync|s3Sync\.(autoSync|enabled))/;

    function expectRowLayout(
      labels: HTMLElement[],
      count: number,
      tops: number,
    ) {
      expect(labels).toHaveLength(count);
      let top = 0;
      for (const label of labels) {
        const row = label.parentElement?.className ?? "";
        const isTop = TOP_ROW.test(label.textContent ?? "");
        top += isTop ? 1 : 0;
        expect(label.className).toContain("sm:w-40");
        expect(label.className).toContain("sm:shrink-0");
        expect(label.className).not.toMatch(/(^|\s)w-40(\s|$)/);
        expect(row).toContain("flex-col");
        expect(row).toContain("sm:flex-row");
        expect(row).toContain("sm:gap-4");
        expect(row).toContain(isTop ? "sm:items-start" : "sm:items-center");
      }
      expect(top).toBe(tops);
    }

    it("WebDAV form: the label sits above the field below sm, beside it from sm", () => {
      renderSection(baseConfig);
      expectRowLayout(rowLabels(), 8, 1);
    });

    it("S3 form: the label sits above the field below sm, beside it from sm", () => {
      renderSection(baseConfig, s3Config);
      expectRowLayout(rowLabels(), 11, 2);
    });

    it.each([
      [
        "WebDAV",
        "https://dav.jianguoyun.com/dav/",
        undefined,
        "settings.webdavSync.presets.jianguoyunHint",
      ],
      ["S3", baseConfig.baseUrl, s3Config, "settings.s3Sync.presets.awsS3Hint"],
    ])(
      "%s preset hint is indented under the field only from sm",
      (_n, baseUrl, s3, key) => {
        renderSection({ ...baseConfig, baseUrl }, s3);
        const hint = screen.getByText(key).parentElement?.className ?? "";
        expect(hint).toContain("sm:pl-44");
        expect(hint).not.toMatch(/(^|\s)pl-44(\s|$)/);
      },
    );
  });
});
