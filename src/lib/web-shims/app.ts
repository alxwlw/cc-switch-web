/** Web-build shim for `@tauri-apps/api/app`. */
export async function getVersion(): Promise<string> {
  return "4.0.20-web";
}

export async function getName(): Promise<string> {
  return "cc-switch-web";
}
