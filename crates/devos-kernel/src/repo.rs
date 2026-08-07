//! Repository layer: all SQL lives here. Commands and modules call these
//! functions instead of writing queries inline.

use sqlx::{Row, SqlitePool};

use crate::error::{KernelError, KernelResult};
use crate::types::{AuditEntry, AuditLog, NotificationDto, Project, Workspace};

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn workspace_from_row(row: &sqlx::sqlite::SqliteRow) -> Workspace {
    Workspace {
        id: row.get("id"),
        name: row.get("name"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn project_from_row(row: &sqlx::sqlite::SqliteRow) -> Project {
    Project {
        id: row.get("id"),
        workspace_id: row.get("workspace_id"),
        name: row.get("name"),
        path: row.get("path"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

// ---- Workspaces ----

pub async fn list_workspaces(pool: &SqlitePool) -> KernelResult<Vec<Workspace>> {
    let rows = sqlx::query("SELECT id, name, created_at, updated_at FROM workspaces ORDER BY name")
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(workspace_from_row).collect())
}

pub async fn get_workspace(pool: &SqlitePool, id: &str) -> KernelResult<Workspace> {
    let row = sqlx::query("SELECT id, name, created_at, updated_at FROM workspaces WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.as_ref()
        .map(workspace_from_row)
        .ok_or_else(|| KernelError::NotFound(format!("workspace {id}")))
}

pub async fn create_workspace(pool: &SqlitePool, name: &str) -> KernelResult<Workspace> {
    let name = name.trim();
    if name.is_empty() {
        return Err(KernelError::InvalidInput("workspace name is empty".into()));
    }
    let now = now_ms();
    let workspace = Workspace {
        id: new_id(),
        name: name.to_string(),
        created_at: now,
        updated_at: now,
    };
    sqlx::query(
        "INSERT INTO workspaces (id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(&workspace.id)
    .bind(&workspace.name)
    .bind(workspace.created_at)
    .bind(workspace.updated_at)
    .execute(pool)
    .await?;
    Ok(workspace)
}

pub async fn rename_workspace(pool: &SqlitePool, id: &str, name: &str) -> KernelResult<Workspace> {
    let name = name.trim();
    if name.is_empty() {
        return Err(KernelError::InvalidInput("workspace name is empty".into()));
    }
    let result = sqlx::query("UPDATE workspaces SET name = ?1, updated_at = ?2 WHERE id = ?3")
        .bind(name)
        .bind(now_ms())
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(KernelError::NotFound(format!("workspace {id}")));
    }
    get_workspace(pool, id).await
}

pub async fn delete_workspace(pool: &SqlitePool, id: &str) -> KernelResult<()> {
    let count: i64 = sqlx::query("SELECT COUNT(*) AS n FROM workspaces")
        .fetch_one(pool)
        .await?
        .get("n");
    if count <= 1 {
        return Err(KernelError::InvalidInput(
            "cannot delete the last workspace".into(),
        ));
    }
    let result = sqlx::query("DELETE FROM workspaces WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(KernelError::NotFound(format!("workspace {id}")));
    }
    Ok(())
}

/// Boot invariant: at least one workspace always exists.
pub async fn ensure_default_workspace(pool: &SqlitePool) -> KernelResult<Workspace> {
    let existing = list_workspaces(pool).await?;
    match existing.into_iter().next() {
        Some(ws) => Ok(ws),
        None => create_workspace(pool, "Personal").await,
    }
}

// ---- Projects ----

pub async fn list_projects(pool: &SqlitePool, workspace_id: &str) -> KernelResult<Vec<Project>> {
    let rows = sqlx::query(
        "SELECT id, workspace_id, name, path, created_at, updated_at
         FROM projects WHERE workspace_id = ?1 ORDER BY updated_at DESC",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(project_from_row).collect())
}

pub async fn add_project(
    pool: &SqlitePool,
    workspace_id: &str,
    name: &str,
    path: &str,
) -> KernelResult<Project> {
    let name = name.trim();
    let path = path.trim();
    if name.is_empty() {
        return Err(KernelError::InvalidInput("project name is empty".into()));
    }
    if path.is_empty() {
        return Err(KernelError::InvalidInput("project path is empty".into()));
    }
    // Verify the workspace exists so we fail with a clear error, not an FK violation.
    get_workspace(pool, workspace_id).await?;

    let duplicate = sqlx::query("SELECT id FROM projects WHERE workspace_id = ?1 AND path = ?2")
        .bind(workspace_id)
        .bind(path)
        .fetch_optional(pool)
        .await?;
    if duplicate.is_some() {
        return Err(KernelError::InvalidInput(format!(
            "project at {path} already exists in this workspace"
        )));
    }

    let now = now_ms();
    let project = Project {
        id: new_id(),
        workspace_id: workspace_id.to_string(),
        name: name.to_string(),
        path: path.to_string(),
        created_at: now,
        updated_at: now,
    };
    sqlx::query(
        "INSERT INTO projects (id, workspace_id, name, path, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(&project.id)
    .bind(&project.workspace_id)
    .bind(&project.name)
    .bind(&project.path)
    .bind(project.created_at)
    .bind(project.updated_at)
    .execute(pool)
    .await?;
    Ok(project)
}

pub async fn remove_project(pool: &SqlitePool, id: &str) -> KernelResult<()> {
    let result = sqlx::query("DELETE FROM projects WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(KernelError::NotFound(format!("project {id}")));
    }
    Ok(())
}

// ---- Notifications ----

fn notification_from_row(row: &sqlx::sqlite::SqliteRow) -> NotificationDto {
    NotificationDto {
        id: row.get("id"),
        module: row.get("module"),
        level: row.get("level"),
        title: row.get("title"),
        body: row.get("body"),
        read: row.get::<i64, _>("read") != 0,
        created_at: row.get("created_at"),
    }
}

pub async fn add_notification(
    pool: &SqlitePool,
    module: &str,
    level: &str,
    title: &str,
    body: Option<&str>,
) -> KernelResult<NotificationDto> {
    let notification = NotificationDto {
        id: new_id(),
        module: module.to_string(),
        level: level.to_string(),
        title: title.to_string(),
        body: body.map(String::from),
        read: false,
        created_at: now_ms(),
    };
    sqlx::query(
        "INSERT INTO notifications (id, module, level, title, body, read, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)",
    )
    .bind(&notification.id)
    .bind(&notification.module)
    .bind(&notification.level)
    .bind(&notification.title)
    .bind(&notification.body)
    .bind(notification.created_at)
    .execute(pool)
    .await?;
    Ok(notification)
}

pub async fn list_notifications(
    pool: &SqlitePool,
    limit: i64,
) -> KernelResult<Vec<NotificationDto>> {
    let rows = sqlx::query(
        "SELECT id, module, level, title, body, read, created_at
         FROM notifications ORDER BY created_at DESC, id DESC LIMIT ?1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(notification_from_row).collect())
}

pub async fn unread_notification_count(pool: &SqlitePool) -> KernelResult<i64> {
    let row = sqlx::query("SELECT COUNT(*) AS n FROM notifications WHERE read = 0")
        .fetch_one(pool)
        .await?;
    Ok(row.get("n"))
}

pub async fn mark_notification_read(pool: &SqlitePool, id: &str) -> KernelResult<()> {
    let result = sqlx::query("UPDATE notifications SET read = 1 WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(KernelError::NotFound(format!("notification {id}")));
    }
    Ok(())
}

pub async fn mark_all_notifications_read(pool: &SqlitePool) -> KernelResult<()> {
    sqlx::query("UPDATE notifications SET read = 1 WHERE read = 0")
        .execute(pool)
        .await?;
    Ok(())
}

// ---- Audit log ----
//
// Append-only in the literal sense: there is no update and no targeted
// delete here. The only thing that removes a row is [`prune_audit_log`],
// which removes rows by *age* and cannot be aimed at a particular entry —
// so nothing in the app, and nothing reachable from IPC, can edit its own
// trail.

fn audit_entry_from_row(row: &sqlx::sqlite::SqliteRow) -> AuditEntry {
    AuditEntry {
        id: row.get("id"),
        actor: row.get("actor"),
        action: row.get("action"),
        detail: row.get("detail"),
        created_at: row.get("created_at"),
    }
}

/// Append one row. Prefer [`crate::audit::record`], which is what call sites
/// use — it fixes the vocabulary and swallows the failure. This is the
/// fallible half, kept public so tests can prove the write actually lands.
pub async fn add_audit_entry(
    pool: &SqlitePool,
    actor: &str,
    action: &str,
    detail: Option<&str>,
) -> KernelResult<AuditEntry> {
    let created_at = now_ms();
    let result = sqlx::query(
        "INSERT INTO audit_log (actor, action, detail, created_at) VALUES (?1, ?2, ?3, ?4)",
    )
    .bind(actor)
    .bind(action)
    .bind(detail)
    .bind(created_at)
    .execute(pool)
    .await?;
    Ok(AuditEntry {
        id: result.last_insert_rowid(),
        actor: actor.to_string(),
        action: action.to_string(),
        detail: detail.map(String::from),
        created_at,
    })
}

/// Newest first.
///
/// Ordered by `id`, not `created_at`: the column is `INTEGER PRIMARY KEY
/// AUTOINCREMENT`, so it *is* insertion order, it breaks the tie two events
/// in the same millisecond would otherwise leave undefined, and it reads
/// straight off the primary key — which is why this table needs no second
/// index to be listed or pruned cheaply.
pub async fn list_audit_entries(pool: &SqlitePool, limit: i64) -> KernelResult<Vec<AuditEntry>> {
    let rows = sqlx::query(
        "SELECT id, actor, action, detail, created_at
         FROM audit_log ORDER BY id DESC LIMIT ?1",
    )
    .bind(limit.max(1))
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(audit_entry_from_row).collect())
}

/// The newest `limit` entries plus the shape of the whole table, so a viewer
/// can say "showing 200 of 1,412, back to 12 May" instead of presenting a
/// truncated list as the complete record.
pub async fn audit_log(pool: &SqlitePool, limit: i64) -> KernelResult<AuditLog> {
    let entries = list_audit_entries(pool, limit).await?;
    let row = sqlx::query("SELECT COUNT(*) AS n, MIN(created_at) AS oldest FROM audit_log")
        .fetch_one(pool)
        .await?;
    Ok(AuditLog {
        entries,
        total: row.get("n"),
        // NULL on an empty table, which is "no record yet" and not epoch zero.
        oldest: row.get("oldest"),
        retention_days: crate::audit::RETENTION_DAYS,
    })
}

/// Delete every entry older than `cutoff_ms`. Returns how many went.
pub async fn prune_audit_log(pool: &SqlitePool, cutoff_ms: i64) -> KernelResult<u64> {
    let result = sqlx::query("DELETE FROM audit_log WHERE created_at < ?1")
        .bind(cutoff_ms)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

// ---- Settings ----

pub async fn get_setting(pool: &SqlitePool, key: &str) -> KernelResult<Option<String>> {
    let row = sqlx::query("SELECT value FROM settings WHERE key = ?1")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get("value")))
}

pub async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> KernelResult<()> {
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .bind(now_ms())
    .execute(pool)
    .await?;
    Ok(())
}
