//! AI 运行时（纯逻辑，无 gpui / i18n 依赖）。
//!
//! 模块：
//! - `error`  — 统一错误类型 [`AIError`]
//! - `task`   — 请求 ID / 生命周期状态 / 取消令牌 / 流式事件
//! - `manager` — 任务管理器 [`AITaskManager`]
//!
//! 权限门禁：所有命令执行类工具在真正执行前必须过 [`AiPermission::allows`]。
//!
//! 架构原则：
//! - UI 永远不等待网络
//! - 网络永远不持有 UI 对象
//! - 所有状态变化通过事件回到主线程
//! - 所有请求都可取消、可超时、可重试、可回收

pub mod error;
pub mod manager;
pub mod task;

pub use error::AIError;
pub use manager::AITaskManager;
pub use task::{
    AIRequest, AIResponse, CancelToken, RequestId, RequestState, RetryConfig, StreamEvent,
    TimeoutConfig,
};

// ─── 权限门禁（保留原有代码） ──────────────────────────────────────────

/// AI 命令执行权限。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AiPermission {
    Deny,
    ReadOnly,
    AllowAll,
}

impl AiPermission {
    pub fn all() -> &'static [AiPermission] {
        &[
            AiPermission::Deny,
            AiPermission::ReadOnly,
            AiPermission::AllowAll,
        ]
    }

    /// 判断给定命令是否被允许执行。
    pub fn allows(self, cmd: &str) -> bool {
        match self {
            AiPermission::Deny => false,
            AiPermission::AllowAll => true,
            AiPermission::ReadOnly => is_readonly_command(cmd),
        }
    }

    pub fn index(self) -> usize {
        AiPermission::all()
            .iter()
            .position(|p| *p == self)
            .unwrap_or(0)
    }
}

const DANGEROUS_TOKENS: &[&str] = &[
    "rm", "mkfs", "dd", "shutdown", "reboot", "halt", "chmod", "chown", "userdel", "passwd",
    "sudo", "mv", "cp", "kill", "pkill", "curl", "wget", "tee", "format", "fdisk", "parted",
    ":(){", ">", "mkfs",
];

const READONLY_COMMANDS: &[&str] = &[
    "free", "ps", "top", "htop", "ls", "cat", "pwd", "echo", "df", "du", "uname", "whoami",
    "id", "env", "printenv", "ifconfig", "ip", "netstat", "ss", "docker ps", "git status",
    "git log", "git diff", "history", "head", "tail", "grep", "find", "date", "uptime", "w",
    "who", "mount", "crontab -l", "awk", "sed", "sort", "uniq", "wc",
];

/// 判断命令是否为只读（安全）命令。
pub fn is_readonly_command(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    if DANGEROUS_TOKENS.iter().any(|d| lower.contains(d)) {
        return false;
    }
    lower
        .split('|')
        .map(str::trim)
        .all(|seg| {
            let seg = seg.trim_start_matches(['-', ' ']);
            READONLY_COMMANDS.iter().any(|r| seg.starts_with(r))
        })
}
