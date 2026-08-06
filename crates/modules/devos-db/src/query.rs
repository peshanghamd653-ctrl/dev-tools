//! Query execution.
//!
//! Two independent guards keep a read from turning into a write:
//! classification of the statement text, and `PRAGMA query_only` on the
//! connection that actually runs it. The first produces a good error message;
//! the second is what makes the guarantee, because SQL is easier to disguise
//! than to parse (`WITH x AS (…) INSERT …` starts with a read keyword).

use std::time::Instant;

use futures_util::TryStreamExt;
use sqlx::sqlite::SqliteRow;
use sqlx::{Column, Executor, Row, SqlitePool, TypeInfo, ValueRef};

use crate::schema::quote_ident;
use crate::{DbError, DbResult, QueryResult};

/// Hard ceiling on rows handed to the UI. A grid can't usefully show more,
/// and the streaming reader stops here instead of buffering a whole table.
pub const MAX_ROWS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatementKind {
    Read,
    Write,
}

/// Drop leading `--` line comments, `/* */` block comments, and whitespace so
/// classification sees the statement's real first keyword.
fn strip_leading_noise(sql: &str) -> &str {
    let mut rest = sql.trim_start();
    loop {
        if let Some(after) = rest.strip_prefix("--") {
            rest = match after.find('\n') {
                Some(end) => &after[end + 1..],
                None => "",
            };
        } else if let Some(after) = rest.strip_prefix("/*") {
            rest = match after.find("*/") {
                Some(end) => &after[end + 2..],
                None => "",
            };
        } else {
            return rest;
        }
        rest = rest.trim_start();
    }
}

/// The statement's leading keyword, uppercased. Empty when there isn't one.
fn leading_keyword(sql: &str) -> String {
    strip_leading_noise(sql)
        .split(|c: char| c.is_whitespace() || c == '(' || c == ';')
        .find(|token| !token.is_empty())
        .unwrap_or_default()
        .to_uppercase()
}

fn classify(sql: &str) -> StatementKind {
    let rest = strip_leading_noise(sql);
    match leading_keyword(rest).as_str() {
        // EXPLAIN never executes the statement it explains.
        "SELECT" | "WITH" | "EXPLAIN" => StatementKind::Read,
        // `PRAGMA foo` and `PRAGMA foo(arg)` interrogate; `PRAGMA foo = bar`
        // sets. Treat the setting form as a write and let the user opt in.
        "PRAGMA" if !rest.contains('=') => StatementKind::Read,
        _ => StatementKind::Write,
    }
}

/// Run `sql` under `PRAGMA query_only`, capping the result at [`MAX_ROWS`].
async fn fetch_read_only(
    pool: &SqlitePool,
    sql: &str,
    limit: Option<i64>,
) -> DbResult<(Vec<String>, Vec<Vec<Option<String>>>, bool)> {
    let mut conn = pool.acquire().await?;
    sqlx::query("PRAGMA query_only = ON")
        .execute(&mut *conn)
        .await?;

    let mut query = sqlx::query(sql);
    if let Some(limit) = limit {
        query = query.bind(limit);
    }

    let mut fetched: Vec<SqliteRow> = Vec::new();
    let mut truncated = false;
    let mut failure: Option<sqlx::Error> = None;
    {
        let mut stream = query.fetch(&mut *conn);
        while let Some(next) = stream.try_next().await.transpose() {
            match next {
                Ok(row) => {
                    if fetched.len() == MAX_ROWS {
                        truncated = true;
                        break;
                    }
                    fetched.push(row);
                }
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            }
        }
    }

    // Reset before the connection goes back to the pool, whatever happened.
    let reset = sqlx::query("PRAGMA query_only = OFF")
        .execute(&mut *conn)
        .await;
    if let Some(e) = failure {
        return Err(e.into());
    }
    reset?;

    let columns = match fetched.first() {
        Some(row) => row
            .columns()
            .iter()
            .map(|column| column.name().to_string())
            .collect(),
        // An empty result still has a shape; ask SQLite for it so the grid can
        // render headers over zero rows.
        None => conn
            .describe(sql)
            .await
            .map(|described| {
                described
                    .columns()
                    .iter()
                    .map(|column| column.name().to_string())
                    .collect()
            })
            .unwrap_or_default(),
    };

    let rows = fetched
        .iter()
        .map(|row| (0..row.columns().len()).map(|i| cell(row, i)).collect())
        .collect();
    Ok((columns, rows, truncated))
}

/// Render one cell as text.
///
/// SQLite is dynamically typed, so what matters is the *value's* runtime type,
/// not the column's declared type — the latter is absent for expressions and
/// views, and decoding everything as text silently drops numbers and blobs.
fn cell(row: &SqliteRow, index: usize) -> Option<String> {
    let raw = row.try_get_raw(index).ok()?;
    if raw.is_null() {
        return None;
    }
    match raw.type_info().name() {
        "INTEGER" => row.try_get::<i64, _>(index).ok().map(|v| v.to_string()),
        "REAL" => row.try_get::<f64, _>(index).ok().map(|v| v.to_string()),
        // Binary is never inlined into the grid; the UI shows a size instead.
        "BLOB" => row
            .try_get::<Vec<u8>, _>(index)
            .ok()
            .map(|bytes| format!("<{} bytes>", bytes.len())),
        _ => row.try_get::<String, _>(index).ok(),
    }
}

/// Execute one statement. Writes require `allow_write`, which the UI gates
/// behind an explicit toggle — the same two-tier consent the AI tools use.
pub async fn run_query(pool: &SqlitePool, sql: &str, allow_write: bool) -> DbResult<QueryResult> {
    let sql = sql.trim();
    let keyword = leading_keyword(sql);
    if keyword.is_empty() {
        return Err(DbError::Invalid("query is empty".into()));
    }
    let kind = classify(sql);
    if kind == StatementKind::Write && !allow_write {
        return Err(DbError::WriteBlocked(format!(
            "{keyword} modifies the database; enable writes to run it"
        )));
    }

    let started = Instant::now();
    match kind {
        StatementKind::Read => {
            let (columns, rows, truncated) = fetch_read_only(pool, sql, None).await?;
            Ok(QueryResult {
                row_count: rows.len() as i64,
                columns,
                rows,
                rows_affected: 0,
                duration_ms: started.elapsed().as_millis() as i64,
                truncated,
                read_only: true,
            })
        }
        StatementKind::Write => {
            let result = sqlx::query(sql).execute(pool).await?;
            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                row_count: 0,
                rows_affected: result.rows_affected() as i64,
                duration_ms: started.elapsed().as_millis() as i64,
                truncated: false,
                read_only: false,
            })
        }
    }
}

/// First `limit` rows of a table, always through the read-only path.
pub async fn table_rows(pool: &SqlitePool, table: &str, limit: i64) -> DbResult<QueryResult> {
    let table = table.trim();
    if table.is_empty() {
        return Err(DbError::Invalid("table name is empty".into()));
    }
    let sql = format!("SELECT * FROM {} LIMIT ?", quote_ident(table));
    let started = Instant::now();
    let (columns, rows, truncated) = fetch_read_only(pool, &sql, Some(limit.max(1))).await?;
    Ok(QueryResult {
        row_count: rows.len() as i64,
        columns,
        rows,
        rows_affected: 0,
        duration_ms: started.elapsed().as_millis() as i64,
        truncated,
        read_only: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    async fn fixture() -> (tempfile::TempDir, SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("app.sqlite"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        for statement in [
            "CREATE TABLE mixed (n INTEGER, r REAL, s TEXT, b BLOB, z TEXT)",
            "INSERT INTO mixed (n, r, s, b, z) VALUES (42, 3.5, 'hello', x'00ff10', NULL)",
            "CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT NOT NULL)",
            "INSERT INTO users (email) VALUES ('a@x'), ('b@x')",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        (dir, pool)
    }

    #[test]
    fn classifies_past_comments_and_case() {
        for sql in [
            "select 1",
            "  SELECT 1",
            "-- a note\nSELECT 1",
            "-- one\n-- two\n  select 1",
            "/* block */ SELECT 1",
            "/* multi\nline */\n\tselect 1",
            "--only line comment\n/* then block */ WITH t AS (SELECT 1) SELECT * FROM t",
            "EXPLAIN DELETE FROM users",
            "PRAGMA table_info(users)",
        ] {
            assert_eq!(classify(sql), StatementKind::Read, "should read: {sql:?}");
        }

        for sql in [
            "INSERT INTO users (email) VALUES ('c@x')",
            "-- sneaky\nDELETE FROM users",
            "/* c */ update users set email = 'x'",
            "DROP TABLE users",
            "PRAGMA journal_mode = WAL",
        ] {
            assert_eq!(classify(sql), StatementKind::Write, "should write: {sql:?}");
        }

        assert_eq!(leading_keyword("-- just a comment"), "");
        assert_eq!(leading_keyword("/* unterminated"), "");
    }

    #[tokio::test]
    async fn select_renders_each_sqlite_type() {
        let (_dir, pool) = fixture().await;
        let result = run_query(&pool, "SELECT n, r, s, b, z FROM mixed", false)
            .await
            .unwrap();

        assert_eq!(result.columns, vec!["n", "r", "s", "b", "z"]);
        assert_eq!(result.row_count, 1);
        assert_eq!(result.rows_affected, 0);
        assert!(result.read_only);
        assert!(!result.truncated);

        let row = &result.rows[0];
        assert_eq!(row[0].as_deref(), Some("42"));
        assert_eq!(row[1].as_deref(), Some("3.5"));
        assert_eq!(row[2].as_deref(), Some("hello"));
        assert_eq!(row[3].as_deref(), Some("<3 bytes>"));
        assert_eq!(row[4], None, "SQL NULL is None, not the string \"NULL\"");
    }

    #[tokio::test]
    async fn empty_result_still_reports_its_columns() {
        let (_dir, pool) = fixture().await;
        let result = run_query(&pool, "SELECT id, email FROM users WHERE id < 0", false)
            .await
            .unwrap();
        assert_eq!(result.row_count, 0);
        assert_eq!(result.columns, vec!["id", "email"]);
    }

    #[tokio::test]
    async fn write_is_blocked_without_consent_and_runs_with_it() {
        let (_dir, pool) = fixture().await;
        let insert = "INSERT INTO users (email) VALUES ('c@x')";

        let blocked = run_query(&pool, insert, false).await;
        assert!(
            matches!(&blocked, Err(DbError::WriteBlocked(message)) if message.contains("INSERT")),
            "expected a named WriteBlocked, got {blocked:?}"
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 2, "blocked write left the data alone");

        let allowed = run_query(&pool, insert, true).await.unwrap();
        assert_eq!(allowed.rows_affected, 1);
        assert!(!allowed.read_only);
        let emails: Vec<String> = sqlx::query_scalar("SELECT email FROM users ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(emails, vec!["a@x", "b@x", "c@x"], "the write really landed");
    }

    /// Classification alone would wave this through — it starts with WITH.
    /// `PRAGMA query_only` is what actually stops it.
    #[tokio::test]
    async fn query_only_stops_a_write_disguised_as_a_read() {
        let (_dir, pool) = fixture().await;
        let disguised = "WITH c AS (SELECT 'z@x' AS e) INSERT INTO users (email) SELECT e FROM c";
        assert_eq!(classify(disguised), StatementKind::Read, "parsed as a read");

        let result = run_query(&pool, disguised, false).await;
        assert!(matches!(result, Err(DbError::Db(_))), "got {result:?}");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 2, "nothing was inserted");

        // The connection is usable afterwards — query_only was reset.
        assert_eq!(
            run_query(&pool, "SELECT 1", false).await.unwrap().row_count,
            1
        );
    }

    #[tokio::test]
    async fn caps_rows_and_flags_truncation() {
        let (_dir, pool) = fixture().await;
        sqlx::query("CREATE TABLE big (i INTEGER)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "WITH RECURSIVE seq(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM seq WHERE i < 620)
             INSERT INTO big (i) SELECT i FROM seq",
        )
        .execute(&pool)
        .await
        .unwrap();

        let result = run_query(&pool, "SELECT i FROM big ORDER BY i", false)
            .await
            .unwrap();
        assert!(result.truncated);
        assert_eq!(result.rows.len(), MAX_ROWS);
        assert_eq!(result.row_count, MAX_ROWS as i64);
        assert_eq!(result.rows[0][0].as_deref(), Some("1"));

        let under = run_query(&pool, "SELECT i FROM big WHERE i <= 10", false)
            .await
            .unwrap();
        assert!(!under.truncated, "the cap only bites when it has to");
    }

    #[tokio::test]
    async fn table_rows_quotes_the_identifier() {
        let (_dir, pool) = fixture().await;
        sqlx::query("CREATE TABLE \"weird\"\"name\" (a TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO \"weird\"\"name\" (a) VALUES ('one'), ('two')")
            .execute(&pool)
            .await
            .unwrap();

        let result = table_rows(&pool, "weird\"name", 10).await.unwrap();
        assert_eq!(result.columns, vec!["a"]);
        assert_eq!(result.row_count, 2);
        assert_eq!(result.rows[1][0].as_deref(), Some("two"));
        assert!(result.read_only);

        let limited = table_rows(&pool, "users", 1).await.unwrap();
        assert_eq!(limited.row_count, 1, "the limit is bound, not formatted");
        assert!(table_rows(&pool, "  ", 10).await.is_err());
    }

    #[tokio::test]
    async fn rejects_an_empty_statement() {
        let (_dir, pool) = fixture().await;
        assert!(matches!(
            run_query(&pool, "   \n  ", false).await,
            Err(DbError::Invalid(_))
        ));
        assert!(matches!(
            run_query(&pool, "-- nothing here", true).await,
            Err(DbError::Invalid(_))
        ));
    }
}
