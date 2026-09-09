//! 凭据生命周期管理（无状态）。
//!
//! `CredentialManager` **不缓存 DEK**：每次操作由调用方（`SecurityService`）经
//! [`crate::context::SecurityContext`] 提供 `DataKey`。内部用 DEK 经 [`crypto::CryptoProvider`]
//! 加密，AAD 由 `profile + id + kind` 派生绑定记录身份防篡改。
//!
//! 业务层永远只接触 [`Credential`]，绝不知 nonce / ciphertext / algorithm。

use crate::context::SecurityContext;
use crate::credential::{
    derive_aad, Credential, EncryptedBlob, SecretKind, SecretRepository, SecretValue,
};
use crate::crypto::CryptoProviderFactory;
use crate::error::Result;
use crate::key_provider::DataKey;

/// 无状态凭据管理器。
pub struct CredentialManager;

impl CredentialManager {
    /// 用运行时 DEK 加密敏感值，返回密文载体。AAD 绑定 `profile + id + kind`。
    pub(crate) fn encrypt(
        ctx: &SecurityContext,
        value: &SecretValue,
        profile: &str,
        id: &str,
        kind: &SecretKind,
    ) -> Result<EncryptedBlob> {
        let dek = ctx.require_dek()?;
        let provider = CryptoProviderFactory::provider_for(CryptoProviderFactory::default_algorithm())?;
        let aad = derive_aad(profile, id, kind);
        let plaintext = value.encode();
        provider.encrypt(dek, &plaintext, &aad)
    }

    /// 用运行时 DEK 解密密文载体，返回业务敏感值。AAD 必须与加密时一致。
    pub(crate) fn decrypt(
        ctx: &SecurityContext,
        blob: &EncryptedBlob,
        profile: &str,
        id: &str,
        kind: &SecretKind,
    ) -> Result<SecretValue> {
        let dek = ctx.require_dek()?;
        let provider = CryptoProviderFactory::provider_for(blob.algorithm)?;
        let aad = derive_aad(profile, id, kind);
        let plaintext = provider.decrypt(dek, blob, &aad)?;
        SecretValue::decode(&plaintext)
    }

    /// 写入一条凭据（内部加密成 blob 落库）。
    pub fn put(
        repo: &dyn SecretRepository,
        ctx: &SecurityContext,
        profile: &str,
        cred: Credential,
    ) -> Result<()> {
        let blob = Self::encrypt(ctx, &cred.value, profile, &cred.id, &cred.kind)?;
        repo.put(
            &cred.id,
            &blob,
            cred.kind.clone(),
            cred.name.as_deref(),
            &cred.metadata,
        )
    }

    /// 读取一条凭据（内部解密返回业务对象）。
    pub fn get(
        repo: &dyn SecretRepository,
        ctx: &SecurityContext,
        profile: &str,
        id: &str,
    ) -> Result<Credential> {
        let blob = repo.get(id)?;
        let (kind, name, metadata) = repo.meta(id)?;
        let value = Self::decrypt(ctx, &blob, profile, id, &kind)?;
        Ok(Credential {
            id: id.to_string(),
            kind,
            name,
            value,
            metadata,
        })
    }

    /// 删除一条凭据。
    pub fn delete(repo: &dyn SecretRepository, id: &str) -> Result<()> {
        repo.delete(id)
    }

    /// 列出凭据（按 kind 过滤可选），内部解密返回业务对象。
    pub fn list(
        repo: &dyn SecretRepository,
        ctx: &SecurityContext,
        profile: &str,
        kind: Option<SecretKind>,
    ) -> Result<Vec<Credential>> {
        let rows = repo.list(kind)?;
        let mut out = Vec::with_capacity(rows.len());
        for (id, k, name, metadata) in rows {
            let blob = repo.get(&id)?;
            let value = Self::decrypt(ctx, &blob, profile, &id, &k)?;
            out.push(Credential {
                id,
                kind: k,
                name,
                value,
                metadata,
            });
        }
        Ok(out)
    }

    /// 轮换 DEK：用旧 DEK 解密全部凭据，再用新 DEK 重新加密落库（AAD 不变）。
    pub fn rotate_dek(
        repo: &dyn SecretRepository,
        old: &DataKey,
        new: &DataKey,
    ) -> Result<()> {
        let rows = repo.list(None)?;
        for (id, kind, name, metadata) in rows {
            let blob = repo.get(&id)?;
            let profile = id.split(':').next().unwrap_or("");
            let aad = derive_aad(profile, &id, &kind);

            let old_provider = CryptoProviderFactory::provider_for(blob.algorithm)?;
            let plaintext = old_provider.decrypt(old, &blob, &aad)?;

            let new_provider =
                CryptoProviderFactory::provider_for(CryptoProviderFactory::default_algorithm())?;
            let new_blob = new_provider.encrypt(new, &plaintext, &aad)?;

            repo.put(&id, &new_blob, kind, name.as_deref(), &metadata)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key_provider::{DataKey, SecurityMode};
    use crate::repository::SqliteSecurityRepository;
    use std::sync::Arc;

    use velowork_core::storage::Database;

    fn make_ctx(dek: [u8; 32]) -> SecurityContext {
        let mut ctx = SecurityContext::new("work", SecurityMode::Standard);
        ctx.set_dek(DataKey::new(dek));
        ctx
    }

    #[test]
    fn manager_put_get_delete_round_trip() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let repo = SqliteSecurityRepository::new(db);
        let ctx = make_ctx([9u8; 32]);

        let cred = Credential {
            id: "work:ssh_password:h1".into(),
            kind: SecretKind::SshPassword,
            name: Some("Host 1".into()),
            value: SecretValue::password("s3cr3t"),
            metadata: serde_json::json!({"host": "h1"}),
        };
        CredentialManager::put(&repo, &ctx, "work", cred).unwrap();

        let got = CredentialManager::get(&repo, &ctx, "work", "work:ssh_password:h1").unwrap();
        assert_eq!(got.value.as_str(), Some("s3cr3t"));
        assert_eq!(got.name.as_deref(), Some("Host 1"));

        // 错误 profile 的 AAD 不一致，解密必须失败
        let other_ctx = make_ctx([9u8; 32]);
        assert!(CredentialManager::get(&repo, &other_ctx, "other", "work:ssh_password:h1").is_err());

        CredentialManager::delete(&repo, "work:ssh_password:h1").unwrap();
        assert!(repo.get("work:ssh_password:h1").is_err());
    }

    #[test]
    fn manager_rotate_dek_rekeys_all() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let repo = SqliteSecurityRepository::new(db);
        let ctx = make_ctx([1u8; 32]);

        for i in 0..3 {
            let cred = Credential {
                id: format!("work:api_key:k{i}"),
                kind: SecretKind::ApiKey,
                name: None,
                value: SecretValue::api_key(format!("key-{i}")),
                metadata: serde_json::Value::Null,
            };
            CredentialManager::put(&repo, &ctx, "work", cred).unwrap();
        }

        // 轮换到新 DEK
        let old = DataKey::new([1u8; 32]);
        let new = DataKey::new([2u8; 32]);
        CredentialManager::rotate_dek(&repo, &old, &new).unwrap();

        // 旧 DEK 上下文应无法解密
        let old_ctx = make_ctx([1u8; 32]);
        assert!(CredentialManager::list(&repo, &old_ctx, "work", None).is_err());

        // 新 DEK 上下文可读
        let new_ctx = make_ctx([2u8; 32]);
        let all = CredentialManager::list(&repo, &new_ctx, "work", None).unwrap();
        assert_eq!(all.len(), 3);
    }
}
