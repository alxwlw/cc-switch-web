/** Web-build shim for `@tauri-apps/api/event`: no event bus in the browser. */
export type UnlistenFn = () => void;

/** Resolves immediately with a no-op unlisten — live refresh falls back to
 *  the UI's own refetching. */
// eslint-disable-next-line @typescript-eslint/no-unused-vars
export async function listen<T = unknown>(
  _event: string,
  _handler?: (event: { payload: T }) => void,
): Promise<UnlistenFn> {
  return () => {};
}

// eslint-disable-next-line @typescript-eslint/no-unused-vars
export async function once<T = unknown>(
  _event: string,
  _handler?: (event: { payload: T }) => void,
): Promise<UnlistenFn> {
  return () => {};
}
