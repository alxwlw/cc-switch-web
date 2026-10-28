import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@/lib/api/adapter";

const ok = (data: unknown) => ({
  ok: true,
  status: 200,
  json: async () => ({ ok: true, data }),
});

describe("adapter · CLI tool lifecycle (WEB-25)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => vi.unstubAllGlobals());

  const lastCall = () => {
    const [url, init] = fetchSpy.mock.calls.at(-1)!;
    return { url, method: init.method, body: JSON.parse(init.body) };
  };

  const plan = { kind: "npm", path: "/usr/bin/claude" };
  const wsl = { claude: { wslShell: "bash", wslShellFlag: "-lc" } };

  it("run_tool_lifecycle_action posts tools, action, wslShellByTool and uninstallPlan", async () => {
    fetchSpy.mockResolvedValue(ok(null));
    await invoke("run_tool_lifecycle_action", {
      tools: ["claude", "codex"],
      action: "uninstall",
      wslShellByTool: wsl,
      uninstallPlan: plan,
    });
    expect(lastCall()).toEqual({
      url: "/api/tools/lifecycle",
      method: "POST",
      body: {
        tools: ["claude", "codex"],
        action: "uninstall",
        wslShellByTool: wsl,
        uninstallPlan: plan,
      },
    });
  });

  it("run_tool_lifecycle_action sends null for omitted optional fields", async () => {
    fetchSpy.mockResolvedValue(ok(null));
    await invoke("run_tool_lifecycle_action", {
      tools: ["gemini"],
      action: "install",
    });
    expect(lastCall().body).toEqual({
      tools: ["gemini"],
      action: "install",
      wslShellByTool: null,
      uninstallPlan: null,
    });
  });

  it("prepare_tool_uninstall posts tool and wslShellByTool", async () => {
    fetchSpy.mockResolvedValue(ok([]));
    await invoke("prepare_tool_uninstall", {
      tool: "claude",
      wslShellByTool: wsl,
    });
    expect(lastCall()).toEqual({
      url: "/api/tools/uninstall-preview",
      method: "POST",
      body: { tool: "claude", wslShellByTool: wsl },
    });
    await invoke("prepare_tool_uninstall", { tool: "codex" });
    expect(lastCall().body).toEqual({ tool: "codex", wslShellByTool: null });
  });

  it("probe_tool_installations posts the tool list", async () => {
    fetchSpy.mockResolvedValue(ok([]));
    await invoke("probe_tool_installations", { tools: ["claude", "opencode"] });
    expect(lastCall()).toEqual({
      url: "/api/tools/probe",
      method: "POST",
      body: { tools: ["claude", "opencode"] },
    });
  });

  it("list_tool_installations posts the tool list and returns the data", async () => {
    const reports = [{ tool: "claude" }];
    fetchSpy.mockResolvedValue(ok(reports));
    await expect(
      invoke("list_tool_installations", { tools: ["claude"] }),
    ).resolves.toEqual(reports);
    expect(lastCall()).toEqual({
      url: "/api/tools/installations",
      method: "POST",
      body: { tools: ["claude"] },
    });
  });

  it.each([
    "TOOL_ACTION_IN_PROGRESS",
    "TOOL_ACTION_TIMEOUT: tool_install exceeded 900 s",
  ])("passes the server error %s through verbatim", async (error) => {
    fetchSpy.mockResolvedValue({
      ok: false,
      status: 500,
      json: async () => ({ ok: false, error }),
    });
    await expect(
      invoke("run_tool_lifecycle_action", {
        tools: ["claude"],
        action: "update",
      }),
    ).rejects.toThrow(error);
  });
});
