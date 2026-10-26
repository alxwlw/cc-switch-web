import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import "@testing-library/jest-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("@/lib/toast", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));

import { RectifierConfigPanel } from "@/components/settings/RectifierConfigPanel";
import { LogConfigPanel } from "@/components/settings/LogConfigPanel";

const ok = (data: unknown) => ({
  status: 200,
  json: async () => ({ ok: true, data }),
});
const rectifier = {
  enabled: true,
  requestThinkingSignature: true,
  requestThinkingBudget: true,
  requestMediaFallback: true,
  requestMediaHeuristic: true,
};
const optimizer = {
  enabled: false,
  thinkingOptimizer: true,
  cacheInjection: true,
};

describe("config panels through the web adapter (WEB-24)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => vi.unstubAllGlobals());

  it("rectifier panel loads both configs and PUTs the changed rectifier config", async () => {
    fetchSpy.mockImplementation(async (url: string) =>
      url === "/api/settings/rectifier"
        ? ok(rectifier)
        : url === "/api/settings/optimizer"
          ? ok(optimizer)
          : ok(true),
    );
    render(<RectifierConfigPanel />);
    const sw = (await screen.findAllByRole("switch"))[0];
    await userEvent.click(sw);
    await waitFor(() =>
      expect(fetchSpy.mock.calls.some(([, i]) => i.method === "PUT")).toBe(
        true,
      ),
    );
    const put = fetchSpy.mock.calls.find(([, i]) => i.method === "PUT")!;
    expect(put[0]).toBe("/api/settings/rectifier");
    expect(JSON.parse(put[1].body)).toEqual({
      config: { ...rectifier, enabled: false },
    });
    expect(fetchSpy.mock.calls.map(([u]) => u)).toContain(
      "/api/settings/optimizer",
    );
  });

  it("log panel loads the level and PUTs the toggled config", async () => {
    fetchSpy.mockImplementation(async (_url: string, init: RequestInit) =>
      init.method === "PUT" ? ok(true) : ok({ enabled: true, level: "debug" }),
    );
    render(<LogConfigPanel />);
    const sw = await screen.findByRole("switch");
    await userEvent.click(sw);
    await waitFor(() =>
      expect(fetchSpy.mock.calls.some(([, i]) => i.method === "PUT")).toBe(
        true,
      ),
    );
    const put = fetchSpy.mock.calls.find(([, i]) => i.method === "PUT")!;
    expect(put[0]).toBe("/api/settings/log");
    expect(JSON.parse(put[1].body)).toEqual({
      config: { enabled: false, level: "debug" },
    });
  });
});
