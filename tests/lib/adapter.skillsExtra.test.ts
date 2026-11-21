import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@/lib/api/adapter";

describe("adapter invoke · skill repo tokens, per-repo discovery, metadata refresh", () => {
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

  it("get_skill_repo_tokens: GET /api/skills/repo-tokens", async () => {
    const list = { hosts: ["git.example.com"], secureStorage: false };
    respond(list);
    await expect(invoke("get_skill_repo_tokens")).resolves.toEqual(list);
    const { url, init } = lastCall();
    expect(url).toBe("/api/skills/repo-tokens");
    expect(init.method).toBe("GET");
  });

  it("set_skill_repo_token: PUT body { host, token } and answers the host", async () => {
    respond("git.example.com");
    await expect(
      invoke("set_skill_repo_token", {
        host: "Git.Example.com",
        token: "tok-1",
        extra: "dropped",
      }),
    ).resolves.toBe("git.example.com");
    const { url, init } = lastCall();
    expect(url).toBe("/api/skills/repo-tokens");
    expect(init.method).toBe("PUT");
    expect(JSON.parse(init.body as string)).toEqual({
      host: "Git.Example.com",
      token: "tok-1",
    });
  });

  it("delete_skill_repo_token: DELETE with the host percent-encoded", async () => {
    respond("git.example.com:8443");
    await expect(
      invoke("delete_skill_repo_token", { host: "git.example.com:8443" }),
    ).resolves.toBe("git.example.com:8443");
    const { url, init } = lastCall();
    expect(url).toBe("/api/skills/repo-tokens/git.example.com%3A8443");
    expect(init.method).toBe("DELETE");
    expect(init.body).toBeUndefined();
  });

  it("discover_repo_skills: GET per-repo URL with owner and name encoded", async () => {
    const report = { skills: [], failures: [] };
    respond(report);
    await expect(
      invoke("discover_repo_skills", { owner: "ac me", name: "skills/x" }),
    ).resolves.toEqual(report);
    const { url, init } = lastCall();
    expect(url).toBe("/api/skills/repos/ac%20me/skills%2Fx/discover");
    expect(init.method).toBe("GET");
  });

  it("refresh_local_skill_metadata: POST /api/skills/refresh-metadata", async () => {
    respond(3);
    await expect(invoke("refresh_local_skill_metadata")).resolves.toBe(3);
    const { url, init } = lastCall();
    expect(url).toBe("/api/skills/refresh-metadata");
    expect(init.method).toBe("POST");
  });
});
