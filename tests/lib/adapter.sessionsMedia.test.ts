import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@/lib/api/adapter";

const enc = new TextEncoder();

/** jsdom's Response has no streaming body: a minimal reader-only stand-in. */
const streamOf = (pieces: Uint8Array[]) => {
  let i = 0;
  return {
    ok: true,
    status: 200,
    body: {
      getReader: () => ({
        read: async () =>
          i < pieces.length
            ? { done: false as const, value: pieces[i++] }
            : { done: true as const, value: undefined },
      }),
    },
  };
};

const readBlob = (blob: Blob) =>
  new Promise<string>((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(String(r.result));
    r.onerror = () => reject(r.error);
    r.readAsText(blob);
  });

describe("adapter · session media (WEB-13)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => vi.unstubAllGlobals());

  describe("get_session_image", () => {
    const image = {
      source: { kind: "inline", data: "AAAA" },
      mediaType: "image/png",
    };

    it("POSTs the ref and resolves with the raw bytes", async () => {
      fetchSpy.mockResolvedValue(new Response(new Uint8Array([1, 2, 3, 250])));
      const out = await invoke<ArrayBuffer>("get_session_image", {
        providerId: "claude",
        sourcePath: "/s/a.jsonl",
        image,
      });
      const [url, init] = fetchSpy.mock.calls[0];
      expect(url).toBe("/api/sessions/image");
      expect(init.method).toBe("POST");
      expect(JSON.parse(init.body)).toEqual({
        providerId: "claude",
        sourcePath: "/s/a.jsonl",
        image,
      });
      expect([...new Uint8Array(out)]).toEqual([1, 2, 3, 250]);
    });

    it("throws the server's JSON error text verbatim", async () => {
      fetchSpy.mockResolvedValue(
        new Response(
          JSON.stringify({ ok: false, error: "path not allowed: /etc" }),
          {
            status: 400,
          },
        ),
      );
      await expect(
        invoke("get_session_image", {
          providerId: "p",
          sourcePath: "s",
          image,
        }),
      ).rejects.toThrow("path not allowed: /etc");
    });

    it("a non-JSON failure body gives a generic error", async () => {
      fetchSpy.mockResolvedValue(new Response("<html>", { status: 502 }));
      await expect(
        invoke("get_session_image", {
          providerId: "p",
          sourcePath: "s",
          image,
        }),
      ).rejects.toThrow(/502/);
    });
  });

  describe("stream_session_messages", () => {
    const lines = [
      { type: "header", total: 1, turns: [], cached: false, parseMs: 1 },
      {
        type: "messages",
        start: 0,
        messages: [{ role: "user", content: "héllo" }],
      },
      { type: "done", payloadBytes: 9 },
    ];
    const ndjson = lines.map((l) => JSON.stringify(l)).join("\n") + "\n";

    it("GETs the stream route and delivers chunks split across arbitrary byte boundaries", async () => {
      const bytes = enc.encode(ndjson);
      // cut inside a multi-byte char and inside a line
      const cut = ndjson.indexOf("é");
      const at = enc.encode(ndjson.slice(0, cut)).length + 1;
      fetchSpy.mockResolvedValue(
        streamOf([bytes.slice(0, 7), bytes.slice(7, at), bytes.slice(at)]),
      );
      const got: unknown[] = [];
      await invoke("stream_session_messages", {
        providerId: "claude",
        sourcePath: "/s/a b.jsonl",
        onChunk: { onmessage: (c: unknown) => got.push(c) },
      });
      expect(fetchSpy.mock.calls[0][0]).toBe(
        "/api/sessions/stream?providerId=claude&sourcePath=%2Fs%2Fa%20b.jsonl",
      );
      expect(got).toEqual(lines);
    });

    it("delivers a final line without trailing newline", async () => {
      fetchSpy.mockResolvedValue(
        streamOf([enc.encode(JSON.stringify(lines[2]))]),
      );
      const got: unknown[] = [];
      await invoke("stream_session_messages", {
        providerId: "p",
        sourcePath: "s",
        onChunk: { onmessage: (c: unknown) => got.push(c) },
      });
      expect(got).toEqual([lines[2]]);
    });

    it("an error chunk is delivered, then thrown", async () => {
      const err = { type: "error", message: "boom: bad file" };
      fetchSpy.mockResolvedValue(
        streamOf([enc.encode(JSON.stringify(err) + "\n")]),
      );
      const got: unknown[] = [];
      await expect(
        invoke("stream_session_messages", {
          providerId: "p",
          sourcePath: "s",
          onChunk: { onmessage: (c: unknown) => got.push(c) },
        }),
      ).rejects.toThrow("boom: bad file");
      expect(got).toEqual([err]);
    });

    it("a failure before any chunk throws without calling onmessage (fallback path)", async () => {
      const onmessage = vi.fn();
      fetchSpy.mockResolvedValueOnce(new Response("no", { status: 500 }));
      await expect(
        invoke("stream_session_messages", {
          providerId: "p",
          sourcePath: "s",
          onChunk: { onmessage },
        }),
      ).rejects.toThrow();
      fetchSpy.mockRejectedValueOnce(new TypeError("offline"));
      await expect(
        invoke("stream_session_messages", {
          providerId: "p",
          sourcePath: "s",
          onChunk: { onmessage },
        }),
      ).rejects.toThrow(/offline/);
      expect(onmessage).not.toHaveBeenCalled();
    });

    it("a 500 with a valid JSON error line is not streamed (status checked, not just the body)", async () => {
      const onmessage = vi.fn();
      fetchSpy.mockResolvedValueOnce({
        ...streamOf([enc.encode('{"ok":false,"error":"x"}\n')]),
        ok: false,
        status: 500,
      });
      await expect(
        invoke("stream_session_messages", {
          providerId: "p",
          sourcePath: "s",
          onChunk: { onmessage },
        }),
      ).rejects.toThrow(/500/);
      expect(onmessage).not.toHaveBeenCalled();
    });
  });

  describe("export_session_markdown", () => {
    afterEach(() => {
      delete (URL as unknown as Record<string, unknown>).createObjectURL;
      delete (URL as unknown as Record<string, unknown>).revokeObjectURL;
      vi.restoreAllMocks();
    });

    it("downloads a markdown Blob under defaultName, returns it, makes no request", async () => {
      let blob: Blob | undefined;
      URL.createObjectURL = vi.fn((b: Blob) => ((blob = b), "blob:md"));
      URL.revokeObjectURL = vi.fn();
      const names: string[] = [];
      vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(
        function (this: HTMLAnchorElement) {
          names.push(this.download);
        },
      );
      const out = await invoke<string>("export_session_markdown", {
        defaultName: "my chat.md",
        content: "# Hi\n",
      });
      expect(out).toBe("my chat.md");
      expect(names).toEqual(["my chat.md"]);
      expect(blob!.type).toBe("text/markdown;charset=utf-8");
      expect(await readBlob(blob!)).toBe("# Hi\n");
      expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:md");
      expect(fetchSpy).not.toHaveBeenCalled();
    });
  });

  it("reveal_session_path and launch_session_terminal stay unmapped", async () => {
    await expect(
      invoke("reveal_session_path", { path: "/x" }),
    ).rejects.toThrow();
    await expect(invoke("launch_session_terminal", {})).rejects.toThrow();
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});
