//! Pool cache. Opening a SQLite pool is expensive relative to a query, and
//! the UI issues many small queries against the same file, so pools are
//! opened lazily and kept keyed by connection id for the life of the app.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

use crate::{DbConnection, DbResult};

const MAX_CONNECTIONS: u32 = 4;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct DbManager {
    pools: Mutex<HashMap<String, SqlitePool>>,
    /// Pools opened with `SQLITE_OPEN_READONLY`. The read path runs here so
    /// that "this query may not write" is enforced by the file handle rather
    /// than by our own parsing — an open mode no statement can undo. See
    /// [`crate::query`] for why that distinction was learned the hard way.
    read_pools: Mutex<HashMap<String, SqlitePool>>,
}

impl DbManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// The pool for a saved connection, opening it on first use.
    pub async fn pool_for(&self, connection: &DbConnection) -> DbResult<SqlitePool> {
        {
            let cache = self.pools.lock().expect("db pool cache poisoned");
            if let Some(pool) = cache.get(&connection.id) {
                return Ok(pool.clone());
            }
        }

        let options = SqliteConnectOptions::new()
            .filename(&connection.path)
            // Never conjure an empty database: pointing at a path that no
            // longer exists is an error the user needs to see, not a silently
            // created blank file that looks like their data vanished.
            .create_if_missing(false)
            .busy_timeout(BUSY_TIMEOUT);
        let pool = SqlitePoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .connect_with(options)
            .await?;

        // Two callers can race here; the loser's pool is dropped rather than
        // stashed, so a connection id always maps to exactly one live pool.
        let mut cache = self.pools.lock().expect("db pool cache poisoned");
        Ok(cache.entry(connection.id.clone()).or_insert(pool).clone())
    }

    /// A genuinely read-only pool for the same file.
    ///
    /// Falls back to the read-write pool when the file cannot be opened
    /// read-only — a live WAL database whose `-shm` does not yet exist cannot
    /// be opened `SQLITE_OPEN_READONLY`, and refusing to show the user their
    /// own data would be a worse outcome. The caller's other guards (single
    /// statement, `PRAGMA query_only`) still apply on that path, so the
    /// fallback is degraded, not unguarded.
    pub async fn read_pool_for(&self, connection: &DbConnection) -> DbResult<SqlitePool> {
        {
            let cache = self.read_pools.lock().expect("db pool cache poisoned");
            if let Some(pool) = cache.get(&connection.id) {
                return Ok(pool.clone());
            }
        }

        let options = SqliteConnectOptions::new()
            .filename(&connection.path)
            .create_if_missing(false)
            .read_only(true)
            .busy_timeout(BUSY_TIMEOUT);
        let pool = match SqlitePoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .connect_with(options)
            .await
        {
            Ok(pool) => pool,
            Err(error) => {
                tracing::warn!(
                    %error,
                    connection = %connection.id,
                    "read-only open failed; falling back to the read-write pool"
                );
                return self.pool_for(connection).await;
            }
        };

        let mut cache = self.read_pools.lock().expect("db pool cache poisoned");
        Ok(cache.entry(connection.id.clone()).or_insert(pool).clone())
    }

    /// Look up a saved connection and hand back both of its pools:
    /// `(connection, read_only, read_write)`.
    pub async fn resolve(
        &self,
        devos_pool: &SqlitePool,
        id: &str,
    ) -> DbResult<(DbConnection, SqlitePool, SqlitePool)> {
        let connection = crate::repo::get_connection(devos_pool, id).await?;
        let write_pool = self.pool_for(&connection).await?;
        let read_pool = self.read_pool_for(&connection).await?;
        Ok((connection, read_pool, write_pool))
    }

    /// Forget a connection's pools. File handles close as the last clone
    /// drops, which matters when the underlying file is about to be replaced.
    pub fn evict(&self, id: &str) {
        self.pools
            .lock()
            .expect("db pool cache poisoned")
            .remove(id);
        self.read_pools
            .lock()
            .expect("db pool cache poisoned")
            .remove(id);
    }

    #[cfg(test)]
    fn cached(&self) -> usize {
        self.pools.lock().expect("db pool cache poisoned").len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DbError;

    fn connection(path: &std::path::Path) -> DbConnection {
        DbConnection {
            id: "conn-1".into(),
            name: "Test".into(),
            driver: "sqlite".into(),
            path: path.to_string_lossy().into_owned(),
            created_at: 0,
        }
    }

    async fn make_db(path: &std::path::Path) {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE t (a INTEGER)")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }

    #[tokio::test]
    async fn caches_pool_per_connection_and_evicts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.sqlite");
        make_db(&path).await;
        let connection = connection(&path);

        let manager = DbManager::new();
        assert_eq!(manager.cached(), 0);
        let first = manager.pool_for(&connection).await.unwrap();
        let second = manager.pool_for(&connection).await.unwrap();
        assert_eq!(manager.cached(), 1, "second open reuses the cached pool");
        for pool in [&first, &second] {
            let name: String = sqlx::query_scalar("SELECT name FROM sqlite_master LIMIT 1")
                .fetch_one(pool)
                .await
                .unwrap();
            assert_eq!(name, "t", "both handles talk to the same database");
        }

        manager.evict(&connection.id);
        assert_eq!(manager.cached(), 0);
    }

    /// The guard that does not depend on parsing anything: a connection opened
    /// `SQLITE_OPEN_READONLY` refuses writes even when the statement asks
    /// nicely, and `PRAGMA query_only(0)` cannot talk it out of that.
    #[tokio::test]
    async fn the_read_pool_refuses_writes_outright() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.sqlite");
        make_db(&path).await;
        let connection = connection(&path);
        let manager = DbManager::new();

        let read = manager.read_pool_for(&connection).await.unwrap();
        assert!(
            sqlx::query("SELECT COUNT(*) FROM t")
                .fetch_one(&read)
                .await
                .is_ok(),
            "reads still work"
        );

        let _ = sqlx::query("PRAGMA query_only(0)").execute(&read).await;
        assert!(
            sqlx::query("INSERT INTO t (a) VALUES (1)")
                .execute(&read)
                .await
                .is_err(),
            "a read-only handle must refuse the write regardless of pragmas"
        );

        // The read-write pool for the same file is unaffected.
        let write = manager.pool_for(&connection).await.unwrap();
        sqlx::query("INSERT INTO t (a) VALUES (1)")
            .execute(&write)
            .await
            .expect("the write pool still writes");
    }

    #[tokio::test]
    async fn refuses_to_create_a_missing_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absent.sqlite");
        let manager = DbManager::new();

        assert!(matches!(
            manager.pool_for(&connection(&path)).await,
            Err(DbError::Db(_))
        ));
        assert!(
            !path.exists(),
            "opening must not create the file it failed to find"
        );
    }
}
