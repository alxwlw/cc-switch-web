import { useQuery } from "@tanstack/react-query";
import { isWeb } from "@/lib/api/adapter";
import { fetchCapabilities } from "@/lib/api/capabilities";
import { isMac } from "@/lib/platform";

// WEB-00: server capabilities exist only behind the web API; no request on desktop.
export function useServerCapabilities() {
  return useQuery({
    queryKey: ["web", "capabilities"],
    queryFn: fetchCapabilities,
    enabled: isWeb,
    retry: false,
    staleTime: Infinity,
  });
}

/**
 * Antigravity account switching works only where agy keeps its login in the
 * macOS keychain. On desktop that is the local OS (`isMac()`); in web the
 * browser UA says nothing about the host, so ask the server's platform.
 */
export function useAgyAccountsAvailable(): boolean {
  const { data } = useServerCapabilities();
  return isWeb ? data?.platform === "macos" : isMac();
}
