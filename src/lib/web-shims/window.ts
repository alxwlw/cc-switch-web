/** Web-build shim for `@tauri-apps/api/window`: window controls are no-ops. */
export interface WebWindow {
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  close(): Promise<void>;
  setDecorations(_decorations: boolean): Promise<void>;
  onFocusChanged(
    _handler: (event: { payload: boolean }) => void,
  ): Promise<() => void>;
  listen<T>(
    _event: string,
    _handler?: (event: { payload: T }) => void,
  ): Promise<() => void>;
}

const noopWindow: WebWindow = {
  async minimize() {},
  async toggleMaximize() {},
  async close() {},
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  async setDecorations(_decorations: boolean) {},
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  async onFocusChanged(_handler: (event: { payload: boolean }) => void) {
    return () => {};
  },
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  async listen<T>(_event: string, _handler?: (event: { payload: T }) => void) {
    return () => {};
  },
};

export function getCurrentWindow(): WebWindow {
  return noopWindow;
}
