import { useTranslation } from "react-i18next";
import { isWeb } from "@/lib/api/adapter";
import { useServerCapabilities } from "@/hooks/useServerCapabilities";

/** WEB-00: permanent web UI (About section), no WEB-NN parity task lifts it. */
export function WebModeInfo() {
  const { t } = useTranslation();
  const caps = useServerCapabilities();
  if (!isWeb) return null;
  const data = caps.data;
  return (
    <div className="space-y-1 px-5 py-4 text-caption text-fg-2">
      <div className="text-body font-medium text-fg-1">
        {t("web.mode.title")}
      </div>
      <p>{t("web.hostNotice")}</p>
      {data && (
        <>
          <p className="tabular-nums">{`${t("web.mode.serverVersion")} v${data.version}`}</p>
          {data.runtime_boundaries && <p>{data.runtime_boundaries}</p>}
        </>
      )}
    </div>
  );
}
