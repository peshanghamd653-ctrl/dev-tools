import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";

import { inDesktopShell, ipc } from "@/shared/ipc/client";

export const projectKeys = {
  list: (workspaceId: string) => ["projects", workspaceId] as const,
};

export function useProjects(workspaceId: string | undefined) {
  return useQuery({
    queryKey: projectKeys.list(workspaceId ?? "none"),
    queryFn: () => ipc.projectsList(workspaceId ?? ""),
    enabled: inDesktopShell && Boolean(workspaceId),
  });
}

export function useAddProject(workspaceId: string | undefined) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ name, path }: { name: string; path: string }) => {
      if (!workspaceId) throw new Error("no active workspace");
      return ipc.projectAdd(workspaceId, name, path);
    },
    onSuccess: () => {
      if (workspaceId) {
        void queryClient.invalidateQueries({
          queryKey: projectKeys.list(workspaceId),
        });
      }
    },
  });
}

export function useRemoveProject(workspaceId: string | undefined) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => {
      if (!workspaceId) throw new Error("no active workspace");
      return ipc.projectRemove(id, workspaceId);
    },
    onSuccess: () => {
      if (workspaceId) {
        void queryClient.invalidateQueries({
          queryKey: projectKeys.list(workspaceId),
        });
      }
    },
  });
}
