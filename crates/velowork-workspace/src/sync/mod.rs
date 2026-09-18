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

fn extract_xml_tag<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)? + start;
    let val = text[start..end].trim();
    if val.is_empty() { None } else { Some(val) }
}

/// 直接从底层错误信息中裁剪出精简、语言中立（天然兼容国际化）的核心原因。
/// 消除 XML 报文冗余、重复的前缀嵌套与底层调用链噪声，详细原始报文依然完整输出至运行日志。
pub fn simplify_sync_error(raw: &str) -> String {
    let mut s = raw.trim();

    // 1. 剥离重复的嵌套失败前缀（支持中英文）
    for prefix in &[
        "连接失败:", "连接失败：",
        "连接 S3 失败:", "连接 S3 失败：",
        "连接 WebDAV 失败:", "连接 WebDAV 失败：",
        "Connection failed:", "Connect failed:",
    ] {
        while let Some(stripped) = s.strip_prefix(prefix) {
            s = stripped.trim();
        }
    }

    // 2. 如果包含 S3 / WebDAV XML 错误载荷：Got HTTP 401 with content '<?xml...><Code>...</Code><Message>...</Message>'
    if let Some(code) = extract_xml_tag(s, "Code") {
        // 提取 HTTP 状态码，如 "HTTP 401"
        let http_prefix = if let Some(idx) = s.find("HTTP ") {
            let part = &s[idx..];
            part.split_whitespace().take(2).collect::<Vec<_>>().join(" ")
        } else {
            String::new()
        };

        let message = extract_xml_tag(s, "Message");

        return match (http_prefix.is_empty(), message) {
            (false, Some(msg)) => {
                // 如果 message 包含过长细节，截取逗号前第一句
                let clean_msg = msg.split(',').next().unwrap_or(msg).trim();
                format!("{http_prefix} ({code}): {clean_msg}")
            }
            (false, None) => format!("{http_prefix} ({code})"),
            (true, Some(msg)) => {
                let clean_msg = msg.split(',').next().unwrap_or(msg).trim();
                format!("{code}: {clean_msg}")
            }
            (true, None) => code.to_string(),
        };
    }

    // 3. 如果包含 "with content '"，说明有未解析的原始响应体，直接剔除 'with content ...'
    if let Some(idx) = s.find(" with content '") {
        let prefix = s[..idx].trim();
        if !prefix.is_empty() {
            let clean = prefix.strip_prefix("Got ").unwrap_or(prefix);
            return clean.to_string();
        }
    }

    // 4. 如果是多级链式错误（如 A: B: C），提取末尾最直接的根因
    // 例如："builder error: failed to resolve address '...': url parse: invalid IPv4 address" -> "url parse: invalid IPv4 address"
    if let Some(pos) = s.rfind(": ") {
        let last_part = s[pos + 2..].trim();
        if !last_part.is_empty() && last_part.len() <= 60 {
            return last_part.to_string();
        }
    }

    // 5. 限制最大长度（保持单行紧凑，不超过 60 字符）
    if s.chars().count() > 60 {
        let truncated: String = s.chars().take(60).collect();
        format!("{truncated}…")
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::simplify_sync_error;

    #[test]
    fn test_simplify_s3_xml_error() {
        let err = "Got HTTP 401 with content '<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>UnauthorizedAccess</Code><Message>Your account is not signed up2, access_key: rustfsadmin1</Message></Error>'";
        assert_eq!(
            simplify_sync_error(err),
            "HTTP 401 (UnauthorizedAccess): Your account is not signed up2"
        );
    }

    #[test]
    fn test_simplify_nested_prefix() {
        let err = "连接失败: 连接 S3 失败：Got HTTP 401 with content '<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>UnauthorizedAccess</Code><Message>Your account is not signed up2, access_key: rustfsadmin1</Message></Error>'";
        assert_eq!(
            simplify_sync_error(err),
            "HTTP 401 (UnauthorizedAccess): Your account is not signed up2"
        );
    }

    #[test]
    fn test_simplify_url_parse_error() {
        let err = "builder error: failed to resolve address 'http://192.168.2.22:9000': url parse: invalid IPv4 address";
        assert_eq!(simplify_sync_error(err), "invalid IPv4 address");
    }

    #[test]
    fn test_simplify_connection_refused() {
        let err = "dispatch failure: connection error: tcp connect error: Connection refused (os error 111)";
        assert_eq!(simplify_sync_error(err), "Connection refused (os error 111)");
    }

    #[test]
    fn test_simplify_bucket_not_found_xml() {
        let err = "Got HTTP 404 with content '<?xml version=\"1.0\"?><Error><Code>NoSuchBucket</Code><Message>The specified bucket does not exist</Message></Error>'";
        assert_eq!(
            simplify_sync_error(err),
            "HTTP 404 (NoSuchBucket): The specified bucket does not exist"
        );
    }
}
