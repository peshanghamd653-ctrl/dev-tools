//! Saved connections. The module owns its `db_connections` table and creates
//! it idempotently at boot, same as every other module.

use sqlx::{Row, SqlitePool};

use crate::{DbConnection, DbError, DbResult};

const DRIVER_SQLITE: &str = "sqlite";

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Resolve a user-supplied path to the absolute form we store.
///
/// On Windows `canonicalize` hands back a `\\?\` verbatim path. That prefix is
/// legal for the OS but leaks into every place the path is displayed, so strip
/// it for ordinary drive paths (UNC paths keep it — `\\?\UNC\host\share` is
/// not a valid path with the prefix removed).
fn canonical_path(path: &str) -> DbResult<String> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|_| DbError::Invalid(format!("database file not found: {path}")))?;
    if !canonical.is_file() {
        return Err(DbError::Invalid(format!("not a file: {path}")));
    }
    let text = canonical.to_string_lossy().into_owned();
    let stripped = text
        .strip_prefix(r"\\?\")
        .filter(|rest| !rest.starts_with("UNC\\"))
        .map(str::to_string);
    Ok(stripped.unwrap_or(text))
}

pub async fn init(pool: &SqlitePool) -> DbResult<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS db_connections (
            id         TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            driver     TEXT NOT NULL,
            path       TEXT NOT NULL,
            created_at INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn save_connection(pool: &SqlitePool, name: &str, path: &str) -> DbResult<DbConnection> {
    let name = name.trim();
    if name.is_empty() {
        return Err(DbError::Invalid("connection name is empty".into()));
    }
    let connection = DbConnection {
        id: new_id(),
        name: name.to_string(),
        driver: DRIVER_SQLITE.to_string(),
        path: canonical_path(path)?,
        created_at: now_ms(),
    };
    sqlx::query(
        "INSERT INTO db_connections (id, name, driver, path, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(&connection.id)
    .bind(&connection.name)
    .bind(&connection.driver)
    .bind(&connection.path)
    .bind(connection.created_at)
    .execute(pool)
    .await?;
    Ok(connection)
}

pub async fn list_connections(pool: &SqlitePool) -> DbResult<Vec<DbConnection>> {
    let rows = sqlx::query(
        "SELECT id, name, driver, path, created_at
         FROM db_connections ORDER BY name, created_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(map_connection).collect())
}

pub async fn get_connection(pool: &SqlitePool, id: &str) -> DbResult<DbConnection> {
    let row = sqlx::query(
        "SELECT id, name, driver, path, created_at
         FROM db_connections WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| DbError::NotFound(format!("connection not found: {id}")))?;
    Ok(map_connection(&row))
}

pub async fn delete_connection(pool: &SqlitePool, id: &str) -> DbResult<()> {
    let result = sqlx::query("DELETE FROM db_connections WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(DbError::NotFound(format!("connection not found: {id}")));
    }
    Ok(())
}

fn map_connection(row: &sqlx::sqlite::SqliteRow) -> DbConnection {
    DbConnection {
        id: row.get("id"),
        name: row.get("name"),
        driver: row.get("driver"),
        path: row.get("path"),
        created_at: row.get("created_at"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    async fn test_pool(dir: &tempfile::TempDir) -> SqlitePool {
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("devos.db"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        init(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn save_list_delete_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let pool = test_pool(&dir).await;
        let target = dir.path().join("app.sqlite");
        std::fs::write(&target, b"").unwrap();

        let saved = save_connection(&pool, "  App DB  ", target.to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(saved.name, "App DB", "name trimmed");
        assert_eq!(saved.driver, "sqlite");
        assert!(
            std::path::Path::new(&saved.path).is_absolute(),
            "path stored canonicalized: {}",
            saved.path
        );
        assert!(!saved.path.starts_with(r"\\?\"), "verbatim prefix stripped");

        let listed = list_connections(&pool).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, saved.id);
        assert_eq!(
            get_connection(&pool, &saved.id).await.unwrap().name,
            "App DB"
        );

        delete_connection(&pool, &saved.id).await.unwrap();
        assert!(matches!(
            delete_connection(&pool, &saved.id).await,
            Err(DbError::NotFound(_))
        ));
        assert!(matches!(
            get_connection(&pool, &saved.id).await,
            Err(DbError::NotFound(_))
        ));
        assert!(list_connections(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn rejects_blank_name_and_missing_path() {
        let dir = tempfile::tempdir().unwrap();
        let pool = test_pool(&dir).await;
        let target = dir.path().join("app.sqlite");
        std::fs::write(&target, b"").unwrap();

        assert!(matches!(
            save_connection(&pool, "   ", target.to_str().unwrap()).await,
            Err(DbError::Invalid(_))
        ));

        let missing = dir.path().join("nope.sqlite");
        assert!(matches!(
            save_connection(&pool, "Ghost", missing.to_str().unwrap()).await,
            Err(DbError::Invalid(_))
        ));

        // A directory exists but is not a database file.
        assert!(matches!(
            save_connection(&pool, "Dir", dir.path().to_str().unwrap()).await,
            Err(DbError::Invalid(_))
        ));

        assert!(list_connections(&pool).await.unwrap().is_empty());
    }
}
