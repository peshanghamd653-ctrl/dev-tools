import { useState } from "react";
import {
  Activity,
  Pause,
  Play,
  Plus,
  RefreshCw,
  Trash2,
  TriangleAlert,
} from "lucide-react";
import { toast } from "sonner";

import {
  inDesktopShell,
  type MonitorCheck,
  type MonitorStatus,
} from "@/shared/ipc/client";
import { cn } from "@/shared/lib/utils";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Card, CardContent } from "@/shared/ui/card";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { Label } from "@/shared/ui/label";
import { Skeleton } from "@/shared/ui/skeleton";
import {
  useMonitorCheckNow,
  useMonitorCreate,
  useMonitorDelete,
  useMonitorToggle,
  useMonitors,
} from "./hooks";
import {
  describeCheck,
  failureReason,
  formatInterval,
  formatLatency,
  formatUptimePct,
  INTERVAL_CHOICES,
  isFetchableUrl,
  monitorState,
  sparklineChecks,
  STATE_LABEL,
  timeAgo,
  type MonitorState,
} from "./status";

/** Bars in the history strip. Enough to make a flapping site obvious. */
const SPARKLINE_LENGTH = 30;

export function MonitorsPage() {
  const { data: monitors, isLoading } = useMonitors();
  const create = useMonitorCreate();
  const [addOpen, setAddOpen] = useState(false);

  if (!inDesktopShell) {
    return (
      <EmptyCard title="Uptime monitoring needs the desktop shell">
        Checks run in a background scheduler inside the DevOS process — there is
        nothing to poll from a browser tab.
      </EmptyCard>
    );
  }

  const counts = summarise(monitors);

  return (
    <div className="mx-auto max-w-5xl space-y-4 p-6">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold tracking-tight">Monitors</h1>
          <p className="text-sm text-muted-foreground">
            {monitors && monitors.length > 0
              ? [
                  `${counts.up} up`,
                  counts.down > 0 ? `${counts.down} down` : null,
                  counts.paused > 0 ? `${counts.paused} paused` : null,
                  counts.pending > 0 ? `${counts.pending} awaiting first check` : null,
                ]
                  .filter(Boolean)
                  .join(" · ")
              : "Watch a site and get told when it stops answering."}
          </p>
        </div>
        <Button size="sm" onClick={() => setAddOpen(true)}>
          <Plus className="size-4" />
          Add monitor
        </Button>
      </div>

      {isLoading && (
        <div className="space-y-2">
          <Skeleton className="h-20 w-full rounded-xl" />
          <Skeleton className="h-20 w-full rounded-xl" />
        </div>
      )}

      {monitors && monitors.length === 0 && (
        <EmptyCard title="Nothing is being watched yet">
          Add a URL and DevOS checks it on a schedule in the background,
          recording the status code and response time every time. The history
          builds up into an uptime percentage, so you can see not just that a
          site is down now but how often it has been.
        </EmptyCard>
      )}

      {monitors && monitors.length > 0 && (
        <ul className="space-y-2">
          {monitors.map((status) => (
            <li key={status.monitor.id}>
              <MonitorRow status={status} />
            </li>
          ))}
        </ul>
      )}

      <AddMonitorDialog
        open={addOpen}
        pending={create.isPending}
        onOpenChange={setAddOpen}
        onAdd={(name, url, intervalSecs) =>
          create.mutate(
            { name, url, intervalSecs },
            {
              onSuccess: (monitor) => {
                setAddOpen(false);
                toast.success(`Now watching "${monitor.name}"`);
              },
              onError: (failure) => toast.error(String(failure)),
            },
          )
        }
      />
    </div>
  );
}

function MonitorRow({ status }: { status: MonitorStatus }) {
  const toggle = useMonitorToggle();
  const checkNow = useMonitorCheckNow();
  const remove = useMonitorDelete();

  const { monitor, lastCheck } = status;
  const state = monitorState(status);
  const bars = sparklineChecks(status, SPARKLINE_LENGTH);

  return (
    <Card className="group gap-0 py-3">
      <CardContent className="space-y-2 px-4">
        <div className="flex items-center gap-3">
          <StateChip state={state} />

          <div className="min-w-0 flex-1">
            <p className="flex items-center gap-1.5">
              <span className="truncate text-sm font-medium">
                {monitor.name}
              </span>
              <Badge
                variant="outline"
                className="h-4 shrink-0 px-1 text-[9px] text-muted-foreground"
                title={`Checked every ${formatInterval(monitor.intervalSecs)}`}
              >
                {formatInterval(monitor.intervalSecs)}
              </Badge>
            </p>
            <p
              className="truncate font-mono text-[11px] text-muted-foreground"
              title={monitor.url}
            >
              {monitor.url}
            </p>
          </div>

          <Sparkline bars={bars} dimmed={state === "paused"} />

          <div className="hidden items-center gap-4 md:flex">
            <Stat
              label="Last"
              value={
                lastCheck
                  ? `${lastCheck.status ?? "—"} · ${formatLatency(lastCheck.durationMs)}`
                  : "—"
              }
              hint={lastCheck ? timeAgo(lastCheck.checkedAt) : "no checks yet"}
              tone={lastCheck && !lastCheck.ok ? "bad" : "neutral"}
            />
            <Stat
              label="24h uptime"
              value={status.recent.length > 0 ? formatUptimePct(status.uptimePct) : "—"}
              hint="of recent checks"
              tone={
                status.recent.length === 0
                  ? "neutral"
                  : status.uptimePct >= 99
                    ? "good"
                    : status.uptimePct >= 95
                      ? "warn"
                      : "bad"
              }
            />
            <Stat
              label="24h avg"
              value={status.recent.length > 0 ? formatLatency(status.avgMs) : "—"}
              hint="response time"
              tone="neutral"
            />
          </div>

          <div className="flex shrink-0 items-center gap-1">
            <Button
              variant="ghost"
              size="icon"
              className="size-7"
              aria-label={
                monitor.enabled
                  ? `Pause ${monitor.name}`
                  : `Resume ${monitor.name}`
              }
              title={
                monitor.enabled
                  ? "Pause — stop checking, keep the history"
                  : "Resume scheduled checks"
              }
              disabled={toggle.isPending}
              onClick={() =>
                toggle.mutate(
                  { id: monitor.id, enabled: !monitor.enabled },
                  {
                    onSuccess: (updated) =>
                      toast.success(
                        updated.enabled
                          ? `Resumed "${updated.name}"`
                          : `Paused "${updated.name}"`,
                      ),
                    onError: (failure) => toast.error(String(failure)),
                  },
                )
              }
            >
              {monitor.enabled ? (
                <Pause className="size-3.5" />
              ) : (
                <Play className="size-3.5" />
              )}
            </Button>

            <Button
              variant="ghost"
              size="icon"
              className="size-7"
              aria-label={`Check ${monitor.name} now`}
              title="Check now"
              disabled={checkNow.isPending}
              onClick={() =>
                checkNow.mutate(monitor.id, {
                  onSuccess: (check) =>
                    check.ok
                      ? toast.success(
                          `${monitor.name} is up — ${check.status ?? "OK"} in ${formatLatency(check.durationMs)}`,
                        )
                      : toast.error(
                          `${monitor.name} is down — ${failureReason(check)}`,
                        ),
                  onError: (failure) => toast.error(String(failure)),
                })
              }
            >
              <RefreshCw
                className={cn(
                  "size-3.5",
                  checkNow.isPending && "animate-spin",
                )}
              />
            </Button>

            <Button
              variant="ghost"
              size="icon"
              className="size-7 opacity-0 group-hover:opacity-70 hover:!opacity-100"
              aria-label={`Delete ${monitor.name}`}
              title="Delete monitor"
              onClick={() =>
                remove.mutate(monitor.id, {
                  onSuccess: () => toast.success(`Removed "${monitor.name}"`),
                  onError: (failure) => toast.error(String(failure)),
                })
              }
            >
              <Trash2 className="size-3" />
            </Button>
          </div>
        </div>

        {state === "down" && lastCheck && (
          <p className="flex items-start gap-1.5 rounded-md border border-destructive/40 bg-destructive/10 px-2.5 py-1.5 text-[11px] text-red-300">
            <TriangleAlert className="mt-px size-3 shrink-0" />
            <span className="min-w-0 break-words">
              {failureReason(lastCheck)}
            </span>
            <span className="ml-auto shrink-0 whitespace-nowrap text-muted-foreground">
              {timeAgo(lastCheck.checkedAt)}
            </span>
          </p>
        )}

        {/* The stats block is hidden on narrow layouts; keep the numbers reachable. */}
        <p className="text-[11px] text-muted-foreground md:hidden">
          {lastCheck
            ? `${lastCheck.status ?? "—"} · ${formatLatency(lastCheck.durationMs)} · ${timeAgo(lastCheck.checkedAt)}`
            : "No checks recorded yet."}
          {status.recent.length > 0 &&
            ` · ${formatUptimePct(status.uptimePct)} uptime · ${formatLatency(status.avgMs)} avg`}
        </p>
      </CardContent>
    </Card>
  );
}

/**
 * State is carried by shape as much as colour — a filled dot, a ringed dot, a
 * pause glyph and a hollow dashed ring — so the four states stay apart for
 * anyone who can't rely on green-vs-red.
 */
function StateChip({ state }: { state: MonitorState }) {
  const style: Record<MonitorState, string> = {
    up: "border-emerald-500/40 bg-emerald-500/10 text-emerald-300",
    down: "border-destructive/50 bg-destructive/15 text-red-300",
    paused: "border-yellow-500/40 bg-yellow-500/10 text-yellow-200",
    pending: "border-border text-muted-foreground",
  };

  return (
    <span
      className={cn(
        "inline-flex w-[7.5rem] shrink-0 items-center gap-1.5 rounded-full border px-2 py-0.5 text-[11px]",
        style[state],
      )}
    >
      <StateGlyph state={state} />
      {STATE_LABEL[state]}
    </span>
  );
}

function StateGlyph({ state }: { state: MonitorState }) {
  if (state === "paused") {
    return <Pause className="size-2.5 shrink-0 fill-current" />;
  }
  if (state === "pending") {
    return (
      <span className="size-2 shrink-0 rounded-full border border-dashed border-current" />
    );
  }
  if (state === "down") {
    return (
      <span className="relative flex size-2 shrink-0">
        <span className="absolute inline-flex size-full animate-ping rounded-full bg-red-500 opacity-75" />
        <span className="relative inline-flex size-2 rounded-full bg-red-500" />
      </span>
    );
  }
  return <span className="size-2 shrink-0 rounded-full bg-emerald-500" />;
}

function Sparkline({
  bars,
  dimmed,
}: {
  bars: MonitorCheck[];
  dimmed: boolean;
}) {
  if (bars.length === 0) {
    return (
      <div
        className="hidden h-6 w-[9.5rem] shrink-0 items-center lg:flex"
        title="No checks recorded yet"
      >
        <span className="h-px w-full border-t border-dashed border-border" />
      </div>
    );
  }

  const failures = bars.filter((b) => !b.ok).length;

  return (
    <div
      className={cn(
        "hidden h-6 w-[9.5rem] shrink-0 items-end justify-end gap-px lg:flex",
        dimmed && "opacity-40",
      )}
      role="img"
      aria-label={`Last ${bars.length} checks: ${bars.length - failures} ok, ${failures} failed`}
    >
      {bars.map((check) => (
        <span
          key={check.id}
          title={describeCheck(check)}
          className={cn(
            "h-full w-1 rounded-[1px]",
            check.ok ? "bg-emerald-500/70" : "bg-red-500",
          )}
        />
      ))}
    </div>
  );
}

function Stat({
  label,
  value,
  hint,
  tone,
}: {
  label: string;
  value: string;
  hint: string;
  tone: "good" | "warn" | "bad" | "neutral";
}) {
  const toneClass = {
    good: "text-emerald-300",
    warn: "text-yellow-200",
    bad: "text-red-300",
    neutral: "text-foreground",
  }[tone];

  return (
    <div className="w-[5.5rem] shrink-0 text-right" title={hint}>
      <p className="text-[9px] uppercase tracking-wider text-muted-foreground">
        {label}
      </p>
      <p className={cn("truncate text-xs tabular-nums", toneClass)}>{value}</p>
    </div>
  );
}

function AddMonitorDialog({
  open,
  pending,
  onOpenChange,
  onAdd,
}: {
  open: boolean;
  pending: boolean;
  onOpenChange: (open: boolean) => void;
  onAdd: (name: string, url: string, intervalSecs: number) => void;
}) {
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [intervalSecs, setIntervalSecs] = useState<number>(
    INTERVAL_CHOICES[1].secs,
  );

  const urlOk = isFetchableUrl(url);
  const canSubmit = Boolean(name.trim()) && urlOk && !pending;

  function submit() {
    if (!canSubmit) return;
    onAdd(name.trim(), url.trim(), intervalSecs);
    setName("");
    setUrl("");
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Add monitor</DialogTitle>
        </DialogHeader>
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="monitor-name">Name</Label>
            <Input
              id="monitor-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g. Marketing site"
              autoFocus
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="monitor-url">URL</Label>
            <Input
              id="monitor-url"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
              placeholder="https://example.com/health"
              className="font-mono text-xs"
            />
            <p className="text-[11px] text-muted-foreground">
              {url.trim() && !urlOk
                ? "Needs a full http:// or https:// URL."
                : "A health endpoint works better than a homepage — it fails when the app does."}
            </p>
          </div>

          <div className="space-y-1.5">
            <Label>Check every</Label>
            <div className="flex gap-1.5">
              {INTERVAL_CHOICES.map((choice) => (
                <button
                  key={choice.secs}
                  type="button"
                  onClick={() => setIntervalSecs(choice.secs)}
                  aria-pressed={intervalSecs === choice.secs}
                  className={cn(
                    "rounded-full border px-3 py-1 text-xs transition-colors",
                    intervalSecs === choice.secs
                      ? "border-primary/40 bg-primary/10 text-foreground"
                      : "border-border text-muted-foreground hover:text-foreground",
                  )}
                >
                  {choice.label}
                </button>
              ))}
            </div>
            <p className="text-[11px] text-muted-foreground">
              One minute is the floor — anything faster is rejected by the
              scheduler.
            </p>
          </div>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button disabled={!canSubmit} onClick={submit}>
            {pending ? "Adding…" : "Add monitor"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function EmptyCard({
  title,
  children,
}: {
  title: string;
  children?: React.ReactNode;
}) {
  return (
    <div className="mx-auto max-w-2xl py-6">
      <Card>
        <CardContent className="flex flex-col items-center gap-2 py-12 text-center">
          <Activity className="size-6 text-muted-foreground" />
          <p className="font-medium">{title}</p>
          {children && (
            <p className="max-w-md text-sm text-muted-foreground">{children}</p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

function summarise(monitors: MonitorStatus[] | undefined) {
  const counts = { up: 0, down: 0, paused: 0, pending: 0 };
  for (const status of monitors ?? []) counts[monitorState(status)] += 1;
  return counts;
}
