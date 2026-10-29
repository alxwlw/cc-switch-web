import type { ReactNode } from "react";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useInstallSkillsFromZip } from "@/hooks/useSkills";
import { skillsApi } from "@/lib/api/skills";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/lib/api/skills", () => ({
  skillsApi: {
    installFromZip: vi.fn(),
    installFromZipUpload: vi.fn(),
  },
}));

const wrapper = ({ children }: { children: ReactNode }) => (
  <QueryClientProvider client={createTestQueryClient()}>
    {children}
  </QueryClientProvider>
);

describe("useInstallSkillsFromZip", () => {
  beforeEach(() => {
    vi.mocked(skillsApi.installFromZip).mockReset().mockResolvedValue({
      installed: [],
      skipped: [],
    });
    vi.mocked(skillsApi.installFromZipUpload).mockReset().mockResolvedValue({
      installed: [],
      skipped: [],
    });
  });

  it("uploads the browser File when one is given", async () => {
    const file = new File(["PK"], "a.zip");
    const { result } = renderHook(() => useInstallSkillsFromZip(), { wrapper });
    await result.current.mutateAsync({ file, currentApp: "codex" });
    await waitFor(() =>
      expect(skillsApi.installFromZipUpload).toHaveBeenCalledWith(
        file,
        "codex",
      ),
    );
    expect(skillsApi.installFromZip).not.toHaveBeenCalled();
  });

  it("installs from the host path on desktop", async () => {
    const { result } = renderHook(() => useInstallSkillsFromZip(), { wrapper });
    await result.current.mutateAsync({
      filePath: "/tmp/a.zip",
      currentApp: "claude",
    });
    expect(skillsApi.installFromZip).toHaveBeenCalledWith(
      "/tmp/a.zip",
      "claude",
    );
    expect(skillsApi.installFromZipUpload).not.toHaveBeenCalled();
  });
});
