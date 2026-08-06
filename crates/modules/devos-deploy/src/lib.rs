//! Deployments module: read-only visibility into Vercel projects and the
//! deployments they have shipped.
//!
//! Two decisions shape the whole module. First, it is *read-only on purpose*,
//! not read-only until M5 — redeploying, promoting, or rolling back changes
//! what the public sees on someone's production site, and nothing inside
//! DevOS can undo that, so those buttons simply do not exist (the same
//! reasoning that kept AI tools read-only first, ADR-0005). Listing what
//! shipped carries none of that risk and is useful on its own.
//!
//! Second, the token never lives in this crate. Every entry point takes it as
//! a parameter alongside the base URL, which keeps secret handling in the
//! command layer and — because the base URL is injectable — lets every test
//! in here run against a local socket instead of the internet.

mod vercel;

pub use vercel::{list_deployments, list_projects, DEFAULT_BASE_URL};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use devos_kernel::module::{Module, ModuleCtx};
use devos_kernel::types::CommandDescriptor;

#[derive(Debug, thiserror::Error)]
pub enum DeployError {
    #[error("no Vercel token configured")]
    NotConfigured,
    /// Held apart from [`DeployError::Api`] so the UI can say "your token is
    /// bad or expired" — a thing the user can actually fix — rather than
    /// showing a generic failure.
    #[error("Vercel rejected the token (HTTP {status})")]
    Auth { status: u16 },
    #[error("Vercel API error (HTTP {status}): {body}")]
    Api { status: u16, body: String },
    #[error("request failed: {0}")]
    Request(String),
    #[error("unexpected response from Vercel: {0}")]
    Decode(String),
}

pub type DeployResult<T> = Result<T, DeployError>;

// A Vercel project the stored token can see.
//
// Plain `//` throughout these DTOs, not `///`, and on the struct as well as
// its fields: ts-rs turns every doc comment into TSDoc, and the frontend is
// built against binding files that carry none.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DeployProject {
    pub id: String,
    pub name: String,
    // Vercel's detected framework ("nextjs", "vite", …); absent for a project
    // it could not classify.
    pub framework: Option<String>,
    #[ts(type = "number")]
    pub updated_at: i64,
}

// One deployment of one project. Everything here is descriptive — there is
// no field that could be sent back to Vercel to act on it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Deployment {
    pub id: String,
    pub name: String,
    // Absolute, scheme included — the frontend renders it as a link.
    pub url: String,
    // Uppercase, as Vercel sends it: READY / ERROR / BUILDING / QUEUED /
    // CANCELED.
    pub state: String,
    // "production" or "preview"; absent on deployments Vercel did not tag.
    pub target: Option<String>,
    #[ts(type = "number")]
    pub created_at: i64,
    // Absent whenever the deployment had no commit behind it — a CLI deploy
    // is the common case.
    pub commit_message: Option<String>,
}

pub struct DeployModule;

impl Module for DeployModule {
    fn id(&self) -> &'static str {
        "deploy"
    }

    fn register(&self, ctx: &ModuleCtx<'_>) {
        ctx.commands.register(vec![CommandDescriptor {
            id: "deploy.open".into(),
            module: "deploy".into(),
            title: "Open Deployments".into(),
            keywords: vec![
                "vercel".into(),
                "deploy".into(),
                "release".into(),
                "hosting".into(),
            ],
            shortcut: Some("Ctrl+Shift+D".into()),
        }]);
    }
}
