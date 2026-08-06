/**
 * `status.ts` already has unit tests for the state machine and the sparkline
 * ordering. What is untested is the wiring: that the page actually renders the
 * state it derives, that a failing monitor says *why* on the row instead of
 * only turning red, and that the pause control sends the inverted value rather
 * than the current one.
 */
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/shared/ipc/client", async () => {
  const { createClientMock } = await import("@/shared/test/ipc");
  return createClientMock();
});

import {
  ipc,
  type MonitorCheck,
  type MonitorStatus,
} from "@/shared/ipc/client";
import { resetClientMock, setDesktopShell } from "@/shared/test/ipc";
import { renderWithClient } from "@/shared/test/render";
import { MonitorsPage } from "./MonitorsPage";

const BASE_MONITOR = {
  id: "m1",
  name: "Marketing site",
  url: "https://example.com/health",
  intervalSecs: 300,
  enabled: true,
  createdAt: 1_700_000_000_000,
};

function check(over: Partial<MonitorCheck> = {}): MonitorCheck {
  return {
    id: "check-1",
    monitorId: "m1",
    status: 200,
    ok: true,
    durationMs: 120,
    error: null,
    checkedAt: Date.now(),
    ...over,
  };
}

function status(over: Partial<MonitorStatus> = {}): MonitorStatus {
  const monitor = { ...BASE_MONITOR, ...over.monitor };
  return {
    lastCheck: check({ monitorId: monitor.id }),
    uptimePct: 100,
    avgMs: 120,
    recent: [check({ monitorId: monitor.id })],
    ...over,
    monitor,
  };
}

beforeEach(resetClientMock);
afterEach(cleanup);

describe("MonitorsPage empty and degraded states", () => {
  it("explains that checks need the desktop shell, and asks for nothing", () => {
    setDesktopShell(false);
    renderWithClient(<MonitorsPage />);

    expect(
      screen.getByText("Uptime monitoring needs the desktop shell"),
    ).toBeInTheDocument();
    expect(ipc.monitorsList).not.toHaveBeenCalled();
  });

  it("invites the first monitor rather than showing a bare list", async () => {
    vi.mocked(ipc.monitorsList).mockResolvedValue([]);
    renderWithClient(<MonitorsPage />);

    expect(
      await screen.findByText("Nothing is being watched yet"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Watch a site and get told when it stops answering/),
    ).toBeInTheDocument();
  });
});

describe("MonitorsPage state rendering", () => {
  it("keeps the four states apart on the rows and in the summary", async () => {
    vi.mocked(ipc.monitorsList).mockResolvedValue([
      status({ monitor: { ...BASE_MONITOR, id: "up", name: "Up site" } }),
      status({
        monitor: { ...BASE_MONITOR, id: "down", name: "Down site" },
        lastCheck: check({ ok: false, status: 503 }),
      }),
      status({
        monitor: { ...BASE_MONITOR, id: "off", name: "Paused site", enabled: false },
      }),
      status({
        monitor: { ...BASE_MONITOR, id: "new", name: "New site" },
        lastCheck: null,
        recent: [],
      }),
    ]);
    renderWithClient(<MonitorsPage />);

    expect(await screen.findByText("Up")).toBeInTheDocument();
    expect(screen.getByText("Down")).toBeInTheDocument();
    expect(screen.getByText("Paused")).toBeInTheDocument();
    // "Never checked" is a distinct state from "Up" — not silently folded in.
    expect(screen.getByText("Never checked")).toBeInTheDocument();

    expect(
      screen.getByText("1 up · 1 down · 1 paused · 1 awaiting first check"),
    ).toBeInTheDocument();
  });

  it("omits the zero buckets from the summary instead of printing '0 down'", async () => {
    vi.mocked(ipc.monitorsList).mockResolvedValue([status()]);
    renderWithClient(<MonitorsPage />);

    expect(await screen.findByText("1 up")).toBeInTheDocument();
  });

  it("says why a monitor is down, not just that it is", async () => {
    vi.mocked(ipc.monitorsList).mockResolvedValue([
      status({
        lastCheck: check({ ok: false, status: null, error: "dns error: no record" }),
        recent: [check({ ok: false, status: null, error: "dns error: no record" })],
      }),
    ]);
    renderWithClient(<MonitorsPage />);

    expect(await screen.findByText("dns error: no record")).toBeInTheDocument();
  });

  it("does not show a failure banner for a healthy monitor", async () => {
    vi.mocked(ipc.monitorsList).mockResolvedValue([status()]);
    renderWithClient(<MonitorsPage />);

    await screen.findByText("Up");
    expect(screen.queryByText(/HTTP 5/)).not.toBeInTheDocument();
  });
});

describe("MonitorsPage sparkline", () => {
  it("describes the strip for anyone not reading the colours", async () => {
    const recent = [
      check({ id: "a", checkedAt: 100 }),
      check({ id: "b", checkedAt: 200, ok: false, status: 500 }),
      check({ id: "c", checkedAt: 300 }),
    ];
    vi.mocked(ipc.monitorsList).mockResolvedValue([status({ recent })]);
    renderWithClient(<MonitorsPage />);

    const strip = await screen.findByRole("img", {
      name: "Last 3 checks: 2 ok, 1 failed",
    });
    expect(strip.children).toHaveLength(3);
  });

  it("draws a placeholder, not a strip, before the first check lands", async () => {
    vi.mocked(ipc.monitorsList).mockResolvedValue([
      status({ lastCheck: null, recent: [] }),
    ]);
    renderWithClient(<MonitorsPage />);

    await screen.findByText("Never checked");
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByTitle("No checks recorded yet")).toBeInTheDocument();
    expect(screen.getByText("No checks recorded yet.")).toBeInTheDocument();
  });
});

describe("MonitorsPage row actions", () => {
  it("labels the pause control by what it will do, and sends the inverse", async () => {
    vi.mocked(ipc.monitorsList).mockResolvedValue([status()]);
    vi.mocked(ipc.monitorToggle).mockResolvedValue({ ...BASE_MONITOR, enabled: false });
    renderWithClient(<MonitorsPage />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Pause Marketing site" }),
    );

    await waitFor(() =>
      expect(ipc.monitorToggle).toHaveBeenCalledWith("m1", false),
    );
  });

  it("offers Resume, not Pause, on an already-paused monitor", async () => {
    vi.mocked(ipc.monitorsList).mockResolvedValue([
      status({ monitor: { ...BASE_MONITOR, enabled: false } }),
    ]);
    vi.mocked(ipc.monitorToggle).mockResolvedValue(BASE_MONITOR);
    renderWithClient(<MonitorsPage />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Resume Marketing site" }),
    );

    await waitFor(() =>
      expect(ipc.monitorToggle).toHaveBeenCalledWith("m1", true),
    );
  });

  it("probes the monitor the button belongs to", async () => {
    vi.mocked(ipc.monitorsList).mockResolvedValue([
      status(),
      status({ monitor: { ...BASE_MONITOR, id: "m2", name: "Docs" } }),
    ]);
    vi.mocked(ipc.monitorCheckNow).mockResolvedValue(check({ monitorId: "m2" }));
    renderWithClient(<MonitorsPage />);

    fireEvent.click(await screen.findByRole("button", { name: "Check Docs now" }));

    await waitFor(() => expect(ipc.monitorCheckNow).toHaveBeenCalledWith("m2"));
  });
});

describe("MonitorsPage add dialog", () => {
  it("refuses a URL the backend could never fetch, and says why", async () => {
    vi.mocked(ipc.monitorsList).mockResolvedValue([]);
    renderWithClient(<MonitorsPage />);

    fireEvent.click(await screen.findByRole("button", { name: "Add monitor" }));
    const dialog = within(screen.getByRole("dialog"));

    fireEvent.change(dialog.getByLabelText("Name"), {
      target: { value: "Local file" },
    });
    fireEvent.change(dialog.getByLabelText("URL"), {
      target: { value: "file:///etc/passwd" },
    });

    expect(
      dialog.getByText("Needs a full http:// or https:// URL."),
    ).toBeInTheDocument();
    expect(dialog.getByRole("button", { name: "Add monitor" })).toBeDisabled();
    expect(ipc.monitorCreate).not.toHaveBeenCalled();
  });

  it("creates the monitor with the interval the user picked", async () => {
    vi.mocked(ipc.monitorsList).mockResolvedValue([]);
    vi.mocked(ipc.monitorCreate).mockResolvedValue(BASE_MONITOR);
    renderWithClient(<MonitorsPage />);

    fireEvent.click(await screen.findByRole("button", { name: "Add monitor" }));
    const dialog = within(screen.getByRole("dialog"));

    fireEvent.change(dialog.getByLabelText("Name"), {
      target: { value: "Marketing site" },
    });
    fireEvent.change(dialog.getByLabelText("URL"), {
      target: { value: "https://example.com/health" },
    });
    fireEvent.click(dialog.getByRole("button", { name: "15m" }));
    fireEvent.click(dialog.getByRole("button", { name: "Add monitor" }));

    await waitFor(() =>
      expect(ipc.monitorCreate).toHaveBeenCalledWith(
        "Marketing site",
        "https://example.com/health",
        900,
      ),
    );
  });
});
