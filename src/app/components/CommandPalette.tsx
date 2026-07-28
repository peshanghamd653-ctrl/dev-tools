import { useNavigate } from "@tanstack/react-router";
import { FolderPlus, MonitorCog, Plus } from "lucide-react";
import { toast } from "sonner";

import { primaryNav } from "@/app/nav";
import { useModuleCommands } from "@/features/app/hooks";
import { useGitStore } from "@/features/git/store";
import { useProjects } from "@/features/projects/hooks";
import { useActiveWorkspace } from "@/features/workspaces/hooks";
import { ipc } from "@/shared/ipc/client";
import { useDialogStore } from "@/shared/stores/dialogs";
import { useUiStore } from "@/shared/stores/ui";
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
  CommandShortcut,
} from "@/shared/ui/command";

/** Module command ids the shell knows how to execute today. */
const moduleCommandHandlers: Record<
  string,
  | "createWorkspace"
  | "addProject"
  | "newTerminal"
  | "openGit"
  | "openAi"
  | "indexProject"
  | "openDocker"
  | "openApi"
> = {
  "core.workspace.create": "createWorkspace",
  "core.project.add": "addProject",
  "terminal.new": "newTerminal",
  "git.open": "openGit",
  "ai.open": "openAi",
  "index.project": "indexProject",
  "docker.open": "openDocker",
  "api.open": "openApi",
};

export function CommandPalette() {
  const open = useUiStore((s) => s.paletteOpen);
  const setOpen = useUiStore((s) => s.setPaletteOpen);
  const setCreateWorkspaceOpen = useDialogStore((s) => s.setCreateWorkspaceOpen);
  const setAddProjectOpen = useDialogStore((s) => s.setAddProjectOpen);
  const navigate = useNavigate();
  const { data: moduleCommands } = useModuleCommands();
  const activeWorkspace = useActiveWorkspace();
  const { data: projects } = useProjects(activeWorkspace?.id);
  const selectedProjectId = useGitStore((s) => s.selectedProjectId);
  const paletteProject =
    projects?.find((p) => p.id === selectedProjectId) ?? projects?.[0] ?? null;

  function runAndClose(action: () => void) {
    setOpen(false);
    action();
  }

  function runModuleCommand(id: string) {
    const handler = moduleCommandHandlers[id];
    if (handler === "createWorkspace") setCreateWorkspaceOpen(true);
    else if (handler === "addProject") setAddProjectOpen(true);
    else if (handler === "openGit") void navigate({ to: "/git" });
    else if (handler === "openAi") void navigate({ to: "/ai" });
    else if (handler === "openDocker") void navigate({ to: "/docker" });
    else if (handler === "openApi") void navigate({ to: "/api" });
    else if (handler === "indexProject") {
      if (!paletteProject) {
        toast.error("Add a project first");
      } else {
        void ipc
          .indexProject(paletteProject.path)
          .then(() => toast.info(`Indexing "${paletteProject.name}"…`))
          .catch((error) => toast.error(String(error)));
      }
    }
    else if (handler === "newTerminal") {
      void navigate({ to: "/terminal" });
      void import("@/features/terminal/session").then((m) =>
        m.createTerminalSession().catch((error) =>
          toast.error(`Could not start terminal: ${error}`),
        ),
      );
    } else toast.info(`Command "${id}" is not wired up yet`);
  }

  return (
    <CommandDialog
      open={open}
      onOpenChange={setOpen}
      title="Command palette"
      description="Search pages and run commands"
    >
      <CommandInput placeholder="Search pages and commands…" />
      <CommandList>
        <CommandEmpty>No results found.</CommandEmpty>

        <CommandGroup heading="Navigation">
          {primaryNav.map((item) => (
            <CommandItem
              key={item.to}
              onSelect={() =>
                runAndClose(() => void navigate({ to: item.to }))
              }
            >
              <item.icon className="size-4" />
              {item.label}
              {item.shortcut && (
                <CommandShortcut>{item.shortcut}</CommandShortcut>
              )}
            </CommandItem>
          ))}
        </CommandGroup>

        <CommandSeparator />

        <CommandGroup heading="Actions">
          <CommandItem
            keywords={["workspace", "new"]}
            onSelect={() => runAndClose(() => setCreateWorkspaceOpen(true))}
          >
            <Plus className="size-4" />
            Create workspace
          </CommandItem>
          <CommandItem
            keywords={["project", "add", "open"]}
            onSelect={() => runAndClose(() => setAddProjectOpen(true))}
          >
            <FolderPlus className="size-4" />
            Add project
          </CommandItem>
        </CommandGroup>

        {moduleCommands && moduleCommands.length > 0 && (
          <>
            <CommandSeparator />
            <CommandGroup heading="Modules">
              {moduleCommands.map((command) => (
                <CommandItem
                  key={command.id}
                  keywords={command.keywords}
                  onSelect={() =>
                    runAndClose(() => runModuleCommand(command.id))
                  }
                >
                  <MonitorCog className="size-4" />
                  {command.title}
                  <span className="ml-auto text-[10px] text-muted-foreground uppercase">
                    {command.module}
                  </span>
                </CommandItem>
              ))}
            </CommandGroup>
          </>
        )}
      </CommandList>
    </CommandDialog>
  );
}
