//! Conversation persistence. The AI module owns its `ai_*` tables and
//! creates them idempotently at boot (per-module migration tables arrive
//! with the plugin system).

use sqlx::{Row, SqlitePool};

use crate::providers::{AiError, AiResult};
use crate::types::{ChatMessage, Conversation, MemoryCategory, MemoryEntry};

const MAX_MEMORY_CONTENT: usize = 500;
const MAX_MEMORY_ENTRIES: i64 = 100;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub async fn init(pool: &SqlitePool) -> AiResult<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS ai_conversations (
            id         TEXT PRIMARY KEY,
            title      TEXT NOT NULL,
            provider   TEXT NOT NULL,
            model      TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS ai_messages (
            id              TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL REFERENCES ai_conversations (id) ON DELETE CASCADE,
            role            TEXT NOT NULL,
            content         TEXT NOT NULL,
            created_at      INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_ai_messages_conversation
         ON ai_messages (conversation_id, created_at)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS ai_memory (
            id         TEXT PRIMARY KEY,
            project    TEXT NOT NULL,
            content    TEXT NOT NULL,
            created_at INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_ai_memory_project ON ai_memory (project, created_at)",
    )
    .execute(pool)
    .await?;
    add_category_column(pool).await?;
    Ok(())
}

/// The first additive migration this module has needed (see the module doc
/// comment: a real per-module migration table is still future work). `init`
/// runs on every boot, not just the first one, so this has to be idempotent
/// by hand: check `PRAGMA table_info` before altering, because SQLite errors
/// on `ADD COLUMN` for a column that already exists rather than no-op'ing.
/// Existing rows backfill to `Other` via the column's own `DEFAULT`.
async fn add_category_column(pool: &SqlitePool) -> AiResult<()> {
    let columns = sqlx::query("PRAGMA table_info(ai_memory)")
        .fetch_all(pool)
        .await?;
    let has_category = columns
        .iter()
        .any(|row| row.get::<String, _>("name") == "category");
    if !has_category {
        sqlx::query("ALTER TABLE ai_memory ADD COLUMN category TEXT NOT NULL DEFAULT 'other'")
            .execute(pool)
            .await?;
    }
    Ok(())
}

// ---- Long-term memory ----

pub async fn memory_list(pool: &SqlitePool, project: &str) -> AiResult<Vec<MemoryEntry>> {
    let rows = sqlx::query(
        "SELECT id, project, content, category, created_at FROM ai_memory
         WHERE project = ?1 ORDER BY created_at ASC, id ASC",
    )
    .bind(project)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|row| MemoryEntry {
            id: row.get("id"),
            project: row.get("project"),
            content: row.get("content"),
            category: MemoryCategory::parse(&row.get::<String, _>("category")),
            created_at: row.get("created_at"),
        })
        .collect())
}

pub async fn memory_add(
    pool: &SqlitePool,
    project: &str,
    content: &str,
    category: MemoryCategory,
) -> AiResult<MemoryEntry> {
    let content = content.trim();
    if content.is_empty() {
        return Err(AiError::InvalidInput("memory content is empty".into()));
    }
    if content.chars().count() > MAX_MEMORY_CONTENT {
        return Err(AiError::InvalidInput(format!(
            "memory entries are capped at {MAX_MEMORY_CONTENT} characters; save a shorter fact"
        )));
    }
    let count: i64 = sqlx::query("SELECT COUNT(*) AS n FROM ai_memory WHERE project = ?1")
        .bind(project)
        .fetch_one(pool)
        .await?
        .get("n");
    if count >= MAX_MEMORY_ENTRIES {
        return Err(AiError::InvalidInput(format!(
            "memory is full ({MAX_MEMORY_ENTRIES} entries); delete outdated entries first"
        )));
    }
    let entry = MemoryEntry {
        id: new_id(),
        project: project.to_string(),
        content: content.to_string(),
        category,
        created_at: now_ms(),
    };
    sqlx::query(
        "INSERT INTO ai_memory (id, project, content, category, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(&entry.id)
    .bind(&entry.project)
    .bind(&entry.content)
    .bind(entry.category.as_str())
    .bind(entry.created_at)
    .execute(pool)
    .await?;
    Ok(entry)
}

pub async fn memory_delete(pool: &SqlitePool, id: &str) -> AiResult<()> {
    let result = sqlx::query("DELETE FROM ai_memory WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AiError::InvalidInput(format!(
            "memory entry not found: {id}"
        )));
    }
    Ok(())
}

fn conversation_from_row(row: &sqlx::sqlite::SqliteRow) -> Conversation {
    Conversation {
        id: row.get("id"),
        title: row.get("title"),
        provider: row.get("provider"),
        model: row.get("model"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

pub async fn create_conversation(
    pool: &SqlitePool,
    provider: &str,
    model: &str,
) -> AiResult<Conversation> {
    let now = now_ms();
    let conversation = Conversation {
        id: new_id(),
        title: "New chat".into(),
        provider: provider.to_string(),
        model: model.to_string(),
        created_at: now,
        updated_at: now,
    };
    sqlx::query(
        "INSERT INTO ai_conversations (id, title, provider, model, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(&conversation.id)
    .bind(&conversation.title)
    .bind(&conversation.provider)
    .bind(&conversation.model)
    .bind(conversation.created_at)
    .bind(conversation.updated_at)
    .execute(pool)
    .await?;
    Ok(conversation)
}

pub async fn get_conversation(pool: &SqlitePool, id: &str) -> AiResult<Option<Conversation>> {
    let row = sqlx::query("SELECT * FROM ai_conversations WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.as_ref().map(conversation_from_row))
}

pub async fn list_conversations(pool: &SqlitePool) -> AiResult<Vec<Conversation>> {
    let rows = sqlx::query("SELECT * FROM ai_conversations ORDER BY updated_at DESC")
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(conversation_from_row).collect())
}

pub async fn delete_conversation(pool: &SqlitePool, id: &str) -> AiResult<()> {
    sqlx::query("DELETE FROM ai_conversations WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn messages(pool: &SqlitePool, conversation_id: &str) -> AiResult<Vec<ChatMessage>> {
    let rows = sqlx::query(
        "SELECT id, conversation_id, role, content, created_at
         FROM ai_messages WHERE conversation_id = ?1 ORDER BY created_at ASC, id ASC",
    )
    .bind(conversation_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|row| ChatMessage {
            id: row.get("id"),
            conversation_id: row.get("conversation_id"),
            role: row.get("role"),
            content: row.get("content"),
            created_at: row.get("created_at"),
        })
        .collect())
}

pub async fn append_message(
    pool: &SqlitePool,
    conversation_id: &str,
    role: &str,
    content: &str,
) -> AiResult<ChatMessage> {
    let message = ChatMessage {
        id: new_id(),
        conversation_id: conversation_id.to_string(),
        role: role.to_string(),
        content: content.to_string(),
        created_at: now_ms(),
    };
    sqlx::query(
        "INSERT INTO ai_messages (id, conversation_id, role, content, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(&message.id)
    .bind(&message.conversation_id)
    .bind(&message.role)
    .bind(&message.content)
    .bind(message.created_at)
    .execute(pool)
    .await?;

    sqlx::query("UPDATE ai_conversations SET updated_at = ?1 WHERE id = ?2")
        .bind(message.created_at)
        .bind(conversation_id)
        .execute(pool)
        .await?;

    // First user message names the chat.
    if role == "user" {
        sqlx::query("UPDATE ai_conversations SET title = ?1 WHERE id = ?2 AND title = 'New chat'")
            .bind(title_from(content))
            .bind(conversation_id)
            .execute(pool)
            .await?;
    }

    Ok(message)
}

fn title_from(content: &str) -> String {
    let flat = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 48 {
        let cut: String = flat.chars().take(48).collect();
        format!("{cut}…")
    } else if flat.is_empty() {
        "New chat".into()
    } else {
        flat
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    async fn test_pool() -> (tempfile::TempDir, SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("ai.db"))
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        init(&pool).await.unwrap();
        (dir, pool)
    }

    #[tokio::test]
    async fn conversation_lifecycle_and_titles() {
        let (_dir, pool) = test_pool().await;
        let conv = create_conversation(&pool, "claude", "claude-sonnet-5")
            .await
            .unwrap();
        assert_eq!(conv.title, "New chat");

        append_message(&pool, &conv.id, "user", "How do I write a Rust macro?")
            .await
            .unwrap();
        append_message(&pool, &conv.id, "assistant", "Like this…")
            .await
            .unwrap();

        let listed = list_conversations(&pool).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].title, "How do I write a Rust macro?");

        let history = messages(&pool, &conv.id).await.unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, "user");
        assert_eq!(history[1].role, "assistant");

        delete_conversation(&pool, &conv.id).await.unwrap();
        assert!(list_conversations(&pool).await.unwrap().is_empty());
        assert!(messages(&pool, &conv.id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn memory_roundtrip_isolation_and_caps() {
        let (_dir, pool) = test_pool().await;
        let a = memory_add(
            &pool,
            "C:/proj/a",
            "uses pnpm, not npm",
            MemoryCategory::Convention,
        )
        .await
        .unwrap();
        memory_add(
            &pool,
            "C:/proj/a",
            "CI runs on windows-latest",
            MemoryCategory::Architecture,
        )
        .await
        .unwrap();
        memory_add(
            &pool,
            "C:/proj/b",
            "different project fact",
            MemoryCategory::Other,
        )
        .await
        .unwrap();

        let listed = memory_list(&pool, "C:/proj/a").await.unwrap();
        assert_eq!(listed.len(), 2, "memory is per-project");
        assert_eq!(listed[0].content, "uses pnpm, not npm");
        assert_eq!(listed[0].category, MemoryCategory::Convention);
        assert_eq!(listed[1].category, MemoryCategory::Architecture);

        assert!(memory_add(&pool, "C:/proj/a", "   ", MemoryCategory::Other)
            .await
            .is_err());
        assert!(
            memory_add(&pool, "C:/proj/a", &"x".repeat(600), MemoryCategory::Other)
                .await
                .is_err()
        );

        memory_delete(&pool, &a.id).await.unwrap();
        assert!(memory_delete(&pool, &a.id).await.is_err(), "double delete");
        assert_eq!(memory_list(&pool, "C:/proj/a").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_row_from_before_categorization_reads_back_as_other() {
        let (_dir, pool) = test_pool().await;
        // Simulates a database that already had `ai_memory` rows before this
        // migration existed — bypasses `memory_add` to insert without a
        // `category`, the way the pre-migration schema would have.
        sqlx::query(
            "INSERT INTO ai_memory (id, project, content, created_at) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind("legacy-1")
        .bind("C:/proj/a")
        .bind("a fact saved before categories existed")
        .bind(now_ms())
        .execute(&pool)
        .await
        .unwrap();

        let listed = memory_list(&pool, "C:/proj/a").await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].category, MemoryCategory::Other);
    }

    #[tokio::test]
    async fn running_init_twice_does_not_error_on_the_category_column() {
        let (_dir, pool) = test_pool().await;
        init(&pool).await.unwrap();
        init(&pool).await.unwrap();
    }

    #[tokio::test]
    async fn long_titles_are_truncated() {
        let (_dir, pool) = test_pool().await;
        let conv = create_conversation(&pool, "ollama", "llama3.2")
            .await
            .unwrap();
        let long = "x".repeat(200);
        append_message(&pool, &conv.id, "user", &long)
            .await
            .unwrap();
        let listed = list_conversations(&pool).await.unwrap();
        assert!(listed[0].title.chars().count() <= 49);
    }
}
