//! 统一 AI 错误类型。
//!
//! 所有 provider / agent / tool 的错误都收敛为 `AIError`，UI 层只认这一种错误。
//! 不直接暴露 `reqwest::Error` / `serde_json::Error` / `std::io::Error`。

use std::fmt;

/// AI 调用可能发生的所有错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AIError {
    /// 请求整体超时（含 DNS / TCP / TLS / HTTP）。
    Timeout {
        message: String,
        /// 当前已重试次数（用于决定是否继续重试）。
        retries: u32,
    },
    /// 流式接收期间超过空闲时限。
    StreamTimeout {
        message: String,
    },
    /// 被取消（用户主动 / 面板关闭等）。
    Cancelled,
    /// 认证失败（API Key 无效 / 过期）。
    Unauthorized {
        message: String,
    },
    /// 无权限（资源不可访问）。
    Forbidden {
        message: String,
    },
    /// 被限流。
    RateLimited {
        message: String,
        /// 建议等待秒数（来自 Retry-After 头）。
        retry_after_secs: Option<u64>,
    },
    /// 网络层错误（DNS / 连接被拒 / TLS 握手失败）。
    Network {
        message: String,
    },
    /// 服务端错误（5xx）。
    Server {
        status: u16,
        message: String,
    },
    /// 响应格式异常（非预期 JSON 结构 / 缺失字段）。
    InvalidResponse {
        message: String,
    },
    /// 流式连接中断。
    StreamBroken {
        message: String,
    },
    /// 响应解析失败（JSON 解析 / UTF-8 编码）。
    ParseError {
        message: String,
    },
    /// Provider 自定义错误（透传上游返回的错误描述）。
    ProviderError {
        message: String,
    },
    /// 通用错误（兜底）。
    Other {
        message: String,
    },
}

impl AIError {
    /// 是否应该重试。
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            AIError::Timeout { .. } | AIError::Network { .. } | AIError::Server { .. } | AIError::StreamBroken { .. } | AIError::StreamTimeout { .. }
        )
    }

    /// 是否表示认证/授权问题（重试无意义）。
    pub fn is_auth_error(&self) -> bool {
        matches!(self, AIError::Unauthorized { .. } | AIError::Forbidden { .. })
    }

    /// 是否表示限流（应等待后重试）。
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, AIError::RateLimited { .. })
    }
}

impl fmt::Display for AIError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AIError::Timeout { message, retries } => {
                write!(f, "超时(已重试 {retries} 次): {message}")
            }
            AIError::StreamTimeout { message } => {
                write!(f, "流式超时: {message}")
            }
            AIError::Cancelled => write!(f, "已取消"),
            AIError::Unauthorized { message } => write!(f, "认证失败: {message}"),
            AIError::Forbidden { message } => write!(f, "无权限: {message}"),
            AIError::RateLimited {
                message,
                retry_after_secs,
            } => {
                if let Some(s) = retry_after_secs {
                    write!(f, "限流(等待 {s}s): {message}")
                } else {
                    write!(f, "限流: {message}")
                }
            }
            AIError::Network { message } => write!(f, "网络错误: {message}"),
            AIError::Server { status, message } => write!(f, "服务端错误({status}): {message}"),
            AIError::InvalidResponse { message } => write!(f, "响应格式异常: {message}"),
            AIError::StreamBroken { message } => write!(f, "流中断: {message}"),
            AIError::ParseError { message } => write!(f, "解析错误: {message}"),
            AIError::ProviderError { message } => write!(f, "Provider 错误: {message}"),
            AIError::Other { message } => write!(f, "错误: {message}"),
        }
    }
}

impl std::error::Error for AIError {}

// ─── 从现有错误类型构造 AIError ───

/// 将 provider 层的字符串错误转为 AIError。
impl From<String> for AIError {
    fn from(msg: String) -> Self {
        AIError::Other { message: msg }
    }
}

/// 将 `&str` 错误转为 AIError。
impl From<&str> for AIError {
    fn from(msg: &str) -> Self {
        AIError::Other {
            message: msg.to_string(),
        }
    }
}

impl From<std::io::Error> for AIError {
    fn from(e: std::io::Error) -> Self {
        AIError::Other {
            message: e.to_string(),
        }
    }
}

impl From<serde_json::Error> for AIError {
    fn from(e: serde_json::Error) -> Self {
        AIError::ParseError {
            message: e.to_string(),
        }
    }
}
