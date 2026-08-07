/**
 * Classification of database errors arriving from the kernel.
 *
 * ## What crosses the boundary
 *
 * Every `db_*` command returns `Result<T, DbErrorDto>` (see
 * `src-tauri/src/db_commands.rs`), so a rejection is an object, not a string:
 *
 * ```json
 * { "kind": "writeBlocked", "message": "write blocked: INSERT modifies the database…" }
 * ```
 *
 * `kind` is generated from `DbErrorKind` in `crates/modules/devos-db/src/error.rs`
 * and is the only thing branched on here. `message` is for the human — it is
 * shown verbatim and never parsed. This replaces an earlier version that
 * recovered the distinction by looking for the words "write" and "block" in
 * the message, which made a Rust `#[error(...)]` attribute load-bearing prose:
 * rewording it downgraded the database page's explanatory card to a raw dump
 * with nothing failing anywhere.
 *
 * ## Rejections that are not that shape
 *
 * A rejection can still arrive as something else — a panic in the command
 * layer, a serialization failure, the shell's own "IPC is only available
 * inside the desktop shell", a frontend built before the backend it is talking
 * to. **Those are treated as ordinary failures, with no fallback to reading
 * the text.** Reasons, in the order they mattered:
 *
 * 1. By construction, nothing that reaches the webview *without* the DTO shape
 *    originated in `DbError`, so an unknown rejection is not a refused write.
 *    Guessing from prose could only ever produce a false positive.
 * 2. The two failure modes are not symmetric. Mis-reading an unknown rejection
 *    as "write blocked" shows a confident, wrong explanation and offers a
 *    button that switches writes *on* — for something that had nothing to do
 *    with writing. Missing a classification costs the styling and the
 *    shortcut; the message itself is still displayed either way, so nothing is
 *    hidden from the user.
 * 3. A prose fallback would keep the coupling this exists to remove. It would
 *    sit there passing its tests while the mechanism it backs up rotted.
 *
 * Drift in the *shape* — the other risk of dropping the fallback — fails
 * loudly instead of silently: `KNOWN_KINDS` below is a total `Record` over the
 * generated union, so renaming or adding a variant in Rust breaks
 * `pnpm typecheck` rather than quietly ceasing to match at runtime.
 */
import type { DbErrorDto } from "@/shared/ipc/bindings/DbErrorDto";
import type { DbErrorKind } from "@/shared/ipc/bindings/DbErrorKind";

/**
 * Every kind the Rust enum declares. A total `Record` rather than a list or a
 * `Set` because those accept a stale spelling silently — this fails to compile
 * when `DbErrorKind` gains, loses or renames a variant.
 */
const KNOWN_KINDS: Record<DbErrorKind, true> = {
  writeBlocked: true,
  invalid: true,
  notFound: true,
  database: true,
};

function isKnownKind(kind: string): kind is DbErrorKind {
  return KNOWN_KINDS[kind as DbErrorKind] === true;
}

/**
 * The rejection as a `DbErrorDto`, or `null` if it is anything else.
 *
 * Structural rather than nominal — the value crossed a JSON boundary, so there
 * is no class to test against — but strict about it: both fields present, both
 * the right type, and `kind` one this build knows.
 */
function asDbError(error: unknown): DbErrorDto | null {
  if (typeof error !== "object" || error === null) return null;
  const { kind, message } = error as { kind?: unknown; message?: unknown };
  if (typeof kind !== "string" || typeof message !== "string") return null;
  return isKnownKind(kind) ? { kind, message } : null;
}

/**
 * True when an error means "the connection refused to write", as opposed to an
 * ordinary SQL failure. The results pane uses this to point at the write
 * toggle instead of dumping the raw message.
 *
 * Note that the backend also reports SQLite's *own* refusal on a read-only
 * handle as `writeBlocked` — the case where a write disguised as a read gets
 * past statement classification and is stopped by the connection instead. The
 * user is in the same position either way, so the frontend does not need to
 * know which guard fired.
 */
export function isWriteBlocked(error: unknown): boolean {
  return asDbError(error)?.kind === "writeBlocked";
}

/**
 * The text to show the user. The DTO's own message when there is one, and
 * whatever the rejection stringifies to when there isn't — an unrecognized
 * failure still has to be readable.
 */
export function dbErrorMessage(error: unknown): string {
  return asDbError(error)?.message ?? String(error);
}
