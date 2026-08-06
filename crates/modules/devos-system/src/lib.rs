//! System metrics module: a point-in-time view of CPU, memory, swap, disks,
//! and the busiest processes.
//!
//! Unlike every other module this one registers no command. System metrics
//! are a widget other pages embed, not a destination of their own, so there
//! is nothing for the palette to open.
//!
//! Nothing here is fallible: reading the machine's own counters either works
//! or reports zeroes, so there is no `SystemError` to define.

mod probe;

pub use probe::SystemProbe;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use devos_kernel::module::{Module, ModuleCtx};

// One mounted volume. Sizes are bytes.
//
// Plain `//` throughout these DTOs, not `///`: ts-rs turns doc comments into
// TSDoc in the generated bindings, and the frontend is built against files
// that carry none.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub name: String,
    pub mount: String,
    #[ts(type = "number")]
    pub total: i64,
    #[ts(type = "number")]
    pub available: i64,
}

// One process from the busiest-first list.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    #[ts(type = "number")]
    pub pid: i64,
    pub name: String,
    // Percent of one CPU, so a busy multi-threaded process can exceed 100.
    pub cpu_usage: f32,
    // Resident memory in bytes.
    #[ts(type = "number")]
    pub memory: i64,
}

// Everything a metrics widget needs from one round trip.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct SystemSnapshot {
    // Machine-wide percentage, already averaged across cores.
    pub cpu_usage: f32,
    pub cpu_cores: u32,
    #[ts(type = "number")]
    pub mem_total: i64,
    #[ts(type = "number")]
    pub mem_used: i64,
    #[ts(type = "number")]
    pub swap_total: i64,
    #[ts(type = "number")]
    pub swap_used: i64,
    #[ts(type = "number")]
    pub uptime_secs: i64,
    pub disks: Vec<DiskInfo>,
    pub top_processes: Vec<ProcessInfo>,
}

pub struct SystemModule;

impl Module for SystemModule {
    fn id(&self) -> &'static str {
        "system"
    }

    fn register(&self, _ctx: &ModuleCtx<'_>) {}
}
