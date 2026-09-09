//! 上下文采集：把应用运行时状态转成结构化文本。
//!
//! 本模块需要读取运行时实体，因此依赖 gpui / workspace / terminal。
//! 输出的均为原始文本，i18n 文案由 [`crate::AiClient`] 负责。

pub mod compression;
pub use compression::{
    compress_chat_history, estimate_messages_tokens, estimate_tokens, AiCompressionStrategy,
    SimpleChatMessage,
};

use gpui::{App, Entity};

use velowork_i18n::i18n;
use velowork_terminal::terminal::Terminal;
use velowork_terminal::TerminalsRegistry;
use velowork_workspace::focus::FocusManager;
use velowork_workspace::state::{LayoutNode, Workspace};

/// 读取当前聚焦终端的屏幕内容（原始文本）。
pub fn read_focused_terminal(
    fm: &Entity<FocusManager>,
    ws: &Entity<Workspace>,
    terms: &TerminalsRegistry,
    cx: &App,
) -> Option<String> {
    let state = fm.read(cx).focused_terminal_state()?;
    let terminal_id = ws
        .read(cx)
        .project(&state.project_id)
        .and_then(|p| p.layout.as_ref())
        .and_then(|layout| layout.get_at_path(&state.layout_path))
        .and_then(|node| match node {
            LayoutNode::Terminal { terminal_id, .. } => terminal_id.clone(),
            _ => None,
        })?;

    let guard = terms.lock();
    let term = guard.get(&terminal_id)?;
    Some(terminal_screen_text(term))
}

fn terminal_screen_text(term: &Terminal) -> String {
    let raw = term.with_content(|t| {
        let grid = t.grid();
        let mut out = String::new();
        let mut line = String::new();
        let mut prev: Option<_> = None;
        for indexed in grid.display_iter() {
            if let Some(p) = prev {
                if indexed.point.line != p {
                    out.push_str(line.trim_end());
                    out.push('\n');
                    line.clear();
                }
            }
            prev = Some(indexed.point.line);
            line.push(indexed.cell.c);
        }
        out.push_str(line.trim_end());
        out
    });
    mask_sensitive_data(&raw)
}

/// 列出当前所有会话（i18n 文案）。
pub fn list_sessions(ws: &Entity<Workspace>, cx: &App) -> String {
    let projects = ws.read(cx).projects();
    if projects.is_empty() {
        return i18n!(cx, "ai_assistant.no_sessions");
    }
    let mut s = String::from(i18n!(cx, "ai_assistant.sessions_header")) + "\n";
    for p in projects {
        let kind = if p.is_remote { "remote" } else { "local" };
        s.push_str(&format!("- {} [{}]\n", p.name, kind));
    }
    s
}

/// 查询单个会话的配置（i18n 文案）。
pub fn session_config(name: &str, ws: &Entity<Workspace>, cx: &App) -> String {
    let projects = ws.read(cx).projects();
    let proj = projects
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name) || p.id == name);
    match proj {
        Some(p) => format!(
            "{}:\n  name: {}\n  id: {}\n  path: {}\n  is_remote: {}\n  connection_id: {:?}\n  pinned: {}\n  last_activity_at: {:?}",
            i18n!(cx, "ai_assistant.session_config"),
            p.name, p.id, p.path, p.is_remote, p.connection_id, p.pinned, p.last_activity_at
        ),
        None => format!("{}: {}", i18n!(cx, "ai_assistant.session_notfound"), name),
    }
}

/// 实时采集的「当前上下文」快照：每次 AI 调用前采集一次，注入 system prompt 与记忆。
///
/// 覆盖用户最关心的现场信息——连接的是哪台服务器、当前 Tab、当前 Workspace、
/// 工作目录、以及终端中已选中的文本。这样用户说「帮我看看这个错误」时，AI 无需
/// 让用户先手动复制终端内容，上下文已自动就位。
#[derive(Clone, Debug, Default)]
pub struct LiveContext {
    /// 当前聚焦的会话 / 项目名（即「连接的是哪台服务器」）。
    pub project: Option<String>,
    /// 该会话是否为远程连接。
    pub is_remote: bool,
    /// 当前聚焦的终端 Tab 名称。
    pub tab: Option<String>,
    /// 当前 Workspace（会话容器）。
    pub workspace: Option<String>,
    /// 当前工作目录（来自 OSC 7，回退到 PTY 初始目录）。
    pub cwd: Option<String>,
    /// 当前终端中选中的文本（用户复制前的上下文）。
    pub selection: Option<String>,
}

impl LiveContext {
    /// 渲染为可注入 system prompt 的 Markdown 段落。
    pub fn to_block(&self) -> String {
        let mut s = String::from("## Active context (auto-captured)\n");
        if let Some(p) = &self.project {
            let kind = if self.is_remote { "remote" } else { "local" };
            s.push_str(&format!("- Connected session: {} [{}]\n", p, kind));
            if let Some(w) = &self.workspace {
                s.push_str(&format!("- Workspace: {}\n", w));
            }
        }
        if let Some(t) = &self.tab {
            s.push_str(&format!("- Active tab: {}\n", t));
        }
        if let Some(c) = &self.cwd {
            s.push_str(&format!("- Working directory: {}\n", c));
        }
        if let Some(sel) = &self.selection
            && !sel.trim().is_empty()
        {
            let masked = mask_sensitive_data(sel);
            s.push_str(&format!("- Selected text:\n```\n{}\n```\n", masked));
        }
        s
    }
}

use std::sync::LazyLock;

static RE_API_KEY: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"sk-[a-zA-Z0-9_-]{16,}").expect("valid regex"));
static RE_BEARER: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i)bearer\s+[a-zA-Z0-9\._-]{16,}").expect("valid regex"));
static RE_AWS: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"AKIA[0-9A-Z]{16}").expect("valid regex"));
static RE_PRIVATE_KEY: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"-----BEGIN (?:[A-Z]+ )?PRIVATE KEY-----[\s\S]*?-----END (?:[A-Z]+ )?PRIVATE KEY-----",
    )
    .expect("valid regex")
});
static RE_SECRET_KV: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)(password|passwd|secret|passphrase)\s*[:=]\s*\S+").expect("valid regex")
});

/// 对发送给 AI 的文本中包含的敏感凭据（API Keys, Bearer Tokens, Passwords, 私钥）进行脱敏。
pub fn mask_sensitive_data(input: &str) -> String {
    let mut masked = input.to_string();

    // 1. API Keys (e.g. sk-...)
    masked = RE_API_KEY.replace_all(&masked, "[REDACTED_API_KEY]").to_string();
    // 2. Bearer Tokens
    masked = RE_BEARER.replace_all(&masked, "Bearer [REDACTED_TOKEN]").to_string();
    // 3. AWS Access Keys
    masked = RE_AWS.replace_all(&masked, "[REDACTED_AWS_KEY]").to_string();
    // 4. Private Keys
    masked = RE_PRIVATE_KEY.replace_all(&masked, "[REDACTED_PRIVATE_KEY]").to_string();
    // 5. Password/Secret key-value pairs
    masked = RE_SECRET_KV.replace_all(&masked, "$1: [REDACTED]").to_string();

    masked
}

/// 从运行时实体采集当前实时上下文。
///
/// 读取聚焦终端对应的会话名、Tab 名、工作目录与选中文本。任何一步缺失都不影响
/// 其余字段的采集（优雅降级）。
pub fn capture_live_context(
    fm: &Entity<FocusManager>,
    ws: &Entity<Workspace>,
    terms: &TerminalsRegistry,
    cx: &App,
) -> LiveContext {
    let mut live = LiveContext::default();
    let state = match fm.read(cx).focused_terminal_state() {
        Some(s) => s,
        None => return live,
    };

    // 解析会话与聚焦终端 ID（保持 ws 读锁作用域内完成）。
    let (project, terminal_id) = {
        let ws_read = ws.read(cx);
        let project = ws_read.project(&state.project_id).cloned();
        let terminal_id = ws_read
            .project(&state.project_id)
            .and_then(|p| p.layout.as_ref())
            .and_then(|layout| layout.get_at_path(&state.layout_path))
            .and_then(|node| match node {
                LayoutNode::Terminal { terminal_id, .. } => terminal_id.clone(),
                _ => None,
            });
        (project, terminal_id)
    };

    if let Some(p) = &project {
        live.project = Some(p.name.clone());
        live.is_remote = p.is_remote;
        live.workspace = Some(p.name.clone());
    }

    if let Some(id) = terminal_id {
        let guard = terms.lock();
        if let Some(term) = guard.get(&id) {
            if let Some(p) = &project {
                live.tab = Some(p.terminal_display_name(&id, term.title()));
            }
            live.cwd = Some(term.current_cwd());
            if term.has_selection() {
                live.selection = term.get_selected_text();
            }
        }
    }

    live
}
