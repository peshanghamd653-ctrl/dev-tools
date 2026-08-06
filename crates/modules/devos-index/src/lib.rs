//! Project index module: hybrid lexical + structural + semantic search over
//! project files.
//!
//! Three rankings, one result list:
//!
//! - **Lexical** — FTS5 chunks (50 lines, 1-based start lines) indexed
//!   incrementally by mtime/size, ranked with bm25 and rendered with
//!   `snippet()` highlights.
//! - **Structural** — the names a file *declares*, extracted with tree-sitter
//!   (`index_symbols`) and ranked by how exactly the query names them.
//! - **Semantic** — one embedding per chunk from a local Ollama, stored as an
//!   `f32` BLOB and scored by brute-force cosine similarity.
//!
//! They are merged with reciprocal-rank fusion, which reads only *positions*,
//! so bm25 (unbounded, lower-is-better), a discrete match tier and cosine
//! (`[-1, 1]`, higher-is-better) never have to be normalized against each
//! other.
//!
//! ## Why symbols became a ranking and not a chunking strategy
//!
//! Three uses of symbols were on the table: a table search consults, symbol
//! names prepended to each chunk's indexed text, and chunking on symbol
//! boundaries instead of fixed 50-line windows. The first shipped.
//!
//! Prepending names to chunk *content* rewrites the text bm25 scores, which
//! means it also rewrites what `snippet()` highlights and invalidates every
//! stored embedding — the vectors describe text that no longer exists. It
//! looks like the cheap option and is not.
//!
//! Chunking on symbol boundaries is the appealing one and the one that pays
//! least here. It re-keys every `index_chunks` and `index_embeddings` row
//! (both key on `(project, file, start_line)`), so it costs a forced reindex
//! *and* a full re-embed. In exchange it makes chunks the size of whatever
//! the author wrote: a 400-line React component or a long `match` becomes one
//! chunk that overflows `nomic-embed-text`'s input window and gets silently
//! truncated, while a one-line getter becomes a chunk of its own. Fixed
//! 50-line windows are, by accident, close to the right size for the
//! embedding model that actually runs here. The unit that retrieval returns
//! was not the problem; the *order* it returned them in was.
//!
//! So symbols correct the order instead. `index_symbols` is purely additive:
//! chunks keep their keys, embeddings stay valid, and an index built before
//! this existed needs one symbol-extraction pass, not a reindex.
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
//! Both extra legs are strictly additive. Ollama missing, the model not
//! pulled, the daemon down mid-run — every one of those leaves a complete
//! lexical index and a working search, and a project with no stored vectors
//! never issues a network request at query time at all.
//!
//! Symbols degrade the same way, file by file. A language with no grammar
//! (anything but Rust, TypeScript/TSX and JavaScript) and a file too broken
//! to parse both yield zero symbols and are indexed lexically exactly as they
//! were before; a query that names nothing declared anywhere skips the leg
//! entirely and comes back in plain bm25 order.

mod embeddings;
mod indexer;
mod symbols;
mod vector;

pub use embeddings::{
    EmbedError, EmbedResult, Embedder, OllamaEmbedder, DEFAULT_EMBED_MODEL, DEFAULT_OLLAMA_URL,
};
pub use indexer::{
    embedded_chunk_count, file_symbols, init, project_files, project_key, reindex_project,
    reindex_project_with, search, search_with, stats, symbol_count, IndexError, SearchHit,
};
pub use symbols::{
    extract as extract_symbols, is_supported as supports_symbols, Symbol, SymbolKind,
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
