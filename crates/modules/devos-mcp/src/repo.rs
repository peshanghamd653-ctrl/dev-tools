//! Saved MCP server configurations: a display name plus the launch command
//! and arguments used to start it. The module owns `mcp_servers` and creates
//! it idempotently at boot.

use sqlx::{Row, SqlitePool};

use crate::{McpError, McpResult};

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// A configured server: how to start it, not whether it is currently
/// running — see the module doc comment on why connections are one-shot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../../src/shared/ipc/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    #[ts(type = "number")]
    pub created_at: i64,
}

pub async fn init(pool: &SqlitePool) -> McpResult<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS mcp_servers (
            id         TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            command    TEXT NOT NULL,
            args       TEXT NOT NULL,
            created_at INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

fn server_from_row(row: &sqlx::sqlite::SqliteRow) -> McpServer {
    let args_json: String = row.get("args");
    McpServer {
        id: row.get("id"),
        name: row.get("name"),
        command: row.get("command"),
        args: serde_json::from_str(&args_json).unwrap_or_default(),
        created_at: row.get("created_at"),
    }
}

pub async fn create_server(
    pool: &SqlitePool,
    name: &str,
    command: &str,
    args: &[String],
) -> McpResult<McpServer> {
    let name = name.trim();
    let command = command.trim();
    if name.is_empty() {
        return Err(McpError::InvalidInput("server name is empty".into()));
    }
    if command.is_empty() {
        return Err(McpError::InvalidInput("launch command is empty".into()));
    }
    let server = McpServer {
        id: new_id(),
        name: name.to_string(),
        command: command.to_string(),
        args: args.to_vec(),
        created_at: now_ms(),
    };
    sqlx::query(
        "INSERT INTO mcp_servers (id, name, command, args, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(&server.id)
    .bind(&server.name)
    .bind(&server.command)
    .bind(serde_json::to_string(&server.args).expect("Vec<String> always serializes"))
    .bind(server.created_at)
    .execute(pool)
    .await?;
    Ok(server)
}

pub async fn list_servers(pool: &SqlitePool) -> McpResult<Vec<McpServer>> {
    let rows =
        sqlx::query("SELECT id, name, command, args, created_at FROM mcp_servers ORDER BY name")
            .fetch_all(pool)
            .await?;
    Ok(rows.iter().map(server_from_row).collect())
}

pub async fn delete_server(pool: &SqlitePool, id: &str) -> McpResult<()> {
    let result = sqlx::query("DELETE FROM mcp_servers WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(McpError::InvalidInput(format!("server not found: {id}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    async fn test_pool() -> (tempfile::TempDir, SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("mcp.db"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        init(&pool).await.unwrap();
        (dir, pool)
    }

    #[tokio::test]
    async fn create_list_delete_roundtrip() {
        let (_dir, pool) = test_pool().await;
        let server = create_server(
            &pool,
            "Filesystem",
            "npx",
            &[
                "-y".into(),
                "@modelcontextprotocol/server-filesystem".into(),
            ],
        )
        .await
        .unwrap();

        let listed = list_servers(&pool).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Filesystem");
        assert_eq!(listed[0].command, "npx");
        assert_eq!(
            listed[0].args,
            vec!["-y", "@modelcontextprotocol/server-filesystem"]
        );

        delete_server(&pool, &server.id).await.unwrap();
        assert!(list_servers(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_blank_name_or_command_is_refused() {
        let (_dir, pool) = test_pool().await;
        assert!(create_server(&pool, "  ", "npx", &[]).await.is_err());
        assert!(create_server(&pool, "Filesystem", "  ", &[]).await.is_err());
        assert!(list_servers(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn deleting_an_id_that_does_not_exist_is_an_error() {
        let (_dir, pool) = test_pool().await;
        assert!(delete_server(&pool, "missing").await.is_err());
    }

    #[tokio::test]
    async fn servers_come_back_alphabetically() {
        let (_dir, pool) = test_pool().await;
        create_server(&pool, "Weather", "uvx", &[]).await.unwrap();
        create_server(&pool, "Filesystem", "npx", &[])
            .await
            .unwrap();

        let listed = list_servers(&pool).await.unwrap();
        assert_eq!(
            listed.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            vec!["Filesystem", "Weather"]
        );
    }
}
