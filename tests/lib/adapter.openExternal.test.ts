import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockInstance } from "vitest";

import { invoke, WebUnsupportedError } from "@/lib/api/adapter";

describe("adapter invoke · open_external (WEB-30)", () => {
  let openSpy: MockInstance<typeof window.open>;
  let fetchSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    openSpy = vi.spyOn(window, "open").mockReturnValue({} as Window);
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("https-ссылка: window.open c noopener,noreferrer, результат true, без fetch", async () => {
    const result = await invoke<boolean>("open_external", {
      url: "https://example.com/releases",
    });
    expect(openSpy).toHaveBeenCalledWith(
      "https://example.com/releases",
      "_blank",
      "noopener,noreferrer",
    );
    expect(result).toBe(true);
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("http-ссылка тоже открывается", async () => {
    await invoke("open_external", { url: "http://10.0.0.5:3000/" });
    expect(openSpy).toHaveBeenCalledWith(
      "http://10.0.0.5:3000/",
      "_blank",
      "noopener,noreferrer",
    );
  });

  it.each([
    ["ftp://example.com/file"],
    ["javascript:alert(1)"],
    ["mailto:someone@example.com"],
    ["file:///etc/passwd"],
    ["не-url"],
    [""],
  ])("не-http(s) URL %s отклоняется без window.open", async (url) => {
    await expect(invoke("open_external", { url })).rejects.toThrow(Error);
    await expect(invoke("open_external", { url })).rejects.not.toBeInstanceOf(
      WebUnsupportedError,
    );
    expect(openSpy).not.toHaveBeenCalled();
  });

  it("отсутствующий/не-строковый url отклоняется", async () => {
    await expect(invoke("open_external", {})).rejects.toThrow(Error);
    await expect(invoke("open_external", { url: 42 })).rejects.toThrow(Error);
    expect(openSpy).not.toHaveBeenCalled();
  });

  it("браузер заблокировал всплывающее окно (window.open вернул null) — честная ошибка", async () => {
    openSpy.mockReturnValue(null);
    await expect(
      invoke("open_external", { url: "https://example.com" }),
    ).rejects.toThrow(/заблоки/i);
  });

  it("не ломает существующее поведение других команд (маппинг в fetch)", async () => {
    fetchSpy.mockResolvedValue({
      json: async () => ({ ok: true, data: [] }),
    } as Response);
    await invoke("check_env_conflicts", { app: "claude" });
    expect(fetchSpy).toHaveBeenCalledWith(
      "/api/env/conflicts?app=claude",
      expect.objectContaining({ method: "GET" }),
    );
    expect(openSpy).not.toHaveBeenCalled();
  });

  it("немаппинговая команда по-прежнему WebUnsupportedError", async () => {
    await expect(invoke("definitely_not_mapped_cmd")).rejects.toBeInstanceOf(
      WebUnsupportedError,
    );
  });
});
