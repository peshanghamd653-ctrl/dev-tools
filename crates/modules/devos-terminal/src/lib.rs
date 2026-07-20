//! Terminal module: native pty sessions streamed to the frontend.
//!
//! Sessions live in Rust, not in the webview — switching routes in the UI
//! never kills a shell. Output flows through a tokio channel the desktop
//! layer bridges onto a Tauri IPC channel.

mod integration;
mod manager;
mod types;

pub use integration::CommandFailure;
pub use manager::{CreateSessionOptions, SessionHandle, TerminalManager};
pub use types::{TermEvent, TermSessionInfo};

use devos_kernel::module::{Module, ModuleCtx};
use devos_kernel::types::CommandDescriptor;

pub struct TerminalModule;

impl Module for TerminalModule {
    fn id(&self) -> &'static str {
        "terminal"
    }

    fn register(&self, ctx: &ModuleCtx<'_>) {
        ctx.commands.register(vec![CommandDescriptor {
            id: "terminal.new".into(),
            module: "terminal".into(),
            title: "New Terminal".into(),
            keywords: vec!["shell".into(), "console".into(), "pty".into()],
            shortcut: Some("Ctrl+3".into()),
        }]);
    }
}
