//! Saved requests + automatic history. The module owns its `api_*` tables
//! and creates them idempotently at boot.

use sqlx::{Row, SqlitePool};

use crate::{ApiHeader, ApiHistoryEntry, ApiRequestSpec, ApiResult, SavedRequest};

const HISTORY_CAP: i64 = 100;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub async fn init(pool: &SqlitePool) -> ApiResult<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS api_requests (
            id         TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            collection TEXT NOT NULL,
            method     TEXT NOT NULL,
            url        TEXT NOT NULL,
            headers    TEXT NOT NULL,
            body       TEXT,
            updated_at INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS api_history (
            id          TEXT PRIMARY KEY,
            method      TEXT NOT NULL,
            url         TEXT NOT NULL,
            status      INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            sent_at     INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn save_request(
    pool: &SqlitePool,
    name: &str,
    collection: &str,
    spec: &ApiRequestSpec,
) -> ApiResult<SavedRequest> {
    let name = name.trim();
    if name.is_empty() {
        return Err(crate::ApiError::Invalid("request name is empty".into()));
    }
    let collection = if collection.trim().is_empty() {
        "Default"
    } else {
        collection.trim()
    };
    let saved = SavedRequest {
        id: new_id(),
        name: name.to_string(),
        collection: collection.to_string(),
        method: spec.method.to_uppercase(),
        url: spec.url.clone(),
        headers: spec.headers.clone(),
        body: spec.body.clone(),
        updated_at: now_ms(),
    };
    let headers_json = serde_json::to_string(&saved.headers)
        .map_err(|e| crate::ApiError::Invalid(e.to_string()))?;
    sqlx::query(
        "INSERT INTO api_requests (id, name, collection, method, url, headers, body, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(&saved.id)
    .bind(&saved.name)
    .bind(&saved.collection)
    .bind(&saved.method)
    .bind(&saved.url)
    .bind(&headers_json)
    .bind(&saved.body)
    .bind(saved.updated_at)
    .execute(pool)
    .await?;
    Ok(saved)
}

pub async fn list_requests(pool: &SqlitePool) -> ApiResult<Vec<SavedRequest>> {
    let rows = sqlx::query(
        "SELECT id, name, collection, method, url, headers, body, updated_at
         FROM api_requests ORDER BY collection, name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|row| SavedRequest {
            id: row.get("id"),
            name: row.get("name"),
            collection: row.get("collection"),
            method: row.get("method"),
            url: row.get("url"),
            headers: serde_json::from_str::<Vec<ApiHeader>>(row.get("headers")).unwrap_or_default(),
            body: row.get("body"),
            updated_at: row.get("updated_at"),
        })
        .collect())
}

pub async fn delete_request(pool: &SqlitePool, id: &str) -> ApiResult<()> {
    let result = sqlx::query("DELETE FROM api_requests WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(crate::ApiError::Invalid(format!("request not found: {id}")));
    }
    Ok(())
}

/// Record a sent request; prunes history beyond the cap.
pub async fn record_history(
    pool: &SqlitePool,
    method: &str,
    url: &str,
    status: u16,
    duration_ms: i64,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO api_history (id, method, url, status, duration_ms, sent_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(new_id())
    .bind(method.to_uppercase())
    .bind(url)
    .bind(status as i64)
    .bind(duration_ms)
    .bind(now_ms())
    .execute(pool)
    .await?;
    sqlx::query(
        "DELETE FROM api_history WHERE id NOT IN (
            SELECT id FROM api_history ORDER BY sent_at DESC, id DESC LIMIT ?1
        )",
    )
    .bind(HISTORY_CAP)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_history(pool: &SqlitePool, limit: i64) -> ApiResult<Vec<ApiHistoryEntry>> {
    let rows = sqlx::query(
        "SELECT id, method, url, status, duration_ms, sent_at
         FROM api_history ORDER BY sent_at DESC, id DESC LIMIT ?1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|row| ApiHistoryEntry {
            id: row.get("id"),
            method: row.get("method"),
            url: row.get("url"),
            status: row.get::<i64, _>("status") as u16,
            duration_ms: row.get("duration_ms"),
            sent_at: row.get("sent_at"),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    async fn test_pool() -> (tempfile::TempDir, SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("api.db"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        init(&pool).await.unwrap();
        (dir, pool)
    }

    fn spec(url: &str) -> ApiRequestSpec {
        ApiRequestSpec {
            method: "get".into(),
            url: url.into(),
            headers: vec![ApiHeader {
                name: "accept".into(),
                value: "application/json".into(),
            }],
            body: None,
        }
    }

    #[tokio::test]
    async fn save_list_delete_roundtrip() {
        let (_dir, pool) = test_pool().await;
        let saved = save_request(&pool, "List users", "  ", &spec("http://x/users"))
            .await
            .unwrap();
        assert_eq!(saved.collection, "Default", "blank collection defaults");
        assert_eq!(saved.method, "GET", "method normalized");

        save_request(&pool, "Create user", "Users API", &spec("http://x/users"))
            .await
            .unwrap();

        let listed = list_requests(&pool).await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].headers.len(), 1, "headers survive JSON roundtrip");

        assert!(save_request(&pool, "  ", "c", &spec("http://x"))
            .await
            .is_err());

        delete_request(&pool, &saved.id).await.unwrap();
        assert!(delete_request(&pool, &saved.id).await.is_err());
        assert_eq!(list_requests(&pool).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn history_records_and_prunes() {
        let (_dir, pool) = test_pool().await;
        for i in 0..110 {
            record_history(&pool, "get", &format!("http://x/{i}"), 200, 5)
                .await
                .unwrap();
        }
        let history = list_history(&pool, 200).await.unwrap();
        assert_eq!(history.len(), 100, "history capped");
        assert_eq!(history[0].url, "http://x/109", "newest first");
        assert_eq!(history[0].method, "GET");
    }
}
