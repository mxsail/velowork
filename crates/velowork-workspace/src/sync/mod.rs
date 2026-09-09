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
pub mod snapshot;
pub mod webdav;

pub use backup::SyncBackupManager;
pub use bundle::{Bundle, BundleFileEntry, BundleManifest, SealedBundle};
pub use engine::{notify_config_changed, register_sync_signal};
pub use exporter::ProfileExporter;
pub use export_service::ExportService;
pub use merge::BundleMerger;
pub use provider::{BoxedSyncProvider, Capabilities, SyncProvider};
pub use snapshot::{
    LocalSyncState, force_push_to_cloud, restore_from_cloud, restore_from_cloud_scoped,
    sync_snapshot,
};
pub use webdav::{SyncResult, WebDavSync};
