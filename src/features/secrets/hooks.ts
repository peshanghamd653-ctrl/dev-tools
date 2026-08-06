import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { deployKeys } from "@/features/deploy/hooks";
import { isDesktopShell, ipc } from "@/shared/ipc/client";

export const secretKeys = {
  /**
   * Deliberately the same key the AI page uses (`aiKeys.secrets`): adding or
   * deleting a key here has to move that page's setup card in the same tick,
   * not after a reload.
   */
  list: ["secrets"] as const,
};

/** Names only. There is no query that returns a value — see docs/security.md. */
export function useSecretNames() {
  return useQuery({
    queryKey: secretKeys.list,
    queryFn: ipc.secretList,
    enabled: isDesktopShell(),
  });
}

/**
 * Both mutations also invalidate the deploy module: whether a Vercel token
 * exists is the difference between the deployments page showing a setup
 * screen and showing projects, and that must not lag behind this form.
 */
function useSecretInvalidation() {
  const queryClient = useQueryClient();
  return async () => {
    await queryClient.invalidateQueries({ queryKey: secretKeys.list });
    await queryClient.invalidateQueries({ queryKey: deployKeys.all });
  };
}

export function useSecretSet() {
  const invalidate = useSecretInvalidation();
  return useMutation({
    mutationFn: ({ name, value }: { name: string; value: string }) =>
      ipc.secretSet(name, value),
    onSuccess: invalidate,
  });
}

export function useSecretDelete() {
  const invalidate = useSecretInvalidation();
  return useMutation({
    mutationFn: (name: string) => ipc.secretDelete(name),
    onSuccess: invalidate,
  });
}
