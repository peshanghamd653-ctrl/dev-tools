//! Incremental FTS5 indexing and search.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sqlx::{Row, SqlitePool};

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
/// (same mtime + size) are skipped; deleted files are pruned.
pub async fn reindex_project(pool: &SqlitePool, project_path: &str) -> IndexResult<IndexStats> {
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

    let result = stats(pool, project_path).await?;
    tracing::info!(project = %project, changed, files = result.files, chunks = result.chunks, "index updated");
    Ok(result)
}

pub async fn search(
    pool: &SqlitePool,
    project_path: &str,
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
    .bind(project_key(project_path))
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
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

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

    #[tokio::test]
    async fn index_search_update_and_prune() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(
            project.join("src/auth.rs"),
            "fn verify_token(token: &str) -> bool {\n    token == \"zebra_secret\"\n}\n",
        )
        .unwrap();
        std::fs::write(
            project.join("notes.md"),
            "# Plans\nrefactor the flamingo module\n",
        )
        .unwrap();
        std::fs::create_dir_all(project.join("node_modules/x")).unwrap();
        std::fs::write(project.join("node_modules/x/y.js"), "zebra_secret").unwrap();

        let pool = test_pool(dir.path()).await;
        let project_str = project.to_string_lossy().into_owned();

        let s = reindex_project(&pool, &project_str).await.unwrap();
        assert_eq!(s.files, 2, "node_modules must be skipped");
        assert!(s.chunks >= 2);
        assert!(s.indexed_at.is_some());

        let hits = search(&pool, &project_str, "zebra_secret", 10)
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file, "src/auth.rs");
        assert_eq!(hits[0].start_line, 1);
        assert!(hits[0].snippet.contains("»zebra_secret«"));

        // Unchanged reindex is a no-op; content edits are picked up.
        reindex_project(&pool, &project_str).await.unwrap();
        std::fs::write(
            project.join("src/auth.rs"),
            "fn verify_token(token: &str) -> bool {\n    token == \"walrus_secret\"\n}\n",
        )
        .unwrap();
        // Ensure the mtime differs even on coarse filesystem clocks.
        filetime_touch(&project.join("src/auth.rs"));
        reindex_project(&pool, &project_str).await.unwrap();
        assert!(search(&pool, &project_str, "zebra_secret", 10)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            search(&pool, &project_str, "walrus_secret", 10)
                .await
                .unwrap()
                .len(),
            1
        );

        // Deleted files are pruned from the index.
        std::fs::remove_file(project.join("notes.md")).unwrap();
        let s = reindex_project(&pool, &project_str).await.unwrap();
        assert_eq!(s.files, 1);
        assert!(search(&pool, &project_str, "flamingo", 10)
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
        assert!(reindex_project(&pool, "C:/definitely/missing/dir")
            .await
            .is_err());
    }
}
