import { useQuery } from "@tanstack/react-query";

import { isDesktopShell, ipc } from "@/shared/ipc/client";

export function useAppInfo() {
  return useQuery({
    queryKey: ["app-info"],
    queryFn: ipc.appInfo,
    enabled: isDesktopShell(),
    staleTime: Infinity,
  });
}

export function useModuleCommands() {
  return useQuery({
    queryKey: ["module-commands"],
    queryFn: ipc.commandsList,
    enabled: isDesktopShell(),
    staleTime: 60_000,
  });
}

/**
 * Backs the "go to symbol" dialog (Ctrl+T). `query` should already be
 * debounced/committed by the caller, same convention as `useFileSearch`.
 * Below two characters every symbol table would match, so the query never
 * runs — it would return noise, not a shortlist.
 */
export function useSymbolSearch(
  projectPath: string | undefined,
  query: string,
) {
  return useQuery({
    queryKey: ["index", "symbols", projectPath ?? "none", query],
    queryFn: () => ipc.indexFindSymbols(projectPath ?? "", query),
    enabled:
      isDesktopShell() && Boolean(projectPath) && query.trim().length >= 2,
  });
}
