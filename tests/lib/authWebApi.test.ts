import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  agyAccountIsRunning,
  authCancelLogin,
  authGetStatus,
  authListAccounts,
  authLogout,
  authPollForAccount,
  authRemoveAccount,
  authSetDefaultAccount,
  authStartLogin,
} from "@/lib/api/auth";
import { copyText } from "@/lib/clipboard";

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

describe("managed auth API over the web adapter (WEB-02)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    fetchSpy = vi.fn();
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

  it("authStartLogin -> POST /api/auth/:provider/login", async () => {
    fetchSpy.mockResolvedValue(ok({ device_code: "d" }));
    await authStartLogin(
      "github_copilot",
      "ghe.example",
      "acct-1",
      "device_code",
    );
    expect(lastCall()).toEqual({
      url: "/api/auth/github_copilot/login",
      method: "POST",
      body: {
        githubDomain: "ghe.example",
        targetAccountId: "acct-1",
        loginMethod: "device_code",
      },
    });
  });

  it("authPollForAccount -> POST .../poll, null account passes through", async () => {
    fetchSpy.mockResolvedValue(ok(null));
    expect(
      await authPollForAccount("codex_oauth", "dev-1", "gh.com"),
    ).toBeNull();
    expect(lastCall()).toEqual({
      url: "/api/auth/codex_oauth/poll",
      method: "POST",
      body: { deviceCode: "dev-1", githubDomain: "gh.com" },
    });
  });

  it("authCancelLogin -> POST .../cancel", async () => {
    fetchSpy.mockResolvedValue(ok(true));
    expect(await authCancelLogin("codex_oauth", "dev-2")).toBe(true);
    expect(lastCall()).toEqual({
      url: "/api/auth/codex_oauth/cancel",
      method: "POST",
      body: { deviceCode: "dev-2" },
    });
  });

  it("authListAccounts -> GET .../accounts", async () => {
    fetchSpy.mockResolvedValue(ok([]));
    expect(await authListAccounts("xai_oauth")).toEqual([]);
    expect(lastCall()).toEqual({
      url: "/api/auth/xai_oauth/accounts",
      method: "GET",
      body: undefined,
    });
  });

  it("authGetStatus -> GET .../status", async () => {
    fetchSpy.mockResolvedValue(ok({ authenticated: false }));
    expect(await authGetStatus("agy_oauth")).toEqual({ authenticated: false });
    expect(lastCall()).toEqual({
      url: "/api/auth/agy_oauth/status",
      method: "GET",
      body: undefined,
    });
  });

  it("authRemoveAccount -> DELETE .../accounts/:id (id is URL-encoded)", async () => {
    fetchSpy.mockResolvedValue(ok(null));
    await authRemoveAccount("github_copilot", "a/b c");
    expect(lastCall()).toEqual({
      url: "/api/auth/github_copilot/accounts/a%2Fb%20c",
      method: "DELETE",
      body: undefined,
    });
  });

  it("authSetDefaultAccount -> PUT .../default {accountId}", async () => {
    fetchSpy.mockResolvedValue(ok(null));
    await authSetDefaultAccount("codex_oauth", "acct-9");
    expect(lastCall()).toEqual({
      url: "/api/auth/codex_oauth/default",
      method: "PUT",
      body: { accountId: "acct-9" },
    });
  });

  it("authLogout -> POST .../logout", async () => {
    fetchSpy.mockResolvedValue(ok(null));
    await authLogout("xai_oauth");
    expect(lastCall()).toEqual({
      url: "/api/auth/xai_oauth/logout",
      method: "POST",
      body: undefined,
    });
  });

  it("agyAccountIsRunning -> GET /api/auth/agy/running", async () => {
    fetchSpy.mockResolvedValue(ok(true));
    expect(await agyAccountIsRunning()).toBe(true);
    expect(lastCall()).toEqual({
      url: "/api/auth/agy/running",
      method: "GET",
      body: undefined,
    });
  });
});

describe("copy_text_to_clipboard over the web adapter (WEB-02)", () => {
  const setSecure = (value: boolean) =>
    Object.defineProperty(window, "isSecureContext", {
      value,
      configurable: true,
    });
  let writeText: ReturnType<typeof vi.fn>;
  let execCommand: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    execCommand = vi.fn().mockReturnValue(true);
    Object.defineProperty(document, "execCommand", {
      value: execCommand,
      configurable: true,
      writable: true,
    });
  });
  afterEach(() => {
    setSecure(false);
    Reflect.deleteProperty(navigator, "clipboard");
    Reflect.deleteProperty(document, "execCommand");
  });

  it("secure context: navigator.clipboard.writeText, no textarea fallback", async () => {
    setSecure(true);
    await copyText("ABCD-1234");
    expect(writeText).toHaveBeenCalledWith("ABCD-1234");
    expect(execCommand).not.toHaveBeenCalled();
  });

  it("insecure context: hidden textarea + execCommand('copy'), textarea removed", async () => {
    setSecure(false);
    let copied: string | undefined;
    execCommand.mockImplementation(() => {
      copied = document.querySelector("textarea")?.value;
      return true;
    });
    await copyText("WXYZ-9876");
    expect(execCommand).toHaveBeenCalledWith("copy");
    expect(copied).toBe("WXYZ-9876");
    expect(writeText).not.toHaveBeenCalled();
    expect(document.querySelector("textarea")).toBeNull();
  });

  it("execCommand refused: rejects and still removes the textarea", async () => {
    setSecure(false);
    // a non-secure page has no navigator.clipboard, so clipboard.ts has nothing to fall back to
    Object.defineProperty(navigator, "clipboard", {
      value: undefined,
      configurable: true,
    });
    execCommand.mockReturnValue(false);
    await expect(copyText("nope")).rejects.toThrow();
    expect(document.querySelector("textarea")).toBeNull();
  });
});
