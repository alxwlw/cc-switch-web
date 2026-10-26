import type { QueryClient, QueryKey } from "@tanstack/react-query";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { proxyKeys } from "./proxy";
import { subscriptionKeys } from "./subscription";
import { usageKeys } from "./usage";

/**
 * WEB-29: server event -> query keys to invalidate. One place; later web
 * tasks (WEB-01/04/06/11/13) add rows here.
 *
 * Only events nobody else handles are mapped. Already owned elsewhere (do not
 * duplicate here):
 *   provider-switched        App.tsx: ["providers", app] (+ pi caches)
 *   universal-provider-synced App.tsx: ["providers"]
 *   profile-applied          App.tsx: profiles, mcp, skills, proxy, claude-desktop
 *   webdav-/s3-sync-status-updated App.tsx: ["settings"] (+ toast)
 *   usage-cache-updated      useUsageCacheBridge: setQueryData
 *   deeplink-pending         DeepLinkImportDialog (no config query exists)
 *
 * `resync` (server lag frame, or the shim reopening after a drop) means
 * "events were missed": invalidate everything.
 */
type Payload = Record<string, unknown> | null | undefined;
type KeysFor = (payload: Payload) => QueryKey[];

const appKeys = (appType: unknown): QueryKey[] =>
  typeof appType === "string"
    ? [
        ["autoFailoverEnabled", appType],
        ["failoverQueue", appType],
        ["availableProvidersForFailover", appType],
        subscriptionKeys.quota(appType as never),
      ]
    : [["autoFailoverEnabled"], ["failoverQueue"]];

export const EVENT_INVALIDATIONS: Record<string, KeysFor> = {
  "provider-switched": (p) => [
    ...appKeys(p?.appType),
    proxyKeys.status,
    proxyKeys.takeoverStatus,
  ],
  "proxy-flags-changed": () => [
    proxyKeys.status,
    proxyKeys.takeoverStatus,
    ["autoFailoverEnabled"],
  ],
  "usage-log-recorded": () => [usageKeys.all],
};

export const RESYNC_EVENT = "resync";

// cancelRefetch:false joins an in-flight fetch instead of restarting it, so
// overlapping handlers (App.tsx, UsageDashboard) and explicit mutation
// refetches do not cancel each other.
const OPTS = { cancelRefetch: false } as const;

/**
 * Subscribe once at the app root (web build only). The subscription is never
 * released in practice, so the event shim keeps its single EventSource open
 * for the app's lifetime regardless of component mount/unmount churn.
 */
export async function subscribeEventInvalidation(
  queryClient: QueryClient,
): Promise<UnlistenFn> {
  const offs: UnlistenFn[] = [];
  for (const [name, keysFor] of Object.entries(EVENT_INVALIDATIONS)) {
    offs.push(
      await listen<Payload>(name, (e) => {
        for (const queryKey of keysFor(e.payload)) {
          void queryClient.invalidateQueries({ queryKey }, OPTS);
        }
      }),
    );
  }
  offs.push(
    await listen(RESYNC_EVENT, () => {
      void queryClient.invalidateQueries(undefined, OPTS);
    }),
  );
  return () => offs.forEach((off) => off());
}
