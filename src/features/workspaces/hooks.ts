import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";

import { isDesktopShell, ipc } from "@/shared/ipc/client";
import { useUiStore } from "@/shared/stores/ui";

export const workspaceKeys = {
  all: ["workspaces"] as const,
};

export function useWorkspaces() {
  return useQuery({
    queryKey: workspaceKeys.all,
    queryFn: ipc.workspacesList,
    enabled: isDesktopShell(),
  });
}

/** The active workspace, falling back to the first one available. */
export function useActiveWorkspace() {
  const { data: workspaces } = useWorkspaces();
  const activeId = useUiStore((s) => s.activeWorkspaceId);
  return (
    workspaces?.find((w) => w.id === activeId) ?? workspaces?.[0] ?? null
  );
}

export function useCreateWorkspace() {
  const queryClient = useQueryClient();
  const setActiveWorkspace = useUiStore((s) => s.setActiveWorkspace);
  return useMutation({
    mutationFn: (name: string) => ipc.workspaceCreate(name),
    onSuccess: (workspace) => {
      setActiveWorkspace(workspace.id);
      void queryClient.invalidateQueries({ queryKey: workspaceKeys.all });
    },
  });
}

export function useRenameWorkspace() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) =>
      ipc.workspaceRename(id, name),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: workspaceKeys.all }),
  });
}

export function useDeleteWorkspace() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.workspaceDelete(id),
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: workspaceKeys.all }),
  });
}
