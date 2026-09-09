//! 运行时安全上下文 [`SecurityContext`]。
//!
//! 收敛 `profile_id` / `mode` / `dek` / `unlocked` 等运行时状态为统一上下文，驱动所有
//! `encrypt` / `decrypt` / `put` / `get`。未来多 Profile / 多 Workspace / 多 Vault 直接复用此结构。
//!
//! 只有 `SecurityService` 持有并缓存 `dek`；[`crate::credential::manager::CredentialManager`]
//! 完全无状态，每次操作由调用方经 `SecurityContext` 提供 `DataKey`。

use crate::key_provider::{DataKey, SecurityMode};

/// 运行时安全上下文。
pub struct SecurityContext {
    pub profile_id: String,
    pub mode: SecurityMode,
    /// 当前 DataKey（仅 `SecurityService` 缓存；`CredentialManager` 无状态，每次经此提供）。
    pub dek: Option<DataKey>,
    pub unlocked: bool,
}

impl SecurityContext {
    pub fn new(profile_id: &str, mode: SecurityMode) -> Self {
        Self {
            profile_id: profile_id.to_string(),
            mode,
            dek: None,
            unlocked: false,
        }
    }

    /// 注入解锁后的 DataKey（仅内存）。
    pub fn set_dek(&mut self, dek: DataKey) {
        self.dek = Some(dek);
        self.unlocked = true;
    }

    /// 清空运行时密钥（锁定时调用）。
    pub fn clear(&mut self) {
        self.dek = None;
        self.unlocked = false;
    }

    /// 取当前 DataKey；未解锁返回 [`crate::error::SecurityError::NotUnlocked`]。
    pub fn require_dek(&self) -> Result<&DataKey, crate::error::SecurityError> {
        self.dek
            .as_ref()
            .ok_or(crate::error::SecurityError::NotUnlocked)
    }
}
