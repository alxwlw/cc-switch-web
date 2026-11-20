import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// In web builds `@tauri-apps/api/core` is aliased to the HTTP adapter; mirror that here.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});

import { settingsApi } from "@/lib/api/settings";

describe("app-specific settings API · web adapter (WEB-20)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
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

  it("hasCodexUnifyHistoryBackup: GET", async () => {
    reply(true);
    expect(await settingsApi.hasCodexUnifyHistoryBackup()).toBe(true);
    expect(lastCall()).toEqual({
      url: "/api/codex/unify-history-backup",
      method: "GET",
      body: undefined,
    });
  });

  it("codexForcesMultiAgentV2: GET", async () => {
    reply(true);
    expect(await settingsApi.codexForcesMultiAgentV2()).toBe(true);
    expect(lastCall()).toEqual({
      url: "/api/codex/forces-multi-agent-v2",
      method: "GET",
      body: undefined,
    });
  });

  it("getCodexSessionCompression: GET", async () => {
    reply(true);
    expect(await settingsApi.getCodexSessionCompression()).toBe(true);
    expect(lastCall()).toEqual({
      url: "/api/codex/session-compression",
      method: "GET",
      body: undefined,
    });
  });

  it("setCodexSessionCompression: PUT {enabled}", async () => {
    reply(false);
    await settingsApi.setCodexSessionCompression(false);
    expect(lastCall()).toEqual({
      url: "/api/codex/session-compression",
      method: "PUT",
      body: { enabled: false },
    });
  });

  it("getCodexSessionsDiskUsage: GET", async () => {
    reply(4096);
    expect(await settingsApi.getCodexSessionsDiskUsage()).toBe(4096);
    expect(lastCall()).toEqual({
      url: "/api/codex/sessions-disk-usage",
      method: "GET",
      body: undefined,
    });
  });

  it("restoreCodexUnifiedHistory: POST, returns the camelCase result", async () => {
    const result = { restoredJsonlFiles: 3, restoredStateRows: 7 };
    reply(result);
    expect(await settingsApi.restoreCodexUnifiedHistory()).toEqual(result);
    expect(lastCall()).toEqual({
      url: "/api/codex/restore-unified-history",
      method: "POST",
      body: undefined,
    });
  });

  it("applyClaudePluginConfig: POST passes `official` through unchanged", async () => {
    reply(true);
    await settingsApi.applyClaudePluginConfig({ official: true });
    expect(lastCall()).toEqual({
      url: "/api/claude/plugin-config",
      method: "POST",
      body: { official: true },
    });
    await settingsApi.applyClaudePluginConfig({ official: false });
    expect(lastCall().body).toEqual({ official: false });
  });

  it("applyClaudeOnboardingSkip: POST", async () => {
    reply(true);
    await settingsApi.applyClaudeOnboardingSkip();
    expect(lastCall()).toEqual({
      url: "/api/claude/onboarding-skip",
      method: "POST",
      body: undefined,
    });
  });

  it("clearClaudeOnboardingSkip: DELETE", async () => {
    reply(true);
    await settingsApi.clearClaudeOnboardingSkip();
    expect(lastCall()).toEqual({
      url: "/api/claude/onboarding-skip",
      method: "DELETE",
      body: undefined,
    });
  });
});
