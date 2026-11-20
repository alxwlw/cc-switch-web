import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { WebHostBanner } from "@/components/web/WebHostBanner";
import { setWeb } from "../helpers/web";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const CAPS = {
  mode: "web",
  version: "9.9.9",
  runtime_boundaries: "server host only",
};

function renderBanner() {
  const client = new QueryClient();
  return render(
    <QueryClientProvider client={client}>
      <WebHostBanner />
    </QueryClientProvider>,
  );
}

describe("WebHostBanner", () => {
  const fetchMock = vi.fn();

  beforeEach(() => {
    localStorage.clear();
    fetchMock
      .mockReset()
      .mockResolvedValue({ ok: true, json: async () => CAPS });
    vi.stubGlobal("fetch", fetchMock);
  });
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("desktop: renders nothing and makes no capabilities request", () => {
    renderBanner();
    expect(screen.queryByText("web.hostNotice")).toBeNull();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("web: shows the statement, with runtime_boundaries as secondary detail", async () => {
    setWeb(true);
    renderBanner();
    expect(screen.getByText("web.hostNotice")).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByRole("note")).toHaveAttribute(
        "title",
        "server host only",
      ),
    );
  });

  it("web: statement shows even when capabilities fail", async () => {
    setWeb(true);
    fetchMock.mockRejectedValue(new Error("down"));
    renderBanner();
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    expect(screen.getByText("web.hostNotice")).toBeInTheDocument();
  });

  it("web: dismissal is remembered across mounts", () => {
    setWeb(true);
    const first = renderBanner();
    fireEvent.click(screen.getByLabelText("web.hostNoticeDismiss"));
    expect(screen.queryByText("web.hostNotice")).toBeNull();
    first.unmount();
    renderBanner();
    expect(screen.queryByText("web.hostNotice")).toBeNull();
  });

  it("web: renders and dismisses when localStorage throws", () => {
    setWeb(true);
    const denied = () => {
      throw new Error("denied");
    };
    vi.stubGlobal("localStorage", { getItem: denied, setItem: denied });
    renderBanner();
    expect(screen.getByText("web.hostNotice")).toBeInTheDocument();
    const errors: unknown[] = [];
    const onError = (e: ErrorEvent) => {
      e.preventDefault();
      errors.push(e.error);
    };
    window.addEventListener("error", onError);
    fireEvent.click(screen.getByLabelText("web.hostNoticeDismiss"));
    window.removeEventListener("error", onError);
    expect(errors).toEqual([]);
    expect(screen.queryByText("web.hostNotice")).toBeNull();
  });
});
