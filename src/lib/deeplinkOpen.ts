import type { DeepLinkImportRequest } from "@/lib/api/deeplink";

/** Window event that opens `DeepLinkImportDialog` from a request that is already parsed (web paste/upload). */
export const OPEN_DEEPLINK_EVENT = "web-deeplink-open";

export function openDeepLinkRequest(request: DeepLinkImportRequest): void {
  window.dispatchEvent(
    new CustomEvent<DeepLinkImportRequest>(OPEN_DEEPLINK_EVENT, {
      detail: request,
    }),
  );
}

/** Deep links with a base64 config are tens of KiB at most; the cap is checked before the file is read. */
export const DEEPLINK_FILE_LIMIT_BYTES = 256 * 1024;

/** First `ccswitch://` token in the text of a .txt or Windows .url (INI) file. */
export function extractDeepLink(text: string): string | null {
  const match = /ccswitch:\/\/\S+/i.exec(text);
  return match ? match[0].trim() : null;
}
