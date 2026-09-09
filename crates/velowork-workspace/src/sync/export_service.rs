//! Application 层凭据导出/导入服务（不在 `velowork-security` 内）。
//!
//! - 导出：经 [`SecurityService`] 取明文凭据 → 序列化为 `Bundle` 文件 → 用**导出密码**
//!   经 `Bundle::seal` 加密成 [`SealedBundle`]。导出密码链与本地 DEK **完全解耦**。
//! - 导入：用导出密码 `Bundle::open` 还原 → 逐条凭据经 [`SecurityService::put`]
//!   按**目标 profile 的 DEK** 重新加密落库。
//!
//! 三种 [`ExportMode`] 本质是 [`Capability`] 的 Preset，由 `velowork-security::bundle` 定义。

use anyhow::{Context, Result, bail};
use base64::{
    engine::general_purpose::STANDARD as B64,
    Engine as _,
};
use serde::{Deserialize, Serialize};

use velowork_security::bundle::{Bundle, ExportMode, SealedBundle};
use velowork_security::credential::{Credential, SecretKind, SecretValue};
use velowork_security::service::SecurityService;

/// 单条凭据在 Bundle 内的序列化形态（不依赖 `SecretValue` 的 serde 派生，
/// 改用其自研 TLV `encode` 的 base64 表示，避免引入 `Zeroizing` 的 serde 实现）。
#[derive(Clone, Debug, Serialize, Deserialize)]
struct CredentialExportEntry {
    id: String,
    kind: SecretKind,
    name: Option<String>,
    /// `SecretValue::encode()` 的 base64，导入时 `decode` 还原。
    value_b64: String,
    metadata: serde_json::Value,
}

/// 凭据导出/导入服务。持有 [`SecurityService`]（需先 `unlock` 才能读写凭据）。
pub struct ExportService {
    svc: SecurityService,
}

impl ExportService {
    pub fn new(svc: SecurityService) -> Self {
        Self { svc }
    }

    /// 导出当前 profile 的全部凭据为封口 Bundle（导出密码链）。
    ///
    /// `mode` 决定 Bundle 清单携带的 `capabilities` 预设；凭据本身始终纳入
    /// （`Backup` / `EnterpriseMigration` 含 Credentials，`Share` 不含机密但本方法
    /// 仅导出凭据，调用方按模式决定是否调用）。
    pub fn export_credentials(&mut self, mode: ExportMode, password: &str) -> Result<SealedBundle> {
        if !self.svc.is_unlocked() {
            bail!("security service must be unlocked before exporting credentials");
        }
        if password.is_empty() {
            bail!("export password must not be empty");
        }
        let creds = self
            .svc
            .list(None)
            .map_err(|e| anyhow::anyhow!("list credentials: {e}"))?;

        let caps = mode.capabilities();
        let app_version = env!("CARGO_PKG_VERSION");
        let mut bundle = Bundle::new(&self.svc.profile_id(), mode, &caps, app_version);

        for c in creds {
            let entry = CredentialExportEntry {
                id: c.id.clone(),
                kind: c.kind,
                name: c.name,
                value_b64: B64.encode(c.value.encode()),
                metadata: c.metadata,
            };
            let json = serde_json::to_vec(&entry).context("serialize credential entry")?;
            let path = format!("credentials/{}.json", sanitize_id(&c.id));
            bundle
                .add_file(&path, &json, Some("credentials"))
                .map_err(|e| anyhow::anyhow!("add bundle file: {e}"))?;
        }

        bundle
            .seal(password)
            .map_err(|e| anyhow::anyhow!("seal bundle: {e}"))
    }

    /// 从封口 Bundle 导入凭据（按目标 profile 的 DEK 重新加密落库）。
    ///
    /// 返回成功导入的凭据条数。Bundle 内非 `credentials/` 前缀的文件会被忽略，
    /// 便于未来扩展导出 Workspace / Theme 等非机密内容。
    pub fn import_credentials(&mut self, sealed: &SealedBundle, password: &str) -> Result<usize> {
        if !self.svc.is_unlocked() {
            bail!("security service must be unlocked before importing credentials");
        }
        let bundle = Bundle::open(sealed, password)
            .map_err(|e| anyhow::anyhow!("open bundle (wrong export password?): {e}"))?;

        let mut count = 0usize;
        for entry in &bundle.manifest().files {
            if !entry.path.starts_with("credentials/") {
                continue;
            }
            let content = bundle
                .get_file(&entry.path)
                .context("read bundle credential file")?;
            let entry: CredentialExportEntry =
                serde_json::from_slice(content).context("parse credential entry")?;
            let raw = B64
                .decode(&entry.value_b64)
                .context("base64 decode secret value")?;
            let value = SecretValue::decode(&raw)
                .map_err(|e| anyhow::anyhow!("decode secret value: {e}"))?;
            let cred = Credential {
                id: entry.id,
                kind: entry.kind,
                name: entry.name,
                value,
                metadata: entry.metadata,
            };
            self.svc
                .put(cred)
                .map_err(|e| anyhow::anyhow!("store imported credential: {e}"))?;
            count += 1;
        }
        Ok(count)
    }
}

/// Bundle 内路径需避免 `:` 等可能在某些存储后端出问题的字符；凭据 id 形如
/// `work:ssh_password:h1`，这里把 `:` 替换为 `_`。
fn sanitize_id(id: &str) -> String {
    id.replace(':', "_")
}

/// 供测试与外部构造：把 [`SealedBundle`] 还原为凭据列表（不落库，仅解析）。
#[allow(dead_code)]
pub(crate) fn peek_credentials(sealed: &SealedBundle, password: &str) -> Result<Vec<Credential>> {
    let bundle =
        Bundle::open(sealed, password).map_err(|e| anyhow::anyhow!("open bundle: {e}"))?;
    let mut out = Vec::new();
    for entry in &bundle.manifest().files {
        if !entry.path.starts_with("credentials/") {
            continue;
        }
        let content = bundle.get_file(&entry.path).context("read bundle file")?;
        let entry: CredentialExportEntry =
            serde_json::from_slice(content).context("parse credential entry")?;
        let raw = B64.decode(&entry.value_b64).context("base64 decode")?;
        let value = SecretValue::decode(&raw).map_err(|e| anyhow::anyhow!("decode: {e}"))?;
        out.push(Credential {
            id: entry.id,
            kind: entry.kind,
            name: entry.name,
            value,
            metadata: entry.metadata,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use velowork_core::storage::Database;
    use velowork_security::credential::SecretValue;

    fn unlocked(app_name: &str, profile_id: &str, db: Arc<Database>) -> SecurityService {
        let mut svc = SecurityService::new(app_name, profile_id, db).unwrap();
        svc.unlock("").unwrap();
        svc
    }

    #[test]
    fn export_import_credentials_round_trip() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let s1 = unlocked("velowork-test", "work", db.clone());
        s1.put(Credential {
            id: "work:ssh_password:h1".into(),
            kind: SecretKind::SshPassword,
            name: Some("H1".into()),
            value: SecretValue::password("pw"),
            metadata: serde_json::Value::Null,
        })
        .unwrap();

        let mut ex = ExportService::new(s1);
        let sealed = ex.export_credentials(ExportMode::Backup, "exp-pw").unwrap();

        // 错误导出密码无法解口。
        let s2 = unlocked("velowork-test", "work", db.clone());
        let mut imp = ExportService::new(s2);
        assert!(imp.import_credentials(&sealed, "wrong").is_err());

        // 正确导出密码导入成功，并按目标 DEK 重新加密落库。
        let n = imp.import_credentials(&sealed, "exp-pw").unwrap();
        assert_eq!(n, 1);

        // 用第三个门面读取，确认明文一致。
        let s3 = unlocked("velowork-test", "work", db.clone());
        let got = s3.get("work:ssh_password:h1").unwrap();
        assert_eq!(got.value.as_str(), Some("pw"));
        assert_eq!(got.name.as_deref(), Some("H1"));
    }

    #[test]
    fn export_requires_unlock_and_password() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let svc = SecurityService::new("velowork-test", "work", db).unwrap(); // 未解锁
        let mut ex = ExportService::new(svc);
        assert!(ex.export_credentials(ExportMode::Backup, "pw").is_err());

        let db = Arc::new(Database::open_in_memory().unwrap());
        let svc = unlocked("velowork-test", "work", db);
        let mut ex = ExportService::new(svc);
        assert!(ex.export_credentials(ExportMode::Backup, "").is_err());
    }
}
