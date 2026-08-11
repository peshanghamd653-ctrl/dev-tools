//! Durable background jobs. Every job is recorded in SQLite so state survives
//! restarts, and every transition is broadcast on the event bus.

use std::future::Future;

use sqlx::{Row, SqlitePool};

use crate::error::KernelResult;
use crate::events::EventBus;
use crate::types::{JobInfo, JobStatus, KernelEvent};

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(Clone)]
pub struct JobRunner {
    pool: SqlitePool,
    events: EventBus,
}

impl JobRunner {
    pub fn new(pool: SqlitePool, events: EventBus) -> Self {
        Self { pool, events }
    }

    /// Run `work` in the background. Returns the job id immediately.
    pub async fn submit<F>(&self, module: &str, kind: &str, work: F) -> KernelResult<String>
    where
        F: Future<Output = Result<serde_json::Value, String>> + Send + 'static,
    {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_ms();
        sqlx::query(
            "INSERT INTO jobs (id, module, kind, status, created_at, started_at)
             VALUES (?1, ?2, ?3, 'running', ?4, ?4)",
        )
        .bind(&id)
        .bind(module)
        .bind(kind)
        .bind(now)
        .execute(&self.pool)
        .await?;

        let info = JobInfo {
            id: id.clone(),
            module: module.to_string(),
            kind: kind.to_string(),
            status: JobStatus::Running,
            error: None,
            created_at: now,
            started_at: Some(now),
            finished_at: None,
        };
        self.events
            .emit(KernelEvent::JobUpdated { job: info.clone() });

        let runner = self.clone();
        tokio::spawn(async move {
            let outcome = work.await;
            let finished = now_ms();
            let (status, result, error) = match outcome {
                Ok(value) => (JobStatus::Succeeded, Some(value.to_string()), None),
                Err(message) => (JobStatus::Failed, None, Some(message)),
            };
            let update = sqlx::query(
                "UPDATE jobs SET status = ?1, result = ?2, error = ?3, finished_at = ?4 WHERE id = ?5",
            )
            .bind(status.as_str())
            .bind(&result)
            .bind(&error)
            .bind(finished)
            .bind(&info.id)
            .execute(&runner.pool)
            .await;
            if let Err(db_error) = update {
                tracing::error!(job = %info.id, %db_error, "failed to persist job result");
            }
            // Failures become persistent notifications, not just transient
            // events — this is how background work reports problems.
            if let Some(message) = &error {
                if let Ok(notification) = crate::repo::add_notification(
                    &runner.pool,
                    &info.module,
                    "error",
                    &format!("{} failed", info.kind),
                    Some(message),
                )
                .await
                {
                    runner
                        .events
                        .emit(KernelEvent::NotificationAdded { notification });
                }
            }
            runner.events.emit(KernelEvent::JobUpdated {
                job: JobInfo {
                    status,
                    error,
                    finished_at: Some(finished),
                    ..info
                },
            });
        });

        Ok(id)
    }

    /// Marks every job left `running` as `failed`, with a notification for
    /// each. Called once at boot, before anything else can submit a job — the
    /// only way a `running` row survives to a new process is a crash mid-job,
    /// since `submit`'s spawned task always reaches the completion `UPDATE`
    /// otherwise, cancellation included (a cancelled `tokio::spawn` still
    /// exits without running past its await point cleanly on graceful
    /// shutdown; a genuine crash is what skips the `UPDATE` entirely).
    /// Returns the count reconciled, for the caller to log.
    pub async fn reconcile_stale(&self) -> KernelResult<usize> {
        let rows = sqlx::query(
            "SELECT id, module, kind, created_at, started_at
             FROM jobs WHERE status = 'running'",
        )
        .fetch_all(&self.pool)
        .await?;
        if rows.is_empty() {
            return Ok(0);
        }

        let now = now_ms();
        const REASON: &str = "interrupted by a crash";
        for row in &rows {
            let id: String = row.get("id");
            let module: String = row.get("module");
            let kind: String = row.get("kind");

            sqlx::query(
                "UPDATE jobs SET status = 'failed', error = ?1, finished_at = ?2 WHERE id = ?3",
            )
            .bind(REASON)
            .bind(now)
            .bind(&id)
            .execute(&self.pool)
            .await?;

            // Same shape `submit`'s own failure path uses — a stale job is a
            // failure the user was not there to see happen live.
            if let Ok(notification) = crate::repo::add_notification(
                &self.pool,
                &module,
                "warning",
                &format!("{kind} was interrupted"),
                Some("DevOS exited unexpectedly while this job was running."),
            )
            .await
            {
                self.events
                    .emit(KernelEvent::NotificationAdded { notification });
            }

            self.events.emit(KernelEvent::JobUpdated {
                job: JobInfo {
                    id,
                    module,
                    kind,
                    status: JobStatus::Failed,
                    error: Some(REASON.to_string()),
                    created_at: row.get("created_at"),
                    started_at: row.get("started_at"),
                    finished_at: Some(now),
                },
            });
        }
        Ok(rows.len())
    }

    pub async fn list_recent(&self, limit: i64) -> KernelResult<Vec<JobInfo>> {
        let rows = sqlx::query(
            "SELECT id, module, kind, status, error, created_at, started_at, finished_at
             FROM jobs ORDER BY created_at DESC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|row| JobInfo {
                id: row.get("id"),
                module: row.get("module"),
                kind: row.get("kind"),
                status: JobStatus::from_db(row.get("status")),
                error: row.get("error"),
                created_at: row.get("created_at"),
                started_at: row.get("started_at"),
                finished_at: row.get("finished_at"),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_runner() -> (tempfile::TempDir, JobRunner) {
        let dir = tempfile::tempdir().expect("tempdir");
        let pool = crate::db::connect(&dir.path().join("test.db"))
            .await
            .expect("connect");
        crate::db::run_migrations(&pool).await.expect("migrate");
        (dir, JobRunner::new(pool, EventBus::default()))
    }

    /// Simulates the only way a `running` row survives to a new process: a
    /// row `submit`'s `INSERT` created, that never reached its completion
    /// `UPDATE` because the process died first. Inserted directly rather than
    /// through `submit`, which always finishes its (in-test, instant) future
    /// and updates the row before the test could observe it mid-flight.
    #[tokio::test]
    async fn reconcile_stale_fails_a_row_left_running() {
        let (_dir, runner) = test_runner().await;
        sqlx::query(
            "INSERT INTO jobs (id, module, kind, status, created_at, started_at)
             VALUES ('orphan', 'test', 'crashed-mid-run', 'running', 1, 1)",
        )
        .execute(&runner.pool)
        .await
        .unwrap();

        let reconciled = runner.reconcile_stale().await.unwrap();
        assert_eq!(reconciled, 1);

        let jobs = runner.list_recent(10).await.unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].status, JobStatus::Failed);
        assert_eq!(jobs[0].error.as_deref(), Some("interrupted by a crash"));
        assert!(jobs[0].finished_at.is_some());

        let notifications = crate::repo::list_notifications(&runner.pool, 10)
            .await
            .unwrap();
        assert_eq!(notifications.len(), 1, "a stale job must be noticed");
        assert!(notifications[0].title.contains("crashed-mid-run"));
    }

    /// The common case: nothing was interrupted, and reconciliation is a
    /// no-op that touches neither jobs nor notifications.
    #[tokio::test]
    async fn reconcile_stale_leaves_a_finished_job_alone() {
        let (_dir, runner) = test_runner().await;
        runner
            .submit("test", "ok", async { Ok(serde_json::json!({})) })
            .await
            .unwrap();
        // `submit`'s completion update runs on a spawned task; give it a
        // moment rather than racing it.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let reconciled = runner.reconcile_stale().await.unwrap();
        assert_eq!(reconciled, 0);

        let jobs = runner.list_recent(10).await.unwrap();
        assert_eq!(jobs[0].status, JobStatus::Succeeded);
        assert!(crate::repo::list_notifications(&runner.pool, 10)
            .await
            .unwrap()
            .is_empty());
    }
}
