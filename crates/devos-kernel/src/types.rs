//! IPC data transfer objects. Every type here is exported to TypeScript via
//! ts-rs (`cargo test -p devos-kernel export_bindings`) so the frontend never
//! hand-writes payload shapes. Timestamps are Unix epoch milliseconds.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub path: String,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

/// A palette-invokable command contributed by a module (or later, a plugin).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CommandDescriptor {
    pub id: String,
    pub module: String,
    pub title: String,
    pub keywords: Vec<String>,
    pub shortcut: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Pending => "pending",
            JobStatus::Running => "running",
            JobStatus::Succeeded => "succeeded",
            JobStatus::Failed => "failed",
        }
    }

    pub fn from_db(value: &str) -> JobStatus {
        match value {
            "pending" => JobStatus::Pending,
            "running" => JobStatus::Running,
            "succeeded" => JobStatus::Succeeded,
            _ => JobStatus::Failed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct JobInfo {
    pub id: String,
    pub module: String,
    pub kind: String,
    pub status: JobStatus,
    pub error: Option<String>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number | null")]
    pub started_at: Option<i64>,
    #[ts(type = "number | null")]
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    #[ts(type = "number")]
    pub startup_ms: i64,
    pub db_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct NotificationDto {
    pub id: String,
    pub module: String,
    /// "info" | "warning" | "error"
    pub level: String,
    pub title: String,
    pub body: Option<String>,
    pub read: bool,
    #[ts(type = "number")]
    pub created_at: i64,
}

/// One entry of a project directory listing (file explorer).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct FsEntry {
    pub name: String,
    pub is_dir: bool,
    #[ts(type = "number")]
    pub size: i64,
}

/// Text preview of a project file (file explorer).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct FilePreview {
    pub content: String,
    pub truncated: bool,
    pub binary: bool,
    #[ts(type = "number")]
    pub size: i64,
}

/// Events broadcast on the kernel event bus and forwarded to the frontend
/// on the `devos://event` channel.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/ipc/bindings/")]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum KernelEvent {
    WorkspacesChanged,
    ProjectsChanged { workspace_id: String },
    SettingsChanged { key: String },
    JobUpdated { job: JobInfo },
    NotificationAdded { notification: NotificationDto },
}
