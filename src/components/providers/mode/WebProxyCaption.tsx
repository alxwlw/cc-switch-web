import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";

/** Web only: the proxy and its takeover live on the server host, not on the browser's machine. */
export function WebProxyCaption({ className }: { className?: string }) {
  const { t } = useTranslation();
  return (
    <p
      data-testid="web-proxy-caption"
      className={cn("m-0 text-caption text-fg-2", className)}
    >
      {t("web.proxyCaption")}
    </p>
  );
}
