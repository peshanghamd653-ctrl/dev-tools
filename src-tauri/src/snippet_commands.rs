//! Snippet-library IPC commands. `snippet_save` is one command for both
//! insert and update — the draft's `id` decides which.

use devos_snippets::{Snippet, SnippetDraft};
use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub async fn snippets_list(state: State<'_, AppState>) -> Result<Vec<Snippet>, String> {
    devos_snippets::list_snippets(&state.kernel.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn snippets_search(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<Snippet>, String> {
    devos_snippets::search_snippets(&state.kernel.pool, &query)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn snippet_save(
    state: State<'_, AppState>,
    draft: SnippetDraft,
) -> Result<Snippet, String> {
    devos_snippets::save_snippet(&state.kernel.pool, &draft)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn snippet_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    devos_snippets::delete_snippet(&state.kernel.pool, &id)
        .await
        .map_err(|e| e.to_string())
}
