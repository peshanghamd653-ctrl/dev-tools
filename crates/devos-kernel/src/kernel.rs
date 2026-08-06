use std::path::Path;
use std::time::Instant;

use sqlx::SqlitePool;

use crate::commands::CommandRegistry;
use crate::db;
use crate::error::KernelResult;
use crate::events::EventBus;
use crate::jobs::JobRunner;
use crate::module::{Module, ModuleCtx};
use crate::timing::BootTimings;

/// The DevOS runtime. One instance per app process; shared behind an `Arc`.
pub struct Kernel {
    pub pool: SqlitePool,
    pub events: EventBus,
    pub commands: CommandRegistry,
    pub jobs: JobRunner,
    module_ids: Vec<&'static str>,
    timings: BootTimings,
}

impl Kernel {
    pub async fn boot(db_path: &Path) -> KernelResult<Self> {
        let boot_started = Instant::now();
        let mut timings = BootTimings::default();

        let phase = Instant::now();
        let pool = db::connect(db_path).await?;
        timings.pool_open = phase.elapsed();

        // Snapshot before migrations touch anything, so a bad one is
        // recoverable. Best effort — a failed backup must never stop the app
        // from starting. See [`crate::backup`].
        crate::backup::run_pre_migration_backup(&pool, db_path, &db::migrator()).await;

        let phase = Instant::now();
        db::run_migrations(&pool).await?;
        timings.migrations = phase.elapsed();

        // At most one rotating copy per calendar day; also best effort.
        crate::backup::run_daily_backup(&pool, db_path).await;

        let events = EventBus::default();
        let jobs = JobRunner::new(pool.clone(), events.clone());
        let mut kernel = Self {
            pool,
            events,
            commands: CommandRegistry::new(),
            jobs,
            module_ids: Vec::new(),
            timings,
        };

        let phase = Instant::now();
        crate::repo::ensure_default_workspace(&kernel.pool).await?;
        kernel.timings.default_workspace = phase.elapsed();

        kernel.timings.boot = boot_started.elapsed();
        tracing::info!(
            boot_ms = kernel.timings.boot.as_millis() as u64,
            pool_open_us = kernel.timings.pool_open.as_micros() as u64,
            migrations_us = kernel.timings.migrations.as_micros() as u64,
            default_workspace_us = kernel.timings.default_workspace.as_micros() as u64,
            "kernel boot phases"
        );
        Ok(kernel)
    }

    /// Register a module's contributions. Call before sharing the kernel.
    pub fn register_module(&mut self, module: &dyn Module) {
        let started = Instant::now();
        module.register(&ModuleCtx {
            commands: &self.commands,
            events: &self.events,
        });
        self.module_ids.push(module.id());
        let elapsed = started.elapsed();
        self.timings.record_module(elapsed);
        tracing::info!(
            module = module.id(),
            elapsed_us = elapsed.as_micros() as u64,
            "module registered"
        );
    }

    pub fn module_ids(&self) -> &[&'static str] {
        &self.module_ids
    }

    /// Per-phase boot durations — see [`BootTimings`]. Cheap to copy; the
    /// module-registration fields keep growing as modules register.
    pub fn boot_timings(&self) -> BootTimings {
        self.timings
    }

    /// Persist a notification and broadcast it — the standard way modules,
    /// jobs, and (later) agents report to the user.
    pub async fn notify(
        &self,
        module: &str,
        level: &str,
        title: &str,
        body: Option<&str>,
    ) -> KernelResult<crate::types::NotificationDto> {
        let notification =
            crate::repo::add_notification(&self.pool, module, level, title, body).await?;
        self.events
            .emit(crate::types::KernelEvent::NotificationAdded {
                notification: notification.clone(),
            });
        Ok(notification)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo;
    use crate::types::KernelEvent;

    /// Generous — this bounds a hang, it does not measure anything, so it only
    /// has to be longer than any legitimate wait on a loaded machine.
    const EVENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

    /// Await one kernel event, failing loudly instead of hanging.
    ///
    /// A bare `rx.recv().await` in a test is a trap: cargo applies no per-test
    /// timeout, so any regression that stops an event being emitted turns into
    /// a CI run stuck at 0% CPU forever rather than a red test. Every wait on
    /// the bus in this module goes through here.
    ///
    /// Note this does not rescue a *subscribe-after-emit* mistake — the bus is
    /// a `tokio::sync::broadcast`, so an event sent before `subscribe()` is
    /// gone for good. Subscribe before triggering the thing you want to see.
    async fn next_event(rx: &mut tokio::sync::broadcast::Receiver<KernelEvent>) -> KernelEvent {
        tokio::time::timeout(EVENT_TIMEOUT, rx.recv())
            .await
            .expect("timed out waiting for a kernel event")
            .expect("event bus closed while waiting")
    }

    async fn test_kernel() -> (tempfile::TempDir, Kernel) {
        let dir = tempfile::tempdir().expect("tempdir");
        let kernel = Kernel::boot(&dir.path().join("test.db"))
            .await
            .expect("kernel boot");
        (dir, kernel)
    }

    #[tokio::test]
    async fn boot_creates_default_workspace() {
        let (_dir, kernel) = test_kernel().await;
        let workspaces = repo::list_workspaces(&kernel.pool).await.unwrap();
        assert_eq!(workspaces.len(), 1);
        assert_eq!(workspaces[0].name, "Personal");
    }

    #[tokio::test]
    async fn workspace_crud_roundtrip() {
        let (_dir, kernel) = test_kernel().await;
        let ws = repo::create_workspace(&kernel.pool, "Client Work")
            .await
            .unwrap();
        let renamed = repo::rename_workspace(&kernel.pool, &ws.id, "Client A")
            .await
            .unwrap();
        assert_eq!(renamed.name, "Client A");
        repo::delete_workspace(&kernel.pool, &ws.id).await.unwrap();
        assert_eq!(repo::list_workspaces(&kernel.pool).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn deleting_last_workspace_is_refused() {
        let (_dir, kernel) = test_kernel().await;
        let only = &repo::list_workspaces(&kernel.pool).await.unwrap()[0];
        assert!(repo::delete_workspace(&kernel.pool, &only.id)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn project_crud_and_duplicate_guard() {
        let (_dir, kernel) = test_kernel().await;
        let ws = &repo::list_workspaces(&kernel.pool).await.unwrap()[0];
        let project = repo::add_project(&kernel.pool, &ws.id, "devos", "C:/code/devos")
            .await
            .unwrap();
        assert!(
            repo::add_project(&kernel.pool, &ws.id, "again", "C:/code/devos")
                .await
                .is_err(),
            "duplicate path in same workspace must be rejected"
        );
        repo::remove_project(&kernel.pool, &project.id)
            .await
            .unwrap();
        assert!(repo::list_projects(&kernel.pool, &ws.id)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn settings_upsert_roundtrip() {
        let (_dir, kernel) = test_kernel().await;
        assert_eq!(
            repo::get_setting(&kernel.pool, "theme").await.unwrap(),
            None
        );
        repo::set_setting(&kernel.pool, "theme", "dark")
            .await
            .unwrap();
        repo::set_setting(&kernel.pool, "theme", "darker")
            .await
            .unwrap();
        assert_eq!(
            repo::get_setting(&kernel.pool, "theme").await.unwrap(),
            Some("darker".into())
        );
    }

    #[tokio::test]
    async fn event_bus_delivers_to_subscribers() {
        let (_dir, kernel) = test_kernel().await;
        let mut rx = kernel.events.subscribe();
        kernel.events.emit(KernelEvent::WorkspacesChanged);
        let event = next_event(&mut rx).await;
        assert!(matches!(event, KernelEvent::WorkspacesChanged));
    }

    #[tokio::test]
    async fn notifications_roundtrip_and_unread_tracking() {
        let (_dir, kernel) = test_kernel().await;
        let mut rx = kernel.events.subscribe();

        let first = kernel
            .notify("git", "info", "Pushed", Some("2 commits to origin/main"))
            .await
            .unwrap();
        kernel
            .notify("index", "error", "Reindex failed", None)
            .await
            .unwrap();

        // notify() broadcasts the full DTO.
        match next_event(&mut rx).await {
            KernelEvent::NotificationAdded { notification } => {
                assert_eq!(notification.id, first.id);
                assert_eq!(notification.title, "Pushed");
            }
            other => panic!("expected NotificationAdded, got {other:?}"),
        }

        assert_eq!(
            repo::unread_notification_count(&kernel.pool).await.unwrap(),
            2
        );
        let listed = repo::list_notifications(&kernel.pool, 10).await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].title, "Reindex failed", "newest first");

        repo::mark_notification_read(&kernel.pool, &first.id)
            .await
            .unwrap();
        assert_eq!(
            repo::unread_notification_count(&kernel.pool).await.unwrap(),
            1
        );
        repo::mark_all_notifications_read(&kernel.pool)
            .await
            .unwrap();
        assert_eq!(
            repo::unread_notification_count(&kernel.pool).await.unwrap(),
            0
        );
        assert!(repo::mark_notification_read(&kernel.pool, "nope")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn failed_jobs_create_notifications() {
        let (_dir, kernel) = test_kernel().await;
        let mut rx = kernel.events.subscribe();
        kernel
            .jobs
            .submit("index", "reindex", async {
                Err("disk on fire".to_string())
            })
            .await
            .unwrap();
        // Wait until the failure notification arrives.
        loop {
            if let KernelEvent::NotificationAdded { notification } = next_event(&mut rx).await {
                assert_eq!(notification.module, "index");
                assert_eq!(notification.level, "error");
                assert_eq!(notification.title, "reindex failed");
                assert_eq!(notification.body.as_deref(), Some("disk on fire"));
                break;
            }
        }
        assert_eq!(
            repo::unread_notification_count(&kernel.pool).await.unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn jobs_persist_success_and_failure() {
        let (_dir, kernel) = test_kernel().await;
        // Subscribe *before* submitting. `submit` spawns the work immediately
        // and the bus is a tokio broadcast channel, so a terminal `JobUpdated`
        // emitted before this call is gone for good — and `recv()` would then
        // wait forever, hanging the entire test binary, because cargo applies
        // no per-test timeout.
        let mut rx = kernel.events.subscribe();
        let ok = kernel
            .jobs
            .submit("test", "ok", async { Ok(serde_json::json!({"n": 1})) })
            .await
            .unwrap();
        let bad = kernel
            .jobs
            .submit("test", "bad", async { Err("boom".to_string()) })
            .await
            .unwrap();
        let mut finished = 0;
        while finished < 2 {
            if let KernelEvent::JobUpdated { job } = next_event(&mut rx).await {
                if job.finished_at.is_some() {
                    finished += 1;
                }
            }
        }
        let jobs = kernel.jobs.list_recent(10).await.unwrap();
        let ok_job = jobs.iter().find(|j| j.id == ok).unwrap();
        let bad_job = jobs.iter().find(|j| j.id == bad).unwrap();
        assert_eq!(ok_job.status, crate::types::JobStatus::Succeeded);
        assert_eq!(bad_job.status, crate::types::JobStatus::Failed);
        assert_eq!(bad_job.error.as_deref(), Some("boom"));
    }
}
