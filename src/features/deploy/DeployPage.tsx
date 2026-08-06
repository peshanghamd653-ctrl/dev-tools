import { useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ExternalLink,
  KeyRound,
  RefreshCw,
  Rocket,
  ShieldAlert,
  TriangleAlert,
} from "lucide-react";
import { toast } from "sonner";

import { isDesktopShell, type Deployment } from "@/shared/ipc/client";
import { cn } from "@/shared/lib/utils";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Card, CardContent } from "@/shared/ui/card";
import { Skeleton } from "@/shared/ui/skeleton";
import { useDeployConfigured, useDeployProjects, useDeployments } from "./hooks";
import {
  classifyDeployError,
  commitSummary,
  deploymentHref,
  deployState,
  deployStateLabel,
  displayUrl,
  sortDeployments,
  sortProjects,
  targetLabel,
  timeAgo,
  type DeployState,
} from "./state";

/** The secret DevOS reads to talk to Vercel. Named here so the copy can say it. */
const TOKEN_SECRET = "vercel_token";

export function DeployPage() {
  const configured = useDeployConfigured();
  const projects = useDeployProjects(configured.data === true);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const ordered = projects.data ? sortProjects(projects.data) : [];
  // Derived, not stored: the first project is selected until the user picks
  // another, and a project that disappears from Vercel falls back rather than
  // leaving the pane pointed at nothing.
  const selected =
    ordered.find((project) => project.id === selectedId) ?? ordered[0] ?? null;
  const deployments = useDeployments(selected?.id ?? null);

  if (!isDesktopShell()) {
    return (
      <Notice title="Deployments need the desktop shell">
        The Vercel token lives in the encrypted secret store inside the DevOS
        process — a browser tab has nothing to read it with.
      </Notice>
    );
  }

  if (configured.isLoading) {
    return (
      <div className="mx-auto max-w-3xl space-y-2 p-6">
        <Skeleton className="h-20 w-full rounded-xl" />
        <Skeleton className="h-20 w-full rounded-xl" />
      </div>
    );
  }

  if (configured.isError) {
    return (
      <Notice title="The deployments module didn't answer">
        {String(configured.error)}
      </Notice>
    );
  }

  if (configured.data === false) {
    return <NotConfigured />;
  }

  if (projects.isError) {
    const failure = classifyDeployError(projects.error);
    if (failure === "auth") return <TokenRejected detail={projects.error} />;
    if (failure === "unconfigured") return <NotConfigured />;
    return (
      <Notice title="Couldn't load your Vercel projects">
        {String(projects.error)}
      </Notice>
    );
  }

  return (
    <div className="grid h-full grid-cols-[280px_1fr]">
      <aside className="flex min-h-0 flex-col border-r">
        <div className="flex h-11 shrink-0 items-center gap-2 border-b px-3">
          <Rocket className="size-3.5 text-muted-foreground" />
          <p className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
            Projects
          </p>
          {ordered.length > 0 && (
            <span className="ml-auto text-xs tabular-nums text-muted-foreground">
              {ordered.length}
            </span>
          )}
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {projects.isLoading && (
            <div className="space-y-1.5 p-1">
              <Skeleton className="h-10 w-full" />
              <Skeleton className="h-10 w-full" />
              <Skeleton className="h-10 w-full" />
            </div>
          )}

          {projects.data && ordered.length === 0 && (
            <p className="px-2 py-6 text-center text-xs text-muted-foreground">
              This token can see no projects. Check that it belongs to the right
              Vercel team.
            </p>
          )}

          <ul className="space-y-0.5">
            {ordered.map((project) => {
              const active = project.id === selected?.id;
              return (
                <li key={project.id}>
                  <button
                    type="button"
                    aria-current={active ? "true" : undefined}
                    onClick={() => setSelectedId(project.id)}
                    className={cn(
                      "w-full rounded-md px-2 py-1.5 text-left",
                      active ? "bg-accent" : "hover:bg-accent/60",
                    )}
                  >
                    <p className="truncate text-xs font-medium">
                      {project.name}
                    </p>
                    <p className="flex items-center gap-1.5 pt-0.5 text-[10px] text-muted-foreground">
                      {project.framework && (
                        <Badge
                          variant="outline"
                          className="h-4 shrink-0 px-1 text-[9px] font-normal text-muted-foreground"
                        >
                          {project.framework}
                        </Badge>
                      )}
                      <span className="truncate">
                        {timeAgo(project.updatedAt)}
                      </span>
                    </p>
                  </button>
                </li>
              );
            })}
          </ul>
        </div>
      </aside>

      <section className="flex min-h-0 flex-col">
        <header className="flex h-11 shrink-0 items-center gap-2 border-b px-4">
          <div className="min-w-0">
            <p className="truncate text-sm font-medium">
              {selected?.name ?? "Deployments"}
            </p>
          </div>
          <p className="ml-auto shrink-0 text-[11px] text-muted-foreground">
            Read-only view of Vercel
          </p>
          <Button
            variant="ghost"
            size="icon"
            className="size-7 shrink-0"
            aria-label="Refresh deployments"
            title="Refresh"
            disabled={deployments.isFetching || !selected}
            onClick={() => void deployments.refetch()}
          >
            <RefreshCw
              className={cn(
                "size-3.5",
                deployments.isFetching && "animate-spin",
              )}
            />
          </Button>
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto p-4">
          <DeploymentList
            projectSelected={Boolean(selected)}
            loading={deployments.isLoading}
            error={deployments.error}
            deployments={deployments.data}
          />
        </div>
      </section>
    </div>
  );
}

function DeploymentList({
  projectSelected,
  loading,
  error,
  deployments,
}: {
  projectSelected: boolean;
  loading: boolean;
  error: unknown;
  deployments: Deployment[] | undefined;
}) {
  if (!projectSelected) {
    return (
      <p className="py-12 text-center text-sm text-muted-foreground">
        Select a project to see its deployments.
      </p>
    );
  }

  if (loading) {
    return (
      <div className="space-y-2">
        <Skeleton className="h-16 w-full rounded-xl" />
        <Skeleton className="h-16 w-full rounded-xl" />
        <Skeleton className="h-16 w-full rounded-xl" />
      </div>
    );
  }

  if (error) {
    // The token can be revoked between the project list and this call.
    const rejected = classifyDeployError(error) === "auth";
    return (
      <p className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-red-300">
        <TriangleAlert className="mt-px size-3.5 shrink-0" />
        <span className="min-w-0 break-words">
          {rejected
            ? `Vercel rejected the stored ${TOKEN_SECRET}. Replace it in Settings → Secrets.`
            : String(error)}
        </span>
      </p>
    );
  }

  if (!deployments || deployments.length === 0) {
    return (
      <p className="py-12 text-center text-sm text-muted-foreground">
        No deployments recorded for this project yet.
      </p>
    );
  }

  return (
    <ul className="space-y-2">
      {sortDeployments(deployments).map((deployment) => (
        <li key={deployment.id}>
          <DeploymentRow deployment={deployment} />
        </li>
      ))}
    </ul>
  );
}

function DeploymentRow({ deployment }: { deployment: Deployment }) {
  const commit = commitSummary(deployment.commitMessage);

  return (
    <Card className="gap-0 py-3">
      <CardContent className="flex items-center gap-3 px-4">
        <StateChip raw={deployment.state} />

        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <DeploymentLink url={deployment.url} />
            <Badge
              variant="outline"
              className="h-4 shrink-0 px-1 text-[9px] font-normal text-muted-foreground"
            >
              {targetLabel(deployment.target)}
            </Badge>
          </div>
          <p className="truncate pt-0.5 text-[11px] text-muted-foreground">
            {timeAgo(deployment.createdAt)}
            {/* Most deployments carry no commit message; the line reads fine
                without one rather than leaving an empty slot behind. */}
            {commit && (
              <>
                {" · "}
                <span title={deployment.commitMessage ?? undefined}>
                  {commit}
                </span>
              </>
            )}
          </p>
        </div>
      </CardContent>
    </Card>
  );
}

/**
 * A real anchor, so the URL can be copied and read from the status line — but
 * the click is intercepted and handed to the OS. Following it inside the
 * webview would replace DevOS with the deployed site and leave no way back.
 */
function DeploymentLink({ url }: { url: string }) {
  const href = deploymentHref(url);
  const text = displayUrl(url);

  if (!href) {
    return (
      <span
        className="truncate font-mono text-xs text-muted-foreground"
        title="Vercel returned a URL DevOS won't open"
      >
        {text || "(no URL)"}
      </span>
    );
  }

  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      title={`Open ${href} in your browser`}
      onClick={(event) => {
        event.preventDefault();
        void openUrl(href).catch((failure) =>
          toast.error(`Could not open your browser: ${failure}`),
        );
      }}
      className="inline-flex min-w-0 items-center gap-1 truncate font-mono text-xs hover:underline"
    >
      <span className="truncate">{text}</span>
      <ExternalLink className="size-3 shrink-0 text-muted-foreground" />
    </a>
  );
}

/**
 * State is spelled out, never colour alone — and an unrecognised state gets
 * the neutral chip with Vercel's own wording instead of disappearing.
 */
function StateChip({ raw }: { raw: string }) {
  const state = deployState(raw);
  const style: Record<DeployState, string> = {
    ready: "border-emerald-500/40 bg-emerald-500/10 text-emerald-300",
    error: "border-destructive/50 bg-destructive/15 text-red-300",
    building: "border-sky-500/40 bg-sky-500/10 text-sky-300",
    queued: "border-yellow-500/40 bg-yellow-500/10 text-yellow-200",
    canceled: "border-border bg-muted/40 text-muted-foreground",
    unknown: "border-border text-muted-foreground",
  };

  return (
    <span
      className={cn(
        "inline-flex w-[6.5rem] shrink-0 items-center gap-1.5 rounded-full border px-2 py-0.5 text-[11px]",
        style[state],
      )}
    >
      <StateGlyph state={state} />
      <span className="truncate">{deployStateLabel(raw)}</span>
    </span>
  );
}

function StateGlyph({ state }: { state: DeployState }) {
  if (state === "building") {
    return (
      <span className="relative flex size-2 shrink-0">
        <span className="absolute inline-flex size-full animate-ping rounded-full bg-sky-400 opacity-75" />
        <span className="relative inline-flex size-2 rounded-full bg-sky-400" />
      </span>
    );
  }
  if (state === "error") {
    return <span className="size-2 shrink-0 rounded-[1px] bg-red-500" />;
  }
  if (state === "ready") {
    return <span className="size-2 shrink-0 rounded-full bg-emerald-500" />;
  }
  if (state === "queued") {
    return (
      <span className="size-2 shrink-0 rounded-full border border-yellow-400" />
    );
  }
  return (
    <span className="size-2 shrink-0 rounded-full border border-dashed border-current" />
  );
}

/**
 * No token has ever been stored. Deliberately a different screen from
 * {@link TokenRejected}: this one asks for a token, that one asks for a
 * *different* token, and telling a user to "add" a credential they already
 * added is the fastest way to lose ten minutes.
 */
function NotConfigured() {
  const navigate = useNavigate();

  return (
    <Notice
      icon={<KeyRound className="size-6 text-muted-foreground" />}
      title="Connect Vercel to see deployments"
    >
      <>
        DevOS reads your projects and deployments through the Vercel API, so it
        needs a personal access token. Store one in Settings → Secrets under the
        name <code className="font-mono text-foreground">{TOKEN_SECRET}</code>{" "}
        and this page fills in.
        <span className="mt-2 block">
          The page only ever reads: it lists what Vercel already has and never
          triggers, promotes, or removes a deployment.
        </span>
        <Button
          size="sm"
          className="mt-4"
          onClick={() => void navigate({ to: "/settings" })}
        >
          Open Settings
        </Button>
      </>
    </Notice>
  );
}

/** A token is stored, and Vercel refused it. */
function TokenRejected({ detail }: { detail: unknown }) {
  const navigate = useNavigate();

  return (
    <Notice
      icon={<ShieldAlert className="size-6 text-yellow-300" />}
      title="Vercel rejected your token"
    >
      <>
        A secret named{" "}
        <code className="font-mono text-foreground">{TOKEN_SECRET}</code> is
        stored, but Vercel refused it — it has most likely been revoked or has
        expired, or it belongs to a team without access to these projects.
        <span className="mt-2 block">
          Create a new token and save it in Settings → Secrets under the same
          name; saving over an existing name replaces the stored value.
        </span>
        <span className="mt-3 block break-words rounded-md bg-card px-2.5 py-1.5 text-left font-mono text-[11px] text-muted-foreground">
          {String(detail)}
        </span>
        <Button
          size="sm"
          className="mt-4"
          onClick={() => void navigate({ to: "/settings" })}
        >
          Replace the token
        </Button>
      </>
    </Notice>
  );
}

function Notice({
  title,
  icon,
  children,
}: {
  title: string;
  icon?: React.ReactNode;
  children?: React.ReactNode;
}) {
  return (
    <div className="mx-auto max-w-2xl py-6">
      <Card>
        <CardContent className="flex flex-col items-center gap-2 py-12 text-center">
          {icon ?? <Rocket className="size-6 text-muted-foreground" />}
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
