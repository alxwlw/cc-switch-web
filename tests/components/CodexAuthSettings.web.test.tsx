import { act, render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { CodexAuthSettings } from "@/components/settings/CodexAuthSettings";
import type { SettingsFormState } from "@/hooks/useSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/lib/api/adapter", () => ({ isWeb: true }));

vi.mock("@/lib/api", () => ({
  settingsApi: {
    codexForcesMultiAgentV2: vi.fn().mockResolvedValue(false),
    hasCodexUnifyHistoryBackup: vi.fn().mockResolvedValue(true),
    restoreCodexUnifiedHistory: vi.fn(),
    getCodexSessionCompression: vi.fn().mockResolvedValue(false),
    setCodexSessionCompression: vi.fn(),
    getCodexSessionsDiskUsage: vi.fn().mockResolvedValue(0),
  },
}));

describe("CodexAuthSettings · web restore wording (WEB-20)", () => {
  it("labels the restore checkbox as a server-side restore", async () => {
    await act(async () => {
      render(
        <CodexAuthSettings
          settings={{ unifyCodexSessionHistory: true } as SettingsFormState}
          onChange={vi.fn()}
        />,
      );
    });
    await act(async () => {
      fireEvent.click(
        screen.getByRole("switch", {
          name: "settings.unifyCodexSessionHistory",
        }),
      );
    });
    expect(
      await screen.findByText("confirm.unifyCodexHistoryOff.restoreBackupWeb"),
    ).toBeTruthy();
    expect(
      screen.queryByText("confirm.unifyCodexHistoryOff.restoreBackup"),
    ).toBeNull();
  });
});
