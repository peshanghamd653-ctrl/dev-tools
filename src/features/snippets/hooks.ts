import {
  keepPreviousData,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";

import { ipc, isDesktopShell, type SnippetDraft } from "@/shared/ipc/client";

export const snippetKeys = {
  all: ["snippets"] as const,
  list: (query: string) => ["snippets", "list", query] as const,
};

/**
 * One query for both the full list and a search — the backend returns the
 * whole list for a blank query, so there is no second code path here and no
 * moment where the page is showing a list built by different rules than the
 * one it will show next.
 *
 * `keepPreviousData` is what stops the pane flashing empty between keystrokes:
 * the previous results stay on screen, marked stale, until the new ones land.
 */
export function useSnippets(query: string) {
  const trimmed = query.trim();
  return useQuery({
    queryKey: snippetKeys.list(trimmed),
    queryFn: () => ipc.snippetsSearch(trimmed),
    enabled: isDesktopShell(),
    placeholderData: keepPreviousData,
  });
}

export function useSnippetSave() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (draft: SnippetDraft) => ipc.snippetSave(draft),
    // Every cached query key, not just the current one: a save changes
    // `updated_at`, which changes the order of every filtered list too.
    onSuccess: () => queryClient.invalidateQueries({ queryKey: snippetKeys.all }),
  });
}

export function useSnippetDelete() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => ipc.snippetDelete(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: snippetKeys.all }),
  });
}
