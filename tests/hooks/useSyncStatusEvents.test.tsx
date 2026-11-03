import { QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { createTestQueryClient } from "../utils/testQueryClient";
import { useSyncStatusEvents } from "@/hooks/useSyncStatusEvents";

const toastErrorMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/toast", () => ({
  toast: { error: toastErrorMock },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { error?: string }) =>
      options?.error === undefined ? key : `${key}|${options.error}`,
  }),
}));

// In the web build `@tauri-apps/api/event` is aliased to the SSE shim; the global test
// mock (a plain emitter) would bypass the very path WEB-11 turns on.
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

async function mount() {
  const client = createTestQueryClient();
  const invalidate = vi.spyOn(client, "invalidateQueries");
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  renderHook(() => useSyncStatusEvents(), { wrapper });
  await vi.waitFor(() => expect(FakeES.instance).not.toBeNull());
  const es = FakeES.instance!;
  await vi.waitFor(() => {
    expect(es.listeners.has("webdav-sync-status-updated")).toBe(true);
    expect(es.listeners.has("s3-sync-status-updated")).toBe(true);
  });
  // The handler awaits the invalidation before it decides on the toast: awaiting the
  // same promise afterwards runs after the handler's continuation.
  const handled = async () => {
    await vi.waitFor(() => expect(invalidate).toHaveBeenCalledTimes(1));
    await invalidate.mock.results[0].value;
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["settings"] });
  };
  return { es, handled };
}

const BACKENDS = [
  ["WebDAV", "webdav-sync-status-updated", "settings.webdavSync"],
  ["S3", "s3-sync-status-updated", "settings.s3Sync"],
] as const;

describe.each(BACKENDS)(
  "%s sync status over the web SSE shim (WEB-11)",
  (_name, event, i18nPrefix) => {
    beforeEach(() => {
      FakeES.instance = null;
      toastErrorMock.mockReset();
      vi.stubGlobal("EventSource", FakeES);
    });
    afterEach(() => {
      vi.unstubAllGlobals();
    });

    it("a failed auto sync toasts the error and refreshes the settings cache", async () => {
      const { es, handled } = await mount();

      es.emit(
        event,
        JSON.stringify({
          source: "auto",
          status: "error",
          error: "network timeout",
        }),
      );
      await handled();

      expect(toastErrorMock).toHaveBeenCalledTimes(1);
      expect(toastErrorMock).toHaveBeenCalledWith(
        `${i18nPrefix}.autoSyncFailedToast|network timeout`,
      );
      expect(es.url).toBe("/api/events");
    });

    it("a failed auto sync without a message falls back to the unknown-error text", async () => {
      const { es, handled } = await mount();

      es.emit(event, JSON.stringify({ source: "auto", status: "error" }));
      await handled();

      expect(toastErrorMock).toHaveBeenCalledWith(
        `${i18nPrefix}.autoSyncFailedToast|common.unknown`,
      );
    });

    it("a successful auto sync refreshes the settings cache without a toast", async () => {
      const { es, handled } = await mount();

      es.emit(event, JSON.stringify({ source: "auto", status: "success" }));
      await handled();

      expect(toastErrorMock).not.toHaveBeenCalled();
    });

    it("a failed manual sync refreshes the cache and stays silent (the caller already toasts)", async () => {
      const { es, handled } = await mount();

      es.emit(
        event,
        JSON.stringify({ source: "manual", status: "error", error: "boom" }),
      );
      await handled();

      expect(toastErrorMock).not.toHaveBeenCalled();
    });

    it("a null payload refreshes the cache and does not throw", async () => {
      const { es, handled } = await mount();

      es.emit(event, "null");
      await handled();

      expect(toastErrorMock).not.toHaveBeenCalled();
    });
  },
);
