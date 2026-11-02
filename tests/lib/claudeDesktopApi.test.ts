import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { providersApi } from "@/lib/api/providers";

// In the web build `@tauri-apps/api/core` is aliased to the adapter; route invoke through it.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter =
    await vi.importActual<typeof import("@/lib/api/adapter")>(
      "@/lib/api/adapter",
    );
  return { invoke: adapter.invoke };
});

const ok = (data: unknown) => ({
  ok: true,
  status: 200,
  json: async () => ({ ok: true, data }),
});

describe("Claude Desktop + official seed API over the web adapter (WEB-19)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  const status = {
    supported: true,
    installed: false,
    configured: false,
    proxyRunning: false,
    staleRawModels: false,
    missingRouteMappings: false,
    gatewayTokenConfigured: false,
  };

  // [name, call, method, url, response]
  const CASES: Array<
    [string, () => Promise<unknown>, string, string, unknown]
  > = [
    [
      "get_claude_desktop_status",
      () => providersApi.getClaudeDesktopStatus(),
      "GET",
      "/api/claude-desktop/status",
      status,
    ],
    [
      "get_claude_desktop_default_routes",
      () => providersApi.getClaudeDesktopDefaultRoutes(),
      "GET",
      "/api/claude-desktop/default-routes",
      [
        {
          routeId: "claude-sonnet-5",
          envKey: "ANTHROPIC_DEFAULT_SONNET_MODEL",
          supports1m: true,
        },
      ],
    ],
    [
      "import_claude_desktop_providers_from_claude",
      () => providersApi.importClaudeDesktopFromClaude(),
      "POST",
      "/api/claude-desktop/import-from-claude",
      3,
    ],
    [
      "ensure_claude_desktop_official_provider",
      () => providersApi.ensureClaudeDesktopOfficialProvider(),
      "POST",
      "/api/official-seeds/claude-desktop/ensure",
      true,
    ],
    [
      "ensure_codex_official_provider",
      () => providersApi.ensureCodexOfficialProvider(),
      "POST",
      "/api/official-seeds/codex/ensure",
      false,
    ],
    [
      "ensure_grokbuild_official_provider",
      () => providersApi.ensureGrokBuildOfficialProvider(),
      "POST",
      "/api/official-seeds/grokbuild/ensure",
      true,
    ],
  ];

  it.each(CASES)(
    "%s → %s %s and unwraps the payload",
    async (_cmd, call, method, url, data) => {
      fetchSpy.mockResolvedValue(ok(data));
      await expect(call()).resolves.toEqual(data);
      expect(fetchSpy).toHaveBeenCalledTimes(1);
      const [calledUrl, init] = fetchSpy.mock.calls[0];
      expect(calledUrl).toBe(url);
      expect(init.method).toBe(method);
      expect(init.body).toBeUndefined();
    },
  );

  it("passes the server's error text through verbatim", async () => {
    fetchSpy.mockResolvedValue({
      ok: false,
      status: 400,
      json: async () => ({ ok: false, error: "no such seed: nope" }),
    });
    await expect(providersApi.ensureCodexOfficialProvider()).rejects.toThrow(
      "no such seed: nope",
    );
  });
});
