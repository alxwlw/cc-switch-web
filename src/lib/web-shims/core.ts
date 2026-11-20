/** Web-build shim for `@tauri-apps/api/core` (see vite.config.web.mts alias). */
export { invoke, WebUnsupportedError } from "../api/adapter";
export { isWeb } from "../api/adapter";

/** Real Tauri API exposes `isTauri()` as a function — keep that shape. */
export function isTauri(): boolean {
  return false;
}

/** Minimal `Channel` stub: construction and handler assignment work, but the
 *  streaming commands that need a real channel are unsupported in web mode. */
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
