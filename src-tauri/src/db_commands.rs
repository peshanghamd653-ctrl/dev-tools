//! Database-manager IPC commands. Saved connections live in the DevOS DB;
//! the user's own databases are opened lazily through the shared pool cache.
//!
//! These are the one family of commands that does **not** flatten its error to
//! a string. The database page renders a refused write differently from any
//! other failure, and `.map_err(|e| e.to_string())` left it recovering that
//! distinction by matching words in the message. `DbErrorDto` carries a `kind`
//! instead; see `devos_db::error`.

use std::path::Path;

use devos_db::{DbConnection, DbErrorDto, DbSchema, QueryResult};
use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub async fn db_connections(state: State<'_, AppState>) -> Result<Vec<DbConnection>, DbErrorDto> {
    devos_db::list_connections(&state.kernel.pool)
        .await
        .map_err(DbErrorDto::from)
}

#[tauri::command]
pub async fn db_connect(
    state: State<'_, AppState>,
    name: String,
    path: String,
) -> Result<DbConnection, DbErrorDto> {
    devos_db::save_connection(&state.kernel.pool, &name, &path)
        .await
        .map_err(DbErrorDto::from)
}

#[tauri::command]
pub async fn db_connection_delete(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), DbErrorDto> {
    devos_db::delete_connection(&state.kernel.pool, &id)
        .await
        .map_err(DbErrorDto::from)?;
    state.db.evict(&id);
    Ok(())
}

#[tauri::command]
pub async fn db_schema(state: State<'_, AppState>, id: String) -> Result<DbSchema, DbErrorDto> {
    let (connection, read_pool, _) = state
        .db
        .resolve(&state.kernel.pool, &id)
        .await
        .map_err(DbErrorDto::from)?;
    devos_db::read_schema(&read_pool, Path::new(&connection.path))
        .await
        .map_err(DbErrorDto::from)
}

#[tauri::command]
pub async fn db_query(
    state: State<'_, AppState>,
    id: String,
    sql: String,
    allow_write: bool,
) -> Result<QueryResult, DbErrorDto> {
    let (_, read_pool, write_pool) = state
        .db
        .resolve(&state.kernel.pool, &id)
        .await
        .map_err(DbErrorDto::from)?;
    devos_db::run_query(&read_pool, &write_pool, &sql, allow_write)
        .await
        .map_err(DbErrorDto::from)
}

#[tauri::command]
pub async fn db_table_rows(
    state: State<'_, AppState>,
    id: String,
    table: String,
    limit: i64,
) -> Result<QueryResult, DbErrorDto> {
    let (_, read_pool, _) = state
        .db
        .resolve(&state.kernel.pool, &id)
        .await
        .map_err(DbErrorDto::from)?;
    devos_db::table_rows(&read_pool, &table, limit)
        .await
        .map_err(DbErrorDto::from)
}
