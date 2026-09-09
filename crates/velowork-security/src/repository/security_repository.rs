//! 安全配置 / 状态持久化 + 通用 Secret Store（SQLite 后端）。
//!
//! - [`SecurityConfig`] / [`SecurityState`]：安全元数据，状态字段与静态配置解耦。
//! - [`security_credentials`] 表：通用 Secret Store，密文（nonce|ciphertext）落库，DEK 保护。
//!   [`SqliteSecurityRepository`] 实现 [`crate::credential::SecretRepository`]，供
//!   `CredentialManager` 使用。
//!
//! 所有函数以 `&Database` 为参数（无状态），便于 `KeyProvider` 在 `unlock(db, ...)` 中直接调用。

use std::sync::Arc;

use chrono::Utc;
use rusqlite::params;
use velowork_core::storage::Database;

use crate::credential::{
    AlgorithmId, EncryptedBlob, SecretKind, SecretRepository,
};
use crate::error::{SecurityError, Result};

/// 当前 Security Metadata schema 版本（`security_state.schema_version`）。
/// 独立于 `bundle_version` 与 `credential.version`，用于未来 Security 自身升级。
pub const SECURITY_SCHEMA_VERSION: i32 = 1;

/// 解锁策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnlockStrategy {
    Automatic,
    Manual,
    Session,
}

impl UnlockStrategy {
    pub fn to_i32(self) -> i32 {
        match self {
            UnlockStrategy::Automatic => 0,
            UnlockStrategy::Manual => 1,
            UnlockStrategy::Session => 2,
        }
    }
    pub fn from_i32(v: i32) -> Self {
        match v {
            0 => UnlockStrategy::Automatic,
            2 => UnlockStrategy::Session,
            _ => UnlockStrategy::Manual,
        }
    }
}

/// 安全配置（security_config 单条，id 固定为 1）。
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub mode: i32, // 0=Standard 1=Enhanced 2=Maximum(预留)
    pub kdf_algorithm: String,
    pub kdf_parameters: String, // JSON {"m":..,"t":..,"p":..}
    pub salt: Vec<u8>,
    pub verifier: Vec<u8>,
    pub encrypted_dek: Vec<u8>,
    pub created_at: String,
    pub updated_at: String,
}

/// 安全状态（security_state 单条，id 固定为 1）。
#[derive(Debug, Clone)]
pub struct SecurityState {
    pub schema_version: i32,
    pub unlock_strategy: i32,
    pub migrated: bool,
    pub verified: bool,
    pub last_unlock: Option<String>,
    pub failed_attempts: i32,
    pub cooldown_until_unix: i64,
    pub last_rotation: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

fn bool_to_i32(b: bool) -> i32 {
    if b {
        1
    } else {
        0
    }
}

// ===================== security_config =====================

/// 读取安全配置（不存在返回 `None`）。
pub fn load_config(db: &Database) -> Result<Option<SecurityConfig>> {
    let conn = db.conn();
    let mut stmt = conn
        .prepare(
            "SELECT mode, kdf_algorithm, kdf_parameters, salt, verifier, encrypted_dek, created_at, updated_at \
             FROM security_config WHERE id = 1",
        )
        .map_err(|e| SecurityError::storage(format!("prepare load_config: {e}")))?;
    let mut rows = stmt
        .query([])
        .map_err(|e| SecurityError::storage(format!("query load_config: {e}")))?;
    match rows.next().map_err(|e| SecurityError::storage(format!("next load_config: {e}")))? {
        None => Ok(None),
        Some(row) => {
            let cfg = SecurityConfig {
                mode: row.get(0)?,
                kdf_algorithm: row.get(1)?,
                kdf_parameters: row.get(2)?,
                salt: row.get(3)?,
                verifier: row.get(4)?,
                encrypted_dek: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            };
            Ok(Some(cfg))
        }
    }
}

/// 写入（upsert）安全配置。
pub fn save_config(db: &Database, cfg: &SecurityConfig) -> Result<()> {
    let conn = db.conn();
    conn.execute(
        "INSERT INTO security_config \
         (id, mode, kdf_algorithm, kdf_parameters, salt, verifier, encrypted_dek, created_at, updated_at) \
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT(id) DO UPDATE SET \
         mode=?1, kdf_algorithm=?2, kdf_parameters=?3, salt=?4, verifier=?5, \
         encrypted_dek=?6, updated_at=?8",
        params![
            cfg.mode,
            cfg.kdf_algorithm,
            cfg.kdf_parameters,
            cfg.salt,
            cfg.verifier,
            cfg.encrypted_dek,
            cfg.created_at,
            cfg.updated_at,
        ],
    )
    .map_err(|e| SecurityError::storage(format!("save_config: {e}")))?;
    Ok(())
}

/// 删除安全配置（关闭主密码时调用）。
pub fn delete_config(db: &Database) -> Result<()> {
    let conn = db.conn();
    conn.execute("DELETE FROM security_config WHERE id = 1", [])
        .map_err(|e| SecurityError::storage(format!("delete_config: {e}")))?;
    Ok(())
}

// ===================== security_state =====================

/// 读取安全状态（不存在则初始化一条默认记录并返回）。
pub fn load_state(db: &Database) -> Result<SecurityState> {
    // 防御性确保字段存在（兼容部分尚未重新初始化 DB 的已有数据库）
    let _ = db.conn().execute("ALTER TABLE security_state ADD COLUMN cooldown_until_unix INTEGER NOT NULL DEFAULT 0", []);

    // 先读（持锁），释放锁后再按需写入，避免对同一个非重入 Mutex 重复加锁导致死锁。
    let existing = {
        let conn = db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT schema_version, unlock_strategy, migrated, verified, last_unlock, \
                 failed_attempts, cooldown_until_unix, last_rotation, created_at, updated_at \
                 FROM security_state WHERE id = 1",
            )
            .map_err(|e| SecurityError::storage(format!("prepare load_state: {e}")))?;
        let mut rows = stmt
            .query([])
            .map_err(|e| SecurityError::storage(format!("query load_state: {e}")))?;
        let row = rows
            .next()
            .map_err(|e| SecurityError::storage(format!("next load_state: {e}")))?;
        match row {
            Some(row) => Some(SecurityState {
                schema_version: row.get(0)?,
                unlock_strategy: row.get(1)?,
                migrated: row.get(2)?,
                verified: row.get(3)?,
                last_unlock: row.get(4)?,
                failed_attempts: row.get(5)?,
                cooldown_until_unix: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                last_rotation: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            }),
            None => None,
        }
    };
    match existing {
        Some(st) => Ok(st),
        None => {
            let now = now_iso();
            let st = SecurityState {
                schema_version: SECURITY_SCHEMA_VERSION,
                unlock_strategy: UnlockStrategy::Manual.to_i32(),
                migrated: false,
                verified: false,
                last_unlock: None,
                failed_attempts: 0,
                cooldown_until_unix: 0,
                last_rotation: None,
                created_at: now.clone(),
                updated_at: now,
            };
            save_state(db, &st)?;
            Ok(st)
        }
    }
}

/// 写入（upsert）安全状态。
pub fn save_state(db: &Database, st: &SecurityState) -> Result<()> {
    let conn = db.conn();
    conn.execute(
        "INSERT INTO security_state \
         (id, schema_version, unlock_strategy, migrated, verified, last_unlock, \
          failed_attempts, cooldown_until_unix, last_rotation, created_at, updated_at) \
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
         ON CONFLICT(id) DO UPDATE SET \
         schema_version=?1, unlock_strategy=?2, migrated=?3, verified=?4, last_unlock=?5, \
         failed_attempts=?6, cooldown_until_unix=?7, last_rotation=?8, updated_at=?10",
        params![
            st.schema_version,
            st.unlock_strategy,
            bool_to_i32(st.migrated),
            bool_to_i32(st.verified),
            st.last_unlock,
            st.failed_attempts,
            st.cooldown_until_unix,
            st.last_rotation,
            st.created_at,
            st.updated_at,
        ],
    )
    .map_err(|e| SecurityError::storage(format!("save_state: {e}")))?;
    Ok(())
}

// ===================== security_credentials (Secret Store) =====================

/// 写入（upsert）一条凭据密文。
pub fn upsert_credential(
    db: &Database,
    id: &str,
    blob: &EncryptedBlob,
    kind: SecretKind,
    name: Option<&str>,
    metadata: &serde_json::Value,
) -> Result<()> {
    let now = now_iso();
    let metadata_str = serde_json::to_string(metadata)
        .map_err(|e| SecurityError::invalid(format!("metadata serialize: {e}")))?;
    let conn = db.conn();
    conn.execute(
        "INSERT INTO security_credentials \
         (id, kind, name, version, algorithm_id, nonce, ciphertext, metadata, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9) \
         ON CONFLICT(id) DO UPDATE SET \
         kind=?2, name=?3, version=?4, algorithm_id=?5, nonce=?6, ciphertext=?7, \
         metadata=?8, updated_at=?9",
        params![
            id,
            kind.to_db(),
            name,
            blob.version,
            blob.algorithm.to_u8() as i64,
            blob.nonce,
            blob.ciphertext,
            metadata_str,
            now,
        ],
    )
    .map_err(|e| SecurityError::storage(format!("upsert_credential: {e}")))?;
    Ok(())
}

/// 读取一条凭据密文载体。
pub fn get_credential_blob(db: &Database, id: &str) -> Result<EncryptedBlob> {
    let conn = db.conn();
    let mut stmt = conn
        .prepare(
            "SELECT version, algorithm_id, nonce, ciphertext FROM security_credentials WHERE id = ?1",
        )
        .map_err(|e| SecurityError::storage(format!("prepare get_credential: {e}")))?;
    let mut rows = stmt
        .query([id])
        .map_err(|e| SecurityError::storage(format!("query get_credential: {e}")))?;
    let row = rows
        .next()
        .map_err(|e| SecurityError::storage(format!("next get_credential: {e}")))?
        .ok_or_else(|| SecurityError::invalid(format!("credential not found: {id}")))?;
    let version: i32 = row.get(0)?;
    let algorithm_id: i64 = row.get(1)?;
    let nonce: Vec<u8> = row.get(2)?;
    let ciphertext: Vec<u8> = row.get(3)?;
    Ok(EncryptedBlob {
        version,
        algorithm: AlgorithmId::from_u8(algorithm_id)?,
        nonce,
        ciphertext,
    })
}

/// 删除一条凭据。
pub fn delete_credential(db: &Database, id: &str) -> Result<()> {
    let conn = db.conn();
    conn.execute("DELETE FROM security_credentials WHERE id = ?1", [id])
        .map_err(|e| SecurityError::storage(format!("delete_credential: {e}")))?;
    Ok(())
}

/// 列出凭据（按 kind 过滤可选），返回 (id, kind, name, metadata)。
pub fn list_credentials(
    db: &Database,
    kind: Option<SecretKind>,
) -> Result<Vec<crate::credential::CredentialMetaRow>> {
    let conn = db.conn();
    let sql = match kind {
        Some(_) => {
            "SELECT id, kind, name, metadata FROM security_credentials WHERE kind = ?1 ORDER BY id"
        }
        None => "SELECT id, kind, name, metadata FROM security_credentials ORDER BY id",
    };
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| SecurityError::storage(format!("prepare list_credentials: {e}")))?;
    let mut rows = match kind {
        Some(k) => stmt
            .query([k.to_db()])
            .map_err(|e| SecurityError::storage(format!("query list_credentials: {e}")))?,
        None => stmt
            .query([])
            .map_err(|e| SecurityError::storage(format!("query list_credentials: {e}")))?,
    };
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let kind_i: i64 = row.get(1)?;
        let name: Option<String> = row.get(2)?;
        let metadata_str: String = row.get(3)?;
        let metadata: serde_json::Value = serde_json::from_str(&metadata_str)
            .map_err(|e| SecurityError::invalid(format!("metadata parse: {e}")))?;
        out.push((id, SecretKind::from_db(kind_i)?, name, metadata));
    }
    Ok(out)
}

/// SQLite 后端实现 [`SecretRepository`]，供 `CredentialManager` 使用。
pub struct SqliteSecurityRepository {
    db: Arc<Database>,
}

impl SqliteSecurityRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub fn database(&self) -> &Database {
        &self.db
    }
}

impl SecretRepository for SqliteSecurityRepository {
    fn put(
        &self,
        id: &str,
        blob: &EncryptedBlob,
        kind: SecretKind,
        name: Option<&str>,
        metadata: &serde_json::Value,
    ) -> Result<()> {
        upsert_credential(&self.db, id, blob, kind, name, metadata)
    }

    fn get(&self, id: &str) -> Result<EncryptedBlob> {
        get_credential_blob(&self.db, id)
    }

    fn meta(&self, id: &str) -> Result<(SecretKind, Option<String>, serde_json::Value)> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare("SELECT kind, name, metadata FROM security_credentials WHERE id = ?1")
            .map_err(|e| SecurityError::storage(format!("prepare meta: {e}")))?;
        let mut rows = stmt
            .query([id])
            .map_err(|e| SecurityError::storage(format!("query meta: {e}")))?;
        let row = rows
            .next()
            .map_err(|e| SecurityError::storage(format!("next meta: {e}")))?
            .ok_or_else(|| SecurityError::invalid(format!("credential not found: {id}")))?;
        let kind_i: i64 = row.get(0)?;
        let name: Option<String> = row.get(1)?;
        let metadata_str: String = row.get(2)?;
        let metadata: serde_json::Value = serde_json::from_str(&metadata_str)
            .map_err(|e| SecurityError::invalid(format!("metadata parse: {e}")))?;
        Ok((SecretKind::from_db(kind_i)?, name, metadata))
    }

    fn delete(&self, id: &str) -> Result<()> {
        delete_credential(&self.db, id)
    }

    fn list(
        &self,
        kind: Option<SecretKind>,
    ) -> Result<Vec<(String, SecretKind, Option<String>, serde_json::Value)>> {
        list_credentials(&self.db, kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::CryptoProviderFactory;
    use crate::key_provider::DataKey;

    #[test]
    fn config_state_round_trip() {
        let db = Database::open_in_memory().unwrap();
        assert!(load_config(&db).unwrap().is_none());
        let cfg = SecurityConfig {
            mode: 1,
            kdf_algorithm: "Argon2id".into(),
            kdf_parameters: "{\"m\":65536,\"t\":3,\"p\":4}".into(),
            salt: vec![1, 2, 3, 4],
            verifier: vec![5, 6],
            encrypted_dek: vec![7, 8],
            created_at: "now".into(),
            updated_at: "now".into(),
        };
        save_config(&db, &cfg).unwrap();
        let loaded = load_config(&db).unwrap().unwrap();
        assert_eq!(loaded.mode, 1);
        assert_eq!(loaded.salt, vec![1, 2, 3, 4]);

        let mut st = load_state(&db).unwrap();
        assert!(!st.migrated);
        st.migrated = true;
        save_state(&db, &st).unwrap();
        assert!(load_state(&db).unwrap().migrated);
    }

    #[test]
    fn credential_secret_store_round_trip() {
        let db = Database::open_in_memory().unwrap();
        let key = DataKey::new([7u8; 32]);
        let provider = CryptoProviderFactory::provider_for(AlgorithmId::Aes256Gcm).unwrap();
        let aad = crate::credential::derive_aad("work", "work:ssh:h1", &SecretKind::SshPassword);
        let blob = provider.encrypt(&key, b"secret", &aad).unwrap();

        upsert_credential(
            &db,
            "work:ssh:h1",
            &blob,
            SecretKind::SshPassword,
            Some("My Host"),
            &serde_json::json!({"host": "h1"}),
        )
        .unwrap();

        let got = get_credential_blob(&db, "work:ssh:h1").unwrap();
        let dec = provider.decrypt(&key, &got, &aad).unwrap();
        assert_eq!(&dec[..], b"secret");

        let list = list_credentials(&db, Some(SecretKind::SshPassword)).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, "work:ssh:h1");
        assert_eq!(list[0].2.as_deref(), Some("My Host"));

        delete_credential(&db, "work:ssh:h1").unwrap();
        assert!(get_credential_blob(&db, "work:ssh:h1").is_err());
    }
}
