//! 凭据存储抽象（`CredentialProvider`）与实现。
//!
//! - `KeyringProvider`：系统密钥库（keyring，Windows Credential Manager /
//!   macOS Keychain / Linux Secret Service）。
//! - `EncryptedFileProvider`：无密钥库环境（WSL / 无桌面 / 容器）下的持久降级，
//!   凭据以 AES-GCM 加密存于本地 0o600 文件，密钥来自机器绑定 key 文件。
//! - `MemoryProvider`：仅测试 / 无持久化场景。
//!
//! `CredentialService` 仅作 Manager，按 [`account_for`] 拼装带 Profile 维度的
//! account，供上层（SSH / AI / WebDAV）统一调用，不直接接触具体后端。

use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use velowork_core::profiles::config_root;
use velowork_security::credential::{Credential, SecretKind, SecretValue};

/// 凭据存储后端抽象。所有方法返回 `anyhow::Result`，便于上层统一处理。
pub trait CredentialProvider: Send + Sync {
    /// 存储凭据（明文 secret 的字节）。
    fn store(&self, account: &str, secret: &[u8]) -> Result<()>;
    /// 读取凭据。
    fn load(&self, account: &str) -> Result<Vec<u8>>;
    /// 删除凭据。
    fn delete(&self, account: &str) -> Result<()>;
    /// 后端是否可用。
    fn is_available(&self) -> bool;
    /// 后端名称（日志用）。
    fn backend_name(&self) -> &str {
        "unknown"
    }
}

/// 系统密钥库后端（keyring crate）。
pub struct KeyringProvider {
    app_name: String,
}

impl KeyringProvider {
    pub fn new(app_name: &str) -> Self {
        Self {
            app_name: app_name.to_string(),
        }
    }
}

impl CredentialProvider for KeyringProvider {
    fn store(&self, account: &str, secret: &[u8]) -> Result<()> {
        let entry = keyring::Entry::new(&self.app_name, account).context("create keyring entry")?;
        let encoded =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, secret);
        entry.set_password(&encoded).context("set keyring password")
    }

    fn load(&self, account: &str) -> Result<Vec<u8>> {
        let entry = keyring::Entry::new(&self.app_name, account).context("create keyring entry")?;
        let pw = entry.get_password().context("get keyring password")?;
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &pw)
            .context("decode keyring secret")
    }

    fn delete(&self, account: &str) -> Result<()> {
        let entry = keyring::Entry::new(&self.app_name, account).context("create keyring entry")?;
        entry.delete_credential().context("delete keyring credential")
    }

    fn is_available(&self) -> bool {
        keyring::Entry::new(&self.app_name, "test_availability").is_ok()
    }

    fn backend_name(&self) -> &str {
        "keyring"
    }
}

/// 无密钥库环境下的持久降级：AES-256-GCM 加密存于本地 0o600 文件。
///
/// 文件内为 `account -> base64(nonce|ciphertext)` 的 JSON 映射；运行时缓存于
/// 内存，写操作原子落盘（tmp + rename）。密钥由 [`machine_key`] 提供。
pub struct EncryptedFileProvider {
    path: PathBuf,
    key: [u8; 32],
    cache: Mutex<HashMap<String, Vec<u8>>>,
}

impl EncryptedFileProvider {
    pub fn new(path: PathBuf, key: [u8; 32]) -> Self {
        let cache = load_store(&path, &key).unwrap_or_default();
        Self {
            path,
            key,
            cache: Mutex::new(cache),
        }
    }
}

impl CredentialProvider for EncryptedFileProvider {
    fn store(&self, account: &str, secret: &[u8]) -> Result<()> {
        let enc = aes_encrypt(&self.key, secret)?;
        let mut cache = self.cache.lock().unwrap();
        cache.insert(account.to_string(), enc);
        self.persist(&cache)
    }

    fn load(&self, account: &str) -> Result<Vec<u8>> {
        let cache = self.cache.lock().unwrap();
        let enc = cache
            .get(account)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("credential not found: {account}"))?;
        aes_decrypt(&self.key, &enc)
    }

    fn delete(&self, account: &str) -> Result<()> {
        let mut cache = self.cache.lock().unwrap();
        cache.remove(account);
        self.persist(&cache)
    }

    fn is_available(&self) -> bool {
        true
    }

    fn backend_name(&self) -> &str {
        "encrypted-file"
    }
}

impl EncryptedFileProvider {
    fn persist(&self, cache: &HashMap<String, Vec<u8>>) -> Result<()> {
        let map: HashMap<String, String> = cache
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, v),
                )
            })
            .collect();
        let json = serde_json::to_string(&map)?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

fn load_store(path: &Path, _key: &[u8; 32]) -> Result<HashMap<String, Vec<u8>>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let content = std::fs::read_to_string(path)?;
    let map: HashMap<String, String> = serde_json::from_str(&content)?;
    let mut out = HashMap::new();
    for (k, v) in map {
        // 缓存中始终存放密文，读取时再解密（与 `store` 保持一致）。
        if let Ok(enc) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &v) {
            out.insert(k, enc);
        }
    }
    Ok(out)
}

/// 仅测试 / 无持久化场景的内存后端。
pub struct MemoryProvider {
    data: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MemoryProvider {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Default for MemoryProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialProvider for MemoryProvider {
    fn store(&self, account: &str, secret: &[u8]) -> Result<()> {
        self.data
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }

    fn load(&self, account: &str) -> Result<Vec<u8>> {
        self.data
            .lock()
            .unwrap()
            .get(account)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("credential not found: {account}"))
    }

    fn delete(&self, account: &str) -> Result<()> {
        self.data.lock().unwrap().remove(account);
        Ok(())
    }

    fn is_available(&self) -> bool {
        true
    }

    fn backend_name(&self) -> &str {
        "memory"
    }
}

/// 拼装带 Profile 维度的 keyring account：`{profile}:{kind}:{id}`。
pub fn account_for(profile_id: &str, kind: &str, id: &str) -> String {
    format!("{profile_id}:{kind}:{id}")
}

/// 选择凭据后端：keyring → 加密文件（机器绑定 key）→ 内存（兜底）。
pub fn create_credential_provider(app_name: &str) -> Box<dyn CredentialProvider> {
    let keyring = KeyringProvider::new(app_name);
    if keyring.is_available() {
        log::info!("Using keyring credential provider");
        return Box::new(keyring);
    }
    match machine_key() {
        Ok(key) => {
            let path = config_root().join("credentials.enc");
            log::warn!(
                "Keyring unavailable; using encrypted-file credential provider at {}",
                path.display()
            );
            Box::new(EncryptedFileProvider::new(path, key))
        }
        Err(e) => {
            log::warn!(
                "No credential backend available ({}); falling back to in-memory (non-persistent)",
                e
            );
            Box::new(MemoryProvider::new())
        }
    }
}

/// 进程级缓存的凭据后端。首次访问时执行后端选择（含 keyring 的 D-Bus 握手），
/// 之后复用同一实例，避免每次读写凭据都重新建立 D-Bus 连接（会导致 UI 卡顿数秒）。
static CREDENTIAL_PROVIDER: OnceLock<Box<dyn CredentialProvider>> = OnceLock::new();

/// 返回进程级缓存的凭据后端（keyring / 加密文件 / 内存）。
///
/// 首次调用时执行后端选择（含 keyring 的 D-Bus 握手），之后所有调用复用同一实例，
/// 因此 [`prewarm_credential_provider`] 在后台预热后，UI 路径不会再同步触发握手。
pub fn credential_provider() -> &'static Box<dyn CredentialProvider> {
    CREDENTIAL_PROVIDER.get_or_init(|| create_credential_provider("velowork"))
}

/// 在后台线程预热凭据后端（建立 keyring 的 D-Bus 连接），
/// 避免首次打开设置等 UI 路径同步触发握手导致卡顿数秒。
pub fn prewarm_credential_provider() {
    let _ = std::thread::Builder::new()
        .name("credential-prewarm".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let _ = credential_provider();
        });
}

/// 机器绑定密钥：首次生成后存于 `config_root/.vault_key`（0o600），后续复用。
fn machine_key() -> Result<[u8; 32]> {
    let key_path = config_root().join(".vault_key");
    if key_path.exists() {
        let bytes = std::fs::read(&key_path)?;
        if bytes.len() == 32 {
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            return Ok(key);
        }
    }
    let mut key = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut key);
    if let Some(parent) = key_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&key_path, &key)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}

/// AES-256-GCM 加密：输出 `nonce(12) | ciphertext`。
fn aes_encrypt(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>> {
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce};

    let cipher = Aes256Gcm::new_from_slice(key).context("create cipher")?;
    let mut nonce_bytes = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, data)
        .map_err(|e| anyhow::anyhow!("encryption failed: {e}"))?;
    let mut result = Vec::with_capacity(12 + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// AES-256-GCM 解密：输入 `nonce(12) | ciphertext`。
fn aes_decrypt(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>> {
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce};

    if data.len() < 12 {
        anyhow::bail!("invalid encrypted data");
    }
    let cipher = Aes256Gcm::new_from_slice(key).context("create cipher")?;
    let (nonce_bytes, ciphertext) = data.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("decryption failed: {e}"))
}

/// WebDAV 密码在 `SecurityService` 中的固定凭据 id（按当前 Profile 隔离）。
const WEBDAV_CREDENTIAL_ID: &str = "webdav:password";
/// S3 Secret Key 在 `SecurityService` 中的固定凭据 id（按当前 Profile 隔离）。
const S3_SECRET_KEY_ID: &str = "s3:secret_key";
/// 同步加密口令在 `SecurityService` 中的固定凭据 id。
const SYNC_CREDENTIAL_ID: &str = "sync:passphrase";

/// 将 WebDAV 密码持久化到安全服务（Level 1 SQLite，DEK 加密）。
///
/// 真实密码不写入 `settings.json`，仅保存 `WebDavConfig::password_stored` 标志。
/// 用于避免每次同步都要求用户重新输入，并为后续自动同步提供免交互的密码来源。
pub fn store_webdav_password(password: &str) -> Result<()> {
    let mut svc = crate::security::current_security_service()
        .map_err(|e| anyhow::anyhow!("获取安全服务失败: {e}"))?;
    if svc.is_unlocked() || (svc.mode() == velowork_security::key_provider::SecurityMode::Standard && svc.unlock("").is_ok()) {
        svc.put(Credential {
            id: WEBDAV_CREDENTIAL_ID.to_string(),
            kind: SecretKind::WebDav,
            name: Some("WebDAV 密码".to_string()),
            value: SecretValue::password(password),
            metadata: serde_json::Value::Null,
        })
        .map_err(|e| anyhow::anyhow!("保存 WebDAV 密码失败: {e}"))?;
        Ok(())
    } else {
        bail!("安全服务未解锁，无法保存 WebDAV 密码");
    }
}

/// 从安全服务读取已持久化的 WebDAV 密码；未存储或损坏时返回 `None`。
pub fn load_webdav_password() -> Option<String> {
    if let Ok(mut svc) = crate::security::current_security_service() {
        if svc.is_unlocked() || (svc.mode() == velowork_security::key_provider::SecurityMode::Standard && svc.unlock("").is_ok()) {
            if let Ok(cred) = svc.get(WEBDAV_CREDENTIAL_ID) {
                if let Some(s) = cred.value.as_str() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

/// 删除已持久化的 WebDAV 密码。
pub fn delete_webdav_password() -> Result<()> {
    if let Ok(mut svc) = crate::security::current_security_service() {
        if svc.is_unlocked() || (svc.mode() == velowork_security::key_provider::SecurityMode::Standard && svc.unlock("").is_ok()) {
            let _ = svc.delete(WEBDAV_CREDENTIAL_ID);
        }
    }
    Ok(())
}

/// 将 S3 Secret Access Key 持久化到安全服务（Level 1 SQLite，DEK 加密）。
pub fn store_s3_secret_key(secret_key: &str) -> Result<()> {
    let mut svc = crate::security::current_security_service()
        .map_err(|e| anyhow::anyhow!("获取安全服务失败: {e}"))?;
    if svc.is_unlocked() || (svc.mode() == velowork_security::key_provider::SecurityMode::Standard && svc.unlock("").is_ok()) {
        svc.put(Credential {
            id: S3_SECRET_KEY_ID.to_string(),
            kind: SecretKind::S3,
            name: Some("S3 Secret Key".to_string()),
            value: SecretValue::password(secret_key),
            metadata: serde_json::Value::Null,
        })
        .map_err(|e| anyhow::anyhow!("保存 S3 Secret Key 失败: {e}"))?;
        Ok(())
    } else {
        bail!("安全服务未解锁，无法保存 S3 Secret Key");
    }
}

/// 从安全服务读取已持久化的 S3 Secret Access Key；未存储或损坏时返回 `None`。
pub fn load_s3_secret_key() -> Option<String> {
    if let Ok(mut svc) = crate::security::current_security_service() {
        if svc.is_unlocked() || (svc.mode() == velowork_security::key_provider::SecurityMode::Standard && svc.unlock("").is_ok()) {
            if let Ok(cred) = svc.get(S3_SECRET_KEY_ID) {
                if let Some(s) = cred.value.as_str() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

/// 删除已持久化的 S3 Secret Access Key。
pub fn delete_s3_secret_key() -> Result<()> {
    if let Ok(mut svc) = crate::security::current_security_service() {
        if svc.is_unlocked() || (svc.mode() == velowork_security::key_provider::SecurityMode::Standard && svc.unlock("").is_ok()) {
            let _ = svc.delete(S3_SECRET_KEY_ID);
        }
    }
    Ok(())
}

/// 将「同步加密口令」（即用户主密码）持久化到安全服务。
///
/// 同步 Bundle 以该口令作为 AES 密钥派生口令。为支持「免交互自动同步」，
/// 在用户设置/解锁主密码时把口令持久化于此；自动同步引擎启动时直接读取，
/// 无需再次弹窗输入。
pub fn store_sync_passphrase(passphrase: &str) -> Result<()> {
    let mut svc = crate::security::current_security_service()
        .map_err(|e| anyhow::anyhow!("获取安全服务失败: {e}"))?;
    if svc.is_unlocked() || (svc.mode() == velowork_security::key_provider::SecurityMode::Standard && svc.unlock("").is_ok()) {
        svc.put(Credential {
            id: SYNC_CREDENTIAL_ID.to_string(),
            kind: SecretKind::Custom("sync_passphrase".to_string()),
            name: Some("同步加密口令".to_string()),
            value: SecretValue::password(passphrase),
            metadata: serde_json::Value::Null,
        })
        .map_err(|e| anyhow::anyhow!("保存同步加密口令失败: {e}"))?;
        Ok(())
    } else {
        bail!("安全服务未解锁，无法保存同步加密口令");
    }
}

/// 从安全服务读取已持久化的同步加密口令；未存储或损坏时返回 `None`。
pub fn load_sync_passphrase() -> Option<String> {
    if let Ok(mut svc) = crate::security::current_security_service() {
        if svc.is_unlocked() || (svc.mode() == velowork_security::key_provider::SecurityMode::Standard && svc.unlock("").is_ok()) {
            if let Ok(cred) = svc.get(SYNC_CREDENTIAL_ID) {
                if let Some(s) = cred.value.as_str() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

/// 删除已持久化的同步加密口令。
pub fn delete_sync_passphrase() -> Result<()> {
    if let Ok(mut svc) = crate::security::current_security_service() {
        if svc.is_unlocked() || (svc.mode() == velowork_security::key_provider::SecurityMode::Standard && svc.unlock("").is_ok()) {
            let _ = svc.delete(SYNC_CREDENTIAL_ID);
        }
    }
    Ok(())
}

/// 为 AI 模型配置存储 API Key（不在 settings.json / ai.json 中明文落盘）。
pub fn store_ai_api_key(model_config_id: &str, api_key: &str) -> Result<()> {
    let profile = velowork_core::profiles::current();
    let account = account_for(&profile.id, "api_key", model_config_id);
    let mut svc = crate::security::current_security_service()
        .map_err(|e| anyhow::anyhow!("获取安全服务失败: {e}"))?;
    if svc.is_unlocked() || (svc.mode() == velowork_security::key_provider::SecurityMode::Standard && svc.unlock("").is_ok()) {
        svc.put(Credential {
            id: account.clone(),
            kind: SecretKind::ApiKey,
            name: Some(format!("AI Model {}", model_config_id)),
            value: SecretValue::password(api_key),
            metadata: serde_json::Value::Null,
        })
        .map_err(|e| anyhow::anyhow!("保存 AI API Key 失败: {e}"))?;

        // 同步更新 credential 表的元数据索引，确保云端同步/导出凭据能包含此 API Key
        if let Some(db) = velowork_core::storage::database() {
            let repo = crate::repositories::CredentialRepository::new(db);
            let now = crate::repositories::now_iso8601();
            let meta = crate::repositories::credential::CredentialMeta {
                id: account,
                profile_id: profile.id.clone(),
                kind: "api_key".to_string(),
                name: Some(format!("AI Model {}", model_config_id)),
                provider: Some("security-service".to_string()),
                created_at: now.clone(),
                updated_at: now,
                last_used_at: None,
            };
            if let Ok(Some(_)) = repo.get_meta(&meta.id) {
                let _ = repo.delete_meta(&meta.id);
            }
            let _ = repo.insert_meta(&meta);
        }

        Ok(())
    } else {
        bail!("安全服务未解锁，无法保存 AI API Key");
    }
}

/// 读取 AI 模型的 API Key。
pub fn load_ai_api_key(model_config_id: &str) -> Option<String> {
    let profile = velowork_core::profiles::current();
    let account = account_for(&profile.id, "api_key", model_config_id);
    if let Ok(mut svc) = crate::security::current_security_service() {
        if svc.is_unlocked() || (svc.mode() == velowork_security::key_provider::SecurityMode::Standard && svc.unlock("").is_ok()) {
            if let Ok(cred) = svc.get(&account) {
                if let Some(s) = cred.value.as_str() {
                    if !s.is_empty() {
                        return Some(s.to_string());
                    }
                }
            }
            // 兼容旧版 account（无 profile 前缀）："ai:api_key:{model_config_id}"
            let legacy_account = format!("ai:api_key:{}", model_config_id);
            if let Ok(cred) = svc.get(&legacy_account) {
                if let Some(s) = cred.value.as_str() {
                    if !s.is_empty() {
                        return Some(s.to_string());
                    }
                }
            }
        }
    }
    None
}

pub fn delete_ai_api_key(model_config_id: &str) -> Result<()> {
    let profile = velowork_core::profiles::current();
    let account = account_for(&profile.id, "api_key", model_config_id);
    let legacy_account = format!("ai:api_key:{}", model_config_id);
    if let Ok(mut svc) = crate::security::current_security_service() {
        if svc.is_unlocked() || (svc.mode() == velowork_security::key_provider::SecurityMode::Standard && svc.unlock("").is_ok()) {
            let _ = svc.delete(&account);
            let _ = svc.delete(&legacy_account);
        }
    }
    if let Some(db) = velowork_core::storage::database() {
        let repo = crate::repositories::CredentialRepository::new(db);
        let _ = repo.delete_meta(&account);
        let _ = repo.delete_meta(&legacy_account);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyring_provider_round_trip() {
        // 仅当 keyring 可用时验证；否则跳过（CI 中可能无密钥库）。
        let p = KeyringProvider::new("velowork-test");
        if !p.is_available() {
            return;
        }
        let account = "test:ssh:host-1";
        let _ = p.delete(account);
        p.store(account, b"secret-value").unwrap();
        assert_eq!(p.load(account).unwrap(), b"secret-value");
        p.delete(account).unwrap();
        assert!(p.load(account).is_err());
    }

    #[test]
    fn encrypted_file_provider_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.enc");
        let key = [7u8; 32];
        let p = EncryptedFileProvider::new(path.clone(), key);
        p.store("work:ssh:h1", b"pw1").unwrap();
        p.store("work:ai:claude", b"key2").unwrap();
        // 重新加载（模拟重启）应能从文件恢复
        let p2 = EncryptedFileProvider::new(path, key);
        assert_eq!(p2.load("work:ssh:h1").unwrap(), b"pw1");
        assert_eq!(p2.load("work:ai:claude").unwrap(), b"key2");
        p2.delete("work:ssh:h1").unwrap();
        assert!(p2.load("work:ssh:h1").is_err());
    }

    #[test]
    fn memory_provider_round_trip() {
        let p = MemoryProvider::new();
        p.store("a:b:c", b"x").unwrap();
        assert_eq!(p.load("a:b:c").unwrap(), b"x");
        p.delete("a:b:c").unwrap();
        assert!(p.load("a:b:c").is_err());
    }
}
