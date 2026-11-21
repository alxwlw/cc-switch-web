/** Web-build shim for `@tauri-apps/api/core` (see vite.config.web.mts alias). */
export { invoke, WebUnsupportedError } from "../api/adapter";
export { isWeb } from "../api/adapter";

/** Real Tauri API exposes `isTauri()` as a function — keep that shape. */
export function isTauri(): boolean {
  return false;
}

/** Minimal `Channel` stub: construction and handler assignment work, but the
 *  stub itself never fires. Streaming commands that take a channel are handled
 *  in `adapter.ts` `invoke`, which reads an NDJSON response and calls
 *  `onmessage` itself (`stream_session_messages`, `start_mcp_connectivity_checks`);
 *  the others are unsupported in web mode. */
export class Channel<T = unknown> {
  onmessage: ((message: T) => void) | null = null;
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  constructor(..._args: unknown[]) {}
}

/**
 * Minimal `Resource` stub (mirrors `@tauri-apps/api/core` shape enough for
 * plugin classes like the updater's to extend it in the web build).
 */
export class Resource {
  readonly rid: number;
  constructor(rid = 0) {
    this.rid = rid;
  }
  async close(): Promise<void> {}
}
