import { Activity } from "lucide-react";

import type { SystemHistoryPoint } from "@/shared/ipc/client";
import { Card, CardContent } from "@/shared/ui/card";
import { formatBytes } from "@/shared/lib/format";
import { formatPercent, usageRatio } from "./format";

const VIEW_WIDTH = 300;
const VIEW_HEIGHT = 56;

/** One value per point, already on a 0-100 scale. */
function sparklinePath(values: number[]): { line: string; area: string } {
  if (values.length < 2) return { line: "", area: "" };

  const step = VIEW_WIDTH / (values.length - 1);
  const coords = values.map((v, i) => {
    const x = i * step;
    const y = VIEW_HEIGHT - (Math.min(100, Math.max(0, v)) / 100) * VIEW_HEIGHT;
    return [x, y] as const;
  });

  const line = coords
    .map(([x, y], i) => `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`)
    .join(" ");
  const area = `${line} L${VIEW_WIDTH},${VIEW_HEIGHT} L0,${VIEW_HEIGHT} Z`;
  return { line, area };
}

/**
 * Two sparklines — CPU and memory, both over the scheduler's ~3h retention
 * window — plus a peak/average summary a squiggle alone can't convey. Hand
 * rolled rather than a charting library: two series on a fixed 0-100 scale is
 * a polyline, not a reason to add a dependency.
 */
export function SystemHistoryChart({
  points,
}: {
  points: SystemHistoryPoint[] | undefined;
}) {
  if (!points || points.length < 2) {
    return (
      <Card className="gap-0 py-3">
        <CardContent className="px-4">
          <PanelLabel />
          <p className="pt-2 text-xs text-muted-foreground">
            {points === undefined
              ? "Loading history…"
              : "Not enough history yet — check back in a minute."}
          </p>
        </CardContent>
      </Card>
    );
  }

  const cpu = points.map((p) => p.cpuUsage);
  const mem = points.map((p) => usageRatio(p.memUsed, p.memTotal));
  const oldest = points[0]!;
  const newest = points[points.length - 1]!;
  const spanMinutes = Math.round((newest.ts - oldest.ts) / 60_000);

  return (
    <Card className="gap-0 py-3">
      <CardContent className="space-y-3 px-4">
        <div className="flex items-center justify-between">
          <PanelLabel />
          <span className="text-[10px] text-muted-foreground">
            last {spanMinutes < 1 ? "minute" : `${spanMinutes}m`}
          </span>
        </div>
        <Sparkline
          label="CPU"
          values={cpu}
          current={formatPercent(newest.cpuUsage)}
          summary={`avg ${formatPercent(average(cpu))} · peak ${formatPercent(Math.max(...cpu))}`}
        />
        <Sparkline
          label="Memory"
          values={mem}
          current={formatBytes(newest.memUsed)}
          summary={`avg ${formatPercent(average(mem))} · peak ${formatPercent(Math.max(...mem))}`}
        />
      </CardContent>
    </Card>
  );
}

function average(values: number[]): number {
  return values.reduce((sum, v) => sum + v, 0) / values.length;
}

function PanelLabel() {
  return (
    <p className="flex items-center gap-1.5 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
      <Activity className="size-3" />
      History
    </p>
  );
}

function Sparkline({
  label,
  values,
  current,
  summary,
}: {
  label: string;
  values: number[];
  current: string;
  summary: string;
}) {
  const { line, area } = sparklinePath(values);
  return (
    <div className="space-y-1">
      <div className="flex items-baseline justify-between">
        <span className="text-[11px] text-muted-foreground">{label}</span>
        <span className="text-xs font-medium tabular-nums">{current}</span>
      </div>
      <svg
        viewBox={`0 0 ${VIEW_WIDTH} ${VIEW_HEIGHT}`}
        preserveAspectRatio="none"
        className="h-10 w-full"
        role="img"
        aria-label={`${label} over time: ${summary}`}
      >
        <path d={area} className="fill-emerald-500/15" />
        <path
          d={line}
          fill="none"
          className="stroke-emerald-500"
          strokeWidth={1.5}
        />
      </svg>
      <p className="text-[10px] text-muted-foreground">{summary}</p>
    </div>
  );
}
