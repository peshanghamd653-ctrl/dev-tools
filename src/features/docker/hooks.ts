import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { isDesktopShell, ipc } from "@/shared/ipc/client";

export const dockerKeys = {
  ping: ["docker", "ping"] as const,
  containers: ["docker", "containers"] as const,
  images: ["docker", "images"] as const,
  logs: (id: string) => ["docker", "logs", id] as const,
};

export const composeKeys = {
  detect: (projectPath: string) =>
    ["docker", "compose", "detect", projectPath] as const,
  status: (composeFile: string) =>
    ["docker", "compose", "status", composeFile] as const,
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
    refetchInterval: (query) =>
      query.state.status === "error" ? 10_000 : 60_000,
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

/** The first compose file found in the project, or `null` if there is none. */
export function useComposeDetect(projectPath: string | null) {
  return useQuery({
    queryKey: composeKeys.detect(projectPath ?? "none"),
    queryFn: () => ipc.dockerComposeDetect(projectPath ?? ""),
    enabled: isDesktopShell() && Boolean(projectPath),
  });
}

/**
 * Declared services and their live status. `available` gates this on
 * `useDockerPing` succeeding the same way `useContainers` does — but unlike
 * containers, a compose file's *declared* roster (`useComposeDetect`) is
 * still worth showing with the daemon down, so only this query, not
 * detection, is gated.
 */
export function useComposeStatus(
  composeFile: string | null,
  available: boolean,
) {
  return useQuery({
    queryKey: composeKeys.status(composeFile ?? "none"),
    queryFn: () => ipc.dockerComposeStatus(composeFile ?? ""),
    enabled: isDesktopShell() && Boolean(composeFile) && available,
    refetchInterval: 5000,
  });
}

export function useComposeActions(composeFile: string | null) {
  const queryClient = useQueryClient();
  const invalidate = () =>
    queryClient.invalidateQueries({
      queryKey: composeKeys.status(composeFile ?? "none"),
    });

  return {
    up: useMutation({
      mutationFn: (service?: string) =>
        ipc.dockerComposeUp(composeFile ?? "", service),
      onSettled: invalidate,
    }),
    down: useMutation({
      mutationFn: () => ipc.dockerComposeDown(composeFile ?? ""),
      onSettled: invalidate,
    }),
    restart: useMutation({
      mutationFn: (service?: string) =>
        ipc.dockerComposeRestart(composeFile ?? "", service),
      onSettled: invalidate,
    }),
  };
}
