import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { isDesktopShell, ipc } from "@/shared/ipc/client";

export const monitorKeys = {
  list: ["monitors", "list"] as const,
};

/**
 * The list is the whole page. A background scheduler records checks without
 * the UI asking, so polling is the only way the rows stay true — ten seconds
 * is well under the 60s minimum interval, so no check goes unseen for long.
 */
export function useMonitors() {
  return useQuery({
    queryKey: monitorKeys.list,
    queryFn: ipc.monitorsList,
    enabled: isDesktopShell(),
    refetchInterval: 10_000,
  });
}

export function useMonitorCreate() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      name,
      url,
      intervalSecs,
    }: {
      name: string;
      url: string;
      intervalSecs: number;
    }) => ipc.monitorCreate(name, url, intervalSecs),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: monitorKeys.list }),
  });
}

export function useMonitorDelete() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.monitorDelete(id),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: monitorKeys.list }),
  });
}

export function useMonitorToggle() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      ipc.monitorToggle(id, enabled),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: monitorKeys.list }),
  });
}

/**
 * An on-demand probe. Settled rather than success: a check that comes back
 * "down" is a recorded result, not a failed call, and the row must refresh
 * either way.
 */
export function useMonitorCheckNow() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.monitorCheckNow(id),
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: monitorKeys.list }),
  });
}
