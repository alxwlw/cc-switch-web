import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { invoke, WebUnsupportedError } from "@/lib/api/adapter";

describe("adapter invoke · desktop-only fire-and-forget (WEB-00)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it.each(["update_tray_menu", "set_window_theme"])(
    "%s: null without fetch and without an exception",
    async (cmd) => {
      await expect(invoke(cmd, { theme: "dark" })).resolves.toBeNull();
      expect(fetchSpy).not.toHaveBeenCalled();
    },
  );

  it("is_portable_mode: false without fetch (the server is not portable)", async () => {
    await expect(invoke("is_portable_mode")).resolves.toBe(false);
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("control: other unsupported commands still throw WebUnsupportedError", async () => {
    await expect(invoke("launch_hermes_dashboard")).rejects.toBeInstanceOf(
      WebUnsupportedError,
    );
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});
