//! 当前 Profile 的 [`SecurityService`] 访问器（Application 层入口）。
//!
//! `velowork-security` 是安全底座；本模块把"当前 Profile 的数据库 + 应用名"
//! 组合起来，给 UI / 业务层一个统一的构造点。业务层永远只通过
//! [`SecurityService`] 门面读写凭据，绝不直接接触 `CredentialManager` /
//! `KeyProvider` / `CryptoProvider`。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use velowork_core::profiles;
use velowork_core::storage::{database, Database};
use velowork_security::key_provider::SecurityMode;
use velowork_security::service::SecurityService;

/// Keyring 服务名（与 `secure_storage::CREDENTIAL_SERVICE` 保持一致）。
pub const SECURITY_APP_NAME: &str = "velowork";

/// 进程级按 Profile 缓存的 `SecurityService` 门面实例。
static SECURITY_SERVICES: Mutex<Option<HashMap<String, SecurityService>>> = Mutex::new(None);

/// 获取或构造当前 Profile 的 `SecurityService`（进程级单例共享）。
///
/// 优先使用进程级已初始化的数据库句柄；否则回退到直接打开 Profile 的
/// `velowork.db`（打开时会自动应用迁移，含 security 相关 v4 表）。
pub fn current_security_service() -> Result<SecurityService> {
    let profile = profiles::current();
    let mut guard = SECURITY_SERVICES.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    if let Some(svc) = map.get(&profile.id) {
        return Ok(svc.clone());
    }

    let db = database()
        .or_else(|| {
            if profile.database_path().exists() {
                Database::open(&profile.database_path()).ok().map(Arc::new)
            } else {
                None
            }
        })
        .context("no profile database available for security service")?;
    let mut svc = SecurityService::new(SECURITY_APP_NAME, &profile.id, db)
        .map_err(|e| anyhow::anyhow!("security service init failed: {e}"))?;

    // Standard 模式开箱即用无密码自动解锁；Enhanced 模式保持锁定等待主密码
    if svc.mode() == SecurityMode::Standard {
        let _ = svc.unlock("");
    }

    map.insert(profile.id.clone(), svc.clone());
    Ok(svc)
}

/// 读取当前安全模式（`"standard"` / `"enhanced"`），失败时回退 `"standard"`。
pub fn current_security_mode() -> String {
    match current_security_service() {
        Ok(svc) => match svc.mode() {
            SecurityMode::Standard => "standard".to_string(),
            SecurityMode::Enhanced => "enhanced".to_string(),
            SecurityMode::Maximum => "maximum".to_string(),
        },
        Err(_) => "standard".to_string(),
    }
}

/// 当前是否已设置主密码（Enhanced 模式且已配置）。
pub fn is_master_password_set() -> bool {
    current_security_service()
        .map(|svc| svc.is_master_password_set())
        .unwrap_or(false)
}
