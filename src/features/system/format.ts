/**
 * Presentation helpers for the system strip. Raw second counts and raw
 * percentages never reach the UI — a snapshot reports `uptimeSecs: 273_120`,
 * and nobody reads that as three days at a glance.
 *
 * Byte formatting used to live here too. It moved to `@/shared/lib/format`
 * when three other pages turned out to be carrying their own truncated copies
 * of it; this file keeps only what is genuinely about the system strip.
 */

/**
 * `273_120` -> `3d 3h 52m`. Seconds only appear below a minute, because an
 * uptime measured in days doesn't need second-level precision on a card that
 * refreshes every two seconds anyway.
 */
export function formatUptime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "—";
  const total = Math.floor(seconds);
  if (total < 60) return `${total}s`;

  const days = Math.floor(total / 86_400);
  const hours = Math.floor((total % 86_400) / 3600);
  const minutes = Math.floor((total % 3600) / 60);

  const parts: string[] = [];
  if (days > 0) parts.push(`${days}d`);
  if (hours > 0) parts.push(`${hours}h`);
  if (minutes > 0 || parts.length === 0) parts.push(`${minutes}m`);
  return parts.join(" ");
}

/** One decimal at most — `42` stays `42%`, `42.37` becomes `42.4%`. */
export function formatPercent(value: number): string {
  if (!Number.isFinite(value)) return "—";
  return `${Number.isInteger(value) ? value : Number(value.toFixed(1))}%`;
}

/** Percentage for a usage bar, clamped so a bad reading can't overflow it. */
export function usageRatio(used: number, total: number): number {
  if (!Number.isFinite(used) || !Number.isFinite(total) || total <= 0) return 0;
  return Math.min(100, Math.max(0, (used / total) * 100));
}

/**
 * The emerald/yellow/red ladder the rest of the app uses for health, applied
 * to load: comfortable, getting tight, about to hurt.
 */
export function loadColor(ratio: number): string {
  if (ratio >= 90) return "bg-red-500";
  if (ratio >= 75) return "bg-yellow-500";
  return "bg-emerald-500";
}
