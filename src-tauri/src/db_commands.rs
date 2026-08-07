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

/// Run one statement against a saved connection.
///
/// A write that actually executes is audited; a read is not, and neither is a
/// write the guards refused. The reasoning, since both halves are choices:
///
/// - **Reads are omitted** because they are the volume, they change nothing,
///   and a row per `SELECT` would be a log of the user browsing their own
///   data — which the connection list already records the existence of.
/// - **Refusals are omitted** because nothing happened. The refusal is
///   surfaced in the editor at the time, and a guard doing its job every time
///   somebody forgets the write toggle is noise, not history.
/// - **The statement is not recorded**, only the connection and how many rows
///   moved. A statement is where the *data* lives — `INSERT INTO users VALUES
///   ('…')` carries whatever the user's database carries — and this table is
///   plaintext beside the encrypted one. "A write ran against Local notes and
///   moved 400 rows, at 14:02" is the fact worth keeping; reconstructing it
///   verbatim is the SQL editor's own history's job, not the security log's.
#[tauri::command]
pub async fn db_query(
    state: State<'_, AppState>,
    id: String,
    sql: String,
    allow_write: bool,
) -> Result<QueryResult, DbErrorDto> {
    let (connection, read_pool, write_pool) = state
        .db
        .resolve(&state.kernel.pool, &id)
        .await
        .map_err(DbErrorDto::from)?;
    let result = devos_db::run_query(&read_pool, &write_pool, &sql, allow_write)
        .await
        .map_err(DbErrorDto::from)?;
    // `read_only` is set by the executor from the path the statement actually
    // took, not from the caller's `allow_write` flag — so this records writes
    // that ran, and cannot be talked into recording one that did not.
    if !result.read_only {
        state
            .kernel
            .audit(devos_kernel::audit::AuditEvent::SqlWrite {
                connection: connection.name.clone(),
                rows_affected: result.rows_affected,
            })
            .await;
    }
    Ok(result)
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
