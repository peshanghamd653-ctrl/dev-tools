//! Project-index IPC commands. Reindexing runs as a kernel job so progress
//! and completion flow through the standard `jobUpdated` events.

use devos_index::IndexStats;
use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub async fn index_project(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let pool = state.kernel.pool.clone();
    state
        .kernel
        .jobs
        .submit("index", "reindex", async move {
            devos_index::reindex_project(&pool, &path)
                .await
                .map(|s| serde_json::json!({ "files": s.files, "chunks": s.chunks }))
                .map_err(|e| e.to_string())
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
