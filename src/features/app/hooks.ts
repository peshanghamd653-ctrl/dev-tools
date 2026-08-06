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
