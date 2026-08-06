import { useQuery } from "@tanstack/react-query";

import { ipc, isDesktopShell } from "@/shared/ipc/client";

export const systemKeys = {
  snapshot: ["system", "snapshot"] as const,
};

/**
 * Live machine stats. Polls only while something is mounted — TanStack stops
 * the interval on unmount, so leaving the Dashboard stops sampling. Three
 * seconds is fast enough to watch a build eat the CPU without making the
 * numbers jitter unreadably.
 */
export function useSystemSnapshot() {
  return useQuery({
    queryKey: systemKeys.snapshot,
    queryFn: ipc.systemSnapshot,
    enabled: isDesktopShell(),
    refetchInterval: 3000,
    // A snapshot is worthless the moment it lands; never serve a stale one.
    staleTime: 0,
  });
}
