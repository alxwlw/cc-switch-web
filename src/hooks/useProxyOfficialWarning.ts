import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { useTauriEvent } from "@/hooks/useTauriEvent";

/** `proxy-official-warning` (desktop Tauri event, web SSE): toast when takeover starts on an official provider. */
export function useProxyOfficialWarning(): void {
  const { t } = useTranslation();
  useTauriEvent<{ appType: string; providerName: string }>(
    "proxy-official-warning",
    (payload) => {
      toast.warning(
        t("notifications.proxyOfficialWarning", {
          name: payload.providerName,
          defaultValue: `当前供应商 ${payload.providerName} 是官方供应商，建议切换到第三方供应商后再使用代理接管`,
        }),
        { duration: 8000 },
      );
    },
  );
}
