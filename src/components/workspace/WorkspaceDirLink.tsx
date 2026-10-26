import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Copy, FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import { isWeb } from "@/lib/api/adapter";
import { workspaceApi } from "@/lib/api/workspace";
import { copyText } from "@/lib/clipboard";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";

interface WorkspaceDirLinkProps {
  subdir: "workspace" | "memory";
  /** Path shown on desktop (and in web until the server path arrives). */
  label: string;
  className?: string;
}

/**
 * Desktop: clickable path that opens the directory in the file manager.
 * Web: there is no file manager on the server, so show the server-side path
 * (`workspace-paths`) with a copy button instead.
 */
const WorkspaceDirLink: React.FC<WorkspaceDirLinkProps> = ({
  subdir,
  label,
  className,
}) => {
  const { t } = useTranslation();
  const [serverPath, setServerPath] = useState<string | null>(null);

  useEffect(() => {
    if (!isWeb) return;
    let cancelled = false;
    workspaceApi
      .getPaths()
      .then((paths) => {
        if (!cancelled) setServerPath(paths[subdir]);
      })
      .catch(() => {
        // keep the generic label
      });
    return () => {
      cancelled = true;
    };
  }, [subdir]);

  if (!isWeb) {
    return (
      <HoverTip content={t("workspace.openDirectory")}>
        <p
          className={cn(
            "text-sm text-fg-2 cursor-pointer hover:text-fg-1 transition-colors inline-flex items-center gap-1",
            className,
          )}
          onClick={() => workspaceApi.openDirectory(subdir)}
        >
          {label}
          <FolderOpen className="w-3.5 h-3.5" />
        </p>
      </HoverTip>
    );
  }

  const shown = serverPath ?? label;
  const handleCopy = async () => {
    try {
      await copyText(shown);
      toast.success(t("workspace.pathCopied"));
    } catch {
      toast.error(t("workspace.pathCopyFailed"));
    }
  };

  return (
    <span
      className={cn(
        "text-sm text-fg-2 inline-flex items-center gap-1",
        className,
      )}
    >
      <span className="break-all">{shown}</span>
      <Button
        variant="ghost"
        size="icon"
        className="h-6 w-6"
        aria-label={t("workspace.copyPath")}
        onClick={handleCopy}
      >
        <Copy className="w-3.5 h-3.5" />
      </Button>
    </span>
  );
};

export default WorkspaceDirLink;
