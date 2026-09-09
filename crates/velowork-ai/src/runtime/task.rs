//! AI 任务抽象：请求 ID、生命周期状态、取消令牌、流式事件。
//!
//! 所有类型纯数据，无 GPUI 依赖，可独立测试。

use super::error::AIError;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

// ── RequestId ────────────────────────────────────────────────────────

/// 唯一请求标识符。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RequestId(pub u64);

impl RequestId {
    /// 从原子计数器生成一个唯一 ID。
    pub fn next() -> Self {
        use std::sync::atomic::AtomicU64;
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        RequestId(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

// ── RequestState ─────────────────────────────────────────────────────

/// AI 请求的完整生命周期状态。
///
/// UI 只观察状态，不直接等待网络。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequestState {
    /// 已排队，等待发送。
    Pending,
    /// 正在建立连接（DNS / TCP / TLS）。
    Connecting,
    /// 正在接收流式回复。
    Streaming {
        /// 已接收的 token 数。
        tokens: usize,
    },
    /// 等待工具执行结果（Agent 模式）。
    WaitingTool {
        /// 工具名称。
        tool_name: String,
    },
    /// 成功完成。
    Completed,
    /// 失败。
    Failed {
        error: AIError,
    },
    /// 被用户取消。
    Cancelled,
    /// 超时。
    Timeout {
        message: String,
    },
}

impl RequestState {
    /// 是否为终态（不会再有后续事件）。
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            RequestState::Completed
                | RequestState::Failed { .. }
                | RequestState::Cancelled
                | RequestState::Timeout { .. }
        )
    }
}

// ── CancelToken ──────────────────────────────────────────────────────

/// 取消令牌。
///
/// 创建后传入后台任务；调用方持有 Clone，调用 `cancel()` 通知后台停止。
/// 后台任务通过 `is_cancelled()` 轮询。
#[derive(Clone)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl fmt::Debug for CancelToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CancelToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 发送取消信号。
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    /// 检查是否已被取消。
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

// ── StreamEvent ──────────────────────────────────────────────────────

/// 流式 AI 回复的单个事件（替代当前的 `AgentEvent`，语义更完整）。
///
/// 从后台任务通过 channel 回传 UI。
#[derive(Clone, Debug)]
pub enum StreamEvent {
    /// 状态变更。
    StateChange(RequestState),
    /// 文本增量（流式 token）。
    Token(String),
    /// 模型请求调用工具。
    ToolRequest {
        /// 工具名称。
        name: String,
        /// JSON 参数字符串。
        arguments: String,
    },
    /// 工具执行结果。
    ToolResult(String),
}

// ── TimeoutConfig ────────────────────────────────────────────────────

/// 分层超时配置（原则五）。
#[derive(Clone, Debug)]
pub struct TimeoutConfig {
    /// DNS 域名解析（默认 3s）。
    pub dns: Duration,
    /// TCP 连接建立（默认 5s）。
    pub tcp: Duration,
    /// TLS 握手（默认 5s）。
    pub tls: Duration,
    /// HTTP 请求整体（默认 30s）。
    pub http: Duration,
    /// 流式接收空闲超时：超过此时间无任何 token 则终止（默认 60s）。
    pub streaming: Duration,
    /// 流中任意两 token 间的等待上限（默认 15s）。
    pub idle: Duration,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            dns: Duration::from_secs(3),
            tcp: Duration::from_secs(5),
            tls: Duration::from_secs(5),
            http: Duration::from_secs(30),
            streaming: Duration::from_secs(60),
            idle: Duration::from_secs(15),
        }
    }
}

// ── RetryConfig ──────────────────────────────────────────────────────

/// 重试策略配置（原则十一）。
#[derive(Clone, Debug)]
pub struct RetryConfig {
    /// 网络错误最大重试次数。
    pub max_network_retries: u32,
    /// 超时最大重试次数。
    pub max_timeout_retries: u32,
    /// 服务端错误最大重试次数。
    pub max_server_retries: u32,
    /// 重试间隔基数（指数退避：base * 2^(n-1)）。
    pub backoff_base: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_network_retries: 2,
            max_timeout_retries: 1,
            max_server_retries: 2,
            backoff_base: Duration::from_secs(1),
        }
    }
}

impl RetryConfig {
    /// 判断给定错误是否应该重试，以及延迟多久。
    pub fn should_retry(&self, error: &AIError, current_retries: u32) -> Option<Duration> {
        if !error.is_retryable() {
            return None;
        }
        let max = match error {
            AIError::Timeout { .. } => self.max_timeout_retries,
            AIError::Network { .. } => self.max_network_retries,
            AIError::Server { .. } => self.max_server_retries,
            AIError::StreamBroken { .. } => self.max_network_retries,
            AIError::StreamTimeout { .. } => self.max_timeout_retries,
            _ => return None,
        };
        if current_retries >= max {
            return None;
        }
        // 指数退避: base * 2^retries
        let delay = self.backoff_base * 2u32.pow(current_retries);
        Some(delay)
    }
}

// ── AIRequest ────────────────────────────────────────────────────────

/// 一个 AI 请求的完整参数（原则二：请求必须有生命周期）。
#[derive(Clone, Debug)]
pub struct AIRequest {
    /// 模型配置。
    pub base_url: String,
    pub api_key: String,
    pub model_id: String,

    /// OpenAI 格式消息列表（含 system / user / tool 等）。
    pub messages: Vec<serde_json::Value>,

    /// 可选工具定义（Agent 模式）。
    pub tools: Vec<super::super::provider::ToolSpec>,

    /// 最大 Agent 轮次（默认 8）。
    pub max_rounds: usize,

    /// 超时配置。
    pub timeout: TimeoutConfig,

    /// 可选取消令牌（调用方持有，可随时 cancel）。
    pub cancel_token: Option<CancelToken>,

    /// 事件回传通道。
    pub event_tx: std::sync::mpsc::Sender<StreamEvent>,

    /// 工具执行结果回传通道（Agent 模式）。
    pub tool_tx: Option<
        std::sync::mpsc::Sender<(
            String,
            serde_json::Value,
            std::sync::mpsc::Sender<Result<String, AIError>>,
        )>,
    >,
}

// ── AIResponse ───────────────────────────────────────────────────────

/// AI 请求完成后的结果。
#[derive(Clone, Debug)]
pub struct AIResponse {
    pub request_id: RequestId,
    pub final_state: RequestState,
    /// 完整输出文本（仅在 Completed 时有效）。
    pub text: String,
    /// 总 token 数（近似）。
    pub total_tokens: usize,
    /// 总耗时。
    pub elapsed: Duration,
}
