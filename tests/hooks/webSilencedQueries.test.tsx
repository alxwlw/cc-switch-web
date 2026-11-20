import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  useAppMode,
  useAppProxyConfig,
  useGlobalProxyConfig,
  useProxyClientHost,
  useProxyStack,
  useProxyStatusQuery,
  useProxyTakeoverStatus,
} from "@/lib/query/proxy";
import {
  useAutoFailoverEnabled,
  useFailoverQueue,
  useProviderHealth,
} from "@/lib/query/failover";
import { useUsageQuery } from "@/lib/query/queries";
import {
  useAgyAccountQuota,
  useCodexOauthQuotaByAccountId,
  useKimiAccountQuota,
  useSubscriptionQuota,
  useXaiOauthQuotaByAccountId,
} from "@/lib/query/subscription";
import { useCopilotQuota } from "@/lib/query/copilot";
import { usePiCurrentState } from "@/lib/query/pi";
import {
  useCcSwitchSkillsDir,
  useScanUnmanagedSkills,
  useSearchSkillsSh,
  useSkillRepos,
} from "@/hooks/useSkills";
import {
  useOpenClawAgentsDefaults,
  useOpenClawDefaultModel,
  useOpenClawEnv,
  useOpenClawHealth,
  useOpenClawLiveProviderIds,
  useOpenClawTools,
} from "@/hooks/useOpenClaw";
import { useSidebarStatus } from "@/hooks/useSidebarStatus";
import { useKimiOAuth } from "@/components/providers/forms/hooks/useKimiOAuth";
import { useManagedAuth } from "@/components/providers/forms/hooks/useManagedAuth";
import { useDesktopConfigMessages } from "@/components/providers/mode/useDesktopConfigMessages";
import { createTestQueryClient } from "../utils/testQueryClient";
import { useProfilesQuery } from "@/lib/query/profiles";
import { setWeb } from "../helpers/web";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

// Each case: the hook and the command it must call on desktop and must not call in web.
// WEB-04 lifted the gates from usage/quota (below, the WEB-04 cases call in web too); only
// copilot stays here (OUT of scope of WEB-04). Desktop config was opened by WEB-19 (below).
const CASES: Array<[string, string, () => unknown]> = [
  [
    "useCopilotQuota",
    "copilot_get_usage",
    () => useCopilotQuota(null, { autoQuery: true }),
  ],
];

// WEB-04: usage/quota work in web: the same hooks call their commands in web too.
const WEB04_CASES: Array<[string, string, () => unknown]> = [
  [
    "useUsageQuery",
    "queryProviderUsage",
    () => useUsageQuery("p1", "claude", { autoQueryInterval: 1 }),
  ],
  [
    "useSubscriptionQuota",
    "get_subscription_quota",
    () => useSubscriptionQuota("claude", true, true, 1),
  ],
  [
    "useCodexOauthQuotaByAccountId",
    "get_codex_oauth_quota",
    () => useCodexOauthQuotaByAccountId("a1", { autoQuery: true }),
  ],
  [
    "useXaiOauthQuotaByAccountId",
    "get_xai_oauth_quota",
    () => useXaiOauthQuotaByAccountId("a1", { autoQuery: true }),
  ],
  [
    "useAgyAccountQuota",
    "agy_account_quota",
    () => useAgyAccountQuota("a1", { autoQuery: true }),
  ],
];

function wrapperFor() {
  const client = createTestQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return { client, wrapper };
}

const calledCommands = () => invokeMock.mock.calls.map((call) => call[0]);

describe("WEB-00: background queries stay silent in web", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(null);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it.each(CASES)("%s: desktop calls %s", async (_name, command, useHook) => {
    const { wrapper } = wrapperFor();
    renderHook(useHook, { wrapper });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(calledCommands()).toContain(command);
  });

  it.each(CASES)(
    "%s: web does not call %s and does not start intervals",
    async (_name, command, useHook) => {
      setWeb(true);
      const { client, wrapper } = wrapperFor();
      renderHook(useHook, { wrapper });
      // longer than the longest interval (5 min for quota with autoQuery)
      await act(async () => {
        await vi.advanceTimersByTimeAsync(6 * 60 * 1000);
      });
      expect(calledCommands()).not.toContain(command);
      const states = client
        .getQueryCache()
        .getAll()
        .map((q) => q.state.status);
      expect(states).not.toContain("error");
    },
  );

  it.each([
    ["usePiCurrentState", "get_pi_current_state", () => usePiCurrentState()],
    [
      "usePiCurrentState(omp)",
      "get_omp_current_state",
      () => usePiCurrentState(true, "omp"),
    ],
    [
      "useOpenClawLiveProviderIds",
      "get_openclaw_live_provider_ids",
      () => useOpenClawLiveProviderIds(true),
    ],
    [
      "useOpenClawDefaultModel",
      "get_openclaw_default_model",
      () => useOpenClawDefaultModel(true),
    ],
    ["useOpenClawEnv", "get_openclaw_env", () => useOpenClawEnv()],
    ["useOpenClawTools", "get_openclaw_tools", () => useOpenClawTools()],
    [
      "useOpenClawAgentsDefaults",
      "get_openclaw_agents_defaults",
      () => useOpenClawAgentsDefaults(),
    ],
    [
      "useOpenClawHealth",
      "scan_openclaw_config_health",
      () => useOpenClawHealth(true),
    ],
    [
      "useScanUnmanagedSkills",
      "scan_unmanaged_skills",
      () => useScanUnmanagedSkills({ enabled: true }),
    ],
    ["useSkillRepos", "get_skill_repos", () => useSkillRepos()],
    [
      "useSearchSkillsSh",
      "search_skills_sh",
      () => useSearchSkillsSh("abc", 10, 0),
    ],
    [
      "useCcSwitchSkillsDir",
      "get_cc_switch_skills_dir",
      () => useCcSwitchSkillsDir(),
    ],
    ["useManagedAuth", "auth_get_status", () => useManagedAuth("xai_oauth")],
    ["useProxyStatusQuery", "get_proxy_status", () => useProxyStatusQuery()],
    [
      "useProxyTakeoverStatus",
      "get_proxy_takeover_status",
      () => useProxyTakeoverStatus(),
    ],
    ["useAppMode", "get_app_mode", () => useAppMode("claude")],
    ["useProxyStack", "get_proxy_stack", () => useProxyStack("claude", true)],
    [
      "useGlobalProxyConfig",
      "get_global_proxy_config",
      () => useGlobalProxyConfig(),
    ],
    ["useProxyClientHost", "get_proxy_client_host", () => useProxyClientHost()],
    [
      "useAppProxyConfig",
      "get_proxy_config_for_app",
      () => useAppProxyConfig("claude"),
    ],
    [
      "useFailoverQueue",
      "get_failover_queue",
      () => useFailoverQueue("claude"),
    ],
    [
      "useProviderHealth",
      "get_provider_health",
      () => useProviderHealth("p1", "claude"),
    ],
    [
      "useAutoFailoverEnabled",
      "get_auto_failover_enabled",
      () => useAutoFailoverEnabled("claude"),
    ],
  ] as Array<[string, string, () => unknown]>)(
    "%s in web calls %s (gate lifted: WEB-15 / WEB-17 / WEB-05 / WEB-02 / WEB-01 / phase 28 failover)",
    async (_name, command, useHook) => {
      setWeb(true);
      const { wrapper } = wrapperFor();
      renderHook(useHook, { wrapper });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(calledCommands()).toContain(command);
    },
  );

  it("useProfilesQuery in web calls list_profiles (WEB-06, gate lifted)", async () => {
    setWeb(true);
    const { wrapper } = wrapperFor();
    renderHook(() => useProfilesQuery(), { wrapper });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(calledCommands()).toContain("list_profiles");
  });

  it("useSidebarStatus in web calls proxy status, get_app_mode and auth_get_status (WEB-01)", async () => {
    setWeb(true);
    const { wrapper } = wrapperFor();
    renderHook(() => useSidebarStatus(), { wrapper });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    const commands = calledCommands();
    expect(commands).toContain("get_proxy_status");
    expect(commands).toContain("get_app_mode");
    expect(commands).toContain("auth_get_status");
  });

  it("useSidebarStatus on desktop calls get_app_mode and auth_get_status", async () => {
    const { wrapper } = wrapperFor();
    renderHook(() => useSidebarStatus(), { wrapper });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    const commands = calledCommands();
    expect(commands).toContain("get_app_mode");
    expect(commands).toContain("auth_get_status");
  });

  it.each([
    ["useKimiOAuth", "kimi_oauth_status", () => useKimiOAuth()],
    [
      "useKimiAccountQuota",
      "kimi_oauth_quota",
      () => useKimiAccountQuota("a1", { autoQuery: true }),
    ],
  ] as Array<[string, string, () => unknown]>)(
    "%s: Kimi is open in web (WEB-02 stage 2), calls %s",
    async (_name, command, useHook) => {
      setWeb(true);
      const { wrapper } = wrapperFor();
      renderHook(useHook, { wrapper });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(calledCommands()).toContain(command);
    },
  );

  it.each(WEB04_CASES)(
    "%s: WEB-04 is open in web, calls %s",
    async (_name, command, useHook) => {
      setWeb(true);
      const { wrapper } = wrapperFor();
      renderHook(useHook, { wrapper });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(calledCommands()).toContain(command);
    },
  );

  it("useDesktopConfigMessages: WEB-19 is open in web, calls get_claude_desktop_status and keeps polling", async () => {
    for (const web of [false, true]) {
      invokeMock.mockClear();
      setWeb(web);
      const { wrapper } = wrapperFor();
      renderHook(() => useDesktopConfigMessages(true), { wrapper });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      const polls = () =>
        calledCommands().filter((c) => c === "get_claude_desktop_status")
          .length;
      expect(polls()).toBe(1);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(5000);
      });
      expect(polls()).toBeGreaterThan(1);
    }
  });

  it("useDesktopConfigMessages(false): does not poll on desktop or in web", async () => {
    for (const web of [false, true]) {
      invokeMock.mockClear();
      setWeb(web);
      const { wrapper } = wrapperFor();
      renderHook(() => useDesktopConfigMessages(false), { wrapper });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(6000);
      });
      expect(calledCommands()).not.toContain("get_claude_desktop_status");
    }
  });

  it("useStreamCheck: no gate (no isWeb), calls stream_check_provider in web too", async () => {
    const { useStreamCheck } = await import("@/hooks/useStreamCheck");
    for (const web of [false, true]) {
      invokeMock.mockClear();
      if (web) setWeb(true);
      const { wrapper } = wrapperFor();
      const { result } = renderHook(() => useStreamCheck("claude"), {
        wrapper,
      });
      await act(async () => {
        await result.current.checkProvider("p1", "P1");
      });
      expect(calledCommands()).toContain("stream_check_provider");
    }
  });
});
