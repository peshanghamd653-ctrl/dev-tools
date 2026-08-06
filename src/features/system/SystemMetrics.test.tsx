/**
 * `format.ts` is unit-tested on its own. What these tests cover is the strip's
 * judgement: when it should render nothing at all, when a metric card is not
 * applicable, and that the formatted numbers land in the right slot — a
 * memory card showing the *total* where the *used* figure belongs is a
 * plausible regression that no formatting test would catch.
 */
import { cleanup, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/shared/ipc/client", async () => {
  const { createClientMock } = await import("@/shared/test/ipc");
  return createClientMock();
});

import { ipc, type SystemSnapshot } from "@/shared/ipc/client";
import { resetClientMock, setDesktopShell } from "@/shared/test/ipc";
import { renderWithClient } from "@/shared/test/render";
import { SystemMetrics } from "./SystemMetrics";

const GB = 1024 ** 3;

function snapshot(over: Partial<SystemSnapshot> = {}): SystemSnapshot {
  return {
    cpuUsage: 42.37,
    cpuCores: 16,
    memTotal: 32 * GB,
    memUsed: 8 * GB,
    swapTotal: 0,
    swapUsed: 0,
    uptimeSecs: 273_120,
    disks: [
      { name: "Windows", mount: "C:\\", total: 500 * GB, available: 125 * GB },
    ],
    topProcesses: [
      { pid: 1234, name: "node.exe", cpuUsage: 12.5, memory: 512 * 1024 * 1024 },
    ],
    ...over,
  };
}

beforeEach(resetClientMock);
afterEach(cleanup);

describe("SystemMetrics availability", () => {
  it("renders nothing at all in a browser tab", () => {
    setDesktopShell(false);
    const { container } = renderWithClient(<SystemMetrics />);

    // Not an empty card, not a placeholder — the strip is simply absent, so
    // the Dashboard below it does not shift down around a hole.
    expect(container).toBeEmptyDOMElement();
    expect(ipc.systemSnapshot).not.toHaveBeenCalled();
  });

  it("says the metrics are unavailable when the probe fails", async () => {
    vi.mocked(ipc.systemSnapshot).mockRejectedValue("sysinfo refresh failed");
    renderWithClient(<SystemMetrics />);

    expect(
      await screen.findByText(/System metrics unavailable — sysinfo refresh failed/),
    ).toBeInTheDocument();
  });
});

describe("SystemMetrics cards", () => {
  it("puts used memory on the card and the ratio in the hint", async () => {
    vi.mocked(ipc.systemSnapshot).mockResolvedValue(snapshot());
    renderWithClient(<SystemMetrics />);

    expect(await screen.findByText("8.0 GB")).toBeInTheDocument();
    expect(screen.getByText("25% of 32.0 GB")).toBeInTheDocument();
  });

  it("shows CPU load against the core count", async () => {
    vi.mocked(ipc.systemSnapshot).mockResolvedValue(snapshot());
    renderWithClient(<SystemMetrics />);

    expect(await screen.findByText("42.4%")).toBeInTheDocument();
    expect(screen.getByText("16 cores")).toBeInTheDocument();
  });

  it("singularises the core count on a one-core machine", async () => {
    vi.mocked(ipc.systemSnapshot).mockResolvedValue(snapshot({ cpuCores: 1 }));
    renderWithClient(<SystemMetrics />);

    expect(await screen.findByText("1 core")).toBeInTheDocument();
  });

  it("renders uptime in days and hours, not seconds", async () => {
    vi.mocked(ipc.systemSnapshot).mockResolvedValue(snapshot());
    renderWithClient(<SystemMetrics />);

    expect(await screen.findByText("3d 3h 52m")).toBeInTheDocument();
  });

  it("hides the swap card on a machine with no swap", async () => {
    vi.mocked(ipc.systemSnapshot).mockResolvedValue(snapshot());
    renderWithClient(<SystemMetrics />);

    await screen.findByText("Memory");
    expect(screen.queryByText("Swap")).not.toBeInTheDocument();
  });

  it("shows the swap card when swap exists", async () => {
    vi.mocked(ipc.systemSnapshot).mockResolvedValue(
      snapshot({ swapTotal: 8 * GB, swapUsed: 2 * GB }),
    );
    renderWithClient(<SystemMetrics />);

    expect(await screen.findByText("Swap")).toBeInTheDocument();
    expect(screen.getByText("25% of 8.0 GB")).toBeInTheDocument();
  });
});

describe("SystemMetrics storage and processes", () => {
  it("reports a disk as used-of-total plus what is left", async () => {
    vi.mocked(ipc.systemSnapshot).mockResolvedValue(snapshot());
    renderWithClient(<SystemMetrics />);

    expect(await screen.findByText("C:\\")).toBeInTheDocument();
    expect(screen.getByText("375 GB / 500 GB")).toBeInTheDocument();
    expect(screen.getByText("125 GB free · 75% used")).toBeInTheDocument();
  });

  it("says so when the probe reports no disks", async () => {
    vi.mocked(ipc.systemSnapshot).mockResolvedValue(snapshot({ disks: [] }));
    renderWithClient(<SystemMetrics />);

    expect(await screen.findByText("No disks reported.")).toBeInTheDocument();
  });

  it("keeps the process strip to five rows however many arrive", async () => {
    vi.mocked(ipc.systemSnapshot).mockResolvedValue(
      snapshot({
        topProcesses: Array.from({ length: 12 }, (_, i) => ({
          pid: i,
          name: `proc-${i}.exe`,
          cpuUsage: 100 - i,
          memory: 1024 * 1024,
        })),
      }),
    );
    renderWithClient(<SystemMetrics />);

    expect(await screen.findByText("proc-4.exe")).toBeInTheDocument();
    expect(screen.queryByText("proc-5.exe")).not.toBeInTheDocument();
  });

  it("says so when the probe reports no processes", async () => {
    vi.mocked(ipc.systemSnapshot).mockResolvedValue(
      snapshot({ topProcesses: [] }),
    );
    renderWithClient(<SystemMetrics />);

    expect(await screen.findByText("No processes reported.")).toBeInTheDocument();
  });
});
