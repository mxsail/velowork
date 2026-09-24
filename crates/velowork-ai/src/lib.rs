//! Velowork AI 能力层门面（facade）。
//!
//! 单一 crate，内部按职责切分为模块：
//! - `provider` — LLM 接入（OpenAI 兼容流式 + 本地规则回退）
//! - `runtime`  — 运行环境与安全门禁（权限校验）
//! - `context`  — 上下文采集（终端屏 / 会话 / 配置）
//! - `tool`     — 工具系统（命令执行等，未来扩展 MCP）
//! - `agent`    — 编排层（Tool-Calling / ReAct 循环，未来 Agent 工作流）
//! - `prompt`   — 提示词模板（按场景组装 system prompt）
//!
//! UI 层只与本 crate 的 [`AiClient`] 交互，不感知内部模块拆分。
//! 依赖关系保持有向无环：provider / runtime / prompt 不依赖 gpui 可独立测试；
//! 仅 `context` / `tool` 需要读取运行时实体（依赖 gpui / workspace / terminal）。

pub mod agent;
pub mod context;
pub mod prompt;
pub mod provider;
pub mod recommendation;
pub mod runtime;
pub mod skill;
pub mod tool;
pub mod workflow;
pub use recommendation::{
    classify_selection_intent, SmartActionKind, SmartActionRecommendation,
};
pub use workflow::{
    ConditionCheck, StepExecutionResult, Workflow, WorkflowExecutionReport, WorkflowExecutor,
    WorkflowRegistry, WorkflowState, WorkflowStep,
};

pub use context::{
    capture_live_context, compress_chat_history, estimate_messages_tokens, estimate_tokens,
    list_sessions, read_focused_terminal, session_config, AiCompressionStrategy, LiveContext,
    SimpleChatMessage, MAX_IMAGES_PER_TURN, MAX_IMAGE_ATTACHMENT_SIZE, MAX_TEXT_ATTACHMENT_SIZE,
    MAX_TOTAL_IMAGES_SIZE,
};
pub use provider::{
    simple_messages_to_api_values, stream_api_reply_simple, AiModel, LlmConfig, LocalRuleProvider,
    RuleReply, StreamChunk, ToolCall, ToolSpec,
};
pub use runtime::{
    AIError, AIRequest, AIResponse, AITaskManager, AiPermission, CancelToken, RequestId,
    RequestState, RetryConfig, StreamEvent, TimeoutConfig,
};
pub use skill::memory::{AiMemory, SharedMemory};
pub use skill::{
    builtin_skills, Skill, SkillCategory, SkillCtx, SkillMeta, SkillRegistry, SkillResult,
    SkillTier, SkillTool,
};
pub use tool::{
    execute_ai_command, send_command_to_focused_terminal, builtin_tools, Tool, ToolCtx, ToolError,
    ToolRegistry,
};
pub use tool::mcp::{discover_mcp_tools, McpServerConfig, McpTool, McpToolSpec};

use gpui::{App, Entity};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use velowork_i18n::i18n;
use velowork_terminal::TerminalsRegistry;
use velowork_workspace::focus::FocusManager;
use velowork_workspace::state::Workspace;

pub use agent::{run_agent_turn, run_agent_turn_with_tool_channel, AgentEvent};
pub use context::{
    capture_terminal_snapshot, head_tail_truncate, strip_ansi, TerminalContextSnapshot,
    TerminalSessionSummary,
};
pub use prompt::{render as render_prompt, render_inline_prompt, ContextBundle, PromptScene};

/// AI 能力门面。UI 层只通过它发起请求，不接触内部引擎。
pub struct AiClient {
    focus_manager: Entity<FocusManager>,
    workspace: Entity<Workspace>,
    terminals: TerminalsRegistry,
    mcp_servers: Vec<McpServerConfig>,
    mcp_cache: RefCell<Option<Vec<McpTool>>>,
    /// 跨对话 / 技能调用的记忆（命令、错误、主机、目录、偏好）。
    memory: Arc<Mutex<AiMemory>>,
}

impl AiClient {
    pub fn new(
        focus_manager: Entity<FocusManager>,
        workspace: Entity<Workspace>,
        terminals: TerminalsRegistry,
    ) -> Self {
        Self {
            focus_manager,
            workspace,
            terminals,
            mcp_servers: Vec::new(),
            mcp_cache: RefCell::new(None),
            memory: Arc::new(Mutex::new(AiMemory::default())),
        }
    }

    /// 从磁盘加载记忆（覆盖默认空记忆）。
    pub fn with_memory(mut self, memory: AiMemory) -> Self {
        self.memory = Arc::new(Mutex::new(memory));
        self
    }

    /// 克隆共享记忆句柄（供 `ToolCtx` / 面板使用）。
    pub fn memory(&self) -> Arc<Mutex<AiMemory>> {
        self.memory.clone()
    }

    /// 配置要接入的 MCP server 列表（懒加载：首次构建工具注册表时启动并发现工具）。
    pub fn with_mcp_servers(mut self, servers: Vec<McpServerConfig>) -> Self {
        self.mcp_servers = servers;
        self
    }

    /// 流式 API 回复（OpenAI 兼容端点）。返回接收流式分片的通道。
    pub fn stream_reply(
        &self,
        base_url: &str,
        api_key: &str,
        model_id: &str,
        messages: &[(String, bool)],
    ) -> mpsc::Receiver<StreamChunk> {
        provider::stream_api_reply(base_url, api_key, model_id, messages)
    }

    /// 支持多模态（文本与图片 Base64 Data URL）的流式 API 回复。
    pub fn stream_reply_simple(
        &self,
        base_url: &str,
        api_key: &str,
        model_id: &str,
        system_prompt: Option<&str>,
        messages: &[SimpleChatMessage],
    ) -> mpsc::Receiver<StreamChunk> {
        provider::stream_api_reply_simple(base_url, api_key, model_id, system_prompt, messages)
    }

    /// 本地规则回退回复。根据输入决策后调用对应上下文 / 工具，并翻译为 i18n 文案。
    pub fn local_reply(&self, input: &str, perm: AiPermission, cx: &App) -> String {
        self.sync_context(cx);
        match LocalRuleProvider::decide(input) {
            RuleReply::TerminalContent => {
                match read_focused_terminal(&self.focus_manager, &self.workspace, &self.terminals, cx)
                {
                    Some(content) if !content.trim().is_empty() => format!(
                        "{}:\n```\n{}\n```",
                        i18n!(cx, "ai.terminal_content"),
                        content
                    ),
                    Some(_) => i18n!(cx, "ai.terminal_empty"),
                    None => i18n!(cx, "ai.no_terminal"),
                }
            }
            RuleReply::ListSessions => list_sessions(&self.workspace, cx),
            RuleReply::SessionConfig(name) => session_config(&name, &self.workspace, cx),
            RuleReply::Exec(cmd) => execute_ai_command(
                &cmd,
                perm,
                &self.focus_manager,
                &self.workspace,
                &self.terminals,
                cx,
            ),
            RuleReply::Script => provider::write_script(input),
            RuleReply::Generic => generic_reply(cx),
        }
    }

    /// 刷新面板上下文信息：会话列表 + 终端连接状态。
    pub fn refresh_context(&self, cx: &App) -> String {
        let sessions = list_sessions(&self.workspace, cx);
        let term_info =
            match read_focused_terminal(&self.focus_manager, &self.workspace, &self.terminals, cx) {
                Some(_) => i18n!(cx, "ai.terminal_connected"),
                None => i18n!(cx, "ai.no_terminal"),
            };
        format!("{}\n{}", sessions, term_info)
    }

    /// 采集实时上下文并写入记忆（供技能 / 本地回退读取）。
    ///
    /// 在每次 AI 调用前调用，使记忆中的「会话 / Tab / Workspace / cwd / 选中文本」
    /// 始终反映当前聚焦现场，无需用户手动复制终端内容。
    pub fn sync_context(&self, cx: &App) {
        let live = capture_live_context(&self.focus_manager, &self.workspace, &self.terminals, cx);
        if let Ok(mut mem) = self.memory.lock() {
            if let Some(p) = &live.project {
                mem.set_project(p);
                mem.set_host(p);
            }
            if let Some(t) = &live.tab {
                mem.set_tab(t);
            }
            if let Some(w) = &live.workspace {
                mem.set_workspace(w);
            }
            if let Some(c) = &live.cwd {
                mem.set_cwd(c);
            }
            if let Some(s) = &live.selection {
                mem.set_selection(s);
            }
        }
    }

    /// 采集实时上下文，组装用于 system prompt 的 [`ContextBundle`]。
    ///
    /// 同时把现场写入记忆。返回的结构体包含聚焦终端屏幕内容与「当前上下文」段落
    /// （会话 / Tab / cwd / 选中文本），可直接交给 [`render_prompt`]。
    pub fn context_bundle(&self, cx: &App) -> ContextBundle {
        let live = capture_live_context(&self.focus_manager, &self.workspace, &self.terminals, cx);
        // 写入记忆，供技能 / 本地回退复用。
        if let Ok(mut mem) = self.memory.lock() {
            if let Some(p) = &live.project {
                mem.set_project(p);
                mem.set_host(p);
            }
            if let Some(t) = &live.tab {
                mem.set_tab(t);
            }
            if let Some(w) = &live.workspace {
                mem.set_workspace(w);
            }
            if let Some(c) = &live.cwd {
                mem.set_cwd(c);
            }
            if let Some(s) = &live.selection {
                mem.set_selection(s);
            }
        }
        let terminal_screen =
            read_focused_terminal(&self.focus_manager, &self.workspace, &self.terminals, cx);
        ContextBundle {
            terminal_screen,
            live_context: Some(live.to_block()),
            ..Default::default()
        }
    }

    /// 构造「底层工具」注册表：内置工具 + 已配置的 MCP server 动态发现的工具。
    ///
    /// 不含 Skill（避免 Skill 嵌套 Skill 的引用环）。MCP 工具在首次调用时懒加载
    /// （启动进程并 `tools/list`），之后缓存复用，进程随 `AiClient` 生命周期结束而终止。
    /// 发现失败的 server 会被静默跳过。
    fn base_tools(&self) -> Vec<Box<dyn Tool>> {
        let mut tools: Vec<Box<dyn Tool>> = builtin_tools();
        if !self.mcp_servers.is_empty() {
            let mut cache = self.mcp_cache.borrow_mut();
            if cache.is_none() {
                let mut discovered: Vec<McpTool> = Vec::new();
                for srv in &self.mcp_servers {
                    if let Ok(ts) = discover_mcp_tools(srv) {
                        discovered.extend(ts);
                    }
                }
                *cache = Some(discovered);
            }
            if let Some(cached) = cache.as_ref() {
                for t in cached {
                    tools.push(Box::new(t.clone()));
                }
            }
        }
        tools
    }

    /// 底层工具注册表（共享 `Arc`，供 SkillTool 引用，避免引用环）。
    pub fn base_registry(&self) -> Arc<ToolRegistry> {
        Arc::new(ToolRegistry::new(self.base_tools()))
    }

    /// 构造完整工具注册表：底层工具 + 内置 Skill 包装成的 Tool。
    ///
    /// `llm` 用于让 Skill 在内部回调模型；每个 `SkillTool` 只引用 `base_registry`
    /// （底层工具），不会递归包含其它 Skill。
    pub fn tool_registry(&self, llm: &LlmConfig) -> Arc<ToolRegistry> {
        let base = self.base_registry();
        let mut all: Vec<Box<dyn Tool>> = self.base_tools();
        for skill in builtin_skills() {
            all.push(Box::new(SkillTool::new(
                skill,
                llm.clone(),
                self.memory.clone(),
                base.clone(),
            )));
        }
        Arc::new(ToolRegistry::new(all))
    }

    /// 列举全部内置技能元数据（供 UI 展示 / 选择）。
    pub fn list_skills(&self) -> Vec<SkillMeta> {
        SkillRegistry::new(builtin_skills()).metas()
    }

    /// 直接运行一个技能（非 Agent 模式），返回最终文本。
    ///
    /// 用于命令面板 / Skills 菜单等「一键运行」入口。`event_sink` 可选：若提供，
    /// 技能内部的思考增量会以 [`AgentEvent::Thought`] 流式回传。
    pub fn run_skill(
        &self,
        name: &str,
        input: &str,
        llm: &LlmConfig,
        perm: AiPermission,
        cx: &App,
        event_sink: Option<mpsc::Sender<AgentEvent>>,
    ) -> Result<String, AIError> {
        self.sync_context(cx);
        run_skill_direct(
            name,
            input,
            llm,
            perm,
            &self.focus_manager,
            &self.workspace,
            &self.terminals,
            &self.memory,
            cx,
            event_sink,
        )
    }

    /// 按名称调用一个底层工具。命令执行类工具会经过 `perm` 权限门禁。
    ///
    /// 注：此方法只暴露底层工具，不直接调用 Skill；Skill 请走 [`Self::run_skill`]
    /// 或 Agent 模式（Skill 会作为 Tool 出现在 `tool_registry` 中）。
    pub fn call_tool(
        &self,
        name: &str,
        args: Value,
        perm: AiPermission,
        cx: &App,
    ) -> Result<String, ToolError> {
        let ctx = ToolCtx {
            focus_manager: self.focus_manager.clone(),
            workspace: self.workspace.clone(),
            terminals: self.terminals.clone(),
            permission: perm,
            memory: self.memory.clone(),
        };
        self.base_registry().call(name, args, &ctx, cx)
    }

    /// 运行一轮 Agent 对话（tool-calling / ReAct 循环），返回模型最终回复。
    ///
    /// 以当前聚焦终端内容作为上下文，组装通用 system prompt，让 LLM 自行决定
    /// 调用工具或技能（读终端 / 跑命令 / 列会话 / 读写文件 / 搜日志 / 生成配置 / 诊断 / 巡检 / 部署 …）。
    /// 命令执行类工具受 `perm` 权限门禁控制。
    pub fn agent_chat(
        &self,
        base_url: &str,
        api_key: &str,
        model_id: &str,
        user_input: &str,
        perm: AiPermission,
        cx: &App,
    ) -> Result<String, AIError> {
        let bundle = self.context_bundle(cx);
        let system = render_prompt(PromptScene::General, &bundle);
        let messages = vec![
            json!({ "role": "system", "content": system }),
            json!({ "role": "user", "content": user_input }),
        ];
        let llm = LlmConfig {
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            model_id: model_id.to_string(),
        };
        let ctx = ToolCtx {
            focus_manager: self.focus_manager.clone(),
            workspace: self.workspace.clone(),
            terminals: self.terminals.clone(),
            permission: perm,
            memory: self.memory.clone(),
        };
        let registry = self.tool_registry(&llm);
        run_agent_turn(
            base_url,
            api_key,
            model_id,
            &messages,
            &registry,
            &ctx,
            cx,
            8,
            None,
        )
    }
}

/// 权限枚举的 i18n 标签。
pub fn permission_label(p: AiPermission, cx: &App) -> String {
    i18n!(
        cx,
        match p {
            AiPermission::Deny => "ai.perm_deny",
            AiPermission::ReadOnly => "ai.perm_readonly",
            AiPermission::AllowAll => "ai.perm_allowall",
        }
    )
}

/// 直接运行一个技能（自由函数版，不依赖 [`AiClient`]）。
///
/// 供 UI 层在「一键运行」入口使用：传入运行时实体、共享记忆与模型配置即可。
/// `event_sink` 可选：若提供，技能内部的思考增量会以 [`AgentEvent::Thought`] 流式回传。
pub fn run_skill_direct(
    name: &str,
    input: &str,
    llm: &LlmConfig,
    perm: AiPermission,
    focus_manager: &Entity<FocusManager>,
    workspace: &Entity<Workspace>,
    terminals: &TerminalsRegistry,
    memory: &Arc<Mutex<AiMemory>>,
    cx: &App,
    event_sink: Option<mpsc::Sender<AgentEvent>>,
) -> Result<String, AIError> {
    let skill = builtin_skills()
        .into_iter()
        .find(|s| s.name() == name)
        .ok_or_else(|| AIError::Other {
            message: format!("unknown skill: {name}"),
        })?;
    let base = Arc::new(ToolRegistry::new(builtin_tools()));
    let mut mem = memory
        .lock()
        .map_err(|_| AIError::Other {
            message: "memory lock poisoned".to_string(),
        })?;
    let tool_ctx = ToolCtx {
        focus_manager: focus_manager.clone(),
        workspace: workspace.clone(),
        terminals: terminals.clone(),
        permission: perm,
        memory: memory.clone(),
    };
    let mut skill_ctx = SkillCtx {
        registry: &base,
        tool_ctx: &tool_ctx,
        memory: &mut mem,
        llm,
        cx,
        events: event_sink.as_ref(),
    };
    Ok(skill.run(input, &mut skill_ctx).text)
}

fn generic_reply(cx: &App) -> String {
    format!(
        "{}\n- {}\n- {}\n- {}\n- {}\n- {}",
        i18n!(cx, "ai.capabilities_intro"),
        i18n!(cx, "ai.cap_read_terminal"),
        i18n!(cx, "ai.cap_exec"),
        i18n!(cx, "ai.cap_script"),
        i18n!(cx, "ai.cap_list_sessions"),
        i18n!(cx, "ai.cap_config"),
    )
}
