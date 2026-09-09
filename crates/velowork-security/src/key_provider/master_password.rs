//! Enhanced 模式 KeyProvider：主密码经 Argon2id 派生 MasterKey，加密 DEK 后存 `security_config`。

use velowork_core::storage::Database;

use crate::crypto::{default_kdf_parameters, derive_master_key, generate_salt, AesGcmProvider, CryptoProvider, KdfParameters};
use crate::error::{SecurityError, Result};
use crate::key_provider::{pack_blob, unpack_blob, DataKey, KeyCapabilities, KeyProvider, SecurityMode};
use crate::repository::{load_config, now_iso, save_config, SecurityConfig};

/// 验证主密码用的已知测试块（经 MasterKey 加密存为 `verifier`）。
const TEST_BLOCK: &[u8] = b"velowork-security-verifier";

pub struct MasterPasswordProvider {
    unlocked: bool,
}

impl Default for MasterPasswordProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MasterPasswordProvider {
    pub fn new() -> Self {
        Self { unlocked: false }
    }

    /// 首次设置主密码：用调用方提供的 DEK（或新生成），派生 MasterKey，加密 DEK 与 verifier，存 `security_config`。
    pub fn setup(&mut self, db: &Database, secret: &str, dek: &DataKey) -> Result<()> {
        let salt = generate_salt();
        let params = default_kdf_parameters();
        let master = derive_master_key(secret, &salt, &params)?;
        let mk = DataKey::new(*master);
        let provider = AesGcmProvider;
        let dek_blob = provider
            .encrypt(&mk, &dek.0[..], b"dek")
            .map_err(|e| SecurityError::crypto(format!("encrypt dek: {e}")))?;
        let verifier_blob = provider
            .encrypt(&mk, TEST_BLOCK, b"verifier")
            .map_err(|e| SecurityError::crypto(format!("encrypt verifier: {e}")))?;
        let now = now_iso();
        let cfg = SecurityConfig {
            mode: SecurityMode::Enhanced.to_i32(),
            kdf_algorithm: "Argon2id".into(),
            kdf_parameters: params.to_json(),
            salt: salt.to_vec(),
            verifier: pack_blob(&verifier_blob),
            encrypted_dek: pack_blob(&dek_blob),
            created_at: now.clone(),
            updated_at: now,
        };
        save_config(db, &cfg)?;
        self.unlocked = true;
        Ok(())
    }
}

impl KeyProvider for MasterPasswordProvider {
    fn mode(&self) -> SecurityMode {
        SecurityMode::Enhanced
    }

    fn capability(&self) -> KeyCapabilities {
        KeyCapabilities {
            supports_rotation: true,
            supports_unlock: true,
            supports_export: true,
            supports_hardware_key: false,
        }
    }

    fn initialize(&mut self, _db: &Database) -> Result<()> {
        Ok(())
    }

    fn unlock(&mut self, db: &Database, secret: &str) -> Result<DataKey> {
        if secret.is_empty() {
            return Err(SecurityError::AuthFailed);
        }
        let cfg = load_config(db)?
            .ok_or_else(|| SecurityError::key_provider("master password is not set up"))?;
        let params = KdfParameters::from_json(&cfg.kdf_parameters)?;
        let master = derive_master_key(secret, &cfg.salt, &params)?;
        let mk = DataKey::new(*master);
        let provider = AesGcmProvider;

        let dek_blob = unpack_blob(&cfg.encrypted_dek)?;
        let dek_bytes = provider
            .decrypt(&mk, &dek_blob, b"dek")
            .map_err(|_| SecurityError::AuthFailed)?;

        let verifier_blob = unpack_blob(&cfg.verifier)?;
        let vt = provider
            .decrypt(&mk, &verifier_blob, b"verifier")
            .map_err(|_| SecurityError::AuthFailed)?;
        if &vt[..] != TEST_BLOCK {
            return Err(SecurityError::AuthFailed);
        }

        let mut raw = [0u8; 32];
        raw.copy_from_slice(&dek_bytes);
        self.unlocked = true;
        Ok(DataKey::new(raw))
    }

    fn rotate(&mut self, db: &Database, old_secret: &str, new_secret: &str) -> Result<()> {
        if old_secret.is_empty() || new_secret.is_empty() {
            return Err(SecurityError::AuthFailed);
        }
        let mut cfg = load_config(db)?
            .ok_or_else(|| SecurityError::key_provider("master password is not set up"))?;
        let params = KdfParameters::from_json(&cfg.kdf_parameters)?;

        let old_master = derive_master_key(old_secret, &cfg.salt, &params)?;
        let old_mk = DataKey::new(*old_master);
        let provider = AesGcmProvider;

        let dek_blob = unpack_blob(&cfg.encrypted_dek)?;
        let dek_bytes = provider
            .decrypt(&old_mk, &dek_blob, b"dek")
            .map_err(|_| SecurityError::AuthFailed)?;

        let new_master = derive_master_key(new_secret, &cfg.salt, &params)?;
        let new_mk = DataKey::new(*new_master);
        let new_dek_blob = provider
            .encrypt(&new_mk, &dek_bytes[..], b"dek")
            .map_err(|e| SecurityError::crypto(format!("re-encrypt dek: {e}")))?;
        let new_verifier_blob = provider
            .encrypt(&new_mk, TEST_BLOCK, b"verifier")
            .map_err(|e| SecurityError::crypto(format!("re-encrypt verifier: {e}")))?;

        cfg.encrypted_dek = pack_blob(&new_dek_blob);
        cfg.verifier = pack_blob(&new_verifier_blob);
        cfg.updated_at = now_iso();
        save_config(db, &cfg)?;
        Ok(())
    }

    fn is_unlocked(&self) -> bool {
        self.unlocked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enhanced_setup_unlock_rotate() {
        let db = Database::open_in_memory().unwrap();
        let mut provider = MasterPasswordProvider::new();
        let dek = DataKey::new([3u8; 32]);
        provider.setup(&db, "pw123", &dek).unwrap();
        assert!(provider.is_unlocked());

        // 重新构造 provider 并解锁
        let mut p2 = MasterPasswordProvider::new();
        let unlocked = p2.unlock(&db, "pw123").unwrap();
        assert_eq!(&unlocked.0[..], &dek.0[..]);

        // 错误密码必须失败
        let mut p3 = MasterPasswordProvider::new();
        assert!(p3.unlock(&db, "wrong").is_err());

        // 轮换主密码
        p2.rotate(&db, "pw123", "newpw").unwrap();
        let mut p4 = MasterPasswordProvider::new();
        let unlocked2 = p4.unlock(&db, "newpw").unwrap();
        assert_eq!(&unlocked2.0[..], &dek.0[..]);

        // 轮换后旧密码失效
        let mut p5 = MasterPasswordProvider::new();
        assert!(p5.unlock(&db, "pw123").is_err());
    }
}
