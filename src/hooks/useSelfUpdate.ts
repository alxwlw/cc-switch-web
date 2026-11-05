import { useCallback, useEffect, useRef, useState } from "react";
import { reloadPage } from "@/lib/pageReload";
import {
  fetchUpdateStatus,
  installWebUpdate,
  type UpdateRunStatus,
} from "@/lib/api/selfUpdate";

/** SSE does not survive the restart, so the run is read from `GET /api/update/status` (WEB-28). */
export const SELF_UPDATE_POLL_INTERVAL_MS = 2_000;
/** A probe that hangs (a half-closed socket while the service restarts) must not stall the poll. */
export const SELF_UPDATE_PROBE_TIMEOUT_MS = 5_000;
/**
 * How long the status may stay the same before the wait is given up, counted from the last
 * change of stage. A release build on the agentbox takes 2-3 minutes with a warm cargo cache and
 * longer from scratch (pnpm install + web build + cargo), and `building` is one stage: 15 minutes
 * leaves a cold build room, yet a unit that hung does not keep the page waiting for good. Network
 * errors and 5xx answers do not reset it: they are what a restart looks like.
 */
export const SELF_UPDATE_STAGE_WAIT_MS = 15 * 60_000;
/**
 * How long a `null` status is waited out after the launch. The server clears the status file
 * before it starts the unit and the script writes its first stage at once, so a status that is
 * still `null` after this long means the unit died before writing it.
 */
export const SELF_UPDATE_FIRST_STATUS_WAIT_MS = 60_000;

/** Where the operator reads the build log; the script's `message` names the same command. */
export const SELF_UPDATE_JOURNAL_COMMAND =
  "journalctl --user -u cc-switch-web-update";

export type SelfUpdateState =
  | { kind: "idle" }
  /** The POST is in flight. */
  | { kind: "starting" }
  /**
   * `stage` is `null` until the script writes its first one. `waiting`: the last probe got no
   * usable answer (the service is restarting).
   */
  | {
      kind: "running";
      stage: string | null;
      branch: string | null;
      waiting: boolean;
    }
  /** `done`: the page is being reloaded onto the new build. */
  | { kind: "reloading" }
  /**
   * A run that ended without a new build. `unchanged`: the script found nothing to merge
   * (`done` with the same commit before and after), so there is nothing to reload.
   */
  | {
      kind: "ended";
      outcome: "failed" | "rolled-back" | "rollback-failed" | "unchanged";
      message: string;
      /** When the script finished, as it wrote it (`null` if it did not say). */
      finishedAt: string | null;
    }
  /** The wait was given up: no stage change in `SELF_UPDATE_STAGE_WAIT_MS`, or no status at all. */
  | { kind: "stuck"; reason: "stalled" | "no-status" }
  /** The server refused the launch (409 with its reason) or could not be reached. */
  | { kind: "refused"; message: string };

type Running = Extract<SelfUpdateState, { kind: "running" }>;

/**
 * Stages that only a run which rebuilds goes through. A `done` with the same commit before and
 * after, after one of these, is a rebuild at a checkout that already was at the target (the
 * retry after a `rolled-back`): the running build is older, so the page must reload. Without
 * them the script found nothing to merge and the build is current.
 */
const REBUILD_STAGES: readonly string[] = [
  "building",
  "installing",
  "restarting",
];

interface SelfUpdateOptions {
  /**
   * Called when a run ends with nothing to update (`done`, same commit, no rebuild seen): the
   * server's cached check may still say `behind` for up to an hour, so the caller asks again.
   */
  onNothingToUpdate?: () => void;
}

/**
 * Self-update of the web server from the console (WEB-28): `POST /api/update/install`, then
 * `GET /api/update/status` every 2 s until a final stage; `done` reloads the page.
 *
 * Only a run this tab started, or one that was under way when the tab opened, is followed. A
 * final status found on disk when the tab opens is not an event: a `done` is not shown and
 * never reloads the page; a `failed`, `rolled-back` or `rollback-failed` is shown with its message
 * and time (the operator who left Settings during the run learns what happened), still without a
 * reload or a poll. A new launch replaces it.
 * Leaving the page stops every timer and the probe in flight.
 */
export function useSelfUpdate(
  enabled: boolean,
  options: SelfUpdateOptions = {},
) {
  const [state, setState] = useState<SelfUpdateState>({ kind: "idle" });
  const mountedRef = useRef(true);
  const busyRef = useRef(false);
  const stopRef = useRef<(() => void) | null>(null);
  const onNothingRef = useRef(options.onNothingToUpdate);
  onNothingRef.current = options.onNothingToUpdate;

  const track = useCallback(
    (initial: { stage: string | null; branch: string | null }) => {
      stopRef.current?.();

      let settled = false;
      let pollTimer: ReturnType<typeof setTimeout> | undefined;
      let stallTimer: ReturnType<typeof setTimeout> | undefined;
      let nullTimer: ReturnType<typeof setTimeout> | undefined;
      let probe: AbortController | undefined;
      let probeTimer: ReturnType<typeof setTimeout> | undefined;
      let view: Running = {
        kind: "running",
        stage: initial.stage,
        branch: initial.branch,
        waiting: false,
      };
      let lastKey: string | null = null;
      let rebuilt =
        initial.stage !== null && REBUILD_STAGES.includes(initial.stage);

      const stop = () => {
        settled = true;
        clearTimeout(pollTimer);
        clearTimeout(stallTimer);
        clearTimeout(nullTimer);
        clearTimeout(probeTimer);
        probe?.abort();
        if (stopRef.current === stop) stopRef.current = null;
      };
      const end = (next: SelfUpdateState) => {
        if (settled) return;
        stop();
        if (mountedRef.current) setState(next);
      };
      const show = (patch: Partial<Running>) => {
        const next = { ...view, ...patch };
        const same =
          next.stage === view.stage &&
          next.waiting === view.waiting &&
          next.branch === view.branch;
        view = next;
        if (!same && mountedRef.current) setState(view);
      };
      const armStall = () => {
        clearTimeout(stallTimer);
        stallTimer = setTimeout(
          () => end({ kind: "stuck", reason: "stalled" }),
          SELF_UPDATE_STAGE_WAIT_MS,
        );
      };
      const armNull = () => {
        clearTimeout(nullTimer);
        nullTimer = setTimeout(
          () => end({ kind: "stuck", reason: "no-status" }),
          SELF_UPDATE_FIRST_STATUS_WAIT_MS,
        );
      };

      const onStatus = (status: UpdateRunStatus) => {
        clearTimeout(nullTimer);
        nullTimer = undefined;
        const key = `${status.stage}|${status.startedAt ?? ""}`;
        if (key !== lastKey) {
          lastKey = key;
          armStall();
        }
        switch (status.stage) {
          case "done":
            if (
              !rebuilt &&
              status.fromSha !== null &&
              status.fromSha === status.toSha
            ) {
              end({
                kind: "ended",
                outcome: "unchanged",
                message: status.message,
                finishedAt: status.finishedAt,
              });
              if (mountedRef.current) onNothingRef.current?.();
            } else {
              end({ kind: "reloading" });
              reloadPage();
            }
            return;
          case "failed":
          case "rolled-back":
          case "rollback-failed":
            end({
              kind: "ended",
              outcome: status.stage,
              message: status.message,
              finishedAt: status.finishedAt,
            });
            return;
          default:
            // Known progress stages and any the script may gain later.
            if (REBUILD_STAGES.includes(status.stage)) rebuilt = true;
            show({ stage: status.stage, waiting: false });
        }
      };

      const poll = async () => {
        const controller = new AbortController();
        probe = controller;
        probeTimer = setTimeout(
          () => controller.abort(),
          SELF_UPDATE_PROBE_TIMEOUT_MS,
        );
        let status: UpdateRunStatus | null = null;
        let reached = true;
        try {
          status = await fetchUpdateStatus(controller.signal);
        } catch {
          reached = false;
        }
        clearTimeout(probeTimer);
        if (settled) return;
        if (!reached) {
          show({ waiting: true });
        } else if (status === null) {
          show({ waiting: false });
          if (nullTimer === undefined) armNull();
        } else {
          onStatus(status);
        }
        if (!settled) {
          pollTimer = setTimeout(
            () => void poll(),
            SELF_UPDATE_POLL_INTERVAL_MS,
          );
        }
      };

      stopRef.current = stop;
      armStall();
      if (initial.stage === null) armNull();
      setState(view);
      pollTimer = setTimeout(() => void poll(), SELF_UPDATE_POLL_INTERVAL_MS);
    },
    [],
  );

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      stopRef.current?.();
      stopRef.current = null;
    };
  }, []);

  // A page opened while an update is under way picks its progress up.
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    const controller = new AbortController();
    const timer = setTimeout(
      () => controller.abort(),
      SELF_UPDATE_PROBE_TIMEOUT_MS,
    );
    fetchUpdateStatus(controller.signal)
      .then((status) => {
        if (cancelled || !status) return;
        if (busyRef.current || stopRef.current) return;
        switch (status.stage) {
          case "done":
            return;
          case "failed":
          case "rolled-back":
          case "rollback-failed":
            setState({
              kind: "ended",
              outcome: status.stage,
              message: status.message,
              finishedAt: status.finishedAt,
            });
            return;
          default:
            track({ stage: status.stage, branch: null });
        }
      })
      .catch(() => {
        // Nothing to adopt: the server may be restarting under someone else's run.
      })
      .finally(() => clearTimeout(timer));
    return () => {
      cancelled = true;
      clearTimeout(timer);
      controller.abort();
    };
  }, [enabled, track]);

  /** Does nothing while a launch or a followed run is under way. */
  const start = useCallback(async (): Promise<void> => {
    if (busyRef.current || stopRef.current) return;
    busyRef.current = true;
    setState({ kind: "starting" });
    try {
      const launch = await installWebUpdate();
      if (!mountedRef.current) return;
      track({ stage: null, branch: launch.branch || null });
    } catch (error) {
      console.error("[useSelfUpdate] Install request failed", error);
      if (mountedRef.current) {
        setState({
          kind: "refused",
          message: error instanceof Error ? error.message : String(error),
        });
      }
    } finally {
      busyRef.current = false;
    }
  }, [track]);

  return { state, start };
}
