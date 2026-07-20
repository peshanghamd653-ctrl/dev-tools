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
        case "jobUpdated": {
          const { job } = event.data;
          if (job.status === "failed") {
            toast.error(`${job.module}: ${job.kind} failed`, {
              description: job.error ?? undefined,
            });
          } else if (job.status === "succeeded" && job.module === "index") {
            toast.success("Project index updated");
          }
          void queryClient.invalidateQueries({ queryKey: ["jobs"] });
          break;
        }
        case "notificationAdded": {
          const { level, title, body } = event.data;
          if (level === "error") toast.error(title, { description: body });
          else toast(title, { description: body });
          break;
        }
      }
    });
  }, [queryClient]);
}
