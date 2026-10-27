import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// In web builds `@tauri-apps/api/core` is aliased to the HTTP adapter; mirror that here.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter = await import("@/lib/api/adapter");
  return { invoke: adapter.invoke };
});

const toastMocks = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }));
vi.mock("sonner", () => ({
  toast: {
    success: toastMocks.success,
    error: toastMocks.error,
    dismiss: vi.fn(),
  },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

import {
  PiPromptTemplates,
  PiSystemPromptFiles,
} from "@/components/prompts/PiNativePromptResources";
import { setWeb } from "../helpers/web";

const CONFLICT =
  "并发冲突: Pi prompt file changed outside CC Switch: /home/u/.pi/agent/APPEND_SYSTEM.md";

describe("Pi native prompt resources · web (WEB-17)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    setWeb(true);
    toastMocks.error.mockReset();
    fetchSpy = vi.fn();
    vi.stubGlobal("fetch", fetchSpy);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  const renderWithClient = (ui: React.ReactNode) =>
    render(
      <QueryClientProvider
        client={
          new QueryClient({
            defaultOptions: { queries: { retry: false } },
          })
        }
      >
        {ui}
      </QueryClientProvider>,
    );

  const json = (status: number, body: unknown) =>
    ({ status, json: async () => body }) as Response;

  it("a 409 from the server reaches the error toast verbatim", async () => {
    fetchSpy.mockImplementation(async (url: string, init?: RequestInit) => {
      if (init?.method === "PUT") {
        return json(409, { ok: false, error: CONFLICT });
      }
      const kind = String(url).endsWith("system_append");
      return json(200, {
        ok: true,
        data: {
          exists: kind,
          revision: kind ? "append-revision" : "missing",
          content: kind ? "append" : "",
        },
      });
    });
    renderWithClient(<PiSystemPromptFiles />);

    await screen.findByText("pi.prompts.configuredSize");
    fireEvent.click(
      screen.getByRole("button", { name: "common.edit APPEND_SYSTEM.md" }),
    );
    fireEvent.change(
      screen.getByPlaceholderText("pi.prompts.instructionPlaceholder"),
      { target: { value: "stale write" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() =>
      expect(toastMocks.error).toHaveBeenCalledWith(CONFLICT),
    );
    const put = fetchSpy.mock.calls.find(([, init]) => init?.method === "PUT")!;
    expect(String(put[0])).toBe("/api/pi/prompt-files/system_append");
    expect(JSON.parse(put[1].body)).toEqual({
      expectedRevision: "append-revision",
      content: "stale write",
    });
  });

  it("lists native templates from the server in web", async () => {
    fetchSpy.mockResolvedValue(
      json(200, {
        ok: true,
        data: [{ slug: "review", content: "body", revision: "r1" }],
      }),
    );
    renderWithClient(<PiPromptTemplates />);

    expect(await screen.findByText("/review")).toBeInTheDocument();
    expect(String(fetchSpy.mock.calls[0][0])).toBe("/api/pi/prompt-templates");
  });
});
