//! Uptime monitor module: periodic HTTP checks against user-registered URLs,
//! with per-monitor history, 24h aggregates, and down/recovered alerts.
//!
//! Two decisions shape the whole module. First, a check records only the
//! status line and how long it took — response bodies are never read, let
//! alone stored (see [`run_check`]). Second, alerts follow *transitions*, not
//! results, so a site that is down for an hour produces one notification
//! rather than one per check; that rule lives in [`alert_for`], deliberately
//! outside the async loop that calls it.

mod check;
mod repo;
mod scheduler;

pub use check::run_check;
pub use repo::{
    create_monitor, delete_monitor, get_monitor, init, latest_check, list_monitors, list_status,
    prune_checks, record_check, set_enabled, MIN_INTERVAL_SECS,
};
pub use scheduler::{alert_for, check_now, run_scheduler, MonitorAlert};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use devos_kernel::module::{Module, ModuleCtx};
use devos_kernel::types::CommandDescriptor;

#[derive(Debug, thiserror::Error)]
pub enum MonitorError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("kernel error: {0}")]
    Kernel(#[from] devos_kernel::KernelError),
}

pub type MonitorResult<T> = Result<T, MonitorError>;

// A URL the user wants watched.
//
// Plain `//` throughout these DTOs, not `///`: ts-rs turns doc comments into
// TSDoc in the generated bindings, and the frontend is built against files
// that carry none.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub id: String,
    pub name: String,
    pub url: String,
    // Clamped to `MIN_INTERVAL_SECS` on the way in.
    #[ts(type = "number")]
    pub interval_secs: i64,
    pub enabled: bool,
    #[ts(type = "number")]
    pub created_at: i64,
}

// The outcome of one check. There is no body field, and that is the point: a
// response body cannot leak into the database through a struct that has
// nowhere to put it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct MonitorCheck {
    pub id: String,
    pub monitor_id: String,
    // `None` when the request never got a response at all.
    pub status: Option<u16>,
    pub ok: bool,
    #[ts(type = "number")]
    pub duration_ms: i64,
    pub error: Option<String>,
    #[ts(type = "number")]
    pub checked_at: i64,
}

// A monitor plus the history the UI renders beside it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct MonitorStatus {
    pub monitor: Monitor,
    pub last_check: Option<MonitorCheck>,
    // Share of checks in the last 24h that succeeded.
    pub uptime_pct: f64,
    // Mean response time over the last 24h, in milliseconds.
    #[ts(type = "number")]
    pub avg_ms: i64,
    // The newest checks, newest first.
    pub recent: Vec<MonitorCheck>,
}

pub struct MonitorModule;

impl Module for MonitorModule {
    fn id(&self) -> &'static str {
        "monitor"
    }

    fn register(&self, ctx: &ModuleCtx<'_>) {
        ctx.commands.register(vec![CommandDescriptor {
            id: "monitor.open".into(),
            module: "monitor".into(),
            title: "Open Monitors".into(),
            keywords: vec![
                "uptime".into(),
                "website".into(),
                "watch".into(),
                "status".into(),
            ],
            shortcut: Some("Ctrl+0".into()),
        }]);
    }
}
