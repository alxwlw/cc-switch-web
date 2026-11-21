import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@/lib/api/adapter";

const enc = new TextEncoder();

/** jsdom's Response has no streaming body: a minimal reader-only stand-in. */
const streamOf = (pieces: string[], failAfter?: Error) => {
  let i = 0;
  return {
    ok: true,
    status: 200,
    body: {
      getReader: () => ({
        read: async () => {
          if (i < pieces.length)
            return { done: false as const, value: enc.encode(pieces[i++]) };
          if (failAfter) throw failAfter;
          return { done: true as const, value: undefined };
        },
      }),
    },
  };
};

describe("adapter · MCP connectivity checks", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => vi.unstubAllGlobals());

  it("resolves with the batch id and feeds results to the channel", async () => {
    // The chunks split the second and third lines in the middle.
    fetchSpy.mockResolvedValue(
      streamOf([
        '{"batchId":"b-1"}\n{"serverI',
        'd":"a","status":"connected"}\n{"serverId":"b",',
        '"status":"failed"}\n',
      ]),
    );
    const received: unknown[] = [];
    const channel = { onmessage: (r: unknown) => received.push(r) };

    const id = await invoke<string>("start_mcp_connectivity_checks", {
      serverIds: ["a", "b"],
      onResult: channel,
    });

    expect(id).toBe("b-1");
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/mcp/connectivity");
    expect(init.method).toBe("POST");
    expect(init.body).toBe('{"serverIds":["a","b"]}');
    // Delivery continues after the promise resolved: wait for the stream to end.
    await vi.waitFor(() => expect(received).toHaveLength(2));
    expect(received).toEqual([
      { serverId: "a", status: "connected" },
      { serverId: "b", status: "failed" },
    ]);
  });

  it("rejects with the server error", async () => {
    fetchSpy.mockResolvedValue(
      new Response(JSON.stringify({ ok: false, error: "boom" }), {
        status: 500,
        headers: { "content-type": "application/json" },
      }),
    );
    await expect(
      invoke("start_mcp_connectivity_checks", {
        serverIds: ["a"],
        onResult: { onmessage: vi.fn() },
      }),
    ).rejects.toThrow("boom");
  });

  it("warns and stops delivering when the stream breaks after the batch id", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    fetchSpy.mockResolvedValue(
      streamOf(
        ['{"batchId":"b-2"}\n{"serverId":"a","status":"failed"}\n'],
        new Error("connection reset"),
      ),
    );
    const onmessage = vi.fn();
    const id = await invoke<string>("start_mcp_connectivity_checks", {
      serverIds: ["a"],
      onResult: { onmessage },
    });
    expect(id).toBe("b-2");
    await vi.waitFor(() => expect(warn).toHaveBeenCalledTimes(1));
    expect(onmessage.mock.calls).toEqual([
      [{ serverId: "a", status: "failed" }],
    ]);
    warn.mockRestore();
  });

  const start = () =>
    invoke("start_mcp_connectivity_checks", {
      serverIds: ["a"],
      onResult: { onmessage: vi.fn() },
    });

  it("rejects when the stream breaks before any line", async () => {
    fetchSpy.mockResolvedValue(streamOf([], new Error("connection reset")));
    await expect(start()).rejects.toThrow("connection reset");
  });

  it("rejects when the stream ends without a line", async () => {
    fetchSpy.mockResolvedValue(streamOf([]));
    await expect(start()).rejects.toThrow("empty stream");
  });

  it("rejects when the first line has no batch id", async () => {
    fetchSpy.mockResolvedValue(streamOf(["{}\n"]));
    await expect(start()).rejects.toThrow("no batch id");
  });

  it("cancel maps to the batch route", async () => {
    fetchSpy.mockResolvedValue(
      new Response(JSON.stringify({ ok: true, data: null }), { status: 200 }),
    );
    await invoke("cancel_mcp_connectivity_checks", { batchId: "x/y" });
    const [url, init] = fetchSpy.mock.calls[0];
    expect(url).toBe("/api/mcp/connectivity/x%2Fy/cancel");
    expect(init.method).toBe("POST");
  });
});
