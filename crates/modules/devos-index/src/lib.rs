//! Project index module: full-text (FTS5) search over project files.
//!
//! This is the lexical half of the documented retrieval plan — bm25-ranked
//! content search with snippets, built incrementally (mtime/size) as a
//! kernel background job. The embeddings/vector half will layer onto the
//! same tables later; `search_code` results already carry file:line so the
//! UI/model contract won't change.

mod indexer;

pub use indexer::{
    init, project_files, project_key, reindex_project, search, stats, IndexError, SearchHit,
};

use devos_kernel::module::{Module, ModuleCtx};
use devos_kernel::types::CommandDescriptor;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One content-search hit, UI-facing (see `search` for the internal type).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct IndexHit {
    pub file: String,
    #[ts(type = "number")]
    pub start_line: i64,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct IndexStats {
    #[ts(type = "number")]
    pub files: i64,
    #[ts(type = "number")]
    pub chunks: i64,
    #[ts(type = "number | null")]
    pub indexed_at: Option<i64>,
}

pub struct IndexModule;

impl Module for IndexModule {
    fn id(&self) -> &'static str {
        "index"
    }

    fn register(&self, ctx: &ModuleCtx<'_>) {
        ctx.commands.register(vec![CommandDescriptor {
            id: "index.project".into(),
            module: "index".into(),
            title: "Index Project for AI Search".into(),
            keywords: vec!["search".into(), "rag".into(), "fts".into()],
            shortcut: None,
        }]);
    }
}
