//! DevOS kernel: the runtime every module plugs into.
//!
//! The kernel owns cross-cutting infrastructure — persistence, the event bus,
//! the command registry, and background jobs. Modules contribute behavior
//! through the [`module::Module`] trait and never depend on each other
//! directly; they communicate via [`events::EventBus`].

pub mod audit;
pub mod backup;
pub mod commands;
pub mod db;
pub mod error;
pub mod events;
pub mod jobs;
pub mod kernel;
pub mod module;
pub mod repo;
pub mod timing;
pub mod types;

pub use error::{KernelError, KernelResult};
pub use kernel::Kernel;
pub use timing::BootTimings;
