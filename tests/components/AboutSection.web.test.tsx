import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AboutSection } from "@/components/settings/AboutSection";
import { UpdateProvider } from "@/contexts/UpdateContext";
import { setWeb } from "../helpers/web";

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  checkUpdates: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en" },
  }),
}));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: mocks.check }));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("3.0.0"),
}));
vi.mock("@/lib/api", () => ({
  settingsApi: {
    checkUpdates: mocks.checkUpdates,
    openExternal: vi.fn(),
  },
}));

function renderAbout() {
  return render(
    <UpdateProvider>
      <AboutSection isPortable={false} />
    </UpdateProvider>,
  );
}

describe("AboutSection · update controls", () => {
  beforeEach(() => {
    mocks.check.mockReset().mockResolvedValue(null);
    mocks.checkUpdates.mockReset();
  });

  it("desktop: shows the check-for-updates button", async () => {
    renderAbout();
    expect(
      await screen.findByText("settings.checkForUpdates"),
    ).toBeInTheDocument();
    expect(screen.queryByText("web.update.unsupported")).toBeNull();
  });

  it("web: shows the unsupported notice, no check button, no up-to-date", async () => {
    setWeb(true);
    renderAbout();
    expect(
      await screen.findByText("web.update.unsupported"),
    ).toBeInTheDocument();
    expect(screen.queryByText("settings.checkForUpdates")).toBeNull();
    expect(screen.queryByText("settings.upToDate")).toBeNull();
    expect(mocks.check).not.toHaveBeenCalled();
    expect(mocks.checkUpdates).not.toHaveBeenCalled();
  });
});
