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
    "%s: null без fetch и без исключения",
    async (cmd) => {
      await expect(invoke(cmd, { theme: "dark" })).resolves.toBeNull();
      expect(fetchSpy).not.toHaveBeenCalled();
    },
  );

  it("контроль: прочие неподдержанные команды по-прежнему кидают WebUnsupportedError", async () => {
    await expect(invoke("launch_hermes_dashboard")).rejects.toBeInstanceOf(
      WebUnsupportedError,
    );
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});
