import { useMutation } from "@tanstack/react-query";

import { ipc } from "@/shared/ipc/client";

/**
 * Not a `useQuery` — a scan spawns real processes (`cargo audit`,
 * `npm`/`pnpm audit`) and walks the whole project tree, so it runs when the
 * user asks for it, not on a poll or on every mount.
 */
export function useSecurityScan() {
  return useMutation({
    mutationFn: (projectPath: string) => ipc.securityScan(projectPath),
  });
}
