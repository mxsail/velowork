//! Standard 模式 KeyProvider：DEK 存系统密钥库（Keyring），开箱即用无密码。
//!
//! 无密钥库环境（WSL / 无桌面 / 容器）降级为机器绑定 key 加密的本地文件（0o600）。

use std::path::PathBuf;

use base64::Engine;
use keyring::{Entry, Error as KeyringError};
use velowork_core::profiles::config_root;
use velowork_core::storage::Database;

use crate::crypto::{AesGcmProvider, CryptoProvider};
use crate::error::{SecurityError, Result};
use crate::key_provider::{
    machine_key, unpack_blob, DataKey, KeyCapabilities, KeyProvider, SecurityMode,
};

/// Keyring 中存放 DEK 的 account（应用级，V1 单 DEK）。
const DEK_ACCOUNT: &str = "velowork:data-encryption-key";

pub struct KeyringProvider {
    app_name: String,
    config_root: PathBuf,
    unlocked: bool,
}

impl KeyringProvider {
    pub fn new(app_name: &str) -> Self {
        Self {
            app_name: app_name.to_string(),
            config_root: config_root(),
            unlocked: false,
        }
    }

    /// 读取已有 DEK；不存在则生成并持久化（keyring 优先，失败降级文件）。
    fn load_or_create_dek(&self) -> Result<DataKey> {
        if let Ok(Some(key)) = self.read_keyring() {
            return Ok(key);
        }
        if let Ok(Some(key)) = self.read_file() {
            return Ok(key);
        }
        let mut raw = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut raw);
        let key = DataKey::new(raw);
        self.write_dek(&key)?;
        Ok(key)
    }

    /// 将指定 DEK 持久化（keyring 优先，失败降级文件）。供模式切换（Enhanced→Standard）复用。
    pub fn store_dek(&self, dek: &DataKey) -> Result<()> {
        self.write_dek(dek)
    }

    fn write_dek(&self, key: &DataKey) -> Result<()> {
        if self.write_keyring(key).is_err() {
            // keyring 不可用，降级到文件。
            self.write_file(key)?;
        }
        Ok(())
    }

    fn read_keyring(&self) -> Result<Option<DataKey>> {
        let entry = Entry::new(&self.app_name, DEK_ACCOUNT)
            .map_err(|e| SecurityError::key_provider(format!("keyring entry: {e}")))?;
        match entry.get_password() {
            Ok(pw) => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(&pw)
                    .map_err(|e| SecurityError::key_provider(format!("decode dek: {e}")))?;
                if bytes.len() != 32 {
                    return Err(SecurityError::invalid("stored dek has wrong length"));
                }
                let mut raw = [0u8; 32];
                raw.copy_from_slice(&bytes);
                Ok(Some(DataKey::new(raw)))
            }
            Err(KeyringError::NoEntry) => Ok(None),
            Err(e) => Err(SecurityError::key_provider(format!("keyring get: {e}"))),
        }
    }

    fn write_keyring(&self, key: &DataKey) -> Result<()> {
        let entry = Entry::new(&self.app_name, DEK_ACCOUNT)
            .map_err(|e| SecurityError::key_provider(format!("keyring entry: {e}")))?;
        let enc = base64::engine::general_purpose::STANDARD.encode(&key.0[..]);
        entry
            .set_password(&enc)
            .map_err(|e| SecurityError::key_provider(format!("keyring set: {e}")))
    }

    fn file_path(&self) -> PathBuf {
        self.config_root.join(".velowork_dek.enc")
    }

    fn read_file(&self) -> Result<Option<DataKey>> {
        let path = self.file_path();
        if !path.exists() {
            return Ok(None);
        }
        let machine = machine_key(&self.config_root)?;
        let content = std::fs::read(&path)
            .map_err(|e| SecurityError::storage(format!("read dek file: {e}")))?;
        let provider = AesGcmProvider;
        let blob = unpack_blob(&content)?;
        let mk = DataKey::new(machine);
        let dec = provider
            .decrypt(&mk, &blob, b"dek-file")
            .map_err(|_| SecurityError::storage("decrypt dek file failed"))?;
        let mut raw = [0u8; 32];
        raw.copy_from_slice(&dec);
        Ok(Some(DataKey::new(raw)))
    }

    fn write_file(&self, key: &DataKey) -> Result<()> {
        let machine = machine_key(&self.config_root)?;
        let provider = AesGcmProvider;
        let mk = DataKey::new(machine);
        let blob = provider
            .encrypt(&mk, &key.0[..], b"dek-file")
            .map_err(|e| SecurityError::crypto(format!("encrypt dek file: {e}")))?;
        let raw = {
            let mut v = blob.nonce.clone();
            v.extend_from_slice(&blob.ciphertext);
            v
        };
        let path = self.file_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| SecurityError::storage(format!("create dek dir: {e}")))?;
        }
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .map_err(|e| SecurityError::storage(format!("open dek file: {e}")))?;
            file.write_all(&raw)
                .map_err(|e| SecurityError::storage(format!("write dek file: {e}")))?;
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&path, &raw)
                .map_err(|e| SecurityError::storage(format!("write dek file: {e}")))?;
        }
        Ok(())
    }
}

impl KeyProvider for KeyringProvider {
    fn mode(&self) -> SecurityMode {
        SecurityMode::Standard
    }

    fn capability(&self) -> KeyCapabilities {
        KeyCapabilities {
            supports_rotation: false,
            supports_unlock: false,
            supports_export: false,
            supports_hardware_key: false,
        }
    }

    fn initialize(&mut self, _db: &Database) -> Result<()> {
        Ok(())
    }

    fn unlock(&mut self, _db: &Database, _secret: &str) -> Result<DataKey> {
        let dek = self.load_or_create_dek()?;
        self.unlocked = true;
        Ok(dek)
    }

    fn rotate(&mut self, _db: &Database, _old_secret: &str, _new_secret: &str) -> Result<()> {
        Err(SecurityError::unsupported(
            "Standard mode does not support master password rotation",
        ))
    }

    fn is_unlocked(&self) -> bool {
        self.unlocked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_load_or_create_is_stable() {
        let provider = KeyringProvider::new("velowork-test-keyring");
        // 无 keyring 环境下走文件降级；无论哪种，两次 unlock 应得到同一 DEK。
        let k1 = provider.load_or_create_dek().unwrap();
        let k2 = provider.load_or_create_dek().unwrap();
        assert_eq!(&k1.0[..], &k2.0[..]);
    }
}
