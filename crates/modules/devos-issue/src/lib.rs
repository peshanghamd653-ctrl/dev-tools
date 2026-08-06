//! Issue reporter module: capture the screen, then file a GitHub issue about
//! whatever it shows.
//!
//! Two decisions from the research recorded in `docs/feature-roadmap.md`
//! shape this crate and are not revisited here. First, **the image is never
//! uploaded**. GitHub documents no API for attaching an image to an issue, so
//! the PNG goes to the clipboard and the user pastes it into the issue body
//! in the browser — which keeps every network call this crate makes a
//! documented one, and leaves room to upgrade to a real attachment later
//! without rework. Second, **the token never lives in this crate**: every
//! entry point takes it as a parameter alongside the base URL, which keeps
//! secret handling in the command layer and — because the base URL is
//! injectable — lets every test in here run against a local socket instead of
//! api.github.com.

mod capture;
mod clipboard;
mod github;
mod remotes;

pub use capture::{capture_primary, KEEP_SHOTS};
pub use clipboard::copy_image;
pub use github::{create_issue, DEFAULT_BASE_URL};
pub use remotes::github_targets;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use devos_kernel::module::{Module, ModuleCtx};
use devos_kernel::types::CommandDescriptor;

#[derive(Debug, thiserror::Error)]
pub enum IssueError {
    #[error("no GitHub token configured")]
    NotConfigured,
    /// Held apart from [`IssueError::NotFound`] because the two send the user
    /// to different fixes: this one means the token itself is wrong — bad,
    /// expired, or lacking the `issues:write` scope.
    #[error("GitHub rejected the token (HTTP {status}) — it may be invalid, expired, or missing `issues:write`")]
    Auth { status: u16 },
    /// GitHub answers 404, not 403, for a repository the token cannot see, so
    /// this is the ordinary private-repo case at least as often as it is a
    /// typo. "Check that the token can see it" is the fix; "not found" alone
    /// would send the user hunting for a spelling mistake that isn't there.
    #[error("repository not found — it may not exist, or the token may not be allowed to see it")]
    NotFound,
    #[error("GitHub API error (HTTP {status}): {body}")]
    Api { status: u16, body: String },
    #[error("request failed: {0}")]
    Request(String),
    #[error("unexpected response from GitHub: {0}")]
    Decode(String),
    #[error("{0}")]
    Invalid(String),
    #[error("could not capture the screen: {0}")]
    Capture(String),
    #[error("could not copy the image: {0}")]
    Clipboard(String),
    #[error("could not read the project's git remotes: {0}")]
    Git(String),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
}

pub type IssueResult<T> = Result<T, IssueError>;

// A screenshot written to disk, waiting to be attached by hand.
//
// Plain `//` throughout these DTOs, not `///`, and on the struct as well as
// its fields: ts-rs turns every doc comment into TSDoc, and the frontend is
// built against binding files that carry none.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CapturedShot {
    // Absolute, inside the app data directory. The frontend hands it back to
    // the clipboard command rather than reading the file itself.
    pub path: String,
    pub width: u32,
    pub height: u32,
    #[ts(type = "number")]
    pub captured_at: i64,
}

// A GitHub repository an issue could be filed against, discovered from one of
// the project's git remotes.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct IssueTarget {
    pub owner: String,
    pub name: String,
    // The git remote this came from ("origin", "upstream"), so a project with
    // a fork and its parent can be told apart in the picker.
    pub remote: String,
}

// An issue that now exists on GitHub.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CreatedIssue {
    #[ts(type = "number")]
    pub number: i64,
    pub url: String,
    pub title: String,
}

pub struct IssueModule;

impl Module for IssueModule {
    fn id(&self) -> &'static str {
        "issue"
    }

    fn register(&self, ctx: &ModuleCtx<'_>) {
        ctx.commands.register(vec![CommandDescriptor {
            id: "issue.capture".into(),
            module: "issue".into(),
            title: "Capture Screenshot → Issue".into(),
            keywords: vec![
                "screenshot".into(),
                "bug".into(),
                "report".into(),
                "github".into(),
                "issue".into(),
            ],
            shortcut: Some("Ctrl+Shift+S".into()),
        }]);
    }
}
