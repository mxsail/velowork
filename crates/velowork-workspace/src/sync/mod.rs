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
