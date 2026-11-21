import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@/lib/api/adapter";

describe("adapter invoke · session content search", () => {
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

  it("sync_session_index: POST /api/sessions/index/sync", async () => {
    const status = { running: true, processed: 0, total: 3 };
    respond(status);
    await expect(invoke("sync_session_index")).resolves.toEqual(status);
    const { url, init } = lastCall();
    expect(url).toBe("/api/sessions/index/sync");
    expect(init.method).toBe("POST");
  });

  it("search_session_content: POST body { query, providerIds, limit }", async () => {
    respond({ hits: [] });
    await invoke("search_session_content", {
      query: "needle",
      providerIds: ["claude", "codex"],
      limit: 7,
    });
    const { url, init } = lastCall();
    expect(url).toBe("/api/sessions/search");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body as string)).toEqual({
      query: "needle",
      providerIds: ["claude", "codex"],
      limit: 7,
    });
  });

  it("search_session_content: undefined providerIds and limit are left out of the body", async () => {
    respond({ hits: [] });
    await invoke("search_session_content", { query: "needle" });
    expect(lastCall().init.body).toBe(JSON.stringify({ query: "needle" }));
  });

  it("clear_session_index: DELETE /api/sessions/index", async () => {
    respond(true);
    await expect(invoke("clear_session_index")).resolves.toBe(true);
    const { url, init } = lastCall();
    expect(url).toBe("/api/sessions/index");
    expect(init.method).toBe("DELETE");
  });
});
