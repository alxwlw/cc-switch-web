export interface ServerCapabilities {
  mode: string;
  version: string;
  runtime_boundaries: string;
  /** `std::env::consts::OS` of the server: "linux" | "macos" | "windows". */
  platform: string;
}

/**
 * `GET /api/capabilities` returns a bare JSON object, not the `{ok,data}`
 * envelope `adapter.invoke` requires, so it gets its own fetch.
 */
export async function fetchCapabilities(): Promise<ServerCapabilities> {
  const res = await fetch("/api/capabilities");
  if (!res.ok) throw new Error(`capabilities: HTTP ${res.status}`);
  const body = (await res.json()) as Partial<ServerCapabilities> | null;
  if (!body || typeof body.version !== "string") {
    throw new Error("capabilities: unexpected payload");
  }
  return {
    mode: String(body.mode ?? ""),
    version: body.version,
    runtime_boundaries: String(body.runtime_boundaries ?? ""),
    platform: String(body.platform ?? ""),
  };
}
