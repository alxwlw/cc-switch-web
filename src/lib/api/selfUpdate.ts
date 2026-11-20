import { invoke } from "@tauri-apps/api/core";

/** `UpdateRunStatus` of `web_api/self_update.rs`: what `scripts/self-update.sh` last wrote. */
export interface UpdateRunStatus {
  stage: string;
  ok: boolean;
  message: string;
  fromSha: string | null;
  toSha: string | null;
  /** Release runs: the version before and after (a git run leaves them `null`). */
  fromVersion: string | null;
  toVersion: string | null;
  startedAt: string | null;
  finishedAt: string | null;
}

/*
 * Stages, as `scripts/self-update.sh` writes them. Progress: checking, fetching, merging,
 * downloading, verifying, building, installing, restarting, rolling-back. Final (`finishedAt` is
 * set): done, failed, rolled-back, rollback-failed. `useSelfUpdate` is where each one is acted on.
 */

/**
 * `POST /api/update/install` answered 202: the unit that runs the script and, in commit mode, the
 * branch it pulls and the repo; both are `null` in release mode.
 */
export interface InstallLaunch {
  unit: string;
  branch: string | null;
  repo: string | null;
}

/**
 * Rejects with the server's own text on a 409 (not enabled, no systemd, already running) and
 * on any other refusal; `adapter.invoke` carries it as the error message.
 */
export async function installWebUpdate(): Promise<InstallLaunch> {
  const data = await invoke<Partial<InstallLaunch> | null>(
    "web_install_update",
  );
  return {
    unit: typeof data?.unit === "string" ? data.unit : "",
    branch: typeof data?.branch === "string" ? data.branch : null,
    repo: typeof data?.repo === "string" ? data.repo : null,
  };
}

function strOrNull(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

/**
 * `GET /api/update/status`. A plain `fetch` with a signal, not `invoke`: the poll must be able to
 * cut a probe that hangs while the service restarts. Resolves `null` when no run has written a
 * status; rejects on a network error, a non-2xx answer or a payload that is not a status.
 */
export async function fetchUpdateStatus(
  signal?: AbortSignal,
): Promise<UpdateRunStatus | null> {
  const res = await fetch("/api/update/status", {
    method: "GET",
    cache: "no-store",
    signal,
  });
  if (!res.ok) throw new Error(`update status: HTTP ${res.status}`);
  const body = (await res.json()) as { ok?: unknown; data?: unknown } | null;
  if (!body || body.ok !== true) {
    throw new Error("update status: unexpected payload");
  }
  const raw = body.data;
  if (raw === null || raw === undefined) return null;
  if (typeof raw !== "object") {
    throw new Error("update status: unexpected payload");
  }
  const r = raw as Record<string, unknown>;
  if (typeof r.stage !== "string" || r.stage === "") {
    throw new Error("update status: unexpected payload");
  }
  return {
    stage: r.stage,
    ok: r.ok === true,
    message: typeof r.message === "string" ? r.message : "",
    fromSha: strOrNull(r.fromSha),
    toSha: strOrNull(r.toSha),
    fromVersion: strOrNull(r.fromVersion),
    toVersion: strOrNull(r.toVersion),
    startedAt: strOrNull(r.startedAt),
    finishedAt: strOrNull(r.finishedAt),
  };
}
