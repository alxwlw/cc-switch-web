import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { UniversalProviderPanel } from "@/components/universal/UniversalProviderPanel";
import type { UniversalProvider } from "@/types";

const api = vi.hoisted(() => ({
  getAll: vi.fn(),
  upsert: vi.fn(),
  sync: vi.fn(),
  delete: vi.fn(),
}));
const toast = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
}));
let cardProps: {
  provider: UniversalProvider;
  onDuplicate: (p: UniversalProvider) => void;
  onSync: (id: string) => void;
  onEdit: (p: UniversalProvider) => void;
}[] = [];
let modalProps: {
  editingProvider?: UniversalProvider | null;
  onSave: (p: UniversalProvider) => Promise<void>;
  onSaveAndSync: (p: UniversalProvider) => Promise<void>;
} | null = null;

vi.mock("@/lib/api", () => ({ universalProvidersApi: api }));
vi.mock("@/lib/toast", () => ({ toast }));
vi.mock("@/components/universal/UniversalProviderCard", () => ({
  UniversalProviderCard: (props: (typeof cardProps)[number]) => {
    cardProps.push(props);
    return (
      <div>
        <span>{props.provider.name}</span>
        <button onClick={() => props.onDuplicate(props.provider)}>dup</button>
        <button onClick={() => props.onSync(props.provider.id)}>sync</button>
        <button onClick={() => props.onEdit(props.provider)}>edit</button>
      </div>
    );
  },
}));
vi.mock("@/components/universal/UniversalProviderFormModal", () => ({
  UniversalProviderFormModal: (props: NonNullable<typeof modalProps>) => {
    modalProps = props;
    return null;
  },
}));
vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: ({
    isOpen,
    onConfirm,
  }: {
    isOpen: boolean;
    onConfirm: () => void;
  }) => (isOpen ? <button onClick={onConfirm}>confirm</button> : null),
}));

const base = {
  id: "u1",
  name: "Univ",
  providerType: "newapi",
  apps: { claude: true, codex: true, gemini: false },
  baseUrl: "https://example.com",
  apiKey: "k",
  createdAt: 1,
} as unknown as UniversalProvider;

const SERVER_TEXT =
  "统一供应商已保存到数据库，但以下应用的配置文件未能写入：codex";

describe("UniversalProviderPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    cardProps = [];
    modalProps = null;
    api.getAll.mockResolvedValue({ u1: base });
    api.upsert.mockResolvedValue(true);
    api.sync.mockResolvedValue(true);
  });

  it("частичный отказ sync после upsert: warning с текстом сервера и перезагрузка списка", async () => {
    render(<UniversalProviderPanel />);
    await screen.findByText("Univ");
    api.sync.mockRejectedValue(new Error(SERVER_TEXT));

    await modalProps!.onSave({ ...base, id: "u2" });

    expect(api.upsert).toHaveBeenCalledWith({ ...base, id: "u2" });
    expect(api.sync).toHaveBeenCalledWith("u2");
    expect(toast.warning).toHaveBeenCalledWith(expect.any(String), {
      description: SERVER_TEXT,
    });
    expect(toast.error).not.toHaveBeenCalled();
    expect(toast.success).not.toHaveBeenCalled();
    await waitFor(() => expect(api.getAll).toHaveBeenCalledTimes(2));
  });

  it("upsert упал: error, sync не вызывается, список не перезагружается", async () => {
    render(<UniversalProviderPanel />);
    await screen.findByText("Univ");
    api.upsert.mockRejectedValue(new Error("db"));

    await modalProps!.onSave(base);

    expect(api.sync).not.toHaveBeenCalled();
    expect(toast.error).toHaveBeenCalledTimes(1);
    expect(toast.warning).not.toHaveBeenCalled();
    expect(api.getAll).toHaveBeenCalledTimes(1);
  });

  it("saveAndSync: частичный отказ sync тоже даёт warning и перезагрузку", async () => {
    render(<UniversalProviderPanel />);
    await screen.findByText("Univ");
    api.sync.mockRejectedValue(new Error(SERVER_TEXT));

    await modalProps!.onSaveAndSync(base);

    expect(toast.warning).toHaveBeenCalledWith(expect.any(String), {
      description: SERVER_TEXT,
    });
    await waitFor(() => expect(api.getAll).toHaveBeenCalledTimes(2));
  });

  it("duplicate = upsert нового id + sync этого id", async () => {
    render(<UniversalProviderPanel />);
    await screen.findByText("Univ");

    fireEvent.click(screen.getByText("dup"));

    await waitFor(() => expect(api.sync).toHaveBeenCalledTimes(1));
    const dup = api.upsert.mock.calls[0][0] as UniversalProvider;
    expect(dup.id).not.toBe("u1");
    expect(dup.name).toBe("Univ copy");
    expect(api.sync).toHaveBeenCalledWith(dup.id);
    expect(toast.success).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(api.getAll).toHaveBeenCalledTimes(2));
  });

  it("duplicate: sync упал после upsert — warning и перезагрузка", async () => {
    render(<UniversalProviderPanel />);
    await screen.findByText("Univ");
    api.sync.mockRejectedValue(new Error(SERVER_TEXT));

    fireEvent.click(screen.getByText("dup"));

    await waitFor(() => expect(toast.warning).toHaveBeenCalledTimes(1));
    expect(toast.error).not.toHaveBeenCalled();
    await waitFor(() => expect(api.getAll).toHaveBeenCalledTimes(2));
  });

  it("кнопка sync: отказ — error с текстом сервера, без перезагрузки", async () => {
    render(<UniversalProviderPanel />);
    await screen.findByText("Univ");
    api.sync.mockRejectedValue(new Error(SERVER_TEXT));

    fireEvent.click(screen.getByText("sync"));
    fireEvent.click(await screen.findByText("confirm"));

    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(expect.any(String), {
        description: SERVER_TEXT,
      }),
    );
    expect(api.getAll).toHaveBeenCalledTimes(1);
  });

  // Реальная модалка закрывается сама (onClose сразу после onSave), здесь — только состояние панели.
  it("правка: upsert без sync, панель сбрасывает editingProvider только при успехе", async () => {
    render(<UniversalProviderPanel />);
    await screen.findByText("Univ");
    fireEvent.click(screen.getByText("edit"));
    await waitFor(() => expect(modalProps!.editingProvider).toEqual(base));

    api.upsert.mockRejectedValueOnce(new Error("db"));
    await modalProps!.onSave(base);
    expect(toast.error).toHaveBeenCalledTimes(1);
    expect(modalProps!.editingProvider).toEqual(base);

    await modalProps!.onSave(base);
    expect(api.sync).not.toHaveBeenCalled();
    expect(toast.success).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(modalProps!.editingProvider).toBeNull());
  });
});
