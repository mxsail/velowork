//! 统一安全门面 `SecurityService`。
//!
//! 业务层（Workspace / AI / SSH / WebDAV）**永远只调用本门面**，绝不直接接触
//! `CredentialManager` / `KeyProvider` / `CryptoProvider`。所有加密细节（nonce / ciphertext /
//! algorithm / AAD）对业务层不可见；业务层只传 [`Credential`] 业务对象。
//!
//! 所有操作由 [`crate::context::SecurityContext`]（profile_id / mode / dek / unlocked）驱动；
//! 只有本门面持有并缓存运行时 `dek`，[`crate::credential::manager::CredentialManager`]
//! 保持完全无状态。

use std::sync::Arc;
use parking_lot::RwLock;

use velowork_core::storage::Database;

use crate::context::SecurityContext;
use crate::credential::manager::CredentialManager;
use crate::credential::{Credential, SecretKind};
use crate::error::{SecurityError, Result};
use crate::key_provider::{
    key_provider_for, KeyProvider, KeyringProvider, MasterPasswordProvider, SecurityMode,
};
use crate::repository::{
    delete_config, load_config, load_state, save_state, SqliteSecurityRepository,
};

struct SecurityServiceInner {
    app_name: String,
    profile_id: String,
    db: Arc<Database>,
    repo: SqliteSecurityRepository,
    provider: Box<dyn KeyProvider>,
    ctx: SecurityContext,
}

/// 统一安全门面。
#[derive(Clone)]
pub struct SecurityService {
    inner: Arc<RwLock<SecurityServiceInner>>,
}

impl SecurityService {
    /// 构造门面。自动确保 `security_state` 行存在，并按已保存的配置（或默认 Standard）选择 `KeyProvider`。
    pub fn new(app_name: &str, profile_id: &str, db: Arc<Database>) -> Result<Self> {
        // 确保 state 行存在（首次运行创建默认）。
        let _state = load_state(&db)?;
        let mode = match load_config(&db)? {
            Some(cfg) => SecurityMode::from_i32(cfg.mode),
            None => SecurityMode::Standard,
        };
        let provider = key_provider_for(mode, app_name);
        let ctx = SecurityContext::new(profile_id, mode);
        let repo = SqliteSecurityRepository::new(db.clone());
        let inner = SecurityServiceInner {
            app_name: app_name.to_string(),
            profile_id: profile_id.to_string(),
            db,
            repo,
            provider,
            ctx,
        };
        Ok(Self {
            inner: Arc::new(RwLock::new(inner)),
        })
    }

    /// 当前安全模式。
    pub fn mode(&self) -> SecurityMode {
        self.inner.read().ctx.mode
    }

    /// 是否已设置主密码（Enhanced 模式且已配置）。
    pub fn is_master_password_set(&self) -> bool {
        let inner = self.inner.read();
        load_config(&inner.db)
            .map(|c| c.is_some())
            .unwrap_or(false)
    }

    /// 是否已解锁（运行时持有 DEK）。
    pub fn is_unlocked(&self) -> bool {
        self.inner.read().ctx.unlocked
    }

    pub fn profile_id(&self) -> String {
        self.inner.read().profile_id.clone()
    }

    /// 解锁：填充运行时 `dek`。Standard 模式传空 `secret` 即可（DEK 存 Keyring）。
    /// 解锁：填充运行时 `dek`。Standard 模式传空 `secret` 即可（DEK 存 Keyring）。
    /// 若当前已处于解锁状态，直接返回成功。
    pub fn unlock(&mut self, secret: &str) -> Result<()> {
        let mut inner = self.inner.write();
        if inner.ctx.mode == SecurityMode::Standard && inner.ctx.unlocked && secret.is_empty() {
            return Ok(());
        }
        let db = inner.db.clone();
        inner.provider.initialize(&db)?;
        let dek = inner.provider.unlock(&db, secret)?;
        inner.ctx.set_dek(dek);
        let mut st = load_state(&db)?;
        st.last_unlock = Some(crate::repository::now_iso());
        st.failed_attempts = 0;
        st.cooldown_until_unix = 0;
        save_state(&db, &st)?;
        Ok(())
    }

    /// 读取当前锁定冷却状态（失败次数，冷却到期 Unix 时间戳）。
    pub fn get_lock_cooldown_state(&self) -> Result<(u32, u64)> {
        let inner = self.inner.read();
        let st = load_state(&inner.db)?;
        let cooldown = if st.cooldown_until_unix > 0 {
            st.cooldown_until_unix as u64
        } else {
            0
        };
        Ok((st.failed_attempts.max(0) as u32, cooldown))
    }

    /// 记录一次解锁失败并落 SQLite 保存冷却截止 Unix 时间戳。
    pub fn record_failed_unlock_attempt(&mut self, cooldown_secs: u64) -> Result<(u32, u64)> {
        let inner = self.inner.read();
        let mut st = load_state(&inner.db)?;
        st.failed_attempts += 1;
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let until_unix = if cooldown_secs > 0 {
            now_unix + cooldown_secs
        } else {
            0
        };
        st.cooldown_until_unix = until_unix as i64;
        st.updated_at = crate::repository::now_iso();
        save_state(&inner.db, &st)?;
        Ok((st.failed_attempts as u32, until_unix))
    }

    /// 重置解锁失败状态（解锁成功时调用）。
    pub fn reset_lock_cooldown_state(&mut self) -> Result<()> {
        let inner = self.inner.read();
        let mut st = load_state(&inner.db)?;
        st.failed_attempts = 0;
        st.cooldown_until_unix = 0;
        st.updated_at = crate::repository::now_iso();
        save_state(&inner.db, &st)?;
        Ok(())
    }

    /// 锁定：清空运行时 `dek`。
    pub fn lock(&mut self) {
        self.inner.write().ctx.clear();
    }

    /// 写入凭据（内部加密成 blob 落库）。
    pub fn put(&self, cred: Credential) -> Result<()> {
        let inner = self.inner.read();
        CredentialManager::put(&inner.repo, &inner.ctx, &inner.profile_id, cred)
    }

    /// 读取凭据（内部解密返回业务对象）。
    pub fn get(&self, id: &str) -> Result<Credential> {
        let inner = self.inner.read();
        CredentialManager::get(&inner.repo, &inner.ctx, &inner.profile_id, id)
    }

    /// 删除凭据。
    pub fn delete(&self, id: &str) -> Result<()> {
        let inner = self.inner.read();
        CredentialManager::delete(&inner.repo, id)
    }

    /// 列出凭据（按 kind 过滤可选）。
    pub fn list(&self, kind: Option<SecretKind>) -> Result<Vec<Credential>> {
        let inner = self.inner.read();
        CredentialManager::list(&inner.repo, &inner.ctx, &inner.profile_id, kind)
    }

    /// 轮换主密码（仅 Enhanced 模式；DEK 不变，仅重加密其外层包裹）。
    pub fn rotate_master_password(&mut self, old: &str, new: &str) -> Result<()> {
        let mut inner = self.inner.write();
        let db = inner.db.clone();
        inner.provider.rotate(&db, old, new)?;
        Ok(())
    }

    /// 设置主密码（Standard → Enhanced）。需当前 DEK 可用（未解锁时以 Standard 空密码解锁取 DEK）。
    pub fn set_master_password(&mut self, secret: &str) -> Result<()> {
        let mut inner = self.inner.write();
        let db = inner.db.clone();
        if !inner.ctx.unlocked {
            // 当前应为 Standard；以空密码解锁取 DEK。
            inner.provider.initialize(&db)?;
            let dek = inner.provider.unlock(&db, "")?;
            inner.ctx.set_dek(dek);
        }
        let dek = inner.ctx.require_dek()?.clone();
        let mut mp = MasterPasswordProvider::new();
        mp.setup(&db, secret, &dek)?;
        inner.provider = key_provider_for(SecurityMode::Enhanced, &inner.app_name);
        inner.ctx.mode = SecurityMode::Enhanced;
        inner.ctx.set_dek(dek);
        Ok(())
    }

    /// 关闭主密码（Enhanced → Standard）。将当前 DEK 写入 Keyring，并清除 `security_config`。
    pub fn clear_master_password(&mut self, secret: &str) -> Result<()> {
        let mut inner = self.inner.write();
        let db = inner.db.clone();
        let dek = inner.provider.unlock(&db, secret)?;
        let kp = KeyringProvider::new(&inner.app_name);
        kp.store_dek(&dek)?;
        delete_config(&db)?;
        inner.provider = key_provider_for(SecurityMode::Standard, &inner.app_name);
        inner.ctx.mode = SecurityMode::Standard;
        inner.ctx.set_dek(dek);
        Ok(())
    }

    /// 切换安全模式（Maximum 在 V1 不支持）。
    pub fn change_mode(&mut self, mode: SecurityMode, secret: &str) -> Result<()> {
        match mode {
            SecurityMode::Enhanced => self.set_master_password(secret),
            SecurityMode::Standard => self.clear_master_password(secret),
            SecurityMode::Maximum => Err(SecurityError::unsupported(
                "Maximum/Hybrid mode is not implemented in V1",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential::SecretValue;
    use std::sync::Arc;

    use velowork_core::storage::Database;

    fn make_service() -> SecurityService {
        let db = Arc::new(Database::open_in_memory().unwrap());
        SecurityService::new("velowork-test", "work", db).unwrap()
    }

    /// 在共享同一 DB 的多个门面实例间模拟"重启"：配置与凭据持久化于 DB，
    /// 运行时 DEK 仅存于内存，需重新解锁。
    fn make_service_on(db: Arc<Database>) -> SecurityService {
        SecurityService::new("velowork-test", "work", db).unwrap()
    }

    #[test]
    fn standard_unlock_put_get_delete() {
        let mut svc = make_service();
        assert_eq!(svc.mode(), SecurityMode::Standard);
        svc.unlock("").unwrap();
        assert!(svc.is_unlocked());

        let cred = Credential {
            id: "work:ssh_password:h1".into(),
            kind: SecretKind::SshPassword,
            name: Some("H1".into()),
            value: SecretValue::password("pw"),
            metadata: serde_json::Value::Null,
        };
        svc.put(cred).unwrap();
        let got = svc.get("work:ssh_password:h1").unwrap();
        assert_eq!(got.value.as_str(), Some("pw"));

        let list = svc.list(Some(SecretKind::SshPassword)).unwrap();
        assert_eq!(list.len(), 1);

        svc.delete("work:ssh_password:h1").unwrap();
        assert!(svc.get("work:ssh_password:h1").is_err());
    }

    #[test]
    fn set_and_unlock_master_password() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let mut svc = make_service_on(db.clone());
        svc.set_master_password("masterpw").unwrap();
        assert_eq!(svc.mode(), SecurityMode::Enhanced);
        assert!(svc.is_master_password_set());

        // 重新构造（模拟重启）：必须凭主密码解锁
        let mut svc2 = make_service_on(db.clone());
        assert!(!svc2.is_unlocked());
        // 错误密码失败
        assert!(svc2.unlock("wrong").is_err());
        // 正确密码成功
        svc2.unlock("masterpw").unwrap();
        assert!(svc2.is_unlocked());

        // 轮换主密码
        svc2.rotate_master_password("masterpw", "newpw").unwrap();
        let mut svc3 = make_service_on(db.clone());
        assert!(svc3.unlock("newpw").is_ok());
        assert!(svc3.unlock("masterpw").is_err());
    }

    #[test]
    fn clear_master_password_returns_to_standard() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let mut svc = make_service_on(db.clone());
        svc.set_master_password("masterpw").unwrap();
        svc.clear_master_password("masterpw").unwrap();
        assert_eq!(svc.mode(), SecurityMode::Standard);
        assert!(!svc.is_master_password_set());
        // 回到 Standard：空密码即可解锁（DEK 在 Keyring）
        svc.unlock("").unwrap();
        assert!(svc.is_unlocked());
    }

    #[test]
    fn lock_clears_dek_and_requires_unlock() {
        let mut svc = make_service();
        svc.unlock("").unwrap();
        svc.put(Credential {
            id: "work:ssh_password:l1".into(),
            kind: SecretKind::SshPassword,
            name: Some("L1".into()),
            value: SecretValue::password("pw"),
            metadata: serde_json::Value::Null,
        })
        .unwrap();

        // 锁定：运行时 DEK 清空。
        svc.lock();
        assert!(!svc.is_unlocked());
        // 未解锁时读取应失败（模拟：旧 Keyring 已可删除，但 SQLite 需重新解锁）。
        assert!(svc.get("work:ssh_password:l1").is_err());

        // 重新解锁后可读（验证 SQLite 密文可在无 Keyring 的情况下还原）。
        svc.unlock("").unwrap();
        let got = svc.get("work:ssh_password:l1").unwrap();
        assert_eq!(got.value.as_str(), Some("pw"));
    }

    #[test]
    fn master_password_rotation_keeps_dek() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let mut svc = make_service_on(db.clone());
        svc.set_master_password("masterpw").unwrap();
        svc.put(Credential {
            id: "work:ssh_password:r1".into(),
            kind: SecretKind::SshPassword,
            name: Some("R1".into()),
            value: SecretValue::password("pw"),
            metadata: serde_json::Value::Null,
        })
        .unwrap();

        // 轮换主密码只重加密外层 DEK 包裹，DEK 与凭据密文不变。
        svc.rotate_master_password("masterpw", "newpw").unwrap();
        svc.lock();
        svc.unlock("newpw").unwrap();
        let got = svc.get("work:ssh_password:r1").unwrap();
        assert_eq!(got.value.as_str(), Some("pw"));
    }
}
