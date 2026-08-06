/**
 * Presentation helpers for the system strip. Raw byte counts and raw second
 * counts never reach the UI — a snapshot reports `memUsed: 13_958_643_712`,
 * and nobody reads that as 13 GB at a glance.
 */

const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"] as const;

/** `13_958_643_712` -> `13.0 GB`. Binary units, matching what the OS reports. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const exponent = Math.min(
    Math.floor(Math.log(bytes) / Math.log(1024)),
    UNITS.length - 1,
  );
  const value = bytes / 1024 ** exponent;
  // Whole bytes have no fraction; past three digits the decimal is noise.
  const decimals = exponent === 0 ? 0 : value >= 100 ? 0 : 1;
  return `${value.toFixed(decimals)} ${UNITS[exponent]}`;
}

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
