/**
 * Presentation rules for the audit log.
 *
 * Everything here is pure. The interesting one is [`describeCoverage`]: the
 * table is pruned by age and the list is capped by the command, so a screen
 * that just renders rows would quietly imply "this is everything that ever
 * happened". Two different truncations, neither of them visible in the rows
 * themselves — so both are stated in words instead.
 */
import type { AuditEntry, AuditLog } from "@/shared/ipc/client";

/**
 * Human labels for the actions `devos_kernel::audit::AuditEvent` emits.
 *
 * A plain lookup rather than a total `Record` over a union, because `action`
 * crosses IPC as a `string`: the kernel owns the vocabulary and a frontend
 * that is older than the backend it boots against will meet identifiers it has
 * never heard of. Those fall through to [`describeAction`]'s derivation, which
 * is always readable — an unlabelled event still has to render as *something*
 * a person can act on, and "unknown action" is not it.
 */
const ACTION_LABEL: Record<string, string> = {
  "ai.tool.approved": "AI tool approved",
  "ai.tool.denied": "AI tool denied",
  "secret.set": "Secret stored",
  "secret.deleted": "Secret deleted",
  "db.write": "Database write",
  "backup.restored": "Database restored",
  "backup.restore_refused": "Restore refused",
  "issue.created": "Issue filed",
};

/**
 * `ai.tool.denied` -> `AI tool denied`; an identifier this build has never
 * seen -> a sentence derived from its own segments rather than a shrug.
 */
export function describeAction(action: string): string {
  const known = ACTION_LABEL[action];
  if (known) return known;
  const words = action.replace(/[._]/g, " ").trim();
  if (!words) return "Unknown event";
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/**
 * Who did it, in the first person the reader thinks in. The `ai` / `user`
 * split is most of why anyone opens this screen: "did I do that, or did the
 * model do it with my permission?"
 */
export function describeActor(actor: string): string {
  switch (actor) {
    case "user":
      return "You";
    case "ai":
      return "AI assistant";
    case "system":
      return "DevOS";
    default:
      return actor;
  }
}

/**
 * Whether an entry records something that did *not* happen — a refused tool
 * call, a refused restore. Rendered differently because scanning for the
 * refusals is the common reason to be here, and reading an outcome off the
 * action identifier is exactly why the outcome is part of that identifier
 * instead of buried in the detail text.
 */
export function isRefusal(entry: AuditEntry): boolean {
  return entry.action === "ai.tool.denied" || entry.action === "backup.restore_refused";
}

/**
 * `2026-08-06 14:03` — local, sortable, unambiguous, and matching the backups
 * list rather than inventing a second format on the same page. Deliberately
 * not "3 hours ago": every use of this screen is reconstructing a sequence of
 * events, and relative times cannot be lined up against anything else.
 */
export function formatAuditTime(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return "Unknown date";
  const at = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return (
    `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}` +
    ` ${pad(at.getHours())}:${pad(at.getMinutes())}`
  );
}

/** `2026-08-06`, for the "reaches back to" phrase where the clock is noise. */
export function formatAuditDate(ms: number): string {
  return formatAuditTime(ms).slice(0, 10);
}

/**
 * One sentence describing what the list below is, and is not.
 *
 * Retention is a policy the user did not choose and cannot see from the rows,
 * so it is said out loud. Both truncations are covered:
 *
 * - **age** — entries past `retentionDays` are gone, which is why "the oldest
 *   entry is from X" is stated rather than left to be inferred from the last
 *   row on screen;
 * - **page** — the command caps how many rows come back, so when `total`
 *   exceeds what arrived the sentence says so instead of letting a capped
 *   list read as a complete one.
 */
export function describeCoverage(log: AuditLog): string {
  const kept = `Entries are kept for ${log.retentionDays} days, then removed the next time DevOS starts.`;
  if (log.total === 0) return kept;

  const shown = log.entries.length;
  const noun = log.total === 1 ? "entry" : "entries";
  const scope =
    log.total > shown
      ? `Showing the newest ${shown} of ${log.total} ${noun}.`
      : `${log.total} ${noun}.`;
  const reach =
    log.oldest === null
      ? ""
      : ` The record reaches back to ${formatAuditDate(log.oldest)}.`;
  return `${scope}${reach} ${kept}`;
}
