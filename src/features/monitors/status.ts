import type { MonitorCheck, MonitorStatus } from "@/shared/ipc/client";

/**
 * Four states, not three. "Never checked" is genuinely different from "up" —
 * a monitor that has just been created and one that answered 200 a minute ago
 * both have zero failures, and conflating them hides the fact that the
 * scheduler hasn't reached the new one yet.
 */
export type MonitorState = "up" | "down" | "paused" | "pending";

export function monitorState(status: MonitorStatus): MonitorState {
  if (!status.monitor.enabled) return "paused";
  if (!status.lastCheck) return "pending";
  return status.lastCheck.ok ? "up" : "down";
}

export const STATE_LABEL: Record<MonitorState, string> = {
  up: "Up",
  down: "Down",
  paused: "Paused",
  pending: "Never checked",
};

/** Bars newest-last so time reads left to right, however the backend orders them. */
export function sparklineChecks(
  status: MonitorStatus,
  limit: number,
): MonitorCheck[] {
  return [...status.recent]
    .sort((a, b) => a.checkedAt - b.checkedAt)
    .slice(-limit);
}

/**
 * Why a check failed, in words. A red bar or a red dot with no reason behind
 * it tells you something is wrong and nothing about what — the transport
 * error if there was one, otherwise the HTTP status that was rejected.
 */
export function failureReason(check: MonitorCheck): string {
  if (check.error) return check.error;
  if (check.status !== null) return `HTTP ${check.status}`;
  return "Check failed";
}

/** One line describing a check, used for the sparkline bars' `title`. */
export function describeCheck(check: MonitorCheck): string {
  const when = new Date(check.checkedAt).toLocaleString();
  const outcome = check.ok
    ? `OK${check.status === null ? "" : ` ${check.status}`}`
    : failureReason(check);
  return `${when} · ${outcome} · ${formatLatency(check.durationMs)}`;
}

export function formatLatency(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return "—";
  if (ms < 1000) return `${Math.round(ms)} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

/** One decimal at most, so `99.9%` reads cleanly next to a flat `100%`. */
export function formatUptimePct(pct: number): string {
  if (!Number.isFinite(pct)) return "—";
  return `${Number.isInteger(pct) ? pct : Number(pct.toFixed(1))}%`;
}

export function timeAgo(ms: number): string {
  const seconds = Math.floor((Date.now() - ms) / 1000);
  if (seconds < 60) return "just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

/**
 * The backend floors the interval at 60s, so the shortest choice is a minute —
 * offering "30 seconds" would only produce a rejected create.
 */
export const INTERVAL_CHOICES = [
  { secs: 60, label: "1m" },
  { secs: 300, label: "5m" },
  { secs: 900, label: "15m" },
  { secs: 3600, label: "1h" },
] as const;

export function formatInterval(secs: number): string {
  const known = INTERVAL_CHOICES.find((choice) => choice.secs === secs);
  if (known) return known.label;
  if (secs % 3600 === 0) return `${secs / 3600}h`;
  if (secs % 60 === 0) return `${secs / 60}m`;
  return `${secs}s`;
}

/** Accept only what the backend can actually fetch, before it round-trips. */
export function isFetchableUrl(raw: string): boolean {
  try {
    const url = new URL(raw.trim());
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}
