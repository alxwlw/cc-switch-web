import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  BROWSER_COMMANDS,
  invoke,
  isWebHandled,
  WebUnsupportedError,
} from "@/lib/api/adapter";

describe("adapter invoke · browser-side commands", () => {
  beforeEach(() => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(new Response("null", { status: 200 })),
    );
    vi.spyOn(window, "open").mockReturnValue({} as Window);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("lists the browser-side commands", () => {
    expect([...BROWSER_COMMANDS].sort()).toEqual([
      "copy_text_to_clipboard",
      "export_session_markdown",
      "is_portable_mode",
      "open_external",
      "run_tool_lifecycle_action",
      "start_mcp_connectivity_checks",
      "stream_session_messages",
    ]);
  });

  it("invoke dispatches every one of them, none falls through to WebUnsupportedError", async () => {
    for (const cmd of BROWSER_COMMANDS) {
      expect(isWebHandled(cmd)).toBe(true);
      // The handlers may reject on empty args; only the missing branch is a failure here.
      const outcome = await invoke(cmd).then(
        () => null,
        (error: unknown) => error,
      );
      expect(outcome, cmd).not.toBeInstanceOf(WebUnsupportedError);
    }
  });
});
