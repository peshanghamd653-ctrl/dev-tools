/**
 * Cross-feature presentation helpers.
 *
 * `formatBytes` lives here rather than in a feature because four screens need
 * it — the system strip, the API client's response meta, the database schema
 * summary and the file explorer's preview header. It previously existed as one
 * good implementation in `features/system/format.ts` and three module-private
 * `formatSize` copies that capped at MB, so a 500 GB disk rendered as
 * "512000.0 MB". One exported function, unit-tested directly, is the fix.
 */

const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"] as const;

/**
 * `13_958_643_712` -> `13.0 GB`. Binary units, matching what the OS reports.
 *
 * Rounding rule: whole bytes have no fraction, and past three digits the
 * decimal is noise — `512 GB`, not `512.0 GB`. Non-finite or non-positive
 * input renders as `0 B` rather than `NaN B` or a negative size, because every
 * caller is displaying a `u64` byte count that arrived over IPC and a bad
 * reading should degrade to a harmless zero.
 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const exponent = Math.min(
    Math.floor(Math.log(bytes) / Math.log(1024)),
    UNITS.length - 1,
  );
  const value = bytes / 1024 ** exponent;
  const decimals = exponent === 0 ? 0 : value >= 100 ? 0 : 1;
  return `${value.toFixed(decimals)} ${UNITS[exponent]}`;
}
