import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UpdateProvider, useUpdate } from "@/contexts/UpdateContext";
import { checkForUpdate } from "@/lib/updater";
import { setWeb } from "../helpers/web";

const mocks = vi.hoisted(() => ({ check: vi.fn(), checkForUpdate: vi.fn() }));

vi.mock("@tauri-apps/plugin-updater", () => ({ check: mocks.check }));
vi.mock("@/lib/updater", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/updater")>();
  mocks.checkForUpdate.mockImplementation(actual.checkForUpdate);
  return { ...actual, checkForUpdate: mocks.checkForUpdate };
});
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("3.0.0"),
}));

const wrapper = ({ children }: { children: ReactNode }) => (
  <UpdateProvider>{children}</UpdateProvider>
);

describe("updater · checkForUpdate", () => {
  beforeEach(() => mocks.check.mockReset().mockResolvedValue(null));

  it("desktop: no update from the plugin is up-to-date", async () => {
    await expect(checkForUpdate()).resolves.toEqual({ status: "up-to-date" });
    expect(mocks.check).toHaveBeenCalledTimes(1);
  });

  it("web: unsupported, never up-to-date, plugin not called", async () => {
    setWeb(true);
    await expect(checkForUpdate()).resolves.toEqual({ status: "unsupported" });
    expect(mocks.check).not.toHaveBeenCalled();
  });
});

describe("UpdateContext", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mocks.check.mockReset().mockResolvedValue(null);
  });
  afterEach(() => vi.useRealTimers());

  it("desktop: auto-checks on mount, reports not unsupported", async () => {
    const { result } = renderHook(() => useUpdate(), { wrapper });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1100);
    });
    expect(mocks.check).toHaveBeenCalledTimes(1);
    expect(result.current.unsupported).toBe(false);
  });

  it("desktop: an available update sets hasUpdate", async () => {
    mocks.check.mockResolvedValue({ version: "9.9.9", notes: "n" });
    const { result } = renderHook(() => useUpdate(), { wrapper });
    let available = false;
    await act(async () => {
      available = await result.current.checkUpdate();
    });
    expect(available).toBe(true);
    expect(result.current.hasUpdate).toBe(true);
    expect(result.current.updateInfo?.availableVersion).toBe("9.9.9");
  });

  it("web: skips the mount check, hasUpdate stays false, manual check is not fake success", async () => {
    setWeb(true);
    const { result } = renderHook(() => useUpdate(), { wrapper });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(mocks.checkForUpdate).not.toHaveBeenCalled();
    expect(mocks.check).not.toHaveBeenCalled();
    expect(result.current.unsupported).toBe(true);
    expect(result.current.hasUpdate).toBe(false);

    let available = true;
    await act(async () => {
      available = await result.current.checkUpdate();
    });
    expect(available).toBe(false);
    expect(mocks.checkForUpdate).toHaveBeenCalledTimes(1);
    expect(mocks.check).not.toHaveBeenCalled();
    expect(result.current.hasUpdate).toBe(false);
  });
});
