import { useQuery } from "@tanstack/react-query";

import { isDesktopShell, ipc } from "@/shared/ipc/client";

export const deployKeys = {
  /** Root — invalidated when the stored Vercel token changes. */
  all: ["deploy"] as const,
  configured: ["deploy", "configured"] as const,
  projects: ["deploy", "projects"] as const,
  deployments: (projectId: string) =>
    ["deploy", "deployments", projectId] as const,
};

/**
 * Whether a `vercel_token` secret exists at all. Local and cheap, and the
 * answer flips the moment the user saves a token in Settings — the secrets
 * form invalidates this key, so no stale "false" survives a fresh token.
 */
export function useDeployConfigured() {
  return useQuery({
    queryKey: deployKeys.configured,
    queryFn: ipc.deployConfigured,
    enabled: isDesktopShell(),
  });
}

/**
 * `enabled` is the configured flag: asking Vercel for projects with no token
 * would turn "you haven't set this up" into a network error.
 *
 * No retries. The likely failure here is a token Vercel refuses, and retrying
 * a rejected credential three times only delays the screen that tells the
 * user to replace it.
 */
export function useDeployProjects(enabled: boolean) {
  return useQuery({
    queryKey: deployKeys.projects,
    queryFn: ipc.deployProjects,
    enabled: isDesktopShell() && enabled,
    retry: false,
    staleTime: 60_000,
  });
}

/**
 * Deployments for the selected project. Polled, because a build that was
 * QUEUED when the pane opened becomes READY without anyone clicking anything.
 */
export function useDeployments(projectId: string | null) {
  return useQuery({
    queryKey: deployKeys.deployments(projectId ?? "none"),
    queryFn: () => ipc.deployList(projectId ?? ""),
    enabled: isDesktopShell() && Boolean(projectId),
    retry: false,
    refetchInterval: 30_000,
  });
}
