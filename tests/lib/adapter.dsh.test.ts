import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@/lib/api/adapter";

describe("adapter invoke · DSH and Copilot CLI account commands", () => {
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

  it("dsh_account_status: GET status", async () => {
    const status = { signedIn: false, expired: false };
    respond(status);
    await expect(invoke("dsh_account_status")).resolves.toEqual(status);
    const { url, init } = lastCall();
    expect(url).toBe("/api/dsh/status");
    expect(init.method).toBe("GET");
  });

  it("dsh_account_sign_out: POST sign-out", async () => {
    respond(null);
    await invoke("dsh_account_sign_out");
    const { url, init } = lastCall();
    expect(url).toBe("/api/dsh/sign-out");
    expect(init.method).toBe("POST");
  });

  it("dsh_account_login_start: POST login, the server's start comes back unchanged", async () => {
    const start = {
      flowId: "f-1",
      authUrl: "https://auth.example/authorize?x=1",
      expiresIn: 300,
    };
    respond(start);
    await expect(invoke("dsh_account_login_start")).resolves.toEqual(start);
    const { url, init } = lastCall();
    expect(url).toBe("/api/dsh/login");
    expect(init.method).toBe("POST");
  });

  it("dsh_account_login_poll: POST poll with the flow id encoded", async () => {
    respond(false);
    await expect(
      invoke("dsh_account_login_poll", { flowId: "a/b" }),
    ).resolves.toBe(false);
    const { url, init } = lastCall();
    expect(url).toBe("/api/dsh/login/a%2Fb/poll");
    expect(init.method).toBe("POST");
  });

  it("dsh_account_login_cancel: POST cancel with the flow id encoded", async () => {
    respond(null);
    await invoke("dsh_account_login_cancel", { flowId: "a/b" });
    const { url, init } = lastCall();
    expect(url).toBe("/api/dsh/login/a%2Fb/cancel");
    expect(init.method).toBe("POST");
  });

  it("copilot_cli_signed_in_account: GET cli-account", async () => {
    const account = { login: "octo", githubDomain: "github.com", accountId: "1" };
    respond(account);
    await expect(invoke("copilot_cli_signed_in_account")).resolves.toEqual(
      account,
    );
    const { url, init } = lastCall();
    expect(url).toBe("/api/copilot/cli-account");
    expect(init.method).toBe("GET");
  });
});
