import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DeepLinkImportDialog } from "@/components/DeepLinkImportDialog";
import { DeepLinkPasteEntry } from "@/components/deeplink/DeepLinkPasteEntry";
import { deeplinkApi } from "@/lib/api/deeplink";
import { DEEPLINK_FILE_LIMIT_BYTES, extractDeepLink } from "@/lib/deeplinkOpen";
import { setWeb } from "../helpers/web";

vi.mock("@/components/ui/dialog", () => ({
  // Content is always rendered; the open flag is exposed for assertions.
  Dialog: ({
    children,
    open,
  }: {
    children: React.ReactNode;
    open?: boolean;
  }) => (
    <div data-testid="deeplink-dialog" data-open={String(!!open)}>
      {children}
    </div>
  ),
  DialogContent: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogHeader: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogTitle: ({ children }: { children: React.ReactNode }) => (
    <h1>{children}</h1>
  ),
  DialogDescription: ({ children }: { children: React.ReactNode }) => (
    <p>{children}</p>
  ),
  DialogFooter: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
}));

const LINK = "ccswitch://v1/import?resource=provider&app=claude&name=Pasted";
const request = {
  version: "v1",
  resource: "provider" as const,
  app: "claude" as const,
  name: "Pasted Provider",
  endpoint: "https://example.com/v1",
  apiKey: "public-test-key",
};

function mount(onOpened?: () => void) {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <DeepLinkPasteEntry onOpened={onOpened} />
      <DeepLinkImportDialog />
    </QueryClientProvider>,
  );
}

const isOpen = () =>
  screen.getByTestId("deeplink-dialog").getAttribute("data-open");

describe("DeepLinkPasteEntry (web)", () => {
  beforeEach(() => setWeb(true));
  afterEach(() => vi.restoreAllMocks());

  it("opens the import dialog with a preview of a pasted link", async () => {
    const parse = vi
      .spyOn(deeplinkApi, "parseDeeplink")
      .mockResolvedValue(request);
    const onOpened = vi.fn();
    mount(onOpened);
    const user = userEvent.setup();
    expect(isOpen()).toBe("false");
    await user.type(screen.getByRole("textbox"), `  ${LINK}\n`);
    await user.click(
      screen.getByRole("button", { name: "deeplink.paste.open" }),
    );
    await waitFor(() => expect(isOpen()).toBe("true"));
    expect(parse).toHaveBeenCalledWith(LINK);
    expect(await screen.findByText("Pasted Provider")).toBeInTheDocument();
    expect(onOpened).toHaveBeenCalledTimes(1);
  });

  it("opens the dialog from a .url file, taking the first ccswitch:// token", async () => {
    const parse = vi
      .spyOn(deeplinkApi, "parseDeeplink")
      .mockResolvedValue(request);
    mount();
    const file = new File(
      [`[InternetShortcut]\r\nURL=${LINK}\r\nIconIndex=0\r\n`],
      "link.url",
    );
    await userEvent
      .setup()
      .upload(screen.getByTestId("deeplink-file-input"), file);
    await waitFor(() => expect(isOpen()).toBe("true"));
    expect(parse).toHaveBeenCalledWith(LINK);
    expect(await screen.findByText("Pasted Provider")).toBeInTheDocument();
  });

  it("merges the config of a pasted link before showing it", async () => {
    const withConfig = { ...request, config: "e30=" };
    vi.spyOn(deeplinkApi, "parseDeeplink").mockResolvedValue(withConfig);
    const merge = vi
      .spyOn(deeplinkApi, "mergeDeeplinkConfig")
      .mockResolvedValue({ ...withConfig, name: "Merged Provider" });
    mount();
    const user = userEvent.setup();
    await user.type(screen.getByRole("textbox"), LINK);
    await user.click(
      screen.getByRole("button", { name: "deeplink.paste.open" }),
    );
    expect(await screen.findByText("Merged Provider")).toBeInTheDocument();
    expect(merge).toHaveBeenCalledWith(withConfig);
  });

  it("shows the parser error inline and opens no dialog", async () => {
    vi.spyOn(deeplinkApi, "parseDeeplink").mockRejectedValue(
      new Error("Unsupported deep link version: v9"),
    );
    mount();
    const user = userEvent.setup();
    await user.type(screen.getByRole("textbox"), LINK);
    await user.click(
      screen.getByRole("button", { name: "deeplink.paste.open" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Unsupported deep link version: v9",
    );
    expect(isOpen()).toBe("false");
  });

  it("rejects text without a ccswitch:// link without calling the backend", async () => {
    const parse = vi.spyOn(deeplinkApi, "parseDeeplink");
    mount();
    const user = userEvent.setup();
    await user.type(screen.getByRole("textbox"), "https://example.com");
    await user.click(
      screen.getByRole("button", { name: "deeplink.paste.open" }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "deeplink.paste.noLink",
    );
    expect(parse).not.toHaveBeenCalled();
    expect(isOpen()).toBe("false");
  });

  it("refuses an oversized file before reading it", async () => {
    const parse = vi.spyOn(deeplinkApi, "parseDeeplink");
    mount();
    const file = new File(["x"], "big.txt");
    Object.defineProperty(file, "size", {
      value: DEEPLINK_FILE_LIMIT_BYTES + 1,
    });
    const text = vi.spyOn(FileReader.prototype, "readAsText");
    await userEvent
      .setup({ applyAccept: false })
      .upload(screen.getByTestId("deeplink-file-input"), file);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "deeplink.paste.fileTooLarge",
    );
    expect(text).not.toHaveBeenCalled();
    expect(parse).not.toHaveBeenCalled();
  });

  it("renders nothing on desktop", () => {
    setWeb(false);
    mount();
    expect(screen.queryByRole("textbox")).toBeNull();
  });
});

describe("extractDeepLink", () => {
  it("takes the first token and trims line breaks", () => {
    expect(extractDeepLink(`a\r\nURL=${LINK}\r\nccswitch://v1/other\n`)).toBe(
      LINK,
    );
    expect(extractDeepLink("nothing here")).toBeNull();
  });
});
