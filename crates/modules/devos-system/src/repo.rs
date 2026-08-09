//! Persisted history for the performance profiler.
//!
//! A snapshot is a live read; history is a trail of samples the scheduler
//! records on a clock, so the dashboard can chart "the last few hours" rather
//! than only "right now." One row per sample: timestamp, CPU percent, and the
//! two memory numbers a usage ratio needs. Disks and processes are left out
//! — they are large, change slowly, and a snapshot already shows the current
//! ones; the chart is for the two series that actually move minute to
//! minute.

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use ts_rs::TS;

use crate::SystemResult;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct SystemHistoryPoint {
    #[ts(type = "number")]
    pub ts: i64,
    pub cpu_usage: f32,
    #[ts(type = "number")]
    pub mem_used: i64,
    #[ts(type = "number")]
    pub mem_total: i64,
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub async fn init(pool: &SqlitePool) -> SystemResult<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS system_history (
            id        TEXT PRIMARY KEY,
            ts        INTEGER NOT NULL,
            cpu_usage REAL NOT NULL,
            mem_used  INTEGER NOT NULL,
            mem_total INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS system_history_ts ON system_history(ts)")
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn record_sample(
    pool: &SqlitePool,
    ts: i64,
    cpu_usage: f32,
    mem_used: i64,
    mem_total: i64,
) -> SystemResult<()> {
    sqlx::query(
        "INSERT INTO system_history (id, ts, cpu_usage, mem_used, mem_total)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(new_id())
    .bind(ts)
    .bind(cpu_usage)
    .bind(mem_used)
    .bind(mem_total)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn prune_before(pool: &SqlitePool, cutoff_ts: i64) -> SystemResult<()> {
    sqlx::query("DELETE FROM system_history WHERE ts < ?1")
        .bind(cutoff_ts)
        .execute(pool)
        .await?;
    Ok(())
}

/// Every sample at or after `since_ts`, oldest first — the order a chart
/// wants to draw in.
pub async fn list_history(
    pool: &SqlitePool,
    since_ts: i64,
) -> SystemResult<Vec<SystemHistoryPoint>> {
    let rows = sqlx::query(
        "SELECT ts, cpu_usage, mem_used, mem_total
         FROM system_history WHERE ts >= ?1 ORDER BY ts ASC",
    )
    .bind(since_ts)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|row| SystemHistoryPoint {
            ts: row.get("ts"),
            cpu_usage: row.get("cpu_usage"),
            mem_used: row.get("mem_used"),
            mem_total: row.get("mem_total"),
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
            .filename(dir.path().join("system.db"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        init(&pool).await.unwrap();
        (dir, pool)
    }

    #[tokio::test]
    async fn recorded_samples_come_back_oldest_first() {
        let (_dir, pool) = test_pool().await;
        record_sample(&pool, 300, 10.0, 100, 1000).await.unwrap();
        record_sample(&pool, 100, 5.0, 90, 1000).await.unwrap();
        record_sample(&pool, 200, 7.5, 95, 1000).await.unwrap();

        let history = list_history(&pool, 0).await.unwrap();
        assert_eq!(
            history.iter().map(|p| p.ts).collect::<Vec<_>>(),
            vec![100, 200, 300]
        );
    }

    #[tokio::test]
    async fn since_ts_excludes_older_samples() {
        let (_dir, pool) = test_pool().await;
        record_sample(&pool, 100, 5.0, 90, 1000).await.unwrap();
        record_sample(&pool, 200, 7.5, 95, 1000).await.unwrap();

        let history = list_history(&pool, 150).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].ts, 200);
    }

    #[tokio::test]
    async fn pruning_drops_only_what_is_older_than_the_cutoff() {
        let (_dir, pool) = test_pool().await;
        record_sample(&pool, 100, 5.0, 90, 1000).await.unwrap();
        record_sample(&pool, 200, 7.5, 95, 1000).await.unwrap();
        record_sample(&pool, 300, 9.0, 99, 1000).await.unwrap();

        prune_before(&pool, 200).await.unwrap();

        let history = list_history(&pool, 0).await.unwrap();
        assert_eq!(
            history.iter().map(|p| p.ts).collect::<Vec<_>>(),
            vec![200, 300]
        );
    }
}
