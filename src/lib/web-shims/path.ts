/** Web-build shim for `@tauri-apps/api/path`: display paths only. */
export async function homeDir(): Promise<string> {
  return "~";
}

export async function join(...parts: string[]): Promise<string> {
  return parts.join("/").replace(/\/{2,}/g, "/");
}
