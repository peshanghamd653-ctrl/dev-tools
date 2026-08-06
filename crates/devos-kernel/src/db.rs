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
