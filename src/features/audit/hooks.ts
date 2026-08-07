import { useQuery } from "@tanstack/react-query";

import { ipc, isDesktopShell } from "@/shared/ipc/client";

/**
 * How many rows the viewer asks for.
 *
 * Below the backend's own cap on purpose: this is a "look when something went
 * wrong" screen, and 200 rows is already more than anyone scrolls. The count
 * that matters — how many entries exist — comes back regardless, so asking for
 * fewer costs nothing but a render.
 */
export const AUDIT_PAGE_SIZE = 200;

export const auditKeys = {
  log: ["audit-log", AUDIT_PAGE_SIZE] as const,
};

/**
 * The audit log.
 *
 * No polling and no event subscription: entries are written by actions the
 * user just performed elsewhere in the app, and a security record that
 * silently reorders itself while being read is worse than one that needs a
 * revisit. Remounting the Settings page refetches.
 */
export function useAuditLog() {
  return useQuery({
    queryKey: auditKeys.log,
    queryFn: () => ipc.auditLog(AUDIT_PAGE_SIZE),
    enabled: isDesktopShell(),
  });
}
