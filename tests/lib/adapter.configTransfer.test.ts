import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke, WebUnsupportedError } from "@/lib/api/adapter";

describe("adapter · config import/export and sync-live (WEB-09)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  const ok = (data: unknown) =>
    new Response(JSON.stringify({ ok: true, data }), { status: 200 });

  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => vi.unstubAllGlobals());

  it("export_config_to_string → GET /api/config/export", async () => {
    fetchSpy.mockResolvedValue(ok({ filename: "f.sql", content: "c" }));
    await expect(invoke("export_config_to_string")).resolves.toEqual({
      filename: "f.sql",
      content: "c",
    });
    expect(fetchSpy.mock.calls[0][0]).toBe("/api/config/export");
    expect(fetchSpy.mock.calls[0][1].method).toBe("GET");
  });

  it("import_config_from_string → POST /api/config/import with {content}", async () => {
    fetchSpy.mockResolvedValue(ok({ success: true, backupId: "b" }));
    await invoke("import_config_from_string", { content: "SELECT 1;" });
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/config/import");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body)).toEqual({ content: "SELECT 1;" });
  });

  it("sync_current_providers_live → POST /api/providers/sync-live", async () => {
    fetchSpy.mockResolvedValue(ok({ success: true, message: "m" }));
    await expect(invoke("sync_current_providers_live")).resolves.toEqual({
      success: true,
      message: "m",
    });
    expect(fetchSpy.mock.calls[0][0]).toBe("/api/providers/sync-live");
    expect(fetchSpy.mock.calls[0][1].method).toBe("POST");
  });

  it("server error text is thrown verbatim", async () => {
    fetchSpy.mockResolvedValue(
      new Response(JSON.stringify({ ok: false, error: "bad dump: line 3" }), {
        status: 400,
      }),
    );
    await expect(
      invoke("import_config_from_string", { content: "x" }),
    ).rejects.toThrow("bad dump: line 3");
  });

  it("desktop-only file commands stay unmapped", async () => {
    for (const cmd of [
      "export_config_to_file",
      "import_config_from_file",
      "save_file_dialog",
      "open_file_dialog",
    ])
      await expect(invoke(cmd, { filePath: "/x" })).rejects.toBeInstanceOf(
        WebUnsupportedError,
      );
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});
