//! AppAIRuntime — GPUI 感知的 AI 任务调度层（原则十）。
//!
//! 包装纯数据的 [`velowork_ai::AITaskManager`]，为 UI 提供：
//! - [`RequestId`] 生成与状态追踪
//! - [`CancelToken`] 取消支持
//! - 后台线程存活检测
//! - 统一清理
//!
//! UI 通过 [`RequestId`] 观察状态，永远不直接 await 网络。

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use velowork_ai::{
    AITaskManager, CancelToken, RequestId, RequestState,
};

/// AI 运行时：包装 [`AITaskManager`]，负责任务生命周期管理。
///
/// ```text
/// AiAssistantPanel
///     ↓ submit(cancel_token)
/// AppAIRuntime
///     → 后台线程执行 HTTP
///     → 消费循环 poll state
///     → UI 观察状态变化
/// ```
pub struct AppAIRuntime {
    manager: AITaskManager,
}

impl AppAIRuntime {
    pub fn new() -> Self {
        Self {
            manager: AITaskManager::new(),
        }
    }

    /// 注册新任务，返回 [`RequestId`] 和 [`CancelToken`]。
    ///
    /// 调用方负责：
    /// 1. 启动后台线程执行实际 HTTP 请求
    /// 2. 通过 `cancel_token.is_cancelled()` 检测取消
    /// 3. 任务结束时调用 `finish()` / `fail()`
    pub fn register(&mut self) -> (RequestId, CancelToken) {
        let cancel = CancelToken::new();
        let id = RequestId::next();
        self.manager.update_state(id, RequestState::Pending);
        (id, cancel)
    }

    /// 为已有 RequestId 挂载一个 agent 线程存活标记。
    ///
    /// 消费循环通过此标记检测后台线程异常退出，防止卡死。
    pub fn bind_alive(
        &mut self,
        _id: RequestId,
    ) -> (Arc<AtomicBool>, Arc<AtomicBool>) {
        let alive = Arc::new(AtomicBool::new(true));
        let clone = alive.clone();
        (alive, clone)
    }

    /// 更新任务状态。
    pub fn update_state(&mut self, id: RequestId, state: RequestState) {
        self.manager.update_state(id, state);
    }

    /// 标记任务失败。
    pub fn fail(
        &mut self,
        id: RequestId,
        error: velowork_ai::AIError,
    ) {
        self.manager.fail(id, error);
    }

    /// 标记任务完成。
    pub fn complete(&mut self, id: RequestId) {
        self.manager.update_state(id, RequestState::Completed);
    }

    /// 取消指定任务。
    pub fn cancel(&mut self, id: RequestId) -> bool {
        self.manager.cancel(id)
    }

    /// 取消所有在途任务。
    pub fn cancel_all(&mut self) {
        self.manager.cancel_all()
    }

    /// 查询任务状态。
    pub fn state(&self, id: RequestId) -> Option<&RequestState> {
        self.manager.state(id)
    }

    /// 是否还有活跃任务。
    pub fn has_active(&self) -> bool {
        self.manager.has_active()
    }

    /// 活跃任务数。
    pub fn active_count(&self) -> usize {
        self.manager.active_count()
    }

    /// 强制清理所有任务（面板关闭时）。
    pub fn cleanup_all(&mut self) {
        self.manager.cleanup_all()
    }

    /// 底层管理器引用（供高级操作）。
    pub fn manager(&self) -> &AITaskManager {
        &self.manager
    }

    pub fn manager_mut(&mut self) -> &mut AITaskManager {
        &mut self.manager
    }
}

impl Default for AppAIRuntime {
    fn default() -> Self {
        Self::new()
    }
}
