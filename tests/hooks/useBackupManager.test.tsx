import type { ReactNode } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useBackupLocations, useBackupManager } from "@/hooks/useBackupManager";
import { backupsApi } from "@/lib/api/settings";
import { createTestQueryClient } from "../utils/testQueryClient";
import { setWeb } from "../helpers/web";

// In the web build `@tauri-apps/api/core` is aliased to the adapter (vite.config.web.mts);
// route the test's invoke through the real adapter the same way.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter =
    await vi.importActual<typeof import("@/lib/api/adapter")>(
      "@/lib/api/adapter",
    );
  return { invoke: adapter.invoke };
});

type Call = { method: string; url: string; body: unknown };

const calls: Call[] = [];
const fetchMock = vi.fn();

const BACKUPS = [
  {
    filename: "db_backup_1.db",
    sizeBytes: 10,
    createdAt: "2026-10-27T10:00:00Z",
  },
];
const LOCATIONS = [
  {
    id: "database",
    path: "/srv/cc-switch/backups",
    sizeBytes: 10,
    itemCount: 1,
    lastModified: null,
    deletable: false,
  },
  {
    id: "skills",
    path: "/srv/cc-switch/skill-backups",
    sizeBytes: 5,
    itemCount: 2,
    lastModified: "2026-10-27T10:00:00Z",
    deletable: true,
  },
];

function respond(method: string, url: string): unknown {
  if (method === "GET" && url === "/api/backups") return BACKUPS;
  if (method === "GET" && url === "/api/backup-locations") return LOCATIONS;
  if (method === "POST" && url === "/api/backups") return "db_backup_2.db";
  if (method === "POST" && url.endsWith("/restore")) return "pre-restore_1";
  if (method === "PATCH") return "renamed.db";
  if (method === "DELETE" && url.startsWith("/api/backup-locations/"))
    return 4096;
  return null;
}

function wrapperWith(client = createTestQueryClient()) {
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return { client, wrapper };
}

const mutating = () => calls.filter((c) => c.method !== "GET");

describe("useBackupManager / useBackupLocations (web)", () => {
  beforeEach(() => {
    setWeb(true);
    calls.length = 0;
    fetchMock
      .mockReset()
      .mockImplementation(async (url: string, init: RequestInit) => {
        const method = init?.method ?? "GET";
        calls.push({
          method,
          url,
          body: init?.body ? JSON.parse(String(init.body)) : undefined,
        });
        return {
          ok: true,
          status: 200,
          json: async () => ({ ok: true, data: respond(method, url) }),
        };
      });
    vi.stubGlobal("fetch", fetchMock);
  });
  afterEach(() => vi.unstubAllGlobals());

  it("lists backups through GET /api/backups", async () => {
    const { wrapper } = wrapperWith();
    const { result } = renderHook(() => useBackupManager(), { wrapper });
    await waitFor(() => expect(result.current.backups).toEqual(BACKUPS));
    expect(calls[0]).toEqual({
      method: "GET",
      url: "/api/backups",
      body: undefined,
    });
  });

  it("create posts to /api/backups and refreshes backups and locations", async () => {
    const { client, wrapper } = wrapperWith();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const { result } = renderHook(() => useBackupManager(), { wrapper });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    let filename = "";
    await act(async () => {
      filename = await result.current.create();
    });
    expect(filename).toBe("db_backup_2.db");
    expect(mutating()).toEqual([
      { method: "POST", url: "/api/backups", body: undefined },
    ]);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["backup-locations"] });
  });

  it("restore posts the encoded filename and invalidates every query", async () => {
    const { client, wrapper } = wrapperWith();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const { result } = renderHook(() => useBackupManager(), { wrapper });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    let safety = "";
    await act(async () => {
      safety = await result.current.restore("my backup.db");
    });
    expect(safety).toBe("pre-restore_1");
    expect(mutating()).toEqual([
      {
        method: "POST",
        url: "/api/backups/my%20backup.db/restore",
        body: undefined,
      },
    ]);
    expect(invalidate).toHaveBeenCalledWith();
    expect(result.current.isRestoring).toBe(false);
  });

  it("rename patches the old filename with {newName}", async () => {
    const { wrapper } = wrapperWith();
    const { result } = renderHook(() => useBackupManager(), { wrapper });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    let renamed = "";
    await act(async () => {
      renamed = await result.current.rename({
        oldFilename: "db_backup_1.db",
        newName: "renamed",
      });
    });
    expect(renamed).toBe("renamed.db");
    expect(mutating()).toEqual([
      {
        method: "PATCH",
        url: "/api/backups/db_backup_1.db",
        body: { newName: "renamed" },
      },
    ]);
  });

  it("delete sends DELETE for the filename and refreshes locations", async () => {
    const { client, wrapper } = wrapperWith();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const { result } = renderHook(() => useBackupManager(), { wrapper });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await act(async () => {
      await result.current.remove("db_backup_1.db");
    });
    expect(mutating()).toEqual([
      { method: "DELETE", url: "/api/backups/db_backup_1.db", body: undefined },
    ]);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["backup-locations"] });
  });

  it("locations: list via GET /api/backup-locations, delete returns freed bytes", async () => {
    const { wrapper } = wrapperWith();
    const { result } = renderHook(() => useBackupLocations(), { wrapper });
    await waitFor(() => expect(result.current.locations).toEqual(LOCATIONS));
    let freed = 0;
    await act(async () => {
      freed = await result.current.remove("skills");
    });
    expect(freed).toBe(4096);
    expect(mutating()).toEqual([
      {
        method: "DELETE",
        url: "/api/backup-locations/skills",
        body: undefined,
      },
    ]);
  });

  it("reveal_backup_location stays unmapped in web (no request)", async () => {
    await expect(backupsApi.revealBackupLocation("database")).rejects.toThrow();
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
