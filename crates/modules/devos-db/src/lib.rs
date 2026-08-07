//! Database manager module: saved connections, schema introspection, and
//! ad-hoc query execution against a user's own databases.
//!
//! SQLite only for this increment. [`DbConnection::driver`] already carries
//! the engine name so Postgres/MySQL can slot in behind the same DTOs later
//! without a frontend change; the extra sqlx driver features aren't worth
//! their compile cost until that work actually starts.

mod error;
mod manager;
mod query;
mod repo;
mod schema;

pub use error::{DbError, DbErrorDto, DbErrorKind, DbResult};
pub use manager::DbManager;
pub use query::{run_query, table_rows, MAX_ROWS};
pub use repo::{delete_connection, get_connection, init, list_connections, save_connection};
pub use schema::read_schema;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use devos_kernel::module::{Module, ModuleCtx};
use devos_kernel::types::CommandDescriptor;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DbConnection {
    pub id: String,
    pub name: String,
    // Engine name; always "sqlite" today. Kept on the record so a second
    // driver doesn't change the DTO the frontend is already built against.
    pub driver: String,
    // Canonicalized absolute path to the database file.
    pub path: String,
    #[ts(type = "number")]
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DbColumn {
    pub name: String,
    pub data_type: String,
    pub not_null: bool,
    pub primary_key: bool,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DbTable {
    pub name: String,
    // "table" | "view"
    pub kind: String,
    // `-1` when the count could not be taken (a view over a missing table).
    #[ts(type = "number")]
    pub row_count: i64,
    pub columns: Vec<DbColumn>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DbSchema {
    pub tables: Vec<DbTable>,
    #[ts(type = "number")]
    pub size_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    pub columns: Vec<String>,
    // Every cell rendered to text; `None` is SQL NULL, which is distinct
    // from the string "NULL" a naive renderer would produce.
    pub rows: Vec<Vec<Option<String>>>,
    #[ts(type = "number")]
    pub row_count: i64,
    #[ts(type = "number")]
    pub rows_affected: i64,
    #[ts(type = "number")]
    pub duration_ms: i64,
    pub truncated: bool,
    pub read_only: bool,
}

pub struct DbModule;

impl Module for DbModule {
    fn id(&self) -> &'static str {
        "db"
    }

    fn register(&self, ctx: &ModuleCtx<'_>) {
        ctx.commands.register(vec![CommandDescriptor {
            id: "db.open".into(),
            module: "db".into(),
            title: "Open Database".into(),
            keywords: vec![
                "sql".into(),
                "sqlite".into(),
                "schema".into(),
                "query".into(),
            ],
            shortcut: Some("Ctrl+9".into()),
        }]);
    }
}
