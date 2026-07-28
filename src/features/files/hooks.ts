import { useQuery } from "@tanstack/react-query";

import { inDesktopShell, ipc } from "@/shared/ipc/client";

export const fileKeys = {
  dir: (project: string, relative: string) =>
    ["fs", "dir", project, relative] as const,
  file: (project: string, relative: string) =>
    ["fs", "file", project, relative] as const,
  find: (project: string, query: string) =>
    ["fs", "find", project, query] as const,
  content: (project: string, query: string) =>
    ["fs", "content", project, query] as const,
};

export function useDirEntries(
  projectPath: string | undefined,
  relative: string,
  enabled = true,
) {
  return useQuery({
    queryKey: fileKeys.dir(projectPath ?? "none", relative),
    queryFn: () => ipc.fsListDir(projectPath ?? "", relative),
    enabled: inDesktopShell && Boolean(projectPath) && enabled,
    staleTime: 10_000,
  });
}

export function useFilePreview(
  projectPath: string | undefined,
  relative: string | null,
) {
  return useQuery({
    queryKey: fileKeys.file(projectPath ?? "none", relative ?? "none"),
    queryFn: () => ipc.fsReadFile(projectPath ?? "", relative ?? ""),
    enabled: inDesktopShell && Boolean(projectPath) && Boolean(relative),
  });
}

/** Both search flavors; `query` should already be debounced/committed. */
export function useFileSearch(projectPath: string | undefined, query: string) {
  const active = inDesktopShell && Boolean(projectPath) && query.length >= 2;
  const names = useQuery({
    queryKey: fileKeys.find(projectPath ?? "none", query),
    queryFn: () => ipc.fsFind(projectPath ?? "", query),
    enabled: active,
  });
  const content = useQuery({
    queryKey: fileKeys.content(projectPath ?? "none", query),
    queryFn: () => ipc.indexSearch(projectPath ?? "", query),
    enabled: active,
    retry: false,
  });
  return { names, content, active };
}
