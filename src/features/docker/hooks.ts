import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";

import { isDesktopShell, ipc } from "@/shared/ipc/client";

export const dockerKeys = {
  ping: ["docker", "ping"] as const,
  containers: ["docker", "containers"] as const,
  images: ["docker", "images"] as const,
  logs: (id: string) => ["docker", "logs", id] as const,
};

export function isUnavailable(error: unknown): boolean {
  return String(error).includes("unavailable:");
}

export function useDockerPing() {
  return useQuery({
    queryKey: dockerKeys.ping,
    queryFn: ipc.dockerPing,
    enabled: isDesktopShell(),
    retry: false,
    refetchInterval: (query) => (query.state.status === "error" ? 10_000 : 60_000),
  });
}

export function useContainers(available: boolean) {
  return useQuery({
    queryKey: dockerKeys.containers,
    queryFn: ipc.dockerContainers,
    enabled: isDesktopShell() && available,
    refetchInterval: 5000,
  });
}

export function useImages(available: boolean) {
  return useQuery({
    queryKey: dockerKeys.images,
    queryFn: ipc.dockerImages,
    enabled: isDesktopShell() && available,
    staleTime: 30_000,
  });
}

export function useContainerLogs(id: string | null) {
  return useQuery({
    queryKey: dockerKeys.logs(id ?? "none"),
    queryFn: () => ipc.dockerLogs(id ?? ""),
    enabled: isDesktopShell() && Boolean(id),
  });
}

export function useContainerActions() {
  const queryClient = useQueryClient();
  const invalidate = () =>
    queryClient.invalidateQueries({ queryKey: dockerKeys.containers });

  return {
    start: useMutation({
      mutationFn: (id: string) => ipc.dockerStart(id),
      onSettled: invalidate,
    }),
    stop: useMutation({
      mutationFn: (id: string) => ipc.dockerStop(id),
      onSettled: invalidate,
    }),
    restart: useMutation({
      mutationFn: (id: string) => ipc.dockerRestart(id),
      onSettled: invalidate,
    }),
  };
}
