//! The module's error type, and the DTO it crosses IPC as.
//!
//! Every other module flattens its error to a string at the command layer
//! (`.map_err(|e| e.to_string())`), because the frontend only ever shows the
//! text. The database page is the exception: one class of failure — a write
//! refused because the connection is read-only — is rendered as an
//! explanatory card with a way out, and everything else as a plain error.
//!
//! That decision needs a discriminant. It used to be recovered on the far side
//! by matching the words "write" and "block" in the `Display` output, which
//! made `#[error("write blocked: {0}")]` load-bearing prose: rewording it
//! downgraded the card to a raw dump with nothing failing. So these commands
//! return [`DbErrorDto`] instead of `String`, and the frontend switches on
//! [`DbErrorDto::kind`].

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("write blocked: {0}")]
    WriteBlocked(String),
}

pub type DbResult<T> = Result<T, DbError>;

/// SQLite's primary result code for "this connection may not write"
/// (`SQLITE_READONLY`). sqlx reports the *extended* code, and every extended
/// form is `8 | (n << 8)`, so the low byte identifies the whole family.
const SQLITE_READONLY: i32 = 8;

// What the database page branches on. One variant per `DbError` case the UI
// treats differently; the message is carried alongside rather than encoded in
// it, so wording and behaviour can change independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum DbErrorKind {
    // A statement was refused because it writes and writes are not enabled.
    // Raised by `run_query` before execution — and also mapped from the
    // driver error SQLite produces when a read-only handle refuses a write
    // that classification waved through. Both mean "turn writes on and try
    // again", which is the only thing the distinction would change.
    WriteBlocked,
    Invalid,
    NotFound,
    Database,
}

// The error every `db_*` command rejects with.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DbErrorDto {
    pub kind: DbErrorKind,
    // Exactly what `DbError`'s `Display` produces, unchanged: the UI still
    // shows the raw text under the explanation, and a kind is not a substitute
    // for "no such table: usrs".
    pub message: String,
}

impl From<DbError> for DbErrorDto {
    fn from(error: DbError) -> Self {
        let kind = match &error {
            DbError::WriteBlocked(_) => DbErrorKind::WriteBlocked,
            DbError::Invalid(_) => DbErrorKind::Invalid,
            DbError::NotFound(_) => DbErrorKind::NotFound,
            DbError::Db(source) if is_read_only_refusal(source) => DbErrorKind::WriteBlocked,
            DbError::Db(_) => DbErrorKind::Database,
        };
        Self {
            kind,
            message: error.to_string(),
        }
    }
}

/// True when SQLite itself refused the statement because the handle may not
/// write — `SQLITE_OPEN_READONLY` or `PRAGMA query_only`.
///
/// This is guards 2 and 3 in [`crate::query`] reporting in. A write that
/// classification waved through (`WITH x AS (…) INSERT …` opens with a read
/// keyword) never becomes [`DbError::WriteBlocked`]; it comes back as a driver
/// error. The user's situation is identical to a refused write, so the UI
/// should say the same thing.
///
/// Matched on the numeric result code rather than the message, so this does
/// not depend on SQLite's error prose either.
fn is_read_only_refusal(error: &sqlx::Error) -> bool {
    let sqlx::Error::Database(source) = error else {
        return false;
    };
    source
        .code()
        .and_then(|code| code.parse::<i32>().ok())
        .is_some_and(|code| code & 0xff == SQLITE_READONLY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use sqlx::SqlitePool;

    /// A real database file with one table, plus a genuinely read-only pool
    /// over it — the same open mode the query read path uses in production.
    async fn read_only_pool(dir: &tempfile::TempDir) -> SqlitePool {
        let path = dir.path().join("app.sqlite");
        let write = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&path)
                    .create_if_missing(true),
            )
            .await
            .unwrap();
        sqlx::query("CREATE TABLE t (a INTEGER)")
            .execute(&write)
            .await
            .unwrap();
        write.close().await;

        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&path)
                    .create_if_missing(false)
                    .read_only(true),
            )
            .await
            .unwrap()
    }

    #[test]
    fn every_variant_keeps_its_own_discriminant_and_its_message() {
        let cases = [
            (
                DbError::WriteBlocked("INSERT modifies the database".into()),
                DbErrorKind::WriteBlocked,
                "write blocked: INSERT modifies the database",
            ),
            (
                DbError::Invalid("query is empty".into()),
                DbErrorKind::Invalid,
                "invalid input: query is empty",
            ),
            (
                DbError::NotFound("connection not found: c1".into()),
                DbErrorKind::NotFound,
                "not found: connection not found: c1",
            ),
            (
                DbError::Db(sqlx::Error::RowNotFound),
                DbErrorKind::Database,
                "database error: no rows returned by a query that expected to return at least one row",
            ),
        ];

        for (error, kind, message) in cases {
            let dto = DbErrorDto::from(error);
            assert_eq!(dto.kind, kind);
            assert_eq!(dto.message, message, "Display text crosses unchanged");
        }
    }

    /// The wire shape the frontend classifier matches on. Serde's renaming is
    /// what makes `kind` a `"writeBlocked"` the TypeScript union can hold, so
    /// it is pinned here rather than assumed.
    #[test]
    fn the_serialized_shape_is_the_one_the_frontend_reads() {
        let dto = DbErrorDto::from(DbError::WriteBlocked("DROP modifies the database".into()));
        assert_eq!(
            serde_json::to_value(&dto).unwrap(),
            serde_json::json!({
                "kind": "writeBlocked",
                "message": "write blocked: DROP modifies the database",
            })
        );

        for (error, kind) in [
            (DbError::Invalid("x".into()), "invalid"),
            (DbError::NotFound("x".into()), "notFound"),
            (DbError::Db(sqlx::Error::PoolClosed), "database"),
        ] {
            assert_eq!(
                serde_json::to_value(DbErrorDto::from(error)).unwrap()["kind"],
                serde_json::json!(kind)
            );
        }
    }

    /// The bypass case: SQLite, not our classifier, is what stops a write
    /// disguised as a read. That refusal must reach the UI as the same kind as
    /// an ordinary refused write — and an ordinary SQL mistake on the same
    /// handle must not be swept up with it.
    #[tokio::test]
    async fn a_refusal_from_a_read_only_handle_reads_as_write_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let pool = read_only_pool(&dir).await;

        let refused = sqlx::query("INSERT INTO t (a) VALUES (1)")
            .execute(&pool)
            .await
            .expect_err("a read-only handle refuses the write");
        let dto = DbErrorDto::from(DbError::from(refused));
        assert_eq!(
            dto.kind,
            DbErrorKind::WriteBlocked,
            "SQLite's own refusal, classified from its result code: {}",
            dto.message
        );

        let broken = sqlx::query_scalar::<_, i64>("SELECT a FROM nope")
            .fetch_one(&pool)
            .await
            .expect_err("no such table");
        assert_eq!(
            DbErrorDto::from(DbError::from(broken)).kind,
            DbErrorKind::Database,
            "an ordinary SQL failure stays an ordinary SQL failure"
        );
    }
}
