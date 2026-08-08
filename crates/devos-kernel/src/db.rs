use std::path::Path;
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;

use crate::error::{KernelError, KernelResult};

/// Open (creating if needed) the DevOS SQLite database and run migrations.
pub async fn open_pool(db_path: &Path) -> KernelResult<SqlitePool> {
    let pool = connect(db_path).await?;
    run_migrations(&pool).await?;
    Ok(pool)
}

/// Open the pool without migrating. Split out of [`open_pool`] so the kernel
/// can time connection setup and migrations as separate boot phases.
pub async fn connect(db_path: &Path) -> KernelResult<SqlitePool> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| KernelError::Other(format!("failed to create data dir: {e}")))?;
    }

    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await?;

    Ok(pool)
}

/// The embedded migration set. Exposed so callers can inspect it — the
/// pre-migration backup needs to know whether anything is actually pending —
/// without re-invoking `sqlx::migrate!` and embedding a second copy.
pub fn migrator() -> Migrator {
    sqlx::migrate!("./migrations")
}

/// Apply any pending embedded migrations. A no-op on an already-current DB.
pub async fn run_migrations(pool: &SqlitePool) -> KernelResult<()> {
    migrator().run(pool).await?;
    Ok(())
}

#[cfg(test)]
mod migration_checksum_tests {
    use super::*;

    /// The checksum of a released migration is frozen, and this test is the
    /// thing that keeps it frozen.
    ///
    /// sqlx records a SHA-384 of each migration's *bytes* in
    /// `_sqlx_migrations` when it first applies it, and re-checks that hash on
    /// every subsequent start. If it ever differs the app does not degrade or
    /// warn — `run()` returns `VersionMismatch`, which surfaces as a panic in
    /// Tauri's setup hook, so the process exits 101 and no window appears.
    /// There is no in-app recovery: the user sees the application fail to
    /// launch, with nothing to click.
    ///
    /// Two ways to trip that, both of which look innocent in a diff:
    ///
    ///   1. Editing an already-released migration file. Fixing a typo in a
    ///      comment is enough.
    ///   2. Changing its line endings. This one actually happened: with no
    ///      .gitattributes, a Windows CI checkout materialized CRLF while the
    ///      developer's working tree still held LF, so the v0.1.0 release
    ///      binary embedded a different checksum than a local build and
    ///      refused to open a database the local build had created. The
    ///      migration SQL was byte-identical apart from the line endings.
    ///
    /// So the value below is pinned to what shipped. It must only ever change
    /// alongside a deliberate decision about every database already in the
    /// wild — which, for a released migration, means "never". A new migration
    /// is a new file with a new version number; it does not edit this one.
    #[test]
    fn released_migration_checksums_never_change() {
        // Sorted by version so the expectations line up regardless of the
        // order the macro happens to embed them in.
        let migrator = migrator();
        let mut applied: Vec<_> = migrator.iter().collect();
        applied.sort_by_key(|m| m.version);

        let frozen: &[(i64, &str)] = &[(
            1,
            "e601a7a83a4e9b260ae843b480\
             64dd3cd72a33b2ba50bc0083c6030e1a870367ce311ef9dd9f0952a5cde6c0336c8728",
        )];

        for (version, expected) in frozen {
            let migration = applied
                .iter()
                .find(|m| m.version == *version)
                .unwrap_or_else(|| panic!("released migration {version} has been deleted"));

            let actual = migration
                .checksum
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();

            assert_eq!(
                actual,
                expected.replace(['\\', ' ', '\n'], ""),
                "migration {version} ({}) no longer hashes to the value that \
                 shipped. Every existing installation records the old checksum \
                 and will refuse to start against this build, exiting 101 with \
                 no window and no message. If the file looks unchanged, check \
                 its line endings — .gitattributes pins *.sql to LF precisely \
                 because a CRLF checkout silently produces a different hash.",
                migration.description,
            );
        }
    }
}
