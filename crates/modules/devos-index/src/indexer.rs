//! Incremental FTS5 indexing plus the two rankings layered over it: chunk
//! embeddings stored as BLOBs and scored by brute-force cosine, and
//! tree-sitter symbols scored by how exactly the query names them. All three
//! are merged by reciprocal-rank fusion behind the unchanged `search` entry
//! point.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::{Row, SqlitePool};

use crate::embeddings::{
    Embedder, OllamaEmbedder, DEFAULT_EMBED_MODEL, DEFAULT_OLLAMA_URL, INDEX_TIMEOUT, QUERY_TIMEOUT,
};
use crate::symbols::{self, Symbol};
use crate::vector::{
    cosine_similarity, decode_vector, encode_vector, reciprocal_rank_fusion, RRF_K,
};
use crate::{IndexStats, SymbolHit};

const CHUNK_LINES: usize = 50;
const MAX_FILE_BYTES: u64 = 512 * 1024;
const MAX_WALK_DEPTH: usize = 12;
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    ".venv",
    "__pycache__",
];

/// Texts per embedding request on the batched route.
const EMBED_BATCH: usize = 16;
/// Ceiling on new embeddings per reindex. A first run over a huge repo with
/// a slow local model would otherwise take hours; the pass is incremental,
/// so the next run picks up exactly where this one stopped.
const MAX_EMBED_PER_RUN: usize = 4_000;
/// Characters of leading chunk text shown for a vector-only hit.
const EXCERPT_CHARS: usize = 160;

/// What `symbols::extract` currently produces. Bumping it makes the next
/// reindex of each project re-extract its symbols — see `reindex_project_with`
/// for why that is a symbol-only pass and not a full reindex.
const SYMBOLS_VERSION: i64 = 1;
const META_SYMBOLS_VERSION: &str = "symbols.version";
/// Identifiers taken from one query. Past a handful, extra terms are noise
/// from a natural-language question rather than a name someone is looking for.
const MAX_SYMBOL_TERMS: usize = 8;
/// Rows fetched per term before ranking. Generous — the tiers below discard
/// most of them — but bounded so a one-letter prefix cannot scan a whole
/// project's symbol table into memory.
const SYMBOL_CANDIDATES_PER_TERM: i64 = 8;

/// Set to `off` to disable embeddings entirely (indexing and search stay
/// lexical, and nothing ever contacts Ollama).
const SETTING_ENABLED: &str = "index.embeddings";
/// Shared with the AI module — a user who moved Ollama sets it once.
const SETTING_OLLAMA_URL: &str = "ai.ollama.url";
const SETTING_EMBED_MODEL: &str = "index.embed.model";

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

pub type IndexResult<T> = Result<T, IndexError>;

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub file: String,
    pub start_line: i64,
    pub snippet: String,
}

/// Identifies one chunk within a project. Fusion works on these, not on
/// hits, so the two rankings can be merged before snippets are chosen.
type ChunkKey = (String, i64);

/// Normalized key identifying a project in the index. Both indexing and
/// search must derive it the same way from the stored `projects.path`.
pub fn project_key(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_string()
}

pub async fn init(pool: &SqlitePool) -> IndexResult<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS index_files (
            project    TEXT NOT NULL,
            file       TEXT NOT NULL,
            mtime      INTEGER NOT NULL,
            size       INTEGER NOT NULL,
            indexed_at INTEGER NOT NULL,
            PRIMARY KEY (project, file)
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE VIRTUAL TABLE IF NOT EXISTS index_chunks USING fts5(
            content, project UNINDEXED, file UNINDEXED, start_line UNINDEXED
        )",
    )
    .execute(pool)
    .await?;
    // One embedding per chunk. `model` is stored so switching models
    // invalidates the old rows instead of mixing embedding spaces; `dim`
    // lets search reject anything the query vector can't be compared to.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS index_embeddings (
            project    TEXT NOT NULL,
            file       TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            model      TEXT NOT NULL,
            dim        INTEGER NOT NULL,
            vector     BLOB NOT NULL,
            PRIMARY KEY (project, file, start_line)
        )",
    )
    .execute(pool)
    .await?;
    // Definitions extracted by tree-sitter. Deliberately no primary key: one
    // file legitimately declares the same name more than once (`fn new` in
    // two `impl` blocks, an overload pair in TypeScript), and collapsing
    // those would lose the line that distinguishes them.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS index_symbols (
            project    TEXT NOT NULL,
            file       TEXT NOT NULL,
            name       TEXT NOT NULL,
            kind       TEXT NOT NULL,
            start_line INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS index_symbols_by_name
         ON index_symbols (project, name)",
    )
    .execute(pool)
    .await?;
    // Every write path is "replace this file's symbols", so the delete needs
    // to be as cheap as the lookup.
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS index_symbols_by_file
         ON index_symbols (project, file)",
    )
    .execute(pool)
    .await?;
    // Per-project format markers. A module-owned table rather than the
    // kernel's `settings` because the index has to work against a pool that
    // has no `settings` table at all — which is exactly what the tests use.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS index_meta (
            project TEXT NOT NULL,
            key     TEXT NOT NULL,
            value   TEXT NOT NULL,
            PRIMARY KEY (project, key)
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// All indexable file paths in a project (relative, `/`-separated), using
/// the same walk rules as the indexer. Used by filename search in the UI.
pub fn project_files(root: &Path) -> Vec<String> {
    let mut disk: Vec<(String, i64, i64)> = Vec::new();
    walk_files(root, root, 0, &mut disk);
    disk.into_iter().map(|(path, _, _)| path).collect()
}

/// Bring the index in line with the project directory. Unchanged files
/// (same mtime + size) are skipped; deleted files are pruned. Chunks that
/// have no embedding yet are embedded if a backend is reachable — and the
/// run succeeds either way.
pub async fn reindex_project(pool: &SqlitePool, project_path: &str) -> IndexResult<IndexStats> {
    let embedder = configured_embedder(pool, INDEX_TIMEOUT).await;
    reindex_project_with(pool, project_path, embedder.as_ref().map(as_embedder)).await
}

/// [`reindex_project`] with an explicit embeddings backend. `None` indexes
/// lexically only.
pub async fn reindex_project_with(
    pool: &SqlitePool,
    project_path: &str,
    embedder: Option<&dyn Embedder>,
) -> IndexResult<IndexStats> {
    let root = PathBuf::from(project_path);
    if !root.is_dir() {
        return Err(IndexError::InvalidInput(format!(
            "not a directory: {project_path}"
        )));
    }
    let project = project_key(project_path);
    let now = chrono::Utc::now().timestamp_millis();

    // Current on-disk state.
    let mut disk: Vec<(String, i64, i64)> = Vec::new();
    walk_files(&root, &root, 0, &mut disk);

    // Current indexed state.
    let rows = sqlx::query("SELECT file, mtime, size FROM index_files WHERE project = ?1")
        .bind(&project)
        .fetch_all(pool)
        .await?;
    let mut indexed: HashMap<String, (i64, i64)> = rows
        .into_iter()
        .map(|r| {
            (
                r.get::<String, _>("file"),
                (r.get::<i64, _>("mtime"), r.get::<i64, _>("size")),
            )
        })
        .collect();

    // An index written before symbol extraction existed has correct chunks
    // and correct embeddings but no symbols — and every one of its files
    // passes the mtime/size skip, so nothing would ever extract them. A
    // stored format version turns that into exactly one extra pass, and
    // deliberately *not* a reindex: symbols live in their own table keyed the
    // way chunks always were, so chunks and the embeddings hanging off them
    // are never invalidated and nothing is re-embedded.
    let backfill_symbols = symbols_version(pool, &project).await != Some(SYMBOLS_VERSION);

    let mut changed = 0usize;
    let mut extracted = 0usize;
    for (file, mtime, size) in &disk {
        let unchanged = indexed
            .remove(file)
            .is_some_and(|(m, s)| m == *mtime && s == *size);
        if unchanged && !backfill_symbols {
            continue;
        }
        let path = root.join(file);
        if unchanged && !symbols::is_supported(&path) {
            // Backfill pass over a file no grammar can read. There is nothing
            // to extract, so skip the read entirely — but still drop any rows
            // an older format version left behind, which is the only way a
            // future version that *narrows* the language set stays correct.
            remove_symbols(pool, &project, file).await?;
            continue;
        }
        let content = read_text(&path);
        if !unchanged {
            let Some(content) = content.as_deref() else {
                // Binary/unreadable: make sure nothing stale remains.
                remove_file(pool, &project, file).await?;
                continue;
            };
            remove_chunks(pool, &project, file).await?;
            for (start_line, chunk) in chunk_lines(content, CHUNK_LINES) {
                sqlx::query(
                    "INSERT INTO index_chunks (content, project, file, start_line)
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .bind(chunk)
                .bind(&project)
                .bind(file)
                .bind(start_line as i64)
                .execute(pool)
                .await?;
            }
            sqlx::query(
                "INSERT INTO index_files (project, file, mtime, size, indexed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(project, file) DO UPDATE SET
                   mtime = excluded.mtime, size = excluded.size, indexed_at = excluded.indexed_at",
            )
            .bind(&project)
            .bind(file)
            .bind(mtime)
            .bind(size)
            .bind(now)
            .execute(pool)
            .await?;
            changed += 1;
        }
        // Reached only for a file that changed, or on the one-time backfill
        // pass. `extract` returns nothing for a file it has no grammar for
        // and nothing it cannot parse, so this is where degradation happens:
        // silently, per file, with the lexical index already written above.
        //
        // An unchanged file that suddenly cannot be read during the backfill
        // keeps the symbols it already has rather than being wiped — it was
        // valid a moment ago and the mtime/size pass did not object.
        if let Some(content) = content.as_deref() {
            let found = symbols::extract(&path, content);
            extracted += found.len();
            replace_symbols(pool, &project, file, &found).await?;
        }
    }

    // Anything left in `indexed` no longer exists on disk.
    for file in indexed.into_keys() {
        remove_file(pool, &project, &file).await?;
        changed += 1;
    }

    // Embeddings are keyed to chunks, and a chunk's embedding is deleted
    // only when the chunk itself is (see `remove_chunks`). So this pass sees
    // exactly the chunks that are new or rewritten — an untouched file is
    // never re-embedded — and it also backfills anything an earlier run
    // missed because the backend was down at the time.
    let embedded = match embedder {
        Some(embedder) => embed_pending_chunks(pool, &project, embedder).await,
        None => 0,
    };

    // Last, so a run that failed part-way retries the backfill next time
    // instead of recording work it did not finish.
    if backfill_symbols {
        set_symbols_version(pool, &project, SYMBOLS_VERSION).await?;
    }

    let result = stats(pool, project_path).await?;
    tracing::info!(project = %project, changed, embedded, symbols = extracted, files = result.files, chunks = result.chunks, "index updated");
    Ok(result)
}

/// Hybrid content search: bm25-ranked FTS5 hits fused with symbol-name hits
/// and vector-similarity hits. Falls back to lexical-only results whenever
/// the other two legs have nothing to say, which is the common case.
pub async fn search(
    pool: &SqlitePool,
    project_path: &str,
    query: &str,
    limit: i64,
) -> IndexResult<Vec<SearchHit>> {
    let embedder = configured_embedder(pool, QUERY_TIMEOUT).await;
    search_with(
        pool,
        project_path,
        query,
        limit,
        embedder.as_ref().map(as_embedder),
    )
    .await
}

/// [`search`] with an explicit embeddings backend. `None` searches lexically
/// only.
pub async fn search_with(
    pool: &SqlitePool,
    project_path: &str,
    query: &str,
    limit: i64,
    embedder: Option<&dyn Embedder>,
) -> IndexResult<Vec<SearchHit>> {
    let project = project_key(project_path);
    // The lexical leg runs first and its errors still propagate — an empty
    // or unusable query is a caller mistake, not a degradation.
    let lexical = lexical_search(pool, &project, query, limit).await?;
    let symbolic = symbol_search(pool, &project, query, limit).await;
    let semantic = match embedder {
        Some(embedder) => semantic_search(pool, &project, query, limit, embedder).await,
        None => Vec::new(),
    };
    // Nothing to fuse: hand back bm25 order untouched, byte for byte what
    // this function returned before either extra leg existed.
    if symbolic.is_empty() && semantic.is_empty() {
        return Ok(lexical);
    }
    Ok(fuse(vec![lexical, symbolic, semantic], limit))
}

pub async fn stats(pool: &SqlitePool, project_path: &str) -> IndexResult<IndexStats> {
    let project = project_key(project_path);
    let row = sqlx::query(
        "SELECT COUNT(*) AS files, MAX(indexed_at) AS last FROM index_files WHERE project = ?1",
    )
    .bind(&project)
    .fetch_one(pool)
    .await?;
    let chunks = sqlx::query("SELECT COUNT(*) AS n FROM index_chunks WHERE project = ?1")
        .bind(&project)
        .fetch_one(pool)
        .await?;
    Ok(IndexStats {
        files: row.get("files"),
        chunks: chunks.get("n"),
        indexed_at: row.get("last"),
    })
}

/// How many chunks of a project currently carry an embedding. Diagnostic —
/// the IPC-facing `IndexStats` shape is unchanged.
pub async fn embedded_chunk_count(pool: &SqlitePool, project_path: &str) -> IndexResult<i64> {
    let row = sqlx::query("SELECT COUNT(*) AS n FROM index_embeddings WHERE project = ?1")
        .bind(project_key(project_path))
        .fetch_one(pool)
        .await?;
    Ok(row.get("n"))
}

/// How many symbols a project's files currently declare. Diagnostic, same as
/// [`embedded_chunk_count`]: no command exposes it.
pub async fn symbol_count(pool: &SqlitePool, project_path: &str) -> IndexResult<i64> {
    let row = sqlx::query("SELECT COUNT(*) AS n FROM index_symbols WHERE project = ?1")
        .bind(project_key(project_path))
        .fetch_one(pool)
        .await?;
    Ok(row.get("n"))
}

/// Every symbol declared by one file, in `(name, kind, start_line)` form and
/// document order. Diagnostic; also what makes extraction assertable end to
/// end rather than only at the parser.
pub async fn file_symbols(
    pool: &SqlitePool,
    project_path: &str,
    file: &str,
) -> IndexResult<Vec<(String, String, i64)>> {
    let rows = sqlx::query(
        "SELECT name, kind, start_line FROM index_symbols
         WHERE project = ?1 AND file = ?2
         ORDER BY start_line, name",
    )
    .bind(project_key(project_path))
    .bind(file)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            (
                r.get::<String, _>("name"),
                r.get::<String, _>("kind"),
                r.get::<i64, _>("start_line"),
            )
        })
        .collect())
}

// ---- embeddings backend selection ----

fn as_embedder(embedder: &OllamaEmbedder) -> &dyn Embedder {
    embedder
}

/// Read a kernel setting, treating any failure (including a pool that has no
/// `settings` table at all) as "unset".
async fn setting(pool: &SqlitePool, key: &str) -> Option<String> {
    devos_kernel::repo::get_setting(pool, key)
        .await
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty())
}

/// The embedder implied by settings. Constructing one is free and opens no
/// connection — whether a backend actually exists is discovered lazily, on
/// the first request, and is never fatal.
async fn configured_embedder(pool: &SqlitePool, timeout: Duration) -> Option<OllamaEmbedder> {
    if setting(pool, SETTING_ENABLED).await.as_deref() == Some("off") {
        return None;
    }
    let base = setting(pool, SETTING_OLLAMA_URL)
        .await
        .unwrap_or_else(|| DEFAULT_OLLAMA_URL.to_string());
    let model = setting(pool, SETTING_EMBED_MODEL)
        .await
        .unwrap_or_else(|| DEFAULT_EMBED_MODEL.to_string());
    Some(OllamaEmbedder::new(&base, &model, timeout))
}

// ---- indexing side ----

/// Embed every chunk that has no vector for the current model. Returns the
/// number stored; **never fails the caller**. The first failed request ends
/// the pass, so a reindex with no Ollama running costs one refused
/// connection, not one per chunk.
async fn embed_pending_chunks(pool: &SqlitePool, project: &str, embedder: &dyn Embedder) -> usize {
    let model = embedder.model_id().to_string();
    let mut pending = match pending_chunk_keys(pool, project, &model).await {
        Ok(pending) => pending,
        Err(e) => {
            tracing::warn!(project, error = %e, "could not determine chunks needing embeddings");
            return 0;
        }
    };
    if pending.is_empty() {
        return 0;
    }
    if pending.len() > MAX_EMBED_PER_RUN {
        tracing::info!(
            project,
            pending = pending.len(),
            cap = MAX_EMBED_PER_RUN,
            "embedding backlog capped for this run; the next reindex continues"
        );
        pending.truncate(MAX_EMBED_PER_RUN);
    }

    let mut stored = 0usize;
    for batch in pending.chunks(EMBED_BATCH) {
        let mut keys: Vec<ChunkKey> = Vec::with_capacity(batch.len());
        let mut texts: Vec<String> = Vec::with_capacity(batch.len());
        for (file, start_line) in batch {
            match chunk_content(pool, project, file, *start_line).await {
                Ok(Some(content)) => {
                    keys.push((file.clone(), *start_line));
                    texts.push(content);
                }
                // Chunk vanished between the two queries: nothing to embed.
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!(project, error = %e, "reading chunk for embedding failed");
                    return stored;
                }
            }
        }
        if texts.is_empty() {
            continue;
        }
        let vectors = match embedder.embed(&texts).await {
            Ok(vectors) => vectors,
            Err(e) => {
                // The expected path for most users. Info, not warn: nothing
                // is broken, the index is simply lexical-only.
                tracing::info!(
                    project,
                    model,
                    error = %e,
                    "embeddings backend unavailable; index stays lexical-only"
                );
                return stored;
            }
        };
        for ((file, start_line), vector) in keys.iter().zip(vectors) {
            match store_embedding(pool, project, file, *start_line, &model, &vector).await {
                Ok(()) => stored += 1,
                Err(e) => {
                    tracing::warn!(project, error = %e, "storing embedding failed");
                    return stored;
                }
            }
        }
    }
    stored
}

/// Chunks with no vector for `model`, in table order.
async fn pending_chunk_keys(
    pool: &SqlitePool,
    project: &str,
    model: &str,
) -> IndexResult<Vec<ChunkKey>> {
    let chunks = sqlx::query("SELECT file, start_line FROM index_chunks WHERE project = ?1")
        .bind(project)
        .fetch_all(pool)
        .await?;
    let embedded = sqlx::query(
        "SELECT file, start_line FROM index_embeddings WHERE project = ?1 AND model = ?2",
    )
    .bind(project)
    .bind(model)
    .fetch_all(pool)
    .await?;
    let have: HashSet<ChunkKey> = embedded
        .into_iter()
        .map(|r| (r.get::<String, _>("file"), r.get::<i64, _>("start_line")))
        .collect();
    Ok(chunks
        .into_iter()
        .map(|r| (r.get::<String, _>("file"), r.get::<i64, _>("start_line")))
        .filter(|key| !have.contains(key))
        .collect())
}

async fn chunk_content(
    pool: &SqlitePool,
    project: &str,
    file: &str,
    start_line: i64,
) -> IndexResult<Option<String>> {
    let row = sqlx::query(
        "SELECT content FROM index_chunks
         WHERE project = ?1 AND file = ?2 AND start_line = ?3 LIMIT 1",
    )
    .bind(project)
    .bind(file)
    .bind(start_line)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.get::<String, _>("content")))
}

async fn store_embedding(
    pool: &SqlitePool,
    project: &str,
    file: &str,
    start_line: i64,
    model: &str,
    vector: &[f32],
) -> IndexResult<()> {
    sqlx::query(
        "INSERT INTO index_embeddings (project, file, start_line, model, dim, vector)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(project, file, start_line) DO UPDATE SET
           model = excluded.model, dim = excluded.dim, vector = excluded.vector",
    )
    .bind(project)
    .bind(file)
    .bind(start_line)
    .bind(model)
    .bind(vector.len() as i64)
    .bind(encode_vector(vector))
    .execute(pool)
    .await?;
    Ok(())
}

// ---- symbols, indexing side ----

/// Swap a file's symbols for a freshly extracted set. Always deletes first,
/// including when `found` is empty: a file that stopped parsing, or stopped
/// declaring anything, must stop contributing to ranking immediately.
async fn replace_symbols(
    pool: &SqlitePool,
    project: &str,
    file: &str,
    found: &[Symbol],
) -> IndexResult<()> {
    remove_symbols(pool, project, file).await?;
    for symbol in found {
        sqlx::query(
            "INSERT INTO index_symbols (project, file, name, kind, start_line)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(project)
        .bind(file)
        .bind(&symbol.name)
        .bind(symbol.kind.as_str())
        .bind(symbol.start_line)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn remove_symbols(pool: &SqlitePool, project: &str, file: &str) -> IndexResult<()> {
    sqlx::query("DELETE FROM index_symbols WHERE project = ?1 AND file = ?2")
        .bind(project)
        .bind(file)
        .execute(pool)
        .await?;
    Ok(())
}

/// The symbol format this project's rows were written by, if any. Any read
/// failure reports `None`, which costs one backfill pass and never a wrong
/// answer.
async fn symbols_version(pool: &SqlitePool, project: &str) -> Option<i64> {
    sqlx::query("SELECT value FROM index_meta WHERE project = ?1 AND key = ?2")
        .bind(project)
        .bind(META_SYMBOLS_VERSION)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .and_then(|row| row.get::<String, _>("value").parse().ok())
}

async fn set_symbols_version(pool: &SqlitePool, project: &str, version: i64) -> IndexResult<()> {
    sqlx::query(
        "INSERT INTO index_meta (project, key, value) VALUES (?1, ?2, ?3)
         ON CONFLICT(project, key) DO UPDATE SET value = excluded.value",
    )
    .bind(project)
    .bind(META_SYMBOLS_VERSION)
    .bind(version.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

// ---- search side ----

async fn lexical_search(
    pool: &SqlitePool,
    project: &str,
    query: &str,
    limit: i64,
) -> IndexResult<Vec<SearchHit>> {
    let fts_query = sanitize_query(query)?;
    let rows = sqlx::query(
        "SELECT file, start_line, snippet(index_chunks, 0, '»', '«', ' … ', 16) AS snip
         FROM index_chunks
         WHERE index_chunks MATCH ?1 AND project = ?2
         ORDER BY bm25(index_chunks)
         LIMIT ?3",
    )
    .bind(&fts_query)
    .bind(project)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| SearchHit {
            file: r.get("file"),
            start_line: r.get("start_line"),
            snippet: r.get("snip"),
        })
        .collect())
}

/// Structural leg: chunks that *declare* something the query names.
///
/// This is the whole point of extracting symbols. bm25 cannot tell a
/// definition from a mention, so a file that says `verify_token` three times
/// in comments outranks the file that actually contains
/// `fn verify_token`. Matching the query's identifiers against declared names
/// produces a second ranking where the definition is first, and fusion lets
/// that correct the lexical order without overriding it.
///
/// Soft-failing like the vector leg: an unreadable symbol table costs
/// ranking quality, never results.
async fn symbol_search(
    pool: &SqlitePool,
    project: &str,
    query: &str,
    limit: i64,
) -> Vec<SearchHit> {
    let terms = identifier_terms(query);
    if terms.is_empty() || limit <= 0 {
        return Vec::new();
    }

    let mut candidates: Vec<(u8, usize, ChunkKey)> = Vec::new();
    for term in &terms {
        // `LIKE` is case-insensitive for ASCII in SQLite, so one query covers
        // the exact, case-folded and prefix cases; `match_tier` separates
        // them afterwards. Escaping matters more than it looks: `_` is a
        // single-character wildcard, and snake_case names are full of it.
        //
        // The `ESCAPE` clause does cost the LIKE-prefix index optimization —
        // SQLite disables it whenever one is present — so this walks the
        // `(project, name)` index entries for one project rather than seeking
        // straight to them. That is a few thousand rows per term against a
        // correct answer, and cheaper than the cosine scan already running
        // beside it; matching `verify_token` literally is not negotiable.
        let rows = sqlx::query(
            "SELECT file, name, start_line FROM index_symbols
             WHERE project = ?1 AND name LIKE ?2 ESCAPE '\\'
             LIMIT ?3",
        )
        .bind(project)
        .bind(format!("{}%", escape_like(term)))
        .bind(limit.saturating_mul(SYMBOL_CANDIDATES_PER_TERM))
        .fetch_all(pool)
        .await;
        let rows = match rows {
            Ok(rows) => rows,
            Err(e) => {
                tracing::debug!(project, error = %e, "symbol lookup failed; ranking without it");
                return Vec::new();
            }
        };
        for row in rows {
            let name: String = row.get("name");
            let tier = match_tier(&name, term);
            candidates.push((
                tier,
                name.chars().count(),
                (
                    row.get::<String, _>("file"),
                    chunk_start_for(row.get::<i64, _>("start_line")),
                ),
            ));
        }
    }

    // Exact beats case-folded beats prefix; inside a tier the shorter name is
    // the closer match (`token` before `token_bucket_refill_interval`). File
    // and line settle the rest so the ranking is reproducible run to run.
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));

    let mut hits: Vec<SearchHit> = Vec::new();
    let mut seen: HashSet<ChunkKey> = HashSet::new();
    for (_, _, key) in candidates {
        if hits.len() as i64 >= limit {
            break;
        }
        if !seen.insert(key.clone()) {
            continue;
        }
        let (file, start_line) = key;
        match chunk_content(pool, project, &file, start_line).await {
            // A declaration has no matched *term* to mark up — the query may
            // not even share a token with it — so this reads like a vector
            // hit: leading text, no »« markers.
            Ok(Some(content)) => hits.push(SearchHit {
                file,
                start_line,
                snippet: excerpt(&content, EXCERPT_CHARS),
            }),
            // The chunk went away between the two queries; skip it.
            Ok(None) => {}
            Err(e) => {
                tracing::debug!(project, error = %e, "reading chunk for a symbol hit failed");
                break;
            }
        }
    }
    hits
}

/// Workspace-wide "go to symbol": every declaration whose name matches the
/// query, ranked exact > case-folded > prefix (same tiering as
/// [`symbol_search`] above), then by name length so `token` beats
/// `token_bucket_refill_interval`.
///
/// Deliberately separate from `symbol_search`: that one blends symbol hits
/// into chunk-level results to correct general search ranking, and soft-fails
/// because it is one signal among three. This returns declaration sites
/// themselves — file, line, kind — for a symbol picker that wants to jump
/// straight to `fn verify_token`, and a caller who explicitly asked "where is
/// this symbol" is better served by a real error than a silently empty list.
pub async fn find_symbols(
    pool: &SqlitePool,
    project_path: &str,
    query: &str,
    limit: i64,
) -> IndexResult<Vec<SymbolHit>> {
    let project = project_key(project_path);
    let terms = identifier_terms(query);
    if terms.is_empty() || limit <= 0 {
        return Ok(Vec::new());
    }

    let mut candidates: Vec<(u8, usize, SymbolHit)> = Vec::new();
    for term in &terms {
        let rows = sqlx::query(
            "SELECT file, name, kind, start_line FROM index_symbols
             WHERE project = ?1 AND name LIKE ?2 ESCAPE '\\'
             LIMIT ?3",
        )
        .bind(&project)
        .bind(format!("{}%", escape_like(term)))
        .bind(limit.saturating_mul(SYMBOL_CANDIDATES_PER_TERM))
        .fetch_all(pool)
        .await?;
        for row in rows {
            let name: String = row.get("name");
            let tier = match_tier(&name, term);
            let len = name.chars().count();
            candidates.push((
                tier,
                len,
                SymbolHit {
                    file: row.get("file"),
                    kind: row.get("kind"),
                    start_line: row.get("start_line"),
                    name,
                },
            ));
        }
    }

    // Same ordering rule as `symbol_search`: tier, then name length, then a
    // deterministic tiebreak so repeated queries return the same order.
    candidates.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then(a.2.file.cmp(&b.2.file))
            .then(a.2.start_line.cmp(&b.2.start_line))
    });

    let mut hits: Vec<SymbolHit> = Vec::new();
    let mut seen: HashSet<(String, String, i64)> = HashSet::new();
    for (_, _, hit) in candidates {
        if hits.len() as i64 >= limit {
            break;
        }
        if !seen.insert((hit.file.clone(), hit.name.clone(), hit.start_line)) {
            continue;
        }
        hits.push(hit);
    }
    Ok(hits)
}

/// Identifier-shaped tokens in a query, in order, deduplicated.
///
/// Splitting on everything that cannot appear *inside* an identifier is what
/// makes `ProjectTools::execute`, `store.verify_token(x)` and
/// `where is verify_token` all yield the same lookups.
fn identifier_terms(query: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in query.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')) {
        // One-character tokens and anything starting with a digit are not
        // names worth looking up; they only add noise to the ranking.
        if token.chars().nth(1).is_none()
            || !token.starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '$')
        {
            continue;
        }
        if !out.iter().any(|existing| existing == token) {
            out.push(token.to_string());
        }
        if out.len() == MAX_SYMBOL_TERMS {
            break;
        }
    }
    out
}

/// 0 = exact, 1 = same name in another case, 2 = the query is a prefix of it.
/// Anything else never reaches here: the `LIKE` guarantees at least a prefix.
fn match_tier(name: &str, term: &str) -> u8 {
    if name == term {
        0
    } else if name.eq_ignore_ascii_case(term) {
        1
    } else {
        2
    }
}

/// Neutralize SQL `LIKE` metacharacters so a name is matched literally.
fn escape_like(term: &str) -> String {
    let mut out = String::with_capacity(term.len() + 8);
    for ch in term.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// The 1-based start line of the fixed-size chunk that contains `line`.
///
/// Must agree with [`chunk_lines`] exactly: symbols are stored at their own
/// declaration line, and mapping that back onto a chunk boundary is what lets
/// the symbol ranking share a key space with the other two legs — so fusion
/// can merge them at all.
fn chunk_start_for(line: i64) -> i64 {
    let line = line.max(1);
    let size = CHUNK_LINES as i64;
    (line - 1) / size * size + 1
}

/// Vector leg. Returns an empty list — never an error — for every kind of
/// unavailability, so the caller keeps whatever lexical hits it has.
async fn semantic_search(
    pool: &SqlitePool,
    project: &str,
    query: &str,
    limit: i64,
    embedder: &dyn Embedder,
) -> Vec<SearchHit> {
    // The gate that keeps users without Ollama at exactly zero cost: a
    // project with no stored vectors never triggers a network request, so
    // search stays as fast as it was before embeddings existed.
    match has_embeddings(pool, project, embedder.model_id()).await {
        Ok(true) => {}
        Ok(false) => return Vec::new(),
        Err(e) => {
            tracing::debug!(project, error = %e, "embedding lookup failed; lexical only");
            return Vec::new();
        }
    }

    let query_vector = match embedder.embed(&[query.to_string()]).await {
        Ok(vectors) => match vectors.into_iter().next() {
            Some(vector) => vector,
            None => return Vec::new(),
        },
        Err(e) => {
            tracing::debug!(project, error = %e, "query embedding failed; lexical only");
            return Vec::new();
        }
    };

    match nearest_chunks(pool, project, embedder.model_id(), &query_vector, limit).await {
        Ok(hits) => hits,
        Err(e) => {
            tracing::debug!(project, error = %e, "vector scan failed; lexical only");
            Vec::new()
        }
    }
}

async fn has_embeddings(pool: &SqlitePool, project: &str, model: &str) -> IndexResult<bool> {
    let row = sqlx::query(
        "SELECT EXISTS(
            SELECT 1 FROM index_embeddings WHERE project = ?1 AND model = ?2
         ) AS present",
    )
    .bind(project)
    .bind(model)
    .fetch_one(pool)
    .await?;
    Ok(row.get::<i64, _>("present") != 0)
}

/// Brute-force cosine over every stored vector for this project.
///
/// Linear, and deliberately so: one project's index is thousands of chunks,
/// where a full scan of 768-float vectors is a few milliseconds and costs no
/// extension, no index build, and no staleness.
async fn nearest_chunks(
    pool: &SqlitePool,
    project: &str,
    model: &str,
    query_vector: &[f32],
    limit: i64,
) -> IndexResult<Vec<SearchHit>> {
    let rows = sqlx::query(
        "SELECT file, start_line, vector FROM index_embeddings
         WHERE project = ?1 AND model = ?2 AND dim = ?3",
    )
    .bind(project)
    .bind(model)
    .bind(query_vector.len() as i64)
    .fetch_all(pool)
    .await?;

    let mut scored: Vec<(f32, ChunkKey)> = Vec::new();
    for row in rows {
        let Some(vector) = decode_vector(&row.get::<Vec<u8>, _>("vector")) else {
            continue;
        };
        let score = cosine_similarity(query_vector, &vector);
        // Only direction-agreeing chunks are candidates. No tuned relevance
        // floor: thresholds are model-specific, and ranking plus `limit`
        // already decide what the user sees.
        if score > 0.0 {
            scored.push((
                score,
                (
                    row.get::<String, _>("file"),
                    row.get::<i64, _>("start_line"),
                ),
            ));
        }
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit.max(0) as usize);

    let mut hits = Vec::with_capacity(scored.len());
    for (_, (file, start_line)) in scored {
        let snippet = chunk_content(pool, project, &file, start_line)
            .await?
            .map(|content| excerpt(&content, EXCERPT_CHARS))
            .unwrap_or_default();
        hits.push(SearchHit {
            file,
            start_line,
            snippet,
        });
    }
    Ok(hits)
}

/// Merge the legs with reciprocal-rank fusion, most-authoritative first.
///
/// Leg order decides two things and only two. RRF ties keep first-seen order,
/// so the earlier leg wins a tie; and the earlier leg's *snippet* wins a
/// collision, which is why lexical goes first — it carries the »« match
/// markers the file explorer renders, and neither other leg can produce them.
fn fuse(legs: Vec<Vec<SearchHit>>, limit: i64) -> Vec<SearchHit> {
    let rankings: Vec<Vec<ChunkKey>> = legs
        .iter()
        .map(|leg| leg.iter().map(hit_key).collect())
        .collect();

    let mut by_key: HashMap<ChunkKey, SearchHit> = HashMap::new();
    for leg in legs.into_iter().rev() {
        for hit in leg {
            by_key.insert(hit_key(&hit), hit);
        }
    }

    reciprocal_rank_fusion(&rankings, RRF_K)
        .into_iter()
        .take(limit.max(0) as usize)
        .filter_map(|(key, _)| by_key.remove(&key))
        .collect()
}

fn hit_key(hit: &SearchHit) -> ChunkKey {
    (hit.file.clone(), hit.start_line)
}

/// Leading text of a chunk, for hits found by similarity rather than by a
/// matched term — there is nothing to highlight, so nothing is marked up.
fn excerpt(content: &str, max_chars: usize) -> String {
    let mut joined = String::new();
    for line in content.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !joined.is_empty() {
            joined.push(' ');
        }
        joined.push_str(line);
        if joined.chars().count() >= max_chars {
            break;
        }
    }
    if joined.chars().count() > max_chars {
        let head: String = joined.chars().take(max_chars).collect();
        format!("{head} …")
    } else {
        joined
    }
}

/// FTS5 treats many characters as syntax; quote every whitespace-separated
/// term so arbitrary code fragments are safe (implicit AND between terms).
fn sanitize_query(query: &str) -> IndexResult<String> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();
    if terms.is_empty() {
        return Err(IndexError::InvalidInput("empty search query".into()));
    }
    Ok(terms.join(" "))
}

/// Split content into fixed-size line chunks, tracking 1-based start lines.
fn chunk_lines(content: &str, chunk_size: usize) -> Vec<(usize, String)> {
    let lines: Vec<&str> = content.lines().collect();
    lines
        .chunks(chunk_size)
        .enumerate()
        .map(|(i, chunk)| (i * chunk_size + 1, chunk.join("\n")))
        .filter(|(_, text)| !text.trim().is_empty())
        .collect()
}

async fn remove_chunks(pool: &SqlitePool, project: &str, file: &str) -> IndexResult<()> {
    sqlx::query("DELETE FROM index_chunks WHERE project = ?1 AND file = ?2")
        .bind(project)
        .bind(file)
        .execute(pool)
        .await?;
    // A chunk's embedding is meaningless without the chunk, and leaving it
    // behind would let a stale vector outlive the code it described.
    sqlx::query("DELETE FROM index_embeddings WHERE project = ?1 AND file = ?2")
        .bind(project)
        .bind(file)
        .execute(pool)
        .await?;
    Ok(())
}

async fn remove_file(pool: &SqlitePool, project: &str, file: &str) -> IndexResult<()> {
    remove_chunks(pool, project, file).await?;
    // A symbol outliving the file that declared it would keep steering
    // ranking towards a definition that no longer exists.
    remove_symbols(pool, project, file).await?;
    sqlx::query("DELETE FROM index_files WHERE project = ?1 AND file = ?2")
        .bind(project)
        .bind(file)
        .execute(pool)
        .await?;
    Ok(())
}

fn read_text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// True for a symlink, a directory junction, or any other reparse point.
///
/// `FileType::is_symlink()` alone is not enough on Windows: std reports it
/// only for `IO_REPARSE_TAG_SYMLINK`, so a `mklink /J` junction — which any
/// unprivileged user can create, and which a cloned repo can carry — looks
/// like an ordinary directory. The `FILE_ATTRIBUTE_REPARSE_POINT` bit covers
/// every tag.
///
/// Deliberately mirrors `src-tauri/src/pathsafe.rs::is_reparse_point`; this
/// crate is a leaf module and cannot depend on the desktop shell. Both copies
/// are pinned by a test that builds a real junction, so the two cannot drift
/// apart silently.
fn is_reparse_point(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

fn walk_files(root: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, i64, i64)>) {
    if depth > MAX_WALK_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        // `DirEntry::metadata()` does not traverse the link. `path.is_dir()`
        // does, which is how the indexer used to walk out of the project
        // through a junction and land file *contents* from outside the root
        // in `index_chunks`, where `search_code` then fed them to the model
        // with no approval anywhere in the path (SEC-004).
        let Ok(meta) = entry.metadata() else { continue };
        if is_reparse_point(&meta) {
            continue;
        }
        if meta.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                walk_files(root, &path, depth + 1, out);
            }
            continue;
        }
        if meta.len() > MAX_FILE_BYTES {
            continue;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if let Ok(relative) = path.strip_prefix(root) {
            out.push((
                relative.to_string_lossy().replace('\\', "/"),
                mtime,
                meta.len() as i64,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embeddings::{EmbedError, EmbedResult};
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::sync::atomic::{AtomicUsize, Ordering};

    async fn test_pool(dir: &Path) -> SqlitePool {
        let options = SqliteConnectOptions::new()
            .filename(dir.join("index.db"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        init(&pool).await.unwrap();
        pool
    }

    /// Deterministic stand-in for Ollama: each text becomes a unit vector
    /// pointing at whichever keyword it contains, so "similar" is exactly
    /// "shares a keyword" and rankings are reproducible. It also counts
    /// calls, which is how the incremental guarantee is asserted.
    struct KeywordEmbedder {
        keywords: Vec<&'static str>,
        embedded: AtomicUsize,
    }

    impl KeywordEmbedder {
        fn new(keywords: &[&'static str]) -> Self {
            Self {
                keywords: keywords.to_vec(),
                embedded: AtomicUsize::new(0),
            }
        }
        fn count(&self) -> usize {
            self.embedded.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl Embedder for KeywordEmbedder {
        fn model_id(&self) -> &str {
            "keyword-test"
        }
        async fn embed(&self, texts: &[String]) -> EmbedResult<Vec<Vec<f32>>> {
            self.embedded.fetch_add(texts.len(), Ordering::SeqCst);
            Ok(texts
                .iter()
                .map(|text| {
                    let lower = text.to_lowercase();
                    let mut v = vec![0.0f32; self.keywords.len()];
                    for (i, keyword) in self.keywords.iter().enumerate() {
                        if lower.contains(keyword) {
                            v[i] = 1.0;
                        }
                    }
                    // Never all-zero: cosine against a zero vector is 0.
                    if v.iter().all(|x| *x == 0.0) {
                        v[0] = 0.001;
                    }
                    v
                })
                .collect())
        }
    }

    /// Stands in for "Ollama isn't running / model isn't pulled".
    struct DeadEmbedder {
        attempts: AtomicUsize,
    }

    impl DeadEmbedder {
        fn new() -> Self {
            Self {
                attempts: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl Embedder for DeadEmbedder {
        fn model_id(&self) -> &str {
            "unreachable-model"
        }
        async fn embed(&self, _texts: &[String]) -> EmbedResult<Vec<Vec<f32>>> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            Err(EmbedError::Unavailable("connection refused".into()))
        }
    }

    fn write_sample_project(root: &Path) {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/auth.rs"),
            "fn verify_token(token: &str) -> bool {\n    token == \"zebra_secret\"\n}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("notes.md"),
            "# Plans\nrefactor the flamingo module\n",
        )
        .unwrap();
    }

    #[test]
    fn chunking_tracks_start_lines() {
        let content = (1..=120)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let chunks = chunk_lines(&content, 50);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].0, 1);
        assert_eq!(chunks[1].0, 51);
        assert_eq!(chunks[2].0, 101);
        assert!(chunks[2].1.contains("line 120"));
    }

    #[test]
    fn sanitize_handles_code_fragments() {
        assert_eq!(sanitize_query("fn main(").unwrap(), "\"fn\" \"main(\"");
        assert_eq!(
            sanitize_query("say \"hi\"").unwrap(),
            "\"say\" \"\"\"hi\"\"\""
        );
        assert!(sanitize_query("   ").is_err());
    }

    #[test]
    fn excerpt_collapses_blank_lines_and_truncates() {
        let content = "\n\n  fn main() {\n\n    println!(\"hi\");\n}";
        assert_eq!(excerpt(content, 80), "fn main() { println!(\"hi\"); }");
        let long = "x".repeat(500);
        let short = excerpt(&long, 10);
        assert!(short.ends_with('…'));
        assert_eq!(short.chars().count(), 12); // 10 chars + space + ellipsis
    }

    #[test]
    fn fusion_prefers_lexical_snippets_on_collision() {
        let shared = || SearchHit {
            file: "a.rs".into(),
            start_line: 1,
            snippet: String::new(),
        };
        let lexical = vec![SearchHit {
            snippet: "»match«".into(),
            ..shared()
        }];
        let semantic = vec![SearchHit {
            snippet: "plain excerpt".into(),
            ..shared()
        }];
        let symbolic = vec![SearchHit {
            snippet: "declaration excerpt".into(),
            ..shared()
        }];
        let fused = fuse(vec![lexical, symbolic, semantic], 10);
        assert_eq!(
            fused.len(),
            1,
            "the same chunk must not appear once per leg"
        );
        assert_eq!(fused[0].snippet, "»match«");
    }

    #[test]
    fn chunk_start_agrees_with_the_chunker() {
        // Fusion only works because a symbol's line maps onto the exact
        // `start_line` the chunker stored. If these two ever disagree, symbol
        // hits silently become keys that match no chunk.
        let content = (1..=140)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let starts: Vec<i64> = chunk_lines(&content, CHUNK_LINES)
            .iter()
            .map(|(start, _)| *start as i64)
            .collect();
        assert_eq!(starts, vec![1, 51, 101]);
        for line in 1..=140i64 {
            let start = chunk_start_for(line);
            assert!(starts.contains(&start), "line {line} mapped to {start}");
            assert!(start <= line && line < start + CHUNK_LINES as i64);
        }
        // Defensive, not reachable from stored data: lines are 1-based.
        assert_eq!(chunk_start_for(0), 1);
        assert_eq!(chunk_start_for(-7), 1);
    }

    #[test]
    fn identifier_terms_reads_names_out_of_code_shaped_queries() {
        assert_eq!(identifier_terms("verify_token"), vec!["verify_token"]);
        assert_eq!(
            identifier_terms("ProjectTools::execute(arg)"),
            vec!["ProjectTools", "execute", "arg"]
        );
        assert_eq!(
            identifier_terms("store.verify_token(x) // verify_token again"),
            vec!["store", "verify_token", "again"],
            "duplicates collapse and single characters are dropped"
        );
        // Nothing identifier-shaped means the leg never runs at all.
        assert!(identifier_terms("   ").is_empty());
        assert!(identifier_terms("42 + 7 == 9").is_empty());
        assert_eq!(identifier_terms("a b c d e f g h i j").len(), 0);
        assert_eq!(
            identifier_terms("t1 t2 t3 t4 t5 t6 t7 t8 t9 t10").len(),
            MAX_SYMBOL_TERMS,
            "term count is capped"
        );
    }

    #[test]
    fn like_wildcards_in_identifiers_are_escaped() {
        // `_` is a single-character LIKE wildcard and snake_case is full of
        // it: unescaped, `verify_token%` would also match `verifyXtoken`.
        assert_eq!(escape_like("verify_token"), r"verify\_token");
        assert_eq!(escape_like("pct%"), r"pct\%");
        assert_eq!(escape_like(r"back\slash"), r"back\\slash");
        assert_eq!(escape_like("plain"), "plain");
    }

    #[test]
    fn match_tiers_order_exact_before_case_folded_before_prefix() {
        assert_eq!(match_tier("verifyToken", "verifyToken"), 0);
        assert_eq!(match_tier("verifytoken", "verifyToken"), 1);
        assert_eq!(match_tier("verifyTokenLater", "verifyToken"), 2);
    }

    #[tokio::test]
    async fn index_search_update_and_prune() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        write_sample_project(&project);
        std::fs::create_dir_all(project.join("node_modules/x")).unwrap();
        std::fs::write(project.join("node_modules/x/y.js"), "zebra_secret").unwrap();

        let pool = test_pool(dir.path()).await;
        let project_str = project.to_string_lossy().into_owned();

        let s = reindex_project_with(&pool, &project_str, None)
            .await
            .unwrap();
        assert_eq!(s.files, 2, "node_modules must be skipped");
        assert!(s.chunks >= 2);
        assert!(s.indexed_at.is_some());

        let hits = search_with(&pool, &project_str, "zebra_secret", 10, None)
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file, "src/auth.rs");
        assert_eq!(hits[0].start_line, 1);
        assert!(hits[0].snippet.contains("»zebra_secret«"));

        // Unchanged reindex is a no-op; content edits are picked up.
        reindex_project_with(&pool, &project_str, None)
            .await
            .unwrap();
        std::fs::write(
            project.join("src/auth.rs"),
            "fn verify_token(token: &str) -> bool {\n    token == \"walrus_secret\"\n}\n",
        )
        .unwrap();
        // Ensure the mtime differs even on coarse filesystem clocks.
        filetime_touch(&project.join("src/auth.rs"));
        reindex_project_with(&pool, &project_str, None)
            .await
            .unwrap();
        assert!(search_with(&pool, &project_str, "zebra_secret", 10, None)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            search_with(&pool, &project_str, "walrus_secret", 10, None)
                .await
                .unwrap()
                .len(),
            1
        );

        // Deleted files are pruned from the index.
        std::fs::remove_file(project.join("notes.md")).unwrap();
        let s = reindex_project_with(&pool, &project_str, None)
            .await
            .unwrap();
        assert_eq!(s.files, 1);
        assert!(search_with(&pool, &project_str, "flamingo", 10, None)
            .await
            .unwrap()
            .is_empty());
    }

    fn filetime_touch(path: &Path) {
        // Rewriting with different content already changes size; bump the
        // clock too so mtime-equal-but-size-equal collisions can't hide it.
        let content = std::fs::read(path).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(path, content).unwrap();
    }

    /// On Windows a **junction** (`mklink /J`) — the only link an
    /// unprivileged process can create, and therefore the shape a hostile
    /// cloned repo would actually ship. On Unix an ordinary symlink.
    fn make_dir_link(link: &Path, target: &Path) -> bool {
        #[cfg(windows)]
        {
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .map(|out| out.status.success())
                .unwrap_or(false)
        }
        #[cfg(not(windows))]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
    }

    /// SEC-004. The indexer walks a project the user granted; a link inside
    /// it can point anywhere. Following one puts file *contents* from outside
    /// the root into `index_chunks`, and `search_code` — a read-tier,
    /// approval-free tool — then hands those snippets to the model.
    ///
    /// This test fails if the reparse-point check is removed: `path.is_dir()`
    /// happily descends a junction.
    #[tokio::test]
    async fn indexing_does_not_follow_links_out_of_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        write_sample_project(&project);

        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("id_rsa"), "PRIVATE_KEY_pangolin\n").unwrap();

        assert!(
            make_dir_link(&project.join("vendor"), &outside),
            "creating a junction/symlink must not need privileges"
        );
        // Guard: the link really is traversable, so a walker that follows
        // links would in fact reach the secret.
        assert!(project.join("vendor/id_rsa").is_file());

        let pool = test_pool(dir.path()).await;
        let project_str = project.to_string_lossy().into_owned();
        let s = reindex_project_with(&pool, &project_str, None)
            .await
            .unwrap();

        assert_eq!(
            s.files, 2,
            "only the project's own two files may be indexed"
        );
        assert!(
            search_with(&pool, &project_str, "PRIVATE_KEY_pangolin", 10, None)
                .await
                .unwrap()
                .is_empty(),
            "content from outside the project must never reach index_chunks"
        );
        assert!(
            !project_files(&project).iter().any(|f| f.contains("vendor")),
            "the link must not appear in the file list either"
        );
    }

    #[tokio::test]
    async fn stats_empty_project_and_bad_paths() {
        let dir = tempfile::tempdir().unwrap();
        let pool = test_pool(dir.path()).await;
        let s = stats(&pool, "C:/does/not/matter").await.unwrap();
        assert_eq!(s.files, 0);
        assert_eq!(s.indexed_at, None);
        assert!(
            reindex_project_with(&pool, "C:/definitely/missing/dir", None)
                .await
                .is_err()
        );
    }

    /// The hard requirement: no embeddings backend must never cost the user
    /// lexical search, at index time or at query time.
    #[tokio::test]
    async fn search_is_correct_when_embeddings_backend_is_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        write_sample_project(&project);
        let pool = test_pool(dir.path()).await;
        let project_str = project.to_string_lossy().into_owned();
        let dead = DeadEmbedder::new();

        // Indexing succeeds and produces a complete lexical index...
        let s = reindex_project_with(&pool, &project_str, Some(&dead))
            .await
            .unwrap();
        assert_eq!(s.files, 2);
        assert!(s.chunks >= 2);
        assert_eq!(embedded_chunk_count(&pool, &project_str).await.unwrap(), 0);
        // ...after exactly one refused attempt, not one per chunk.
        assert_eq!(dead.attempts.load(Ordering::SeqCst), 1);

        // ...and search returns the same hits it would have without vectors.
        let hits = search_with(&pool, &project_str, "zebra_secret", 10, Some(&dead))
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file, "src/auth.rs");
        assert!(hits[0].snippet.contains("»zebra_secret«"));

        // With nothing stored, search must not even try the backend.
        assert_eq!(dead.attempts.load(Ordering::SeqCst), 1);

        // The query contract is unchanged too.
        assert!(search_with(&pool, &project_str, "   ", 10, Some(&dead))
            .await
            .is_err());
    }

    /// Same guarantee through the real public entry points, which build a
    /// real Ollama client from settings. This pool has no `settings` table
    /// at all — the harshest version of "config is missing".
    #[tokio::test]
    async fn default_entry_points_survive_without_settings_or_ollama() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        write_sample_project(&project);
        let pool = test_pool(dir.path()).await;
        let project_str = project.to_string_lossy().into_owned();

        let s = reindex_project(&pool, &project_str).await.unwrap();
        assert_eq!(s.files, 2);
        let hits = search(&pool, &project_str, "flamingo", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file, "notes.md");
    }

    #[tokio::test]
    async fn unchanged_files_are_never_re_embedded() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        write_sample_project(&project);
        let pool = test_pool(dir.path()).await;
        let project_str = project.to_string_lossy().into_owned();
        let embedder = KeywordEmbedder::new(&["token", "flamingo"]);

        reindex_project_with(&pool, &project_str, Some(&embedder))
            .await
            .unwrap();
        let after_first = embedder.count();
        assert_eq!(after_first, 2, "one embedding per chunk on the first run");
        assert_eq!(embedded_chunk_count(&pool, &project_str).await.unwrap(), 2);

        // Nothing changed on disk: not a single chunk may be re-embedded.
        reindex_project_with(&pool, &project_str, Some(&embedder))
            .await
            .unwrap();
        reindex_project_with(&pool, &project_str, Some(&embedder))
            .await
            .unwrap();
        assert_eq!(embedder.count(), after_first);

        // Touching one file re-embeds that file's chunk and only that one.
        std::fs::write(
            project.join("notes.md"),
            "# Plans\nrefactor the flamingo module today\n",
        )
        .unwrap();
        filetime_touch(&project.join("notes.md"));
        reindex_project_with(&pool, &project_str, Some(&embedder))
            .await
            .unwrap();
        assert_eq!(embedder.count(), after_first + 1);
        assert_eq!(embedded_chunk_count(&pool, &project_str).await.unwrap(), 2);

        // Deleting a file takes its embedding with it.
        std::fs::remove_file(project.join("notes.md")).unwrap();
        reindex_project_with(&pool, &project_str, Some(&embedder))
            .await
            .unwrap();
        assert_eq!(embedded_chunk_count(&pool, &project_str).await.unwrap(), 1);
    }

    /// Embeddings missed while the backend was down are filled in on a later
    /// run — without re-reading or re-embedding anything already stored.
    #[tokio::test]
    async fn a_later_run_backfills_what_a_dead_backend_missed() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        write_sample_project(&project);
        let pool = test_pool(dir.path()).await;
        let project_str = project.to_string_lossy().into_owned();

        reindex_project_with(&pool, &project_str, Some(&DeadEmbedder::new()))
            .await
            .unwrap();
        assert_eq!(embedded_chunk_count(&pool, &project_str).await.unwrap(), 0);

        // Ollama comes up later. No file changed, so the mtime/size pass
        // does nothing — the embedding pass still catches up.
        let embedder = KeywordEmbedder::new(&["token", "flamingo"]);
        reindex_project_with(&pool, &project_str, Some(&embedder))
            .await
            .unwrap();
        assert_eq!(embedded_chunk_count(&pool, &project_str).await.unwrap(), 2);
        assert_eq!(embedder.count(), 2);
    }

    /// The point of the whole feature: a chunk no lexical query would find
    /// still surfaces, and lexical hits are not displaced by it.
    #[tokio::test]
    async fn hybrid_search_fuses_both_rankings() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("login.rs"),
            "fn check_token(t: &str) -> bool { true }\n",
        )
        .unwrap();
        std::fs::write(project.join("birds.md"), "the flamingo stands on one leg\n").unwrap();
        let pool = test_pool(dir.path()).await;
        let project_str = project.to_string_lossy().into_owned();
        let embedder = KeywordEmbedder::new(&["token", "flamingo"]);

        reindex_project_with(&pool, &project_str, Some(&embedder))
            .await
            .unwrap();

        // "flamingo" matches birds.md lexically *and* semantically; it must
        // rank first, and keep its highlighted snippet.
        let hits = search_with(&pool, &project_str, "flamingo", 10, Some(&embedder))
            .await
            .unwrap();
        assert_eq!(hits[0].file, "birds.md");
        assert!(hits[0].snippet.contains("»flamingo«"));

        // "token" appears in no file as a standalone FTS term match for
        // birds.md, so login.rs leads; the vector leg agrees.
        let hits = search_with(&pool, &project_str, "token", 10, Some(&embedder))
            .await
            .unwrap();
        assert_eq!(hits[0].file, "login.rs");

        // Now the payoff: a query whose *words* appear nowhere, but whose
        // embedding points at the token chunk. Lexical search finds nothing.
        assert!(
            search_with(&pool, &project_str, "authorization", 10, None)
                .await
                .unwrap()
                .is_empty(),
            "guard: this query must have no lexical hits"
        );
        let hits = search_with(
            &pool,
            &project_str,
            "authorization token handling",
            10,
            Some(&embedder),
        )
        .await
        .unwrap();
        assert_eq!(hits.len(), 1, "vector-only hit should surface");
        assert_eq!(hits[0].file, "login.rs");
        assert!(
            !hits[0].snippet.is_empty() && !hits[0].snippet.contains('»'),
            "a similarity hit has no matched term to highlight"
        );
    }

    /// A model swap must not mix embedding spaces or resurrect old vectors.
    #[tokio::test]
    async fn switching_models_re_embeds_and_ignores_the_old_space() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        write_sample_project(&project);
        let pool = test_pool(dir.path()).await;
        let project_str = project.to_string_lossy().into_owned();

        struct OtherModel(KeywordEmbedder);
        #[async_trait::async_trait]
        impl Embedder for OtherModel {
            fn model_id(&self) -> &str {
                "other-test-model"
            }
            async fn embed(&self, texts: &[String]) -> EmbedResult<Vec<Vec<f32>>> {
                self.0.embed(texts).await
            }
        }

        reindex_project_with(
            &pool,
            &project_str,
            Some(&KeywordEmbedder::new(&["token", "flamingo"])),
        )
        .await
        .unwrap();
        assert_eq!(embedded_chunk_count(&pool, &project_str).await.unwrap(), 2);

        let other = OtherModel(KeywordEmbedder::new(&["token", "flamingo", "extra"]));
        reindex_project_with(&pool, &project_str, Some(&other))
            .await
            .unwrap();
        // Replaced in place, not accumulated: still one row per chunk.
        assert_eq!(embedded_chunk_count(&pool, &project_str).await.unwrap(), 2);
        assert_eq!(
            other.0.count(),
            2,
            "every chunk re-embedded for the new model"
        );

        // The old model has no rows left, so it degrades to lexical-only.
        let stale = KeywordEmbedder::new(&["token", "flamingo"]);
        let hits = search_with(&pool, &project_str, "flamingo", 10, Some(&stale))
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(stale.count(), 0, "no query embedded against an empty space");
    }

    // ---- symbols ----

    /// Extraction reaching the database, in both supported language families,
    /// and staying in step with the file that produced it.
    #[tokio::test]
    async fn symbols_land_in_the_index_and_follow_their_file() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(
            project.join("src/store.rs"),
            "pub struct TokenStore {\n    inner: u8,\n}\n\n\
             impl TokenStore {\n    pub fn revoke_token(&self) -> bool {\n        true\n    }\n}\n",
        )
        .unwrap();
        std::fs::write(
            project.join("src/Panel.tsx"),
            "export interface PanelProps {\n  title: string;\n}\n\n\
             export class PanelController {\n  refreshPanel() {\n    return 1;\n  }\n}\n\n\
             export const PanelView = ({ title }: PanelProps) => <div>{title}</div>;\n",
        )
        .unwrap();

        let pool = test_pool(dir.path()).await;
        let project_str = project.to_string_lossy().into_owned();
        reindex_project_with(&pool, &project_str, None)
            .await
            .unwrap();

        let rust = file_symbols(&pool, &project_str, "src/store.rs")
            .await
            .unwrap();
        assert_eq!(
            rust,
            vec![
                ("TokenStore".into(), "struct".into(), 1),
                // The method case: a `fn` inside an `impl`, not a top-level one.
                ("revoke_token".into(), "method".into(), 6),
            ]
        );

        let tsx = file_symbols(&pool, &project_str, "src/Panel.tsx")
            .await
            .unwrap();
        assert_eq!(
            tsx,
            vec![
                ("PanelProps".into(), "interface".into(), 1),
                ("PanelController".into(), "class".into(), 5),
                // A method inside a class...
                ("refreshPanel".into(), "method".into(), 6),
                // ...and the arrow-const form React components actually use.
                ("PanelView".into(), "function".into(), 11),
            ]
        );

        // Editing a file replaces its symbols rather than accumulating them.
        std::fs::write(
            project.join("src/store.rs"),
            "pub struct TokenStore {\n    inner: u8,\n}\n\n\
             impl TokenStore {\n    pub fn expire_token(&self) -> bool {\n        true\n    }\n}\n",
        )
        .unwrap();
        filetime_touch(&project.join("src/store.rs"));
        reindex_project_with(&pool, &project_str, None)
            .await
            .unwrap();
        let rust = file_symbols(&pool, &project_str, "src/store.rs")
            .await
            .unwrap();
        assert_eq!(rust.len(), 2, "the old method must not linger: {rust:?}");
        assert!(rust.iter().any(|(name, ..)| name == "expire_token"));

        // Deleting a file takes its symbols with it.
        std::fs::remove_file(project.join("src/Panel.tsx")).unwrap();
        reindex_project_with(&pool, &project_str, None)
            .await
            .unwrap();
        assert!(file_symbols(&pool, &project_str, "src/Panel.tsx")
            .await
            .unwrap()
            .is_empty());
        assert_eq!(symbol_count(&pool, &project_str).await.unwrap(), 2);
    }

    #[tokio::test]
    async fn find_symbols_ranks_exact_before_case_folded_before_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(
            project.join("src/store.rs"),
            "pub struct TokenStore {\n    inner: u8,\n}\n\n\
             impl TokenStore {\n    pub fn verify_token(&self) -> bool {\n        true\n    }\n\
             pub fn verify_token_and_scope(&self) -> bool {\n        true\n    }\n}\n",
        )
        .unwrap();
        std::fs::write(
            project.join("src/legacy.rs"),
            "pub fn Verify_Token() -> bool {\n    true\n}\n",
        )
        .unwrap();

        let pool = test_pool(dir.path()).await;
        let project_str = project.to_string_lossy().into_owned();
        reindex_project_with(&pool, &project_str, None)
            .await
            .unwrap();

        let hits = find_symbols(&pool, &project_str, "verify_token", 10)
            .await
            .unwrap();
        assert_eq!(hits.len(), 3, "{hits:?}");
        assert_eq!(hits[0].name, "verify_token", "exact match ranks first");
        assert_eq!(hits[0].file, "src/store.rs");
        assert_eq!(hits[0].kind, "method");
        assert_eq!(
            hits[1].name, "Verify_Token",
            "case-folded match ranks second"
        );
        assert_eq!(hits[1].kind, "function");
        assert_eq!(
            hits[2].name, "verify_token_and_scope",
            "prefix match ranks last"
        );
    }

    #[tokio::test]
    async fn find_symbols_respects_the_limit_and_ignores_a_query_with_no_identifier() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(
            project.join("src/widgets.rs"),
            "pub fn widget_a() {}\npub fn widget_b() {}\npub fn widget_c() {}\n",
        )
        .unwrap();

        let pool = test_pool(dir.path()).await;
        let project_str = project.to_string_lossy().into_owned();
        reindex_project_with(&pool, &project_str, None)
            .await
            .unwrap();

        assert!(
            find_symbols(&pool, &project_str, "  ", 10)
                .await
                .unwrap()
                .is_empty(),
            "a query with no identifier-shaped token finds nothing rather than everything"
        );

        let hits = find_symbols(&pool, &project_str, "widget", 2)
            .await
            .unwrap();
        assert_eq!(hits.len(), 2, "the limit is respected: {hits:?}");
    }

    /// A project of languages this crate has no grammar for must index and
    /// search exactly as it did before symbols existed.
    #[tokio::test]
    async fn unsupported_extensions_index_normally_with_zero_symbols() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("service.py"),
            "def verify_token(token):\n    return token == 'zebra_secret'\n",
        )
        .unwrap();
        std::fs::write(project.join("notes.md"), "# Plans\nthe flamingo module\n").unwrap();
        std::fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"pangolin\"\n",
        )
        .unwrap();

        let pool = test_pool(dir.path()).await;
        let project_str = project.to_string_lossy().into_owned();
        let stats = reindex_project_with(&pool, &project_str, None)
            .await
            .unwrap();

        assert_eq!(stats.files, 3);
        assert_eq!(
            symbol_count(&pool, &project_str).await.unwrap(),
            0,
            "no grammar means no symbols, not a failure"
        );
        for path in ["service.py", "notes.md", "Cargo.toml"] {
            assert_eq!(symbols::parse_count(&project.join(path)), 0);
        }

        // Lexical retrieval is completely unaffected.
        let hits = search_with(&pool, &project_str, "zebra_secret", 10, None)
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file, "service.py");
        assert!(hits[0].snippet.contains("»zebra_secret«"));
        // Even a query that names the Python function still works — it just
        // gets no structural boost.
        assert_eq!(
            search_with(&pool, &project_str, "verify_token", 10, None)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            search_with(&pool, &project_str, "pangolin", 10, None)
                .await
                .unwrap()[0]
                .file,
            "Cargo.toml"
        );
    }

    /// Genuinely broken source, in both grammars. tree-sitter is handed the
    /// file (asserted, so this cannot pass by silently skipping it) and the
    /// index run must still succeed with the file fully searchable.
    #[tokio::test]
    async fn a_syntactically_broken_file_never_fails_the_index() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("wreck.rs"),
            "fn (((  unterminated_zebra <<< ,,, {{{\n\
             impl impl impl for for {\n\
             struct 42Nonsense }}}\n\
             let ) = ; ; ;\n",
        )
        .unwrap();
        std::fs::write(
            project.join("wreck.tsx"),
            "export function ( { { const = = =>\n\
             class {{{ <div flamingo_marker\n",
        )
        .unwrap();
        std::fs::write(
            project.join("ok.rs"),
            "pub fn healthy_walrus() -> u8 {\n 1\n}\n",
        )
        .unwrap();

        let pool = test_pool(dir.path()).await;
        let project_str = project.to_string_lossy().into_owned();

        let stats = reindex_project_with(&pool, &project_str, None)
            .await
            .expect("a malformed file must not fail the run");
        assert_eq!(stats.files, 3);

        // The parser really was invoked on the broken files — recovery, not
        // an extension check, is what kept the run alive.
        assert_eq!(symbols::parse_count(&project.join("wreck.rs")), 1);
        assert_eq!(symbols::parse_count(&project.join("wreck.tsx")), 1);

        // Both broken files are indexed lexically, exactly as before.
        let hits = search_with(&pool, &project_str, "unterminated_zebra", 10, None)
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file, "wreck.rs");
        assert_eq!(
            search_with(&pool, &project_str, "flamingo_marker", 10, None)
                .await
                .unwrap()[0]
                .file,
            "wreck.tsx"
        );

        // And the healthy file next to them still contributes symbols.
        assert_eq!(
            file_symbols(&pool, &project_str, "ok.rs").await.unwrap(),
            vec![("healthy_walrus".into(), "function".into(), 1)]
        );
    }

    /// The incremental guarantee, measured where it matters: tree-sitter is
    /// handed a file's contents once, and not again until the file changes.
    #[tokio::test]
    async fn unchanged_files_are_never_re_parsed() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        write_sample_project(&project);
        let pool = test_pool(dir.path()).await;
        let project_str = project.to_string_lossy().into_owned();
        let source = project.join("src/auth.rs");
        let markdown = project.join("notes.md");

        reindex_project_with(&pool, &project_str, None)
            .await
            .unwrap();
        assert_eq!(symbols::parse_count(&source), 1);
        assert_eq!(symbols::parse_count(&markdown), 0, "no grammar, no parse");
        assert_eq!(symbol_count(&pool, &project_str).await.unwrap(), 1);

        // Nothing changed on disk: not a single re-parse across two runs.
        reindex_project_with(&pool, &project_str, None)
            .await
            .unwrap();
        reindex_project_with(&pool, &project_str, None)
            .await
            .unwrap();
        assert_eq!(symbols::parse_count(&source), 1);
        assert_eq!(symbols::parse_count(&markdown), 0);

        // Touching the file parses it again — once.
        std::fs::write(
            &source,
            "fn verify_token(token: &str) -> bool {\n    token == \"walrus_secret\"\n}\n",
        )
        .unwrap();
        filetime_touch(&source);
        reindex_project_with(&pool, &project_str, None)
            .await
            .unwrap();
        assert_eq!(symbols::parse_count(&source), 2);
        assert_eq!(symbols::parse_count(&markdown), 0);

        // ...and then stops again.
        reindex_project_with(&pool, &project_str, None)
            .await
            .unwrap();
        assert_eq!(symbols::parse_count(&source), 2);
    }

    /// The payoff, and the only assertion that proves the feature does
    /// anything: bm25 puts the file that *mentions* a name first, and the
    /// symbol leg puts the file that *declares* it first instead.
    #[tokio::test]
    async fn a_declaration_outranks_a_comment_that_mentions_it() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        std::fs::create_dir_all(&project).unwrap();

        // Short, and says the name five times without defining it — exactly
        // the shape bm25 rewards most.
        std::fs::write(
            project.join("mentions.rs"),
            "// verify_token is called from here.\n\
             // see verify_token for the rules.\n\
             fn caller() {\n\
             \x20   // verify_token verify_token verify_token\n\
             }\n",
        )
        .unwrap();

        // Long, and says the name once — in the declaration itself.
        let mut auth = String::from("//! Session verification.\n\n");
        for i in 0..30 {
            auth.push_str(&format!("// housekeeping note {i} about sessions\n"));
        }
        auth.push_str("pub struct Session {\n    id: u64,\n}\n\n");
        auth.push_str("pub fn verify_token(token: &str) -> bool {\n    !token.is_empty()\n}\n");
        std::fs::write(project.join("auth.rs"), &auth).unwrap();

        let pool = test_pool(dir.path()).await;
        let project_str = project.to_string_lossy().into_owned();
        reindex_project_with(&pool, &project_str, None)
            .await
            .unwrap();

        // Guard: without symbols, the comment file genuinely wins. If this
        // assertion ever flips, the test below is proving nothing.
        let lexical = lexical_search(&pool, &project_key(&project_str), "verify_token", 10)
            .await
            .unwrap();
        let lexical_order: Vec<&str> = lexical.iter().map(|h| h.file.as_str()).collect();
        assert_eq!(
            lexical_order,
            vec!["mentions.rs", "auth.rs"],
            "bm25 cannot tell a definition from a mention"
        );

        // With symbols consulted, the declaration comes first — and no
        // embeddings backend was involved in the correction.
        let fused = search_with(&pool, &project_str, "verify_token", 10, None)
            .await
            .unwrap();
        let fused_order: Vec<&str> = fused.iter().map(|h| h.file.as_str()).collect();
        assert_eq!(fused_order, vec!["auth.rs", "mentions.rs"]);

        // The reordering did not cost recall: both files are still returned,
        // and the lexical snippet survives the swap.
        assert!(fused[1].snippet.contains("»verify_token«"));

        // A name nothing declares leaves bm25 order completely alone.
        let untouched = search_with(&pool, &project_str, "housekeeping", 10, None)
            .await
            .unwrap();
        assert_eq!(untouched.len(), 1);
        assert_eq!(untouched[0].file, "auth.rs");
    }

    /// An index written before symbol extraction existed passes the
    /// mtime/size skip on every file, so symbols would never appear. The
    /// stored format version turns that into one extraction pass — and it
    /// must not re-embed anything, because chunks did not move.
    #[tokio::test]
    async fn an_index_predating_symbols_backfills_without_re_embedding() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        write_sample_project(&project);
        let pool = test_pool(dir.path()).await;
        let project_str = project.to_string_lossy().into_owned();
        let project = project_key(&project_str);
        let embedder = KeywordEmbedder::new(&["token", "flamingo"]);

        let before = reindex_project_with(&pool, &project_str, Some(&embedder))
            .await
            .unwrap();
        let symbols_before = symbol_count(&pool, &project_str).await.unwrap();
        assert_eq!(symbols_before, 1);
        assert_eq!(embedder.count(), 2);
        assert_eq!(embedded_chunk_count(&pool, &project_str).await.unwrap(), 2);

        // Roll the index back to what an older build would have left behind:
        // chunks and embeddings intact, no symbols, no version marker.
        sqlx::query("DELETE FROM index_symbols WHERE project = ?1")
            .bind(&project)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM index_meta WHERE project = ?1")
            .bind(&project)
            .execute(&pool)
            .await
            .unwrap();

        let after = reindex_project_with(&pool, &project_str, Some(&embedder))
            .await
            .unwrap();

        assert_eq!(
            symbol_count(&pool, &project_str).await.unwrap(),
            symbols_before,
            "the backfill must reach files the mtime/size pass skipped"
        );
        // The expensive halves were left alone: same chunks, same vectors,
        // and not one extra call to the embedder.
        assert_eq!(after.chunks, before.chunks);
        assert_eq!(after.files, before.files);
        assert_eq!(embedder.count(), 2, "nothing may be re-embedded");
        assert_eq!(embedded_chunk_count(&pool, &project_str).await.unwrap(), 2);

        // And it happens exactly once: the marker is now current.
        let parses = symbols::parse_count(&PathBuf::from(&project_str).join("src/auth.rs"));
        reindex_project_with(&pool, &project_str, Some(&embedder))
            .await
            .unwrap();
        assert_eq!(
            symbols::parse_count(&PathBuf::from(&project_str).join("src/auth.rs")),
            parses,
            "a second run must not re-parse anything"
        );
        assert_eq!(
            symbols_version(&pool, &project).await,
            Some(SYMBOLS_VERSION)
        );
    }
}
