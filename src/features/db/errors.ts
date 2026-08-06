/**
 * Classification of database errors that arrive from the kernel as bare text.
 *
 * ## The coupling, stated plainly
 *
 * `crates/modules/devos-db/src/lib.rs` declares:
 *
 * ```rust
 * #[error("write blocked: {0}")]
 * WriteBlocked(String),
 * ```
 *
 * raised by `run_query` in `crates/modules/devos-db/src/query.rs`, and
 * `src-tauri/src/db_commands.rs` flattens it with `.map_err(|e| e.to_string())`
 * on its way to the webview. By the time it reaches this file it is a string
 * with no discriminant, so the only thing left to match on is its wording.
 *
 * **Changing that `#[error(...)]` attribute changes this file's behaviour.**
 * A reword that drops either the word "write" or the word "block" silently
 * downgrades the database page's explanatory card back to a raw error dump —
 * nothing fails, the UI just gets worse. Grep for "write blocked" before
 * touching the Rust variant and you will land here.
 *
 * The real fix is a typed discriminant over IPC (an error DTO carrying a
 * `kind` field, or a `WriteBlocked` marker the frontend can test for) so this
 * module can stop guessing. That needs a backend change; this is the best the
 * frontend can do alone, made explicit and tested rather than hidden inside a
 * hook.
 */

/**
 * Both must appear for a message to count as a refused write. Two loose tokens
 * rather than the literal "write blocked:" prefix is deliberate: it survives
 * "writes blocked", "write is blocked" and similar rewordings of the Rust
 * variant, which is the failure mode most likely to happen by accident.
 */
const WRITE_BLOCKED_TOKENS = ["write", "block"] as const;

/**
 * SQLite's own wording when a read-only handle refuses a statement outright
 * ("attempt to write a readonly database"). That path reaches the frontend as
 * `DbError::Db(sqlx::Error)`, not `WriteBlocked`, but it means the same thing
 * to the user and deserves the same card. Both spellings appear in the wild.
 */
const READ_ONLY_MARKERS = ["read-only", "readonly"] as const;

/**
 * True when an error means "the connection refused to write", as opposed to
 * an ordinary SQL failure. The results pane uses this to point at the write
 * toggle instead of dumping the raw message.
 */
export function isWriteBlocked(error: unknown): boolean {
  const message = String(error).toLowerCase();
  return (
    WRITE_BLOCKED_TOKENS.every((token) => message.includes(token)) ||
    READ_ONLY_MARKERS.some((marker) => message.includes(marker))
  );
}
