import { useState } from "react";
import { Server, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { isWeb } from "@/lib/api/adapter";
import { useServerCapabilities } from "@/hooks/useServerCapabilities";

const DISMISS_KEY = "web_host_banner_dismissed";

function readDismissed(): boolean {
  try {
    return localStorage.getItem(DISMISS_KEY) === "1";
  } catch {
    return false;
  }
}

/**
 * WEB-00: permanent web UI, no WEB-NN parity task lifts it. The statement is
 * true in web regardless of whether capabilities loaded, so it never waits for them.
 */
export function WebHostBanner() {
  const { t } = useTranslation();
  const [dismissed, setDismissed] = useState(readDismissed);
  const caps = useServerCapabilities();

  if (!isWeb || dismissed) return null;

  const dismiss = () => {
    setDismissed(true);
    try {
      localStorage.setItem(DISMISS_KEY, "1");
    } catch {
      // storage unavailable: dismissal lasts until reload
    }
  };

  return (
    <div
      role="note"
      title={caps.data?.runtime_boundaries || undefined}
      className="flex items-center gap-2 border-b border-border bg-surface px-4 py-1.5 text-caption text-fg-2"
    >
      <Server className="h-3.5 w-3.5 shrink-0" />
      <span className="min-w-0 flex-1">{t("web.hostNotice")}</span>
      <button
        type="button"
        onClick={dismiss}
        aria-label={t("web.hostNoticeDismiss")}
        className="shrink-0 rounded p-0.5 hover:text-fg-1"
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}
