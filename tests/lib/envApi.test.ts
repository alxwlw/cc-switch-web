import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// In web builds `@tauri-apps/api/core` is aliased to the HTTP adapter; mirror that here.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});

import type { EnvConflict } from "@/types/env";
import { deleteEnvVars, restoreEnvBackup } from "@/lib/api/env";

describe("env API · web adapter (WEB-22)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  const jsonResponse = (body: unknown, status = 200) =>
    ({ status, json: async () => body }) as Response;

  it("deleteEnvVars: POST /api/env/conflicts/cleanup with {conflicts}", async () => {
    const conflicts: EnvConflict[] = [
      {
        varName: "ANTHROPIC_API_KEY",
        varValue: "sk-x",
        sourceType: "file",
        sourcePath: "/home/u/.bashrc:3",
      },
    ];
    fetchSpy.mockResolvedValue(
      jsonResponse({ ok: true, data: { backupPath: "/srv/bk.json" } }),
    );
    expect(await deleteEnvVars(conflicts)).toEqual({
      backupPath: "/srv/bk.json",
    });
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/env/conflicts/cleanup");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body)).toEqual({ conflicts });
  });

  it("restoreEnvBackup: POST /api/env/restore with {backupPath}", async () => {
    fetchSpy.mockResolvedValue(jsonResponse({ ok: true, data: null }));
    await restoreEnvBackup("/srv/bk.json");
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/env/restore");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body)).toEqual({ backupPath: "/srv/bk.json" });
  });
});
