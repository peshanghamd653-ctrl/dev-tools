import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { isDesktopShell, ipc } from "@/shared/ipc/client";

export const envFileKeys = {
  list: (projectPath: string) => ["envfiles", "list", projectPath] as const,
  entries: (projectPath: string, fileName: string) =>
    ["envfiles", "entries", projectPath, fileName] as const,
};

export function useEnvFileList(projectPath: string | null) {
  return useQuery({
    queryKey: envFileKeys.list(projectPath ?? "none"),
    queryFn: () => ipc.envFileList(projectPath ?? ""),
    enabled: isDesktopShell() && Boolean(projectPath),
  });
}

export function useEnvFileEntries(
  projectPath: string | null,
  fileName: string | null,
) {
  return useQuery({
    queryKey: envFileKeys.entries(projectPath ?? "none", fileName ?? "none"),
    queryFn: () => ipc.envFileRead(projectPath ?? "", fileName ?? ""),
    enabled: isDesktopShell() && Boolean(projectPath) && Boolean(fileName),
  });
}

/**
 * Both mutations invalidate the file list too, not just the entries: the
 * first `set` against a file name that didn't exist yet is how a new `.env`
 * gets created, and the list has to pick that up.
 */
function useEnvFileInvalidation(projectPath: string, fileName: string) {
  const queryClient = useQueryClient();
  return async () => {
    await queryClient.invalidateQueries({
      queryKey: envFileKeys.entries(projectPath, fileName),
    });
    await queryClient.invalidateQueries({
      queryKey: envFileKeys.list(projectPath),
    });
  };
}

export function useEnvFileSet(projectPath: string, fileName: string) {
  const invalidate = useEnvFileInvalidation(projectPath, fileName);
  return useMutation({
    mutationFn: ({ key, value }: { key: string; value: string }) =>
      ipc.envFileSet(projectPath, fileName, key, value),
    onSuccess: invalidate,
  });
}

export function useEnvFileDeleteKey(projectPath: string, fileName: string) {
  const invalidate = useEnvFileInvalidation(projectPath, fileName);
  return useMutation({
    mutationFn: (key: string) =>
      ipc.envFileDeleteKey(projectPath, fileName, key),
    onSuccess: invalidate,
  });
}
