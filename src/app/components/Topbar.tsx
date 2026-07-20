import { Check, ChevronsUpDown, Command as CommandIcon, Plus } from "lucide-react";

import { NotificationBell } from "@/features/notifications/NotificationBell";
import {
  useActiveWorkspace,
  useWorkspaces,
} from "@/features/workspaces/hooks";
import { useDialogStore } from "@/shared/stores/dialogs";
import { useUiStore } from "@/shared/stores/ui";
import { Button } from "@/shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";

export function Topbar() {
  const { data: workspaces } = useWorkspaces();
  const activeWorkspace = useActiveWorkspace();
  const setActiveWorkspace = useUiStore((s) => s.setActiveWorkspace);
  const setPaletteOpen = useUiStore((s) => s.setPaletteOpen);
  const setCreateWorkspaceOpen = useDialogStore((s) => s.setCreateWorkspaceOpen);

  return (
    <header className="flex h-12 shrink-0 items-center gap-2 border-b px-3">
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="sm" className="gap-1.5 font-medium">
            {activeWorkspace?.name ?? "Workspace"}
            <ChevronsUpDown className="size-3.5 text-muted-foreground" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-56">
          <DropdownMenuLabel>Workspaces</DropdownMenuLabel>
          {workspaces?.map((workspace) => (
            <DropdownMenuItem
              key={workspace.id}
              onSelect={() => setActiveWorkspace(workspace.id)}
            >
              <span className="truncate">{workspace.name}</span>
              {workspace.id === activeWorkspace?.id && (
                <Check className="ml-auto size-4" />
              )}
            </DropdownMenuItem>
          ))}
          <DropdownMenuSeparator />
          <DropdownMenuItem onSelect={() => setCreateWorkspaceOpen(true)}>
            <Plus className="size-4" />
            Create workspace
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <div className="flex-1" />

      <NotificationBell />

      <Button
        variant="outline"
        size="sm"
        className="gap-2 text-muted-foreground"
        onClick={() => setPaletteOpen(true)}
      >
        <CommandIcon className="size-3.5" />
        <span className="text-xs">Search or run a command</span>
        <kbd className="rounded bg-muted px-1.5 py-0.5 text-[10px]">Ctrl K</kbd>
      </Button>
    </header>
  );
}
