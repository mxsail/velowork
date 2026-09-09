//! KeyProvider 抽象：定义数据加密密钥（DEK）的来源与生命周期。
//!
//! - [`KeyringProvider`]（Standard）：DEK 存系统密钥库（Keyring），开箱即用无密码；
//!   无密钥库环境降级为机器绑定 key 加密文件。
//! - [`MasterPasswordProvider`]（Enhanced）：主密码经 Argon2id 派生 MasterKey，加密 DEK 后存 `security_config`。
//! - [`HybridProvider`]（Maximum）：仅保留接口，V1 不实现（方法体返回 unsupported）。

mod hybrid;
mod keyring;
mod master_password;

pub use hybrid::HybridProvider;
pub use keyring::KeyringProvider;
pub use master_password::MasterPasswordProvider;

use std::path::Path;

use velowork_core::storage::Database;
use zeroize::Zeroizing;

use crate::credential::{AlgorithmId, EncryptedBlob};
use crate::error::{SecurityError, Result};

/// 数据加密密钥（DEK）。`Zeroizing` 包裹，退出作用域自动清零。
#[derive(Clone)]
pub struct DataKey(pub Zeroizing<[u8; 32]>);

impl DataKey {
    pub fn new(key: [u8; 32]) -> Self {
        DataKey(Zeroizing::new(key))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0[..]
    }
}

/// 安全模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityMode {
    Standard,
    Enhanced,
    Maximum, // V1 保留，不实现
}

impl SecurityMode {
    pub fn to_i32(self) -> i32 {
        match self {
            SecurityMode::Standard => 0,
            SecurityMode::Enhanced => 1,
            SecurityMode::Maximum => 2,
        }
    }

    pub fn from_i32(v: i32) -> Self {
        match v {
            1 => SecurityMode::Enhanced,
            2 => SecurityMode::Maximum,
            _ => SecurityMode::Standard,
        }
    }
}

/// KeyProvider 能力声明，取代业务层散落 `if mode == Enhanced`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyCapabilities {
    pub supports_rotation: bool,
    pub supports_unlock: bool,
    pub supports_export: bool,
    pub supports_hardware_key: bool,
}

/// 密钥提供方抽象。
///
/// `unlock` 返回运行时 `DataKey`（仅内存缓存，由 `SecurityService` 持有），
/// `CredentialManager` 保持无状态，每次操作由调用方经 `SecurityContext` 提供 `DataKey`。
pub trait KeyProvider: Send + Sync {
    fn mode(&self) -> SecurityMode;
    fn capability(&self) -> KeyCapabilities;
    fn initialize(&mut self, db: &Database) -> Result<()>;
    fn unlock(&mut self, db: &Database, secret: &str) -> Result<DataKey>;
    fn rotate(&mut self, db: &Database, old_secret: &str, new_secret: &str) -> Result<()>;
    fn is_unlocked(&self) -> bool;
}

/// 将密文载体打包为 `nonce(12) | ciphertext` 原始字节（存 security_config 列）。
pub(crate) fn pack_blob(blob: &EncryptedBlob) -> Vec<u8> {
    let mut v = blob.nonce.clone();
    v.extend_from_slice(&blob.ciphertext);
    v
}

/// 从 `nonce(12) | ciphertext` 原始字节还原密文载体。
pub(crate) fn unpack_blob(raw: &[u8]) -> Result<EncryptedBlob> {
    if raw.len() < 12 {
        return Err(SecurityError::invalid("invalid encrypted blob (too short)"));
    }
    let (n, ct) = raw.split_at(12);
    Ok(EncryptedBlob::new(
        AlgorithmId::Aes256Gcm,
        n.to_vec(),
        ct.to_vec(),
    ))
}

/// 机器绑定密钥：首次生成后存于 `config_root/.vault_key`（0o600），后续复用。
/// 用于无密钥库环境下的 DEK 文件降级存储。
pub(crate) fn machine_key(config_root: &Path) -> Result<[u8; 32]> {
    let key_path = config_root.join(".vault_key");
    if key_path.exists() {
        let bytes = std::fs::read(&key_path)
            .map_err(|e| SecurityError::storage(format!("read machine key: {e}")))?;
        if bytes.len() == 32 {
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            return Ok(key);
        }
    }
    let mut key = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut key);
    if let Some(parent) = key_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| SecurityError::storage(format!("create key dir: {e}")))?;
    }
    std::fs::write(&key_path, key)
        .map_err(|e| SecurityError::storage(format!("write machine key: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}

/// 按安全模式构造对应 `KeyProvider`。
pub fn key_provider_for(mode: SecurityMode, app_name: &str) -> Box<dyn KeyProvider> {
    match mode {
        SecurityMode::Standard => Box::new(KeyringProvider::new(app_name)),
        SecurityMode::Enhanced => Box::new(MasterPasswordProvider::new()),
        SecurityMode::Maximum => Box::new(HybridProvider),
    }
}
