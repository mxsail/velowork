//! 事件枚举：在全局 [`EventBus`](crate::event::EventBus) 上广播的领域事件。
//!
//! 事件用于解耦生产者（工作区变更、凭据变更、同步）与消费者（AI 记忆、
//! 终端、资源监控、状态栏、同步引擎）。生产者 `publish`，消费者实现
//! [`Subscriber`](crate::event::Subscriber)。

/// 通过 [`EventBus`](crate::event::EventBus) 广播的领域事件。
#[derive(Debug, Clone)]
pub enum Event {
    /// 工作区树变更（项目/布局/文件夹）。`id` 为受影响的工作区 id（已知时）。
    WorkspaceChanged {
        id: Option<String>,
    },
    /// 凭据在密钥库或元数据中新增/更新/删除。`kind` 为凭据类型（已知时）。
    CredentialChanged {
        kind: Option<String>,
    },
    /// 同步状态变脏，可能需要 push/pull。
    SyncDirty,
    /// 应用设置变更。
    SettingsChanged,
    /// SSH / 会话连接状态变更。`id` 为会话 id（已知时）。
    SessionChanged {
        id: Option<String>,
    },
    /// AI 上下文（记忆 / 快照）已更新。
    AiContextChanged,
}
