//! Snippet library: short reusable fragments — a shell incantation, a
//! `tsconfig` block, a regex you will never write from memory again — that the
//! user saves once and pastes forever.
//!
//! The module owns one table and does one interesting thing: search. That
//! choice is deliberate and documented at [`repo::search_snippets`]; the short
//! version is that a substring scan is both faster to reason about and *more
//! correct* here than FTS5, because the thing being searched is code.

mod repo;

pub use repo::{
    delete_snippet, init, list_snippets, normalize_language, normalize_tags, save_snippet,
    search_snippets, DEFAULT_LANGUAGE,
};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use devos_kernel::module::{Module, ModuleCtx};
use devos_kernel::types::CommandDescriptor;

#[derive(Debug, thiserror::Error)]
pub enum SnippetError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("invalid snippet: {0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
}

pub type SnippetResult<T> = Result<T, SnippetError>;

// A saved fragment.
//
// Plain `//` throughout these DTOs, not `///`: ts-rs turns doc comments into
// TSDoc in the generated bindings, and the frontend is built against files
// that carry none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    pub id: String,
    pub title: String,
    // A free-form label ("rust", "powershell", "sql"). Nothing parses it —
    // there is no syntax highlighting — so an unknown value costs nothing.
    pub language: String,
    pub body: String,
    // Normalized on write: trimmed, lowercased, deduped, order preserved.
    pub tags: Vec<String>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

// What the editor sends back. `id` is the insert-vs-update switch: absent
// means "new snippet", present means "this one, edited". Modelled as one
// command rather than two so the page has a single Save button whose meaning
// does not depend on which pane the user last touched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct SnippetDraft {
    pub id: Option<String>,
    pub title: String,
    pub language: String,
    pub body: String,
    pub tags: Vec<String>,
}

pub struct SnippetsModule;

impl Module for SnippetsModule {
    fn id(&self) -> &'static str {
        "snippets"
    }

    fn register(&self, ctx: &ModuleCtx<'_>) {
        ctx.commands.register(vec![CommandDescriptor {
            id: "snippets.open".into(),
            module: "snippets".into(),
            title: "Open Snippets".into(),
            keywords: vec![
                "snippet".into(),
                "paste".into(),
                "clipboard".into(),
                "boilerplate".into(),
                "template".into(),
            ],
            shortcut: Some("Ctrl+Shift+N".into()),
        }]);
    }
}
