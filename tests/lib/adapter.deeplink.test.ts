import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@/lib/api/adapter";

describe("adapter · deep-link routes (WEB-27)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  const ok = (data: unknown) =>
    new Response(JSON.stringify({ ok: true, data }), { status: 200 });

  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => vi.unstubAllGlobals());

  it.each([
    ["parse_deeplink", { url: "ccswitch://x" }, "/api/deeplink/parse"],
    [
      "merge_deeplink_config",
      { request: { version: "v1", resource: "provider" } },
      "/api/deeplink/merge",
    ],
    [
      "fetch_deeplink_icon",
      { url: "https://i.example/a.png" },
      "/api/deeplink/icon",
    ],
    [
      "import_from_deeplink_unified",
      { request: { version: "v1", resource: "prompt" } },
      "/api/deeplink/import",
    ],
  ])("%s → POST %s with the same argument names", async (cmd, args, url) => {
    fetchSpy.mockResolvedValue(ok(null));
    await invoke(cmd, args);
    const [calledUrl, init] = fetchSpy.mock.calls[0];
    expect(calledUrl).toBe(url);
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body)).toEqual(args);
  });

  it("peek/ack of the pending queue stay silent no-ops", async () => {
    await expect(invoke("peek_pending_deeplink")).resolves.toBeNull();
    await expect(invoke("ack_pending_deeplink", { id: 1 })).resolves.toBeNull();
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});
