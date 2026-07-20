//! Encrypted secret store.
//!
//! A 32-byte master key lives in the OS keystore (Windows Credential
//! Manager / macOS Keychain / libsecret). Values are AES-256-GCM encrypted
//! (fresh nonce per write) into SQLite. The database file alone is useless
//! without the OS keystore, and plaintext values are only ever returned by
//! [`SecretStore::get`] — listing exposes names and timestamps, never values.

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use sqlx::{Row, SqlitePool};

const KEYRING_SERVICE: &str = "DevOS";
const KEYRING_USER: &str = "master-key";
const NONCE_LEN: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("os keystore error: {0}")]
    Keystore(String),
    #[error("crypto error")]
    Crypto,
    #[error("secret not found: {0}")]
    NotFound(String),
}

pub type SecretResult<T> = Result<T, SecretError>;

#[derive(Debug, Clone)]
pub struct SecretMeta {
    pub name: String,
    pub updated_at: i64,
}

#[derive(Clone)]
pub struct SecretStore {
    pool: SqlitePool,
    cipher: Aes256Gcm,
}

impl SecretStore {
    /// Open the store using the master key from the OS keystore, creating
    /// the key on first use.
    pub async fn init(pool: SqlitePool) -> SecretResult<Self> {
        let key = load_or_create_master_key()?;
        Self::with_key(pool, key).await
    }

    /// Open with an explicit key (tests, headless environments).
    pub async fn with_key(pool: SqlitePool, key: [u8; 32]) -> SecretResult<Self> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS secrets (
                name       TEXT PRIMARY KEY,
                value      BLOB NOT NULL,
                updated_at INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
        Ok(Self { pool, cipher })
    }

    pub async fn set(&self, name: &str, value: &str) -> SecretResult<()> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, value.as_bytes())
            .map_err(|_| SecretError::Crypto)?;
        let mut blob = nonce.to_vec();
        blob.extend_from_slice(&ciphertext);
        sqlx::query(
            "INSERT INTO secrets (name, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(name) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        )
        .bind(name)
        .bind(blob)
        .bind(chrono::Utc::now().timestamp_millis())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Decrypt one secret. Callers must never log or broadcast the value.
    pub async fn get(&self, name: &str) -> SecretResult<Option<String>> {
        let row = sqlx::query("SELECT value FROM secrets WHERE name = ?1")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(None) };
        let blob: Vec<u8> = row.get("value");
        if blob.len() <= NONCE_LEN {
            return Err(SecretError::Crypto);
        }
        let (nonce, ciphertext) = blob.split_at(NONCE_LEN);
        let plaintext = self
            .cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| SecretError::Crypto)?;
        String::from_utf8(plaintext)
            .map(Some)
            .map_err(|_| SecretError::Crypto)
    }

    /// Names and timestamps only — the redaction boundary for the UI.
    pub async fn list(&self) -> SecretResult<Vec<SecretMeta>> {
        let rows = sqlx::query("SELECT name, updated_at FROM secrets ORDER BY name")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| SecretMeta {
                name: r.get("name"),
                updated_at: r.get("updated_at"),
            })
            .collect())
    }

    pub async fn delete(&self, name: &str) -> SecretResult<()> {
        let result = sqlx::query("DELETE FROM secrets WHERE name = ?1")
            .bind(name)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(SecretError::NotFound(name.to_string()));
        }
        Ok(())
    }
}

fn load_or_create_master_key() -> SecretResult<[u8; 32]> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| SecretError::Keystore(e.to_string()))?;
    match entry.get_password() {
        Ok(encoded) => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| SecretError::Crypto)?;
            bytes.try_into().map_err(|_| SecretError::Crypto)
        }
        Err(keyring::Error::NoEntry) => {
            let mut key = [0u8; 32];
            use rand::RngCore;
            rand::rngs::OsRng.fill_bytes(&mut key);
            let encoded = base64::engine::general_purpose::STANDARD.encode(key);
            entry
                .set_password(&encoded)
                .map_err(|e| SecretError::Keystore(e.to_string()))?;
            tracing::info!("generated new DevOS master key in OS keystore");
            Ok(key)
        }
        Err(e) => Err(SecretError::Keystore(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    async fn test_store(key: [u8; 32]) -> (tempfile::TempDir, SecretStore) {
        let dir = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("s.db"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        let store = SecretStore::with_key(pool, key).await.unwrap();
        (dir, store)
    }

    #[tokio::test]
    async fn set_get_roundtrip_and_overwrite() {
        let (_dir, store) = test_store([7u8; 32]).await;
        assert_eq!(store.get("api_key").await.unwrap(), None);
        store.set("api_key", "sk-first").await.unwrap();
        store.set("api_key", "sk-second").await.unwrap();
        assert_eq!(
            store.get("api_key").await.unwrap().as_deref(),
            Some("sk-second")
        );
    }

    #[tokio::test]
    async fn list_never_exposes_values() {
        let (_dir, store) = test_store([7u8; 32]).await;
        store.set("a", "value-a").await.unwrap();
        let listed = store.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "a");
        // SecretMeta has no value field — compile-time redaction.
    }

    #[tokio::test]
    async fn wrong_key_cannot_decrypt() {
        let dir = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("s.db"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();

        let store = SecretStore::with_key(pool.clone(), [1u8; 32])
            .await
            .unwrap();
        store.set("k", "secret").await.unwrap();

        let wrong = SecretStore::with_key(pool, [2u8; 32]).await.unwrap();
        assert!(matches!(wrong.get("k").await, Err(SecretError::Crypto)));
    }

    #[tokio::test]
    async fn delete_removes_secret() {
        let (_dir, store) = test_store([7u8; 32]).await;
        store.set("gone", "x").await.unwrap();
        store.delete("gone").await.unwrap();
        assert_eq!(store.get("gone").await.unwrap(), None);
        assert!(store.delete("gone").await.is_err());
    }
}
