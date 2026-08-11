//! Saved requests + automatic history. The module owns its `api_*` tables
//! and creates them idempotently at boot.

use devos_secrets::SecretStore;
use sqlx::{Row, SqlitePool};

use crate::{
    ApiEnvVar, ApiEnvironment, ApiHeader, ApiHistoryEntry, ApiRequestSpec, ApiResult, SavedRequest,
};

const HISTORY_CAP: i64 = 100;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// The vault key a secret [`ApiEnvVar`] is stored under — keyed by the
/// var's stable `id`, not its `key`, so a rename in the editor never
/// orphans the vault entry.
fn vault_key(var_id: &str) -> String {
    format!("api-env-var:{var_id}")
}

fn parse_vars(raw: &str) -> Vec<ApiEnvVar> {
    serde_json::from_str(raw).unwrap_or_default()
}

/// Fill secret variables' real values back in from the vault. Best-effort
/// per variable: a vault miss or decrypt error leaves the stored
/// placeholder (empty string) rather than failing the whole read, so one
/// damaged entry cannot take an entire environment off the Environments
/// list.
async fn hydrate(secrets: &SecretStore, vars: Vec<ApiEnvVar>) -> Vec<ApiEnvVar> {
    let mut out = Vec::with_capacity(vars.len());
    for mut var in vars {
        if var.secret {
            if let Ok(Some(value)) = secrets.get(&vault_key(&var.id)).await {
                var.value = value;
            }
        }
        out.push(var);
    }
    out
}

/// Assign a stable `id` to any variable that arrived without one (a
/// brand-new row from the editor), route secret values into the vault
/// keyed by that id, and return the vars as they should be written to
/// `api_environments.vars` — secret values replaced with an empty string,
/// never the plaintext.
async fn persist_vars(secrets: &SecretStore, vars: &[ApiEnvVar]) -> ApiResult<Vec<ApiEnvVar>> {
    let mut out = vars.to_vec();
    for var in &mut out {
        if var.id.is_empty() {
            var.id = new_id();
        }
        if var.secret {
            secrets.set(&vault_key(&var.id), &var.value).await?;
            var.value = String::new();
        } else {
            // Not secret now — make sure no stale vault entry lingers from
            // a var that used to be marked secret and got toggled off.
            let _ = secrets.delete(&vault_key(&var.id)).await;
        }
    }
    Ok(out)
}

pub async fn init(pool: &SqlitePool) -> ApiResult<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS api_requests (
            id         TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            collection TEXT NOT NULL,
            method     TEXT NOT NULL,
            url        TEXT NOT NULL,
            headers    TEXT NOT NULL,
            body       TEXT,
            updated_at INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS api_history (
            id          TEXT PRIMARY KEY,
            method      TEXT NOT NULL,
            url         TEXT NOT NULL,
            status      INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            sent_at     INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS api_environments (
            id         TEXT PRIMARY KEY,
            name       TEXT NOT NULL,
            vars       TEXT NOT NULL,
            is_active  INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

fn environment_from_row(row: &sqlx::sqlite::SqliteRow) -> ApiEnvironment {
    ApiEnvironment {
        id: row.get("id"),
        name: row.get("name"),
        vars: parse_vars(row.get("vars")),
        active: row.get::<i64, _>("is_active") != 0,
        updated_at: row.get("updated_at"),
    }
}

pub async fn create_environment(pool: &SqlitePool, name: &str) -> ApiResult<ApiEnvironment> {
    let name = name.trim();
    if name.is_empty() {
        return Err(crate::ApiError::Invalid("environment name is empty".into()));
    }
    let env = ApiEnvironment {
        id: new_id(),
        name: name.to_string(),
        vars: Vec::new(),
        active: false,
        updated_at: now_ms(),
    };
    sqlx::query(
        "INSERT INTO api_environments (id, name, vars, is_active, updated_at)
         VALUES (?1, ?2, ?3, 0, ?4)",
    )
    .bind(&env.id)
    .bind(&env.name)
    .bind("[]")
    .bind(env.updated_at)
    .execute(pool)
    .await?;
    Ok(env)
}

/// Rename an environment and replace its variable set wholesale — simpler
/// and easier to reason about than diffing individual variable edits, and
/// the frontend already holds the full list in the editor it calls this
/// from. Vault entries do need a diff against the previous vars, though:
/// a variable removed from the list, or turned secret and never was
/// before, must not leave an orphaned entry behind — see [`persist_vars`].
pub async fn update_environment(
    pool: &SqlitePool,
    secrets: &SecretStore,
    id: &str,
    name: &str,
    vars: &[ApiEnvVar],
) -> ApiResult<ApiEnvironment> {
    let name = name.trim();
    if name.is_empty() {
        return Err(crate::ApiError::Invalid("environment name is empty".into()));
    }
    let previous = sqlx::query("SELECT vars FROM api_environments WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| crate::ApiError::Invalid(format!("environment not found: {id}")))?;
    let previous_vars = parse_vars(previous.get("vars"));

    let to_store = persist_vars(secrets, vars).await?;

    // A variable that existed before but is gone from the new list (the
    // user deleted its row in the editor) leaves no trace in `to_store` for
    // `persist_vars` to have cleaned up — remove its vault entry here.
    let kept_ids: std::collections::HashSet<&str> =
        to_store.iter().map(|v| v.id.as_str()).collect();
    for old in &previous_vars {
        if old.secret && !kept_ids.contains(old.id.as_str()) {
            let _ = secrets.delete(&vault_key(&old.id)).await;
        }
    }

    let vars_json =
        serde_json::to_string(&to_store).map_err(|e| crate::ApiError::Invalid(e.to_string()))?;
    let updated_at = now_ms();
    sqlx::query("UPDATE api_environments SET name = ?1, vars = ?2, updated_at = ?3 WHERE id = ?4")
        .bind(name)
        .bind(&vars_json)
        .bind(updated_at)
        .bind(id)
        .execute(pool)
        .await?;
    let active: bool = sqlx::query("SELECT is_active FROM api_environments WHERE id = ?1")
        .bind(id)
        .fetch_one(pool)
        .await
        .map(|row| row.get::<i64, _>("is_active") != 0)?;
    Ok(ApiEnvironment {
        id: id.to_string(),
        name: name.to_string(),
        vars: hydrate(secrets, to_store).await,
        active,
        updated_at,
    })
}

pub async fn delete_environment(
    pool: &SqlitePool,
    secrets: &SecretStore,
    id: &str,
) -> ApiResult<()> {
    let row = sqlx::query("SELECT vars FROM api_environments WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| crate::ApiError::Invalid(format!("environment not found: {id}")))?;
    let vars = parse_vars(row.get("vars"));

    sqlx::query("DELETE FROM api_environments WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;

    for var in vars {
        if var.secret {
            let _ = secrets.delete(&vault_key(&var.id)).await;
        }
    }
    Ok(())
}

pub async fn list_environments(
    pool: &SqlitePool,
    secrets: &SecretStore,
) -> ApiResult<Vec<ApiEnvironment>> {
    let rows = sqlx::query(
        "SELECT id, name, vars, is_active, updated_at FROM api_environments ORDER BY name",
    )
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut env = environment_from_row(row);
        env.vars = hydrate(secrets, env.vars).await;
        out.push(env);
    }
    Ok(out)
}

/// One-time migration for environments created before secret values were
/// vault-backed: any variable still holding a plaintext value in the
/// `vars` JSON despite being marked `secret` gets that value moved into
/// the vault and the stored JSON emptied out. Safe to call on every boot —
/// a variable already migrated has nothing left in its JSON value to move,
/// so a second pass is a no-op for it.
pub async fn migrate_secrets(pool: &SqlitePool, secrets: &SecretStore) -> ApiResult<()> {
    let rows = sqlx::query("SELECT id, vars FROM api_environments")
        .fetch_all(pool)
        .await?;
    for row in &rows {
        let env_id: String = row.get("id");
        let mut vars = parse_vars(row.get("vars"));
        let mut changed = false;
        for var in &mut vars {
            if var.id.is_empty() {
                var.id = new_id();
                changed = true;
            }
            if var.secret && !var.value.is_empty() {
                secrets.set(&vault_key(&var.id), &var.value).await?;
                var.value.clear();
                changed = true;
            }
        }
        if changed {
            let vars_json = serde_json::to_string(&vars)
                .map_err(|e| crate::ApiError::Invalid(e.to_string()))?;
            sqlx::query("UPDATE api_environments SET vars = ?1 WHERE id = ?2")
                .bind(&vars_json)
                .bind(&env_id)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

/// Make `id` the active environment, or clear the active environment
/// entirely when `id` is `None`. "At most one active row" is a transaction
/// (clear every row, then set the target), not a database constraint,
/// because SQLite has no partial-unique-index shorthand for "at most one
/// row where this column is true" simple enough to be worth reaching for
/// here — the invariant is small enough that a transaction enforces it just
/// as reliably, and a local desktop app has no concurrent writer to race.
pub async fn set_active_environment(pool: &SqlitePool, id: Option<&str>) -> ApiResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE api_environments SET is_active = 0")
        .execute(&mut *tx)
        .await?;
    if let Some(id) = id {
        let result = sqlx::query("UPDATE api_environments SET is_active = 1 WHERE id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            // Roll back rather than leave every environment inactive because
            // the one the caller asked for does not exist.
            tx.rollback().await?;
            return Err(crate::ApiError::Invalid(format!(
                "environment not found: {id}"
            )));
        }
    }
    tx.commit().await?;
    Ok(())
}

/// The environment `api_send` should resolve `{{VAR}}` placeholders
/// against, or `None` when nothing is active — in which case a request
/// containing placeholders is sent exactly as typed, literal braces and
/// all, which is the same "leave it visible rather than guess" choice
/// [`crate::substitute`] makes for an unrecognized key.
pub async fn active_environment(
    pool: &SqlitePool,
    secrets: &SecretStore,
) -> ApiResult<Option<ApiEnvironment>> {
    let row = sqlx::query(
        "SELECT id, name, vars, is_active, updated_at FROM api_environments WHERE is_active = 1",
    )
    .fetch_optional(pool)
    .await?;
    let Some(mut env) = row.map(|r| environment_from_row(&r)) else {
        return Ok(None);
    };
    env.vars = hydrate(secrets, env.vars).await;
    Ok(Some(env))
}

pub async fn save_request(
    pool: &SqlitePool,
    name: &str,
    collection: &str,
    spec: &ApiRequestSpec,
) -> ApiResult<SavedRequest> {
    let name = name.trim();
    if name.is_empty() {
        return Err(crate::ApiError::Invalid("request name is empty".into()));
    }
    let collection = if collection.trim().is_empty() {
        "Default"
    } else {
        collection.trim()
    };
    let saved = SavedRequest {
        id: new_id(),
        name: name.to_string(),
        collection: collection.to_string(),
        method: spec.method.to_uppercase(),
        url: spec.url.clone(),
        headers: spec.headers.clone(),
        body: spec.body.clone(),
        updated_at: now_ms(),
    };
    let headers_json = serde_json::to_string(&saved.headers)
        .map_err(|e| crate::ApiError::Invalid(e.to_string()))?;
    sqlx::query(
        "INSERT INTO api_requests (id, name, collection, method, url, headers, body, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(&saved.id)
    .bind(&saved.name)
    .bind(&saved.collection)
    .bind(&saved.method)
    .bind(&saved.url)
    .bind(&headers_json)
    .bind(&saved.body)
    .bind(saved.updated_at)
    .execute(pool)
    .await?;
    Ok(saved)
}

pub async fn list_requests(pool: &SqlitePool) -> ApiResult<Vec<SavedRequest>> {
    let rows = sqlx::query(
        "SELECT id, name, collection, method, url, headers, body, updated_at
         FROM api_requests ORDER BY collection, name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|row| SavedRequest {
            id: row.get("id"),
            name: row.get("name"),
            collection: row.get("collection"),
            method: row.get("method"),
            url: row.get("url"),
            headers: serde_json::from_str::<Vec<ApiHeader>>(row.get("headers")).unwrap_or_default(),
            body: row.get("body"),
            updated_at: row.get("updated_at"),
        })
        .collect())
}

pub async fn delete_request(pool: &SqlitePool, id: &str) -> ApiResult<()> {
    let result = sqlx::query("DELETE FROM api_requests WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(crate::ApiError::Invalid(format!("request not found: {id}")));
    }
    Ok(())
}

/// Record a sent request; prunes history beyond the cap.
pub async fn record_history(
    pool: &SqlitePool,
    method: &str,
    url: &str,
    status: u16,
    duration_ms: i64,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO api_history (id, method, url, status, duration_ms, sent_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(new_id())
    .bind(method.to_uppercase())
    .bind(url)
    .bind(status as i64)
    .bind(duration_ms)
    .bind(now_ms())
    .execute(pool)
    .await?;
    sqlx::query(
        "DELETE FROM api_history WHERE id NOT IN (
            SELECT id FROM api_history ORDER BY sent_at DESC, id DESC LIMIT ?1
        )",
    )
    .bind(HISTORY_CAP)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_history(pool: &SqlitePool, limit: i64) -> ApiResult<Vec<ApiHistoryEntry>> {
    let rows = sqlx::query(
        "SELECT id, method, url, status, duration_ms, sent_at
         FROM api_history ORDER BY sent_at DESC, id DESC LIMIT ?1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|row| ApiHistoryEntry {
            id: row.get("id"),
            method: row.get("method"),
            url: row.get("url"),
            status: row.get::<i64, _>("status") as u16,
            duration_ms: row.get("duration_ms"),
            sent_at: row.get("sent_at"),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    async fn test_pool() -> (tempfile::TempDir, SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("api.db"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        init(&pool).await.unwrap();
        (dir, pool)
    }

    async fn test_secrets(pool: &SqlitePool) -> SecretStore {
        SecretStore::with_key(pool.clone(), [9u8; 32])
            .await
            .unwrap()
    }

    /// The raw `vars` JSON as stored on disk — used to assert a secret
    /// value never lands there in plaintext.
    async fn raw_vars_json(pool: &SqlitePool, env_id: &str) -> String {
        sqlx::query("SELECT vars FROM api_environments WHERE id = ?1")
            .bind(env_id)
            .fetch_one(pool)
            .await
            .unwrap()
            .get("vars")
    }

    fn spec(url: &str) -> ApiRequestSpec {
        ApiRequestSpec {
            method: "get".into(),
            url: url.into(),
            headers: vec![ApiHeader {
                name: "accept".into(),
                value: "application/json".into(),
            }],
            body: None,
        }
    }

    #[tokio::test]
    async fn save_list_delete_roundtrip() {
        let (_dir, pool) = test_pool().await;
        let saved = save_request(&pool, "List users", "  ", &spec("http://x/users"))
            .await
            .unwrap();
        assert_eq!(saved.collection, "Default", "blank collection defaults");
        assert_eq!(saved.method, "GET", "method normalized");

        save_request(&pool, "Create user", "Users API", &spec("http://x/users"))
            .await
            .unwrap();

        let listed = list_requests(&pool).await.unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].headers.len(), 1, "headers survive JSON roundtrip");

        assert!(save_request(&pool, "  ", "c", &spec("http://x"))
            .await
            .is_err());

        delete_request(&pool, &saved.id).await.unwrap();
        assert!(delete_request(&pool, &saved.id).await.is_err());
        assert_eq!(list_requests(&pool).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn history_records_and_prunes() {
        let (_dir, pool) = test_pool().await;
        for i in 0..110 {
            record_history(&pool, "get", &format!("http://x/{i}"), 200, 5)
                .await
                .unwrap();
        }
        let history = list_history(&pool, 200).await.unwrap();
        assert_eq!(history.len(), 100, "history capped");
        assert_eq!(history[0].url, "http://x/109", "newest first");
        assert_eq!(history[0].method, "GET");
    }

    fn env_var(key: &str, value: &str, secret: bool) -> ApiEnvVar {
        ApiEnvVar {
            id: String::new(),
            key: key.into(),
            value: value.into(),
            secret,
        }
    }

    #[tokio::test]
    async fn environment_create_list_update_delete_roundtrip() {
        let (_dir, pool) = test_pool().await;
        let secrets = test_secrets(&pool).await;
        let env = create_environment(&pool, "Local").await.unwrap();
        assert_eq!(env.name, "Local");
        assert!(env.vars.is_empty());
        assert!(!env.active);

        assert!(
            create_environment(&pool, "   ").await.is_err(),
            "blank name refused"
        );

        let vars = vec![
            env_var("API_URL", "http://localhost:3000", false),
            env_var("TOKEN", "dev-secret", true),
        ];
        let updated = update_environment(&pool, &secrets, &env.id, "Local (renamed)", &vars)
            .await
            .unwrap();
        assert_eq!(updated.name, "Local (renamed)");
        assert_eq!(updated.vars.len(), 2);
        assert!(
            updated.vars[1].secret,
            "the secret flag survives the JSON roundtrip"
        );
        assert_eq!(
            updated.vars[1].value, "dev-secret",
            "the real secret value is hydrated back for the caller"
        );

        let listed = list_environments(&pool, &secrets).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].vars[0].value, "http://localhost:3000");
        assert_eq!(
            listed[0].vars[1].value, "dev-secret",
            "list_environments hydrates secrets too, not just update_environment's response"
        );

        assert!(update_environment(&pool, &secrets, "missing", "x", &[])
            .await
            .is_err());

        delete_environment(&pool, &secrets, &env.id).await.unwrap();
        assert!(delete_environment(&pool, &secrets, &env.id).await.is_err());
        assert!(list_environments(&pool, &secrets).await.unwrap().is_empty());
    }

    /// The whole point of SEC-105: a secret variable's value must never sit
    /// in `api_environments.vars` in plaintext, even though every read-path
    /// caller still sees the real value.
    #[tokio::test]
    async fn secret_values_are_never_stored_in_the_plaintext_vars_column() {
        let (_dir, pool) = test_pool().await;
        let secrets = test_secrets(&pool).await;
        let env = create_environment(&pool, "Local").await.unwrap();

        update_environment(
            &pool,
            &secrets,
            &env.id,
            "Local",
            &[env_var("TOKEN", "sk-do-not-leak", true)],
        )
        .await
        .unwrap();

        let raw = raw_vars_json(&pool, &env.id).await;
        assert!(
            !raw.contains("sk-do-not-leak"),
            "secret value leaked into the plaintext column: {raw}"
        );
    }

    /// Vault entries are keyed by the variable's stable `id`, not its
    /// `key` — renaming a secret variable must not orphan its vault entry
    /// or lose the value.
    #[tokio::test]
    async fn renaming_a_secret_variable_keeps_its_vault_entry() {
        let (_dir, pool) = test_pool().await;
        let secrets = test_secrets(&pool).await;
        let env = create_environment(&pool, "Local").await.unwrap();

        let created = update_environment(
            &pool,
            &secrets,
            &env.id,
            "Local",
            &[env_var("TOKEN", "sk-abc", true)],
        )
        .await
        .unwrap();
        let stable_id = created.vars[0].id.clone();
        assert!(!stable_id.is_empty(), "a var gets a real id on first save");

        let mut renamed = created.vars[0].clone();
        renamed.key = "API_TOKEN".into();
        let updated = update_environment(&pool, &secrets, &env.id, "Local", &[renamed])
            .await
            .unwrap();

        assert_eq!(updated.vars[0].id, stable_id, "id survives the rename");
        assert_eq!(updated.vars[0].value, "sk-abc", "value survives the rename");
    }

    /// Deleting a variable from an environment — or the environment itself
    /// — must not leave its vault entry behind forever.
    #[tokio::test]
    async fn removing_a_secret_variable_deletes_its_vault_entry() {
        let (_dir, pool) = test_pool().await;
        let secrets = test_secrets(&pool).await;
        let env = create_environment(&pool, "Local").await.unwrap();

        let created = update_environment(
            &pool,
            &secrets,
            &env.id,
            "Local",
            &[env_var("TOKEN", "sk-abc", true)],
        )
        .await
        .unwrap();
        let vault_id = created.vars[0].id.clone();
        assert!(secrets.get(&vault_key(&vault_id)).await.unwrap().is_some());

        // Removing the variable from the list entirely.
        update_environment(&pool, &secrets, &env.id, "Local", &[])
            .await
            .unwrap();
        assert!(
            secrets.get(&vault_key(&vault_id)).await.unwrap().is_none(),
            "vault entry should be gone once the variable is removed"
        );
    }

    #[tokio::test]
    async fn deleting_an_environment_deletes_its_secret_vault_entries() {
        let (_dir, pool) = test_pool().await;
        let secrets = test_secrets(&pool).await;
        let env = create_environment(&pool, "Local").await.unwrap();

        let created = update_environment(
            &pool,
            &secrets,
            &env.id,
            "Local",
            &[env_var("TOKEN", "sk-abc", true)],
        )
        .await
        .unwrap();
        let vault_id = created.vars[0].id.clone();

        delete_environment(&pool, &secrets, &env.id).await.unwrap();

        assert!(secrets.get(&vault_key(&vault_id)).await.unwrap().is_none());
    }

    /// Toggling `secret` off must move the value back out of the vault and
    /// into plaintext, and clean up the now-unused vault entry.
    #[tokio::test]
    async fn toggling_secret_off_moves_the_value_back_to_plaintext() {
        let (_dir, pool) = test_pool().await;
        let secrets = test_secrets(&pool).await;
        let env = create_environment(&pool, "Local").await.unwrap();

        let created = update_environment(
            &pool,
            &secrets,
            &env.id,
            "Local",
            &[env_var("TOKEN", "sk-abc", true)],
        )
        .await
        .unwrap();
        let vault_id = created.vars[0].id.clone();

        let mut unmasked = created.vars[0].clone();
        unmasked.secret = false;
        let updated = update_environment(&pool, &secrets, &env.id, "Local", &[unmasked])
            .await
            .unwrap();

        assert_eq!(updated.vars[0].value, "sk-abc");
        let raw = raw_vars_json(&pool, &env.id).await;
        assert!(
            raw.contains("sk-abc"),
            "value should be back in plaintext JSON once unmasked: {raw}"
        );
        assert!(
            secrets.get(&vault_key(&vault_id)).await.unwrap().is_none(),
            "the old vault entry should be cleaned up"
        );
    }

    /// A pre-existing row from before secrets were vault-backed — no `id`
    /// on its vars, a real plaintext value sitting in a `secret: true`
    /// var — gets fixed up in place the first time `migrate_secrets` runs,
    /// and is a no-op the second time.
    #[tokio::test]
    async fn migrate_secrets_moves_legacy_plaintext_values_into_the_vault() {
        let (_dir, pool) = test_pool().await;
        let secrets = test_secrets(&pool).await;

        // Simulate a row written before the `id` field existed: raw SQL,
        // old-shape JSON, real plaintext secret value.
        sqlx::query(
            "INSERT INTO api_environments (id, name, vars, is_active, updated_at)
             VALUES (?1, ?2, ?3, 0, ?4)",
        )
        .bind("legacy-env")
        .bind("Legacy")
        .bind(r#"[{"key":"TOKEN","value":"sk-legacy","secret":true}]"#)
        .bind(now_ms())
        .execute(&pool)
        .await
        .unwrap();

        migrate_secrets(&pool, &secrets).await.unwrap();

        let raw = raw_vars_json(&pool, "legacy-env").await;
        assert!(
            !raw.contains("sk-legacy"),
            "migration must not leave the plaintext value behind: {raw}"
        );

        let listed = list_environments(&pool, &secrets).await.unwrap();
        assert_eq!(listed[0].vars[0].value, "sk-legacy", "hydrated back");
        assert!(!listed[0].vars[0].id.is_empty(), "assigned a stable id");

        // Running it again must not error and must not change anything —
        // the value is already gone from the JSON, nothing left to move.
        migrate_secrets(&pool, &secrets).await.unwrap();
        let listed_again = list_environments(&pool, &secrets).await.unwrap();
        assert_eq!(listed_again[0].vars[0].id, listed[0].vars[0].id);
    }

    #[tokio::test]
    async fn at_most_one_environment_is_ever_active() {
        let (_dir, pool) = test_pool().await;
        let secrets = test_secrets(&pool).await;
        let a = create_environment(&pool, "Local").await.unwrap();
        let b = create_environment(&pool, "Production").await.unwrap();

        assert!(active_environment(&pool, &secrets).await.unwrap().is_none());

        set_active_environment(&pool, Some(&a.id)).await.unwrap();
        assert_eq!(
            active_environment(&pool, &secrets)
                .await
                .unwrap()
                .unwrap()
                .id,
            a.id
        );

        // Switching active environments must not leave both marked active.
        set_active_environment(&pool, Some(&b.id)).await.unwrap();
        let active = active_environment(&pool, &secrets).await.unwrap().unwrap();
        assert_eq!(active.id, b.id);
        let all = list_environments(&pool, &secrets).await.unwrap();
        assert_eq!(all.iter().filter(|e| e.active).count(), 1);

        set_active_environment(&pool, None).await.unwrap();
        assert!(active_environment(&pool, &secrets).await.unwrap().is_none());
    }

    /// Asking to activate an environment that does not exist must change
    /// nothing — not even clear whatever was already active — rather than
    /// leaving the app with no active environment because of a typo'd id.
    #[tokio::test]
    async fn activating_an_unknown_environment_leaves_the_previous_one_active() {
        let (_dir, pool) = test_pool().await;
        let secrets = test_secrets(&pool).await;
        let a = create_environment(&pool, "Local").await.unwrap();
        set_active_environment(&pool, Some(&a.id)).await.unwrap();

        let result = set_active_environment(&pool, Some("does-not-exist")).await;
        assert!(result.is_err());

        assert_eq!(
            active_environment(&pool, &secrets)
                .await
                .unwrap()
                .unwrap()
                .id,
            a.id
        );
    }
}
