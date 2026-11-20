import { useTranslation } from "react-i18next";
import { AlertTriangle } from "lucide-react";

export interface InitErrorScreenProps {
  payload: { path?: string; error?: string } | null;
}

/**
 * WEB-32: web replacement for the desktop "config load failed → dialog → exit"
 * flow. The server stays up in init-error mode, so the page explains the
 * problem instead of rendering the app against a server that answers 503.
 * Nothing to exit: the operator fixes the server host and restarts it.
 */
export function InitErrorScreen({ payload }: InitErrorScreenProps) {
  const { t } = useTranslation();
  const path = payload?.path ?? "~/.cc-switch/config.json";
  const detail = payload?.error ?? "Unknown error";
  return (
    <div
      role="alert"
      className="mx-auto flex min-h-screen max-w-2xl flex-col justify-center gap-4 px-4 py-8"
    >
      <div className="flex items-center gap-2 text-fg-1">
        <AlertTriangle className="h-5 w-5 shrink-0 text-red-500" aria-hidden />
        <h1 className="text-lg font-semibold">
          {t("errors.configLoadFailedTitle", {
            defaultValue: "配置加载失败",
          })}
        </h1>
      </div>
      <p className="whitespace-pre-wrap break-words text-body text-fg-2">
        {t("web.initError.message", { path, detail })}
      </p>
    </div>
  );
}
