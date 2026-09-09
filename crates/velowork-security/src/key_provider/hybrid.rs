//! Maximum 模式 KeyProvider（HybridProvider）：仅保留接口，V1 不实现。
//!
//! 未来企业版 TPM / YubiKey / Vault 直接实现 `KeyProvider` trait 即可接入，
//! 业务层零改动。

use velowork_core::storage::Database;

use crate::error::{SecurityError, Result};
use crate::key_provider::{DataKey, KeyCapabilities, KeyProvider, SecurityMode};

pub struct HybridProvider;

impl KeyProvider for HybridProvider {
    fn mode(&self) -> SecurityMode {
        SecurityMode::Maximum
    }

    fn capability(&self) -> KeyCapabilities {
        KeyCapabilities {
            supports_rotation: false,
            supports_unlock: false,
            supports_export: false,
            supports_hardware_key: true,
        }
    }

    fn initialize(&mut self, _db: &Database) -> Result<()> {
        Err(SecurityError::unsupported(
            "Maximum/Hybrid mode is not implemented in V1",
        ))
    }

    fn unlock(&mut self, _db: &Database, _secret: &str) -> Result<DataKey> {
        Err(SecurityError::unsupported(
            "Maximum/Hybrid mode is not implemented in V1",
        ))
    }

    fn rotate(&mut self, _db: &Database, _old_secret: &str, _new_secret: &str) -> Result<()> {
        Err(SecurityError::unsupported(
            "Maximum/Hybrid mode is not implemented in V1",
        ))
    }

    fn is_unlocked(&self) -> bool {
        false
    }
}
