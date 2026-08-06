//! Automatic backups of the DevOS database file.
//!
//! Two triggers, both at boot, both best effort:
//!
//! - **pre-migration** — a snapshot taken immediately before `sqlx::migrate!`
//!   applies anything, so a migration that drops or mangles data can be
//!   undone by hand. Only when a migration is actually pending.
//! - **daily** — at most one snapshot per calendar day, keeping the newest
//!   [`DAILY_RETENTION`] and deleting the rest.
//!
//! Both write into a `backups/` directory beside the live database. Neither
//! can fail the boot: every error is logged and swallowed, because an app
//! that starts without a backup beats an app that refuses to start.

use std::path::{Path, PathBuf};

use chrono::Local;
use sqlx::migrate::Migrator;
use sqlx::SqlitePool;

use crate::error::{KernelError, KernelResult};

/// How many daily snapshots to keep before the oldest is deleted. A week is
/// enough to notice a bad day and roll back to before it, without the backup
/// directory growing to several times the size of the database itself.
pub const DAILY_RETENTION: usize = 7;

const BACKUP_DIR: &str = "backups";
/// `devos-daily-YYYY-MM-DD.db` — fixed-width date, so lexical order over the
/// file names *is* chronological order and rotation needs no `mtime` lookups.
const DAILY_PREFIX: &str = "devos-daily-";
const PRE_MIGRATION_PREFIX: &str = "devos-premigration-";

/// The `backups/` directory beside `db_path`.
pub fn backup_dir(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(BACKUP_DIR)
}

/// Snapshot the database before migrations run. Best effort — returns the
/// backup path when one was written, `None` when it was skipped or failed.
///
/// Call this *immediately* before `Migrator::run`, with the same migrator.
pub async fn run_pre_migration_backup(
    pool: &SqlitePool,
    db_path: &Path,
    migrator: &Migrator,
) -> Option<PathBuf> {
    match try_pre_migration_backup(pool, db_path, migrator).await {
        Ok(Some(path)) => {
            tracing::info!(backup = %path.display(), "pre-migration database backup written");
            Some(path)
        }
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(error = %e, "pre-migration database backup failed; continuing to migrate");
            None
        }
    }
}

/// Snapshot the database once per calendar day, then prune to
/// [`DAILY_RETENTION`]. Best effort — returns the backup path only when this
/// call is the one that wrote it.
pub async fn run_daily_backup(pool: &SqlitePool, db_path: &Path) -> Option<PathBuf> {
    match try_daily_backup(pool, db_path).await {
        Ok(Some(path)) => {
            tracing::info!(backup = %path.display(), "daily database backup written");
            Some(path)
        }
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(error = %e, "daily database backup failed; continuing without it");
            None
        }
    }
}

async fn try_pre_migration_backup(
    pool: &SqlitePool,
    db_path: &Path,
    migrator: &Migrator,
) -> KernelResult<Option<PathBuf>> {
    if !db_path.is_file() {
        tracing::debug!("no database file yet, skipping pre-migration backup");
        return Ok(None);
    }

    let (applied_any, pending) = migration_state(pool, migrator).await;
    if !applied_any {
        // First run. The pool is opened with `create_if_missing`, so the file
        // already exists by the time we hold a pool to query through — an
        // empty `_sqlx_migrations` is the honest first-run signal, and a
        // brand new database has nothing worth preserving.
        tracing::debug!("first run, skipping pre-migration backup");
        return Ok(None);
    }
    if !pending {
        // Steady-state boot. Copying here would mean a snapshot on every
        // single launch; that job belongs to the daily rotation.
        return Ok(None);
    }

    let target = migrator.iter().map(|m| m.version).max().unwrap_or_default();
    let dir = backup_dir(db_path);
    // Timestamp first so these sort chronologically alongside each other;
    // the target version says what the snapshot is "before".
    let dest = dir.join(format!(
        "{PRE_MIGRATION_PREFIX}{}-v{target:04}.db",
        Local::now().format("%Y%m%d-%H%M%S")
    ));
    create_dir(&dir)?;
    snapshot(pool, db_path, &dest).await?;
    Ok(Some(dest))
}

async fn try_daily_backup(pool: &SqlitePool, db_path: &Path) -> KernelResult<Option<PathBuf>> {
    if !db_path.is_file() {
        tracing::debug!("no database file yet, skipping daily backup");
        return Ok(None);
    }

    let dir = backup_dir(db_path);
    let dest = dir.join(format!(
        "{DAILY_PREFIX}{}.db",
        Local::now().format("%Y-%m-%d")
    ));
    if dest.exists() {
        // Already backed up today. The file name is the whole record of that
        // — no state to keep in `settings`, and deleting the file is a valid
        // way for the user to ask for a fresh one.
        return Ok(None);
    }

    create_dir(&dir)?;
    snapshot(pool, db_path, &dest).await?;
    prune_daily(&dir);
    Ok(Some(dest))
}

/// `(any migration applied, any migration pending)`.
async fn migration_state(pool: &SqlitePool, migrator: &Migrator) -> (bool, bool) {
    // A missing `_sqlx_migrations` is not an error here: it just means sqlx
    // has never migrated this file, which is exactly the fresh-database case.
    let applied: Vec<i64> = sqlx::query_scalar("SELECT version FROM _sqlx_migrations")
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    let pending = migrator.iter().any(|m| !applied.contains(&m.version));
    (!applied.is_empty(), pending)
}

/// Write a consistent snapshot of the live database to `dest`.
///
/// The database runs in WAL mode ([ADR-0004]), which is what makes this
/// non-trivial: committed transactions live in `devos.db-wal` until a
/// checkpoint folds them back into `devos.db`. Copying the main file on its
/// own therefore yields a backup that is silently missing the most recent
/// commits, and — if a checkpoint is writing pages while the copy reads them
/// — possibly a torn one. A backup like that is worse than no backup, because
/// it will be trusted.
///
/// `VACUUM INTO` is the correct answer: SQLite runs it inside a read
/// transaction, so the file it produces contains everything committed at that
/// instant (WAL frames included), is self-contained, and the live database is
/// never modified — no checkpoint forced on a running app, no writer blocked
/// beyond the read transaction. It has existed since SQLite 3.27 (2019) and
/// the `libsqlite3-sys` bundled by sqlx 0.8 is far newer, so this is the path
/// that actually runs; [`checkpoint_and_copy`] exists for a build linked
/// against an older system SQLite.
///
/// [ADR-0004]: ../../../docs/adr/0004-single-sqlite-file-with-wal.md
async fn snapshot(pool: &SqlitePool, db_path: &Path, dest: &Path) -> KernelResult<()> {
    if supports_vacuum_into(pool).await {
        vacuum_into(pool, dest).await
    } else {
        tracing::warn!("SQLite predates VACUUM INTO; falling back to checkpoint + file copy");
        checkpoint_and_copy(pool, db_path, dest).await
    }
}

async fn supports_vacuum_into(pool: &SqlitePool) -> bool {
    let version: String = match sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(pool)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "could not read sqlite_version()");
            return false;
        }
    };
    let mut parts = version.split('.').map(str::parse::<u32>);
    match (parts.next(), parts.next()) {
        (Some(Ok(major)), Some(Ok(minor))) => (major, minor) >= (3, 27),
        _ => false,
    }
}

async fn vacuum_into(pool: &SqlitePool, dest: &Path) -> KernelResult<()> {
    // Bound, not interpolated: the destination is a filesystem path we build,
    // but SQL string-building around user-controlled data directories is a
    // habit not worth acquiring. VACUUM cannot run inside a transaction, and
    // sqlx does not wrap a bare `execute` in one.
    let dest_str = dest.to_str().ok_or_else(|| {
        KernelError::Other(format!("backup path is not UTF-8: {}", dest.display()))
    })?;
    sqlx::query("VACUUM INTO ?")
        .bind(dest_str)
        .execute(pool)
        .await?;
    Ok(())
}

/// Fallback for a SQLite older than 3.27.
///
/// `wal_checkpoint(TRUNCATE)` blocks until every committed frame has been
/// written back into the main database file and the WAL is emptied, so the
/// copy that follows contains all of them. Any writer that commits in the gap
/// between the checkpoint and the copy lands in a fresh WAL, so that file is
/// copied alongside as `<dest>-wal`: SQLite recovers it on next open, which
/// turns a lost commit into a recovered one.
async fn checkpoint_and_copy(pool: &SqlitePool, db_path: &Path, dest: &Path) -> KernelResult<()> {
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(pool)
        .await?;
    std::fs::copy(db_path, dest).map_err(|e| io_err("copy database to", dest, &e))?;

    let wal = wal_sidecar(db_path);
    if std::fs::metadata(&wal)
        .map(|m| m.len() > 0)
        .unwrap_or(false)
    {
        let dest_wal = wal_sidecar(dest);
        std::fs::copy(&wal, &dest_wal)
            .map_err(|e| io_err("copy write-ahead log to", &dest_wal, &e))?;
    }
    Ok(())
}

/// Delete the oldest daily backups until only [`DAILY_RETENTION`] remain.
fn prune_daily(dir: &Path) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(error = %e, dir = %dir.display(), "could not list backups to prune");
            return;
        }
    };
    let mut daily: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_daily_backup(path))
        .collect();
    // Chronological, because the date in the name is fixed-width ISO-8601.
    daily.sort();

    let excess = daily.len().saturating_sub(DAILY_RETENTION);
    for old in daily.iter().take(excess) {
        match std::fs::remove_file(old) {
            Ok(()) => tracing::debug!(backup = %old.display(), "pruned old daily backup"),
            Err(e) => tracing::warn!(error = %e, backup = %old.display(), "could not prune backup"),
        }
        // Only present if the checkpoint fallback wrote one.
        let _ = std::fs::remove_file(wal_sidecar(old));
    }
}

fn is_daily_backup(path: &Path) -> bool {
    path.is_file()
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(DAILY_PREFIX) && name.ends_with(".db"))
}

/// `foo.db` -> `foo.db-wal`, the name SQLite gives the write-ahead log.
fn wal_sidecar(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push("-wal");
    PathBuf::from(name)
}

fn create_dir(dir: &Path) -> KernelResult<()> {
    std::fs::create_dir_all(dir).map_err(|e| io_err("create backup directory", dir, &e))
}

fn io_err(what: &str, path: &Path, e: &std::io::Error) -> KernelError {
    KernelError::Other(format!("failed to {what} {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    /// A real WAL-mode DevOS database with one extra table for test markers.
    async fn wal_db(dir: &Path) -> (PathBuf, SqlitePool) {
        let db_path = dir.join("devos.db");
        let pool = crate::db::open_pool(&db_path).await.expect("open pool");
        sqlx::query("CREATE TABLE backup_marker (note TEXT NOT NULL)")
            .execute(&pool)
            .await
            .expect("create marker table");
        (db_path, pool)
    }

    async fn commit_marker(pool: &SqlitePool, note: &str) {
        sqlx::query("INSERT INTO backup_marker (note) VALUES (?)")
            .bind(note)
            .execute(pool)
            .await
            .expect("insert marker");
    }

    /// Markers readable from a standalone database file. `Err` for "this file
    /// isn't a usable database", which is what a naive mid-WAL copy produces.
    async fn markers_in(path: &Path) -> Result<Vec<String>, sqlx::Error> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(false)
            .read_only(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        let notes = sqlx::query_scalar("SELECT note FROM backup_marker ORDER BY note")
            .fetch_all(&pool)
            .await;
        pool.close().await;
        notes
    }

    fn daily_backups(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("read backup dir")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// The assumption the whole module rests on: the SQLite sqlx links
    /// against is new enough for `VACUUM INTO`. If this ever fails, the
    /// checkpoint fallback is what runs — and it has its own test below.
    #[tokio::test]
    async fn linked_sqlite_supports_vacuum_into() {
        let dir = tempfile::tempdir().unwrap();
        let (_db_path, pool) = wal_db(dir.path()).await;
        assert!(
            supports_vacuum_into(&pool).await,
            "sqlx 0.8 is expected to bundle SQLite >= 3.27"
        );
    }

    /// The WAL correctness case: a row committed immediately before the
    /// backup must be *in* the backup, even though it is still sitting in
    /// `devos.db-wal` and not in `devos.db` at all.
    #[tokio::test]
    async fn backup_contains_a_row_committed_moments_before_it() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, pool) = wal_db(dir.path()).await;
        commit_marker(&pool, "committed-just-before-backup").await;

        let backup = run_daily_backup(&pool, &db_path).await.expect("backup");
        assert_eq!(
            markers_in(&backup)
                .await
                .expect("backup is a valid database"),
            vec!["committed-just-before-backup".to_string()],
        );

        // And prove the problem is real rather than hypothetical: the naive
        // "just copy devos.db" backup that VACUUM INTO replaces does not have
        // the row (it is still in the uncheckpointed WAL).
        let naive = dir.path().join("naive.db");
        std::fs::copy(&db_path, &naive).expect("naive copy");
        assert!(
            markers_in(&naive).await.unwrap_or_default().is_empty(),
            "a plain file copy of a WAL database must not be trusted; if this \
             starts passing, the test is no longer proving anything"
        );
    }

    /// Same guarantee, via the pre-3.27 path, so the fallback is not
    /// untested code that only runs on the machines we cannot reproduce.
    #[tokio::test]
    async fn checkpoint_fallback_also_captures_the_latest_commit() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, pool) = wal_db(dir.path()).await;
        commit_marker(&pool, "in-the-wal").await;

        let dest = dir.path().join("fallback.db");
        checkpoint_and_copy(&pool, &db_path, &dest)
            .await
            .expect("checkpoint + copy");
        assert_eq!(
            markers_in(&dest).await.expect("fallback backup is valid"),
            vec!["in-the-wal".to_string()],
        );
    }

    #[tokio::test]
    async fn daily_rotation_keeps_the_newest_and_deletes_the_oldest() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, pool) = wal_db(dir.path()).await;
        let backups = backup_dir(&db_path);
        std::fs::create_dir_all(&backups).unwrap();

        // Nine stale dailies, all older than today, plus one unrelated file
        // that rotation must leave alone.
        for day in 1..=9 {
            std::fs::write(
                backups.join(format!("{DAILY_PREFIX}2000-01-0{day}.db")),
                b"stale",
            )
            .unwrap();
        }
        std::fs::write(
            backups.join(format!("{PRE_MIGRATION_PREFIX}20000101-000000-v0001.db")),
            b"keep me",
        )
        .unwrap();

        let today = run_daily_backup(&pool, &db_path).await.expect("backup");

        let names = daily_backups(&backups);
        let dailies: Vec<&String> = names
            .iter()
            .filter(|n| n.starts_with(DAILY_PREFIX))
            .collect();
        assert_eq!(dailies.len(), DAILY_RETENTION, "rotation keeps exactly N");
        assert!(names.iter().any(|n| n.starts_with(PRE_MIGRATION_PREFIX)));
        assert!(today.exists(), "today's backup survives its own pruning");

        // The three oldest went; the ones just above the cut stayed.
        for day in 1..=3 {
            assert!(!backups
                .join(format!("{DAILY_PREFIX}2000-01-0{day}.db"))
                .exists());
        }
        for day in 4..=9 {
            assert!(backups
                .join(format!("{DAILY_PREFIX}2000-01-0{day}.db"))
                .exists());
        }
    }

    #[tokio::test]
    async fn at_most_one_daily_backup_per_calendar_day() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, pool) = wal_db(dir.path()).await;
        commit_marker(&pool, "before-first-backup").await;

        let first = run_daily_backup(&pool, &db_path).await.expect("backup");

        commit_marker(&pool, "after-first-backup").await;
        assert!(
            run_daily_backup(&pool, &db_path).await.is_none(),
            "second call on the same day must not back up again"
        );
        assert!(run_daily_backup(&pool, &db_path).await.is_none());

        assert_eq!(daily_backups(&backup_dir(&db_path)).len(), 1);
        assert_eq!(
            markers_in(&first).await.expect("backup is valid"),
            vec!["before-first-backup".to_string()],
            "the existing backup was left untouched, not silently rewritten"
        );
    }

    #[tokio::test]
    async fn missing_database_file_is_not_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        let (_db_path, pool) = wal_db(dir.path()).await;

        let absent = dir.path().join("nested").join("gone.db");
        assert!(run_daily_backup(&pool, &absent).await.is_none());
        assert!(
            !backup_dir(&absent).exists(),
            "nothing should be created for a database that isn't there"
        );
    }

    #[tokio::test]
    async fn unwritable_destination_is_swallowed_not_propagated() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, pool) = wal_db(dir.path()).await;
        // A regular file where the backup directory needs to go: creating it
        // can never succeed, on any platform.
        std::fs::write(backup_dir(&db_path), b"not a directory").unwrap();

        assert!(run_daily_backup(&pool, &db_path).await.is_none());
        let migrator = crate::db::migrator();
        assert!(run_pre_migration_backup(&pool, &db_path, &migrator)
            .await
            .is_none());

        // The database itself is untouched and still usable.
        commit_marker(&pool, "still-working").await;
    }

    /// Requirement 4 end to end: a broken backup path must not stop the app.
    #[tokio::test]
    async fn boot_succeeds_when_backups_cannot_be_written() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("devos.db");
        std::fs::write(backup_dir(&db_path), b"not a directory").unwrap();

        let kernel = crate::Kernel::boot(&db_path)
            .await
            .expect("boot must survive a failing backup");
        assert_eq!(
            crate::repo::list_workspaces(&kernel.pool)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn pre_migration_backup_skipped_on_first_run_and_when_up_to_date() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("devos.db");
        let migrator = crate::db::migrator();

        // Fresh file, nothing recorded in `_sqlx_migrations` yet.
        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .unwrap();
        assert!(
            run_pre_migration_backup(&pool, &db_path, &migrator)
                .await
                .is_none(),
            "a brand new database has nothing worth preserving"
        );

        // Now fully migrated: nothing pending, so nothing to snapshot either.
        migrator.run(&pool).await.unwrap();
        assert!(
            run_pre_migration_backup(&pool, &db_path, &migrator)
                .await
                .is_none(),
            "every-boot copies are the daily rotation's job"
        );
        assert!(!backup_dir(&db_path).exists());
    }

    #[tokio::test]
    async fn pre_migration_backup_runs_when_a_migration_is_pending() {
        let dir = tempfile::tempdir().unwrap();
        let (db_path, pool) = wal_db(dir.path()).await;
        commit_marker(&pool, "pre-existing-data").await;

        // Rewrite history so the embedded migrations look one step ahead of
        // what this database has applied — the upgrade case, without needing
        // a second migration file that would then have to exist forever.
        let migrator = crate::db::migrator();
        let earliest = migrator.iter().map(|m| m.version).min().unwrap();
        sqlx::query("UPDATE _sqlx_migrations SET version = version - ? WHERE version = ?")
            .bind(1000)
            .bind(earliest)
            .execute(&pool)
            .await
            .unwrap();

        let backup = run_pre_migration_backup(&pool, &db_path, &migrator)
            .await
            .expect("pending migration must trigger a backup");
        assert!(backup
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(PRE_MIGRATION_PREFIX));
        assert_eq!(
            markers_in(&backup).await.expect("backup is valid"),
            vec!["pre-existing-data".to_string()],
            "the snapshot is of the database as it stood before migrating"
        );
    }
}
