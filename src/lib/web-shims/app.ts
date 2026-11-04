import { fetchCapabilities } from "../api/capabilities";

/**
 * Web-build shim for `@tauri-apps/api/app`. The version is the server's
 * (`/api/capabilities`); when the server cannot be reached it rejects, as the
 * real call does, and each caller shows "unknown" instead of a made-up number.
 */
export async function getVersion(): Promise<string> {
  return (await fetchCapabilities()).version;
}

export async function getName(): Promise<string> {
  return "cc-switch-web";
}
