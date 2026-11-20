import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// In web builds `@tauri-apps/api/core` is aliased to the HTTP adapter; mirror that here.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});

import { piApi } from "@/lib/api/pi";
import { promptsApi } from "@/lib/api/prompts";
import { setWeb } from "../helpers/web";

describe("Pi / Oh My Pi API · web adapter (WEB-17)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    setWeb(true);
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
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

  it.each([
    ["pi", "/api/pi/current-state"],
    ["omp", "/api/omp/current-state"],
  ] as const)("getCurrentState(%s): GET", async (app, url) => {
    reply({ enabledProviderIds: ["a"], defaultProviderId: "a" });
    expect(await piApi.getCurrentState(app)).toEqual({
      enabledProviderIds: ["a"],
      defaultProviderId: "a",
    });
    expect(lastCall()).toEqual({ url, method: "GET", body: undefined });
  });

  it.each(["pi", "omp"] as const)(
    "updateProviderUsageScript(%s): PUT with camelCase usageScript",
    async (app) => {
      reply(true);
      const usageScript = { enabled: true, language: "javascript", code: "x" };
      expect(
        await piApi.updateProviderUsageScript("p 1", usageScript as any, app),
      ).toBe(true);
      expect(lastCall()).toEqual({
        url: `/api/${app}/providers/p%201/usage-script`,
        method: "PUT",
        body: { usageScript },
      });
    },
  );

  it("getSessionDiscovery: GET", async () => {
    reply({ status: "available" });
    expect(await piApi.getSessionDiscovery()).toEqual({ status: "available" });
    expect(lastCall()).toEqual({
      url: "/api/pi/session-discovery",
      method: "GET",
      body: undefined,
    });
  });

  it("getPiPromptFile: GET by kind", async () => {
    reply({ exists: false, revision: "missing", content: "" });
    await promptsApi.getPiPromptFile("system_override");
    expect(lastCall()).toEqual({
      url: "/api/pi/prompt-files/system_override",
      method: "GET",
      body: undefined,
    });
  });

  it("replacePiPromptFile: PUT with expectedRevision and content", async () => {
    reply({ exists: true, revision: "r2", content: "c" });
    await promptsApi.replacePiPromptFile("system_append", "r1", "c");
    expect(lastCall()).toEqual({
      url: "/api/pi/prompt-files/system_append",
      method: "PUT",
      body: { expectedRevision: "r1", content: "c" },
    });
  });

  it("deletePiPromptFile: DELETE with expectedRevision", async () => {
    reply(true);
    await promptsApi.deletePiPromptFile("system_append", "r1");
    expect(lastCall()).toEqual({
      url: "/api/pi/prompt-files/system_append",
      method: "DELETE",
      body: { expectedRevision: "r1" },
    });
  });

  it("listPiPromptTemplates: GET", async () => {
    reply([]);
    await promptsApi.listPiPromptTemplates();
    expect(lastCall()).toEqual({
      url: "/api/pi/prompt-templates",
      method: "GET",
      body: undefined,
    });
  });

  it("upsertPiPromptTemplate: PUT, slug in the path, originalSlug in the body", async () => {
    reply({ slug: "new", content: "c", revision: "r2" });
    await promptsApi.upsertPiPromptTemplate("new", "r1", "c", "old");
    expect(lastCall()).toEqual({
      url: "/api/pi/prompt-templates/new",
      method: "PUT",
      body: { originalSlug: "old", expectedRevision: "r1", content: "c" },
    });
    await promptsApi.upsertPiPromptTemplate("fresh", "missing", "c");
    expect(lastCall().body.originalSlug).toBeNull();
  });

  it("deletePiPromptTemplate: DELETE with expectedRevision", async () => {
    reply(true);
    await promptsApi.deletePiPromptTemplate("old", "r1");
    expect(lastCall()).toEqual({
      url: "/api/pi/prompt-templates/old",
      method: "DELETE",
      body: { expectedRevision: "r1" },
    });
  });

  it("a 409 conflict reaches the caller with the server text verbatim", async () => {
    const text =
      "并发冲突: Pi prompt file changed outside CC Switch: /home/u/.pi/agent/SYSTEM.md";
    fetchSpy.mockResolvedValue({
      status: 409,
      json: async () => ({ ok: false, error: text }),
    } as Response);
    await expect(
      promptsApi.replacePiPromptFile("system_override", "old", "c"),
    ).rejects.toThrow(text);
  });
});
