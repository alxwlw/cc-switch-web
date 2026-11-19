import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { homeDir, join } from "@tauri-apps/api/path";
import { settingsApi } from "@/lib/api";
import type { AppConfigDirStatus } from "@/lib/api/settings";
import { isWeb } from "@/lib/api/adapter";
import {
  DIRECTORY_APP,
  DIRECTORY_APPS,
  type DirectoryAppId,
  sanitizeDir,
} from "@/config/directoryApps";
import type { SettingsFormState } from "./useSettingsForm";

type DirectoryKey = "appConfig" | DirectoryAppId;

export type ResolvedDirectories = Record<DirectoryKey, string>;

const emptyDirectories = (): ResolvedDirectories => ({
  appConfig: "",
  ...(Object.fromEntries(DIRECTORY_APPS.map(({ app }) => [app, ""])) as Record<
    DirectoryAppId,
    string
  >),
});

const computeDefaultAppConfigDir = async (): Promise<string | undefined> => {
  try {
    const home = await homeDir();
    return await join(home, ".cc-switch");
  } catch (error) {
    console.error(
      "[useDirectorySettings] Failed to resolve default app config dir",
      error,
    );
    return undefined;
  }
};

/**
 * The stored data-directory override. Web (WEB-26): the server's own status, which also says
 * whether `CC_SWITCH_CONFIG_DIR` fixes the directory (then there is no override to edit). A
 * failed status read is logged and must not empty the other directories, as the missing
 * route did before.
 */
const loadAppConfigDir = async (): Promise<{
  status: AppConfigDirStatus | null;
  override: string | null;
}> => {
  if (!isWeb) {
    return {
      status: null,
      override: await settingsApi.getAppConfigDirOverride(),
    };
  }
  try {
    const status = await settingsApi.getAppConfigDirStatus();
    return { status, override: status.envLocked ? null : status.override };
  } catch (error) {
    console.error(
      "[useDirectorySettings] Failed to load app config dir status",
      error,
    );
    return { status: null, override: null };
  }
};

const computeDefaultConfigDir = async (
  app: DirectoryAppId,
): Promise<string | undefined> => {
  try {
    const home = await homeDir();
    return await join(home, DIRECTORY_APP[app].defaultFolder);
  } catch (error) {
    console.error(
      "[useDirectorySettings] Failed to resolve default config dir",
      error,
    );
    return undefined;
  }
};

export interface UseDirectorySettingsProps {
  settings: SettingsFormState | null;
  onUpdateSettings: (updates: Partial<SettingsFormState>) => void;
}

export interface UseDirectorySettingsResult {
  appConfigDir?: string;
  resolvedDirs: ResolvedDirectories;
  isLoading: boolean;
  initialAppConfigDir?: string;
  /** Web only: the server's data directory status (null on desktop or when it failed to load). */
  appConfigDirStatus: AppConfigDirStatus | null;
  /** 保存成功后把已保存的数据目录换成刚存的值 */
  commitAppConfigDir: (value: string | undefined) => void;
  updateDirectory: (app: DirectoryAppId, value?: string) => void;
  updateAppConfigDir: (value?: string) => void;
  browseDirectory: (app: DirectoryAppId) => Promise<void>;
  browseAppConfigDir: () => Promise<void>;
  resetDirectory: (app: DirectoryAppId) => Promise<void>;
  resetAppConfigDir: () => Promise<void>;
  resetAllDirectories: (overrides?: ResolvedAppDirectoryOverrides) => void;
}

export type ResolvedAppDirectoryOverrides = Partial<
  Record<DirectoryAppId, string | undefined>
>;

/**
 * useDirectorySettings - 目录管理
 * 负责：
 * - appConfigDir 状态
 * - resolvedDirs 状态
 * - 目录选择（browse）
 * - 目录重置
 * - 默认值计算
 */
export function useDirectorySettings({
  settings,
  onUpdateSettings,
}: UseDirectorySettingsProps): UseDirectorySettingsResult {
  const { t } = useTranslation();

  const [appConfigDir, setAppConfigDir] = useState<string | undefined>(
    undefined,
  );
  const [resolvedDirs, setResolvedDirs] =
    useState<ResolvedDirectories>(emptyDirectories);
  const [appConfigDirStatus, setAppConfigDirStatus] =
    useState<AppConfigDirStatus | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  const defaultsRef = useRef<ResolvedDirectories>(emptyDirectories());
  // 已保存的数据目录（保存按钮和「需要重启」都和它比）。ref 给回调用，state 让比较它的
  // 组件在保存后重渲染：保存成功要把它更新成刚存的值，否则保存按钮一直在、改回去存不了
  const initialAppConfigDirRef = useRef<string | undefined>(undefined);
  const [initialAppConfigDir, setInitialAppConfigDir] = useState<
    string | undefined
  >(undefined);
  const commitAppConfigDir = useCallback((value: string | undefined) => {
    initialAppConfigDirRef.current = value;
    setInitialAppConfigDir(value);
  }, []);

  // 加载目录信息
  useEffect(() => {
    let active = true;
    setIsLoading(true);

    const load = async () => {
      try {
        const [appDirSource, remoteDirs, defaultAppConfig, defaultDirs] =
          await Promise.all([
            loadAppConfigDir(),
            Promise.all(
              DIRECTORY_APPS.map(({ app }) => settingsApi.getConfigDir(app)),
            ),
            computeDefaultAppConfigDir(),
            Promise.all(
              DIRECTORY_APPS.map(({ app }) => computeDefaultConfigDir(app)),
            ),
          ]);

        if (!active) return;

        const { status, override: overrideRaw } = appDirSource;
        const normalizedOverride = sanitizeDir(overrideRaw ?? undefined);
        setAppConfigDirStatus(status);

        const defaults = emptyDirectories();
        const resolved = emptyDirectories();
        // Web: the server's `~/.cc-switch` (the shim's `homeDir` is not the server's home); a
        // directory fixed by the env has no default to go back to, so it shows the env value.
        defaults.appConfig = status
          ? status.envLocked
            ? status.effective
            : status.default
          : ((isWeb ? "" : defaultAppConfig) ?? "");
        resolved.appConfig = normalizedOverride ?? defaults.appConfig;
        DIRECTORY_APPS.forEach(({ app }, i) => {
          defaults[app] = defaultDirs[i] ?? "";
          resolved[app] = remoteDirs[i] || defaults[app];
        });
        defaultsRef.current = defaults;

        setAppConfigDir(normalizedOverride);
        commitAppConfigDir(normalizedOverride);

        setResolvedDirs(resolved);
      } catch (error) {
        console.error(
          "[useDirectorySettings] Failed to load directory info",
          error,
        );
      } finally {
        if (active) {
          setIsLoading(false);
        }
      }
    };

    void load();
    return () => {
      active = false;
    };
  }, []);

  const previousSettingsRef = useRef(settings);
  const localDirectoryUpdatesRef = useRef<ResolvedAppDirectoryOverrides>({});
  const directoryRevisionsRef = useRef<Partial<Record<DirectoryAppId, number>>>(
    {},
  );
  useEffect(() => {
    const previousSettings = previousSettingsRef.current;
    previousSettingsRef.current = settings;
    if (!previousSettings || !settings) return;

    // Refetch can discard a draft. Resolve cleared overrides through the backend,
    // whose defaults may depend on the platform or environment variables.
    for (const { app: key, field } of DIRECTORY_APPS) {
      const value = sanitizeDir(settings[field] as string | undefined);
      if (
        value === sanitizeDir(previousSettings[field] as string | undefined)
      ) {
        continue;
      }
      if (
        key in localDirectoryUpdatesRef.current &&
        localDirectoryUpdatesRef.current[key] === value
      ) {
        delete localDirectoryUpdatesRef.current[key];
        continue;
      }
      delete localDirectoryUpdatesRef.current[key];
      const revision = (directoryRevisionsRef.current[key] ?? 0) + 1;
      directoryRevisionsRef.current[key] = revision;
      setResolvedDirs((prev) => ({ ...prev, [key]: value ?? "" }));
      if (value !== undefined) continue;

      void settingsApi.getConfigDir(key).then(
        (resolved) => {
          setResolvedDirs((prev) =>
            directoryRevisionsRef.current[key] === revision
              ? { ...prev, [key]: resolved }
              : prev,
          );
        },
        (error) => {
          console.error(
            "[useDirectorySettings] Failed to resolve config dir",
            error,
          );
        },
      );
    }
  }, [settings]);

  useEffect(
    () => () => {
      for (const { app: key } of DIRECTORY_APPS) {
        directoryRevisionsRef.current[key] =
          (directoryRevisionsRef.current[key] ?? 0) + 1;
      }
    },
    [],
  );

  const updateDirectoryState = useCallback(
    (key: DirectoryKey, value?: string) => {
      const sanitized = sanitizeDir(value);
      if (key === "appConfig") {
        setAppConfigDir(sanitized);
      } else {
        const field = DIRECTORY_APP[key].field;
        // Local edits already update the display below. Do not resolve a local
        // reset against the old persisted override, or let an older read win.
        directoryRevisionsRef.current[key] =
          (directoryRevisionsRef.current[key] ?? 0) + 1;
        localDirectoryUpdatesRef.current[key] = sanitized;
        onUpdateSettings({ [field]: sanitized });
      }

      setResolvedDirs((prev) => {
        const next = sanitized ?? defaultsRef.current[key];
        // Same-ref early-return: unchanged value shouldn't cascade renders
        // through the settings tree.
        if (prev[key] === next) return prev;
        return { ...prev, [key]: next };
      });
    },
    [onUpdateSettings],
  );

  const updateAppConfigDir = useCallback(
    (value?: string) => {
      updateDirectoryState("appConfig", value);
    },
    [updateDirectoryState],
  );

  const updateDirectory = useCallback(
    (app: DirectoryAppId, value?: string) => {
      updateDirectoryState(app, value);
    },
    [updateDirectoryState],
  );

  const browseDirectory = useCallback(
    async (app: DirectoryAppId) => {
      const currentValue =
        (settings?.[DIRECTORY_APP[app].field] as string | undefined) ??
        resolvedDirs[app];

      try {
        const picked = await settingsApi.selectConfigDirectory(currentValue);
        const sanitized = sanitizeDir(picked ?? undefined);
        if (!sanitized) return;
        updateDirectoryState(app, sanitized);
      } catch (error) {
        console.error("[useDirectorySettings] Failed to pick directory", error);
        toast.error(
          t("settings.selectFileFailed", {
            defaultValue: "选择目录失败",
          }),
        );
      }
    },
    [settings, resolvedDirs, t, updateDirectoryState],
  );

  const browseAppConfigDir = useCallback(async () => {
    const currentValue = appConfigDir ?? resolvedDirs.appConfig;
    try {
      const picked = await settingsApi.selectConfigDirectory(currentValue);
      const sanitized = sanitizeDir(picked ?? undefined);
      if (!sanitized) return;
      updateDirectoryState("appConfig", sanitized);
    } catch (error) {
      console.error(
        "[useDirectorySettings] Failed to pick app config directory",
        error,
      );
      toast.error(
        t("settings.selectFileFailed", {
          defaultValue: "选择目录失败",
        }),
      );
    }
  }, [appConfigDir, resolvedDirs.appConfig, t, updateDirectoryState]);

  const resetDirectory = useCallback(
    async (app: DirectoryAppId) => {
      if (!defaultsRef.current[app]) {
        const fallback = await computeDefaultConfigDir(app);
        if (fallback) {
          defaultsRef.current = {
            ...defaultsRef.current,
            [app]: fallback,
          };
        }
      }
      updateDirectoryState(app, undefined);
    },
    [updateDirectoryState],
  );

  const resetAppConfigDir = useCallback(async () => {
    // Web: the default comes from the server; the shim's `homeDir` is not the server's home.
    if (!isWeb && !defaultsRef.current.appConfig) {
      const fallback = await computeDefaultAppConfigDir();
      if (fallback) {
        defaultsRef.current = {
          ...defaultsRef.current,
          appConfig: fallback,
        };
      }
    }
    updateDirectoryState("appConfig", undefined);
  }, [updateDirectoryState]);

  const resetAllDirectories = useCallback(
    (overrides?: ResolvedAppDirectoryOverrides) => {
      for (const { app: key } of DIRECTORY_APPS) {
        directoryRevisionsRef.current[key] =
          (directoryRevisionsRef.current[key] ?? 0) + 1;
      }
      setAppConfigDir(initialAppConfigDirRef.current);
      const next = emptyDirectories();
      next.appConfig =
        initialAppConfigDirRef.current ?? defaultsRef.current.appConfig;
      for (const { app } of DIRECTORY_APPS) {
        next[app] = overrides?.[app] ?? defaultsRef.current[app];
      }
      setResolvedDirs(next);
    },
    [],
  );

  return {
    appConfigDir,
    resolvedDirs,
    isLoading,
    initialAppConfigDir,
    appConfigDirStatus,
    commitAppConfigDir,
    updateDirectory,
    updateAppConfigDir,
    browseDirectory,
    browseAppConfigDir,
    resetDirectory,
    resetAppConfigDir,
    resetAllDirectories,
  };
}
