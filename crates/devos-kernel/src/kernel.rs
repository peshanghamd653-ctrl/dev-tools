use std::path::Path;

use sqlx::SqlitePool;

use crate::commands::CommandRegistry;
use crate::db;
use crate::error::KernelResult;
use crate::events::EventBus;
use crate::jobs::JobRunner;
use crate::module::{Module, ModuleCtx};

/// The DevOS runtime. One instance per app process; shared behind an `Arc`.
pub struct Kernel {
    pub pool: SqlitePool,
    pub events: EventBus,
    pub commands: CommandRegistry,
    pub jobs: JobRunner,
    module_ids: Vec<&'static str>,
}

impl Kernel {
    pub async fn boot(db_path: &Path) -> KernelResult<Self> {
        let pool = db::open_pool(db_path).await?;
        let events = EventBus::default();
        let jobs = JobRunner::new(pool.clone(), events.clone());
        let kernel = Self {
            pool,
            events,
            commands: CommandRegistry::new(),
            jobs,
            module_ids: Vec::new(),
        };
        crate::repo::ensure_default_workspace(&kernel.pool).await?;
        Ok(kernel)
    }

    /// Register a module's contributions. Call before sharing the kernel.
    pub fn register_module(&mut self, module: &dyn Module) {
        module.register(&ModuleCtx {
            commands: &self.commands,
            events: &self.events,
        });
        self.module_ids.push(module.id());
        tracing::info!(module = module.id(), "module registered");
    }

    pub fn module_ids(&self) -> &[&'static str] {
        &self.module_ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo;
    use crate::types::KernelEvent;

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
        let event = rx.recv().await.unwrap();
        assert!(matches!(event, KernelEvent::WorkspacesChanged));
    }

    #[tokio::test]
    async fn jobs_persist_success_and_failure() {
        let (_dir, kernel) = test_kernel().await;
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
        // Wait for both spawned jobs to finish.
        let mut rx = kernel.events.subscribe();
        let mut finished = 0;
        while finished < 2 {
            if let Ok(KernelEvent::JobUpdated { job }) = rx.recv().await {
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
