import type { ReactNode } from "react";
import { Toaster } from "sonner";

import { CommandPalette } from "@/app/components/CommandPalette";
import { Sidebar } from "@/app/components/Sidebar";
import { Topbar } from "@/app/components/Topbar";
import { AddProjectDialog } from "@/app/components/dialogs/AddProjectDialog";
import { CreateWorkspaceDialog } from "@/app/components/dialogs/CreateWorkspaceDialog";
import { CaptureIssueDialog } from "@/features/issues/CaptureIssueDialog";
import { useGlobalHotkeys } from "@/shared/hooks/useGlobalHotkeys";
import { useKernelEventBridge } from "@/shared/hooks/useKernelEvents";
import { TooltipProvider } from "@/shared/ui/tooltip";

export function AppShell({ children }: { children: ReactNode }) {
  useKernelEventBridge();
  useGlobalHotkeys();

  return (
    <TooltipProvider delayDuration={200}>
      <div className="flex h-screen w-screen overflow-hidden">
        <Sidebar />
        <div className="flex min-w-0 flex-1 flex-col">
          <Topbar />
          <main className="min-w-0 flex-1 overflow-y-auto">{children}</main>
        </div>
      </div>
      <CommandPalette />
      <CreateWorkspaceDialog />
      <AddProjectDialog />
      <CaptureIssueDialog />
      <Toaster theme="dark" position="bottom-right" richColors />
    </TooltipProvider>
  );
}
