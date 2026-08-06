import { useMutation, useQuery } from "@tanstack/react-query";

import { useGitStore } from "@/features/git/store";
import { useProjects } from "@/features/projects/hooks";
import { useActiveWorkspace } from "@/features/workspaces/hooks";
import { inDesktopShell, ipc } from "@/shared/ipc/client";

export const issueKeys = {
  all: ["issues"] as const,
  configured: ["issues", "configured"] as const,
  targets: (projectPath: string) => ["issues", "targets", projectPath] as const,
};

/**
 * Whether a `github_token` secret exists at all. Only asked while the dialog
 * is open — this hook is mounted in the shell, and a query that runs at
 * startup for a feature nobody has invoked is a call nobody asked for. Query
 * data is stale by default, so re-opening the dialog after storing a token
 * gets the fresh answer without any cross-feature invalidation.
 */
export function useIssueConfigured(enabled: boolean) {
  return useQuery({
    queryKey: issueKeys.configured,
    queryFn: ipc.issueConfigured,
    enabled: inDesktopShell && enabled,
    retry: false,
  });
}

/**
 * GitHub repositories the project's git remotes point at. No retries: the
 * failures here are "this isn't a repo" and "the token is bad", neither of
 * which improves on a second attempt.
 */
export function useIssueTargets(projectPath: string | null) {
  return useQuery({
    queryKey: issueKeys.targets(projectPath ?? "none"),
    queryFn: () => ipc.issueTargets(projectPath ?? ""),
    enabled: inDesktopShell && Boolean(projectPath),
    retry: false,
    staleTime: 60_000,
  });
}

/** Takes the screenshot. A mutation, not a query: it has a side effect on disk. */
export function useIssueCapture() {
  return useMutation({ mutationFn: () => ipc.issueCapture() });
}

/** The one outward-facing write in this flow. Never retried automatically. */
export function useIssueCreate() {
  return useMutation({
    mutationFn: (input: {
      owner: string;
      name: string;
      title: string;
      body: string;
    }) => ipc.issueCreate(input.owner, input.name, input.title, input.body),
    retry: false,
  });
}

/**
 * The project the issue is filed about — the same one the command palette
 * acts on, so `Ctrl+Shift+S` and the palette entry can't disagree about which
 * repository is "current". Reading the git store here is one-directional;
 * nothing in `git` knows this feature exists.
 */
export function useActiveProject() {
  const workspace = useActiveWorkspace();
  const { data: projects } = useProjects(workspace?.id);
  const selectedId = useGitStore((s) => s.selectedProjectId);
  return projects?.find((p) => p.id === selectedId) ?? projects?.[0] ?? null;
}
