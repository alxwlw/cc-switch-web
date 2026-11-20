import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { isWeb } from "@/lib/api/adapter";

export type UpdateChannel = "stable" | "beta";

export interface UpdateInfo {
  currentVersion: string;
  availableVersion: string;
  notes?: string;
  pubDate?: string;
}

/** 自动更新在后台下好、等用户确认安装的版本（后端 `app_update::ReadyUpdate`）。 */
export interface ReadyUpdate {
  version: string;
  notes?: string | null;
}

/** 后端 `app_update::AutoUpdateStatus`。 */
export interface AutoUpdateStatus {
  /** 这台机器能不能自动安装（便携版、Linux 非 AppImage 不行） */
  supported: boolean;
  /** 自动下载开关是否打开 */
  enabled: boolean;
  downloading: boolean;
  ready: ReadyUpdate | null;
  /** 有应用在用路由服务：重启会切断进行中的请求 */
  routingInUse: boolean;
}

/** 待安装版本变了的事件，载荷是 {@link ReadyUpdate}；null 表示下好的版本被撤回 */
export const UPDATE_READY_EVENT = "app-update-ready";

export interface CheckOptions {
  timeout?: number;
  channel?: UpdateChannel;
  /**
   * web: skip the server's one-hour cache. It spends the shared anonymous GitHub
   * quota (60 requests an hour), so only the manual button sets it.
   */
  force?: boolean;
}

export type WebUpdateStatus = "up-to-date" | "behind" | "unknown";

export interface WebUpdateCommit {
  sha: string;
  /** First line of the commit message. */
  message: string;
  date: string | null;
}

/** `release`: the build is compared with the latest release; `commit`: with the head of a branch. */
export type WebUpdateMode = "release" | "commit";

/** `GET /api/update/check` (`UpdateCheck` in `services/web_update.rs`). */
export interface WebUpdateCheck {
  status: WebUpdateStatus;
  /** A server that does not send it predates release mode: `commit`. */
  mode: WebUpdateMode;
  currentVersion: string;
  currentSha: string | null;
  latestSha: string | null;
  /** Commits the branch has that the build lacks; can exceed `commits.length`. */
  behindBy: number;
  /** The newest of them, newest first. */
  commits: WebUpdateCommit[];
  checkedAt: string;
  /** The branch the server compared against, and would pull on self-update. */
  branch: string;
  instructions: { systemd: string; docker: string };
  /** Release mode: the latest release's version (no `v`), its notes page and publish time. */
  latestVersion: string | null;
  releaseUrl: string | null;
  publishedAt: string | null;
}

export type CheckResult =
  | { status: "up-to-date"; web?: WebUpdateCheck }
  | { status: "unknown"; web: WebUpdateCheck }
  | { status: "available"; info: UpdateInfo; web?: WebUpdateCheck };

export async function getCurrentVersion(): Promise<string> {
  try {
    return await getVersion();
  } catch {
    return "";
  }
}

function fail(what: string): never {
  throw new Error(`update check: unexpected payload (${what})`);
}

function str(value: unknown, what: string): string {
  return typeof value === "string" ? value : fail(what);
}

function strOrNull(value: unknown, what: string): string | null {
  if (value === null || value === undefined) return null;
  return typeof value === "string" ? value : fail(what);
}

/** The server's answer is checked at the boundary, not trusted to be the shape it should be. */
export function parseWebUpdateCheck(raw: unknown): WebUpdateCheck {
  if (!raw || typeof raw !== "object") return fail("not an object");
  const r = raw as Record<string, unknown>;
  const status = r.status;
  if (status !== "up-to-date" && status !== "behind" && status !== "unknown") {
    return fail("status");
  }
  if (
    typeof r.behindBy !== "number" ||
    !Number.isInteger(r.behindBy) ||
    r.behindBy < 0
  ) {
    return fail("behindBy");
  }
  if (!Array.isArray(r.commits)) return fail("commits");
  const instr = r.instructions as Record<string, unknown> | null | undefined;
  if (!instr || typeof instr !== "object") return fail("instructions");
  const mode: WebUpdateMode = r.mode === "release" ? "release" : "commit";
  const latestSha = strOrNull(r.latestSha, "latestSha");
  const latestVersion = strOrNull(r.latestVersion, "latestVersion");
  if (status === "behind") {
    // A release has no commit to name; a commit check has no version.
    if (mode === "release" ? !latestVersion : !latestSha) {
      return fail(mode === "release" ? "latestVersion" : "latestSha");
    }
  }
  return {
    status,
    mode,
    currentVersion: str(r.currentVersion, "currentVersion"),
    currentSha: strOrNull(r.currentSha, "currentSha"),
    latestSha,
    behindBy: r.behindBy,
    commits: r.commits.map((c) => {
      const commit = (c ?? {}) as Record<string, unknown>;
      return {
        sha: str(commit.sha, "commit.sha"),
        message: str(commit.message, "commit.message"),
        date: strOrNull(commit.date, "commit.date"),
      };
    }),
    checkedAt: str(r.checkedAt, "checkedAt"),
    branch: str(r.branch, "branch"),
    instructions: {
      systemd: str(instr.systemd, "instructions.systemd"),
      docker: str(instr.docker, "instructions.docker"),
    },
    latestVersion,
    releaseUrl: strOrNull(r.releaseUrl, "releaseUrl"),
    publishedAt: strOrNull(r.publishedAt, "publishedAt"),
  };
}

/**
 * The adapter's fetch has no client deadline, so a server that accepts and never answers would
 * keep the check (and its button) busy for good; the desktop plugin takes the same `timeout`.
 */
function withTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(
      () =>
        reject(
          new Error(
            `update check: no answer from the server in ${ms / 1000} s`,
          ),
        ),
      ms,
    );
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (err) => {
        clearTimeout(timer);
        reject(err);
      },
    );
  });
}

async function checkWebUpdate(opts: CheckOptions): Promise<CheckResult> {
  // A failed check rejects with the server's own text (rate limit, network): it is
  // not turned into "up-to-date" here.
  const web = parseWebUpdateCheck(
    await withTimeout(
      invoke<unknown>("web_check_update", { force: opts.force === true }),
      opts.timeout ?? 30000,
    ),
  );
  if (web.status === "unknown") return { status: "unknown", web };
  if (web.status === "up-to-date") return { status: "up-to-date", web };
  if (web.mode === "release") {
    return {
      status: "available",
      web,
      info: {
        currentVersion: web.currentVersion,
        availableVersion: web.latestVersion as string,
        pubDate: web.publishedAt ?? undefined,
      },
    };
  }
  const latest = web.latestSha as string;
  return {
    status: "available",
    web,
    info: {
      currentVersion: web.currentVersion,
      availableVersion: `${web.currentVersion}+${latest.slice(0, 7)}`,
      pubDate: web.commits[0]?.date ?? undefined,
    },
  };
}

export async function checkForUpdate(
  opts: CheckOptions = {},
): Promise<CheckResult> {
  // Web: the signal is the build's commit against the fork's branch, asked of the
  // server. The Tauri updater plugin does not exist there.
  if (isWeb) return checkWebUpdate(opts);

  // 动态引入，避免在未安装插件时导致打包期问题
  const { check } = await import("@tauri-apps/plugin-updater");

  const currentVersion = await getCurrentVersion();
  const update = await check({ timeout: opts.timeout ?? 30000 } as any);

  if (!update) {
    return { status: "up-to-date" };
  }

  const info: UpdateInfo = {
    currentVersion,
    availableVersion: (update as any).version ?? "",
    notes: (update as any).notes,
    pubDate: (update as any).date,
  };

  return { status: "available", info };
}
