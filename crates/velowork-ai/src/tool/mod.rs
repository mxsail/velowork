//! 工具系统：Tool trait + 内置工具 + 注册表。
//!
//! 本模块依赖 gpui / workspace / terminal（读取运行时实体与发送字节），
//! 以及 velowork-files（日志内容搜索）。
//!
//! 所有「命令执行类」工具在执行前必须经过 [`crate::runtime::AiPermission`] 门禁。

use std::path::Path;
use std::sync::{Arc, Mutex};

use gpui::{App, Entity};
use serde_json::{json, Value};
use velowork_i18n::i18n;
use velowork_terminal::TerminalsRegistry;
use velowork_workspace::focus::FocusManager;
use velowork_workspace::state::Workspace;

use crate::context::{list_sessions, read_focused_terminal, session_config};
use crate::runtime::AiPermission;
use crate::skill::memory::AiMemory;

pub mod mcp;
pub use mcp::{discover_mcp_tools, McpServerConfig, McpTool, McpToolSpec};

/// 工具执行错误。
#[derive(Debug)]
pub enum ToolError {
    InvalidArgs(String),
    PermissionDenied,
    Execution(String),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolError::InvalidArgs(m) => write!(f, "invalid args: {}", m),
            ToolError::PermissionDenied => write!(f, "permission denied"),
            ToolError::Execution(m) => write!(f, "execution error: {}", m),
        }
    }
}

/// 工具执行上下文：持有运行时实体、权限级别与跨调用记忆。
pub struct ToolCtx {
    pub focus_manager: Entity<FocusManager>,
    pub workspace: Entity<Workspace>,
    pub terminals: TerminalsRegistry,
    pub permission: AiPermission,
    /// 跨对话 / 技能调用的记忆（命令执行类工具会写入命令与错误）。
    pub memory: Arc<Mutex<AiMemory>>,
}

/// 工具抽象。内置工具与未来 MCP 工具都实现此 trait。
///
/// `execute` 同步执行（命令发送 / 文件读写 / 日志搜索均为阻塞 IO），
/// 由上层 agent 循环在异步任务中调用。
///
/// 需要 `Send + Sync`：工具注册表会被移动到后台异步任务中执行，且 MCP 工具
/// 通过 `Arc<Mutex<_>>` 跨线程共享连接。
pub trait Tool: Send + Sync {
    /// 工具名（LLM 调用时使用的标识，需全局唯一）。
    fn name(&self) -> &str;
    /// 人类可读描述（注入 system prompt，供 LLM 选择工具）。
    fn description(&self) -> &str;
    /// 参数 JSON Schema（用于 LLM 生成 tool_call 的 `arguments`）。
    fn schema(&self) -> Value;
    /// 执行工具。返回工具输出文本（回填给 LLM）或错误。
    fn execute(&self, args: Value, ctx: &ToolCtx, cx: &App) -> Result<String, ToolError>;
}

/// 工具注册表：聚合所有可用工具，按名称查找与调用。
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new(tools: Vec<Box<dyn Tool>>) -> Self {
        Self { tools }
    }

    /// 按名称查找工具。
    pub fn find(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|t| t.name() == name)
            .map(|b| b.as_ref())
    }

    /// 列出全部已注册工具。
    pub fn list(&self) -> &[Box<dyn Tool>] {
        &self.tools
    }

    /// 按名称调用工具。未知工具名视为参数错误。
    pub fn call(
        &self,
        name: &str,
        args: Value,
        ctx: &ToolCtx,
        cx: &App,
    ) -> Result<String, ToolError> {
        match self.find(name) {
            Some(t) => t.execute(args, ctx, cx),
            None => Err(ToolError::InvalidArgs(format!("unknown tool: {}", name))),
        }
    }
}

/// 构造全部内置工具。
pub fn builtin_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(RunTerminalCommand),
        Box::new(GetTerminalBlocks),
        Box::new(GetTerminalOutput),
        Box::new(ReadTerminalScreen),
        Box::new(ListSessions),
        Box::new(GetSessionConfig),
        Box::new(ReadFile),
        Box::new(WriteFile),
        Box::new(SearchLogs),
        Box::new(GenerateConfig),
        Box::new(ListWorkflows),
        Box::new(RunWorkflow),
    ]
}

// ---------------------------------------------------------------------------
// 低层发送（不含权限判断，供工具与面板复用）
// ---------------------------------------------------------------------------

/// 获取当前聚焦的终端实例（若存在）。
pub fn get_focused_terminal(
    focus_manager: &Entity<FocusManager>,
    workspace: &Entity<Workspace>,
    terminals: &TerminalsRegistry,
    cx: &App,
) -> Option<Arc<velowork_terminal::terminal::Terminal>> {
    let terminal_id = focus_manager
        .read(cx)
        .focused_terminal_state()
        .and_then(|state| {
            workspace
                .read(cx)
                .project(&state.project_id)
                .and_then(|p| p.layout.as_ref())
                .and_then(|layout| layout.get_at_path(&state.layout_path))
                .and_then(|node| match node {
                    velowork_workspace::state::LayoutNode::Terminal { terminal_id, .. } => {
                        terminal_id.clone()
                    }
                    _ => None,
                })
        })?;

    let guard = terminals.lock();
    guard.get(&terminal_id).cloned()
}

/// 在聚焦终端上执行命令并等待捕获其结构化输出与退出码。
pub fn execute_and_capture_on_focused_terminal(
    focus_manager: &Entity<FocusManager>,
    workspace: &Entity<Workspace>,
    terminals: &TerminalsRegistry,
    cmd: &str,
    timeout: std::time::Duration,
    cx: &App,
) -> Result<velowork_terminal::terminal::TerminalBlock, String> {
    let terminal = get_focused_terminal(focus_manager, workspace, terminals, cx)
        .ok_or_else(|| "no focused terminal found".to_string())?;

    let block_res = if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| {
            handle.block_on(terminal.execute_command_and_capture(cmd, timeout))
        })
    } else {
        futures::executor::block_on(terminal.execute_command_and_capture(cmd, timeout))
    };

    block_res.map_err(|e| e.to_string())
}

/// 将命令发送到当前聚焦的终端（低层发送，不含权限判断）。
pub fn send_command_to_focused_terminal(
    focus_manager: &Entity<FocusManager>,
    workspace: &Entity<Workspace>,
    terminals: &TerminalsRegistry,
    cmd: &str,
    cx: &App,
) {
    let terminal_id = focus_manager
        .read(cx)
        .focused_terminal_state()
        .and_then(|state| {
            workspace
                .read(cx)
                .project(&state.project_id)
                .and_then(|p| p.layout.as_ref())
                .and_then(|layout| layout.get_at_path(&state.layout_path))
                .and_then(|node| match node {
                    velowork_workspace::state::LayoutNode::Terminal { terminal_id, .. } => {
                        terminal_id.clone()
                    }
                    _ => None,
                })
        });

    if let Some(id) = terminal_id {
        let terminals = terminals.lock();
        if let Some(terminal) = terminals.get(&id) {
            let mut cmd_str = cmd.to_string();
            if !cmd_str.ends_with('\n') && !cmd_str.ends_with('\r') {
                cmd_str.push('\r');
            }
            terminal.send_bytes(cmd_str.as_bytes());
        }
    }
}

/// 经权限门禁后执行命令，返回结果文案（i18n）。
pub fn execute_ai_command(
    cmd: &str,
    perm: AiPermission,
    fm: &Entity<FocusManager>,
    ws: &Entity<Workspace>,
    terms: &TerminalsRegistry,
    cx: &App,
) -> String {
    if !perm.allows(cmd) {
        return i18n!(cx, "ai.perm_denied");
    }
    send_command_to_focused_terminal(fm, ws, terms, cmd, cx);
    format!("{}: {}", i18n!(cx, "ai.executed"), cmd)
}

// ---------------------------------------------------------------------------
// 内置工具
// ---------------------------------------------------------------------------

/// 向聚焦终端发送并执行命令，并捕获执行结果与退出码（受权限门禁控制）。
pub struct RunTerminalCommand;

impl Tool for RunTerminalCommand {
    fn name(&self) -> &str {
        "run_terminal_command"
    }
    fn description(&self) -> &str {
        "Send a shell command to the currently focused terminal, execute it, and return its output and exit code. \
         Read-only commands are always allowed; other commands are gated by the current permission level."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The shell command to execute." },
                "timeout_seconds": { "type": "integer", "description": "Maximum seconds to wait for output (default 25)." }
            },
            "required": ["command"]
        })
    }
    fn execute(&self, args: Value, ctx: &ToolCtx, cx: &App) -> Result<String, ToolError> {
        let cmd = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'command'".into()))?;
        if !ctx.permission.allows(cmd) {
            if let Ok(mut mem) = ctx.memory.lock() {
                mem.record_error(&format!("blocked command: {}", cmd));
            }
            return Err(ToolError::PermissionDenied);
        }

        let timeout_secs = args
            .get("timeout_seconds")
            .and_then(|v| v.as_u64())
            .unwrap_or(25);
        let timeout = std::time::Duration::from_secs(timeout_secs);

        match execute_and_capture_on_focused_terminal(
            &ctx.focus_manager,
            &ctx.workspace,
            &ctx.terminals,
            cmd,
            timeout,
            cx,
        ) {
            Ok(block) => {
                let status_str = match block.exit_code {
                    Some(0) => "Command succeeded (exit code: 0)".to_string(),
                    Some(code) => format!("Command failed (exit code: {})", code),
                    None => "Command executed (exit code: unknown)".to_string(),
                };
                let dur_str = block
                    .duration_ms
                    .map(|d| format!(" in {}ms", d))
                    .unwrap_or_default();
                let output = if block.clean_output.trim().is_empty() {
                    "(no output)".to_string()
                } else {
                    crate::context::mask_sensitive_data(&block.clean_output)
                };

                if let Ok(mut mem) = ctx.memory.lock() {
                    mem.record_command(cmd);
                    if let Some(code) = block.exit_code
                        && code != 0
                    {
                        mem.record_error(&format!("command '{}' exited with code {}", cmd, code));
                    }
                }

                Ok(format!("{status_str}{dur_str}:\n```\n{output}\n```"))
            }
            Err(e) => {
                // Fallback: send directly and inform caller
                send_command_to_focused_terminal(
                    &ctx.focus_manager,
                    &ctx.workspace,
                    &ctx.terminals,
                    cmd,
                    cx,
                );
                if let Ok(mut mem) = ctx.memory.lock() {
                    mem.record_command(cmd);
                }
                Ok(format!(
                    "{}: {} (capture degraded: {})",
                    i18n!(cx, "ai.executed"),
                    cmd,
                    e
                ))
            }
        }
    }
}

/// 查询聚焦终端的历史执行块（Blocks）。
pub struct GetTerminalBlocks;

impl Tool for GetTerminalBlocks {
    fn name(&self) -> &str {
        "get_terminal_blocks"
    }
    fn description(&self) -> &str {
        "List recent structured terminal command blocks with their commands, outputs, exit codes, and durations."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": { "type": "integer", "description": "Maximum number of recent blocks to retrieve (default 5)." }
            }
        })
    }
    fn execute(&self, args: Value, ctx: &ToolCtx, cx: &App) -> Result<String, ToolError> {
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
        let terminal = get_focused_terminal(&ctx.focus_manager, &ctx.workspace, &ctx.terminals, cx)
            .ok_or_else(|| ToolError::Execution("no focused terminal found".into()))?;
        let blocks = terminal.blocks();
        if blocks.is_empty() {
            return Ok("No command blocks recorded yet.".to_string());
        }
        let take_count = blocks.len().min(limit);
        let start_idx = blocks.len().saturating_sub(take_count);
        let mut out = String::new();
        for b in &blocks[start_idx..] {
            let status = match b.exit_code {
                Some(0) => "SUCCESS (0)".to_string(),
                Some(code) => format!("FAILED ({})", code),
                None => "UNKNOWN".to_string(),
            };
            let dur = b.duration_ms.map(|d| format!(" ({}ms)", d)).unwrap_or_default();
            out.push_str(&format!(
                "### Block #{}: `{}` [{}]{}\nOutput:\n```\n{}\n```\n\n",
                b.id,
                b.command,
                status,
                dur,
                crate::context::mask_sensitive_data(&b.clean_output)
            ));
        }
        Ok(out)
    }
}

/// 读取指定终端（包括非聚焦的其它终端分屏）的最新输出或历史 Block。
pub struct GetTerminalOutput;

impl Tool for GetTerminalOutput {
    fn name(&self) -> &str {
        "get_terminal_output"
    }
    fn description(&self) -> &str {
        "Read recent output lines or last command block from a specific terminal session by terminal_id."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "terminal_id": {
                    "type": "string",
                    "description": "ID of the target terminal session to inspect."
                },
                "max_lines": {
                    "type": "integer",
                    "description": "Maximum number of recent lines to retrieve (default 50, max 200)."
                }
            },
            "required": ["terminal_id"]
        })
    }
    fn execute(&self, args: Value, ctx: &ToolCtx, _cx: &App) -> Result<String, ToolError> {
        let terminal_id = args
            .get("terminal_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing terminal_id".into()))?;

        let max_lines = args
            .get("max_lines")
            .and_then(|v| v.as_u64())
            .map(|n| (n as usize).clamp(1, 200))
            .unwrap_or(50);

        let term_arc = {
            let guard = ctx.terminals.lock();
            guard.get(terminal_id).cloned()
        };

        let Some(term) = term_arc else {
            return Err(ToolError::Execution(format!("terminal '{terminal_id}' not found")));
        };

        let title = term.title().unwrap_or_else(|| "Terminal".to_string());
        let cwd = term.current_cwd();
        let recent = term.get_recent_lines(max_lines);
        let cleaned = crate::context::strip_ansi(&recent);
        let masked = crate::context::mask_sensitive_data(&cleaned);

        let last_block_info = {
            let blocks = term.blocks();
            if let Some(last_b) = blocks.last() {
                let status = match last_b.exit_code {
                    Some(0) => "SUCCESS (0)".to_string(),
                    Some(code) => format!("FAILED ({code})"),
                    None => "RUNNING / UNKNOWN".to_string(),
                };
                format!("\nLast Command: `{}` [{status}]\n", last_b.command)
            } else {
                String::new()
            }
        };

        Ok(format!(
            "### Terminal `{title}` ({terminal_id})\nCWD: `{cwd}`{last_block_info}\nOutput:\n```\n{masked}\n```"
        ))
    }
}

/// 读取聚焦终端当前屏幕内容。
pub struct ReadTerminalScreen;

impl Tool for ReadTerminalScreen {
    fn name(&self) -> &str {
        "read_terminal_screen"
    }
    fn description(&self) -> &str {
        "Read the current visible content of the focused terminal's screen."
    }
    fn schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn execute(&self, _args: Value, ctx: &ToolCtx, cx: &App) -> Result<String, ToolError> {
        match read_focused_terminal(&ctx.focus_manager, &ctx.workspace, &ctx.terminals, cx) {
            Some(c) if !c.trim().is_empty() => Ok(c),
            Some(_) => Ok(i18n!(cx, "ai.terminal_empty")),
            None => Ok(i18n!(cx, "ai.no_terminal")),
        }
    }
}

/// 列出所有会话。
pub struct ListSessions;

impl Tool for ListSessions {
    fn name(&self) -> &str {
        "list_sessions"
    }
    fn description(&self) -> &str {
        "List all configured SSH/local sessions and their connection state."
    }
    fn schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn execute(&self, _args: Value, ctx: &ToolCtx, cx: &App) -> Result<String, ToolError> {
        Ok(list_sessions(&ctx.workspace, cx))
    }
}

/// 查询单个会话配置。
pub struct GetSessionConfig;

impl Tool for GetSessionConfig {
    fn name(&self) -> &str {
        "get_session_config"
    }
    fn description(&self) -> &str {
        "Get the configuration details of a single session by name or id."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Session name or id." }
            },
            "required": ["name"]
        })
    }
    fn execute(&self, args: Value, ctx: &ToolCtx, cx: &App) -> Result<String, ToolError> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'name'".into()))?;
        Ok(session_config(name, &ctx.workspace, cx))
    }
}

/// 读取本地文件内容（SFTP 远程文件为未来扩展）。
pub struct ReadFile;

impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read the text content of a local file by absolute path."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute file path." }
            },
            "required": ["path"]
        })
    }
    fn execute(&self, args: Value, _ctx: &ToolCtx, cx: &App) -> Result<String, ToolError> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'path'".into()))?;
        std::fs::read_to_string(path).map_err(|e| {
            ToolError::Execution(format!("{}: {}", i18n!(cx, "ai.read_failed"), e))
        })
    }
}

/// 写入本地文件内容（SFTP 远程文件为未来扩展）。
pub struct WriteFile;

impl Tool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }
    fn description(&self) -> &str {
        "Write text content to a local file at the given absolute path, creating or overwriting it."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute file path." },
                "content": { "type": "string", "description": "Text content to write." }
            },
            "required": ["path", "content"]
        })
    }
    fn execute(&self, args: Value, _ctx: &ToolCtx, cx: &App) -> Result<String, ToolError> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'path'".into()))?;
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'content'".into()))?;
        std::fs::write(path, content).map_err(|e| {
            ToolError::Execution(format!("{}: {}", i18n!(cx, "ai.write_failed"), e))
        })?;
        Ok(format!("{}: {}", i18n!(cx, "ai.write_ok"), path))
    }
}

/// 在目录/文件中按模式搜索内容（用于日志分析等）。
pub struct SearchLogs;

impl Tool for SearchLogs {
    fn name(&self) -> &str {
        "search_logs"
    }
    fn description(&self) -> &str {
        "Search file contents (e.g. logs) under a directory for a pattern, returning matching lines with file and line numbers."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Literal or regex pattern to search." },
                "path": { "type": "string", "description": "Directory or file to search." },
                "regex": { "type": "boolean", "description": "Treat pattern as regex. Default false (literal)." }
            },
            "required": ["pattern", "path"]
        })
    }
    fn execute(&self, args: Value, _ctx: &ToolCtx, cx: &App) -> Result<String, ToolError> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'pattern'".into()))?;
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'path'".into()))?;
        let regex = args.get("regex").and_then(|v| v.as_bool()).unwrap_or(false);

        let p = Path::new(path);
        if !p.exists() {
            return Err(ToolError::Execution(format!(
                "{}: {}",
                i18n!(cx, "ai.search_no_path"),
                path
            )));
        }

        let mut out = String::new();
        let mut count = 0usize;

        let re = if regex {
            regex::RegexBuilder::new(pattern)
                .case_insensitive(true)
                .build()
                .map_err(|e| ToolError::InvalidArgs(format!("Invalid regex pattern: {}", e)))?
        } else {
            let escaped = regex::escape(pattern);
            regex::RegexBuilder::new(&escaped)
                .case_insensitive(true)
                .build()
                .map_err(|e| ToolError::InvalidArgs(format!("Invalid pattern: {}", e)))?
        };

        fn search_dir(
            root: &Path,
            dir: &Path,
            re: &regex::Regex,
            count: &mut usize,
            out: &mut String,
        ) {
            if *count >= 200 {
                return;
            }
            if dir.is_file() {
                if let Ok(file) = std::fs::File::open(dir) {
                    use std::io::{BufRead, BufReader};
                    let reader = BufReader::new(file);
                    let rel = dir.strip_prefix(root).unwrap_or(dir).display().to_string();
                    for (line_idx, line_res) in reader.lines().enumerate() {
                        if *count >= 200 {
                            break;
                        }
                        if let Ok(line) = line_res {
                            if re.is_match(&line) {
                                *count += 1;
                                out.push_str(&format!("{}:{}: {}\n", rel, line_idx + 1, line));
                            }
                        }
                    }
                }
            } else if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    if *count >= 200 {
                        break;
                    }
                    let p = entry.path();
                    search_dir(root, &p, re, count, out);
                }
            }
        }

        search_dir(p, p, &re, &mut count, &mut out);

        if count == 0 {
            out.push_str(&i18n!(cx, "ai.search_none"));
        }
        Ok(out)
    }
}

/// 生成配置片段模板（ssh / nginx / systemd / docker-compose）。
pub struct GenerateConfig;

impl Tool for GenerateConfig {
    fn name(&self) -> &str {
        "generate_config"
    }
    fn description(&self) -> &str {
        "Generate a starter configuration snippet for a given kind: ssh, nginx, systemd, or docker-compose."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "kind": { "type": "string", "description": "One of: ssh, nginx, systemd, docker-compose." },
                "name": { "type": "string", "description": "Identifier used inside the template (default 'app')." }
            },
            "required": ["kind"]
        })
    }
    fn execute(&self, args: Value, _ctx: &ToolCtx, cx: &App) -> Result<String, ToolError> {
        let kind = args
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'kind'".into()))?;
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("app");
        let body = match kind.to_lowercase().as_str() {
            "ssh" => format!(
                "Host {name}\n  HostName example.com\n  User deploy\n  Port 22\n  IdentityFile ~/.ssh/id_ed25519\n",
                name = name
            ),
            "nginx" => format!(
                "server {{\n  listen 80;\n  server_name {name};\n  location / {{\n    proxy_pass http://127.0.0.1:3000;\n  }}\n}}\n",
                name = name
            ),
            "systemd" => format!(
                "[Unit]\nDescription={name}\n\n[Service]\nExecStart=/usr/bin/{name}\nRestart=on-failure\n\n[Install]\nWantedBy=multi-user.target\n",
                name = name
            ),
            "docker-compose" | "compose" => format!(
                "services:\n  {name}:\n    image: {name}:latest\n    restart: unless-stopped\n    ports:\n      - \"8080:8080\"\n",
                name = name
            ),
            other => {
                return Err(ToolError::Execution(format!(
                    "{}: {}",
                    i18n!(cx, "ai.config_unknown"),
                    other
                )))
            }
        };
        Ok(format!("```\n{}\n```", body))
    }
}

/// 列出所有可用的多步工作流。
pub struct ListWorkflows;

impl Tool for ListWorkflows {
    fn name(&self) -> &str {
        "list_workflows"
    }
    fn description(&self) -> &str {
        "List all available automated multi-step workflows with their IDs, names, and descriptions."
    }
    fn schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn execute(&self, _args: Value, _ctx: &ToolCtx, _cx: &App) -> Result<String, ToolError> {
        let registry = crate::workflow::WorkflowRegistry::default();
        let list = registry.list();
        let mut out = String::from("Available Workflows:\n");
        for wf in list {
            out.push_str(&format!(
                "- `{}`: {} ({} steps)\n  {}\n",
                wf.id,
                wf.name,
                wf.steps.len(),
                wf.description
            ));
        }
        Ok(out)
    }
}

/// 执行指定的多步自动化工作流。
pub struct RunWorkflow;

impl Tool for RunWorkflow {
    fn name(&self) -> &str {
        "run_workflow"
    }
    fn description(&self) -> &str {
        "Execute a predefined multi-step automated workflow by its ID."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "workflow_id": {
                    "type": "string",
                    "description": "The unique identifier of the workflow to run (e.g. 'service_health_check', 'git_safe_precommit', 'quick_diagnostics')."
                },
                "auto_confirm": {
                    "type": "boolean",
                    "description": "Whether to automatically approve confirmation steps. Defaults to false."
                }
            },
            "required": ["workflow_id"]
        })
    }
    fn execute(&self, args: Value, ctx: &ToolCtx, _cx: &App) -> Result<String, ToolError> {
        let wf_id = args
            .get("workflow_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidArgs("missing 'workflow_id'".into()))?;
        let auto_confirm = args
            .get("auto_confirm")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let registry = crate::workflow::WorkflowRegistry::default();
        let workflow = registry.get(wf_id).ok_or_else(|| {
            ToolError::Execution(format!("workflow '{wf_id}' not found"))
        })?;

        // Pluggable runner that routes to target terminal if active, or falls back to system shell
        let terms = ctx.terminals.clone();
        let runner: crate::workflow::executor::CommandRunner = Arc::new(move |cmd, target_id, timeout| {
            if let Some(tid) = target_id {
                let term_opt = {
                    let guard = terms.lock();
                    guard.get(tid).cloned()
                };
                if let Some(term) = term_opt {
                    let block_res = if let Ok(handle) = tokio::runtime::Handle::try_current() {
                        tokio::task::block_in_place(|| {
                            handle.block_on(term.execute_command_and_capture(cmd, timeout))
                        })
                    } else {
                        futures::executor::block_on(term.execute_command_and_capture(cmd, timeout))
                    };
                    return match block_res {
                        Ok(b) => Ok((b.exit_code, b.clean_output)),
                        Err(e) => Err(e.to_string()),
                    };
                }
            }
            // Fallback to system command runner
            let sys = crate::workflow::executor::default_system_command_runner();
            sys(cmd, target_id, timeout)
        });

        let executor =
            crate::workflow::WorkflowExecutor::new(runner).with_auto_confirm(auto_confirm);
        let report = executor.execute(&workflow);

        let mut out = format!(
            "### Workflow Execution Report: {}\nStatus: {:?}\nDuration: {:.2}s\n\nSteps:\n",
            report.workflow_name,
            report.state,
            report.total_duration_ms as f64 / 1000.0
        );
        for (idx, step) in report.step_results.iter().enumerate() {
            let status = if step.success { "PASSED" } else { "FAILED" };
            out.push_str(&format!(
                "{}. [{status}] {} ({}ms)\n",
                idx + 1,
                step.step_name,
                step.duration_ms
            ));
            if !step.output_preview.is_empty() {
                out.push_str(&format!("   Output:\n```\n{}\n```\n", step.output_preview));
            }
            if let Some(err) = &step.error {
                out.push_str(&format!("   Error: {}\n", err));
            }
        }

        Ok(out)
    }
}

