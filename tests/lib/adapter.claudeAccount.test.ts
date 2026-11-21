import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@/lib/api/adapter";

describe("adapter invoke · Claude account commands", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;

  const respond = (data: unknown) =>
    fetchSpy.mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => ({ ok: true, data }),
    });

  const lastCall = () => {
    const [url, init] = fetchSpy.mock.calls[0] as [string, RequestInit];
    return { url, init };
  };

  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("get_claude_account_quota: GET with the account id URL-encoded", async () => {
    const quota = { tool: "claude", success: true };
    respond(quota);
    await expect(
      invoke("get_claude_account_quota", { accountId: "a/b" }),
    ).resolves.toEqual(quota);
    const { url, init } = lastCall();
    expect(url).toBe("/api/claude-account/quota/a%2Fb");
    expect(init.method).toBe("GET");
  });

  it("get_claude_default_plan: GET default-plan, resolves the data", async () => {
    respond({ subscriptionType: "max" });
    await expect(invoke("get_claude_default_plan")).resolves.toEqual({
      subscriptionType: "max",
    });
    const { url, init } = lastCall();
    expect(url).toBe("/api/claude-account/default-plan");
    expect(init.body).toBeUndefined();
    expect(init.method).toBe("GET");
  });

  it("get_claude_login_switch: GET login-switch, resolves the data", async () => {
    respond({ active: true, blocked: null, live: "acc-1" });
    await expect(invoke("get_claude_login_switch")).resolves.toEqual({
      active: true,
      blocked: null,
      live: "acc-1",
    });
    const { url, init } = lastCall();
    expect(url).toBe("/api/claude-account/login-switch");
    expect(init.method).toBe("GET");
  });

  it("claude_account_submit_paste: POST paste with flowId and pasted", async () => {
    respond(null);
    await expect(
      invoke("claude_account_submit_paste", {
        flowId: "flow-7",
        pasted: "http://localhost/cb?code=x",
      }),
    ).resolves.toBeNull();
    const { url, init } = lastCall();
    expect(url).toBe("/api/claude-account/paste");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body as string)).toEqual({
      flowId: "flow-7",
      pasted: "http://localhost/cb?code=x",
    });
  });
});
