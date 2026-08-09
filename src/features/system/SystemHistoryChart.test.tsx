/**
 * The sparkline geometry itself isn't asserted here — an SVG path string is
 * an implementation detail. What matters is the judgement calls: what shows
 * before there's enough history, and that the summary numbers (current,
 * average, peak) are computed from the right series.
 */
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { SystemHistoryPoint } from "@/shared/ipc/client";
import { SystemHistoryChart } from "./SystemHistoryChart";

function point(over: Partial<SystemHistoryPoint> = {}): SystemHistoryPoint {
  return { ts: 0, cpuUsage: 10, memUsed: 50, memTotal: 100, ...over };
}

afterEach(cleanup);

describe("SystemHistoryChart availability", () => {
  it("says it's loading when history hasn't arrived yet", () => {
    render(<SystemHistoryChart points={undefined} />);
    expect(screen.getByText("Loading history…")).toBeInTheDocument();
  });

  it("asks the user to check back with no samples yet", () => {
    render(<SystemHistoryChart points={[]} />);
    expect(
      screen.getByText("Not enough history yet — check back in a minute."),
    ).toBeInTheDocument();
  });

  it("still asks to check back with only one sample — a line needs two points", () => {
    render(<SystemHistoryChart points={[point()]} />);
    expect(
      screen.getByText("Not enough history yet — check back in a minute."),
    ).toBeInTheDocument();
  });
});

describe("SystemHistoryChart summary", () => {
  const points = [
    point({ ts: 0, cpuUsage: 10, memUsed: 50, memTotal: 100 }),
    point({ ts: 60_000, cpuUsage: 30, memUsed: 80, memTotal: 100 }),
  ];

  it("shows the newest reading as the current value", () => {
    render(<SystemHistoryChart points={points} />);
    expect(screen.getByText("30%")).toBeInTheDocument(); // newest CPU
    expect(screen.getByText("80 B")).toBeInTheDocument(); // newest mem used
  });

  it("averages and peaks each series independently", () => {
    render(<SystemHistoryChart points={points} />);

    // CPU: 10, 30 -> avg 20%, peak 30%
    expect(screen.getByText("avg 20% · peak 30%")).toBeInTheDocument();
    // Memory ratio: 50%, 80% -> avg 65%, peak 80%
    expect(screen.getByText("avg 65% · peak 80%")).toBeInTheDocument();
  });

  it("reports the window covered in minutes", () => {
    render(<SystemHistoryChart points={points} />);
    expect(screen.getByText("last 1m")).toBeInTheDocument();
  });

  it("says 'minute' rather than '0m' for a window under a minute", () => {
    render(
      <SystemHistoryChart
        points={[point({ ts: 0 }), point({ ts: 20_000, cpuUsage: 15 })]}
      />,
    );
    expect(screen.getByText("last minute")).toBeInTheDocument();
  });
});
