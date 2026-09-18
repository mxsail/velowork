//! Sync module for Velowork settings synchronization.
//!
//! 同步链路：`Repository → ProfileExporter → Bundle → SyncProvider`。
//! - `bundle`：封口 Bundle 结构（AES-GCM + zstd + manifest）。
//! - `provider`：`SyncProvider` trait + `Capabilities`，与后端解耦。
//! - `exporter`：`ProfileExporter` 组装/还原 Bundle。
//! - `webdav`：WebDAV 后端（实现 `SyncProvider`）。

pub mod backup;
pub mod bundle;
pub mod engine;
pub mod exporter;
pub mod export_service;
pub mod merge;
pub mod provider;
pub mod s3;
pub mod snapshot;
pub mod webdav;

pub use backup::SyncBackupManager;
pub use bundle::{Bundle, BundleFileEntry, BundleManifest, SealedBundle};
pub use engine::{notify_config_changed, register_sync_signal};
pub use exporter::ProfileExporter;
pub use export_service::ExportService;
pub use merge::BundleMerger;
pub use provider::{BoxedSyncProvider, Capabilities, SyncProvider};
pub use s3::S3Sync;
pub use snapshot::{
    LocalSyncState, force_push_to_cloud, restore_from_cloud, restore_from_cloud_scoped,
    sync_snapshot,
};
pub use webdav::{SyncResult, WebDavSync};

use crate::settings::{SyncProvider as SyncProviderKind, SyncSettings};

/// 多态同步后端枚举：封装所有已支持的后端实现，天然解决 RPITIT 的 dyn 兼容问题。
pub enum AnySyncProvider {
    WebDav(WebDavSync),
    S3(S3Sync),
}

impl SyncProvider for AnySyncProvider {
    fn capabilities(&self) -> Capabilities {
        match self {
            Self::WebDav(p) => p.capabilities(),
            Self::S3(p) => p.capabilities(),
        }
    }

    async fn test_connection(&self) -> anyhow::Result<()> {
        match self {
            Self::WebDav(p) => p.test_connection().await,
            Self::S3(p) => p.test_connection().await,
        }
    }

    async fn push(&self, bundle: &SealedBundle) -> anyhow::Result<()> {
        match self {
            Self::WebDav(p) => p.push(bundle).await,
            Self::S3(p) => p.push(bundle).await,
        }
    }

    async fn pull(&self) -> anyhow::Result<SealedBundle> {
        match self {
            Self::WebDav(p) => p.pull().await,
            Self::S3(p) => p.pull().await,
        }
    }

    async fn pull_manifest(&self) -> anyhow::Result<Option<BundleManifest>> {
        match self {
            Self::WebDav(p) => p.pull_manifest().await,
            Self::S3(p) => p.pull_manifest().await,
        }
    }
}

/// 统一提供商工厂：根据当前同步配置多态构建具体的 `AnySyncProvider`
pub fn create_sync_provider(
    settings: &SyncSettings,
    override_secret: Option<&str>,
) -> anyhow::Result<AnySyncProvider> {
    match settings.provider {
        SyncProviderKind::WebDav => {
            let p = WebDavSync::new(&settings.webdav, override_secret)?;
            Ok(AnySyncProvider::WebDav(p))
        }
        SyncProviderKind::S3 => {
            let p = S3Sync::new(&settings.s3, override_secret)?;
            Ok(AnySyncProvider::S3(p))
        }
    }
}

/// 将底层冗长复杂的同步/存储网络错误精简为易于用户理解的业务短语。
/// 详细的原始技术错误（URL、Socket 堆栈等）应完整记录于系统运行日志中。
pub fn simplify_sync_error(raw: &str) -> String {
    let lower = raw.to_lowercase();

    // 1. 地址与 URL 格式解析错误
    if lower.contains("invalid ipv4 address")
        || lower.contains("invalid ipv6 address")
        || lower.contains("relative url without a base")
        || lower.contains("empty host")
        || lower.contains("cannot be a base")
        || lower.contains("invalid port")
        || lower.contains("url parse")
    {
        return "服务地址或端口格式无效".to_string();
    }

    // 2. 连接拒绝 / 服务未启动 / TCP 连接失败
    if lower.contains("connection refused")
        || lower.contains("tcp connect error")
        || lower.contains("network unreachable")
        || lower.contains("host is down")
        || lower.contains("no route to host")
    {
        return "网络连接被拒绝，请检查服务是否启动或防火墙设置".to_string();
    }

    // 3. DNS 域名解析失败
    if lower.contains("failed to lookup address")
        || lower.contains("dns error")
        || lower.contains("name or service not known")
        || lower.contains("nodename nor servname provided")
    {
        return "域名解析失败，请检查服务地址是否正确".to_string();
    }

    // 4. 超时
    if lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("deadline has elapsed")
    {
        return "网络请求超时，请检查服务器连接质量".to_string();
    }

    // 5. 认证与权限拒绝 (401 / 403 / AccessDenied / SignatureDoesNotMatch / InvalidAccessKeyId)
    if lower.contains("invalidaccesskeyid")
        || lower.contains("signaturedoesnotmatch")
        || lower.contains("accessdenied")
        || lower.contains("401")
        || lower.contains("unauthorized")
        || lower.contains("403")
        || lower.contains("forbidden")
    {
        return "身份认证失败，请检查账号密码或访问密钥".to_string();
    }

    // 6. 存储桶 / 路径不存在 (404 / NoSuchBucket)
    if lower.contains("nosuchbucket") || lower.contains("404") || lower.contains("not found") {
        return "存储桶或远程路径不存在".to_string();
    }

    // 7. SSL / TLS 证书错误
    if lower.contains("certificate")
        || lower.contains("unknownissuer")
        || lower.contains("certverify")
        || lower.contains("handshake failure")
    {
        return "SSL/TLS 证书验证失败".to_string();
    }

    // 8. 5xx 服务器端错误
    if lower.contains("502 bad gateway") || lower.contains("bad gateway") {
        return "服务器网关错误 (HTTP 502)".to_string();
    }
    if lower.contains("503 service unavailable") {
        return "服务暂不可用 (HTTP 503)".to_string();
    }
    if lower.contains("500 internal server error") {
        return "服务器内部错误 (HTTP 500)".to_string();
    }

    // 9. 如果已经是结构化中文业务提示（清理冗余前缀如“连接 S3 失败：”、“连接失败：”）
    let cleaned = raw
        .trim()
        .trim_start_matches("连接 S3 失败：")
        .trim_start_matches("连接 S3 失败:")
        .trim_start_matches("连接失败：")
        .trim_start_matches("连接失败:")
        .trim();

    // 如果清理后的中文说明长度在合理范围（<= 45 字符），直接展示
    if !cleaned.is_empty()
        && cleaned.chars().count() <= 45
        && !cleaned.contains("builder error")
        && !cleaned.contains("dispatch failure")
    {
        return cleaned.to_string();
    }

    // 10. 兜底精简
    "网络请求异常，详情见运行日志".to_string()
}

#[cfg(test)]
mod tests {
    use super::simplify_sync_error;

    #[test]
    fn test_simplify_url_parse_error() {
        let err = "builder error: failed to resolve address 'http://192.168.2.22:9000': url parse: invalid IPv4 address";
        assert_eq!(simplify_sync_error(err), "服务地址或端口格式无效");
    }

    #[test]
    fn test_simplify_connection_refused() {
        let err = "dispatch failure: connection error: tcp connect error: Connection refused (os error 111)";
        assert_eq!(
            simplify_sync_error(err),
            "网络连接被拒绝，请检查服务是否启动或防火墙设置"
        );
    }

    #[test]
    fn test_simplify_auth_error() {
        let err = "InvalidAccessKeyId: The AWS Access Key Id you provided does not exist";
        assert_eq!(
            simplify_sync_error(err),
            "身份认证失败，请检查账号密码或访问密钥"
        );
    }

    #[test]
    fn test_simplify_bucket_not_found() {
        let err = "NoSuchBucket: The specified bucket does not exist";
        assert_eq!(simplify_sync_error(err), "存储桶或远程路径不存在");
    }

    #[test]
    fn test_simplify_clean_chinese() {
        let err = "连接 S3 失败：存储桶不存在，请先在云服务商创建存储桶";
        assert_eq!(
            simplify_sync_error(err),
            "存储桶不存在，请先在云服务商创建存储桶"
        );
    }
}
