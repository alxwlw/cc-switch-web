import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { isWeb } from "@/lib/api/adapter";
import { settingsApi } from "@/lib/api/settings";
import { setWeb } from "../helpers/web";
import type { S3SyncSettings, WebDavSyncSettings } from "@/types";

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

const webdav: WebDavSyncSettings = {
  enabled: true,
  baseUrl: "https://dav.example.com/dav/",
  username: "alice",
  password: "dav-secret",
  remoteRoot: "cc-switch-sync",
  profile: "default",
  autoSync: true,
};

const s3: S3SyncSettings = {
  enabled: true,
  autoSync: true,
  region: "eu-west-1",
  bucket: "my-bucket",
  accessKeyId: "AKIAEXAMPLE",
  secretAccessKey: "s3-secret",
  endpoint: "https://s3.example.com",
  remoteRoot: "cc-switch-sync",
  profile: "work",
};

const remoteInfo = {
  deviceName: "box",
  createdAt: "2026-11-03T00:00:00Z",
  snapshotId: "snap",
  version: 2,
  protocolVersion: 2,
  compatible: true,
  artifacts: ["db.sql"],
  layout: "current",
  remotePath: "cc-switch-sync/v2/default",
};

describe("WebDAV / S3 manual sync API over the web adapter (WEB-11)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    setWeb(true);
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("runs in web mode (the test double reports web, jsdom has no Tauri internals)", () => {
    expect(isWeb).toBe(true);
    expect("__TAURI_INTERNALS__" in window).toBe(false);
  });

  // [name, call, method, url, expected JSON body (undefined = no body), response data]
  const CASES: Array<
    [string, () => Promise<unknown>, string, string, unknown, unknown]
  > = [
    [
      "webdav test, password preserved by default",
      () => settingsApi.webdavTestConnection(webdav),
      "POST",
      "/api/sync/webdav/test",
      { settings: webdav, preserveEmptyPassword: true },
      { success: true, message: "ok" },
    ],
    [
      "webdav test, preserveEmptyPassword=false",
      () => settingsApi.webdavTestConnection(webdav, false),
      "POST",
      "/api/sync/webdav/test",
      { settings: webdav, preserveEmptyPassword: false },
      { success: true },
    ],
    [
      "webdav upload",
      () => settingsApi.webdavSyncUpload(),
      "POST",
      "/api/sync/webdav/upload",
      undefined,
      { status: "uploaded" },
    ],
    [
      "webdav download",
      () => settingsApi.webdavSyncDownload(),
      "POST",
      "/api/sync/webdav/download",
      undefined,
      { status: "downloaded" },
    ],
    [
      "webdav save, passwordTouched false by default",
      () => settingsApi.webdavSyncSaveSettings(webdav),
      "PUT",
      "/api/sync/webdav/settings",
      { settings: webdav, passwordTouched: false },
      { success: true },
    ],
    [
      "webdav save, passwordTouched=true",
      () => settingsApi.webdavSyncSaveSettings(webdav, true),
      "PUT",
      "/api/sync/webdav/settings",
      { settings: webdav, passwordTouched: true },
      { success: true },
    ],
    [
      "webdav remote info",
      () => settingsApi.webdavSyncFetchRemoteInfo(),
      "GET",
      "/api/sync/webdav/remote-info",
      undefined,
      remoteInfo,
    ],
    [
      "s3 test, secret preserved by default",
      () => settingsApi.s3TestConnection(s3),
      "POST",
      "/api/sync/s3/test",
      { settings: s3, preserveEmptyPassword: true },
      { success: true },
    ],
    [
      "s3 test, preserveEmptyPassword=false",
      () => settingsApi.s3TestConnection(s3, false),
      "POST",
      "/api/sync/s3/test",
      { settings: s3, preserveEmptyPassword: false },
      { success: true },
    ],
    [
      "s3 upload",
      () => settingsApi.s3SyncUpload(),
      "POST",
      "/api/sync/s3/upload",
      undefined,
      { status: "uploaded" },
    ],
    [
      "s3 download",
      () => settingsApi.s3SyncDownload(),
      "POST",
      "/api/sync/s3/download",
      undefined,
      { status: "downloaded" },
    ],
    [
      "s3 save, passwordTouched=false",
      () => settingsApi.s3SyncSaveSettings(s3, false),
      "PUT",
      "/api/sync/s3/settings",
      { settings: s3, passwordTouched: false },
      { success: true },
    ],
    [
      "s3 save, passwordTouched=true",
      () => settingsApi.s3SyncSaveSettings(s3, true),
      "PUT",
      "/api/sync/s3/settings",
      { settings: s3, passwordTouched: true },
      { success: true },
    ],
    [
      "s3 remote info (empty remote)",
      () => settingsApi.s3SyncFetchRemoteInfo(),
      "GET",
      "/api/sync/s3/remote-info",
      undefined,
      { empty: true },
    ],
  ];

  for (const [name, call, method, url, body, data] of CASES) {
    it(`${name} → ${method} ${url}`, async () => {
      fetchSpy.mockResolvedValue(ok(data));
      await expect(call()).resolves.toEqual(data);
      expect(fetchSpy).toHaveBeenCalledTimes(1);
      const [calledUrl, init] = fetchSpy.mock.calls[0];
      expect(calledUrl).toBe(url);
      expect(init.method).toBe(method);
      // The test and settings routes answer 415 without this header.
      expect(init.headers["Content-Type"]).toBe("application/json");
      if (body === undefined) {
        expect(init.body).toBeUndefined();
      } else {
        expect(JSON.parse(init.body)).toEqual(body);
      }
    });
  }

  // [name, call] for every one of the 10 functions
  const FAILING: Array<[string, () => Promise<unknown>]> = [
    ["webdav test", () => settingsApi.webdavTestConnection(webdav)],
    ["webdav upload", () => settingsApi.webdavSyncUpload()],
    ["webdav download", () => settingsApi.webdavSyncDownload()],
    ["webdav save", () => settingsApi.webdavSyncSaveSettings(webdav)],
    ["webdav remote info", () => settingsApi.webdavSyncFetchRemoteInfo()],
    ["s3 test", () => settingsApi.s3TestConnection(s3)],
    ["s3 upload", () => settingsApi.s3SyncUpload()],
    ["s3 download", () => settingsApi.s3SyncDownload()],
    ["s3 save", () => settingsApi.s3SyncSaveSettings(s3, false)],
    ["s3 remote info", () => settingsApi.s3SyncFetchRemoteInfo()],
  ];

  for (const [name, call] of FAILING) {
    it(`${name}: {ok:false} throws the server text verbatim`, async () => {
      const text = `WebDAV GET failed: HTTP 401 Unauthorized (${name})`;
      fetchSpy.mockResolvedValue({
        ok: false,
        status: 500,
        json: async () => ({ ok: false, error: text }),
      });
      await expect(call()).rejects.toThrow(new Error(text));
    });
  }

  it("every one of the 10 functions is exercised by the success table", () => {
    const urls = new Set(CASES.map(([, , method, url]) => `${method} ${url}`));
    expect(urls).toEqual(
      new Set(
        ["webdav", "s3"].flatMap((b) => [
          `POST /api/sync/${b}/test`,
          `POST /api/sync/${b}/upload`,
          `POST /api/sync/${b}/download`,
          `PUT /api/sync/${b}/settings`,
          `GET /api/sync/${b}/remote-info`,
        ]),
      ),
    );
    expect(FAILING).toHaveLength(10);
  });
});
