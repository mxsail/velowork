//! 同步链路抽象：`SyncProvider` trait + `Capabilities`。
//!
//! 同步与具体后端（WebDAV / 云 / S3 / Git）解耦；每个后端实现同一 trait，
//! 由 `Capabilities` 声明其能力，导出/导入逻辑（见 `exporter`）不感知后端。

use anyhow::Result;

use crate::sync::bundle::{BundleManifest, SealedBundle};

/// 后端能力声明。导出方据此选择策略（如不支持 delta 则整包推送）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// 传输层是否加密（TLS / HTTPS）。
    pub encryption: bool,
    /// 是否支持增量同步（仅变更文件）。
    pub delta: bool,
    /// 是否支持冲突检测 / 合并。
    pub conflict: bool,
    /// 是否保留历史版本。
    pub history: bool,
    /// 是否支持版本回滚。
    pub versioning: bool,
}

/// 同步后端统一接口。实现可为 WebDAV / 云存储 / S3 / Git 等。
#[allow(async_fn_in_trait)]
pub trait SyncProvider {
    /// 该后端的能力。
    fn capabilities(&self) -> Capabilities;

    /// 连通性与权限探针（验证鉴权、服务可达性、Bucket 或目录可写）
    async fn test_connection(&self) -> Result<()>;

    /// 推送一个封口 Bundle 到远端。
    async fn push(&self, bundle: &SealedBundle) -> Result<()>;

    /// 从远端拉取一个封口 Bundle。
    async fn pull(&self) -> Result<SealedBundle>;

    /// 仅拉取远端最新快照的 manifest（元数据），避免整包下载。
    /// 用于 SyncEngine 比较祖先关系；无远端快照时返回 `None`。
    async fn pull_manifest(&self) -> Result<Option<BundleManifest>>;
}

/// 便于在运行时按配置选择后端的别名。
pub type BoxedSyncProvider = Box<dyn SyncProvider + Send + Sync>;
