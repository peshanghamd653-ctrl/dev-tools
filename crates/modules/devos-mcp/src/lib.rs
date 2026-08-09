//! MCP client module: named external tool servers a user configures (a
//! launch command and arguments), a stdio JSON-RPC handshake against them,
//! and `tools/list` discovery.
//!
//! ## Why this is discovery-only, not invocation
//!
//! The obvious next feature is handing a discovered tool to the AI agent
//! loop, the same way `run_command` or `edit_file` are. That is deliberately
//! not what shipped here, and the reason is this codebase's own precedent
//! for exactly this kind of decision:
//!
//! - [ADR-0005](../../../docs/adr/0005-read-only-tools-first-with-explicit-grant.md)
//!   shipped read-only AI tools first and let write tools follow behind a
//!   real per-call approval gate, rather than inheriting write access by
//!   accident.
//! - [ADR-0009](../../../docs/adr/0009-deployments-read-only-no-write-actions.md)
//!   shipped Vercel deployments read-only and named two concrete
//!   prerequisites — the approval gate, and an unambiguous display of what a
//!   write action targets — before write actions could land.
//! - [ADR-0010](../../../docs/adr/0010-wasmi-interpreter-for-plugin-runtime.md)
//!   built a WASM plugin sandbox and then kept it **out of the running app**
//!   after an adversarial review found the enforced properties were
//!   partly decorative, listing what would have to be true before it ships.
//!
//! An MCP tool is a materially bigger trust step than any of those: it is
//! arbitrary code the user pointed a launch command at, running as a real
//! child process, whose self-described `inputSchema` would have to be
//! rendered in an approval card and whose output would flow straight back
//! into the model's context. None of the machinery that makes that safe —
//! approval-card rendering for an arbitrary externally-defined schema, a
//! trust tier decision (should an MCP tool ever be read-grant-only, or
//! always require approval regardless of what it claims to do), name
//! collision handling against DevOS's own tool names — exists yet. Shipping
//! "connect to a server and see what it offers" is useful on its own (does
//! the server start, does it speak the protocol, what does it claim to do)
//! and it is how this module earns the right to be trusted with more —
//! exactly ADR-0009's framing for why read-only shipped alone.
//!
//! ## Why connections are one-shot, not persistent
//!
//! [`discover_tools`] spawns the server, performs the handshake, lists its
//! tools, and terminates the process before returning. There is no
//! `McpManager` holding a live child process between IPC calls the way
//! `DbManager` holds a `SqlitePool`: with no invocation feature to justify
//! keeping a process warm, a persistent connection would only be a process
//! this app has to remember to clean up on every exit path, for a benefit
//! (skipping the handshake next time) that discovery — an occasional,
//! user-initiated action — does not need.

mod client;
mod process;
mod repo;

pub use process::discover_tools;
pub use repo::{create_server, delete_server, init, list_servers, McpServer};

use devos_kernel::module::{Module, ModuleCtx};
use devos_kernel::types::CommandDescriptor;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("could not start the server process: {0}")]
    Spawn(String),
    #[error("the server did not respond in time")]
    Timeout,
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("the server reported an error: {0}")]
    Rpc(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type McpResult<T> = Result<T, McpError>;

/// One tool a server advertised via `tools/list`. `input_schema` is passed
/// through verbatim — it is the server's own JSON Schema, not something this
/// module interprets.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub name: String,
    pub description: Option<String>,
    #[ts(type = "unknown")]
    pub input_schema: serde_json::Value,
}

pub struct McpModule;

impl Module for McpModule {
    fn id(&self) -> &'static str {
        "mcp"
    }

    fn register(&self, ctx: &ModuleCtx<'_>) {
        ctx.commands.register(vec![CommandDescriptor {
            id: "mcp.open".into(),
            module: "mcp".into(),
            title: "MCP Servers".into(),
            keywords: vec![
                "mcp".into(),
                "model context protocol".into(),
                "tools".into(),
            ],
            shortcut: None,
        }]);
    }
}
