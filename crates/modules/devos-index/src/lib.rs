//! Project index module: hybrid lexical + semantic search over project files.
//!
//! Two rankings, one result list:
//!
//! - **Lexical** — FTS5 chunks (50 lines, 1-based start lines) indexed
//!   incrementally by mtime/size, ranked with bm25 and rendered with
//!   `snippet()` highlights.
//! - **Semantic** — one embedding per chunk from a local Ollama, stored as an
//!   `f32` BLOB and scored by brute-force cosine similarity.
//!
//! They are merged with reciprocal-rank fusion, which reads only *positions*,
//! so bm25 (unbounded, lower-is-better) and cosine (`[-1, 1]`,
//! higher-is-better) never have to be normalized against each other.
//!
//! ## Why BLOBs and a linear scan instead of `sqlite-vec`
//!
//! `sqlite-vec` is a *loadable* SQLite extension, and sqlx only loads
//! extensions through `SqliteConnectOptions::extension()` — at connect time,
//! on options this module never sees, because the pool is built once by
//! `devos_kernel::db::open_pool`. sqlx also disables `ENABLE_LOAD_EXTENSION`
//! again as soon as a connection is established, so there is no runtime
//! escape hatch either. Static registration via `sqlite3_auto_extension` is
//! not exposed by sqlx at all, and would mean linking a second copy of
//! SQLite alongside the one `libsqlite3-sys` already bundles.
//!
//! Either route means shipping a native artifact and reaching outside this
//! crate. A linear scan does not: one project's index is thousands of chunks,
//! where comparing every vector is a few milliseconds. The stored layout is
//! deliberately the same little-endian `f32` packing `sqlite-vec` expects, so
//! swapping in a real vector index later is a query change, not a re-index.
//!
//! ## Degradation
//!
//! Embeddings are strictly additive. Ollama missing, the model not pulled,
//! the daemon down mid-run — every one of those leaves a complete lexical
//! index and a working search. A project with no stored vectors never issues
//! a network request at query time at all.

mod embeddings;
mod indexer;
mod vector;

pub use embeddings::{
    EmbedError, EmbedResult, Embedder, OllamaEmbedder, DEFAULT_EMBED_MODEL, DEFAULT_OLLAMA_URL,
};
pub use indexer::{
    embedded_chunk_count, init, project_files, project_key, reindex_project, reindex_project_with,
    search, search_with, stats, IndexError, SearchHit,
};
pub use vector::{cosine_similarity, reciprocal_rank_fusion, RRF_K};

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
