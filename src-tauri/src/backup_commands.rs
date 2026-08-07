//! Backups and restore.
//!
//! Thin, like every other command module — the mechanism lives in
//! [`devos_kernel::backup`]. What is *not* thin is the argument check: the
//! frontend names a backup, and a name that is allowed to be a path would let
//! any file on disk be staged as the database. So the only thing accepted here
//! is a bare file name that resolves inside the backup directory.
//!
//! There is no `backup_delete`. Rotation prunes dailies on its own, and the
//! folder is one "Reveal in Explorer" click away; an in-app delete button next
//! to a restore button is a mis-click that removes the thing you were about to
//! recover from.

use std::path::{Path, PathBuf};

use devos_kernel::backup::{self, BackupListing, PendingRestore};
use tauri::{AppHandle, State};

use crate::state::AppState;

#[tauri::command]
pub async fn backups_list(state: State<'_, AppState>) -> Result<BackupListing, String> {
    let db_path = Path::new(&state.db_path);
    Ok(BackupListing {
        dir: backup::backup_dir(db_path).display().to_string(),
        entries: backup::list_backups(db_path),
        pending: backup::pending_restore(db_path),
    })
}

/// Stage `name` to be restored on the next boot. Validates first; nothing
/// about the live database changes here.
#[tauri::command]
pub async fn backup_restore_stage(
    state: State<'_, AppState>,
    name: String,
) -> Result<PendingRestore, String> {
    let db_path = Path::new(&state.db_path);
    let backup = resolve_backup(db_path, &name)?;
    backup::stage_restore(db_path, &backup)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn backup_restore_cancel(state: State<'_, AppState>) -> Result<(), String> {
    backup::cancel_restore(Path::new(&state.db_path)).map_err(|e| e.to_string())
}

/// Restart the app so a staged restore applies.
///
/// `request_restart` rather than `restart`: called off the main thread — which
/// every command is — `restart` parks the calling thread in
/// `sleep(Duration::MAX)` until the process dies, holding a runtime worker
/// hostage for the duration. This asks the event loop to exit with the restart
/// code and returns, which is the same outcome without the parked thread.
///
/// Returning `Ok` means the restart was *requested*. The window is on its way
/// out, so there is nothing after this worth reporting to it.
#[tauri::command]
pub async fn backup_restart(app: AppHandle) -> Result<(), String> {
    app.request_restart();
    Ok(())
}

/// Turn a user-supplied name into a path inside `backups/`, or refuse.
///
/// Split out and tested directly because it is the whole security boundary of
/// this module: `stage_restore` copies whatever it is handed into the position
/// the next boot installs as the database, so "which files may be named here"
/// is the question that matters. A bare file name is the only answer that
/// cannot be argued with — `..`, an absolute path, a UNC share and a Windows
/// alternate data stream (`x.db:hidden`) are all rejected before the
/// filesystem is touched.
fn resolve_backup(db_path: &Path, name: &str) -> Result<PathBuf, String> {
    let refused = || format!("not a backup file name: {name}");
    if name.is_empty() || name.contains(['/', '\\', ':']) {
        return Err(refused());
    }
    // Catches `.` and `..`, which contain no separator and would otherwise
    // resolve to the backup directory itself.
    if Path::new(name).file_name().is_none_or(|file| file != name) {
        return Err(refused());
    }

    let path = backup::backup_dir(db_path).join(name);
    if !path.is_file() {
        return Err(format!("no backup named {name}"));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every name that must not become a path. Each of these would otherwise
    /// stage a file the user never put in the backup folder.
    #[test]
    fn only_a_bare_file_name_inside_the_backup_folder_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("devos.db");
        let backups = backup::backup_dir(&db_path);
        std::fs::create_dir_all(&backups).unwrap();
        std::fs::write(backups.join("devos-daily-2026-08-01.db"), b"x").unwrap();
        // A file one level up, standing in for anything outside the folder.
        std::fs::write(dir.path().join("elsewhere.db"), b"x").unwrap();

        assert_eq!(
            resolve_backup(&db_path, "devos-daily-2026-08-01.db").unwrap(),
            backups.join("devos-daily-2026-08-01.db")
        );

        for hostile in [
            "",
            ".",
            "..",
            "../elsewhere.db",
            "..\\elsewhere.db",
            "sub/devos-daily-2026-08-01.db",
            "sub\\devos-daily-2026-08-01.db",
            r"C:\Windows\win.ini",
            "/etc/passwd",
            r"\\server\share\devos.db",
            // Windows alternate data stream: a bare name to `file_name()`,
            // but a different byte range on disk.
            "devos-daily-2026-08-01.db:hidden",
        ] {
            assert!(
                resolve_backup(&db_path, hostile).is_err(),
                "{hostile:?} must be refused"
            );
        }
    }

    /// A name that is shaped correctly but names nothing fails as "no backup",
    /// not as a traversal attempt — the message a user gets after deleting a
    /// file in Explorer while the list was on screen.
    #[test]
    fn a_name_that_no_longer_exists_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("devos.db");
        let err = resolve_backup(&db_path, "devos-daily-1999-01-01.db").unwrap_err();
        assert!(err.contains("no backup named"), "got: {err}");
    }
}
