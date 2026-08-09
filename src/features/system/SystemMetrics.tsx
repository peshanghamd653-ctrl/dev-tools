import {
  ArrowDownUp,
  Cpu,
  Gauge,
  HardDrive,
  MemoryStick,
  Timer,
} from "lucide-react";

import type { DiskInfo, ProcessInfo } from "@/shared/ipc/client";
import { formatBytes } from "@/shared/lib/format";
import { cn } from "@/shared/lib/utils";
import { Card, CardContent } from "@/shared/ui/card";
import { Skeleton } from "@/shared/ui/skeleton";
import { formatPercent, formatUptime, loadColor, usageRatio } from "./format";
import { useSystemHistory, useSystemSnapshot } from "./hooks";
import { SystemHistoryChart } from "./SystemHistoryChart";

/** Enough to spot a runaway process, few enough to stay a strip. */
const TOP_PROCESS_COUNT = 5;

/**
 * The Dashboard's live machine strip: four metric cards plus one row pairing
 * storage with the heaviest processes. Deliberately capped in height — this
 * sits above the rest of the Dashboard, it isn't the Dashboard.
 */
export function SystemMetrics() {
  const { data: snapshot, isLoading, error } = useSystemSnapshot();
  const { data: history } = useSystemHistory();

  if (isLoading) {
    return (
      <section className="space-y-3">
        <SectionLabel />
        <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
          {Array.from({ length: 4 }, (_, i) => (
            <Skeleton key={i} className="h-[86px] w-full rounded-xl" />
          ))}
        </div>
      </section>
    );
  }

  if (!snapshot) {
    // Outside the desktop shell the query never runs and there is nothing
    // honest to show, so the strip simply isn't there.
    if (!error) return null;
    return (
      <section className="space-y-3">
        <SectionLabel />
        <p className="text-xs text-muted-foreground">
          System metrics unavailable — {String(error)}
        </p>
      </section>
    );
  }

  const memRatio = usageRatio(snapshot.memUsed, snapshot.memTotal);
  const swapRatio = usageRatio(snapshot.swapUsed, snapshot.swapTotal);
  const hasSwap = snapshot.swapTotal > 0;

  return (
    <section className="space-y-3">
      <SectionLabel />

      <div
        className={cn(
          "grid grid-cols-2 gap-3",
          hasSwap ? "lg:grid-cols-4" : "lg:grid-cols-3",
        )}
      >
        <MetricCard
          icon={<Cpu className="size-3.5" />}
          label="CPU"
          value={formatPercent(snapshot.cpuUsage)}
          ratio={Math.min(100, Math.max(0, snapshot.cpuUsage))}
          hint={`${snapshot.cpuCores} ${snapshot.cpuCores === 1 ? "core" : "cores"}`}
        />
        <MetricCard
          icon={<MemoryStick className="size-3.5" />}
          label="Memory"
          value={formatBytes(snapshot.memUsed)}
          ratio={memRatio}
          hint={`${formatPercent(memRatio)} of ${formatBytes(snapshot.memTotal)}`}
        />
        {hasSwap && (
          <MetricCard
            icon={<ArrowDownUp className="size-3.5" />}
            label="Swap"
            value={formatBytes(snapshot.swapUsed)}
            ratio={swapRatio}
            hint={`${formatPercent(swapRatio)} of ${formatBytes(snapshot.swapTotal)}`}
          />
        )}
        <MetricCard
          icon={<Timer className="size-3.5" />}
          label="Uptime"
          value={formatUptime(snapshot.uptimeSecs)}
          hint="since last boot"
        />
      </div>

      <div className="grid gap-3 lg:grid-cols-3">
        <Card className="gap-0 py-3">
          <CardContent className="px-4">
            <PanelLabel
              icon={<HardDrive className="size-3" />}
              text="Storage"
            />
            {snapshot.disks.length === 0 ? (
              <p className="pt-2 text-xs text-muted-foreground">
                No disks reported.
              </p>
            ) : (
              <ul className="space-y-2 pt-2">
                {snapshot.disks.map((disk) => (
                  <li key={`${disk.name}-${disk.mount}`}>
                    <DiskRow disk={disk} />
                  </li>
                ))}
              </ul>
            )}
          </CardContent>
        </Card>

        <Card className="gap-0 py-3">
          <CardContent className="px-4">
            <PanelLabel
              icon={<Gauge className="size-3" />}
              text="Top processes"
            />
            {snapshot.topProcesses.length === 0 ? (
              <p className="pt-2 text-xs text-muted-foreground">
                No processes reported.
              </p>
            ) : (
              <ul className="pt-1">
                {snapshot.topProcesses
                  .slice(0, TOP_PROCESS_COUNT)
                  .map((process) => (
                    <li key={process.pid}>
                      <ProcessRow process={process} />
                    </li>
                  ))}
              </ul>
            )}
          </CardContent>
        </Card>

        <SystemHistoryChart points={history} />
      </div>
    </section>
  );
}

function SectionLabel() {
  return (
    <p className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
      System
    </p>
  );
}

function PanelLabel({ icon, text }: { icon: React.ReactNode; text: string }) {
  return (
    <p className="flex items-center gap-1.5 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
      {icon}
      {text}
    </p>
  );
}

function MetricCard({
  icon,
  label,
  value,
  hint,
  ratio,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  hint: string;
  /** Omitted for metrics that aren't a fraction of anything (uptime). */
  ratio?: number;
}) {
  return (
    <Card className="gap-0 py-3">
      <CardContent className="space-y-1.5 px-4">
        <p className="flex items-center gap-1.5 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
          {icon}
          {label}
        </p>
        <p className="text-lg font-semibold leading-none tabular-nums">
          {value}
        </p>
        {ratio === undefined ? (
          <div className="h-1" />
        ) : (
          <UsageBar ratio={ratio} />
        )}
        <p className="truncate text-[11px] text-muted-foreground">{hint}</p>
      </CardContent>
    </Card>
  );
}

function DiskRow({ disk }: { disk: DiskInfo }) {
  const used = Math.max(0, disk.total - disk.available);
  const ratio = usageRatio(used, disk.total);

  return (
    <div
      className="space-y-1"
      title={`${disk.name} mounted at ${disk.mount} — ${formatBytes(used)} used, ${formatBytes(disk.available)} free of ${formatBytes(disk.total)}`}
    >
      <div className="flex items-baseline gap-2">
        <span className="truncate font-mono text-[11px]">{disk.mount}</span>
        <span className="ml-auto shrink-0 text-[11px] tabular-nums text-muted-foreground">
          {formatBytes(used)} / {formatBytes(disk.total)}
        </span>
      </div>
      <UsageBar ratio={ratio} />
      <p className="text-[10px] text-muted-foreground">
        {formatBytes(disk.available)} free · {formatPercent(ratio)} used
      </p>
    </div>
  );
}

function ProcessRow({ process }: { process: ProcessInfo }) {
  return (
    <div
      className="flex items-center gap-2 rounded px-1 py-1 hover:bg-accent/40"
      title={`PID ${process.pid}`}
    >
      <span className="min-w-0 flex-1 truncate text-xs">{process.name}</span>
      <span className="shrink-0 text-[11px] tabular-nums text-muted-foreground">
        {formatBytes(process.memory)}
      </span>
      <span className="w-12 shrink-0 text-right text-[11px] font-medium tabular-nums">
        {formatPercent(process.cpuUsage)}
      </span>
    </div>
  );
}

function UsageBar({ ratio }: { ratio: number }) {
  return (
    <div
      className="h-1 w-full overflow-hidden rounded-full bg-muted"
      role="presentation"
    >
      <div
        className={cn(
          "h-full rounded-full transition-[width] duration-500",
          loadColor(ratio),
        )}
        style={{ width: `${ratio}%` }}
      />
    </div>
  );
}
