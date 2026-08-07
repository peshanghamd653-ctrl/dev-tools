import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { ipc, isDesktopShell } from "@/shared/ipc/client";

export const backupKeys = {
  /** The folder, its contents, and any staged restore — one query, one call. */
  listing: ["backups"] as const,
};

export function useBackups() {
  return useQuery({
    queryKey: backupKeys.listing,
    queryFn: ipc.backupsList,
    enabled: isDesktopShell(),
  });
}

/**
 * Stage a restore for the next boot.
 *
 * Invalidating on success is not cosmetic: the card switches from a list of
 * candidates to a "restart to apply" banner, and a list that still offers
 * restore buttons after one has been staged invites a second, contradictory
 * request.
 */
export function useStageRestore() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => ipc.backupRestoreStage(name),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: backupKeys.listing }),
  });
}

export function useCancelRestore() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => ipc.backupRestoreCancel(),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: backupKeys.listing }),
  });
}
