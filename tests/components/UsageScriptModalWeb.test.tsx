import type { ReactNode } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { Provider } from "@/types";
import { setWeb } from "../helpers/web";

// Route invoke through the web adapter (not raw Tauri): the modal's
// test-script must hit the HTTP endpoint.
vi.mock("@tauri-apps/api/core", async () => {
  const adapter =
    await vi.importActual<typeof import("@/lib/api/adapter")>(
      "@/lib/api/adapter",
    );
  return { invoke: adapter.invoke };
});

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    isOpen,
    children,
    footer,
  }: {
    isOpen: boolean;
    children: ReactNode;
    footer?: ReactNode;
  }) =>
    isOpen ? (
      <div>
        <div>{children}</div>
        <div>{footer}</div>
      </div>
    ) : null,
}));

vi.mock("@/components/JsonEditor", () => ({
  default: ({ value }: { value: string }) => (
    <textarea data-testid="usage-script-code" value={value} readOnly />
  ),
}));

const ok = (data: unknown) => ({
  ok: true,
  status: 200,
  json: async () => ({ ok: true, data }),
});

const provider: Provider = {
  id: "sub2api-site",
  name: "Example Sub2API",
  settingsConfig: {
    env: {
      ANTHROPIC_BASE_URL: "https://api.example.com",
      ANTHROPIC_AUTH_TOKEN: "sk-test",
    },
  },
  meta: {
    usage_script: {
      enabled: true,
      language: "javascript",
      code: "({ request: { url: '' }, extractor: function () { return {}; } })",
    },
  },
};

describe("UsageScriptModal test-script over the web adapter (WEB-04)", () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    setWeb(true);
    fetchSpy = vi.fn().mockResolvedValue(
      ok({
        success: true,
        data: [
          {
            planName: "p",
            remaining: 10,
            total: 100,
            used: 90,
            unit: "USD",
          },
        ],
      }),
    );
    vi.stubGlobal("fetch", fetchSpy);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("posts the custom script to /api/:app/providers/:id/usage-script/test", async () => {
    const { default: Modal } = await import("@/components/UsageScriptModal");
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      <QueryClientProvider client={client}>
        <Modal
          provider={provider}
          appId="claude"
          isOpen
          onClose={() => {}}
          onSave={() => {}}
        />
      </QueryClientProvider>,
    );

    const testButton = screen.getByRole("button", {
      name: "usageScript.testScript",
    });
    // WEB-04 ungated: the button works in web (no WebUnavailableTip).
    expect(testButton).toBeEnabled();
    await userEvent.click(testButton);

    await waitFor(() => {
      const urls = fetchSpy.mock.calls.map((call) => call[0] as string);
      expect(urls).toContain(
        "/api/claude/providers/sub2api-site/usage-script/test",
      );
    });
    const [, init] = fetchSpy.mock.calls.find(
      ([url]) => url === "/api/claude/providers/sub2api-site/usage-script/test",
    )!;
    expect(init.method).toBe("POST");
    const body = JSON.parse(init.body as string);
    expect(body.scriptCode).toContain("request");
  });
});
