import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { onKernelEvent } from "@/shared/ipc/client";
import { projectKeys } from "@/features/projects/hooks";
import { workspaceKeys } from "@/features/workspaces/hooks";

/**
 * Bridges the kernel event stream into the frontend: invalidates the right
 * queries and surfaces job failures. Mount exactly once, in the app shell.
 */
export function useKernelEventBridge() {
  const queryClient = useQueryClient();

  useEffect(() => {
    return onKernelEvent((event) => {
      switch (event.kind) {
        case "workspacesChanged":
          void queryClient.invalidateQueries({ queryKey: workspaceKeys.all });
          break;
        case "projectsChanged":
          void queryClient.invalidateQueries({
            queryKey: projectKeys.list(event.data.workspaceId),
          });
          break;
        case "settingsChanged":
          void queryClient.invalidateQueries({ queryKey: ["settings"] });
          break;
        case "jobUpdated":
          // Job outcomes surface as notifications; here we only refresh data.
          void queryClient.invalidateQueries({ queryKey: ["jobs"] });
          break;
        case "notificationAdded": {
          const { notification } = event.data;
          const options = { description: notification.body ?? undefined };
          if (notification.level === "error") {
            toast.error(notification.title, options);
          } else {
            toast.success(notification.title, options);
          }
          void queryClient.invalidateQueries({ queryKey: ["notifications"] });
          break;
        }
      }
    });
  }, [queryClient]);
}
