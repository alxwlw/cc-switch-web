import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { EnvWarningBanner } from "@/components/env/EnvWarningBanner";
import type { EnvConflict } from "@/types/env";

const mocks = vi.hoisted(() => ({
  web: { value: true },
  deleteEnvVars: vi.fn(),
  success: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, params?: { path?: string }) =>
      params?.path ? `${key}:${params.path}` : key,
  }),
}));
vi.mock("@/lib/api/adapter", () => ({
  get isWeb() {
    return mocks.web.value;
  },
}));
vi.mock("@/lib/api/env", () => ({ deleteEnvVars: mocks.deleteEnvVars }));
vi.mock("@/lib/toast", () => ({
  toast: { success: mocks.success, warning: vi.fn(), error: vi.fn() },
}));

const conflicts: EnvConflict[] = [
  {
    varName: "ANTHROPIC_API_KEY",
    varValue: "sk-x",
    sourceType: "file",
    sourcePath: "/home/u/.bashrc:3",
  },
];

async function openConfirm() {
  fireEvent.click(screen.getByText("env.actions.expand"));
  fireEvent.click(screen.getByLabelText("env.actions.selectAll"));
  fireEvent.click(screen.getByText("env.actions.deleteSelected"));
}

describe("EnvWarningBanner · web host warning (WEB-22)", () => {
  beforeEach(() => {
    mocks.web.value = true;
    mocks.deleteEnvVars.mockReset();
    mocks.success.mockReset();
  });

  it("shows the server-host warning in the confirm dialog in web mode", async () => {
    render(
      <EnvWarningBanner
        conflicts={conflicts}
        onDismiss={vi.fn()}
        onDeleted={vi.fn()}
      />,
    );
    await openConfirm();
    expect(await screen.findByTestId("env-web-host-warning")).toHaveTextContent(
      "env.confirm.webHostWarning",
    );
  });

  it("does not show it in desktop mode", async () => {
    mocks.web.value = false;
    render(
      <EnvWarningBanner
        conflicts={conflicts}
        onDismiss={vi.fn()}
        onDeleted={vi.fn()}
      />,
    );
    await openConfirm();
    await screen.findByText("env.confirm.backupNotice");
    expect(screen.queryByTestId("env-web-host-warning")).toBeNull();
  });

  it("cleanup flow still toasts the backup path and notifies the parent", async () => {
    mocks.deleteEnvVars.mockResolvedValue({ backupPath: "/srv/bk.json" });
    const onDeleted = vi.fn();
    render(
      <EnvWarningBanner
        conflicts={conflicts}
        onDismiss={vi.fn()}
        onDeleted={onDeleted}
      />,
    );
    await openConfirm();
    fireEvent.click(await screen.findByText("env.confirm.confirm"));
    await waitFor(() => expect(onDeleted).toHaveBeenCalledTimes(1));
    expect(mocks.deleteEnvVars).toHaveBeenCalledWith(conflicts);
    expect(mocks.success).toHaveBeenCalledWith(
      "env.delete.success",
      expect.objectContaining({
        description: "env.backup.location:/srv/bk.json",
      }),
    );
  });
});
