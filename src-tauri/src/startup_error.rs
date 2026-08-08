//! What the user sees when DevOS cannot start.
//!
//! Before this existed, every startup failure looked identical from outside:
//! the process exited with code 101 and no window ever appeared. Tauri's setup
//! hook turns an `Err` into a panic, and a release binary is built for the
//! Windows subsystem, so it has no console to print to. The panic message went
//! nowhere. Double-clicking the icon simply did nothing, forever, with no
//! indication that a fix existed.
//!
//! That is the worst failure mode an application can have, because it denies
//! the user the one thing they need — a name for the problem. A corrupt
//! database, a full disk, a file held open by another copy, and a migration
//! mismatch all presented as "nothing happens".
//!
//! The migration case is the one that prompted this. A build whose embedded
//! migrations hash differently from the ones recorded in an existing database
//! refuses to open it. The data is intact and a backup is sitting in a folder
//! the user could restore by hand in thirty seconds — but nothing on screen
//! says so.

use std::fmt::Display;
use std::path::Path;

use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

/// Report a fatal startup failure to the user, then exit.
///
/// Returns `!` because there is no recovering in-process: the kernel is the
/// thing that failed, and every command handler assumes it exists.
///
/// Exits with code 1 rather than panicking. 101 is Rust's "this program has a
/// bug" code, and it is the wrong signal for "your disk is full" — a launcher
/// or a support conversation should be able to tell those apart.
pub fn fatal(
    handle: &tauri::AppHandle,
    stage: &str,
    error: &dyn Display,
    data_dir: Option<&Path>,
) -> ! {
    let detail = error.to_string();

    // Logged first and unconditionally. The dialog is for the user; this line
    // is for whoever reads a bug report, and it has to survive the case where
    // the dialog itself cannot be shown.
    tracing::error!(stage, error = %detail, "fatal startup failure");

    let body = build_message(stage, &detail, data_dir);

    handle
        .dialog()
        .message(&body)
        .title("DevOS cannot start")
        .kind(MessageDialogKind::Error)
        .buttons(MessageDialogButtons::Ok)
        // Blocking on purpose: the process is about to exit, and a dialog the
        // user never gets to read is no better than the silent exit this
        // replaces.
        .blocking_show();

    std::process::exit(1);
}

/// The text of the dialog, split out so it can be tested without a window.
fn build_message(stage: &str, detail: &str, data_dir: Option<&Path>) -> String {
    let mut out = format!("DevOS could not start.\n\nFailed while {stage}:\n{detail}\n");

    // A checksum mismatch is worth special handling because it is the one
    // failure here where the user's data is completely intact and the fix is
    // easy — but the raw sqlx wording ("previously applied but has been
    // modified") reads like corruption and invites someone to delete their
    // database.
    if is_migration_mismatch(detail) {
        out.push_str(
            "\nThis does not mean your data is damaged. It means this version of \
             DevOS was built from different sources than the version that created \
             your database, so it will not risk touching it.\n\n\
             This usually happens after switching between an official release and \
             a build you compiled yourself. Installing the matching version again \
             is the safest fix.\n",
        );
    }

    if let Some(dir) = data_dir {
        out.push_str(&format!("\nYour data is in:\n{}\n", dir.display()));
        out.push_str(&format!(
            "\nAutomatic backups are in:\n{}\n\nNothing there has been deleted.",
            dir.join("backups").display()
        ));
    }

    out
}

/// True for the sqlx checksum-mismatch failure.
///
/// Matched on substrings rather than an error type because it arrives as an
/// opaque `MigrateError` flattened into a boxed error by the time it reaches
/// the setup hook. If sqlx rewords this, the dialog quietly loses its extra
/// paragraph and still says everything else — which is why the generic message
/// above has to stand on its own.
fn is_migration_mismatch(detail: &str) -> bool {
    let lower = detail.to_lowercase();
    lower.contains("previously applied but has been modified")
        || (lower.contains("migration") && lower.contains("checksum"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_stage_and_the_underlying_error() {
        let msg = build_message("opening the database", "disk I/O error", None);
        assert!(msg.contains("opening the database"));
        assert!(msg.contains("disk I/O error"));
        // No data directory was known, so it must not invent one.
        assert!(!msg.contains("Your data is in"));
    }

    #[test]
    fn points_at_the_data_directory_and_backups_when_known() {
        let dir = Path::new("C:/Users/x/AppData/Roaming/com.peshang.devos");
        let msg = build_message("opening the database", "locked", Some(dir));
        assert!(msg.contains("com.peshang.devos"));
        assert!(msg.contains("backups"));
        // The reassurance matters as much as the path: a user staring at a
        // failure is deciding whether to delete something.
        assert!(msg.contains("Nothing there has been deleted"));
    }

    #[test]
    fn explains_a_checksum_mismatch_rather_than_repeating_sqlx() {
        let msg = build_message(
            "opening the database",
            "migration error: migration 1 was previously applied but has been modified",
            None,
        );
        assert!(msg.contains("does not mean your data is damaged"));
        assert!(msg.contains("built from different sources"));
    }

    #[test]
    fn does_not_claim_data_is_safe_for_unrelated_failures() {
        // Over-reassuring about a genuinely corrupt file would be worse than
        // saying nothing, so the paragraph is gated on the specific error.
        let msg = build_message(
            "opening the database",
            "database disk image is malformed",
            None,
        );
        assert!(!msg.contains("does not mean your data is damaged"));
    }

    #[test]
    fn recognises_the_mismatch_wording_and_nothing_else() {
        assert!(is_migration_mismatch(
            "migration 1 was previously applied but has been modified"
        ));
        assert!(is_migration_mismatch("migration checksum mismatch"));
        assert!(!is_migration_mismatch("no such table: workspaces"));
        assert!(!is_migration_mismatch("permission denied"));
    }
}
