import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { isDesktopShell, ipc } from "@/shared/ipc/client";

export const mcpKeys = {
  list: ["mcp", "servers"] as const,
};

export function useMcpServers() {
  return useQuery({
    queryKey: mcpKeys.list,
    queryFn: ipc.mcpServers,
    enabled: isDesktopShell(),
  });
}

export function useMcpServerCreate() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      name,
      command,
      args,
    }: {
      name: string;
      command: string;
      args: string[];
    }) => ipc.mcpServerCreate(name, command, args),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: mcpKeys.list }),
  });
}

export function useMcpServerDelete() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.mcpServerDelete(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: mcpKeys.list }),
  });
}

/**
 * Not a `useQuery` — discovery is a one-shot action the user asks for by
 * clicking a button, not data the page loads and keeps fresh. It spawns a
 * real process, so it belongs behind an explicit click either way.
 */
export function useMcpDiscoverTools() {
  return useMutation({
    mutationFn: ({ command, args }: { command: string; args: string[] }) =>
      ipc.mcpDiscoverTools(command, args),
  });
}
