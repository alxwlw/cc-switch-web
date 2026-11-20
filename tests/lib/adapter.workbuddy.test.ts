import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@/lib/api/adapter";

describe("adapter invoke · WorkBuddy account commands", () => {
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

  it("workbuddy_account_live_state: GET live-state", async () => {
    const live = { running: false, signedIn: true };
    respond(live);
    await expect(invoke("workbuddy_account_live_state")).resolves.toEqual(live);
    const { url, init } = lastCall();
    expect(url).toBe("/api/workbuddy/live-state");
    expect(init.method).toBe("GET");
  });

  it("workbuddy_account_import_current: POST without a body", async () => {
    respond({ created: true });
    await invoke("workbuddy_account_import_current");
    const { url, init } = lastCall();
    expect(url).toBe("/api/workbuddy/import-current");
    expect(init.method).toBe("POST");
    expect(init.body).toBeUndefined();
  });

  it("workbuddy_account_quota: force true goes into the query", async () => {
    respond({ snapshot: null });
    await invoke("workbuddy_account_quota", { accountId: "a/b", force: true });
    const { url, init } = lastCall();
    expect(url).toBe("/api/workbuddy/accounts/a%2Fb/quota?force=true");
    expect(init.method).toBe("GET");
  });

  it("workbuddy_account_quota: no force means no query string", async () => {
    respond({ snapshot: null });
    await invoke("workbuddy_account_quota", { accountId: "u-1" });
    expect(lastCall().url).toBe("/api/workbuddy/accounts/u-1/quota");
  });

  it("workbuddy_model_owners: GET model-owners", async () => {
    const owners = [{ id: "m", owner: null }];
    respond(owners);
    await expect(invoke("workbuddy_model_owners")).resolves.toEqual(owners);
    const { url, init } = lastCall();
    expect(url).toBe("/api/workbuddy/model-owners");
    expect(init.method).toBe("GET");
  });
});
