//! Incremental FTS5 indexing plus the vector half: chunk embeddings stored
//! as BLOBs, brute-force cosine at query time, and reciprocal-rank fusion of
//! the two rankings behind the unchanged `search` entry point.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::{Row, SqlitePool};

use crate::embeddings::{
    Embedder, OllamaEmbedder, DEFAULT_EMBED_MODEL, DEFAULT_OLLAMA_URL, INDEX_TIMEOUT, QUERY_TIMEOUT,
};
use crate::vector::{
    cosine_similarity, decode_vector, encode_vector, reciprocal_rank_fusion, RRF_K,
};
use crate::IndexStats;

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

    let mut changed = 0usize;
    for (file, mtime, size) in &disk {
        let unchanged = indexed
            .remove(file)
            .is_some_and(|(m, s)| m == *mtime && s == *size);
        if unchanged {
            continue;
        }
        let Some(content) = read_text(&root.join(file)) else {
            // Binary/unreadable: make sure nothing stale remains.
            remove_file(pool, &project, file).await?;
            continue;
        };
        remove_chunks(pool, &project, file).await?;
        for (start_line, chunk) in chunk_lines(&content, CHUNK_LINES) {
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

    let result = stats(pool, project_path).await?;
    tracing::info!(project = %project, changed, embedded, files = result.files, chunks = result.chunks, "index updated");
    Ok(result)
}

/// Hybrid content search: bm25-ranked FTS5 hits fused with vector-similarity
/// hits. Falls back to lexical-only results whenever embeddings are
/// unavailable, which is the common case.
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
    let Some(embedder) = embedder else {
        return Ok(lexical);
    };
    let semantic = semantic_search(pool, &project, query, limit, embedder).await;
    if semantic.is_empty() {
        return Ok(lexical);
    }
    Ok(fuse(lexical, semantic, limit))
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

/// Merge the two rankings with reciprocal-rank fusion.
///
/// Lexical snippets win on collision: they carry the »« match markers the
/// file explorer renders, which a vector hit has no way to produce.
fn fuse(lexical: Vec<SearchHit>, semantic: Vec<SearchHit>, limit: i64) -> Vec<SearchHit> {
    let lexical_ranking: Vec<ChunkKey> = lexical.iter().map(hit_key).collect();
    let semantic_ranking: Vec<ChunkKey> = semantic.iter().map(hit_key).collect();

    let mut by_key: HashMap<ChunkKey, SearchHit> = HashMap::new();
    for hit in semantic {
        by_key.insert(hit_key(&hit), hit);
    }
    for hit in lexical {
        by_key.insert(hit_key(&hit), hit);
    }

    reciprocal_rank_fusion(&[lexical_ranking, semantic_ranking], RRF_K)
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
        if path.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                walk_files(root, &path, depth + 1, out);
            }
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
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
        let fused = fuse(lexical, semantic, 10);
        assert_eq!(fused.len(), 1, "the same chunk must not appear twice");
        assert_eq!(fused[0].snippet, "»match«");
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
}
