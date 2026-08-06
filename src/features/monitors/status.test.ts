import { describe, expect, it } from "vitest";

import type { MonitorCheck, MonitorStatus } from "@/shared/ipc/client";
import {
  failureReason,
  formatInterval,
  formatLatency,
  formatUptimePct,
  isFetchableUrl,
  monitorState,
  sparklineChecks,
} from "./status";

function check(over: Partial<MonitorCheck> = {}): MonitorCheck {
  return {
    id: "c1",
    monitorId: "m1",
    status: 200,
    ok: true,
    durationMs: 120,
    error: null,
    checkedAt: 1_700_000_000_000,
    ...over,
  };
}

function status(over: Partial<MonitorStatus> = {}): MonitorStatus {
  return {
    monitor: {
      id: "m1",
      name: "Site",
      url: "https://example.com",
      intervalSecs: 300,
      enabled: true,
      createdAt: 1_700_000_000_000,
    },
    lastCheck: check(),
    uptimePct: 100,
    avgMs: 120,
    recent: [check()],
    ...over,
  };
}

describe("monitorState", () => {
  it("separates never-checked from up", () => {
    expect(monitorState(status())).toBe("up");
    expect(monitorState(status({ lastCheck: null, recent: [] }))).toBe(
      "pending",
    );
  });

  it("reports down on a failing last check", () => {
    expect(monitorState(status({ lastCheck: check({ ok: false }) }))).toBe(
      "down",
    );
  });

  it("reports paused regardless of the last result", () => {
    const disabled = { ...status().monitor, enabled: false };
    expect(monitorState(status({ monitor: disabled }))).toBe("paused");
    expect(
      monitorState(
        status({ monitor: disabled, lastCheck: check({ ok: false }) }),
      ),
    ).toBe("paused");
  });
});

describe("sparklineChecks", () => {
  it("orders oldest first so time reads left to right", () => {
    const out = sparklineChecks(
      status({
        recent: [
          check({ id: "c", checkedAt: 300 }),
          check({ id: "a", checkedAt: 100 }),
          check({ id: "b", checkedAt: 200 }),
        ],
      }),
      10,
    );
    expect(out.map((c) => c.id)).toEqual(["a", "b", "c"]);
  });

  it("keeps the newest N when the history is longer than the strip", () => {
    const recent = Array.from({ length: 50 }, (_, i) =>
      check({ id: `c${i}`, checkedAt: i }),
    );
    const out = sparklineChecks(status({ recent }), 30);
    expect(out).toHaveLength(30);
    expect(out.at(-1)?.id).toBe("c49");
  });

  it("does not mutate the source array", () => {
    const recent = [
      check({ id: "c", checkedAt: 300 }),
      check({ id: "a", checkedAt: 100 }),
    ];
    sparklineChecks(status({ recent }), 10);
    expect(recent.map((c) => c.id)).toEqual(["c", "a"]);
  });
});

describe("failureReason", () => {
  it("prefers the transport error, falls back to the status code", () => {
    expect(failureReason(check({ ok: false, error: "dns error" }))).toBe(
      "dns error",
    );
    expect(failureReason(check({ ok: false, status: 503 }))).toBe("HTTP 503");
    expect(failureReason(check({ ok: false, status: null }))).toBe(
      "Check failed",
    );
  });
});

describe("formatting", () => {
  it("switches latency to seconds past a second", () => {
    expect(formatLatency(120)).toBe("120 ms");
    expect(formatLatency(2500)).toBe("2.50 s");
  });

  it("keeps uptime to one decimal", () => {
    expect(formatUptimePct(100)).toBe("100%");
    expect(formatUptimePct(99.8571)).toBe("99.9%");
  });

  it("labels known and unknown intervals", () => {
    expect(formatInterval(300)).toBe("5m");
    expect(formatInterval(7200)).toBe("2h");
    expect(formatInterval(120)).toBe("2m");
  });
});

describe("isFetchableUrl", () => {
  it("accepts only http and https", () => {
    expect(isFetchableUrl("https://example.com")).toBe(true);
    expect(isFetchableUrl("  http://example.com/health  ")).toBe(true);
    expect(isFetchableUrl("example.com")).toBe(false);
    expect(isFetchableUrl("file:///etc/passwd")).toBe(false);
    expect(isFetchableUrl("")).toBe(false);
  });
});
