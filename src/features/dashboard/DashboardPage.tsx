import { Link } from "@tanstack/react-router";
import { ArrowRight, FolderKanban, FolderPlus, Gauge, Layers, Plus } from "lucide-react";

import { useAppInfo } from "@/features/app/hooks";
import { useProjects } from "@/features/projects/hooks";
import { SystemMetrics } from "@/features/system/SystemMetrics";
import {
  useActiveWorkspace,
  useWorkspaces,
} from "@/features/workspaces/hooks";
import { isDesktopShell } from "@/shared/ipc/client";
import { useDialogStore } from "@/shared/stores/dialogs";
import { Button } from "@/shared/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/shared/ui/card";

export function DashboardPage() {
  const activeWorkspace = useActiveWorkspace();
  const { data: workspaces } = useWorkspaces();
  const { data: projects } = useProjects(activeWorkspace?.id);
  const { data: appInfo } = useAppInfo();
  const setAddProjectOpen = useDialogStore((s) => s.setAddProjectOpen);
  const setCreateWorkspaceOpen = useDialogStore((s) => s.setCreateWorkspaceOpen);

  const recentProjects = projects?.slice(0, 5) ?? [];
  /**
   * A first-run install has nothing to say about recent projects and a great
   * deal to say about how to get one, so the two cards move above the machine
   * strip until there is a project. CPU and disk are context for work already
   * under way; they are not the first thing to read on an empty install.
   */
  const emptyWorkspace = projects?.length === 0;

  const projectCards = (
    <div className="grid gap-4 lg:grid-cols-2">
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Recent projects</CardTitle>
          <CardDescription>
            Most recently updated in this workspace
          </CardDescription>
        </CardHeader>
        <CardContent>
          {recentProjects.length === 0 ? (
            <div className="flex flex-col items-start gap-3 py-2">
              <p className="text-sm text-muted-foreground">
                No projects yet. Add your first one to get started.
              </p>
              <p className="text-xs text-muted-foreground">
                A project is just a folder on this machine. Once one is
                registered, Terminal, Git, Files and the AI assistant all work
                against it.
              </p>
              <Button size="sm" onClick={() => setAddProjectOpen(true)}>
                <FolderPlus className="size-4" />
                Add project
              </Button>
            </div>
          ) : (
            <ul className="space-y-1">
              {recentProjects.map((project) => (
                <li
                  key={project.id}
                  className="flex items-baseline justify-between gap-3 rounded-md px-2 py-1.5 hover:bg-accent"
                >
                  <span className="text-sm font-medium">{project.name}</span>
                  <span className="truncate font-mono text-[11px] text-muted-foreground">
                    {project.path}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Quick actions</CardTitle>
          <CardDescription>Everything is one keystroke away</CardDescription>
        </CardHeader>
        <CardContent className="space-y-2">
          <QuickAction
            label="Add a project"
            hint="Register an existing folder"
            onClick={() => setAddProjectOpen(true)}
            icon={<FolderPlus className="size-4" />}
          />
          <QuickAction
            label="Create a workspace"
            hint="Separate context for other work"
            onClick={() => setCreateWorkspaceOpen(true)}
            icon={<Plus className="size-4" />}
          />
          <Link
            to="/projects"
            className="flex items-center gap-3 rounded-md border px-3 py-2.5 text-sm hover:bg-accent"
          >
            <FolderKanban className="size-4 text-muted-foreground" />
            <span className="flex-1">
              Browse projects
              <span className="block text-xs text-muted-foreground">
                Manage everything in this workspace
              </span>
            </span>
            <ArrowRight className="size-4 text-muted-foreground" />
          </Link>
        </CardContent>
      </Card>
    </div>
  );

  return (
    <div className="mx-auto max-w-5xl space-y-6 p-6">
      <div>
        <h1 className="text-xl font-semibold tracking-tight">
          {activeWorkspace ? activeWorkspace.name : "Dashboard"}
        </h1>
        <p className="text-sm text-muted-foreground">
          {isDesktopShell()
            ? "Your development operating center."
            : "Running outside the desktop shell — data is unavailable."}
        </p>
      </div>

      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <StatCard
          icon={<FolderKanban className="size-4" />}
          label="Projects"
          value={projects ? String(projects.length) : "—"}
        />
        <StatCard
          icon={<Layers className="size-4" />}
          label="Workspaces"
          value={workspaces ? String(workspaces.length) : "—"}
        />
        <StatCard
          icon={<Gauge className="size-4" />}
          label="Kernel startup"
          value={appInfo ? `${appInfo.startupMs} ms` : "—"}
        />
        <StatCard
          icon={<span className="text-xs font-semibold">v</span>}
          label="Version"
          value={appInfo?.version ?? "—"}
        />
      </div>

      {emptyWorkspace && projectCards}

      <SystemMetrics />

      {!emptyWorkspace && projectCards}
    </div>
  );
}

function StatCard({
  icon,
  label,
  value,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
}) {
  return (
    <Card className="gap-2 py-4">
      <CardContent className="flex items-center gap-3 px-4">
        <div className="flex size-8 items-center justify-center rounded-md bg-muted text-muted-foreground">
          {icon}
        </div>
        <div className="min-w-0">
          <p className="text-lg font-semibold leading-tight">{value}</p>
          <p className="text-xs text-muted-foreground">{label}</p>
        </div>
      </CardContent>
    </Card>
  );
}

function QuickAction({
  label,
  hint,
  icon,
  onClick,
}: {
  label: string;
  hint: string;
  icon: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex w-full items-center gap-3 rounded-md border px-3 py-2.5 text-left text-sm hover:bg-accent"
    >
      <span className="text-muted-foreground">{icon}</span>
      <span className="flex-1">
        {label}
        <span className="block text-xs text-muted-foreground">{hint}</span>
      </span>
      <ArrowRight className="size-4 text-muted-foreground" />
    </button>
  );
}
