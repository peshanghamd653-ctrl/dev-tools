import { useQuery } from "@tanstack/react-query";

import { ipc, isDesktopShell } from "@/shared/ipc/client";

export const systemKeys = {
  snapshot: ["system", "snapshot"] as const,
  history: ["system", "history"] as const,
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

/**
 * The profiler's chart data: every sample the in-process scheduler has
 * recorded (30s apart, ~3h retention — see `devos_system::scheduler`).
 * Refetching every 30s matches the scheduler's own cadence; anything faster
 * would just re-request the same rows.
 */
export function useSystemHistory() {
  return useQuery({
    queryKey: systemKeys.history,
    queryFn: ipc.systemHistory,
    enabled: isDesktopShell(),
    refetchInterval: 30_000,
  });
}
