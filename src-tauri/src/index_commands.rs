//! Project-index IPC commands. Reindexing runs as a kernel job so progress
//! and completion flow through the standard `jobUpdated` events.

use devos_index::IndexStats;
use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub async fn index_project(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let kernel = state.kernel.clone();
    state
        .kernel
        .jobs
        .submit("index", "reindex", async move {
            match devos_index::reindex_project(&kernel.pool, &path).await {
                Ok(stats) => {
                    let _ = kernel
                        .notify(
                            "index",
                            "info",
                            "Project index updated",
                            Some(&format!("{} files, {} chunks", stats.files, stats.chunks)),
                        )
                        .await;
                    Ok(serde_json::json!({ "files": stats.files, "chunks": stats.chunks }))
                }
                Err(e) => Err(e.to_string()),
            }
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn index_stats(state: State<'_, AppState>, path: String) -> Result<IndexStats, String> {
    devos_index::stats(&state.kernel.pool, &path)
        .await
        .map_err(|e| e.to_string())
}

/// Content search over the FTS index, for the file explorer's search panel.
#[tauri::command]
pub async fn index_search(
    state: State<'_, AppState>,
    path: String,
    query: String,
) -> Result<Vec<devos_index::IndexHit>, String> {
    let hits = devos_index::search(&state.kernel.pool, &path, &query, 30)
        .await
        .map_err(|e| e.to_string())?;
    Ok(hits
        .into_iter()
        .map(|h| devos_index::IndexHit {
            file: h.file,
            start_line: h.start_line,
            snippet: h.snippet,
        })
        .collect())
}

/// "Go to symbol": every declaration matching `query`, ranked exact before
/// case-folded before prefix. For the command palette's symbol picker, not
/// the general search panel — see `devos_index::find_symbols`.
#[tauri::command]
pub async fn index_find_symbols(
    state: State<'_, AppState>,
    path: String,
    query: String,
) -> Result<Vec<devos_index::SymbolHit>, String> {
    devos_index::find_symbols(&state.kernel.pool, &path, &query, 30)
        .await
        .map_err(|e| e.to_string())
}
