import { useState } from "react";
import { Container, Play, RefreshCw, ScrollText, Square } from "lucide-react";
import { toast } from "sonner";

import { useGitStore } from "@/features/git/store";
import { useProjects } from "@/features/projects/hooks";
import { useActiveWorkspace } from "@/features/workspaces/hooks";
import { isDesktopShell, type ComposeServiceStatus } from "@/shared/ipc/client";
import { cn } from "@/shared/lib/utils";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Card, CardContent } from "@/shared/ui/card";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Skeleton } from "@/shared/ui/skeleton";
import {
  isUnavailable,
  useComposeActions,
  useComposeDetect,
  useComposeStatus,
  useContainerActions,
  useContainerLogs,
  useContainers,
  useDockerPing,
  useImages,
} from "./hooks";

export function DockerPage() {
  const ping = useDockerPing();
  const available = ping.isSuccess;
  const { data: containers, isLoading } = useContainers(available);
  const { data: images } = useImages(available);
  const actions = useContainerActions();
  const [tab, setTab] = useState<"containers" | "images" | "compose">(
    "containers",
  );
  const [logsFor, setLogsFor] = useState<{ id: string; name: string } | null>(
    null,
  );

  const activeWorkspace = useActiveWorkspace();
  const { data: projects } = useProjects(activeWorkspace?.id);
  const selectedProjectId = useGitStore((s) => s.selectedProjectId);
  const project =
    projects?.find((p) => p.id === selectedProjectId) ?? projects?.[0] ?? null;
  const { data: composeFile } = useComposeDetect(project?.path ?? null);
  const composeStatus = useComposeStatus(composeFile ?? null, available);
  const composeActions = useComposeActions(composeFile ?? null);

  if (!isDesktopShell()) {
    return <Notice title="Docker needs the desktop shell" />;
  }

  if (ping.isError && isUnavailable(ping.error)) {
    return (
      <Notice title="Docker isn't running">
        Start Docker Desktop, then this page will connect automatically
        (checking every 10 seconds).
      </Notice>
    );
  }

  function run(
    mutation: { mutate: (id: string, opts: object) => void },
    id: string,
    verb: string,
  ) {
    mutation.mutate(id, {
      onSuccess: () => toast.success(`Container ${verb}`),
      onError: (error: unknown) => toast.error(String(error)),
    });
  }

  return (
    <div className="mx-auto max-w-6xl space-y-4 p-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold tracking-tight">Docker</h1>
          <p className="text-sm text-muted-foreground">
            {ping.data ??
              (ping.isError
                ? "Not connected"
                : "Looking for the Docker daemon…")}
          </p>
        </div>
        <div className="flex gap-1">
          <TabButton
            active={tab === "containers"}
            label={`Containers${containers ? ` (${containers.length})` : ""}`}
            onClick={() => setTab("containers")}
          />
          <TabButton
            active={tab === "images"}
            label={`Images${images ? ` (${images.length})` : ""}`}
            onClick={() => setTab("images")}
          />
          {composeFile && (
            <TabButton
              active={tab === "compose"}
              label={`Compose${composeStatus.data ? ` (${composeStatus.data.services.length})` : ""}`}
              onClick={() => setTab("compose")}
            />
          )}
        </div>
      </div>

      {/* "The daemon isn't running" already returned above, so what is left
          here is a ping still in flight or one that failed for some other
          reason. Both used to render nothing at all, under a header that said
          "Connecting…" and never stopped saying it. */}
      {!available &&
        (ping.isError ? (
          <Notice title="Couldn't reach Docker">
            <>
              Docker answered with something other than a version, so there is
              nothing to list. DevOS keeps retrying every 10 seconds.
              <span className="mt-2 block break-words rounded-md bg-card px-2.5 py-1.5 text-left font-mono text-[11px]">
                {String(ping.error)}
              </span>
            </>
          </Notice>
        ) : (
          <Notice title="Looking for Docker">
            Checking whether the Docker Engine is running on this machine.
          </Notice>
        ))}

      {isLoading && (
        <div className="space-y-2">
          <Skeleton className="h-14 w-full" />
          <Skeleton className="h-14 w-full" />
        </div>
      )}

      {tab === "containers" && containers && (
        <>
          {containers.length === 0 && (
            <Notice title="No containers">
              Nothing here yet — containers you create will show up
              automatically.
            </Notice>
          )}
          <ul className="space-y-2">
            {containers.map((c) => {
              const running = c.state === "running";
              return (
                <li key={c.id}>
                  <Card className="py-3">
                    <CardContent className="flex items-center gap-3 px-4">
                      <span
                        className={cn(
                          "size-2 shrink-0 rounded-full",
                          running
                            ? "bg-emerald-500"
                            : c.state === "paused"
                              ? "bg-yellow-500"
                              : "bg-zinc-500",
                        )}
                        title={c.state}
                      />
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-sm font-medium">{c.name}</p>
                        <p className="truncate text-xs text-muted-foreground">
                          {c.image} · {c.status}
                        </p>
                      </div>
                      {c.ports.length > 0 && (
                        <div className="hidden gap-1 lg:flex">
                          {c.ports.slice(0, 3).map((p) => (
                            <Badge
                              key={p}
                              variant="outline"
                              className="font-mono text-[10px]"
                            >
                              {p}
                            </Badge>
                          ))}
                        </div>
                      )}
                      <div className="flex gap-1">
                        {running ? (
                          <>
                            <IconAction
                              label="Stop"
                              icon={<Square className="size-3.5" />}
                              onClick={() => run(actions.stop, c.id, "stopped")}
                            />
                            <IconAction
                              label="Restart"
                              icon={<RefreshCw className="size-3.5" />}
                              onClick={() =>
                                run(actions.restart, c.id, "restarted")
                              }
                            />
                          </>
                        ) : (
                          <IconAction
                            label="Start"
                            icon={<Play className="size-3.5" />}
                            onClick={() => run(actions.start, c.id, "started")}
                          />
                        )}
                        <IconAction
                          label="Logs"
                          icon={<ScrollText className="size-3.5" />}
                          onClick={() => setLogsFor({ id: c.id, name: c.name })}
                        />
                      </div>
                    </CardContent>
                  </Card>
                </li>
              );
            })}
          </ul>
        </>
      )}

      {tab === "images" && images && (
        <>
          {images.length === 0 && <Notice title="No images" />}
          <ul className="space-y-2">
            {images.map((image) => (
              <li key={image.id}>
                <Card className="py-3">
                  <CardContent className="flex items-center gap-3 px-4">
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm font-medium">
                        {image.tags[0] ?? "(untagged)"}
                      </p>
                      <p className="font-mono text-xs text-muted-foreground">
                        {image.id}
                      </p>
                    </div>
                    <span className="text-xs text-muted-foreground">
                      {(image.size / (1024 * 1024)).toFixed(0)} MB
                    </span>
                  </CardContent>
                </Card>
              </li>
            ))}
          </ul>
        </>
      )}

      {tab === "compose" && composeFile && (
        <div className="space-y-3">
          <div className="flex items-center justify-between gap-4">
            <p className="min-w-0 truncate text-xs text-muted-foreground">
              {composeStatus.data?.project.name ?? "…"} — {composeFile}
            </p>
            <div className="flex shrink-0 gap-2">
              <Button
                size="sm"
                variant="outline"
                disabled={composeActions.up.isPending}
                onClick={() =>
                  composeActions.up.mutate(undefined, {
                    onSuccess: () => toast.success("Stack started"),
                    onError: (error: unknown) => toast.error(String(error)),
                  })
                }
              >
                <Play className="size-3.5" />
                Up
              </Button>
              <Button
                size="sm"
                variant="outline"
                disabled={composeActions.down.isPending}
                onClick={() =>
                  composeActions.down.mutate(undefined, {
                    onSuccess: () => toast.success("Stack stopped"),
                    onError: (error: unknown) => toast.error(String(error)),
                  })
                }
              >
                <Square className="size-3.5" />
                Down
              </Button>
            </div>
          </div>

          {!available && (
            <Notice title="Docker isn't running">
              Declared services are shown below with no live status. Start
              Docker Desktop to see which are running.
            </Notice>
          )}

          {composeStatus.data && (
            <ul className="space-y-2">
              {composeStatus.data.services.map((service) => (
                <ComposeServiceRow
                  key={service.service}
                  service={service}
                  onUp={() =>
                    composeActions.up.mutate(service.service, {
                      onSuccess: () => toast.success(`${service.service} up`),
                      onError: (error: unknown) => toast.error(String(error)),
                    })
                  }
                  onRestart={() =>
                    composeActions.restart.mutate(service.service, {
                      onSuccess: () =>
                        toast.success(`${service.service} restarted`),
                      onError: (error: unknown) => toast.error(String(error)),
                    })
                  }
                />
              ))}
            </ul>
          )}
        </div>
      )}

      <LogsDialog target={logsFor} onClose={() => setLogsFor(null)} />
    </div>
  );
}

function ComposeServiceRow({
  service,
  onUp,
  onRestart,
}: {
  service: ComposeServiceStatus;
  onUp: () => void;
  onRestart: () => void;
}) {
  const running = service.container?.state === "running";
  return (
    <li>
      <Card className="py-3">
        <CardContent className="flex items-center gap-3 px-4">
          <span
            className={cn(
              "size-2 shrink-0 rounded-full",
              running
                ? "bg-emerald-500"
                : service.container
                  ? "bg-zinc-500"
                  : "bg-zinc-700",
            )}
            title={service.container?.state ?? "not created"}
          />
          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-medium">{service.service}</p>
            <p className="truncate text-xs text-muted-foreground">
              {service.container
                ? `${service.container.image} · ${service.container.status}`
                : "not created yet"}
            </p>
          </div>
          <div className="flex gap-1">
            {running ? (
              <IconAction
                label="Restart"
                icon={<RefreshCw className="size-3.5" />}
                onClick={onRestart}
              />
            ) : (
              <IconAction
                label="Up"
                icon={<Play className="size-3.5" />}
                onClick={onUp}
              />
            )}
          </div>
        </CardContent>
      </Card>
    </li>
  );
}

function LogsDialog({
  target,
  onClose,
}: {
  target: { id: string; name: string } | null;
  onClose: () => void;
}) {
  const { data: logs, isLoading } = useContainerLogs(target?.id ?? null);
  return (
    <Dialog open={Boolean(target)} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle className="font-mono text-sm">
            {target?.name} — last 200 lines
          </DialogTitle>
        </DialogHeader>
        <pre className="max-h-[60vh] overflow-auto whitespace-pre-wrap break-all rounded-md bg-card p-3 font-mono text-xs">
          {isLoading ? "Loading…" : logs?.trim() || "(no output)"}
        </pre>
      </DialogContent>
    </Dialog>
  );
}

function IconAction({
  label,
  icon,
  onClick,
}: {
  label: string;
  icon: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <Button
      variant="ghost"
      size="icon"
      className="size-7"
      aria-label={label}
      title={label}
      onClick={onClick}
    >
      {icon}
    </Button>
  );
}

function TabButton({
  active,
  label,
  onClick,
}: {
  active: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "rounded-md px-3 py-1 text-sm",
        active
          ? "bg-accent text-foreground"
          : "text-muted-foreground hover:bg-accent/60",
      )}
    >
      {label}
    </button>
  );
}

function Notice({
  title,
  children,
}: {
  title: string;
  children?: React.ReactNode;
}) {
  return (
    <div className="mx-auto max-w-3xl py-6">
      <Card>
        <CardContent className="flex flex-col items-center gap-2 py-12 text-center">
          <Container className="size-6 text-muted-foreground" />
          <p className="font-medium">{title}</p>
          {children && (
            <div className="max-w-md text-sm text-muted-foreground">
              {children}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
