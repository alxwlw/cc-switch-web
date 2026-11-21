import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const toastWarning = vi.hoisted(() => vi.fn());
vi.mock("@/lib/toast", () => ({ toast: { warning: toastWarning } }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      `${key}:${options?.name}`,
  }),
}));
// In the web build `@tauri-apps/api/event` is aliased to the SSE shim.
vi.mock("@tauri-apps/api/event", async () => {
  const shim = await vi.importActual<typeof import("@/lib/web-shims/event")>(
    "@/lib/web-shims/event",
  );
  return { listen: shim.listen };
});

class FakeES {
  static instance: FakeES | null = null;
  readyState = 1;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  listeners = new Map<string, Array<(e: { data: string }) => void>>();
  constructor(public url: string) {
    FakeES.instance = this;
  }
  addEventListener(name: string, fn: (e: { data: string }) => void) {
    this.listeners.set(name, [...(this.listeners.get(name) ?? []), fn]);
  }
  close() {}
  emit(name: string, data: string) {
    for (const fn of this.listeners.get(name) ?? []) fn({ data });
  }
}

describe("useProxyOfficialWarning over the web SSE shim (WEB-01)", () => {
  beforeEach(() => {
    toastWarning.mockReset();
    FakeES.instance = null;
    vi.stubGlobal("EventSource", FakeES);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("shows the warning toast for a proxy-official-warning frame", async () => {
    const { useProxyOfficialWarning } = await import(
      "@/hooks/useProxyOfficialWarning"
    );
    renderHook(() => useProxyOfficialWarning());
    await vi.waitFor(() => expect(FakeES.instance).not.toBeNull());
    await vi.waitFor(() =>
      expect(FakeES.instance!.listeners.has("proxy-official-warning")).toBe(
        true,
      ),
    );

    FakeES.instance!.emit(
      "proxy-official-warning",
      JSON.stringify({ appType: "claude", providerName: "Claude Official" }),
    );

    expect(url()).toBe("/api/events");
    expect(toastWarning).toHaveBeenCalledTimes(1);
    expect(toastWarning).toHaveBeenCalledWith(
      "notifications.proxyOfficialWarning:Claude Official",
      { duration: 8000 },
    );
  });
});

function url() {
  return FakeES.instance!.url;
}
