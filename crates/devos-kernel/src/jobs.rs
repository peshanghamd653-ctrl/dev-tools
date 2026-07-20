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
