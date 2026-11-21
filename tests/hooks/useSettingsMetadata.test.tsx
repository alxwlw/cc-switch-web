import { renderHook, act } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { useSettingsMetadata } from "@/hooks/useSettingsMetadata";
import { setWeb } from "../helpers/web";

const isPortableMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api", () => ({
  settingsApi: {
    isPortable: (...args: unknown[]) => isPortableMock(...args),
  },
}));

describe("useSettingsMetadata", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("loads portable flag and handles success path", async () => {
    isPortableMock.mockResolvedValue(true);

    const { result } = renderHook(() => useSettingsMetadata());

    expect(result.current.isLoading).toBe(true);
    expect(result.current.isPortable).toBe(false);

    await act(async () => {
      await Promise.resolve();
    });

    expect(result.current.isPortable).toBe(true);
    expect(result.current.isLoading).toBe(false);
  });

  it("handles errors from settingsApi and proceeds", async () => {
    isPortableMock.mockRejectedValue(new Error("network failure"));

    const { result } = renderHook(() => useSettingsMetadata());

    await act(async () => {
      await Promise.resolve();
    });

    expect(result.current.isPortable).toBe(false);
    expect(result.current.isLoading).toBe(false);
  });

  it("allows updating restart flag via setters", async () => {
    isPortableMock.mockResolvedValue(false);

    const { result } = renderHook(() => useSettingsMetadata());

    await act(async () => {
      await Promise.resolve();
    });

    await act(async () => {
      result.current.setRequiresRestart(true);
      await Promise.resolve();
    });

    expect(result.current.requiresRestart).toBe(true);

    await act(async () => {
      result.current.acknowledgeRestart();
      await Promise.resolve();
    });

    expect(result.current.requiresRestart).toBe(false);
  });

  it("web: asks is_portable_mode through the adapter and takes its answer", async () => {
    setWeb(true);
    isPortableMock.mockResolvedValue(false);

    const { result } = renderHook(() => useSettingsMetadata());

    expect(result.current.isLoading).toBe(true);

    await act(async () => {
      await Promise.resolve();
    });

    expect(isPortableMock).toHaveBeenCalledTimes(1);
    expect(result.current.isPortable).toBe(false);
    expect(result.current.isLoading).toBe(false);
  });
});
