import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@/lib/api/adapter";

const ok = (data: unknown) => ({
  ok: true,
  status: 200,
  json: async () => ({ ok: true, data }),
});

describe("adapter · skills (WEB-05)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    fetchSpy = vi.fn().mockResolvedValue(ok(null));
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  const lastJson = () => {
    const [url, init] = fetchSpy.mock.calls.at(-1)!;
    return {
      url,
      method: init.method,
      contentType: init.headers["Content-Type"],
      body: init.body === undefined ? undefined : JSON.parse(init.body),
    };
  };

  const skill = { key: "o/n:dir", name: "Dir", repoOwner: "o", repoName: "n" };

  it.each([
    ["get_skill_backups", {}, "GET", "/api/skills/backups", undefined],
    [
      "delete_skill_backup",
      { backupId: "20260101 a/b" },
      "DELETE",
      "/api/skills/backups/20260101%20a%2Fb",
      undefined,
    ],
    [
      "restore_skill_backup",
      { backupId: "b1", currentApp: "claude" },
      "POST",
      "/api/skills/backups/b1/restore",
      { currentApp: "claude" },
    ],
    [
      "install_skill_unified",
      { skill, currentApp: "codex" },
      "POST",
      "/api/skills/install",
      { skill, currentApp: "codex" },
    ],
    ["scan_unmanaged_skills", {}, "GET", "/api/skills/unmanaged", undefined],
    [
      "import_skills_from_apps",
      { imports: [{ directory: "d", apps: ["claude"] }] },
      "POST",
      "/api/skills/import-from-apps",
      { imports: [{ directory: "d", apps: ["claude"] }] },
    ],
    ["discover_available_skills", {}, "GET", "/api/skills/discover", undefined],
    ["check_skill_updates", {}, "GET", "/api/skills/updates", undefined],
    [
      "update_skill",
      { id: "owner/name:dir" },
      "POST",
      "/api/skills/update",
      { id: "owner/name:dir" },
    ],
    [
      "migrate_skill_storage",
      { target: "unified" },
      "POST",
      "/api/skills/migrate-storage",
      { target: "unified" },
    ],
    ["get_cc_switch_skills_dir", {}, "GET", "/api/skills/dir", undefined],
    [
      "search_skills_sh",
      { query: "a b&c", limit: 20, offset: 40 },
      "GET",
      "/api/skills/search?query=a%20b%26c&limit=20&offset=40",
      undefined,
    ],
    ["get_skill_repos", {}, "GET", "/api/skills/repos", undefined],
    [
      "add_skill_repo",
      { repo: { owner: "o", name: "n", branch: "main", enabled: true } },
      "POST",
      "/api/skills/repos",
      { repo: { owner: "o", name: "n", branch: "main", enabled: true } },
    ],
    [
      "remove_skill_repo",
      { owner: "o w", name: "n/x" },
      "DELETE",
      "/api/skills/repos/o%20w/n%2Fx",
      undefined,
    ],
  ])("%s -> %s %s", async (cmd, args, method, url, body) => {
    await invoke(cmd, args);
    expect(lastJson()).toEqual({
      url,
      method,
      contentType: "application/json",
      body,
    });
  });

  it("returns the data of a successful call", async () => {
    fetchSpy.mockResolvedValueOnce(ok("/home/u/.cc-switch/skills"));
    await expect(invoke("get_cc_switch_skills_dir")).resolves.toBe(
      "/home/u/.cc-switch/skills",
    );
  });

  it.each(["open_cc_switch_skills_dir", "open_zip_file_dialog"])(
    "%s stays unsupported in web (no fetch)",
    async (cmd) => {
      await expect(invoke(cmd)).rejects.toThrow();
      expect(fetchSpy).not.toHaveBeenCalled();
    },
  );

  it("ZIP upload sends the File untouched as an octet-stream body with currentApp and filename in the query", async () => {
    const file = new File(["PK\u0003\u0004"], "my skill.zip", {
      type: "application/zip",
    });
    fetchSpy.mockResolvedValueOnce(ok({ installed: [], skipped: [] }));
    await invoke("web_install_skills_from_zip_upload", {
      file,
      currentApp: "claude",
    });

    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe(
      "/api/skills/install-zip?currentApp=claude&filename=my%20skill.zip",
    );
    expect(init.method).toBe("POST");
    expect(init.headers["Content-Type"]).toBe("application/octet-stream");
    expect(init.body).toBe(file);
  });

  it("surfaces a server error verbatim", async () => {
    fetchSpy.mockResolvedValueOnce({
      ok: false,
      status: 500,
      json: async () => ({ ok: false, error: "Failed to read ZIP file" }),
    });
    await expect(
      invoke("web_install_skills_from_zip_upload", {
        file: new File(["x"], "a.zip"),
        currentApp: "claude",
      }),
    ).rejects.toThrow("Failed to read ZIP file");
  });
});
