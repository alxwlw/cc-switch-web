import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { FileUp, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { deeplinkApi } from "@/lib/api/deeplink";
import { isWeb } from "@/lib/api/adapter";
import {
  DEEPLINK_FILE_LIMIT_BYTES,
  extractDeepLink,
  openDeepLinkRequest,
} from "@/lib/deeplinkOpen";

function readText(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result ?? ""));
    reader.onerror = () => reject(reader.error);
    reader.readAsText(file);
  });
}

interface DeepLinkPasteEntryProps {
  /** Called after the import dialog was handed a parsed request (e.g. to close a surrounding panel). */
  onOpened?: () => void;
}

/** Web-only: paste a ccswitch:// link or load a .txt/.url file holding one, then open the import dialog. */
export function DeepLinkPasteEntry({ onOpened }: DeepLinkPasteEntryProps) {
  const { t } = useTranslation();
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);

  if (!isWeb) return null;

  const submit = async (raw: string) => {
    const url = extractDeepLink(raw);
    if (!url) {
      setError(t("deeplink.paste.noLink"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const request = await deeplinkApi.parseDeeplink(url);
      openDeepLinkRequest(request);
      setValue("");
      onOpened?.();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const onFile = async (file: File | undefined) => {
    if (!file) return;
    if (file.size > DEEPLINK_FILE_LIMIT_BYTES) {
      setError(
        t("deeplink.paste.fileTooLarge", {
          limit: DEEPLINK_FILE_LIMIT_BYTES / 1024,
        }),
      );
      return;
    }
    let text: string;
    try {
      text = await readText(file);
    } catch {
      setError(t("deeplink.paste.fileReadFailed"));
      return;
    }
    await submit(text);
  };

  return (
    <div className="space-y-2">
      <div className="text-sm font-medium text-fg-1">
        {t("deeplink.paste.title")}
      </div>
      <form
        className="flex items-center gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          void submit(value);
        }}
      >
        <Input
          value={value}
          onChange={(e) => {
            setValue(e.target.value);
            setError(null);
          }}
          placeholder={t("deeplink.paste.placeholder")}
          aria-label={t("deeplink.paste.title")}
          aria-invalid={error ? true : undefined}
          spellCheck={false}
          autoComplete="off"
        />
        <Button type="submit" disabled={busy || !value.trim()}>
          {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
          {t("deeplink.paste.open")}
        </Button>
        <Button
          type="button"
          variant="outline"
          disabled={busy}
          onClick={() => fileRef.current?.click()}
        >
          <FileUp className="h-3.5 w-3.5" />
          {t("deeplink.paste.upload")}
        </Button>
        <input
          ref={fileRef}
          type="file"
          accept=".txt,.url"
          className="hidden"
          data-testid="deeplink-file-input"
          onChange={(e) => {
            const file = e.target.files?.[0];
            e.target.value = "";
            void onFile(file);
          }}
        />
      </form>
      {error && (
        <p role="alert" className="text-sm text-danger-text">
          {error}
        </p>
      )}
    </div>
  );
}
