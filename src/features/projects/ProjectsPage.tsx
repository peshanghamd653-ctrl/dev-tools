import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  FolderKanban,
  FolderOpen,
  FolderPlus,
  MoreHorizontal,
  ScanSearch,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";

import { ipc } from "@/shared/ipc/client";

import { useProjects, useRemoveProject } from "@/features/projects/hooks";
import { useActiveWorkspace } from "@/features/workspaces/hooks";
import { useDialogStore } from "@/shared/stores/dialogs";
import { Button } from "@/shared/ui/button";
import { Card, CardContent } from "@/shared/ui/card";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import { Skeleton } from "@/shared/ui/skeleton";

export function ProjectsPage() {
  const activeWorkspace = useActiveWorkspace();
  const { data: projects, isLoading } = useProjects(activeWorkspace?.id);
  const removeProject = useRemoveProject(activeWorkspace?.id);
  const setAddProjectOpen = useDialogStore((s) => s.setAddProjectOpen);

  return (
    <div className="mx-auto max-w-5xl space-y-6 p-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold tracking-tight">Projects</h1>
          <p className="text-sm text-muted-foreground">
            {activeWorkspace
              ? `Everything in ${activeWorkspace.name}`
              : "No workspace selected"}
          </p>
        </div>
        <Button onClick={() => setAddProjectOpen(true)}>
          <FolderPlus className="size-4" />
          Add project
        </Button>
      </div>

      {isLoading && (
        <div className="space-y-2">
          <Skeleton className="h-16 w-full" />
          <Skeleton className="h-16 w-full" />
        </div>
      )}

      {projects && projects.length === 0 && (
        <Card>
          <CardContent className="flex flex-col items-center gap-3 py-12 text-center">
            <div className="flex size-12 items-center justify-center rounded-full bg-muted">
              <FolderKanban className="size-6 text-muted-foreground" />
            </div>
            <div>
              <p className="font-medium">No projects yet</p>
              <p className="text-sm text-muted-foreground">
                Add an existing folder to start working with it in DevOS.
              </p>
            </div>
            <Button size="sm" onClick={() => setAddProjectOpen(true)}>
              <FolderPlus className="size-4" />
              Add your first project
            </Button>
          </CardContent>
        </Card>
      )}

      {projects && projects.length > 0 && (
        <ul className="space-y-2">
          {projects.map((project) => (
            <li key={project.id}>
              <Card className="py-3">
                <CardContent className="flex items-center gap-4 px-4">
                  <div className="flex size-9 shrink-0 items-center justify-center rounded-md bg-muted">
                    <FolderKanban className="size-4 text-muted-foreground" />
                  </div>
                  <div className="min-w-0 flex-1">
                    <p className="text-sm font-medium">{project.name}</p>
                    <p className="truncate font-mono text-[11px] text-muted-foreground">
                      {project.path}
                    </p>
                  </div>
                  <span className="hidden text-xs text-muted-foreground sm:block">
                    added {new Date(project.createdAt).toLocaleDateString()}
                  </span>
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="size-8"
                        aria-label={`Actions for ${project.name}`}
                      >
                        <MoreHorizontal className="size-4" />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem
                        onSelect={() => {
                          revealItemInDir(project.path).catch((error) =>
                            toast.error(`Could not open folder: ${error}`),
                          );
                        }}
                      >
                        <FolderOpen className="size-4" />
                        Reveal in Explorer
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        onSelect={() => {
                          ipc
                            .indexProject(project.path)
                            .then(() =>
                              toast.info(`Indexing "${project.name}"…`),
                            )
                            .catch((error) => toast.error(String(error)));
                        }}
                      >
                        <ScanSearch className="size-4" />
                        Index for AI search
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        variant="destructive"
                        onSelect={() =>
                          removeProject.mutate(project.id, {
                            onSuccess: () =>
                              toast.success(`Removed "${project.name}"`),
                            onError: (error) => toast.error(String(error)),
                          })
                        }
                      >
                        <Trash2 className="size-4" />
                        Remove from workspace
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </CardContent>
              </Card>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
