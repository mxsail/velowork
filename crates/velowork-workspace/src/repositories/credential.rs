//! Credential 的 Repository → Application 分层。
//!
//! **重要**：真正的 secret 永远不在 `velowork.db` 中，只存于系统密钥库
//! （`CredentialService`）。本模块只管理 `credential` 元数据表（`id` / `kind` /
//! `name` / `provider` / 时间戳），`id` 即带 Profile 维度的 keyring account
//! `"<profile>:<type>:<id>"`。

use std::sync::Arc;

use anyhow::{Context, Result};
use velowork_core::storage::{Database, rusqlite};
use velowork_security::credential::{Credential, SecretKind, SecretValue};
use velowork_security::service::SecurityService;

use crate::secure_storage::account_for;

use super::now_iso8601;

/// 凭据元数据（不含 secret）。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CredentialMeta {
    pub id: String, // account string "<profile>:<type>:<id>"
    pub profile_id: String,
    pub kind: String, // password | api_key | ssh_key | token | ...
    pub name: Option<String>,
    pub provider: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_used_at: Option<String>,
}

/// 元数据表的纯 CRUD 映射。
pub struct CredentialRepository {
    db: Arc<Database>,
}

impl CredentialRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub fn insert_meta(&self, meta: &CredentialMeta) -> Result<()> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO credential (id, profile_id, kind, name, provider, created_at, updated_at, last_used_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![
                meta.id,
                meta.profile_id,
                meta.kind,
                meta.name,
                meta.provider,
                meta.created_at,
                meta.updated_at,
                meta.last_used_at,
            ],
        )
        .context("insert credential meta")?;
        Ok(())
    }

    pub fn get_meta(&self, id: &str) -> Result<Option<CredentialMeta>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, profile_id, kind, name, provider, created_at, updated_at, last_used_at \
                 FROM credential WHERE id = ?",
            )
            .context("prepare credential get")?;
        let mut rows = stmt.query([id]).context("query credential get")?;
        match rows.next().context("iterate credential get")? {
            Some(row) => Ok(Some(row_to_meta(row)?)),
            None => Ok(None),
        }
    }

    pub fn update_last_used(&self, id: &str) -> Result<()> {
        let conn = self.db.conn();
        conn.execute(
            "UPDATE credential SET last_used_at = ?2, updated_at = ?2 WHERE id = ?1",
            rusqlite::params![id, now_iso8601()],
        )
        .context("update credential last_used")?;
        Ok(())
    }

    pub fn delete_meta(&self, id: &str) -> Result<()> {
        let conn = self.db.conn();
        conn.execute("DELETE FROM credential WHERE id = ?", [id])
            .context("delete credential meta")?;
        Ok(())
    }

    pub fn list_by_profile(&self, profile_id: &str) -> Result<Vec<CredentialMeta>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, profile_id, kind, name, provider, created_at, updated_at, last_used_at \
                 FROM credential WHERE profile_id = ? ORDER BY name",
            )
            .context("prepare credential list")?;
        let rows = stmt
            .query_map([profile_id], |row| row_to_meta(row))
            .context("query credential list")?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.context("map credential row")?);
        }
        Ok(out)
    }
}

fn row_to_meta(row: &rusqlite::Row<'_>) -> rusqlite::Result<CredentialMeta> {
    Ok(CredentialMeta {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        kind: row.get(2)?,
        name: row.get(3)?,
        provider: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        last_used_at: row.get(7)?,
    })
}

/// 用例编排：secret 明文经 `SecurityService`（Level 1 SQLite，DEK 加密）读写，
/// 元数据索引仍落 `credential` 表（供列表/设置展示，保持既有 API 兼容）。
pub struct CredentialApplicationService {
    repo: CredentialRepository,
    security: SecurityService,
}

impl CredentialApplicationService {
    /// 构造。`security` 需已解锁（Standard 模式空密码即可）；
    /// Enhanced 模式需调用方预先用主密码解锁，若未解锁后续读写会因未解锁而失败。
    pub fn new(db: Arc<Database>, mut security: SecurityService) -> Self {
        if security.mode() == velowork_security::key_provider::SecurityMode::Standard && !security.is_unlocked() {
            if let Err(e) = security.unlock("") {
                log::warn!("标准模式凭据服务自动解锁失败: {e}");
            }
        }
        Self {
            repo: CredentialRepository::new(db),
            security,
        }
    }

    pub fn repo(&self) -> &CredentialRepository {
        &self.repo
    }

    /// 存储凭据：secret 明文经 `SecurityService` 加密落 SQLite，元数据索引写入 DB。
    /// 返回 `credential_id`（即带 Profile 维度的 account）。
    pub fn store(
        &self,
        profile: &str,
        kind: &str,
        id: &str,
        secret: &str,
        name: Option<&str>,
    ) -> Result<CredentialMeta> {
        let credential_id = account_for(profile, kind, id);
        // 明文经 SecurityService 加密（DEK）落 SQLite。
        self.security
            .put(Credential {
                id: credential_id.clone(),
                kind: map_kind(kind),
                name: name.map(|s| s.to_string()),
                value: SecretValue::password(secret),
                metadata: serde_json::Value::Null,
            })
            .context("写入凭据到安全服务失败")?;
        let now = now_iso8601();
        let meta = CredentialMeta {
            id: credential_id.clone(),
            profile_id: profile.to_string(),
            kind: kind.to_string(),
            name: name.map(|s| s.to_string()),
            provider: Some("security-service".to_string()),
            created_at: now.clone(),
            updated_at: now,
            last_used_at: None,
        };
        // 若元数据已存在则覆盖（同 account 重新设密）。
        if self.repo.get_meta(&meta.id)?.is_some() {
            self.repo.delete_meta(&meta.id)?;
        }
        self.repo.insert_meta(&meta)?;
        Ok(meta)
    }

    /// 读取凭据明文（同时刷新 last_used）。
    pub fn load_secret(&self, credential_id: &str) -> Result<String> {
        let cred = self.security.get(credential_id).context("读取凭据失败")?;
        let secret = cred
            .value
            .as_str()
            .context("凭据不是字符串类型")?
            .to_string();
        let _ = self.repo.update_last_used(credential_id);
        Ok(secret)
    }

    /// 删除凭据：安全服务与元数据一并清理。
    pub fn delete(&self, credential_id: &str) -> Result<()> {
        let _ = self.security.delete(credential_id);
        self.repo.delete_meta(credential_id)
    }

    pub fn get_meta(&self, credential_id: &str) -> Result<Option<CredentialMeta>> {
        self.repo.get_meta(credential_id)
    }

    pub fn list(&self, profile_id: &str) -> Result<Vec<CredentialMeta>> {
        self.repo.list_by_profile(profile_id)
    }

    /// 导出凭据（metadata + secret），用于同步 Bundle。不刷新 last_used。
    pub fn export_credentials(
        &self,
        profile_id: &str,
    ) -> Result<Vec<(CredentialMeta, String)>> {
        let metas = self.repo.list_by_profile(profile_id)?;
        let mut out = Vec::with_capacity(metas.len());
        for meta in metas {
            let cred = self.security.get(&meta.id)?;
            let secret = cred
                .value
                .as_str()
                .context("凭据不是字符串类型")?
                .to_string();
            out.push((meta, secret));
        }
        Ok(out)
    }
}

/// 将遗留字符串 kind 映射到 `SecretKind`（DB 判别值稳定，未知种类归为 Custom）。
fn map_kind(kind: &str) -> SecretKind {
    match kind {
        "password" | "ssh_password" | "ssh_key" => SecretKind::SshPassword,
        "api_key" => SecretKind::ApiKey,
        "token" | "oauth" => SecretKind::OAuth,
        "git" => SecretKind::Git,
        "webdav" => SecretKind::WebDav,
        "plugin" => SecretKind::Plugin,
        _ => SecretKind::Custom(kind.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure_storage::account_for;
    use velowork_security::service::SecurityService;

    fn svc() -> CredentialApplicationService {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let security = SecurityService::new("velowork-test", "default", db.clone()).unwrap();
        CredentialApplicationService::new(db, security)
    }

    #[test]
    fn store_then_load_secret() {
        let s = svc();
        let meta = s
            .store("default", "api_key", "openai", "sk-secret", Some("OpenAI"))
            .unwrap();
        assert_eq!(meta.kind, "api_key");
        assert_eq!(meta.profile_id, "default");
        // account 格式
        assert_eq!(meta.id, account_for("default", "api_key", "openai"));
        // secret 往返
        let loaded = s.load_secret(&meta.id).unwrap();
        assert_eq!(loaded, "sk-secret");
        // 元数据可查
        assert_eq!(s.list("default").unwrap().len(), 1);
    }

    #[test]
    fn delete_clears_both() {
        let s = svc();
        let meta = s
            .store("default", "password", "host1", "pw", None)
            .unwrap();
        s.delete(&meta.id).unwrap();
        assert!(s.get_meta(&meta.id).unwrap().is_none());
        assert!(s.load_secret(&meta.id).is_err());
    }

    #[test]
    fn re_store_overwrites_secret() {
        let s = svc();
        s.store("default", "api_key", "k", "v1", None).unwrap();
        s.store("default", "api_key", "k", "v2", None).unwrap();
        // 仍只有一条元数据
        assert_eq!(s.list("default").unwrap().len(), 1);
        assert_eq!(s.load_secret(&account_for("default", "api_key", "k")).unwrap(), "v2");
    }
}
