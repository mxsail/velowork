//! AI 任务管理器（原则十）。
//!
//! 管理所有在途 AI 请求的生命周期：[`AITaskManager`] 是纯数据结构，
//! 不依赖 GPUI，可独立测试。实际线程调度由上层 `AppAIRuntime` 负责。

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Instant;

use super::error::AIError;
use super::task::{AIRequest, AIResponse, CancelToken, RequestId, RequestState, StreamEvent};

/// 一个在途 AI 任务的内部记录。
struct TaskRecord {
    /// 发起时间。
    started_at: Instant,
    /// 当前状态。
    state: RequestState,
    /// 取消令牌。
    cancel_token: CancelToken,
    /// 接收流式事件（供查询实时事件）。
    event_rx: Option<mpsc::Receiver<StreamEvent>>,
    /// 已接收的事件数。
    event_count: usize,
    /// 已接收的 token 字符数。
    token_count: usize,
    /// 重试次数。
    #[allow(dead_code)]
    retries: u32,
}

/// AI 任务管理器（原则三：不要直接 await）。
///
/// ```text
/// ChatView
///   ↓ submit()
/// AITaskManager
///   → 后台线程执行 HTTP
///   → channel 回传 StreamEvent
///   → UI poll_state() 查询状态
/// ```
pub struct AITaskManager {
    tasks: HashMap<RequestId, TaskRecord>,
    /// 已完成的任务缓存（保留最新 N 条供 UI 回溯）。
    finished: Vec<(RequestId, AIResponse)>,
    max_finished: usize,
}

impl AITaskManager {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            finished: Vec::new(),
            max_finished: 50,
        }
    }

    // ── Submit ───────────────────────────────────────────────────

    /// 注册一个新请求，返回 `RequestId`。
    ///
    /// 调用方自行在后台线程运行 HTTP，通过返回的 `CancelToken` 控制。
    /// 后台线程将 [`StreamEvent`] 推入 `request.event_tx`。
    pub fn register(&mut self, request: &AIRequest) -> (RequestId, CancelToken) {
        let id = RequestId::next();
        let cancel = request.cancel_token.clone().unwrap_or_default();
        self.tasks.insert(
            id,
            TaskRecord {
                started_at: Instant::now(),
                state: RequestState::Pending,
                cancel_token: cancel.clone(),
                event_rx: None,
                event_count: 0,
                token_count: 0,
                retries: 0,
            },
        );
        (id, cancel)
    }

    /// 注册请求并设置事件接收端（用于消费 `StreamEvent`）。
    pub fn register_with_rx(
        &mut self,
        request: &AIRequest,
        event_rx: mpsc::Receiver<StreamEvent>,
    ) -> RequestId {
        let id = RequestId::next();
        let cancel = request.cancel_token.clone().unwrap_or_default();
        self.tasks.insert(
            id,
            TaskRecord {
                started_at: Instant::now(),
                state: RequestState::Pending,
                cancel_token: cancel,
                event_rx: Some(event_rx),
                event_count: 0,
                token_count: 0,
                retries: 0,
            },
        );
        id
    }

    // ── Cancel ───────────────────────────────────────────────────

    /// 取消指定请求（原则四：必须支持 Cancel）。
    ///
    /// 后台线程通过 `CancelToken::is_cancelled()` 检测并停止。
    pub fn cancel(&mut self, id: RequestId) -> bool {
        if let Some(record) = self.tasks.get_mut(&id) {
            record.cancel_token.cancel();
            record.state = RequestState::Cancelled;
            true
        } else {
            false
        }
    }

    /// 取消所有在途任务。
    pub fn cancel_all(&mut self) {
        for record in self.tasks.values_mut() {
            record.cancel_token.cancel();
            record.state = RequestState::Cancelled;
        }
    }

    // ── State ────────────────────────────────────────────────────

    /// 查询请求当前状态。
    pub fn state(&self, id: RequestId) -> Option<&RequestState> {
        self.tasks.get(&id).map(|r| &r.state)
    }

    /// 更新任务状态（由消费循环调用）。
    pub fn update_state(&mut self, id: RequestId, state: RequestState) {
        if let Some(record) = self.tasks.get_mut(&id) {
            record.state = state.clone();
            if state.is_terminal() {
                self.archive_if_terminal(id);
            }
        }
    }

    /// 消费事件通道中的事件并更新状态与计数器。
    ///
    /// 由主线程在 `update()` 中调用。返回本轮消费的事件数。
    pub fn drain_events(&mut self, id: RequestId) -> usize {
        let record = match self.tasks.get_mut(&id) {
            Some(r) => r,
            None => return 0,
        };
        if record.state.is_terminal() {
            return 0;
        }
        let mut count = 0;
        if let Some(ref rx) = record.event_rx {
            loop {
                match rx.try_recv() {
                    Ok(ev) => {
                        count += 1;
                        record.event_count += 1;
                        match &ev {
                            StreamEvent::StateChange(s) => {
                                record.state = s.clone();
                            }
                            StreamEvent::Token(_) => {
                                record.token_count += 1;
                                record.state = RequestState::Streaming {
                                    tokens: record.token_count,
                                };
                            }
                            StreamEvent::ToolRequest { name, .. } => {
                                record.state = RequestState::WaitingTool {
                                    tool_name: name.clone(),
                                };
                            }
                            StreamEvent::ToolResult(_) => {
                                // 工具结果后回到 streaming 状态。
                                record.state = RequestState::Streaming {
                                    tokens: record.token_count,
                                };
                            }
                        }
                        if record.state.is_terminal() {
                            self.archive_if_terminal(id);
                            break;
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        // 后台线程退出但未发出终端事件 → 异常退出。
                        if !record.state.is_terminal() {
                            record.state = RequestState::Failed {
                                error: AIError::StreamBroken {
                                    message: "后台任务意外退出".into(),
                                },
                            };
                        }
                        self.archive_if_terminal(id);
                        break;
                    }
                }
            }
        }
        count
    }

    /// 标记失败并收尾。
    pub fn fail(&mut self, id: RequestId, error: AIError) {
        if let Some(record) = self.tasks.get_mut(&id) {
            record.state = RequestState::Failed {
                error: error.clone(),
            };
            self.archive_if_terminal(id);
        }
    }

    /// 标记完成并收尾。
    pub fn complete(&mut self, id: RequestId, _text: String, total_tokens: usize) {
        if let Some(record) = self.tasks.get_mut(&id) {
            record.state = RequestState::Completed;
            record.token_count = total_tokens;
            self.archive_if_terminal(id);
        }
    }

    // ── Query ────────────────────────────────────────────────────

    /// 是否还有活跃（非终态）任务。
    pub fn has_active(&self) -> bool {
        self.tasks
            .values()
            .any(|r| !r.state.is_terminal())
    }

    /// 活跃任务数量。
    pub fn active_count(&self) -> usize {
        self.tasks
            .values()
            .filter(|r| !r.state.is_terminal())
            .count()
    }

    /// 获取任务已运行时长。
    pub fn elapsed(&self, id: RequestId) -> Option<std::time::Duration> {
        self.tasks
            .get(&id)
            .map(|r| r.started_at.elapsed())
    }

    /// 获取已完成任务列表（最新在前）。
    pub fn finished_tasks(&self) -> &[(RequestId, AIResponse)] {
        &self.finished
    }

    // ── Cleanup ──────────────────────────────────────────────────

    /// 清理所有已完成任务（原则十二：一定要 Cleanup）。
    pub fn cleanup(&mut self) {
        self.tasks.retain(|_, r| !r.state.is_terminal());
    }

    /// 强制清理所有任务（面板关闭时调用）。
    pub fn cleanup_all(&mut self) {
        self.cancel_all();
        self.tasks.clear();
        self.finished.clear();
    }

    // ── Internal ─────────────────────────────────────────────────

    fn archive_if_terminal(&mut self, id: RequestId) {
        if let Some(record) = self.tasks.get(&id) {
            if record.state.is_terminal() {
                let response = AIResponse {
                    request_id: id,
                    final_state: record.state.clone(),
                    text: String::new(),
                    total_tokens: record.token_count,
                    elapsed: record.started_at.elapsed(),
                };
                self.finished.push((id, response));
                if self.finished.len() > self.max_finished {
                    self.finished.remove(0);
                }
            }
        }
    }
}

impl Default for AITaskManager {
    fn default() -> Self {
        Self::new()
    }
}
