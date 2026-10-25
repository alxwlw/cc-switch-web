import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// In web builds `@tauri-apps/api/core` is aliased to the HTTP adapter; mirror that here.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});

import { promptsApi } from "@/lib/api/prompts";

describe("promptsApi.importFromFile · web adapter (WEB-31)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  const jsonResponse = (body: unknown) =>
    ({ status: 200, json: async () => body }) as Response;

  it("POST /api/:app/prompts/import-from-file без тела, возвращает строку id", async () => {
    fetchSpy.mockResolvedValue(
      jsonResponse({ ok: true, data: "imported-1761350400" }),
    );
    const id = await promptsApi.importFromFile("claude");
    expect(id).toBe("imported-1761350400");
    expect(fetchSpy).toHaveBeenCalledTimes(1);
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/claude/prompts/import-from-file");
    expect(init.method).toBe("POST");
    expect(init.body).toBeUndefined();
  });

  it("ошибка сервиса {ok:false,error} пробрасывается дословно", async () => {
    fetchSpy.mockResolvedValue(
      jsonResponse({ ok: false, error: "Prompt file not found" }),
    );
    await expect(promptsApi.importFromFile("codex")).rejects.toThrow(
      "Prompt file not found",
    );
  });
});
