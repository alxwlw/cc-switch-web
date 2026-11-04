import { useTranslation } from "react-i18next";
import type { AppConfigDirStatus } from "@/lib/api/settings";

/**
 * Web only (WEB-26): which data directory this server process uses and why, who may change it,
 * and what a change moves. Nothing here promises that everything moves: the database and the
 * logs follow the directory, settings.json and live-state.json stay in ~/.cc-switch.
 */
export function AppConfigDirNotes({
  status,
}: {
  status: AppConfigDirStatus | null;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-1 text-caption text-fg-2">
      {status && (
        <p data-testid="app-config-dir-current" className="break-all">
          {t("web.dataDir.current", {
            path: status.effective,
            source: t(`web.dataDir.source.${status.source}`),
          })}
        </p>
      )}
      {status?.envLocked && (
        <p role="note" data-testid="app-config-dir-locked">
          {t("web.dataDir.envLocked")}
        </p>
      )}
      <p data-testid="app-config-dir-scope">{t("web.dataDir.scope")}</p>
    </div>
  );
}
