//! Monitors and their check history. The module owns its `monitors` and
//! `monitor_checks` tables and creates them idempotently at boot, same as
//! every other module.

use sqlx::{Row, SqlitePool};

use crate::{Monitor, MonitorCheck, MonitorError, MonitorResult, MonitorStatus};

/// The fastest a monitor may poll. Without a floor, a slipped decimal point
/// in the interval box turns DevOS into a small denial-of-service tool
/// pointed at whatever host the user typed.
pub const MIN_INTERVAL_SECS: i64 = 60;

/// How many checks the UI gets per monitor.
const RECENT_LIMIT: i64 = 30;

/// The window `uptime_pct` and `avg_ms` are computed over.
const WINDOW_MS: i64 = 24 * 60 * 60 * 1000;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Validate a URL the app will later fetch on a timer.
///
/// This is a security boundary rather than a convenience check. The scheduler
/// issues a GET against whatever is stored, so a scheme that reaches outside
/// HTTP — `file://` above all, but equally `ftp://` or some app handler — has
/// to be refused before it lands in the database.
pub(crate) fn validate_url(url: &str) -> MonitorResult<String> {
    let url = url.trim();
    let parsed: reqwest::Url = url
        .parse()
        .map_err(|e| MonitorError::Invalid(format!("invalid URL: {e}")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(MonitorError::Invalid(format!(
            "unsupported URL scheme: {}",
            parsed.scheme()
        )));
    }
    Ok(url.to_string())
}

pub async fn init(pool: &SqlitePool) -> MonitorResult<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS monitors (
            id            TEXT PRIMARY KEY,
            name          TEXT NOT NULL,
            url           TEXT NOT NULL,
            interval_secs INTEGER NOT NULL,
            enabled       INTEGER NOT NULL,
            created_at    INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS monitor_checks (
            id          TEXT PRIMARY KEY,
            monitor_id  TEXT NOT NULL,
            status      INTEGER,
            ok          INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            error       TEXT,
            checked_at  INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    // Every read of this table is "one monitor, newest first". Without the
    // index that is a full scan of a table which grows by a row per monitor
    // per minute, on the path the monitors page hits on every render.
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_monitor_checks_monitor_checked_at
         ON monitor_checks (monitor_id, checked_at)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn create_monitor(
    pool: &SqlitePool,
    name: &str,
    url: &str,
    interval_secs: i64,
) -> MonitorResult<Monitor> {
    let name = name.trim();
    if name.is_empty() {
        return Err(MonitorError::Invalid("monitor name is empty".into()));
    }
    let monitor = Monitor {
        id: new_id(),
        name: name.to_string(),
        url: validate_url(url)?,
        interval_secs: interval_secs.max(MIN_INTERVAL_SECS),
        enabled: true,
        created_at: now_ms(),
    };
    sqlx::query(
        "INSERT INTO monitors (id, name, url, interval_secs, enabled, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(&monitor.id)
    .bind(&monitor.name)
    .bind(&monitor.url)
    .bind(monitor.interval_secs)
    .bind(monitor.enabled)
    .bind(monitor.created_at)
    .execute(pool)
    .await?;
    Ok(monitor)
}

pub async fn list_monitors(pool: &SqlitePool) -> MonitorResult<Vec<Monitor>> {
    let rows = sqlx::query(
        "SELECT id, name, url, interval_secs, enabled, created_at
         FROM monitors ORDER BY name, created_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(map_monitor).collect())
}

pub async fn get_monitor(pool: &SqlitePool, id: &str) -> MonitorResult<Monitor> {
    let row = sqlx::query(
        "SELECT id, name, url, interval_secs, enabled, created_at
         FROM monitors WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| MonitorError::NotFound(format!("monitor not found: {id}")))?;
    Ok(map_monitor(&row))
}

pub async fn delete_monitor(pool: &SqlitePool, id: &str) -> MonitorResult<()> {
    let result = sqlx::query("DELETE FROM monitors WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(MonitorError::NotFound(format!("monitor not found: {id}")));
    }
    // History is cleared explicitly rather than by `ON DELETE CASCADE`:
    // SQLite only enforces foreign keys when `PRAGMA foreign_keys` is on, and
    // that is a per-connection setting no single module should be flipping on
    // the pool every other module shares.
    sqlx::query("DELETE FROM monitor_checks WHERE monitor_id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_enabled(pool: &SqlitePool, id: &str, enabled: bool) -> MonitorResult<Monitor> {
    let result = sqlx::query("UPDATE monitors SET enabled = ?1 WHERE id = ?2")
        .bind(enabled)
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(MonitorError::NotFound(format!("monitor not found: {id}")));
    }
    get_monitor(pool, id).await
}

pub async fn record_check(pool: &SqlitePool, check: &MonitorCheck) -> MonitorResult<()> {
    sqlx::query(
        "INSERT INTO monitor_checks (id, monitor_id, status, ok, duration_ms, error, checked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .bind(&check.id)
    .bind(&check.monitor_id)
    .bind(check.status.map(i64::from))
    .bind(check.ok)
    .bind(check.duration_ms)
    .bind(&check.error)
    .bind(check.checked_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// The newest check for a monitor, or `None` if it has never been checked.
pub async fn latest_check(
    pool: &SqlitePool,
    monitor_id: &str,
) -> MonitorResult<Option<MonitorCheck>> {
    Ok(recent_checks(pool, monitor_id, 1).await?.into_iter().next())
}

/// Drop checks recorded before `cutoff_ms`. Returns how many rows went.
pub async fn prune_checks(pool: &SqlitePool, cutoff_ms: i64) -> MonitorResult<u64> {
    let result = sqlx::query("DELETE FROM monitor_checks WHERE checked_at < ?1")
        .bind(cutoff_ms)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub async fn list_status(pool: &SqlitePool) -> MonitorResult<Vec<MonitorStatus>> {
    let monitors = list_monitors(pool).await?;
    let mut statuses = Vec::with_capacity(monitors.len());
    for monitor in monitors {
        statuses.push(status_for(pool, monitor).await?);
    }
    Ok(statuses)
}

async fn status_for(pool: &SqlitePool, monitor: Monitor) -> MonitorResult<MonitorStatus> {
    let recent = recent_checks(pool, &monitor.id, RECENT_LIMIT).await?;
    let (uptime_pct, avg_ms) = aggregate(pool, &monitor.id, now_ms() - WINDOW_MS).await?;
    let last_check = recent.first().cloned();
    Ok(MonitorStatus {
        monitor,
        last_check,
        uptime_pct,
        avg_ms,
        recent,
    })
}

async fn recent_checks(
    pool: &SqlitePool,
    monitor_id: &str,
    limit: i64,
) -> MonitorResult<Vec<MonitorCheck>> {
    let rows = sqlx::query(
        "SELECT id, monitor_id, status, ok, duration_ms, error, checked_at
         FROM monitor_checks WHERE monitor_id = ?1
         ORDER BY checked_at DESC, id DESC LIMIT ?2",
    )
    .bind(monitor_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(map_check).collect())
}

/// Uptime share and typical response time since `cutoff_ms`.
///
/// `avg_ms` averages only checks that came back with a status. A connection
/// refused in 1ms is not evidence the site is fast, and counting it would
/// leave a fully-down monitor looking like the quickest one on the page.
async fn aggregate(
    pool: &SqlitePool,
    monitor_id: &str,
    cutoff_ms: i64,
) -> MonitorResult<(f64, i64)> {
    let row = sqlx::query(
        "SELECT CAST(COUNT(*) AS INTEGER) AS total,
                CAST(COALESCE(SUM(ok), 0) AS INTEGER) AS ok_count,
                CAST(COALESCE(AVG(CASE WHEN status IS NOT NULL THEN duration_ms END), 0) AS REAL)
                    AS avg_ms
         FROM monitor_checks WHERE monitor_id = ?1 AND checked_at >= ?2",
    )
    .bind(monitor_id)
    .bind(cutoff_ms)
    .fetch_one(pool)
    .await?;

    let total: i64 = row.get("total");
    // A monitor with no checks in the window has recorded no downtime, so it
    // reads as 100% rather than as an alarming 0% — and, more to the point,
    // never divides by zero.
    let uptime_pct = if total == 0 {
        100.0
    } else {
        row.get::<i64, _>("ok_count") as f64 * 100.0 / total as f64
    };
    Ok((uptime_pct, row.get::<f64, _>("avg_ms").round() as i64))
}

fn map_monitor(row: &sqlx::sqlite::SqliteRow) -> Monitor {
    Monitor {
        id: row.get("id"),
        name: row.get("name"),
        url: row.get("url"),
        interval_secs: row.get("interval_secs"),
        enabled: row.get::<i64, _>("enabled") != 0,
        created_at: row.get("created_at"),
    }
}

fn map_check(row: &sqlx::sqlite::SqliteRow) -> MonitorCheck {
    MonitorCheck {
        id: row.get("id"),
        monitor_id: row.get("monitor_id"),
        status: row.get::<Option<i64>, _>("status").map(|s| s as u16),
        ok: row.get::<i64, _>("ok") != 0,
        duration_ms: row.get("duration_ms"),
        error: row.get("error"),
        checked_at: row.get("checked_at"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    async fn test_pool() -> (tempfile::TempDir, SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("monitor.db"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        init(&pool).await.unwrap();
        (dir, pool)
    }

    /// Seed a check with exactly the values a test wants to aggregate over.
    async fn seed(
        pool: &SqlitePool,
        monitor_id: &str,
        ok: bool,
        status: Option<u16>,
        duration_ms: i64,
        checked_at: i64,
    ) -> MonitorCheck {
        let check = MonitorCheck {
            id: new_id(),
            monitor_id: monitor_id.to_string(),
            status,
            ok,
            duration_ms,
            error: if ok { None } else { Some("boom".into()) },
            checked_at,
        };
        record_check(pool, &check).await.unwrap();
        check
    }

    #[tokio::test]
    async fn create_list_delete_roundtrip() {
        let (_dir, pool) = test_pool().await;

        let created = create_monitor(&pool, "  Docs site  ", " https://example.com/docs ", 300)
            .await
            .unwrap();
        assert_eq!(created.name, "Docs site", "name trimmed");
        assert_eq!(created.url, "https://example.com/docs", "url trimmed");
        assert_eq!(created.interval_secs, 300);
        assert!(created.enabled, "monitors start enabled");

        let listed = list_monitors(&pool).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, created.id);
        assert_eq!(
            get_monitor(&pool, &created.id).await.unwrap().name,
            "Docs site"
        );

        delete_monitor(&pool, &created.id).await.unwrap();
        assert!(matches!(
            delete_monitor(&pool, &created.id).await,
            Err(MonitorError::NotFound(_))
        ));
        assert!(matches!(
            get_monitor(&pool, &created.id).await,
            Err(MonitorError::NotFound(_))
        ));
        assert!(list_monitors(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn rejects_blank_names() {
        let (_dir, pool) = test_pool().await;
        for name in ["", "   ", "\t\n"] {
            assert!(
                matches!(
                    create_monitor(&pool, name, "https://example.com", 60).await,
                    Err(MonitorError::Invalid(_))
                ),
                "blank name {name:?} accepted"
            );
        }
        assert!(list_monitors(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn rejects_non_http_schemes_and_garbage_urls() {
        let (_dir, pool) = test_pool().await;
        // The guard that matters: the scheduler fetches whatever is stored,
        // so a `file://` URL would turn the monitor into a local file reader.
        for url in [
            "file:///etc/passwd",
            "file://C:/Windows/win.ini",
            "ftp://x",
            "javascript:alert(1)",
            "data:text/plain,hi",
            "not a url",
            "",
        ] {
            assert!(
                matches!(
                    create_monitor(&pool, "Bad", url, 60).await,
                    Err(MonitorError::Invalid(_))
                ),
                "url {url:?} was accepted"
            );
        }
        assert!(list_monitors(&pool).await.unwrap().is_empty());

        for url in ["http://example.com", "https://example.com:8443/health"] {
            assert!(
                create_monitor(&pool, "Good", url, 60).await.is_ok(),
                "url {url:?} was rejected"
            );
        }
    }

    #[tokio::test]
    async fn interval_is_clamped_to_the_floor() {
        let (_dir, pool) = test_pool().await;
        for (requested, expected) in [(1, 60), (0, 60), (-30, 60), (59, 60), (60, 60), (900, 900)] {
            let monitor = create_monitor(&pool, "Site", "https://example.com", requested)
                .await
                .unwrap();
            assert_eq!(
                monitor.interval_secs, expected,
                "requested {requested}s should store {expected}s"
            );
        }
    }

    #[tokio::test]
    async fn toggling_enabled_persists() {
        let (_dir, pool) = test_pool().await;
        let monitor = create_monitor(&pool, "Site", "https://example.com", 60)
            .await
            .unwrap();

        let disabled = set_enabled(&pool, &monitor.id, false).await.unwrap();
        assert!(!disabled.enabled);
        assert!(!get_monitor(&pool, &monitor.id).await.unwrap().enabled);

        let enabled = set_enabled(&pool, &monitor.id, true).await.unwrap();
        assert!(enabled.enabled);

        assert!(matches!(
            set_enabled(&pool, "ghost", false).await,
            Err(MonitorError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn aggregates_uptime_and_average_over_the_window() {
        let (_dir, pool) = test_pool().await;
        let monitor = create_monitor(&pool, "Site", "https://example.com", 60)
            .await
            .unwrap();
        let now = now_ms();

        // In-window: 3 of 4 succeeded, so 75%. Response times 100/200/300 for
        // the checks that answered; the transport failure has no status and
        // must not drag the average down.
        seed(&pool, &monitor.id, true, Some(200), 100, now - 1_000).await;
        seed(&pool, &monitor.id, true, Some(200), 200, now - 2_000).await;
        seed(&pool, &monitor.id, true, Some(204), 300, now - 3_000).await;
        seed(&pool, &monitor.id, false, None, 5, now - 4_000).await;
        // Out of window by an hour; must not move either number.
        seed(
            &pool,
            &monitor.id,
            false,
            Some(500),
            9_000,
            now - 25 * 3_600_000,
        )
        .await;

        let status = &list_status(&pool).await.unwrap()[0];
        assert_eq!(status.uptime_pct, 75.0);
        assert_eq!(status.avg_ms, 200, "mean of 100/200/300, failure excluded");
        assert_eq!(status.recent.len(), 5, "recent spans all history");
        assert_eq!(
            status.last_check.as_ref().unwrap().checked_at,
            now - 1_000,
            "newest check is the last check"
        );
    }

    #[tokio::test]
    async fn a_monitor_with_no_checks_does_not_divide_by_zero() {
        let (_dir, pool) = test_pool().await;
        create_monitor(&pool, "Fresh", "https://example.com", 60)
            .await
            .unwrap();

        let status = &list_status(&pool).await.unwrap()[0];
        assert_eq!(
            status.uptime_pct, 100.0,
            "no checks means no recorded downtime"
        );
        assert_eq!(status.avg_ms, 0);
        assert!(status.last_check.is_none());
        assert!(status.recent.is_empty());
    }

    #[tokio::test]
    async fn recent_is_capped_at_thirty_newest_first() {
        let (_dir, pool) = test_pool().await;
        let monitor = create_monitor(&pool, "Site", "https://example.com", 60)
            .await
            .unwrap();
        let now = now_ms();
        for i in 0..40 {
            seed(&pool, &monitor.id, true, Some(200), i, now - i * 1_000).await;
        }

        let status = &list_status(&pool).await.unwrap()[0];
        assert_eq!(status.recent.len(), 30);
        assert_eq!(status.recent[0].checked_at, now, "newest first");
        assert_eq!(status.recent[29].checked_at, now - 29 * 1_000);
    }

    #[tokio::test]
    async fn pruning_drops_old_checks_and_keeps_recent_ones() {
        let (_dir, pool) = test_pool().await;
        let monitor = create_monitor(&pool, "Site", "https://example.com", 60)
            .await
            .unwrap();
        let now = now_ms();
        let week = 7 * 24 * 3_600_000;
        let kept = seed(&pool, &monitor.id, true, Some(200), 10, now - 3_600_000).await;
        seed(&pool, &monitor.id, true, Some(200), 10, now - week - 1).await;
        seed(
            &pool,
            &monitor.id,
            false,
            None,
            10,
            now - 30 * 24 * 3_600_000,
        )
        .await;

        let removed = prune_checks(&pool, now - week).await.unwrap();
        assert_eq!(removed, 2);

        let status = &list_status(&pool).await.unwrap()[0];
        assert_eq!(status.recent.len(), 1);
        assert_eq!(status.recent[0].id, kept.id);
    }

    #[tokio::test]
    async fn deleting_a_monitor_takes_its_history_with_it() {
        let (_dir, pool) = test_pool().await;
        let doomed = create_monitor(&pool, "Doomed", "https://example.com", 60)
            .await
            .unwrap();
        let keeper = create_monitor(&pool, "Keeper", "https://example.org", 60)
            .await
            .unwrap();
        let now = now_ms();
        seed(&pool, &doomed.id, true, Some(200), 10, now).await;
        seed(&pool, &keeper.id, true, Some(200), 10, now).await;

        delete_monitor(&pool, &doomed.id).await.unwrap();

        let orphans: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM monitor_checks WHERE monitor_id = ?1")
                .bind(&doomed.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(orphans, 0, "history outlived its monitor");
        assert!(latest_check(&pool, &keeper.id).await.unwrap().is_some());
    }
}
