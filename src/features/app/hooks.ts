import { useQuery } from "@tanstack/react-query";

import { inDesktopShell, ipc } from "@/shared/ipc/client";

export function useAppInfo() {
  return useQuery({
    queryKey: ["app-info"],
    queryFn: ipc.appInfo,
    enabled: inDesktopShell,
    staleTime: Infinity,
  });
}

export function useModuleCommands() {
  return useQuery({
    queryKey: ["module-commands"],
    queryFn: ipc.commandsList,
    enabled: inDesktopShell,
    staleTime: 60_000,
  });
}
