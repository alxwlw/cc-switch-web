import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { useTauriEvent } from "./useTauriEvent";

interface SyncStatusUpdatedPayload {
  source?: string;
  status?: string;
  error?: string;
}

/**
 * 云同步（WebDAV / S3）每次写完状态后端都会 emit `<backend>-sync-status-updated`：
 * 刷新设置缓存（页面上的「上次同步」/ 错误提示），后台自动同步失败时弹 toast。
 * 桌面端走 Tauri 事件，web 端走 SSE shim（WEB-11），载荷相同。
 */
export function useSyncStatusEvents() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  useTauriEvent<SyncStatusUpdatedPayload | null | undefined>(
    "webdav-sync-status-updated",
    async (payload) => {
      const statusPayload = payload ?? {};
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      if (statusPayload.source !== "auto" || statusPayload.status !== "error") {
        return;
      }
      toast.error(
        t("settings.webdavSync.autoSyncFailedToast", {
          error: statusPayload.error || t("common.unknown"),
        }),
      );
    },
  );

  useTauriEvent<SyncStatusUpdatedPayload | null | undefined>(
    "s3-sync-status-updated",
    async (payload) => {
      const statusPayload = payload ?? {};
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      if (statusPayload.source !== "auto" || statusPayload.status !== "error") {
        return;
      }
      toast.error(
        t("settings.s3Sync.autoSyncFailedToast", {
          error: statusPayload.error || t("common.unknown"),
        }),
      );
    },
  );
}
