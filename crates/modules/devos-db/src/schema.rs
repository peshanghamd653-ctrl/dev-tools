//! Schema introspection: what tables and views exist, their columns, and how
//! many rows each holds.

use std::path::Path;

use sqlx::{Row, SqlitePool};

use crate::{DbColumn, DbResult, DbSchema, DbTable};

/// Wrap an identifier in double quotes, doubling any embedded quote.
///
/// Table names come from `sqlite_master`, but they still reach SQL through
/// string formatting (identifiers can't be bound as parameters), so a name
/// like `weird"name` must not be able to terminate the quoted region.
pub(crate) fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub async fn read_schema(pool: &SqlitePool, path: &Path) -> DbResult<DbSchema> {
    // `sqlite_%` is SQLite's reserved namespace (sqlite_sequence, the FTS
    // shadow tables, …); those are plumbing, not the user's schema. The
    // ESCAPE clause keeps `_` from matching any character.
    let entries = sqlx::query(
        "SELECT name, type FROM sqlite_master
         WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
         ORDER BY type, name",
    )
    .fetch_all(pool)
    .await?;

    let mut tables = Vec::with_capacity(entries.len());
    for entry in &entries {
        let name: String = entry.get("name");
        let kind: String = entry.get("type");
        tables.push(DbTable {
            columns: read_columns(pool, &name).await,
            row_count: count_rows(pool, &name).await,
            name,
            kind,
        });
    }

    let size_bytes = std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0);
    Ok(DbSchema { tables, size_bytes })
}

/// Empty on failure for the same reason [`count_rows`] returns `-1`: a view
/// over a dropped table can't be described, and one broken object must not
/// make the rest of the schema unreadable.
async fn read_columns(pool: &SqlitePool, table: &str) -> Vec<DbColumn> {
    let Ok(rows) = sqlx::query(&format!("PRAGMA table_info({})", quote_ident(table)))
        .fetch_all(pool)
        .await
    else {
        return Vec::new();
    };
    rows.iter()
        .map(|row| DbColumn {
            name: row.get("name"),
            data_type: row.get("type"),
            not_null: row.get::<i64, _>("notnull") != 0,
            primary_key: row.get::<i64, _>("pk") != 0,
            // Defaults are reported as the source text of the expression, but
            // be lenient: a non-text literal shouldn't sink the whole call.
            default_value: row.try_get("dflt_value").unwrap_or_default(),
        })
        .collect()
}

/// `-1` on failure rather than an error, for the same reason.
async fn count_rows(pool: &SqlitePool, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {}", quote_ident(table)))
        .fetch_one(pool)
        .await
        .unwrap_or(-1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    async fn fixture(dir: &tempfile::TempDir) -> (std::path::PathBuf, SqlitePool) {
        let path = dir.path().join("app.sqlite");
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        for statement in [
            "CREATE TABLE users (
                id    INTEGER PRIMARY KEY,
                email TEXT NOT NULL,
                role  TEXT DEFAULT 'member'
            )",
            "CREATE TABLE notes (id INTEGER, body TEXT)",
            "CREATE VIEW admins AS SELECT * FROM users WHERE role = 'admin'",
            "INSERT INTO users (email, role) VALUES ('a@x', 'admin'), ('b@x', 'member')",
            "INSERT INTO notes (id, body) VALUES (1, 'hi')",
            // Forces SQLite to materialize its internal sqlite_sequence table.
            "CREATE TABLE seqd (id INTEGER PRIMARY KEY AUTOINCREMENT, v TEXT)",
            "INSERT INTO seqd (v) VALUES ('x')",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        (path, pool)
    }

    #[test]
    fn quotes_identifiers_by_doubling_embedded_quotes() {
        assert_eq!(quote_ident("users"), "\"users\"");
        assert_eq!(quote_ident("weird\"name"), "\"weird\"\"name\"");
        assert_eq!(
            quote_ident("x\"; DROP TABLE users; --"),
            "\"x\"\"; DROP TABLE users; --\""
        );
    }

    #[tokio::test]
    async fn introspects_tables_views_columns_and_counts() {
        let dir = tempfile::tempdir().unwrap();
        let (path, pool) = fixture(&dir).await;

        let schema = read_schema(&pool, &path).await.unwrap();
        let names: Vec<&str> = schema.tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["notes", "seqd", "users", "admins"]);
        assert!(
            !names.iter().any(|n| n.starts_with("sqlite_")),
            "internal tables hidden: {names:?}"
        );
        assert!(schema.size_bytes > 0, "size read from the file itself");

        let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
        assert_eq!(users.kind, "table");
        assert_eq!(users.row_count, 2);
        assert_eq!(users.columns.len(), 3);

        let id = &users.columns[0];
        assert_eq!(id.name, "id");
        assert_eq!(id.data_type, "INTEGER");
        assert!(id.primary_key);
        assert!(!id.not_null, "an INTEGER PRIMARY KEY is nullable in SQLite");

        let email = &users.columns[1];
        assert!(email.not_null);
        assert!(!email.primary_key);
        assert_eq!(email.default_value, None);

        let role = &users.columns[2];
        assert_eq!(role.default_value.as_deref(), Some("'member'"));

        let admins = schema.tables.iter().find(|t| t.name == "admins").unwrap();
        assert_eq!(admins.kind, "view");
        assert_eq!(admins.row_count, 1, "views are counted too");
        assert_eq!(admins.columns.len(), 3, "views report their columns");
    }

    #[tokio::test]
    async fn broken_view_reports_minus_one_instead_of_failing() {
        let dir = tempfile::tempdir().unwrap();
        let (path, pool) = fixture(&dir).await;
        sqlx::query("CREATE VIEW orphan AS SELECT * FROM gone")
            .execute(&pool)
            .await
            .unwrap();

        let schema = read_schema(&pool, &path).await.expect("schema still reads");
        let orphan = schema.tables.iter().find(|t| t.name == "orphan").unwrap();
        assert_eq!(orphan.row_count, -1);
        assert!(
            orphan.columns.is_empty(),
            "undescribable view has no columns"
        );
        let users = schema.tables.iter().find(|t| t.name == "users").unwrap();
        assert_eq!(users.row_count, 2, "healthy tables are unaffected");
    }

    #[tokio::test]
    async fn handles_a_table_whose_name_contains_a_quote() {
        let dir = tempfile::tempdir().unwrap();
        let (path, pool) = fixture(&dir).await;
        sqlx::query("CREATE TABLE \"weird\"\"name\" (a TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO \"weird\"\"name\" (a) VALUES ('v')")
            .execute(&pool)
            .await
            .unwrap();

        let schema = read_schema(&pool, &path).await.unwrap();
        let weird = schema
            .tables
            .iter()
            .find(|t| t.name == "weird\"name")
            .expect("quoted table listed");
        assert_eq!(weird.row_count, 1);
        assert_eq!(weird.columns.len(), 1);
    }
}
