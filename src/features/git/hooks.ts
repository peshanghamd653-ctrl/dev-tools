import {
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";

import { isDesktopShell, ipc } from "@/shared/ipc/client";

export const gitKeys = {
  all: (path: string) => ["git", path] as const,
  status: (path: string) => ["git", path, "status"] as const,
  log: (path: string) => ["git", path, "log"] as const,
  branches: (path: string) => ["git", path, "branches"] as const,
  diff: (path: string, file: string, staged: boolean) =>
    ["git", path, "diff", file, staged] as const,
};

export function useGitStatus(path: string | undefined) {
  return useQuery({
    queryKey: gitKeys.status(path ?? "none"),
    queryFn: () => ipc.gitStatus(path ?? ""),
    enabled: isDesktopShell() && Boolean(path),
    refetchInterval: 4000,
  });
}

export function useGitLog(path: string | undefined) {
  return useQuery({
    queryKey: gitKeys.log(path ?? "none"),
    queryFn: () => ipc.gitLog(path ?? "", 50),
    enabled: isDesktopShell() && Boolean(path),
    staleTime: 10_000,
  });
}

export function useGitBranches(path: string | undefined) {
  return useQuery({
    queryKey: gitKeys.branches(path ?? "none"),
    queryFn: () => ipc.gitBranches(path ?? ""),
    enabled: isDesktopShell() && Boolean(path),
    staleTime: 10_000,
  });
}

export function useGitDiff(
  path: string | undefined,
  file: string | null,
  staged: boolean,
  untracked: boolean,
) {
  return useQuery({
    queryKey: gitKeys.diff(path ?? "none", file ?? "none", staged),
    queryFn: () => ipc.gitDiff(path ?? "", file ?? "", staged, untracked),
    enabled: isDesktopShell() && Boolean(path) && Boolean(file),
    refetchInterval: 4000,
  });
}

/** All git mutations for one repo, each invalidating the repo's queries. */
export function useGitMutations(path: string | undefined) {
  const queryClient = useQueryClient();
  const invalidate = () => {
    if (path) {
      void queryClient.invalidateQueries({ queryKey: gitKeys.all(path) });
    }
  };
  const repo = () => {
    if (!path) throw new Error("no repository selected");
    return path;
  };

  return {
    stage: useMutation({
      mutationFn: (files: string[]) => ipc.gitStage(repo(), files),
      onSettled: invalidate,
    }),
    unstage: useMutation({
      mutationFn: (files: string[]) => ipc.gitUnstage(repo(), files),
      onSettled: invalidate,
    }),
    discard: useMutation({
      mutationFn: (args: { file: string; untracked: boolean }) =>
        ipc.gitDiscard(repo(), args.file, args.untracked),
      onSettled: invalidate,
    }),
    commit: useMutation({
      mutationFn: (message: string) => ipc.gitCommit(repo(), message),
      onSettled: invalidate,
    }),
    switchBranch: useMutation({
      mutationFn: (args: { branch: string; create: boolean }) =>
        ipc.gitSwitch(repo(), args.branch, args.create),
      onSettled: invalidate,
    }),
    push: useMutation({
      mutationFn: () => ipc.gitPush(repo()),
      onSettled: invalidate,
    }),
    pull: useMutation({
      mutationFn: () => ipc.gitPull(repo()),
      onSettled: invalidate,
    }),
  };
}
