import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});

import { WebUnsupportedError, invoke } from "@/lib/api/adapter";
import { workspaceApi } from "@/lib/api/workspace";
import { setWeb } from "../helpers/web";

describe("Workspace API · web adapter (WEB-15)", () => {
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

  it("readFile / writeFile: filename in the path, content in the body; null kept", async () => {
    reply(null);
    expect(await workspaceApi.readFile("AGENTS.md")).toBeNull();
    expect(lastCall()).toEqual({
      url: "/api/openclaw/workspace/AGENTS.md",
      method: "GET",
      body: undefined,
    });
    reply(null);
    await workspaceApi.writeFile("SOUL.md", "hi");
    expect(lastCall()).toEqual({
      url: "/api/openclaw/workspace/SOUL.md",
      method: "PUT",
      body: { content: "hi" },
    });
  });

  it("filename is URL-encoded, so a traversal name stays one path segment", async () => {
    reply(null);
    await workspaceApi.readFile("../secret.md");
    expect(lastCall().url).toBe("/api/openclaw/workspace/..%2Fsecret.md");
  });

  it("listDailyMemoryFiles: GET", async () => {
    reply([]);
    expect(await workspaceApi.listDailyMemoryFiles()).toEqual([]);
    expect(lastCall()).toEqual({
      url: "/api/openclaw/daily-memory",
      method: "GET",
      body: undefined,
    });
  });

  it("readDailyMemoryFile: GET, null kept", async () => {
    reply(null);
    expect(await workspaceApi.readDailyMemoryFile("2026-10-10.md")).toBeNull();
    expect(lastCall()).toEqual({
      url: "/api/openclaw/daily-memory/2026-10-10.md",
      method: "GET",
      body: undefined,
    });
  });

  it("writeDailyMemoryFile / deleteDailyMemoryFile", async () => {
    reply(null);
    await workspaceApi.writeDailyMemoryFile("2026-10-10.md", "note");
    expect(lastCall()).toEqual({
      url: "/api/openclaw/daily-memory/2026-10-10.md",
      method: "PUT",
      body: { content: "note" },
    });
    await workspaceApi.deleteDailyMemoryFile("2026-10-10.md");
    expect(lastCall()).toEqual({
      url: "/api/openclaw/daily-memory/2026-10-10.md",
      method: "DELETE",
      body: undefined,
    });
  });

  it("searchDailyMemoryFiles: query goes as ?q=, encoded", async () => {
    reply([]);
    await workspaceApi.searchDailyMemoryFiles("a b&c");
    expect(lastCall()).toEqual({
      url: "/api/openclaw/daily-memory/search?q=a%20b%26c",
      method: "GET",
      body: undefined,
    });
  });

  it("getPaths: GET workspace-paths", async () => {
    reply({
      workspace: "/srv/.openclaw/workspace",
      memory: "/srv/.openclaw/workspace/memory",
    });
    expect(await workspaceApi.getPaths()).toEqual({
      workspace: "/srv/.openclaw/workspace",
      memory: "/srv/.openclaw/workspace/memory",
    });
    expect(lastCall().url).toBe("/api/openclaw/workspace-paths");
  });

  it("open_workspace_directory stays unmapped (desktop-only)", async () => {
    await expect(
      workspaceApi.openDirectory("workspace"),
    ).rejects.toBeInstanceOf(WebUnsupportedError);
    await expect(
      invoke("open_workspace_directory", { subdir: "memory" }),
    ).rejects.toBeInstanceOf(WebUnsupportedError);
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});
