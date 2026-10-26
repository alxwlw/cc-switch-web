import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DatabaseUpgrade } from "@/components/DatabaseUpgrade";

const mocks = vi.hoisted(() => ({
  web: { value: true },
  invoke: vi.fn(),
  listen: vi.fn(),
  exit: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, params?: unknown) => {
      const p = params as { db?: number; supported?: number } | undefined;
      return typeof params === "object" && p?.db !== undefined
        ? `${key}[${p.db}][${p.supported}]`
        : key;
    },
  }),
}));
vi.mock("@/lib/api/adapter", () => ({
  get isWeb() {
    return mocks.web.value;
  },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@tauri-apps/plugin-process", () => ({ exit: mocks.exit }));

const payload = {
  path: "/srv/.cc-switch/cc-switch.db",
  error: "db too new",
  kind: "db_version_too_new",
  db_version: 9999,
  supported_version: 23,
};

describe("DatabaseUpgrade (WEB-32 / WEB-00)", () => {
  beforeEach(() => {
    mocks.invoke.mockReset();
    mocks.invoke.mockResolvedValue("1.2.3");
  });

  it("web: no command is invoked, no buttons, version info and operator hint shown", () => {
    mocks.web.value = true;
    render(<DatabaseUpgrade payload={payload} />);

    expect(mocks.invoke).not.toHaveBeenCalled();
    expect(mocks.listen).not.toHaveBeenCalled();
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.getByText("dbUpgrade.versionInfo[9999][23]")).toBeTruthy();
    expect(screen.getByText("db too new")).toBeTruthy();
    expect(screen.getByText("web.update.unsupported")).toBeTruthy();
  });

  it("desktop: checks for updates and keeps the upgrade, folder and quit buttons", async () => {
    mocks.web.value = false;
    render(<DatabaseUpgrade payload={payload} />);

    expect(mocks.invoke).toHaveBeenCalledWith("check_app_update_available");
    expect(await screen.findByText("dbUpgrade.upgradeNow")).toBeTruthy();
    expect(screen.getByText("dbUpgrade.openConfigDir")).toBeTruthy();
    expect(screen.getByText("dbUpgrade.quit")).toBeTruthy();
    expect(screen.queryByText("web.update.unsupported")).toBeNull();
  });
});
