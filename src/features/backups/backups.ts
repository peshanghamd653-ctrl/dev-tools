/**
 * Presentation rules for the backups list.
 *
 * Everything here is pure so it can be tested without a shell: what a backup
 * kind is called, how a timestamp reads, and — the one with teeth — whether
 * the typed confirmation actually authorises a restore.
 */
import type { BackupEntry, BackupKind } from "@/shared/ipc/client";

/**
 * A total record rather than a switch: adding a kind to the Rust enum makes
 * `pnpm typecheck` fail here instead of rendering `undefined` in the UI.
 */
export const KIND_LABEL: Record<BackupKind, string> = {
  daily: "Daily",
  preMigration: "Before migration",
  replaced: "Replaced by a restore",
  unknown: "Unrecognised",
};

/** One line saying where a backup came from, shown under its name. */
export const KIND_HINT: Record<BackupKind, string> = {
  daily: "Taken automatically the first time DevOS started that day.",
  preMigration:
    "Taken just before a schema migration ran, so the migration can be undone.",
  replaced:
    "The database that was in use before a restore replaced it. Restore this to undo that restore.",
  unknown:
    "Not written by DevOS — something placed this file in the backups folder. It is still checked before it can be restored.",
};

/**
 * Newest first, name descending as the tie-break.
 *
 * The backend already sorts, and this sorts again anyway: the list is a plain
 * array over IPC with no ordering guarantee in its type, and a list of
 * restore candidates in a surprising order is how somebody picks the wrong
 * one. Sorting twice costs nothing; trusting the wire costs a database.
 */
export function sortBackups(entries: readonly BackupEntry[]): BackupEntry[] {
  return [...entries].sort(
    (a, b) => b.modifiedAt - a.modifiedAt || b.name.localeCompare(a.name),
  );
}

/**
 * `2026-08-06 14:03` — local, sortable, and unambiguous.
 *
 * Deliberately not "3 hours ago": a relative time is fine for a notification
 * you are reading now and wrong for choosing which snapshot of your data to
 * keep. `0` is the backend's "the platform would not tell me", which must not
 * render as 1970.
 */
export function formatBackupTime(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return "Unknown date";
  const at = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return (
    `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}` +
    ` ${pad(at.getHours())}:${pad(at.getMinutes())}`
  );
}

/** The word someone has to type to unlock the restore button. */
export const CONFIRM_WORD = "restore";

/**
 * Whether the typed confirmation counts.
 *
 * Case and surrounding whitespace are forgiven because they are not the point
 * — the point is that the button cannot be reached by a single mis-click on a
 * destructive, unrecoverable action, the same reasoning as the SQL write
 * toggle. Anything other than the word itself does not count, including the
 * empty string, so a dialog that opens with an empty field opens disarmed.
 */
export function isRestoreConfirmed(typed: string): boolean {
  return typed.trim().toLowerCase() === CONFIRM_WORD;
}
