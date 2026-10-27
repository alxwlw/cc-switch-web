import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  useAppMode,
  useAppProxyConfig,
  useDirectProviderId,
  useGlobalProxyConfig,
  useProxyClientHost,
  useProxyStack,
  useProxyStatusQuery,
  useProxyTakeoverStatus,
} from "@/lib/query/proxy";
import {
  useAutoFailoverEnabled,
  useAvailableProvidersForFailover,
  useCircuitBreakerConfig,
  useCircuitBreakerStats,
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
import { useProviderLimits } from "@/lib/query/usage";
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

// Каждый кейс: хук и команда, которую он обязан звать на desktop и не звать в web.
const CASES: Array<[string, string, () => unknown]> = [
  ["useProxyStatusQuery", "get_proxy_status", () => useProxyStatusQuery()],
  [
    "useProxyTakeoverStatus",
    "get_proxy_takeover_status",
    () => useProxyTakeoverStatus(),
  ],
  ["useAppMode", "get_app_mode", () => useAppMode("claude")],
  ["useFailoverQueue", "get_failover_queue", () => useFailoverQueue("claude")],
  [
    "useProviderHealth",
    "get_provider_health",
    () => useProviderHealth("p1", "claude"),
  ],
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
  ["usePiCurrentState", "get_pi_current_state", () => usePiCurrentState()],
  [
    "useScanUnmanagedSkills",
    "scan_unmanaged_skills",
    () => useScanUnmanagedSkills({ enabled: true }),
  ],
  ["useKimiOAuth", "kimi_oauth_status", () => useKimiOAuth()],
  ["useManagedAuth", "auth_get_status", () => useManagedAuth("xai_oauth")],
  [
    "useDesktopConfigMessages",
    "get_claude_desktop_status",
    () => useDesktopConfigMessages(true),
  ],
  [
    "useDirectProviderId",
    "get_direct_provider",
    () => useDirectProviderId("claude", true),
  ],
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
    "useCircuitBreakerConfig",
    "get_circuit_breaker_config",
    () => useCircuitBreakerConfig(),
  ],
  [
    "useCircuitBreakerStats",
    "get_circuit_breaker_stats",
    () => useCircuitBreakerStats("p1", "claude"),
  ],
  [
    "useAvailableProvidersForFailover",
    "get_available_providers_for_failover",
    () => useAvailableProvidersForFailover("claude"),
  ],
  [
    "useAutoFailoverEnabled",
    "get_auto_failover_enabled",
    () => useAutoFailoverEnabled("claude"),
  ],
  [
    "useProviderLimits",
    "check_provider_limits",
    () => useProviderLimits("p1", "claude"),
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
  [
    "useKimiAccountQuota",
    "kimi_oauth_quota",
    () => useKimiAccountQuota("a1", { autoQuery: true }),
  ],
  [
    "useCopilotQuota",
    "copilot_get_usage",
    () => useCopilotQuota(null, { autoQuery: true }),
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
];

function wrapperFor() {
  const client = createTestQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return { client, wrapper };
}

const calledCommands = () => invokeMock.mock.calls.map((call) => call[0]);

describe("WEB-00: фоновые query молчат в web", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(null);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it.each(CASES)("%s: desktop зовёт %s", async (_name, command, useHook) => {
    const { wrapper } = wrapperFor();
    renderHook(useHook, { wrapper });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(calledCommands()).toContain(command);
  });

  it.each(CASES)(
    "%s: web не зовёт %s и не стартует интервалы",
    async (_name, command, useHook) => {
      setWeb(true);
      const { client, wrapper } = wrapperFor();
      renderHook(useHook, { wrapper });
      // дольше самого длинного интервала (5 мин у quota при autoQuery)
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
  ] as Array<[string, string, () => unknown]>)(
    "%s в web зовёт %s (WEB-15, гейт снят)",
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

  it("useProfilesQuery в web зовёт list_profiles (WEB-06, гейт снят)", async () => {
    setWeb(true);
    const { wrapper } = wrapperFor();
    renderHook(() => useProfilesQuery(), { wrapper });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(calledCommands()).toContain("list_profiles");
  });

  it("useSidebarStatus в web: режим direct, без alert и точки auth, запросов mode/auth нет", async () => {
    setWeb(true);
    const { wrapper } = wrapperFor();
    const { result } = renderHook(() => useSidebarStatus(), { wrapper });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2 * 60 * 1000);
    });
    expect(result.current.appStatus("claude")).toEqual({
      mode: "direct",
      alert: false,
    });
    expect(result.current.authNeedsAttention).toBe(false);
    const commands = calledCommands();
    expect(commands).not.toContain("get_proxy_status");
    expect(commands).not.toContain("get_app_mode");
    expect(commands).not.toContain("auth_get_status");
  });

  it("useSidebarStatus на desktop зовёт get_app_mode и auth_get_status", async () => {
    const { wrapper } = wrapperFor();
    renderHook(() => useSidebarStatus(), { wrapper });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    const commands = calledCommands();
    expect(commands).toContain("get_app_mode");
    expect(commands).toContain("auth_get_status");
  });
});
