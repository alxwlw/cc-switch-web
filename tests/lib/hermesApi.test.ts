import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockInstance } from "vitest";

// In web builds `@tauri-apps/api/core` is aliased to the HTTP adapter; mirror that here.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});

const toastMock = vi.hoisted(() => ({ error: vi.fn(), success: vi.fn() }));
vi.mock("sonner", () => ({ toast: toastMock }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts ? `${key}|${JSON.stringify(opts)}` : key,
  }),
}));

import { WebUnsupportedError, invoke } from "@/lib/api/adapter";
import { hermesApi } from "@/lib/api/hermes";
import { providersApi } from "@/lib/api/providers";
import { useOpenHermesWebUI } from "@/hooks/useHermes";
import { setWeb } from "../helpers/web";

describe("Hermes API · web adapter (WEB-16)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  let openSpy: MockInstance<typeof window.open>;

  beforeEach(() => {
    setWeb(true);
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
    openSpy = vi.spyOn(window, "open").mockReturnValue({} as Window);
    toastMock.error.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  const reply = (data: unknown) =>
    fetchSpy.mockResolvedValue({
      status: 200,
      json: async () => ({ ok: true, data }),
    } as Response);

  const lastCall = () => {
    const [url, init] = fetchSpy.mock.calls.at(-1)!;
    return {
      url: String(url),
      method: init?.method as string,
      body: init?.body ? JSON.parse(init.body as string) : undefined,
    };
  };

  it("importHermesFromLive: POST", async () => {
    reply(3);
    expect(await providersApi.importHermesFromLive()).toBe(3);
    expect(lastCall()).toEqual({
      url: "/api/hermes/import-live",
      method: "POST",
      body: undefined,
    });
  });

  it("getHermesLiveProviderIds: GET", async () => {
    reply(["a", "b"]);
    expect(await providersApi.getHermesLiveProviderIds()).toEqual(["a", "b"]);
    expect(lastCall()).toEqual({
      url: "/api/hermes/live-provider-ids",
      method: "GET",
      body: undefined,
    });
  });

  it("getModelConfig: GET", async () => {
    reply({ provider: "p1" });
    expect(await hermesApi.getModelConfig()).toEqual({ provider: "p1" });
    expect(lastCall().url).toBe("/api/hermes/model-config");
  });

  it("getMemory / setMemory: kind in the path, content in the body", async () => {
    reply("hello");
    expect(await hermesApi.getMemory("user")).toBe("hello");
    expect(lastCall()).toEqual({
      url: "/api/hermes/memory/user",
      method: "GET",
      body: undefined,
    });
    reply(null);
    await hermesApi.setMemory("memory", "new text");
    expect(lastCall()).toEqual({
      url: "/api/hermes/memory/memory",
      method: "PUT",
      body: { content: "new text" },
    });
  });

  it("getMemoryLimits / setMemoryEnabled", async () => {
    reply({ memory: 1 });
    await hermesApi.getMemoryLimits();
    expect(lastCall()).toEqual({
      url: "/api/hermes/memory-limits",
      method: "GET",
      body: undefined,
    });
    reply(null);
    await hermesApi.setMemoryEnabled("user", false);
    expect(lastCall()).toEqual({
      url: "/api/hermes/memory/user/enabled",
      method: "PUT",
      body: { enabled: false },
    });
  });

  it("launch_hermes_dashboard stays unmapped (desktop-only)", async () => {
    await expect(invoke("launch_hermes_dashboard")).rejects.toBeInstanceOf(
      WebUnsupportedError,
    );
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("openWebUI: opens the URL the server returned, path in the query", async () => {
    reply({ reachable: true, port: 9119, url: "http://box:9119/config" });
    await hermesApi.openWebUI("/config");
    expect(lastCall()).toEqual({
      url: "/api/hermes/web-ui?path=%2Fconfig",
      method: "GET",
      body: undefined,
    });
    expect(openSpy).toHaveBeenCalledWith(
      "http://box:9119/config",
      "_blank",
      "noopener,noreferrer",
    );
  });

  it("openWebUI hook: reachable:false shows the remote toast with the port and does not open", async () => {
    reply({ reachable: false, port: 9555, url: "http://box:9555/" });
    const onOffline = vi.fn();
    const { result } = renderHook(() => useOpenHermesWebUI(onOffline));
    await act(async () => {
      await result.current();
    });
    expect(openSpy).not.toHaveBeenCalled();
    expect(onOffline).not.toHaveBeenCalled();
    expect(toastMock.error).toHaveBeenCalledWith(
      'hermes.webui.unreachableRemote|{"port":9555}',
    );
  });
});
