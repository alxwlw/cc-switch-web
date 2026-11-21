import { useMemo } from "react";
import { FolderSearch, Undo2 } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import { isWeb } from "@/lib/api/adapter";
import { useTranslation } from "react-i18next";

export interface DirectoryInputProps {
  label: string;
  description?: string;
  value?: string;
  resolvedValue: string;
  placeholder?: string;
  onChange: (value?: string) => void;
  onBrowse: () => Promise<void>;
  onReset: () => Promise<void>;
  /** Shown but not editable, and nothing to reset (a directory fixed by the server's env). */
  readOnly?: boolean;
}

export function DirectoryInput({
  label,
  description,
  value,
  resolvedValue,
  placeholder,
  onChange,
  onBrowse,
  onReset,
  readOnly = false,
}: DirectoryInputProps) {
  const { t } = useTranslation();
  const displayValue = useMemo(
    () => value ?? resolvedValue ?? "",
    [value, resolvedValue],
  );

  return (
    <div className="space-y-1.5">
      {label || description ? (
        <div className="space-y-1">
          {label ? (
            <p className="text-xs font-medium text-fg-1">{label}</p>
          ) : null}
          {description ? (
            <p className="text-xs text-fg-2">{description}</p>
          ) : null}
        </div>
      ) : null}
      <div className="flex items-center gap-2">
        <Input
          value={displayValue}
          placeholder={placeholder}
          className="text-xs"
          readOnly={readOnly}
          onChange={(event) => onChange(event.target.value)}
        />
        {/* Web: pick_directory is a native dialog on the server host, the path is typed (WEB-26). */}
        {!isWeb && !readOnly && (
          <HoverTip content={t("settings.browseDirectory")}>
            <Button
              type="button"
              variant="outline"
              size="icon"
              onClick={onBrowse}
              aria-label={t("settings.browseDirectory")}
            >
              <FolderSearch className="h-4 w-4" />
            </Button>
          </HoverTip>
        )}
        {!readOnly && (
          <HoverTip content={t("settings.resetDefault")}>
            <Button
              type="button"
              variant="outline"
              size="icon"
              onClick={onReset}
              aria-label={t("settings.resetDefault")}
            >
              <Undo2 className="h-4 w-4" />
            </Button>
          </HoverTip>
        )}
      </div>
    </div>
  );
}
