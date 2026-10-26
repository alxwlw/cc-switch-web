import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Download, Loader2, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  SERVER_LOG_DOWNLOAD_URL,
  settingsApi,
  type ServerLogTail,
} from "@/lib/api/settings";

const TAIL_LINES = 500;

interface ServerLogViewerProps {
  open: boolean;
  onClose: () => void;
}

/** Web only: tail of the log file of the machine running cc-switch-web (not the browser's). */
export function ServerLogViewer({ open, onClose }: ServerLogViewerProps) {
  const { t } = useTranslation();
  const [tail, setTail] = useState<ServerLogTail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setTail(await settingsApi.getServerLogTail(TAIL_LINES));
      setError(null);
    } catch (e) {
      setTail(null);
      // A missing file answers 404 with a message that already names the path.
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (open) void load();
  }, [open, load]);

  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="max-w-3xl">
        <DialogHeader>
          <DialogTitle>
            {t("settings.advanced.logConfig.viewer.title")}
          </DialogTitle>
          <DialogDescription>
            {t("settings.advanced.logConfig.viewer.serverNote")}
          </DialogDescription>
        </DialogHeader>
        <div className="flex min-h-0 flex-1 flex-col gap-3 px-6 pb-5">
          {tail && (
            <div className="break-all font-mono text-caption text-fg-2">
              {t("settings.advanced.logConfig.viewer.path")}: {tail.path}
            </div>
          )}
          {error ? (
            <div
              role="status"
              className="rounded-panel border border-dashed border-border px-4 py-3 text-caption text-fg-2"
            >
              <p>{t("settings.advanced.logConfig.viewer.unavailable")}</p>
              <p className="mt-1 break-all font-mono">{error}</p>
            </div>
          ) : (
            <pre
              aria-label={t("settings.advanced.logConfig.viewer.title")}
              className="max-h-[50vh] min-h-[10rem] overflow-auto whitespace-pre rounded-panel border border-border bg-subtle p-3 font-mono text-caption text-fg-1"
            >
              {tail
                ? tail.lines.length > 0
                  ? tail.lines.join("\n")
                  : t("settings.advanced.logConfig.viewer.empty")
                : ""}
            </pre>
          )}
          <div className="flex items-center justify-end gap-2">
            <Button
              variant="neutral"
              size="compact"
              disabled={loading}
              onClick={() => void load()}
            >
              {loading ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <RefreshCw className="h-3.5 w-3.5" />
              )}
              {t("settings.advanced.logConfig.viewer.refresh")}
            </Button>
            <Button variant="neutral" size="compact" asChild>
              <a href={SERVER_LOG_DOWNLOAD_URL} download>
                <Download className="h-3.5 w-3.5" />
                {t("settings.advanced.logConfig.viewer.download")}
              </a>
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
