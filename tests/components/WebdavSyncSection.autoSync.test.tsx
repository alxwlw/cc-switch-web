import { screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import "@testing-library/jest-dom";

import { setWeb } from "../helpers/web";
import {
  baseConfig,
  renderSection,
  s3Config,
} from "../helpers/webdavSyncSection";

// The auto-sync switch of the cloud-sync section, in web and on desktop (WEB-11).
// Split from WebdavSyncSection.test.tsx, which had grown past 800 lines.

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    warning: vi.fn(),
    info: vi.fn(),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
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
    webdavSyncSaveSettings: vi.fn(),
    s3SyncSaveSettings: vi.fn(),
  },
}));

vi.mock("@/lib/api", () => ({
  settingsApi: settingsApiMock,
}));

beforeEach(() => {
  settingsApiMock.webdavSyncSaveSettings.mockReset();
  settingsApiMock.s3SyncSaveSettings.mockReset();
  settingsApiMock.webdavSyncSaveSettings.mockResolvedValue({ success: true });
  settingsApiMock.s3SyncSaveSettings.mockResolvedValue({ success: true });
});

describe.each([
  ["web", true],
  ["desktop", false],
])("auto-sync switch in %s (WEB-11)", (_mode, web) => {
  const webdavSwitch = () =>
    screen.getByRole("switch", { name: "settings.webdavSync.autoSync" });
  const s3Switch = () =>
    screen.getByRole("switch", { name: "settings.s3Sync.autoSync" });

  beforeEach(() => setWeb(web));

  it("WebDAV: the switch is enabled and there is no 'later' caption", () => {
    renderSection({ ...baseConfig, autoSync: false });

    expect(webdavSwitch()).toBeEnabled();
    expect(screen.queryByText("web.syncAutoLater")).toBeNull();
    // The server-host note is the only web-only text left in the header.
    expect(screen.queryByText("web.syncHost") !== null).toBe(web);
  });

  it("WebDAV: a stored autoSync:true can be switched off and the save sends false", async () => {
    renderSection({ ...baseConfig, autoSync: true });

    expect(webdavSwitch()).toHaveAttribute("aria-checked", "true");
    fireEvent.click(webdavSwitch());
    expect(webdavSwitch()).toHaveAttribute("aria-checked", "false");
    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.save" }),
    );
    await waitFor(() => {
      expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledWith(
        expect.objectContaining({ autoSync: false }),
        false,
      );
    });
  });

  it("WebDAV: switching on asks for the confirmation, then the save sends true", async () => {
    renderSection({ ...baseConfig, autoSync: false });

    fireEvent.click(webdavSwitch());
    expect(webdavSwitch()).toHaveAttribute("aria-checked", "false");
    fireEvent.click(
      await screen.findByRole("button", { name: "confirm.autoSync.confirm" }),
    );
    await waitFor(() =>
      expect(webdavSwitch()).toHaveAttribute("aria-checked", "true"),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.save" }),
    );
    await waitFor(() => {
      expect(settingsApiMock.webdavSyncSaveSettings).toHaveBeenCalledWith(
        expect.objectContaining({ autoSync: true }),
        false,
      );
    });
  });

  it("S3: the switch is enabled, there is no caption, and toggling it off is saved", async () => {
    renderSection(baseConfig, s3Config);

    expect(s3Switch()).toBeEnabled();
    expect(s3Switch()).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByText("web.syncAutoLater")).toBeNull();
    fireEvent.click(s3Switch());
    expect(s3Switch()).toHaveAttribute("aria-checked", "false");
    fireEvent.click(
      screen.getByRole("button", { name: "settings.s3Sync.save" }),
    );
    await waitFor(() => {
      expect(settingsApiMock.s3SyncSaveSettings).toHaveBeenCalledWith(
        expect.objectContaining({ bucket: "my-bucket", autoSync: false }),
        false,
      );
    });
  });

  it("S3: toggling it back on is saved as true", async () => {
    renderSection(baseConfig, { ...s3Config, autoSync: false });

    fireEvent.click(s3Switch());
    expect(s3Switch()).toHaveAttribute("aria-checked", "true");
    fireEvent.click(
      screen.getByRole("button", { name: "settings.s3Sync.save" }),
    );
    await waitFor(() => {
      expect(settingsApiMock.s3SyncSaveSettings).toHaveBeenCalledWith(
        expect.objectContaining({ autoSync: true }),
        false,
      );
    });
  });

  it("WebDAV: the switch is locked while a save is running", async () => {
    settingsApiMock.webdavSyncSaveSettings.mockReturnValue(
      new Promise(() => {}),
    );
    renderSection(baseConfig);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.webdavSync.save" }),
    );
    await waitFor(() => expect(webdavSwitch()).toBeDisabled());
  });

  it("S3: the switch is locked while a save is running", async () => {
    settingsApiMock.s3SyncSaveSettings.mockReturnValue(new Promise(() => {}));
    renderSection(baseConfig, s3Config);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.s3Sync.save" }),
    );
    await waitFor(() => expect(s3Switch()).toBeDisabled());
  });
});
