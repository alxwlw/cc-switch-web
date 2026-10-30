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
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  const lastCall = () => {
    const [url, init] = fetchSpy.mock.calls.at(-1)!;
    return { url, method: init.method, body: JSON.parse(init.body) };
  };

  const plan = { kind: "npm", path: "/usr/bin/claude" };
  const wsl = { claude: { wslShell: "bash", wslShellFlag: "-lc" } };

  const jobRes = (jobId = "job-1") => ok({ jobId });
  const status = (state: string, error?: string) =>
    ok({ state, error: error ?? null });
  const startArgs = { tools: ["claude"], action: "install" };

  /** Runs the invoke with fake timers, attaching the rejection handler up front. */
  const runLifecycle = async (args: Record<string, unknown> = startArgs) => {
    const p = invoke("run_tool_lifecycle_action", args);
    const settled = p.then(
      (v) => ({ v }),
      (e) => ({ e }),
    );
    return { p, settled };
  };

  it("run_tool_lifecycle_action starts a job posting tools, action, wslShellByTool and uninstallPlan", async () => {
    vi.useFakeTimers();
    fetchSpy.mockResolvedValueOnce(jobRes()).mockResolvedValue(status("done"));
    const { settled } = await runLifecycle({
      tools: ["claude", "codex"],
      action: "uninstall",
      wslShellByTool: wsl,
      uninstallPlan: plan,
    });
    await vi.advanceTimersByTimeAsync(2000);
    await settled;
    const [url, init] = fetchSpy.mock.calls[0];
    expect({ url, method: init.method, body: JSON.parse(init.body) }).toEqual({
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
    vi.useFakeTimers();
    fetchSpy.mockResolvedValueOnce(jobRes()).mockResolvedValue(status("done"));
    const { settled } = await runLifecycle({
      tools: ["gemini"],
      action: "install",
    });
    await vi.advanceTimersByTimeAsync(2000);
    await settled;
    expect(JSON.parse(fetchSpy.mock.calls[0][1].body)).toEqual({
      tools: ["gemini"],
      action: "install",
      wslShellByTool: null,
      uninstallPlan: null,
    });
  });

  it("polls the job every 2 s through running, running, done and resolves undefined", async () => {
    vi.useFakeTimers();
    fetchSpy
      .mockResolvedValueOnce(jobRes("abc 1"))
      .mockResolvedValueOnce(status("running"))
      .mockResolvedValueOnce(status("running"))
      .mockResolvedValueOnce(status("done"));
    const { settled } = await runLifecycle();
    expect(fetchSpy).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1999);
    expect(fetchSpy).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(fetchSpy).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(4000);
    expect(await settled).toEqual({ v: undefined });
    expect(fetchSpy).toHaveBeenCalledTimes(4);
    const [url, init] = fetchSpy.mock.calls[3];
    expect({ url, method: init.method }).toEqual({
      url: "/api/tools/lifecycle/abc%201",
      method: "GET",
    });
  });

  it.each([
    "TOOL_ACTION_IN_PROGRESS",
    "TOOL_ACTION_TIMEOUT: tool_install exceeded 900 s",
    "boom",
  ])(
    "a failed job rejects with the server error %s verbatim",
    async (error) => {
      vi.useFakeTimers();
      fetchSpy
        .mockResolvedValueOnce(jobRes())
        .mockResolvedValueOnce(status("running"))
        .mockResolvedValueOnce(status("failed", error));
      const { settled } = await runLifecycle();
      await vi.advanceTimersByTimeAsync(4000);
      const r: any = await settled;
      expect(r.e).toBeInstanceOf(Error);
      expect(r.e.message).toBe(error);
    },
  );

  it("a fetch rejection while polling is retried and the job still resolves", async () => {
    vi.useFakeTimers();
    fetchSpy
      .mockResolvedValueOnce(jobRes())
      .mockRejectedValueOnce(new TypeError("Load failed"))
      .mockResolvedValueOnce({ ok: false, status: 502, json: async () => ({}) })
      .mockResolvedValueOnce(status("done"));
    const { settled } = await runLifecycle();
    await vi.advanceTimersByTimeAsync(6000);
    expect(await settled).toEqual({ v: undefined });
    expect(fetchSpy).toHaveBeenCalledTimes(4);
  });

  it("a 404 while polling rejects: the server lost the operation", async () => {
    vi.useFakeTimers();
    fetchSpy.mockResolvedValueOnce(jobRes()).mockResolvedValueOnce({
      ok: false,
      status: 404,
      json: async () => ({ ok: false, error: "unknown tool job: job-1" }),
    });
    const { settled } = await runLifecycle();
    await vi.advanceTimersByTimeAsync(2000);
    const r: any = await settled;
    expect(r.e.message).toMatch(/lost track of this operation/);
    expect(fetchSpy).toHaveBeenCalledTimes(2);
  });

  it("a refused start rejects with the server error verbatim and never polls", async () => {
    fetchSpy.mockResolvedValue({
      ok: false,
      status: 400,
      json: async () => ({ ok: false, error: "bad body" }),
    });
    await expect(
      invoke("run_tool_lifecycle_action", startArgs),
    ).rejects.toThrow("bad body");
    expect(fetchSpy).toHaveBeenCalledTimes(1);
  });

  it("gives up after 20 minutes of a job that stays running", async () => {
    vi.useFakeTimers();
    fetchSpy
      .mockResolvedValueOnce(jobRes())
      .mockResolvedValue(status("running"));
    const { settled } = await runLifecycle();
    await vi.advanceTimersByTimeAsync(20 * 60 * 1000 - 2000);
    const before = fetchSpy.mock.calls.length;
    await vi.advanceTimersByTimeAsync(2000);
    const r: any = await settled;
    expect(r.e.message).toMatch(/may still be running on the server/);
    expect(fetchSpy.mock.calls.length).toBe(before);
    expect(before).toBeGreaterThan(500);
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
});
