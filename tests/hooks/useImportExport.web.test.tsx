import { renderHook, act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useImportExport } from "@/hooks/useImportExport";
import { setWeb } from "../helpers/web";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const toastWarningMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    warning: (...args: unknown[]) => toastWarningMock(...args),
  },
}));

const exportForDownloadMock = vi.fn();
const importFromTextMock = vi.fn();
const importFromFileMock = vi.fn();
const openFileDialogMock = vi.fn();
const saveFileDialogMock = vi.fn();
const syncLiveMock = vi.fn();

vi.mock("@/lib/api", () => ({
  settingsApi: {
    exportConfigForDownload: (...a: unknown[]) => exportForDownloadMock(...a),
    importConfigFromText: (...a: unknown[]) => importFromTextMock(...a),
    importConfigFromFile: (...a: unknown[]) => importFromFileMock(...a),
    openFileDialog: (...a: unknown[]) => openFileDialogMock(...a),
    saveFileDialog: (...a: unknown[]) => saveFileDialogMock(...a),
    syncCurrentProvidersLive: (...a: unknown[]) => syncLiveMock(...a),
  },
}));

/** Make the next `<input type=file>.click()` "choose" the given file (or cancel). */
const pickers: { type: string; accept: string }[] = [];

function pickNext(file: unknown | null) {
  pickers.length = 0;
  return vi
    .spyOn(HTMLInputElement.prototype, "click")
    .mockImplementation(function (this: HTMLInputElement) {
      pickers.push({ type: this.type, accept: this.accept });
      if (file === null) {
        this.dispatchEvent(new Event("cancel"));
        return;
      }
      Object.defineProperty(this, "files", { value: [file] });
      this.dispatchEvent(new Event("change"));
    });
}

const fakeFile = (name: string, text: string, size = text.length) => ({
  name,
  size,
  text: vi.fn().mockResolvedValue(text),
});

const readBlob = (blob: Blob) =>
  new Promise<string>((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(String(r.result));
    r.onerror = () => reject(r.error);
    r.readAsText(blob);
  });

describe("useImportExport · web branch (WEB-09)", () => {
  beforeEach(() => {
    setWeb(true);
    for (const m of [
      exportForDownloadMock,
      importFromTextMock,
      importFromFileMock,
      openFileDialogMock,
      saveFileDialogMock,
      syncLiveMock,
      toastSuccessMock,
      toastErrorMock,
      toastWarningMock,
    ])
      m.mockReset();
    syncLiveMock.mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    // jsdom has no URL.createObjectURL: tests define it, drop it again.
    delete (URL as unknown as Record<string, unknown>).createObjectURL;
    delete (URL as unknown as Record<string, unknown>).revokeObjectURL;
  });

  it("export downloads the server content under the server filename, no save dialog", async () => {
    exportForDownloadMock.mockResolvedValue({
      filename: "cc-switch-20261027-101500.sql",
      content: "-- dump\nINSERT INTO t VALUES (1);",
    });
    let blob: Blob | undefined;
    URL.createObjectURL = vi.fn((b: Blob) => {
      blob = b;
      return "blob:x";
    });
    URL.revokeObjectURL = vi.fn();
    const names: string[] = [];
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (
      this: HTMLAnchorElement,
    ) {
      names.push(this.download);
    });
    const { result } = renderHook(() => useImportExport());

    await act(async () => {
      await result.current.exportConfig();
    });

    expect(names).toEqual(["cc-switch-20261027-101500.sql"]);
    expect(await readBlob(blob!)).toBe("-- dump\nINSERT INTO t VALUES (1);");
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:x");
    expect(saveFileDialogMock).not.toHaveBeenCalled();
    expect(toastSuccessMock).toHaveBeenCalledTimes(1);
    expect(toastSuccessMock.mock.calls[0][0]).toContain(
      "cc-switch-20261027-101500.sql",
    );
  });

  it("export server error reaches the toast verbatim", async () => {
    exportForDownloadMock.mockRejectedValue(new Error("db is locked: busy"));
    const { result } = renderHook(() => useImportExport());

    await act(async () => {
      await result.current.exportConfig();
    });

    expect(toastErrorMock).toHaveBeenCalledTimes(1);
    expect(toastErrorMock.mock.calls[0][0]).toContain("db is locked: busy");
  });

  it("import: choosing a file sends nothing; the confirm step sends its text", async () => {
    pickNext(fakeFile("my.sql", "INSERT INTO a VALUES ('é');"));
    importFromTextMock.mockResolvedValue({
      success: true,
      message: "ok",
      backupId: "b1",
    });
    const { result } = renderHook(() => useImportExport());

    await act(async () => {
      await result.current.selectImportFile();
    });

    expect(pickers).toEqual([{ type: "file", accept: ".sql" }]);
    expect(result.current.selectedFile).toBe("my.sql");
    expect(importFromTextMock).not.toHaveBeenCalled();
    expect(openFileDialogMock).not.toHaveBeenCalled();

    await act(async () => {
      await result.current.importConfig();
    });

    expect(importFromTextMock).toHaveBeenCalledWith(
      "INSERT INTO a VALUES ('é');",
    );
    expect(importFromFileMock).not.toHaveBeenCalled();
    expect(result.current.backupId).toBe("b1");
    expect(result.current.status).toBe("success");
    expect(syncLiveMock).toHaveBeenCalledTimes(1);
  });

  it("import: a server error shows its text in the toast and the status", async () => {
    pickNext(fakeFile("bad.sql", "garbage"));
    importFromTextMock.mockRejectedValue(
      new Error('near "garbage": syntax error'),
    );
    const { result } = renderHook(() => useImportExport());

    await act(async () => {
      await result.current.selectImportFile();
    });
    await act(async () => {
      await result.current.importConfig();
    });

    expect(result.current.status).toBe("error");
    expect(result.current.errorMessage).toBe('near "garbage": syntax error');
    expect(toastErrorMock.mock.calls[0][0]).toContain(
      'near "garbage": syntax error',
    );
  });

  it("import: a oversized file is rejected with a toast and never read", async () => {
    const file = fakeFile("huge.sql", "x", 65 * 1024 * 1024);
    pickNext(file);
    const { result } = renderHook(() => useImportExport());

    await act(async () => {
      await result.current.selectImportFile();
    });

    expect(toastErrorMock).toHaveBeenCalledTimes(1);
    expect(toastErrorMock.mock.calls[0][0]).toContain("64");
    expect(file.text).not.toHaveBeenCalled();
    expect(result.current.selectedFile).toBe("");
  });

  it("import: a file read failure is a toast with the reason", async () => {
    const file = fakeFile("x.sql", "");
    file.text.mockRejectedValue(new Error("NotReadableError"));
    pickNext(file);
    const { result } = renderHook(() => useImportExport());

    await act(async () => {
      await result.current.selectImportFile();
    });

    expect(toastErrorMock.mock.calls[0][0]).toContain("NotReadableError");
    expect(result.current.selectedFile).toBe("");
  });

  it("import: a change event with no file selects nothing", async () => {
    vi.spyOn(HTMLInputElement.prototype, "click").mockImplementation(function (
      this: HTMLInputElement,
    ) {
      this.dispatchEvent(new Event("change"));
    });
    const { result } = renderHook(() => useImportExport());
    await act(async () => {
      await result.current.selectImportFile();
    });
    expect(result.current.selectedFile).toBe("");
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("import: cancelling the picker changes nothing; clear drops the text", async () => {
    pickNext(null);
    const { result } = renderHook(() => useImportExport());
    await act(async () => {
      await result.current.selectImportFile();
    });
    expect(result.current.selectedFile).toBe("");
    expect(toastErrorMock).not.toHaveBeenCalled();

    vi.restoreAllMocks();
    pickNext(fakeFile("a.sql", "SELECT 1;"));
    await act(async () => {
      await result.current.selectImportFile();
    });
    act(() => result.current.clearSelection());
    await act(async () => {
      await result.current.importConfig();
    });
    expect(importFromTextMock).not.toHaveBeenCalled();
    expect(toastErrorMock).toHaveBeenCalledTimes(1);
  });
});
