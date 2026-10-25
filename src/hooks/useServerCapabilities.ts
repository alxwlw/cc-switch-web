import { useQuery } from "@tanstack/react-query";
import { isWeb } from "@/lib/api/adapter";
import { fetchCapabilities } from "@/lib/api/capabilities";

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
