import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Copy,
  Download,
  ExternalLink,
  Github,
  Globe,
  Info,
  Loader2,
  RefreshCw,
  Sparkles,
  Star,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { getVersion } from "@tauri-apps/api/app";
import { settingsApi } from "@/lib/api";
import { useUpdate } from "@/contexts/UpdateContext";
import { Badge } from "@/components/ui/badge";
import { WhatsNewDialog } from "@/components/WhatsNewDialog";
import { WHATS_NEW_ENTRIES, entriesUpTo } from "@/lib/whatsNew";
import ccswitchLogo from "@/assets/icons/logo.svg";
import { WebModeInfo } from "@/components/web/WebModeInfo";
import { extractErrorMessage } from "@/utils/errorUtils";
import { isWeb } from "@/lib/api/adapter";
import { copyText } from "@/lib/clipboard";
import type { WebUpdateCheck } from "@/lib/updater";
import { useServerCapabilities } from "@/hooks/useServerCapabilities";
import { useSelfUpdate } from "@/hooks/useSelfUpdate";
import { WebCommitList } from "@/components/settings/WebCommitList";
import {
  SelfUpdateButton,
  SelfUpdateNotice,
} from "@/components/settings/SelfUpdatePanel";

interface AboutSectionProps {
  isPortable: boolean;
}

// 应用自身版本（getVersion，本地毫秒级、无网络）也缓存一份，纯为重挂时免去 loading 闪烁。
let appVersionCache: string | null = null;

// 邀 Star 条关掉后记在本机（纯界面偏好，不进库、不随云同步）
const STAR_PROMPT_DISMISSED_KEY = "ccswitch:about:starPromptDismissed";

function readStarPromptDismissed(): boolean {
  try {
    return localStorage.getItem(STAR_PROMPT_DISMISSED_KEY) === "1";
  } catch {
    return false;
  }
}

/** web: the result of the server's update check — how far behind, what is new, how to update. */
function WebUpdateStatus({
  webUpdate,
  error,
  selfUpdate,
}: {
  webUpdate: WebUpdateCheck | null;
  error: string | null;
  /** Set when the server accepts self-update; the button shows only for `behind`. */
  selfUpdate: ReturnType<typeof useSelfUpdate> | null;
}) {
  const { t } = useTranslation();

  const handleCopy = useCallback(
    async (text: string) => {
      try {
        await copyText(text);
        toast.success(t("web.update.copied"), { closeButton: true });
      } catch (copyError) {
        console.error("[AboutSection] Failed to copy instructions", copyError);
        toast.error(t("web.update.copyFailed"), {
          description: extractErrorMessage(copyError) || undefined,
          closeButton: true,
        });
      }
    },
    [t],
  );

  // The server's own text (rate limit, no network), as it came: not "up to date".
  if (error) {
    return (
      <div role="alert" className="space-y-1 px-5 py-4 text-body">
        <p className="font-medium text-fg-1">
          {t("web.update.checkFailedTitle")}
        </p>
        <p className="break-words text-fg-2">{error}</p>
      </div>
    );
  }
  if (!webUpdate) return null;

  const checked = new Date(webUpdate.checkedAt);
  const checkedAt = Number.isNaN(checked.getTime())
    ? webUpdate.checkedAt
    : checked.toLocaleString();
  const footer = (
    <p className="text-caption text-fg-2">
      {webUpdate.currentSha && (
        <span className="tabular-nums">
          {t("web.update.buildSha", { sha: webUpdate.currentSha.slice(0, 7) })}
          {" · "}
        </span>
      )}
      {t("web.update.checkedAt", { time: checkedAt })}
    </p>
  );

  if (webUpdate.status === "unknown") {
    return (
      <div className="space-y-1 px-5 py-4 text-body">
        <p className="font-medium text-fg-1">{t("web.update.unknownTitle")}</p>
        <p className="text-fg-2">{t("web.update.unknownHint")}</p>
        {footer}
      </div>
    );
  }

  if (webUpdate.status === "up-to-date") {
    return (
      <div className="space-y-1 px-5 py-4 text-body">
        <p className="font-medium text-fg-1">{t("web.update.upToDateTitle")}</p>
        {footer}
      </div>
    );
  }

  const instructions = [
    { key: "systemd", text: webUpdate.instructions.systemd },
    { key: "docker", text: webUpdate.instructions.docker },
  ];
  return (
    <div className="space-y-3 px-5 py-4 text-body">
      <div className="space-y-1">
        <p className="font-medium text-fg-1">
          {t("web.update.behindTitle", { count: webUpdate.behindBy })}
        </p>
        {footer}
      </div>

      {selfUpdate && (
        <SelfUpdateButton
          webUpdate={webUpdate}
          state={selfUpdate.state}
          onStart={selfUpdate.start}
        />
      )}

      <WebCommitList
        commits={webUpdate.commits}
        behindBy={webUpdate.behindBy}
      />

      <div className="space-y-2">
        <p className="text-caption font-medium text-fg-2">
          {t("web.update.instructionsTitle")}
        </p>
        {instructions.map(({ key, text }) => (
          <div key={key} className="space-y-1">
            <div className="flex items-center justify-between gap-2">
              <span className="text-caption text-fg-1">
                {t(`web.update.${key}`)}
              </span>
              <Button
                type="button"
                variant="neutral"
                size="compact"
                onClick={() => void handleCopy(text)}
                aria-label={`${t("web.update.copy")}: ${t(`web.update.${key}`)}`}
              >
                <Copy className="h-3.5 w-3.5" />
                {t("web.update.copy")}
              </Button>
            </div>
            <pre className="overflow-x-auto whitespace-pre-wrap break-all rounded-control bg-subtle p-2 text-caption text-fg-1">
              {text}
            </pre>
          </div>
        ))}
      </div>
    </div>
  );
}

/**
 * 设置 → 关于：版本、便携模式、检查更新 / 安装并重启、项目链接。
 * 命令行应用的安装与升级在侧栏「应用」页。
 */
export function AboutSection({ isPortable }: AboutSectionProps) {
  const { t } = useTranslation();
  // 惰性初始化自模块缓存：重挂时首帧即渲染上次的值，避免 loading 闪烁；首次挂载缓存
  // 为空则回退到原始初值（null / loading）。
  const [version, setVersion] = useState<string | null>(() => appVersionCache);
  const [isLoadingVersion, setIsLoadingVersion] = useState(
    () => appVersionCache === null,
  );
  const [isDownloading, setIsDownloading] = useState(false);
  const [whatsNewOpen, setWhatsNewOpen] = useState(false);
  const [starPromptDismissed, setStarPromptDismissed] = useState(
    readStarPromptDismissed,
  );
  const recentEntries = useMemo(
    () => (version ? entriesUpTo(WHATS_NEW_ENTRIES, version) : []),
    [version],
  );

  const {
    hasUpdate,
    updateInfo,
    checkUpdate,
    resetDismiss,
    isChecking,
    webUpdate,
    error: checkError,
  } = useUpdate();

  // web: the server says whether the install route would accept a request now.
  const capabilities = useServerCapabilities();
  const selfUpdateEnabled = isWeb && capabilities.data?.selfUpdate === true;
  const selfUpdate = useSelfUpdate(selfUpdateEnabled, {
    // Nothing was merged: the server's cached `behind` is stale, ask again past its cache.
    onNothingToUpdate: () => {
      checkUpdate({ force: true }).catch((error) =>
        console.error("[AboutSection] Re-check after the update failed", error),
      );
    },
  });

  useEffect(() => {
    let active = true;
    const loadAppVersion = async () => {
      try {
        const appVersion = await getVersion();
        appVersionCache = appVersion;
        if (active) {
          setVersion(appVersion);
        }
      } catch (error) {
        console.error("[AboutSection] Failed to load app version", error);
        if (active) {
          setVersion(null);
        }
      } finally {
        if (active) {
          setIsLoadingVersion(false);
        }
      }
    };

    void loadAppVersion();
    return () => {
      active = false;
    };
  }, []);

  const handleOpenReleaseNotes = useCallback(async () => {
    try {
      // web: `availableVersion` is the build string `<version>+<sha>`, not a release tag;
      // the link is the running version's page, as when up to date.
      const targetVersion = isWeb
        ? (version ?? "")
        : (updateInfo?.availableVersion ?? version ?? "");
      const displayVersion = targetVersion.startsWith("v")
        ? targetVersion
        : targetVersion
          ? `v${targetVersion}`
          : "";

      if (!displayVersion) {
        await settingsApi.openExternal(
          "https://github.com/farion1231/cc-switch/releases",
        );
        return;
      }

      await settingsApi.openExternal(
        `https://github.com/farion1231/cc-switch/releases/tag/${displayVersion}`,
      );
    } catch (error) {
      console.error("[AboutSection] Failed to open release notes", error);
      toast.error(t("settings.openReleaseNotesFailed"));
    }
  }, [t, updateInfo?.availableVersion, version]);

  const handleOpenGithub = useCallback(() => {
    void settingsApi.openExternal("https://github.com/farion1231/cc-switch");
  }, []);

  const handleDismissStarPrompt = useCallback(() => {
    setStarPromptDismissed(true);
    try {
      localStorage.setItem(STAR_PROMPT_DISMISSED_KEY, "1");
    } catch {
      // 存不下就只在本次会话里隐藏
    }
  }, []);

  const handleCheckUpdate = useCallback(async () => {
    if (isWeb) {
      // The button is the only caller that forces past the server's cache; the result
      // (or the server's error text) is shown by WebUpdateStatus.
      try {
        await checkUpdate({ force: true });
      } catch (error) {
        console.error("[AboutSection] Check update failed", error);
      }
      return;
    }

    if (hasUpdate) {
      if (isPortable) {
        try {
          await settingsApi.openPortableUpdateDownload(
            updateInfo?.availableVersion ?? "",
          );
        } catch (error) {
          console.error("[AboutSection] Portable update failed", error);
        }
        return;
      }

      setIsDownloading(true);
      try {
        resetDismiss();
        const installed = await settingsApi.installUpdateAndRestart();
        if (!installed) {
          toast.success(t("settings.upToDate"), { closeButton: true });
        }
      } catch (error) {
        console.error("[AboutSection] Update failed", error);
        toast.error(t("settings.updateFailed"), {
          description: extractErrorMessage(error) || undefined,
          closeButton: true,
        });
        try {
          await settingsApi.checkUpdates();
        } catch (fallbackError) {
          console.error(
            "[AboutSection] Failed to open fallback updater",
            fallbackError,
          );
        }
      } finally {
        setIsDownloading(false);
      }
      return;
    }

    try {
      const available = await checkUpdate();
      if (!available) {
        toast.success(t("settings.upToDate"), { closeButton: true });
      }
    } catch (error) {
      console.error("[AboutSection] Check update failed", error);
      toast.error(t("settings.checkUpdateFailed"), {
        description: extractErrorMessage(error) || undefined,
        closeButton: true,
      });
    }
  }, [
    checkUpdate,
    hasUpdate,
    isPortable,
    resetDismiss,
    t,
    updateInfo?.availableVersion,
  ]);

  const displayVersion = version ?? t("common.unknown");

  return (
    <div className="divide-y divide-border overflow-hidden rounded-panel border border-border bg-surface">
      <div className="flex flex-wrap items-center gap-4 px-5 py-4">
        <img
          src={ccswitchLogo}
          alt=""
          draggable={false}
          className="h-10 w-10 shrink-0 p-1"
        />
        <div className="min-w-0 flex-1">
          <div className="text-title text-fg-1">CC Switch</div>
          <div className="flex flex-wrap items-center gap-2 text-caption text-fg-2">
            {isLoadingVersion ? (
              <Loader2 className="h-3 w-3 animate-spin" />
            ) : (
              <span className="tabular-nums">{`${t("common.version")} v${displayVersion}`}</span>
            )}
            {isPortable && (
              <Badge
                variant="secondary"
                className="gap-1 px-1.5 py-0 text-badge"
              >
                <Info className="h-3 w-3" />
                {t("settings.portableMode")}
              </Badge>
            )}
          </div>
        </div>
        {/* web: the button only checks; installing is the "Update now" button of the behind block */}
        <Button
          type="button"
          variant={hasUpdate && !isWeb ? "solid" : "neutral"}
          size="regular"
          onClick={handleCheckUpdate}
          disabled={isChecking || isDownloading}
        >
          {isDownloading ? (
            <>
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
              {t("settings.updating")}
            </>
          ) : hasUpdate && !isWeb ? (
            <>
              <Download className="h-3.5 w-3.5" />
              {t("settings.updateTo", {
                version: updateInfo?.availableVersion ?? "",
              })}
            </>
          ) : isChecking ? (
            <>
              <RefreshCw className="h-3.5 w-3.5 animate-spin" />
              {t("settings.checking")}
            </>
          ) : (
            <>
              <RefreshCw className="h-3.5 w-3.5" />
              {t("settings.checkForUpdates")}
            </>
          )}
        </Button>
      </div>

      {isWeb && (
        <WebUpdateStatus
          webUpdate={webUpdate}
          error={checkError}
          selfUpdate={selfUpdateEnabled ? selfUpdate : null}
        />
      )}
      {isWeb && selfUpdateEnabled && (
        <SelfUpdateNotice state={selfUpdate.state} />
      )}

      {!isWeb && hasUpdate && updateInfo && (
        <div className="space-y-1 px-5 py-4 text-body">
          <p className="font-medium text-fg-1">
            {t("settings.updateAvailable", {
              version: updateInfo.availableVersion,
            })}
          </p>
          {updateInfo.notes && (
            <p className="line-clamp-3 text-fg-2">{updateInfo.notes}</p>
          )}
          {isPortable && (
            <p className="text-fg-2">{t("settings.portableUpdateHint")}</p>
          )}
        </div>
      )}

      <WebModeInfo />

      <div className="flex flex-wrap items-center gap-2 px-5 py-4">
        {starPromptDismissed && (
          <Button
            type="button"
            variant="neutral"
            size="compact"
            onClick={handleOpenGithub}
          >
            <Github className="h-3.5 w-3.5" />
            {t("settings.github")}
          </Button>
        )}
        <Button
          type="button"
          variant="neutral"
          size="compact"
          onClick={() => settingsApi.openExternal("https://ccswitch.io")}
        >
          <Globe className="h-3.5 w-3.5" />
          {t("settings.officialWebsite")}
        </Button>
        <Button
          type="button"
          variant="neutral"
          size="compact"
          onClick={handleOpenReleaseNotes}
        >
          <ExternalLink className="h-3.5 w-3.5" />
          {t("settings.releaseNotes")}
        </Button>
        {recentEntries.length > 0 && (
          <Button
            type="button"
            variant="neutral"
            size="compact"
            onClick={() => setWhatsNewOpen(true)}
          >
            <Sparkles className="h-3.5 w-3.5" />
            {t("whatsNew.recentTitle")}
          </Button>
        )}
      </div>

      {/* 下载与更新都走 ccswitch.io，用户不再经过仓库页，邀 Star 单独占一行；按钮即 GitHub 入口。
          关掉后链接行补回 GitHub 按钮 */}
      {!starPromptDismissed && (
        <div className="flex flex-wrap items-center gap-x-4 gap-y-3 bg-action-soft py-3.5 ps-5 pe-3">
          <div className="flex min-w-0 flex-1 items-center gap-3">
            <Star
              aria-hidden="true"
              className="h-5 w-5 shrink-0 animate-[spin_6s_linear_infinite] fill-amber-500 text-amber-500 motion-reduce:animate-none"
            />
            <p className="min-w-0 text-body text-fg-1">
              {t("settings.starPrompt")}
            </p>
          </div>
          <div className="flex shrink-0 items-center gap-1">
            <Button
              type="button"
              variant="neutral"
              size="compact"
              onClick={handleOpenGithub}
            >
              <Github className="h-3.5 w-3.5" />
              {t("settings.starOnGithub")}
            </Button>
            <Button
              type="button"
              variant="quiet"
              size="icon-compact"
              onClick={handleDismissStarPrompt}
              aria-label={t("common.close")}
              title={t("common.close")}
            >
              <X className="h-3.5 w-3.5" />
            </Button>
          </div>
        </div>
      )}

      <WhatsNewDialog
        open={whatsNewOpen}
        onClose={() => setWhatsNewOpen(false)}
        entries={recentEntries}
      />
    </div>
  );
}
