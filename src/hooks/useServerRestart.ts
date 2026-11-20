import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { settingsApi } from "@/lib/api";
import { reloadPage } from "@/lib/pageReload";

/** One probe a second; the whole wait ends after a minute (WEB-26). */
export const RESTART_POLL_INTERVAL_MS = 1_000;
export const RESTART_WAIT_MS = 60_000;
/** A probe that hangs (a half-closed socket during shutdown) must not stall the poll. */
const PROBE_TIMEOUT_MS = 3_000;

/**
 * Has the process that answers `/api/health` changed? `/api/health` has no `ok` envelope, so it
 * is read with a plain `fetch`. Yes: 200 with another `instanceId`, or 503 (the new process came
 * up in its init-error screen, which still needs a page load to show). No: the old process
 * (same id, it answers until its shutdown cleanup is done), a network error or anything else.
 */
async function processChanged(
  previousId: string,
  signal: AbortSignal,
): Promise<boolean> {
  try {
    const res = await fetch("/api/health", { cache: "no-store", signal });
    if (res.status === 503) return true;
    if (res.status !== 200) return false;
    const body = (await res.json()) as { instanceId?: unknown };
    return (
      typeof body?.instanceId === "string" && body.instanceId !== previousId
    );
  } catch {
    return false;
  }
}

/**
 * Polls until the process behind the server is another one. Resolves `true` then, `false` after
 * `RESTART_WAIT_MS` or when `stop` is called (the caller went away). Every timer and the probe in
 * flight end with it.
 */
function waitForNewProcess(
  previousId: string,
  setStop: (stop: () => void) => void,
): Promise<boolean> {
  return new Promise((resolve) => {
    let pollTimer: ReturnType<typeof setTimeout> | undefined;
    let probe: AbortController | undefined;
    let settled = false;

    const finish = (changed: boolean) => {
      if (settled) return;
      settled = true;
      clearTimeout(pollTimer);
      clearTimeout(deadline);
      probe?.abort();
      resolve(changed);
    };
    const deadline = setTimeout(() => finish(false), RESTART_WAIT_MS);
    setStop(() => finish(false));

    const poll = async () => {
      const controller = new AbortController();
      probe = controller;
      const probeTimer = setTimeout(() => controller.abort(), PROBE_TIMEOUT_MS);
      const changed = await processChanged(previousId, controller.signal);
      clearTimeout(probeTimer);
      if (settled) return;
      if (changed) finish(true);
      else pollTimer = setTimeout(() => void poll(), RESTART_POLL_INTERVAL_MS);
    };
    pollTimer = setTimeout(() => void poll(), RESTART_POLL_INTERVAL_MS);
  });
}

/**
 * Restart of the web server from the console (WEB-26): `POST /api/restart`, then wait for the new
 * process and reload the page. A second call while one is under way does nothing, so only one
 * poll ever runs; `restarting` blocks the button until the flow is over. Leaving the page stops
 * the timers.
 */
export function useServerRestart() {
  const { t } = useTranslation();
  const [restarting, setRestarting] = useState(false);
  const busyRef = useRef(false);
  const mountedRef = useRef(true);
  const stopRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      stopRef.current?.();
      stopRef.current = null;
    };
  }, []);

  /** Resolves `false` at once when a restart is already under way: that call did nothing. */
  const restart = useCallback(async (): Promise<boolean> => {
    if (busyRef.current) return false;
    busyRef.current = true;
    setRestarting(true);
    try {
      let instanceId: unknown;
      try {
        ({ instanceId } = await settingsApi.restartServer());
      } catch (error) {
        // 409 without a supervisor carries the server's own text; a network error is ours.
        console.error("[useServerRestart] Restart request failed", error);
        if (mountedRef.current) {
          toast.error(
            error instanceof Error && error.message
              ? error.message
              : t("settings.restartFailed"),
          );
        }
        return true;
      }
      // Without the id of the old process "another process answered" cannot be told apart.
      const changed =
        typeof instanceId === "string" &&
        mountedRef.current &&
        (await waitForNewProcess(instanceId, (stop) => {
          stopRef.current = stop;
        }));
      stopRef.current = null;
      if (!mountedRef.current) return true;
      if (changed) {
        reloadPage();
      } else {
        toast.error(t("web.dataDir.restartTimeout"), { duration: 10_000 });
      }
      return true;
    } finally {
      busyRef.current = false;
      if (mountedRef.current) setRestarting(false);
    }
  }, [t]);

  return { restart, restarting };
}
