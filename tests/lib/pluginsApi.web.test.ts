import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// In web builds `@tauri-apps/api/core` is aliased to the HTTP adapter; mirror that here, so a
// renamed argument in `pluginsApi` shows up as a wrong URL or body.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});

import { pluginsApi } from "@/lib/api/plugins";

const ok = (data: unknown) => ({
  ok: true,
  status: 200,
  json: async () => ({ ok: true, data }),
});

describe("adapter · plugins", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    fetchSpy = vi.fn().mockResolvedValue(ok(null));
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  const lastCall = () => {
    const [url, init] = fetchSpy.mock.calls.at(-1)!;
    return {
      url,
      method: init.method,
      body: init.body === undefined ? undefined : JSON.parse(init.body),
    };
  };

  it.each([
    [
      "listInstalled",
      () => pluginsApi.listInstalled("claude"),
      "GET",
      "/api/plugins/claude/installed",
      undefined,
    ],
    [
      "listCatalog",
      () => pluginsApi.listCatalog("codex"),
      "GET",
      "/api/plugins/codex/catalog",
      undefined,
    ],
    [
      "listMarketplaces",
      () => pluginsApi.listMarketplaces("grokbuild"),
      "GET",
      "/api/plugins/grokbuild/marketplaces",
      undefined,
    ],
    [
      "addMarketplace",
      () => pluginsApi.addMarketplace("claude", "owner/repo@main"),
      "POST",
      "/api/plugins/claude/marketplaces",
      { source: "owner/repo@main" },
    ],
    [
      "removeMarketplace",
      () => pluginsApi.removeMarketplace("codex", "market"),
      "POST",
      "/api/plugins/codex/marketplaces/remove",
      { name: "market" },
    ],
    [
      "refreshMarketplaces",
      () => pluginsApi.refreshMarketplaces("openclaw"),
      "POST",
      "/api/plugins/openclaw/marketplaces/refresh",
      undefined,
    ],
    [
      "install",
      () => pluginsApi.install("pi", "npm:@scope/pkg"),
      "POST",
      "/api/plugins/pi/install",
      { id: "npm:@scope/pkg" },
    ],
    [
      "uninstall",
      () => pluginsApi.uninstall("claude", "fmt@market"),
      "POST",
      "/api/plugins/claude/uninstall",
      { id: "fmt@market" },
    ],
    [
      "setEnabled",
      () => pluginsApi.setEnabled("mcode", "fmt@market", false),
      "POST",
      "/api/plugins/mcode/enabled",
      { id: "fmt@market", enabled: false },
    ],
    [
      "update",
      () => pluginsApi.update("grokbuild", "fmt"),
      "POST",
      "/api/plugins/grokbuild/update",
      { id: "fmt" },
    ],
  ])("%s -> %s %s", async (_name, call, method, url, body) => {
    await call();
    expect(lastCall()).toEqual({ url, method, body });
  });

  it("resolves with the data of the response", async () => {
    const entries = [{ id: "fmt@market", installed: true }];
    fetchSpy.mockResolvedValueOnce(ok(entries));
    await expect(pluginsApi.listInstalled("claude")).resolves.toEqual(entries);
  });
});
