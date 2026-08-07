//! The read side of the audit log.
//!
//! One command, and deliberately only one. This module exposes **no** way to
//! write, edit, delete or prune an entry from the webview: rows are appended
//! by the code paths that perform the audited actions
//! ([`devos_kernel::audit`]), and removed only by the kernel's own
//! age-based prune at boot. An IPC surface that could add or remove entries
//! would be a surface a prompt injection or a rogue frontend could use to
//! write its own alibi, and "append-only" would be a description of the
//! schema rather than a property of the system.

use devos_kernel::types::AuditLog;
use tauri::State;

use crate::state::AppState;

/// How many rows a single read may return.
///
/// The viewer is a "look when something went wrong" surface, not an archive
/// browser, and rendering an unbounded list into the webview is how a
/// long-lived install turns a diagnostic screen into a freeze.
/// [`AuditLog::total`] carries the real count, so the page can say it is
/// showing a slice rather than implying the slice is everything.
const MAX_ENTRIES: i64 = 500;

#[tauri::command]
pub async fn audit_log(state: State<'_, AppState>, limit: i64) -> Result<AuditLog, String> {
    devos_kernel::repo::audit_log(&state.kernel.pool, limit.clamp(1, MAX_ENTRIES))
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The clamp is the whole of this module's logic, so it is tested
    /// directly: a caller asking for everything gets a page, and a caller
    /// asking for nothing (or for a negative page, which SQLite reads as
    /// "no limit") still gets a bounded one.
    #[test]
    fn the_requested_limit_is_clamped_in_both_directions() {
        assert_eq!(1_000_000i64.clamp(1, MAX_ENTRIES), MAX_ENTRIES);
        assert_eq!(0i64.clamp(1, MAX_ENTRIES), 1);
        assert_eq!((-1i64).clamp(1, MAX_ENTRIES), 1);
        assert_eq!(50i64.clamp(1, MAX_ENTRIES), 50);
    }
}
