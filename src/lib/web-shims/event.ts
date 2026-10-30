/**
 * Web-build shim for `@tauri-apps/api/event`, backed by one shared
 * `EventSource("/api/events")` (SSE). Wire contract: one named SSE event per
 * bus event, `data` = payload JSON (`null` when none); no replay.
 *
 * Synthetic event `"resync"` means "state may have been missed, refetch
 * everything". Its listeners hear it when the server sends the frame (the
 * client lagged) and on every reopen after a dropped connection, never on the
 * first open. The invalidation itself lives with the subscriber.
 */
export type UnlistenFn = () => void;

export interface Event<T> {
  event: string;
  id: number;
  payload: T;
}
export type EventCallback<T> = (event: Event<T>) => void;

const EVENTS_URL = "/api/events";
const RESYNC = "resync";
const BACKOFF_MIN_MS = 1_000;
const BACKOFF_MAX_MS = 30_000;

type Entry = (event: Event<unknown>) => void;

const handlers = new Map<string, Set<Entry>>();
let source: EventSource | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let backoffMs = BACKOFF_MIN_MS;
let dropped = false; // the connection broke since the last successful open
let nextId = 1;

function dispatch(name: string, payload: unknown): void {
  const set = handlers.get(name);
  if (!set) return;
  // Snapshot: a handler may unlisten (once) while we iterate.
  for (const entry of [...set]) {
    try {
      entry({ event: name, id: nextId++, payload });
    } catch (e) {
      console.error(`[events] handler for "${name}" threw`, e);
    }
  }
}

function bindName(es: EventSource, name: string): void {
  es.addEventListener(name, (e) => {
    if (source !== es) return; // frame from a replaced connection
    let payload: unknown = null;
    const raw = (e as MessageEvent).data;
    try {
      payload = raw === undefined || raw === "" ? null : JSON.parse(raw);
    } catch (err) {
      console.error(`[events] bad payload for "${name}"`, err);
      return;
    }
    dispatch(name, payload);
  });
}

function clearTimer(): void {
  if (reconnectTimer !== null) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
}

function connect(): void {
  clearTimer();
  const es = new EventSource(EVENTS_URL);
  source = es;
  // A new EventSource has no listeners: re-bind every registered name.
  for (const name of handlers.keys()) bindName(es, name);
  es.onopen = () => {
    if (source !== es) return;
    backoffMs = BACKOFF_MIN_MS;
    if (dropped) {
      dropped = false;
      dispatch(RESYNC, null); // events emitted while down are lost
    }
  };
  es.onerror = () => {
    if (source !== es) return;
    dropped = true;
    // CONNECTING: the browser retries by itself. CLOSED (non-200, e.g. the
    // init-error 503): it never will, so retry with backoff.
    if (es.readyState === EventSource.CLOSED) {
      es.close();
      source = null;
      const delay = backoffMs;
      backoffMs = Math.min(backoffMs * 2, BACKOFF_MAX_MS);
      reconnectTimer = setTimeout(() => {
        reconnectTimer = null;
        if (handlers.size > 0 && source === null) connect();
      }, delay);
    }
  };
}

function disconnect(): void {
  clearTimer();
  source?.close();
  source = null;
  dropped = false;
  backoffMs = BACKOFF_MIN_MS;
}

function subscribe(name: string, entry: Entry): UnlistenFn {
  let set = handlers.get(name);
  const isNewName = !set;
  if (!set) {
    set = new Set();
    handlers.set(name, set);
  }
  set.add(entry);
  if (source) {
    if (isNewName) bindName(source, name);
  } else if (reconnectTimer === null) {
    connect();
  }
  return () => {
    const s = handlers.get(name);
    if (!s || !s.delete(entry)) return; // idempotent
    // Keep an empty set while connected: the EventSource listener for the
    // name stays bound and a later re-subscribe must not bind it twice.
    if (
      handlers.size > 0 &&
      [...handlers.values()].every((x) => x.size === 0)
    ) {
      handlers.clear();
      disconnect();
    }
  };
}

export async function listen<T = unknown>(
  event: string,
  handler: EventCallback<T>,
): Promise<UnlistenFn> {
  return subscribe(event, handler as Entry);
}

export async function once<T = unknown>(
  event: string,
  handler: EventCallback<T>,
): Promise<UnlistenFn> {
  const unlisten: UnlistenFn = subscribe(event, (e) => {
    unlisten();
    (handler as Entry)(e);
  });
  return unlisten;
}
