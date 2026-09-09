use crate::ai_runtime::AppAIRuntime;
use crate::settings::settings_entity;
use crate::views::overlays::overlay_manager::OverlayManager;
use gpui::prelude::*;
use gpui::*;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use velowork_ai::{
    AgentEvent, AiClient, AiPermission, LlmConfig, PromptScene, RequestId, RequestState, SkillMeta,
    StreamChunk, StreamEvent, ToolCtx, ToolError, ToolRegistry, permission_label, render_prompt,
    run_agent_turn_with_tool_channel, run_skill_direct, send_command_to_focused_terminal,
};
use velowork_i18n::i18n;
use velowork_terminal::TerminalsRegistry;
use velowork_ui::behavior::{HoverBehavior, StatefulElementBehaviorExt};
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::dock::{Panel, PanelAction, PanelInfo};
use velowork_ui::dock::resize::ResizeHandle;
use velowork_ui::dropdown::{
    dropdown_anchored_above, dropdown_overlay,
};
use velowork_ui::icon::AppIcon;
use velowork_ui::menu::PopupMenu;
use crate::views::overlays::menus::ai_context_menu::open_ai_context_menu;
use velowork_ui::input::{Input, InputState};
use velowork_ui::scrollable::{Scrollbar, ScrollbarAxis, ScrollbarShow};
use velowork_ui::select::{Select, SelectEvent, SelectOption, SelectPlacement, SelectState};
use velowork_ui::theme::{ThemeColors, surface_bg, theme, with_alpha};
use velowork_ui::tokens::{
    ui_space_lg, ui_space_md, ui_space_sm, ui_space_xs,
    ICON_MD, ICON_SM, ICON_STD, RADIUS_LG, RADIUS_MD, RADIUS_STD, SPACE_LG, SPACE_MD, SPACE_SM,
    SPACE_XL,
    SPACE_XS, markdown_font_family, mono_font_family, ui_font_family, ui_text_md, ui_text_sm,
    ui_text_xs, use_custom_markdown_font, use_custom_ui_font,
};
use velowork_ui::tooltip::Tooltip;
use velowork_ui::{Button, button_primary, h_flex, v_flex};
use velowork_workspace::focus::FocusManager;
use velowork_workspace::settings::AiModelConfig;
use velowork_workspace::state::Workspace;

use regex::Regex;

#[derive(Clone)]
pub struct ChatMessage {
    pub is_user: bool,
    pub text: String,
    pub streaming: bool,
    pub document_views: std::cell::RefCell<Vec<Entity<velowork_markdown::widgets::DocumentView>>>,
    /// 结构化工具调用卡片（若有），优先于 `text` 渲染为专属组件。
    pub tool_call: Option<ToolCallCardData>,
    /// Agent 模式的思考过程（工具调用前的推理内容），可折叠展示。
    pub thinking: Option<String>,
    /// 随本条消息发送的终端「AI 解读」引用内容，聊天界面中默认折叠展示。
    pub quote: Option<String>,
    /// 随本条消息发送的上下文附件（本地文件 / 图片），仅在用户消息上非空。
    pub attachments: Vec<ChatAttachment>,
}

/// 输入框添加的上下文附件：本地文件或图片。
///
/// 发送消息时，文本类附件的内容会被解析并作为上下文随用户消息一并
/// 传递给 AI 模型；图片类附件以缩略图展示，路径与名称作为上下文传递。
#[derive(Clone)]
pub struct ChatAttachment {
    /// 附件在磁盘上的绝对路径。
    pub path: std::path::PathBuf,
    /// 显示用文件名。
    pub name: String,
    /// 是否为图片（决定缩略图渲染方式）。
    pub is_image: bool,
    /// 文本类附件解析出的内容；图片类为 `None`。
    pub text_content: Option<String>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct SavedChatMessage {
    #[serde(default)]
    pub is_user: bool,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub thinking: Option<String>,
    #[serde(default)]
    pub quote: Option<String>,
    #[serde(default)]
    pub tool_call: Option<SavedToolCallCardData>,
    #[serde(default)]
    pub attachments: Vec<SavedChatAttachment>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct SavedToolCallCardData {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub params: Vec<(String, String)>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct SavedChatAttachment {
    #[serde(default)]
    pub path: std::path::PathBuf,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub is_image: bool,
    #[serde(default)]
    pub text_content: Option<String>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct SavedProjectChatSession {
    #[serde(default)]
    pub messages: Vec<SavedChatMessage>,
    #[serde(default)]
    pub ai_history: Vec<(usize, String)>,
    #[serde(default)]
    pub selected_model_id: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct SavedAiChatStore {
    #[serde(default)]
    pub project_sessions: std::collections::HashMap<String, SavedProjectChatSession>,
}

impl ChatMessage {
    pub fn to_saved(&self) -> SavedChatMessage {
        SavedChatMessage {
            is_user: self.is_user,
            text: self.text.clone(),
            thinking: self.thinking.clone(),
            quote: self.quote.clone(),
            tool_call: self.tool_call.as_ref().map(|tc| SavedToolCallCardData {
                kind: match tc.kind {
                    ToolCallKind::Use => "use".to_string(),
                    ToolCallKind::Result => "result".to_string(),
                },
                name: tc.name.clone(),
                body: tc.body.clone(),
                params: tc.params.clone(),
            }),
            attachments: self
                .attachments
                .iter()
                .map(|a| SavedChatAttachment {
                    path: a.path.clone(),
                    name: a.name.clone(),
                    is_image: a.is_image,
                    text_content: a.text_content.clone(),
                })
                .collect(),
        }
    }

    pub fn from_saved(saved: SavedChatMessage) -> Self {
        ChatMessage {
            is_user: saved.is_user,
            text: saved.text,
            streaming: false,
            document_views: std::cell::RefCell::new(Vec::new()),
            tool_call: saved.tool_call.and_then(|tc| {
                if tc.name.is_empty() && tc.body.is_empty() {
                    None
                } else {
                    Some(ToolCallCardData {
                        kind: if tc.kind == "result" {
                            ToolCallKind::Result
                        } else {
                            ToolCallKind::Use
                        },
                        name: tc.name,
                        body: tc.body,
                        params: tc.params,
                    })
                }
            }),
            thinking: saved.thinking,
            quote: saved.quote,
            attachments: saved
                .attachments
                .into_iter()
                .map(|a| ChatAttachment {
                    path: a.path,
                    name: a.name,
                    is_image: a.is_image,
                    text_content: a.text_content,
                })
                .collect(),
        }
    }
}

impl ProjectChatSession {
    pub fn to_saved(&self) -> SavedProjectChatSession {
        SavedProjectChatSession {
            messages: self.messages.iter().map(|m| m.to_saved()).collect(),
            ai_history: self.ai_history.clone(),
            selected_model_id: self.selected_model_id.clone(),
        }
    }

    pub fn from_saved(saved: SavedProjectChatSession) -> Self {
        ProjectChatSession {
            messages: saved.messages.into_iter().map(ChatMessage::from_saved).collect(),
            ai_history: saved.ai_history,
            selected_model_id: saved.selected_model_id,
        }
    }
}



fn load_single_project_session_from_db(pid: &str) -> Option<ProjectChatSession> {
    let db = velowork_core::storage::database()?;
    let repo = velowork_workspace::repositories::AiRepository::new(db);

    let convs = if pid == "default" {
        repo.list_conversations(None).ok()?
    } else {
        repo.list_conversations(Some(pid)).ok()?
    };

    let conv = convs.into_iter().find(|c| {
        let c_pid = c.project_id.as_deref().unwrap_or("default");
        c_pid == pid
    })?;

    let msgs_rows = repo.list_messages(&conv.id).ok()?;
    log::info!("[AI DB] Found conversation {} for project {}, row count: {}", conv.id, pid, msgs_rows.len());
    let mut messages = Vec::new();
    let mut ai_history = Vec::new();
    for (idx, m_row) in msgs_rows.into_iter().enumerate() {
        let is_user = m_row.role == "user";
        if is_user {
            ai_history.push((idx, m_row.content.clone()));
        }
        let mut quote = None;
        let mut thinking = None;
        let mut tool_call = None;
        let mut streaming = false;

        let mut attachments = Vec::new();
        if let Ok(att_rows) = repo.list_attachments(&m_row.id) {
            for att in att_rows {
                let p = std::path::PathBuf::from(&att.path);
                let is_img = is_image_path(&p);
                let text_content = read_text_safe(&p);
                attachments.push(ChatAttachment {
                    name: p
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    path: p,
                    is_image: is_img,
                    text_content,
                });
            }
        }

        if let Ok(meta_json) = serde_json::from_str::<serde_json::Value>(&m_row.metadata) {
            if let Some(q) = meta_json.get("quote").and_then(|v| v.as_str()) {
                quote = Some(q.to_string());
            }
            if let Some(t) = meta_json.get("thinking").and_then(|v| v.as_str()) {
                thinking = Some(t.to_string());
            }
            if let Some(s) = meta_json.get("streaming").and_then(|v| v.as_bool()) {
                streaming = s;
            }
            if let Some(tc_json) = meta_json.get("tool_call") {
                let kind_str = tc_json
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("use");
                let name = tc_json
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let body = tc_json
                    .get("body")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let mut params = Vec::new();
                if let Some(arr) = tc_json.get("params").and_then(|v| v.as_array()) {
                    for p in arr {
                        if let (Some(k), Some(v)) = (
                            p.get(0).and_then(|x| x.as_str()),
                            p.get(1).and_then(|x| x.as_str()),
                        ) {
                            params.push((k.to_string(), v.to_string()));
                        }
                    }
                }
                if name.is_empty() && body.is_empty() {
                    tool_call = None;
                } else {
                    tool_call = Some(ToolCallCardData {
                        kind: if kind_str == "result" {
                            ToolCallKind::Result
                        } else {
                            ToolCallKind::Use
                        },
                        name,
                        body,
                        params,
                    });
                }
            }
        }

        messages.push(ChatMessage {
            is_user,
            text: m_row.content,
            streaming,
            document_views: std::cell::RefCell::new(Vec::new()),
            tool_call,
            thinking,
            attachments,
            quote,
        });
    }

    if messages.is_empty() {
        return None;
    }

    Some(ProjectChatSession {
        messages,
        ai_history,
        selected_model_id: conv.model,
    })
}

fn chat_messages_to_simple(messages: &[ChatMessage]) -> Vec<velowork_ai::SimpleChatMessage> {
    messages
        .iter()
        .filter(|m| !m.streaming)
        .map(|m| {
            let mut text = m.text.clone();
            if !m.attachments.is_empty() {
                text.push_str(&attachment_context(&m.attachments));
            }
            let mut simple = velowork_ai::SimpleChatMessage::new(m.is_user, text);
            if let Some(t) = &m.thinking {
                simple = simple.with_thinking(t);
            }
            if let Some(q) = &m.quote {
                simple = simple.with_quote(q);
            }
            simple
        })
        .collect()
}

/// 待发送消息：AI 生成进行中用户输入的新消息先暂存到此队列，
/// 待当前生成终止 / 完成后按序自动发送。
#[derive(Clone)]
struct PendingSend {
    /// 用户输入的提示词文本（已 trim）。
    text: String,
    /// 随消息携带的终端「AI 解读」引用内容。
    quote: Option<String>,
    /// 随消息携带的上下文附件（本地文件 / 图片）。
    attachments: Vec<ChatAttachment>,
}

impl std::fmt::Debug for ChatMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatMessage")
            .field("is_user", &self.is_user)
            .field("text", &self.text)
            .field("streaming", &self.streaming)
            .field("thinking_len", &self.thinking.as_ref().map(|t| t.len()))
            .finish()
    }
}

/// 判断路径是否为常见图片格式（用于决定缩略图渲染方式）。
fn is_image_path(p: &std::path::Path) -> bool {
    const IMAGE_EXTS: &[&str] = &[
        "png", "jpg", "jpeg", "gif", "bmp", "webp", "svg", "ico", "tiff", "avif",
    ];
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| IMAGE_EXTS.contains(&e.as_str()))
}

/// 安全读取文本类附件内容：仅读取体量适中（< 200KB）且可成功解码为 UTF-8 的文件，
/// 其余（二进制 / 超大文件）返回 `None`，避免把无意义数据塞进模型上下文。
fn read_text_safe(p: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(p).ok()?;
    if content.is_empty() || content.len() > 200_000 {
        return None;
    }
    Some(content)
}

/// 统计一条消息正文中「纯文本片段」的总行数（工具调用卡片不计入），至少返回 1。
/// 用于驱动 AI 回复的逐行显示动画：每行文本完整出现后再展示下一行。
fn ai_text_line_count(text: &str) -> usize {
    split_tool_calls(text)
        .iter()
        .map(|s| match s {
            ToolSegment::Text(t) => t.split('\n').count(),
            ToolSegment::Tool { .. } => 0,
        })
        .sum::<usize>()
        .max(1)
}

/// 判断一条 AI 消息是否应跳过「逐行浮现」动画、直接完整渲染。
/// 包含结构化工具调用、原始 XML 工具标签或 Markdown 代码块的消息视为「重内容」，
/// 一次性完整显示，避免逐行动画打断代码 / 工具结果的阅读节奏。
fn ai_message_skips_reveal(msg: &ChatMessage) -> bool {
    if msg.tool_call.is_some() {
        return true;
    }
    for seg in split_tool_calls(&msg.text) {
        if matches!(seg, ToolSegment::Tool { .. }) {
            return true;
        }
    }
    // Markdown 围栏代码块 ```
    msg.text.contains("```")
}

/// 将附件拼装为供模型消费的上下文段落。文本附件内联其内容，图片附件给出
/// 路径与名称（当前文本模型无法接收图像像素，路径可作为检索线索）。
fn attachment_context(attachments: &[ChatAttachment]) -> String {
    if attachments.is_empty() {
        return String::new();
    }
    let mut s = String::from("\n\n[附件上下文 / Attachments]\n");
    for a in attachments {
        let kind = if a.is_image {
            "图片 Image"
        } else {
            "文本 Text"
        };
        s.push_str(&format!("--- 文件: {} ({})\n", a.name, kind));
        match &a.text_content {
            Some(c) => s.push_str(c),
            None => s.push_str(&format!("(图片附件，磁盘路径: {})\n", a.path.display())),
        }
        s.push('\n');
    }
    s
}

/// 将文本截断为最多两行展示（约 45 个汉字字符），超出部分以 "…" 标示。
/// 用于历史消息列表的紧凑预览。
fn truncate_two_lines(text: &str) -> String {
    let stripped: String = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let chars: Vec<char> = stripped.chars().collect();
    let max = 45;
    if chars.len() <= max {
        stripped
    } else {
        let mut cut = chars[..max].iter().collect::<String>();
        cut.push('…');
        cut
    }
}

/// 工具调用卡片的种类：模型发起调用 vs 工具返回结果。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolCallKind {
    Use,
    Result,
}

/// 一条结构化的工具调用 / 工具结果，渲染为气泡内的专属卡片。
///
/// 来源有两种：
/// 1. Agent 事件流（`AgentEvent::ToolUse` / `AgentEvent::ToolResult`）直接填充；
/// 2. 模型把工具调用以原始 XML（`<function=NAME>...</function>`）写进回复正文，
///    由 `split_tool_calls` 在渲染时解析提取。
#[derive(Clone)]
pub struct ToolCallCardData {
    pub kind: ToolCallKind,
    /// 函数 / 工具名（仅 `Use` 有意义）。
    pub name: String,
    /// 原始内容（参数 JSON / 参数 XML / 结果文本）。
    pub body: String,
    /// 从 `body` 解析出的结构化参数 `(key, value)`。
    pub params: Vec<(String, String)>,
}

/// `split_tool_calls` 的片段：纯文本 或 一段工具调用。
enum ToolSegment {
    Text(String),
    Tool {
        kind: ToolCallKind,
        name: String,
        body: String,
    },
}

pub struct AiAssistantPanel {
    workspace: Entity<Workspace>,
    focus_manager: Entity<FocusManager>,
    terminals: TerminalsRegistry,
    overlay_manager: Entity<OverlayManager>,
    focus_handle: FocusHandle,
    ai_client: AiClient,

    chat_input: Option<Entity<InputState>>,
    messages: Vec<ChatMessage>,

    ai_permission: AiPermission,
    ai_search_open: bool,
    ai_search_input: Option<Entity<InputState>>,
    ai_search_case_sensitive: bool,
    ai_search_use_regex: bool,
    ai_search_flat_index: Option<usize>,
    ai_selected_model_id: Option<String>,
    ai_model_select: Entity<SelectState<String>>,
    ai_perm_select: Entity<SelectState<AiPermission>>,
    ai_slash_menu_open: bool,
    ai_slash_selected_index: usize,
    ai_slash_scroll_handle: ScrollHandle,
    ai_stream_rx: Option<std::sync::mpsc::Receiver<StreamChunk>>,
    _ai_stream_task: Option<Task<()>>,
    ai_streaming_index: Option<usize>,
    ai_agent_mode: bool,
    /// Agent 模式：是否已收到第一个工具调用（用于思考过程分离）。
    ai_seen_tool_use: bool,
    ai_agent_rx: Option<mpsc::Receiver<AgentEvent>>,
    ai_tool_rx: Option<
        mpsc::Receiver<(
            String,
            serde_json::Value,
            mpsc::Sender<Result<String, ToolError>>,
        )>,
    >,
    ai_agent_registry: Option<Arc<ToolRegistry>>,
    ai_runtime: AppAIRuntime,
    /// 当前在途 AI 请求的 RequestId（仅 agent 模式注册，用于 cancel 真正中断后台 HTTP）。
    ai_current_request_id: Option<RequestId>,
    /// 待发送消息队列：生成进行中输入的消息暂存于此，当前生成结束后按序自动发送。
    ai_pending_queue: Vec<PendingSend>,
    /// Agent 后台线程存活标志：消费循环据此检测异常退出并强制收尾。
    ai_agent_alive: Option<Arc<AtomicBool>>,
    _ai_agent_task: Option<Task<()>>,
    _ai_consume_task: Option<Task<()>>,
    ai_scroll_handle: ScrollHandle,
    /// 历史记录下拉列表的滚动句柄，用于驱动右侧可见滚动条。
    ai_history_scroll: ScrollHandle,
    ai_context_menu: Option<Entity<PopupMenu>>,

    ai_editing_index: Option<usize>,
    ai_edit_input: Option<Entity<InputState>>,

    /// 动画帧计数器（驱动加载动画与 AI 回复逐行显示）。
    animation_frame: u64,
    /// 已完成复制的消息索引集合（用于将复制按钮图标切换为 ✓）。
    ai_copy_done_indices: std::cell::RefCell<Vec<usize>>,
    /// AI 回复逐行显示动画：每条消息的起始帧与已显示行数（按消息索引对齐）。
    ai_reveal_start: std::cell::RefCell<Vec<u64>>,
    ai_reveal_revealed: std::cell::RefCell<Vec<usize>>,
    /// 自动滚动开关：用户向上滚动查看历史时置为 false 暂停跟随，回到底部时恢复。
    ai_autoscroll: std::cell::Cell<bool>,
    /// 上一帧的滚动偏移，用于区分「用户主动上滚」与「内容增长导致的偏移变化」。
    ai_prev_scroll_offset: std::cell::Cell<Point<Pixels>>,

    /// 待发送的输入框附件（本地文件 / 图片），发送后转移到对应 ChatMessage。
    attachments: Vec<ChatAttachment>,

    /// 历史消息下拉是否展开。
    ai_history_open: bool,
    /// 输入框区域的窗口绝对 bounds，用于把历史下拉锚定在其正上方且等宽。
    ai_input_area_bounds: Option<Bounds<Pixels>>,
    /// 历史记录：仅保存用户发送的消息（消息索引 + 文本），不记录 AI 回复。
    ai_history: Vec<(usize, String)>,
    /// 当前关联的项目 ID
    current_project_id: Option<String>,
    /// 按项目隔离的 AI 聊天会话状态 Map
    project_chat_sessions: std::collections::HashMap<String, ProjectChatSession>,
    /// 从终端右键「AI 解读」注入的引用内容。展示在输入框上方，可删除/编辑。
    ai_quote: Option<String>,
    /// 引用内容是否处于编辑状态（编辑态显示独立输入框）。
    ai_quote_editing: bool,
    /// 引用内容的编辑输入框。
    ai_quote_input: Option<Entity<InputState>>,
    /// 聊天界面中已展开引用块的用户消息索引集合（默认全部折叠）。
    ai_expanded_quotes: std::cell::RefCell<Vec<usize>>,
    /// 聊天界面中已展开「工具调用组」的 AI 消息索引集合（默认全部折叠）。
    ai_expanded_tools: std::cell::RefCell<Vec<usize>>,
    /// 鼠标是否悬停在聊天滚动区域（内容 + 滚动条整体），用于控制滚动条显隐。
    ai_scrollbar_hovered: std::cell::Cell<bool>,

    /// 输入区域（输入框 + 工具栏）的可拖拽高度（像素）。
    ai_input_area_height: f32,
    /// 拖拽调整输入区域高度的状态。
    ai_input_resize_dragging: Option<AiInputResizeDrag>,
}

#[derive(Clone)]
pub struct ProjectChatSession {
    pub messages: Vec<ChatMessage>,
    pub ai_history: Vec<(usize, String)>,
    pub selected_model_id: Option<String>,
}

/// agent 模式单轮对话允许的最大工具调用轮数。
///
/// 多步任务（读文件 → 搜索 → 改代码 → 验证）很容易超过较小的上限，
/// 此处取一个能覆盖绝大多数真实任务的较大值，避免“max rounds exceeded”
/// 这类因上限过低导致的假性失败。循环防护由 [`velowork_ai`] 在内部负责。
const AI_AGENT_MAX_ROUNDS: usize = 24;

const MAX_MEMORY_MESSAGES: usize = 200;

/// 输入区域默认高度（像素）。
const AI_INPUT_AREA_DEFAULT_HEIGHT: f32 = 160.0;
/// 输入区域最小高度（像素）。
const AI_INPUT_AREA_MIN_HEIGHT: f32 = 100.0;
/// 输入区域最大高度（像素）。
const AI_INPUT_AREA_MAX_HEIGHT: f32 = 500.0;

/// 拖拽调整输入区域高度的状态。
#[derive(Clone, Copy, Debug)]
struct AiInputResizeDrag {
    start_y: f32,
    start_height: f32,
}

impl AiAssistantPanel {
    fn push_message(&mut self, msg: ChatMessage) {
        self.messages.push(msg);
        if self.messages.len() > MAX_MEMORY_MESSAGES {
            let overflow = self.messages.len() - MAX_MEMORY_MESSAGES;
            self.messages.drain(0..overflow);
        }
    }
    pub fn new(
        workspace: Entity<Workspace>,
        focus_manager: Entity<FocusManager>,
        terminals: TerminalsRegistry,
        overlay_manager: Entity<OverlayManager>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut project_chat_sessions = std::collections::HashMap::new();

        if let Some(db) = velowork_core::storage::database() {
            let repo = velowork_workspace::repositories::AiRepository::new(db);
            if let Ok(convs) = repo.list_conversations(None) {
                for conv in convs {
                    let pid = conv.project_id.unwrap_or_else(|| "default".into());
                    if !project_chat_sessions.contains_key(&pid) {
                        if let Some(sess) = load_single_project_session_from_db(&pid) {
                            project_chat_sessions.insert(pid, sess);
                        }
                    }
                }
            }
        }

        let initial_pid = focus_manager.read(cx).active_project_id().cloned();
        let key = initial_pid.as_deref().unwrap_or("default").to_string();

        let (messages, ai_history, restored_model_id) =
            if let Some(sess) = project_chat_sessions.remove(&key) {
                (sess.messages, sess.ai_history, sess.selected_model_id)
            } else if let Some(sess) = load_single_project_session_from_db(&key) {
                (sess.messages, sess.ai_history, sess.selected_model_id)
            } else {
                (
                    vec![ChatMessage {
                        is_user: false,
                        text: i18n!(cx, "ai_assistant.welcome"),
                        streaming: false,
                        document_views: std::cell::RefCell::new(Vec::new()),
                        tool_call: None,
                        thinking: None,
                        attachments: Vec::new(),
                        quote: None,
                    }],
                    Vec::new(),
                    None,
                )
            };

        let ai_client = AiClient::new(focus_manager.clone(), workspace.clone(), terminals.clone());

        let reg = overlay_manager.read(cx).overlay_registry();

        let ai_model_select = cx.new(|cx| {
            let mut s = SelectState::new(cx)
                .placeholder(i18n!(cx, "ai_assistant.model"))
                .placement(SelectPlacement::Above);
            s.set_overlay_registry(reg.clone());
            s
        });

        let ai_perm_select = cx.new(|cx| {
            let mut s = SelectState::new(cx)
                .options(
                    AiPermission::all()
                        .iter()
                        .map(|&p| SelectOption::new(p, permission_label(p, cx)))
                        .collect(),
                )
                .selected(Some(AiPermission::ReadOnly))
                .placement(SelectPlacement::Above);
            s.set_overlay_registry(reg.clone());
            s
        });

        cx.subscribe(
            &ai_model_select,
            |this, _, event: &SelectEvent<String>, cx| {
                let SelectEvent::Change(opt_id) = event;
                this.ai_selected_model_id = opt_id.clone();
                cx.notify();
            },
        )
        .detach();

        cx.subscribe(
            &ai_perm_select,
            |this, _, event: &SelectEvent<AiPermission>, cx| {
                if let SelectEvent::Change(Some(perm)) = event {
                    this.ai_permission = *perm;
                    cx.notify();
                }
            },
        )
        .detach();

        let chat_input = cx.new(|cx| {
            InputState::new(cx)
                .multiline()
                .wrap(true)
                .fill_height(true)
                .placeholder(i18n!(cx, "ai_assistant.title"))
        });
        let chat_input_clone = chat_input.clone();
        cx.subscribe(
            &chat_input_clone,
            |this: &mut Self, _, _: &velowork_ui::input::InputEvent, cx| {
                let val = this
                    .chat_input
                    .as_ref()
                    .map(|i| i.read(cx).text().to_string())
                    .unwrap_or_default();
                if val.starts_with('/') && !val.contains(' ') {
                    this.ai_slash_menu_open = true;
                    let count = this.filtered_slash_skills(cx).len();
                    if count > 0 {
                        this.ai_slash_selected_index =
                            this.ai_slash_selected_index.min(count - 1);
                    } else {
                        this.ai_slash_selected_index = 0;
                    }
                    this.ai_slash_scroll_handle
                        .scroll_to_item(this.ai_slash_selected_index);
                } else {
                    this.ai_slash_menu_open = false;
                }
                cx.notify();
            },
        )
        .detach();

        let panel = Self {
            workspace,
            focus_manager: focus_manager.clone(),
            terminals,
            overlay_manager,
            focus_handle: cx.focus_handle(),
            ai_client,
            chat_input: Some(chat_input),
            messages,
            ai_permission: AiPermission::ReadOnly,
            ai_search_open: false,
            ai_search_input: None,
            ai_search_case_sensitive: false,
            ai_search_use_regex: false,
            ai_search_flat_index: None,
            ai_selected_model_id: restored_model_id,
            ai_model_select,
            ai_perm_select,
            ai_slash_menu_open: false,
            ai_slash_selected_index: 0,
            ai_slash_scroll_handle: ScrollHandle::new(),
            ai_stream_rx: None,
            _ai_stream_task: None,
            ai_streaming_index: None,
            ai_agent_mode: true,
            ai_seen_tool_use: false,
            ai_agent_rx: None,
            ai_tool_rx: None,
            ai_agent_registry: None,
            ai_agent_alive: None,
            ai_runtime: AppAIRuntime::new(),
            ai_current_request_id: None,
            ai_pending_queue: Vec::new(),
            _ai_agent_task: None,
            _ai_consume_task: None,
            ai_scroll_handle: ScrollHandle::new(),
            ai_history_scroll: ScrollHandle::new(),
            ai_context_menu: None,
            ai_editing_index: None,
            ai_edit_input: None,
            animation_frame: 0,
            ai_copy_done_indices: std::cell::RefCell::new(Vec::new()),
            ai_reveal_start: std::cell::RefCell::new(Vec::new()),
            ai_reveal_revealed: std::cell::RefCell::new(Vec::new()),
            ai_autoscroll: std::cell::Cell::new(true),
            ai_prev_scroll_offset: std::cell::Cell::new(Point::default()),
            attachments: Vec::new(),

            ai_history_open: false,
            ai_input_area_bounds: None,
            ai_history,
            current_project_id: initial_pid,
            project_chat_sessions,
            ai_quote: None,
            ai_quote_editing: false,
            ai_quote_input: None,
            ai_expanded_quotes: std::cell::RefCell::new(Vec::new()),
            ai_expanded_tools: std::cell::RefCell::new(Vec::new()),
            ai_scrollbar_hovered: std::cell::Cell::new(false),
            ai_input_area_height: AI_INPUT_AREA_DEFAULT_HEIGHT,
            ai_input_resize_dragging: None,
        };

        // 监听 Workspace 实体而非 FocusManager：所有项目切换路径（如
        // `Workspace::set_focused_project`）都会调用 `cx.notify()` 通知
        // Workspace，而 FocusManager 的 setter 仅修改字段、不会触发实体
        // 通知，因此观察 FocusManager 无法感知项目切换。改观察 Workspace
        // 后，切换项目时即可正确触发聊天记录按项目切换。
        cx.observe(&panel.workspace, |this, _, cx| {
            this.check_and_switch_project(cx);
        })
        .detach();

        cx.observe(&panel.focus_manager, |this, _, cx| {
            this.check_and_switch_project(cx);
        })
        .detach();

        // 监听云端恢复/同步拉取完成通知以刷新 AI 历史会话
        if let Some(sync_store) = crate::sync_engine::sync_status_store(cx) {
            cx.observe(&sync_store, |this: &mut Self, _, cx| {
                this.reload_from_db(cx);
            })
            .detach();
        }
        // 启动动画帧循环（~20fps），在「AI 流式回复中」「仍有逐行显示未完成」
        // 或「自动滚动开启但尚未到底部」时驱动重渲染。
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            loop {
                smol::Timer::after(std::time::Duration::from_millis(50)).await;
                let _ = this.update(cx, |this, cx| {
                    this.animation_frame = this.animation_frame.wrapping_add(1);
                    // 自动滚动开启但未到底部时，仍需持续重渲染以平滑跟随最新内容
                    // （例如一次性完整到达的含代码块 / 工具调用消息）。
                    let follow = if this.ai_autoscroll.get() {
                        let max = this.ai_scroll_handle.max_offset();
                        let cur = this.ai_scroll_handle.offset();
                        (cur.y + max.y).abs() > px(4.0)
                    } else {
                        false
                    };
                    // 仅在需要动画 / 跟随时通知重渲染；全部完成后静默，节省 CPU。
                    if this.animation_active() || follow {
                        cx.notify();
                    }
                });
            }
        })
        .detach();

        panel.update_all_message_input_states(cx);
        panel
    }

    /// Set window-level OverlayRegistry on inner selects for click-outside dismissal.
    pub fn set_overlay_registry(
        &mut self,
        registry: Entity<velowork_ui::overlay_registry::OverlayRegistry>,
        cx: &mut Context<Self>,
    ) {
        self.ai_model_select
            .update(cx, |s, _| s.set_overlay_registry(registry.clone()));
        self.ai_perm_select
            .update(cx, |s, _| s.set_overlay_registry(registry.clone()));
    }

    pub fn update_all_message_input_states(&self, cx: &mut Context<Self>) {
        for i in 0..self.messages.len() {
            self.update_message_input_states(i, cx);
        }
    }

    pub fn check_and_switch_project(&mut self, cx: &mut Context<Self>) {
        let active_pid = self.focus_manager.read(cx).active_project_id().cloned();
        if self.current_project_id != active_pid {
            let old_key = self
                .current_project_id
                .as_deref()
                .unwrap_or("default")
                .to_string();
            let session = ProjectChatSession {
                messages: std::mem::take(&mut self.messages),
                ai_history: std::mem::take(&mut self.ai_history),
                selected_model_id: self.ai_selected_model_id.clone(),
            };
            self.project_chat_sessions.insert(old_key, session);

            let new_key = active_pid.as_deref().unwrap_or("default").to_string();
            let new_session = if let Some(sess) = self.project_chat_sessions.remove(&new_key) {
                Some(sess)
            } else {
                load_single_project_session_from_db(&new_key)
            };

            if let Some(sess) = new_session {
                self.messages = sess.messages;
                self.ai_history = sess.ai_history;
                self.ai_selected_model_id = sess.selected_model_id;
            } else {
                self.messages = vec![ChatMessage {
                    is_user: false,
                    text: i18n!(cx, "ai_assistant.welcome"),
                    streaming: false,
                    document_views: std::cell::RefCell::new(Vec::new()),
                    tool_call: None,
                    thinking: None,
                    attachments: Vec::new(),
                    quote: None,
                }];
                self.ai_history = Vec::new();
            }
            self.current_project_id = active_pid;
            self.ai_history_open = false;
            self.update_all_message_input_states(cx);
            self.save_current_sessions_to_disk();
            cx.notify();
        }
    }

    /// 云端数据恢复/同步后，从 SQLite 数据库重新加载所有 AI 对话会话
    pub fn reload_from_db(&mut self, cx: &mut Context<Self>) {
        self.project_chat_sessions.clear();
        if let Some(db) = velowork_core::storage::database() {
            let repo = velowork_workspace::repositories::AiRepository::new(db);
            if let Ok(convs) = repo.list_conversations(None) {
                for conv in convs {
                    let pid = conv.project_id.unwrap_or_else(|| "default".into());
                    if !self.project_chat_sessions.contains_key(&pid) {
                        if let Some(sess) = load_single_project_session_from_db(&pid) {
                            self.project_chat_sessions.insert(pid, sess);
                        }
                    }
                }
            }
        }

        let key = self
            .current_project_id
            .as_deref()
            .unwrap_or("default")
            .to_string();

        let new_session = if let Some(sess) = self.project_chat_sessions.remove(&key) {
            Some(sess)
        } else {
            load_single_project_session_from_db(&key)
        };

        if let Some(sess) = new_session {
            self.messages = sess.messages;
            self.ai_history = sess.ai_history;
            if let Some(model_id) = sess.selected_model_id {
                self.ai_selected_model_id = Some(model_id);
            }
        } else {
            self.messages = vec![ChatMessage {
                is_user: false,
                text: i18n!(cx, "ai_assistant.welcome"),
                streaming: false,
                document_views: std::cell::RefCell::new(Vec::new()),
                tool_call: None,
                thinking: None,
                attachments: Vec::new(),
                quote: None,
            }];
            self.ai_history = Vec::new();
        }
        self.ai_history_open = false;
        self.update_all_message_input_states(cx);
        cx.notify();
    }

    pub fn save_current_sessions_to_disk(&self) {
        let active_pid = self
            .current_project_id
            .as_deref()
            .unwrap_or("default")
            .to_string();

        let current_session = ProjectChatSession {
            messages: self.messages.clone(),
            ai_history: self.ai_history.clone(),
            selected_model_id: self.ai_selected_model_id.clone(),
        };

        let is_welcome = |sess: &ProjectChatSession| {
            sess.messages.len() <= 1 && !sess.messages.iter().any(|m| m.is_user)
        };

        let mut all_sessions = self.project_chat_sessions.clone();
        all_sessions.insert(active_pid.clone(), current_session);

        // Sync into SQLite relational database
        if let Some(db) = velowork_core::storage::database() {
            let repo = velowork_workspace::repositories::AiRepository::new(db);
            let now_iso = chrono::Utc::now().to_rfc3339();

            for (pid, sess) in all_sessions {
                if is_welcome(&sess) {
                    if let Ok(msgs) = repo.list_messages(&format!("conv_{}", pid)) {
                        if msgs.iter().any(|m| m.role == "user") {
                            log::info!("[AI Assistant] Skipping SQLite overwrite for project {} as DB has real messages", pid);
                            continue;
                        }
                    }
                }

                let conv_id = format!("conv_{}", pid);
                let title = sess.messages.iter().find(|m| m.is_user).map(|m| m.text.clone());
                let existing = repo.get_conversation(&conv_id).ok().flatten();
                let created_at = existing.map(|c| c.created_at).unwrap_or_else(|| now_iso.clone());
                let conv_row = velowork_workspace::repositories::AiConversationRow {
                    id: conv_id.clone(),
                    profile_id: Some("default".into()),
                    project_id: Some(pid.clone()),
                    title,
                    provider_id: None,
                    model: sess.selected_model_id.clone(),
                    status: "active".into(),
                    context_mode: "session".into(),
                    created_at,
                    updated_at: now_iso.clone(),
                    revision: 1,
                    device_id: String::new(),
                };
                let _ = repo.save_conversation(&conv_row);

                let _ = repo.delete_messages(&conv_id);
                for (idx, msg) in sess.messages.iter().enumerate() {
                    let msg_id = format!("{}_{}", conv_id, idx);
                    let meta = serde_json::json!({
                        "streaming": msg.streaming,
                        "thinking": msg.thinking,
                        "quote": msg.quote,
                        "tool_call": msg.tool_call.as_ref().map(|tc| serde_json::json!({
                            "kind": if tc.kind == ToolCallKind::Use { "use" } else { "result" },
                            "name": tc.name,
                            "body": tc.body,
                            "params": tc.params,
                        })),
                    });
                    let msg_row = velowork_workspace::repositories::AiMessageRow {
                        id: msg_id.clone(),
                        conversation_id: conv_id.clone(),
                        role: if msg.is_user { "user".into() } else { "assistant".into() },
                        content: msg.text.clone(),
                        token_count: None,
                        metadata: meta.to_string(),
                        created_at: now_iso.clone(),
                        revision: 1,
                        device_id: String::new(),
                    };
                    let _ = repo.save_message(&msg_row);

                    for (att_idx, att) in msg.attachments.iter().enumerate() {
                        let att_id = format!("{}_att_{}", msg_id, att_idx);
                        let sz = std::fs::metadata(&att.path).map(|m| m.len()).unwrap_or(0);
                        let att_row = velowork_workspace::repositories::AiAttachmentRow {
                            id: att_id,
                            message_id: msg_id.clone(),
                            path: att.path.to_string_lossy().to_string(),
                            size: sz,
                            hash: None,
                            created_at: now_iso.clone(),
                        };
                        let _ = repo.save_attachment(&att_row);
                    }
                }

                // Save Context Snapshot for session
                let snap_id = format!("snap_{}", conv_id);
                let snap_row = velowork_workspace::repositories::AiContextRow {
                    id: snap_id,
                    conversation_id: conv_id,
                    type_: "session".into(),
                    target_id: Some(pid),
                    snapshot: serde_json::json!({
                        "timestamp": now_iso,
                    }).to_string(),
                    created_at: now_iso.clone(),
                };
                let _ = repo.save_context_snapshot(&snap_row);
            }
        }
    }

    pub fn current_session_tokens(&self) -> usize {
        let simple = chat_messages_to_simple(&self.messages);
        velowork_ai::estimate_messages_tokens(&simple)
    }

    fn scroll_to_bottom(&self) {
        self.ai_scroll_handle.scroll_to_bottom();
    }

    fn scroll_to_message(&self, message_index: usize, _cx: &mut Context<Self>) {
        self.ai_scroll_handle.scroll_to_item(message_index);
    }

    /// 是否仍有需要动画驱动的状态：AI 正在流式回复，或某条 AI 消息尚未完成逐行显示。
    /// 用于决定动画帧循环是否继续通知重渲染，避免无谓的 CPU 占用。
    fn animation_active(&self) -> bool {
        if self.ai_streaming_index.is_some() {
            return true;
        }
        if self.messages.iter().any(|m| m.streaming) {
            return true;
        }
        let revealed = self.ai_reveal_revealed.borrow();
        for (i, m) in self.messages.iter().enumerate() {
            // 仅对最近的 3 条消息进行动画追踪，历史更久的消息直接过滤
            if i + 3 < self.messages.len() {
                continue;
            }
            if m.is_user || !m.streaming {
                continue;
            }
            let total = ai_text_line_count(&m.text);
            let rev = revealed.get(i).copied().unwrap_or(total);
            if rev < total {
                return true;
            }
        }
        false
    }

    /// 计算某条 AI 消息当前应显示的文本行数（逐行显示动画）。
    /// 含代码块或工具调用的消息直接返回总行数（完整渲染，跳过逐行动画）；
    /// 其余消息按「起始帧 + 帧间隔」推进，每行约 200ms（4 帧）完整出现。
    fn ai_revealed_lines(&self, mi: usize, msg: &ChatMessage, frame: u64) -> usize {
        let total = ai_text_line_count(&msg.text);
        if !msg.streaming || ai_message_skips_reveal(msg) {
            return total;
        }
        // 起始帧可能尚未与消息索引对齐（流式推送后立即调用、早于渲染循环的对齐），用当前帧兜底。
        let start = self
            .ai_reveal_start
            .borrow()
            .get(mi)
            .copied()
            .unwrap_or(frame);
        let elapsed = frame.saturating_sub(start);
        let rate: u64 = 4;
        (1 + elapsed / rate).min(total as u64) as usize
    }

    fn update_message_input_states(&self, msg_idx: usize, cx: &mut Context<Self>) {
        if msg_idx >= self.messages.len() {
            return;
        }
        let msg = &self.messages[msg_idx];

        if !msg.is_user {
            // AI message: split into ToolSegments
            let segments = split_tool_calls(&msg.text);
            // 仅统计非空文本片段，作为 markdown 视图数量。
            let text_segment_count = segments
                .iter()
                .filter(|s| matches!(s, ToolSegment::Text(t) if !t.trim().is_empty()))
                .count();
            // 当前已揭示行数（含代码块 / 工具调用的消息直接完整渲染）。
            let revealed = self.ai_revealed_lines(msg_idx, msg, self.animation_frame);

            let mut views = msg.document_views.borrow_mut();
            while views.len() < text_segment_count {
                let view = cx.new(|cx| velowork_markdown::widgets::DocumentView::new("", cx));

                cx.subscribe(&view, move |this, _, event, cx| {
                    match event {
                        velowork_markdown::widgets::document_view::DocumentViewEvent::SelectionStarted => {
                            this.clear_selections_except(msg_idx, cx);
                        }
                    }
                }).detach();

                views.push(view);
            }
            views.truncate(text_segment_count);

            // 逐行显示：每个文本片段只展示已揭示行，避免一次性铺满整段文本。
            let mut text_seg_idx = 0usize;
            let mut text_line_offset = 0usize;
            for seg in &segments {
                if let ToolSegment::Text(s) = seg {
                    let seg_lines = s.split('\n').count();
                    if !s.trim().is_empty() {
                        let reveal_in_seg =
                            revealed.saturating_sub(text_line_offset).min(seg_lines);
                        let revealed_text: String = s
                            .split('\n')
                            .take(reveal_in_seg)
                            .collect::<Vec<_>>()
                            .join("\n");
                        views[text_seg_idx].update(cx, |v, cx| {
                            v.set_content(&revealed_text, cx);
                        });
                        text_seg_idx += 1;
                    }
                    text_line_offset += seg_lines;
                }
            }
        }
    }

    fn clear_selections_except(&self, msg_idx: usize, cx: &mut App) {
        for (i, msg) in self.messages.iter().enumerate() {
            if i != msg_idx {
                let views = msg.document_views.borrow();
                for v in views.iter() {
                    v.update(cx, |view, cx| {
                        view.clear_selection(cx);
                    });
                }
            }
        }
    }

    fn get_active_selection_text(&self, cx: &App) -> Option<String> {
        for msg in &self.messages {
            let views = msg.document_views.borrow();
            for v in views.iter() {
                let v_ref = v.read(cx);
                if let Some(txt) = v_ref.get_selected_markdown() {
                    if !txt.is_empty() {
                        return Some(txt);
                    }
                }
            }
        }
        None
    }

    /// 打开系统文件选择框，将选中的本地文件 / 图片作为上下文附件加入输入框。
    fn attach_files(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(i18n!(cx, "ai_assistant.attach").into()),
        });
        let _this = cx.entity().clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    for p in paths {
                        let name = p
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "附件".to_string());
                        let is_image = is_image_path(&p);
                        let text_content = if is_image { None } else { read_text_safe(&p) };
                        this.attachments.push(ChatAttachment {
                            path: p,
                            name,
                            is_image,
                            text_content,
                        });
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 移除输入框中指定索引的附件。
    fn remove_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.attachments.len() {
            self.attachments.remove(index);
            cx.notify();
        }
    }

    /// 切换历史消息下拉的展开 / 收起。
    fn toggle_history(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.ai_history_open = !self.ai_history_open;
        cx.notify();
    }

    /// 关闭历史消息下拉。
    fn close_history(&mut self, cx: &mut Context<Self>) {
        if self.ai_history_open {
            self.ai_history_open = false;
            cx.notify();
        }
    }

    /// 点击历史列表中的某条消息：平滑滚动定位到聊天主区域该消息的原始位置。
    fn scroll_to_history_message(&mut self, index: usize, cx: &mut Context<Self>) {
        self.ai_history_open = false;
        self.ai_scroll_handle.scroll_to_item(index);
        cx.notify();
    }

    /// 从终端右键「AI 解读选中内容」注入引用文字：展示在输入框上方，用户可补充提示词后一并发送。
    /// 若已有引用，则直接覆盖为最新注入的内容。
    pub fn set_quote(&mut self, text: String, cx: &mut Context<Self>) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.ai_quote = Some(text);
        self.ai_quote_editing = false;
        self.ai_quote_input = None;
        cx.notify();
    }

    fn send_ai_message(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let input_text = self
            .chat_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string().trim().to_string())
            .unwrap_or_default();
        if (input_text.is_empty() && self.ai_quote.is_none()) || self.ai_streaming_index.is_some() {
            return;
        }

        if input_text.starts_with('/') {
            let cmd_name = input_text[1..].trim();
            let enabled = self.enabled_skills(cx);
            if let Some(skill) = enabled
                .iter()
                .find(|s| s.name == cmd_name || i18n!(cx, &s.i18n_key) == cmd_name)
            {
                let name = skill.name.clone();
                if let Some(ref input) = self.chat_input {
                    input.update(cx, |input, cx| input.set_value("", cx));
                }
                self.ai_slash_menu_open = false;
                let cfg = self.current_model_config(cx);
                self.run_skill_reply(cx, name, &cfg);
                cx.notify();
                return;
            }
        }

        // 将输入框中已添加的附件随消息一并提交（发送后清空输入框附件）。
        let attachments = std::mem::take(&mut self.attachments);
        // 若注入了终端「AI 解读」引用内容，将其作为独立引用块随消息携带
        // （聊天界面默认折叠展示），不拼入用户提示词文本本身。
        let quote = self.ai_quote.take();
        let final_text = if let Some(ref q) = quote {
            if input_text.is_empty() {
                q.clone()
            } else {
                format!("{}\n\n{}", q, input_text)
            }
        } else {
            input_text.clone()
        };
        self.push_message(ChatMessage {
            is_user: true,
            text: input_text.clone(),
            streaming: false,
            document_views: std::cell::RefCell::new(Vec::new()),
            tool_call: None,
            thinking: None,
            quote,
            attachments,
        });
        let new_idx = self.messages.len() - 1;
        // 仅将用户发送的消息存入历史记录（不记录 AI 回复）。
        self.ai_history.push((new_idx, final_text.clone()));
        self.update_message_input_states(new_idx, cx);
        if let Some(ref input) = self.chat_input {
            input.update(cx, |input, cx| {
                input.set_value("", cx);
            });
        }
        self.ai_autoscroll.set(true);
        self.scroll_to_bottom();
        self.save_current_sessions_to_disk();
        self.generate_ai_reply(cx);

        cx.notify();
    }

    /// 发送按钮的统一入口：根据当前是否正在生成以及输入框是否有内容，
    /// 在「发送新消息 / 终止生成 / 加入待发送队列」三种行为间切换。
    fn on_send_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let is_streaming = self.ai_streaming_index.is_some();
        let has_input = self.has_input_content(cx);
        if is_streaming && !has_input {
            // 仅正在生成且无新输入：点击终止当前生成。
            self.stop_generation(cx);
        } else if is_streaming && has_input {
            // 正在生成且已输入文字：当前消息入队，待生成结束后自动发送。
            self.enqueue_current_input(window, cx);
        } else {
            // 空闲：直接发送。
            self.send_ai_message(window, cx);
        }
    }

    /// 输入框当前是否包含可发送的内容（文本 / 引用 / 附件）。
    fn has_input_content(&self, cx: &App) -> bool {
        let text = self
            .chat_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string().trim().to_string())
            .unwrap_or_default();
        !text.is_empty() || self.ai_quote.is_some() || !self.attachments.is_empty()
    }

    /// 立即停止当前正在进行的 AI 生成：
    /// - agent 模式通过 runtime 真正中断后台 HTTP 请求；
    /// - 普通流式 / 兜底：丢弃事件接收端并立即在 UI 层收尾当前流，
    ///   随后若有待发送队列则自动发送队首消息。
    fn stop_generation(&mut self, cx: &mut Context<Self>) {
        // agent 模式：通过 runtime 真正取消后台 HTTP。
        if let Some(id) = self.ai_current_request_id.take() {
            self.ai_runtime.cancel(id);
        }
        // 普通流式 / 兜底：丢弃事件接收端，让消费循环自然收尾（或下方手动收尾）。
        self.ai_stream_rx = None;
        self.ai_agent_rx = None;
        // 立即在 UI 层收尾当前流消息（若消费循环尚未处理）。
        if let Some(idx) = self.ai_streaming_index.take() {
            if let Some(msg) = self.messages.get_mut(idx) {
                msg.streaming = false;
                self.update_message_input_states(idx, cx);
            }
        }
        cx.notify();
        // 生成终止后，自动发送队列中的下一条消息。
        self.maybe_send_pending(cx);
    }

    /// 将当前输入框内容（文本 / 引用 / 附件）克隆一份入队，清空输入框，
    /// 待当前生成结束后按序自动发送。
    fn enqueue_current_input(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let text = self
            .chat_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string().trim().to_string())
            .unwrap_or_default();
        if text.is_empty() && self.ai_quote.is_none() && self.attachments.is_empty() {
            return;
        }
        let quote = self.ai_quote.take();
        let attachments = std::mem::take(&mut self.attachments);
        self.ai_pending_queue.push(PendingSend {
            text,
            quote,
            attachments,
        });
        if let Some(ref input) = self.chat_input {
            input.update(cx, |input, cx| {
                input.set_value("", cx);
            });
        }
        self.ai_autoscroll.set(true);
        self.scroll_to_bottom();
        cx.notify();
    }

    /// 若当前没有正在生成的回复且队列非空，自动发送队首消息。
    /// 在所有生成收尾点（普通流式 / agent 模式 / 手动终止）均会调用，
    /// 由 `ai_streaming_index` 是否为 `None` 保证不会重复发送。
    fn maybe_send_pending(&mut self, cx: &mut Context<Self>) {
        if self.ai_streaming_index.is_some() || self.ai_pending_queue.is_empty() {
            return;
        }
        let pending = self.ai_pending_queue.remove(0);
        let final_text = if let Some(ref q) = pending.quote {
            if pending.text.is_empty() {
                q.clone()
            } else {
                format!("{}\n\n{}", q, pending.text)
            }
        } else {
            pending.text.clone()
        };
        self.push_message(ChatMessage {
            is_user: true,
            text: pending.text,
            streaming: false,
            document_views: std::cell::RefCell::new(Vec::new()),
            tool_call: None,
            thinking: None,
            quote: pending.quote,
            attachments: pending.attachments,
        });
        let new_idx = self.messages.len() - 1;
        self.ai_history.push((new_idx, final_text));
        self.update_message_input_states(new_idx, cx);
        self.ai_autoscroll.set(true);
        self.scroll_to_bottom();
        self.save_current_sessions_to_disk();
        self.generate_ai_reply(cx);
        cx.notify();
    }

    /// 发送按钮当前应显示的图标：
    /// - 正在生成且输入框无内容 → `Stop`（点击终止生成）
    /// - 否则 → `Send`（空闲发送，或生成中已输入文字则入队）
    fn ai_send_button_icon(&self, cx: &App) -> AppIcon {
        let is_streaming = self.ai_streaming_index.is_some();
        let has_input = self.has_input_content(cx);
        if is_streaming && !has_input {
            AppIcon::Stop
        } else {
            AppIcon::Send
        }
    }

    /// 发送按钮的 tooltip 文案，与图标状态保持一致。
    fn ai_send_button_tooltip(&self, cx: &App) -> String {
        let is_streaming = self.ai_streaming_index.is_some();
        let has_input = self.has_input_content(cx);
        if is_streaming && !has_input {
            i18n!(cx, "ai_assistant.stop")
        } else if is_streaming && has_input {
            i18n!(cx, "ai_assistant.pending_send_now")
        } else {
            i18n!(cx, "ai_assistant.send")
        }
    }

    /// Generate an AI reply for the last user message in `self.messages`.
    /// Used both after sending a new message and after re-editing a user message.
    fn generate_ai_reply(&mut self, cx: &mut Context<Self>) {
        let user_text = self
            .messages
            .last()
            .map(|m| m.text.clone())
            .unwrap_or_default();

        let s = settings_entity(cx).read(cx).settings.clone();
        let enabled_models: Vec<_> = s.ai_models.iter().filter(|m| m.enabled).collect();

        if s.ai_enabled && !enabled_models.is_empty() {
            let model_config = self
                .ai_selected_model_id
                .as_ref()
                .and_then(|id| enabled_models.iter().find(|m| &m.id == id))
                .or_else(|| {
                    s.ai_default_model_id
                        .as_ref()
                        .and_then(|id| enabled_models.iter().find(|m| &m.id == id))
                })
                .or(enabled_models.first())
                .unwrap();

            if self.ai_agent_mode {
                self.run_agent_reply(cx, model_config);
                return;
            }

            let simple_msgs = chat_messages_to_simple(&self.messages);
            let compressed = if s.ai_auto_compress {
                velowork_ai::compress_chat_history(
                    &simple_msgs,
                    s.ai_compression_strategy,
                    s.ai_max_context_tokens,
                    s.ai_max_history_messages,
                )
            } else {
                simple_msgs
            };

            let history: Vec<(String, bool)> = compressed
                .into_iter()
                .map(|m| (m.text, m.is_user))
                .collect();

            self.push_message(ChatMessage {
                is_user: false,
                text: String::new(),
                streaming: true,
                document_views: std::cell::RefCell::new(Vec::new()),
                tool_call: None,
                thinking: None,
                attachments: Vec::new(),

                quote: None,
            });
            let streaming_idx = self.messages.len() - 1;
            self.ai_streaming_index = Some(streaming_idx);
            self.update_message_input_states(streaming_idx, cx);
            self.scroll_to_bottom();

            let rx = self.ai_client.stream_reply(
                &model_config.base_url,
                &model_config.api_key,
                &model_config.model_id,
                &history,
            );
            self.ai_stream_rx = Some(rx);

            let task = cx.spawn(async move |this: WeakEntity<Self>, cx| {
                loop {
                    smol::Timer::after(std::time::Duration::from_millis(50)).await;

                    let done = this
                        .update(cx, |this, cx| {
                            let rx = this.ai_stream_rx.take();
                            if let Some(rx) = rx {
                                let mut done = false;
                                let mut should_clear = false;
                                while let Ok(chunk) = rx.try_recv() {
                                    match chunk {
                                        StreamChunk::Delta(text) => {
                                            if let Some(idx) = this.ai_streaming_index {
                                                this.messages[idx].text.push_str(&text);
                                                this.update_message_input_states(idx, cx);
                                            }
                                        }
                                        StreamChunk::Done => {
                                            if let Some(idx) = this.ai_streaming_index {
                                                this.messages[idx].streaming = false;
                                                this.update_message_input_states(idx, cx);
                                            }
                                            this.ai_streaming_index = None;
                                            this.save_current_sessions_to_disk();
                                            this.maybe_send_pending(cx);
                                            should_clear = true;
                                            done = true;
                                        }
                                        StreamChunk::Error(e) => {
                                            if let Some(idx) = this.ai_streaming_index {
                                                this.messages[idx].text = format!(
                                                    "{}: {}",
                                                    i18n!(cx, "ai_assistant.error"),
                                                    e
                                                );
                                                this.messages[idx].streaming = false;
                                                this.update_message_input_states(idx, cx);
                                            }
                                            this.ai_streaming_index = None;
                                            this.save_current_sessions_to_disk();
                                            this.maybe_send_pending(cx);
                                            should_clear = true;
                                            done = true;
                                        }
                                        StreamChunk::ToolCalls(_) => {
                                            // 纯聊天模式不使用工具，忽略。
                                        }
                                    }
                                }
                                if !should_clear {
                                    this.ai_stream_rx = Some(rx);
                                }
                                this.scroll_to_bottom();
                                cx.notify();
                                done
                            } else {
                                true
                            }
                        })
                        .unwrap_or(true);

                    if done {
                        break;
                    }
                }
            });
            self._ai_stream_task = Some(task);
        } else {
            let perm = self.ai_permission;

            cx.spawn(async move |this: WeakEntity<Self>, cx| {
                smol::Timer::after(std::time::Duration::from_millis(600)).await;
                let _ = this.update(cx, |this, cx| {
                    let reply = this.ai_client.local_reply(&user_text, perm, cx);
                    this.push_message(ChatMessage {
                        is_user: false,
                        text: reply,
                        streaming: false,
                        document_views: std::cell::RefCell::new(Vec::new()),
                        tool_call: None,
                        thinking: None,
                        attachments: Vec::new(),

                        quote: None,
                    });
                    let reply_idx = this.messages.len() - 1;
                    this.update_message_input_states(reply_idx, cx);
                    this.scroll_to_bottom();
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// 以 Agent 模式运行一轮对话：流式消费 [`AgentEvent`]，将「思考 / 工具调用 / 工具结果」
    /// 实时渲染到聊天区。命令执行类工具受当前权限门禁控制。
    fn run_agent_reply(&mut self, cx: &mut Context<Self>, model_config: &AiModelConfig) {
        let _user_text = self
            .messages
            .last()
            .map(|m| {
                let mut t = m.text.clone();
                if !m.attachments.is_empty() {
                    t.push_str(&attachment_context(&m.attachments));
                }
                t
            })
            .unwrap_or_default();

        // 占位助手消息（流式）。
        self.push_message(ChatMessage {
            is_user: false,
            text: String::new(),
            streaming: true,
            document_views: std::cell::RefCell::new(Vec::new()),
            tool_call: None,
            thinking: None,
            attachments: Vec::new(),

            quote: None,
        });
        let streaming_idx = self.messages.len() - 1;
        self.ai_streaming_index = Some(streaming_idx);
        self.update_message_input_states(streaming_idx, cx);
        self.scroll_to_bottom();

        // 工具调用通道（Agent → 主线程）。
        let (tool_tx, tool_rx) = mpsc::channel::<(
            String,
            serde_json::Value,
            mpsc::Sender<Result<String, ToolError>>,
        )>();
        self.ai_tool_rx = Some(tool_rx);

        // StreamEvent → AgentEvent 适配通道：
        // Agent 层现在输出 StreamEvent，消费循环仍使用 AgentEvent（与 Skill 共用）。
        let (stream_tx, stream_rx) = mpsc::channel::<StreamEvent>();
        let (agent_tx, agent_rx) = mpsc::channel::<AgentEvent>();
        self.ai_agent_rx = Some(agent_rx);
        // 适配器线程：StreamEvent → AgentEvent
        std::thread::spawn(move || {
            for ev in stream_rx {
                let agent_ev = match ev {
                    StreamEvent::Token(t) => AgentEvent::Thought(t),
                    StreamEvent::ToolRequest { name, arguments } => {
                        AgentEvent::ToolUse { name, arguments }
                    }
                    StreamEvent::ToolResult(r) => AgentEvent::ToolResult(r),
                    StreamEvent::StateChange(s) => match s {
                        RequestState::Completed => AgentEvent::Done,
                        RequestState::Failed { error } => AgentEvent::Error(error.to_string()),
                        RequestState::Cancelled => AgentEvent::Error("cancelled".into()),
                        RequestState::Timeout { message } => AgentEvent::Error(message),
                        _ => continue, // Pending/Connecting etc. — 无数据事件跳过
                    },
                };
                if agent_tx.send(agent_ev).is_err() {
                    break;
                }
            }
        });

        let base_url = model_config.base_url.clone();
        let api_key = model_config.api_key.clone();
        let model_id = model_config.model_id.clone();
        // 使用 base_registry（不含 SkillTool）：避免 Skill 内部的阻塞 LLM 调用在
        // cx.update() 中执行时卡死主线程。Skill 可通过面板菜单单独触发。
        let registry = self.ai_client.base_registry();
        // 采集实时上下文（会话 / Tab / cwd / 选中文本）并组装 system prompt。
        let bundle = self.ai_client.context_bundle(cx);

        self.ai_agent_registry = Some(registry.clone());
        self.ai_seen_tool_use = false;

        let simple_msgs = chat_messages_to_simple(&self.messages);
        let s = settings_entity(cx).read(cx).settings.clone();
        let compressed = if s.ai_auto_compress {
            velowork_ai::compress_chat_history(
                &simple_msgs,
                s.ai_compression_strategy,
                s.ai_max_context_tokens,
                s.ai_max_history_messages,
            )
        } else {
            simple_msgs
        };

        // Agent 任务：在后台线程运行 ReAct 循环，避免阻塞主线程。
        // HTTP 流式读取与 agent 轮次在后台线程执行；工具调用请求通过 tool_tx
        // 回传主线程执行（因工具需要 &App 访问 GPUI 实体）。
        // 事件经 event_tx 回传，UI 由 spawn_stream_consumer 实时渲染。
        //
        // 通过 AppAIRuntime 注册任务，获得 CancelToken 和 alive 标记。
        // 消费循环据此检测异常退出并强制收尾，防止卡死。
        let (request_id, cancel_token) = self.ai_runtime.register();
        self.ai_current_request_id = Some(request_id);
        let agent_alive = Arc::new(AtomicBool::new(true));
        self.ai_agent_alive = Some(agent_alive.clone());
        let alive_for_thread = agent_alive.clone();
        let rt_cancel = cancel_token.clone();
        std::thread::spawn(move || {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let system = render_prompt(PromptScene::General, &bundle);
                let mut agent_messages = Vec::new();
                agent_messages.push(json!({ "role": "system", "content": system }));
                for m in compressed {
                    let role = if m.is_user { "user" } else { "assistant" };
                    let mut content = m.text;
                    if let Some(q) = m.quote {
                        content = format!("> {}\n\n{}", q, content);
                    }
                    agent_messages.push(json!({ "role": role, "content": content }));
                }
                // 在 HTTP 调用前检查取消
                if rt_cancel.is_cancelled() {
                    return;
                }
                run_agent_turn_with_tool_channel(
                    &base_url,
                    &api_key,
                    &model_id,
                    &agent_messages,
                    &*registry,
                    &tool_tx,
                    AI_AGENT_MAX_ROUNDS,
                    Some(stream_tx),
                );
            }));
            // 无论正常返回还是 panic，退出时都将 alive 置 false。
            alive_for_thread.store(false, Ordering::SeqCst);
        });

        // 消费循环：每 50ms 拉取事件与工具请求并渲染/执行。
        self.spawn_stream_consumer(cx);
    }

    /// 启动流式事件消费任务：从 `ai_agent_rx` 拉取 [`AgentEvent`] 并实时渲染到聊天区，
    /// 同时从 `ai_tool_rx` 拉取工具请求并在主线程执行（工具需要 &App 访问 GPUI 实体）。
    /// Agent 模式与 Skill 一键运行共用同一通道与渲染逻辑。
    fn spawn_stream_consumer(&mut self, cx: &mut Context<Self>) {
        let consume_task = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            loop {
                smol::Timer::after(std::time::Duration::from_millis(50)).await;
                let done = this
                    .update(cx, |this, cx| {
                        // 1. 处理待执行的工具请求（在主线程执行，因工具需要 &App）。
                        let tool_rx = this.ai_tool_rx.take();
                        if let Some(tool_rx) = tool_rx {
                            loop {
                                match tool_rx.try_recv() {
                                    Ok((name, args, result_tx)) => {
                                        let ctx = ToolCtx {
                                            focus_manager: this.focus_manager.clone(),
                                            workspace: this.workspace.clone(),
                                            terminals: this.terminals.clone(),
                                            permission: this.ai_permission,
                                            memory: this.ai_client.memory(),
                                        };
                                        let result = this
                                            .ai_agent_registry
                                            .as_ref()
                                            .map(|r| r.call(&name, args, &ctx, cx))
                                            .unwrap_or(Err(ToolError::Execution(
                                                "registry unavailable".into(),
                                            )));
                                        let _ = result_tx.send(result);
                                    }
                                    Err(_) => break,
                                }
                            }
                            this.ai_tool_rx = Some(tool_rx);
                        }

                        // 2. 处理 Agent 事件流。
                        let rx = this.ai_agent_rx.take();
                        if let Some(rx) = rx {
                            let mut done = false;
                            loop {
                                match rx.try_recv() {
                                    Ok(ev) => match ev {
                                        AgentEvent::Thought(t) => {
                                            if let Some(idx) = this.ai_streaming_index {
                                                this.messages[idx].text.push_str(&t);
                                                this.update_message_input_states(idx, cx);
                                            }
                                        }
                                        AgentEvent::ToolUse { name, arguments } => {
                                            // 首次 ToolUse：将已累积的思考文本移到 `thinking`。
                                            if !this.ai_seen_tool_use {
                                                this.ai_seen_tool_use = true;
                                                if let Some(idx) = this.ai_streaming_index {
                                                    let accumulated = std::mem::take(
                                                        &mut this.messages[idx].text,
                                                    );
                                                    if !accumulated.trim().is_empty() {
                                                        this.messages[idx].thinking =
                                                            Some(accumulated);
                                                    }
                                                }
                                            }
                                            let args = arguments.trim();
                                            let params = parse_tool_body(args);
                                            // 复制用文本取命令（若有），否则取原始参数。
                                            let copy_text = params
                                                .iter()
                                                .find(|(k, _)| k == "command")
                                                .map(|(_, v)| v.clone())
                                                .unwrap_or_else(|| args.to_string());
                                            this.push_message(ChatMessage {
                                                is_user: false,
                                                text: copy_text,
                                                streaming: false,
                                                document_views: std::cell::RefCell::new(Vec::new()),
                                                tool_call: Some(ToolCallCardData {
                                                    kind: ToolCallKind::Use,
                                                    name: name.clone(),
                                                    body: args.to_string(),
                                                    params,
                                                }),
                                                thinking: None,
                                                attachments: Vec::new(),

                                                quote: None,
                                            });
                                            let idx = this.messages.len() - 1;
                                            this.update_message_input_states(idx, cx);
                                        }
                                        AgentEvent::ToolResult(r) => {
                                            this.push_message(ChatMessage {
                                                is_user: false,
                                                text: r.clone(),
                                                streaming: false,
                                                document_views: std::cell::RefCell::new(Vec::new()),
                                                tool_call: Some(ToolCallCardData {
                                                    kind: ToolCallKind::Result,
                                                    name: String::new(),
                                                    body: r,
                                                    params: Vec::new(),
                                                }),
                                                thinking: None,
                                                attachments: Vec::new(),

                                                quote: None,
                                            });
                                            let idx = this.messages.len() - 1;
                                            this.update_message_input_states(idx, cx);
                                        }
                                        AgentEvent::Done => {
                                            if let Some(idx) = this.ai_streaming_index {
                                                this.messages[idx].streaming = false;
                                                this.update_message_input_states(idx, cx);
                                            }
                                            this.ai_streaming_index = None;
                                            done = true;
                                        }
                                        AgentEvent::Error(e) => {
                                            if let Some(idx) = this.ai_streaming_index {
                                                this.messages[idx].text = format!(
                                                    "{}: {}",
                                                    i18n!(cx, "ai_assistant.error"),
                                                    e
                                                );
                                                this.messages[idx].streaming = false;
                                                this.update_message_input_states(idx, cx);
                                            }
                                            this.ai_streaming_index = None;
                                            done = true;
                                        }
                                    },
                                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                        if let Some(idx) = this.ai_streaming_index {
                                            this.messages[idx].streaming = false;
                                            this.update_message_input_states(idx, cx);
                                        }
                                        this.ai_streaming_index = None;
                                        done = true;
                                        break;
                                    }
                                }
                            }
                            if !done {
                                // 检测 agent 线程是否已异常退出（API 失败 / panic /
                                // 线程死锁等）。若线程已死但未收到 Done 事件，强制收尾以免卡死。
                                let agent_dead = this
                                    .ai_agent_alive
                                    .as_ref()
                                    .map(|a| !a.load(Ordering::SeqCst))
                                    .unwrap_or(false);
                                if agent_dead {
                                    if let Some(idx) = this.ai_streaming_index {
                                        this.messages[idx].streaming = false;
                                        this.update_message_input_states(idx, cx);
                                    }
                                    this.ai_streaming_index = None;
                                    done = true;
                                }
                            }
                            if !done {
                                this.ai_agent_rx = Some(rx);
                            } else {
                                this.ai_agent_rx = None;
                                this.ai_tool_rx = None;
                                this.ai_agent_registry = None;
                                this.ai_agent_alive = None;
                                this.ai_current_request_id = None;
                                this.save_current_sessions_to_disk();
                                this.maybe_send_pending(cx);
                            }
                            this.scroll_to_bottom();
                            cx.notify();
                            done
                        } else {
                            true
                        }
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
            }
        });
        self._ai_consume_task = Some(consume_task);
    }

    /// 以 Skill 一键运行模式执行指定技能：流式消费 [`AgentEvent`]，实时渲染结果。
    fn run_skill_reply(
        &mut self,
        cx: &mut Context<Self>,
        skill_name: String,
        model_config: &AiModelConfig,
    ) {
        let user_text = self
            .messages
            .last()
            .map(|m| m.text.clone())
            .unwrap_or_default();

        // 占位助手消息（流式）。
        self.push_message(ChatMessage {
            is_user: false,
            text: String::new(),
            streaming: true,
            document_views: std::cell::RefCell::new(Vec::new()),
            tool_call: None,
            thinking: None,
            attachments: Vec::new(),

            quote: None,
        });
        let streaming_idx = self.messages.len() - 1;
        self.ai_streaming_index = Some(streaming_idx);
        self.update_message_input_states(streaming_idx, cx);
        self.scroll_to_bottom();

        let (_tx, rx) = mpsc::channel::<AgentEvent>();
        self.ai_agent_rx = Some(rx);

        let base_url = model_config.base_url.clone();
        let api_key = model_config.api_key.clone();
        let model_id = model_config.model_id.clone();
        let perm = self.ai_permission;
        let llm = LlmConfig {
            base_url: base_url.clone(),
            api_key: api_key.clone(),
            model_id: model_id.clone(),
        };
        let memory = self.ai_client.memory();
        let focus_manager = self.focus_manager.clone();
        let workspace = self.workspace.clone();
        let terminals = self.terminals.clone();

        // 先采集实时上下文（会话 / Tab / cwd / 选中文本）写入记忆，供技能读取。
        self.ai_client.sync_context(cx);

        // 技能任务：在持有 &App 的线程上同步运行，最终文本回填到占位消息。
        // 技能内部不直接回传流式事件（避免主线程阻塞期间与最终文本重复），结果一次性渲染。
        let skill_task = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = cx.update(|app| {
                run_skill_direct(
                    &skill_name,
                    &user_text,
                    &llm,
                    perm,
                    &focus_manager,
                    &workspace,
                    &terminals,
                    &memory,
                    app,
                    None,
                )
            });
            let _ = this.update(cx, |this, cx| {
                if let Some(idx) = this.ai_streaming_index {
                    match &result {
                        Ok(text) => {
                            this.messages[idx].text = text.clone();
                        }
                        Err(e) => {
                            this.messages[idx].text =
                                format!("{}: {}", i18n!(cx, "ai_assistant.error"), e);
                        }
                    }
                    this.messages[idx].streaming = false;
                    this.update_message_input_states(idx, cx);
                }
                this.ai_streaming_index = None;
                this.maybe_send_pending(cx);
                cx.notify();
            });
        });
        self._ai_agent_task = Some(skill_task);

        self.spawn_stream_consumer(cx);
    }

    /// Enter edit mode for the user message at `idx`, pre-filling the editable
    /// input with the message's current text.
    fn start_edit(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        if idx >= self.messages.len() || !self.messages[idx].is_user {
            return;
        }
        if self.ai_streaming_index.is_some() {
            return;
        }
        let text = self.messages[idx].text.clone();
        let edit_input = self.ai_edit_input.get_or_insert_with(|| {
            cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(i18n!(cx, "ai_assistant.title"))
            })
        });
        edit_input.update(cx, |input, cx| {
            input.set_value(text, cx);
            input.focus(window, cx);
        });
        self.ai_editing_index = Some(idx);
        cx.notify();
    }

    /// Commit the edited text: update the message, drop subsequent replies and
    /// regenerate a fresh AI reply.
    fn confirm_edit(&mut self, cx: &mut Context<Self>) {
        let idx = match self.ai_editing_index {
            Some(i) => i,
            None => return,
        };
        if idx >= self.messages.len() {
            self.ai_editing_index = None;
            return;
        }
        let new_text = self
            .ai_edit_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string().trim().to_string())
            .unwrap_or_default();
        if new_text.is_empty() {
            return;
        }
        self.messages[idx].text = new_text;
        self.update_message_input_states(idx, cx);
        // Remove any AI replies that followed this user message, then regenerate.
        self.messages.truncate(idx + 1);
        self.ai_editing_index = None;
        self.ai_autoscroll.set(true);
        self.scroll_to_bottom();
        self.generate_ai_reply(cx);
        cx.notify();
    }

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.ai_editing_index = None;
        cx.notify();
    }

    fn search_next(&mut self, cx: &mut Context<Self>) {
        let q = self
            .ai_search_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_default();
        if q.is_empty() {
            return;
        }

        let mut matches = Vec::new();
        for (mi, msg) in self.messages.iter().enumerate() {
            for range in find_match_ranges(
                &msg.text,
                &q,
                self.ai_search_case_sensitive,
                self.ai_search_use_regex,
            ) {
                matches.push((mi, range));
            }
        }

        if matches.is_empty() {
            self.ai_search_flat_index = None;
            cx.notify();
            return;
        }

        let mut new_idx = self.ai_search_flat_index.map(|i| i + 1).unwrap_or(0);
        if new_idx >= matches.len() {
            new_idx = 0;
        }
        self.ai_search_flat_index = Some(new_idx);

        if let Some((mi, _range)) = matches.get(new_idx) {
            self.scroll_to_message(*mi, cx);
        }

        cx.notify();
    }

    fn search_prev(&mut self, cx: &mut Context<Self>) {
        let q = self
            .ai_search_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_default();
        if q.is_empty() {
            return;
        }

        let mut matches = Vec::new();
        for (mi, msg) in self.messages.iter().enumerate() {
            for range in find_match_ranges(
                &msg.text,
                &q,
                self.ai_search_case_sensitive,
                self.ai_search_use_regex,
            ) {
                matches.push((mi, range));
            }
        }

        if matches.is_empty() {
            self.ai_search_flat_index = None;
            cx.notify();
            return;
        }

        let new_idx = self
            .ai_search_flat_index
            .map(|i| if i == 0 { matches.len() - 1 } else { i - 1 })
            .unwrap_or(matches.len().saturating_sub(1));
        self.ai_search_flat_index = Some(new_idx);

        if let Some((mi, _range)) = matches.get(new_idx) {
            self.scroll_to_message(*mi, cx);
        }

        cx.notify();
    }

    fn ai_icon_btn<F>(
        &self,
        id: &str,
        icon: AppIcon,
        tooltip: String,
        t: &ThemeColors,
        cx: &mut Context<Self>,
        on_click: F,
    ) -> impl IntoElement
    where
        F: Fn(&mut AiAssistantPanel, &mut Window, &mut Context<Self>) + 'static,
    {
        let this = cx.entity();
        let is_search = id == "ai-search" && self.ai_search_open;
        velowork_ui::icon_button::icon_button(id.to_string(), icon, t, cx)
            .when(is_search, |b| b.bg(surface_bg(t.bg_hover, cx)))
            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(tooltip.clone())).into())
            .on_click(move |_ev, window, cx| {
                this.update(cx, |this, cx| on_click(this, window, cx));
            })
            .into_any_element()
    }

    /// 输入框上方、附件预览行：以缩略图（图片）或文件图标（文本）展示已添加附件，
    /// 每项提供移除按钮。点击图片缩略图可在系统默认程序中打开该文件。
    fn render_attachment_preview(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let this = cx.entity();
        let attachments = self.attachments.clone();

        h_flex()
            .w_full()
            .flex_wrap()
            .px(px(10.0))
            .pb(ui_space_sm(cx))
            .gap(ui_space_sm(cx))
            .children(attachments.into_iter().enumerate().map(|(i, att)| {
                let this = this.clone();
                let att_path = att.path.clone();
                let is_image = att.is_image;
                let name = att.name.clone();
                let remove_tip = i18n!(cx, "ai_assistant.attachment_remove");
                div()
                    .id(ElementId::Name(format!("ai-att-{}", i).into()))
                    .relative()
                    .w(px(56.0))
                    .h(px(56.0))
                    .rounded(RADIUS_STD)
                    .border_1()
                    .border_color(rgb(t.border))
                    .bg(rgb(t.bg_hover))
                    .overflow_hidden()
                    .child(if is_image {
                        img(att_path.clone())
                            .w_full()
                            .h_full()
                            .object_fit(ObjectFit::Cover)
                            .into_any_element()
                    } else {
                        div()
                            .w_full()
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(rgb(t.text_muted))
                            .child(AppIcon::File.size(px(22.0)).text_color(rgb(t.text_muted)))
                            .into_any_element()
                    })
                    .tooltip(move |_, cx| {
                        let __tip = name.clone();
                        cx.new(|_| Tooltip::new(__tip)).into()
                    })
                    .on_click({
                        let _this = this.clone();
                        cx.listener(move |this, _, _window, cx| {
                            this.remove_attachment(i, cx);
                        })
                    })
                    // 右上角移除按钮（覆盖在缩略图之上）。
                    .child(
                        div()
                            .id(ElementId::Name(format!("ai-att-remove-{}", i).into()))
                            .absolute()
                            .top(px(1.0))
                            .right(px(1.0))
                            .w(px(16.0))
                            .h(px(16.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(RADIUS_MD)
                            .bg(rgb(t.bg_secondary))
                            .border_1()
                            .border_color(rgb(t.border))
                            .cursor_pointer()
                            .text_color(rgb(t.text_muted))
                            .stateful_behavior(HoverBehavior {
                                hover_bg: rgb(t.border_active).into(),
                                ..Default::default()
                            })
                            .child(AppIcon::Close.size(ICON_SM).text_color(rgb(t.text_muted)))
                            .on_click({
                                let _this = this.clone();
                                cx.listener(move |this, _, _window, cx| {
                                    this.remove_attachment(i, cx);
                                })
                            })
                            .tooltip(move |_, cx| {
                                let __tip = remove_tip.clone();
                                cx.new(|_| Tooltip::new(__tip)).into()
                            }),
                    )
            }))
    }

    /// 输入框上方、终端「AI 解读」引用块：以差异化背景色展示被引用文本，
    /// 右侧提供删除与编辑按钮。编辑态切换为独立多行输入框，可保存修改。
    fn render_ai_quote(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);
        let _this = cx.entity();
        let quote = self.ai_quote.clone().unwrap_or_default();
        let quote_label = i18n!(cx, "ai_assistant.quote");
        let edit_tip = i18n!(cx, "ai_assistant.quote_edit");
        let delete_tip = i18n!(cx, "ai_assistant.quote_delete");
        let save_tip = i18n!(cx, "ai_assistant.quote_save");

        div().w_full().px(px(10.0)).pb(SPACE_SM).child(
            div()
                .w_full()
                .rounded(RADIUS_STD)
                .border_1()
                .border_color(rgb(t.border))
                .border_l(px(3.0))
                .border_color(p.surface_accent)
                .bg(p.surface_raised)
                .overflow_hidden()
                .child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap(SPACE_SM)
                        .px(SPACE_MD)
                        .py(SPACE_SM)
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .child(if self.ai_quote_editing {
                                    self.render_ai_quote_edit_input(cx).into_any_element()
                                } else {
                                    let quote_tip = quote.clone();
                                    div()
                                        .id("ai-quote-content")
                                        .w_full()
                                        .text_color(rgb(t.text_primary))
                                        .text_size(ui_text_md(cx))
                                        .tooltip(move |_, cx| {
                                            cx.new(|_| Tooltip::new(quote_tip.clone())).into()
                                        })
                                        // 默认最多显示一行，过长自动截断（hover 可查看完整引用）。
                                        .truncate()
                                        .whitespace_nowrap()
                                        .child(quote.lines().collect::<Vec<_>>().join(" "))
                                        .into_any_element()
                                }),
                        )
                        .child(
                            h_flex()
                                .flex_shrink_0()
                                .items_center()
                                .gap(px(2.0))
                                .when(self.ai_quote_editing, |d| {
                                    d.child(self.ai_icon_btn(
                                        "ai-quote-save",
                                        AppIcon::Check,
                                        save_tip.clone(),
                                        &t,
                                        cx,
                                        |this, window, cx| this.save_ai_quote(window, cx),
                                    ))
                                })
                                .when(!self.ai_quote_editing, |d| {
                                    d.child(self.ai_icon_btn(
                                        "ai-quote-edit",
                                        AppIcon::Edit,
                                        edit_tip.clone(),
                                        &t,
                                        cx,
                                        |this, window, cx| this.start_edit_ai_quote(window, cx),
                                    ))
                                })
                                .child(self.ai_icon_btn(
                                    "ai-quote-delete",
                                    AppIcon::Close,
                                    delete_tip.clone(),
                                    &t,
                                    cx,
                                    |this, window, cx| this.clear_ai_quote(window, cx),
                                )),
                        ),
                )
                .when(!self.ai_quote_editing, |d| {
                    d.child(
                        div()
                            .w_full()
                            .px(SPACE_MD)
                            .pb(px(4.0))
                            .text_color(rgb(t.text_muted))
                            .text_size(ui_text_xs(cx))
                            .child(quote_label),
                    )
                }),
        )
    }

    /// 引用内容编辑态的多行输入框。
    fn render_ai_quote_edit_input(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .when_some(self.ai_quote_input.as_ref(), |d, input| {
                d.child(Input::new(input))
            })
    }

    /// 输入框上方、引用块下方的「待发送消息队列」：列出当前生成进行中已经
    /// 用户输入但尚未发送的消息；当前生成终止 / 完成后会按序自动发送，
    /// 每条也提供手动移除按钮。
    fn render_pending_queue(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);
        let title = i18n!(cx, "ai_assistant.pending_queue");
        let remove_tip = i18n!(cx, "ai_assistant.pending_remove");

        div().w_full().px(px(10.0)).pb(SPACE_SM).child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_color(p.text_muted)
                        .text_size(ui_text_xs(cx))
                        .child(format!("{} ({})", title, self.ai_pending_queue.len())),
                )
                .children(self.ai_pending_queue.iter().enumerate().map(|(i, item)| {
                    let text = if item.text.is_empty() {
                        i18n!(cx, "ai_assistant.quote")
                    } else {
                        item.text.clone()
                    };
                    let tip = remove_tip.clone();
                    div()
                        .w_full()
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(p.border_subtle)
                        .bg(p.surface_raised)
                        .overflow_hidden()
                        .child(
                            h_flex()
                                .w_full()
                                .items_center()
                                .gap(SPACE_SM)
                                .px(SPACE_MD)
                                .py(px(6.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .truncate()
                                        .whitespace_nowrap()
                                        .text_color(rgb(t.text_primary))
                                        .text_size(ui_text_md(cx))
                                        .child(text),
                                )
                                .child(self.ai_icon_btn(
                                    "ai-pending-remove",
                                    AppIcon::Close,
                                    tip,
                                    &t,
                                    cx,
                                    move |this, _, cx| this.remove_pending(i, cx),
                                )),
                        )
                })),
        )
    }

    /// 从待发送队列中移除指定索引的消息。
    fn remove_pending(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.ai_pending_queue.len() {
            self.ai_pending_queue.remove(index);
            cx.notify();
        }
    }

    /// 进入引用编辑态：将当前引用内容载入编辑输入框。
    fn start_edit_ai_quote(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let quote = self.ai_quote.clone().unwrap_or_default();
        self.ai_quote_editing = true;
        let quote_input = self.ai_quote_input.get_or_insert_with(|| {
            cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(i18n!(cx, "ai_assistant.title"))
            })
        });
        quote_input.update(cx, |input, cx| {
            input.set_value(&quote, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    /// 保存编辑态下的引用内容。
    fn save_ai_quote(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let new_text = self
            .ai_quote_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string().trim().to_string())
            .unwrap_or_default();
        self.ai_quote = if new_text.is_empty() {
            None
        } else {
            Some(new_text)
        };
        self.ai_quote_editing = false;
        cx.notify();
    }

    /// 删除引用块。
    fn clear_ai_quote(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.ai_quote = None;
        self.ai_quote_editing = false;
        if let Some(ref input) = self.ai_quote_input {
            input.update(cx, |input, cx| input.set_value("", cx));
        }
        cx.notify();
    }

    /// 历史消息下拉列表：向上展开、与输入框等宽，仅展示用户发送的消息。
    /// 顶部带标题栏与关闭按钮；每条消息自动换行、最多两行（超出 "…" 截断），
    /// 点击平滑滚动定位到聊天主区域该消息的原始位置。
    fn render_history_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let this = cx.entity();
        let history = self.ai_history.clone();

        let title = i18n!(cx, "ai_assistant.history");
        let close_tip = i18n!(cx, "common.close");

        // 标题栏（固定，不随列表滚动）+ 右侧关闭按钮。
        let header = h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .px(SPACE_LG)
            .py(SPACE_MD)
            .border_b_1()
            .border_color(rgb(t.border))
            .child(
                div()
                    .text_size(ui_text_md(cx))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(t.text_primary))
                    .child(title),
            )
            .child(
                div()
                    .id("ai-history-close")
                    .w(px(20.0))
                    .h(px(20.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(RADIUS_STD)
                    .cursor_pointer()
                    .text_color(rgb(t.text_muted))
                    .stateful_behavior(HoverBehavior {
                        hover_bg: rgb(t.bg_hover).into(),
                        ..Default::default()
                    })
                    .child(AppIcon::Close.size(px(12.0)).text_color(rgb(t.text_muted)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_history(cx);
                    }))
                    .tooltip(move |_, cx| {
                        let __tip = close_tip.clone();
                        cx.new(|_| Tooltip::new(__tip)).into()
                    }),
            );

        let list = if history.is_empty() {
            div()
                .w_full()
                .px(SPACE_LG)
                .py(px(10.0))
                .text_size(ui_text_md(cx))
                .text_color(rgb(t.text_muted))
                .child(i18n!(cx, "ai_assistant.history_empty"))
                .into_any_element()
        } else {
            let scroll_handle = &self.ai_history_scroll;
            let items: Vec<AnyElement> = history
                .into_iter()
                .enumerate()
                .map(|(li, (msg_idx, text))| {
                    let this = this.clone();
                    let full_text = text.clone();
                    let truncated = truncate_two_lines(&text);
                    let is_truncated = text.chars().count() > 45;

                    let row = div()
                        .id(ElementId::Name(format!("ai-history-{}", li).into()))
                        .w_full()
                        .pl(SPACE_LG)
                        .pr(px(14.0))
                        .py(SPACE_SM)
                        .cursor_pointer()
                        .stateful_behavior(HoverBehavior {
                            hover_bg: rgb(t.bg_hover).into(),
                            ..Default::default()
                        })
                        .child(
                            h_flex().items_start().gap(SPACE_SM).child(
                                // min_w(0) 允许 flex 子项收缩并自动换行，避免右侧被截断。
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .text_size(ui_text_md(cx))
                                    .text_color(rgb(t.text_primary))
                                    .child(truncated),
                            ),
                        )
                        .on_click({
                            let _this = this.clone();
                            cx.listener(move |this, _, _window, cx| {
                                this.scroll_to_history_message(msg_idx, cx);
                            })
                        });

                    if is_truncated {
                        row.tooltip(move |_, cx| {
                            let __tip = full_text.clone();
                            cx.new(|_| Tooltip::new(__tip)).into()
                        })
                        .into_any_element()
                    } else {
                        row.into_any_element()
                    }
                })
                .collect();

            // 滚动容器：max_h 限高 + overflow_y_scroll + track_scroll 实现内容自适应滚动；
            // 叠加手动 Scrollbar（右侧可见、可拖动）。
            div()
                .id("ai-history-scroll-wrap")
                .relative()
                .w_full()
                .child(
                    div()
                        .id("ai-history-scroll")
                        .flex()
                        .flex_col()
                        .w_full()
                        .max_h(px(260.0))
                        .py(SPACE_XS)
                        .overflow_y_scroll()
                        .track_scroll(scroll_handle)
                        .children(items),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .right_0()
                        .left_0()
                        .child(
                            Scrollbar::new(scroll_handle)
                                .axis(ScrollbarAxis::Vertical)
                                .scrollbar_show(ScrollbarShow::Always),
                        ),
                )
                .into_any_element()
        };

        dropdown_overlay("ai-history-list", &t, cx)
            .max_h(px(320.0))
            .overflow_hidden()
            .child(v_flex().w_full().child(header).child(list))
    }

    fn render_ai_model_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = settings_entity(cx).read(cx).settings.clone();
        let options: Vec<SelectOption<String>> = s
            .ai_models
            .iter()
            .filter(|m| m.enabled)
            .map(|m| SelectOption::new(m.id.clone(), m.name.clone()))
            .collect();

        let selected = self
            .ai_selected_model_id
            .clone()
            .or_else(|| s.ai_default_model_id.clone())
            .or_else(|| options.first().map(|m| m.value.clone()));

        self.ai_model_select.update(cx, |state, cx| {
            state.set_options(options, cx);
            if state.selected_value() != selected.as_ref() {
                state.set_selected_value(selected, cx);
            }
        });

        div().w(px(140.0)).child(Select::new(&self.ai_model_select))
    }

    fn render_perm_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current_perm = self.ai_permission;
        self.ai_perm_select.update(cx, |state, cx| {
            if state.selected_value() != Some(&current_perm) {
                state.set_selected_value(Some(current_perm), cx);
            }
        });

        div().w(px(110.0)).child(Select::new(&self.ai_perm_select))
    }

    fn enabled_skills(&self, cx: &App) -> Vec<SkillMeta> {
        let s = settings_entity(cx).read(cx).settings.clone();
        self.ai_client
            .list_skills()
            .into_iter()
            .filter(|meta| s.is_ai_skill_enabled(&meta.name))
            .collect()
    }

    fn filtered_slash_skills_with_text(&self, input_val: &str, cx: &App) -> Vec<SkillMeta> {
        let query = if input_val.starts_with('/') {
            input_val[1..].trim().to_lowercase()
        } else {
            String::new()
        };

        self.enabled_skills(cx)
            .into_iter()
            .filter(|meta| {
                if query.is_empty() {
                    return true;
                }
                meta.name.to_lowercase().contains(&query)
                    || i18n!(cx, &meta.i18n_key).to_lowercase().contains(&query)
            })
            .collect()
    }

    fn filtered_slash_skills(&self, cx: &App) -> Vec<SkillMeta> {
        let input_val = self
            .chat_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_default();
        self.filtered_slash_skills_with_text(&input_val, cx)
    }

    fn render_slash_menu(&self, cx: &mut Context<Self>) -> Option<Deferred> {
        if !self.ai_slash_menu_open {
            return None;
        }

        let bounds = self.ai_input_area_bounds?;
        let skills = self.filtered_slash_skills(cx);
        if skills.is_empty() {
            return None;
        }

        let t = theme(cx);
        let selected_idx = self
            .ai_slash_selected_index
            .min(skills.len().saturating_sub(1));

        let items: Vec<AnyElement> = skills
            .into_iter()
            .enumerate()
            .map(|(idx, meta)| {
                let name = meta.name.clone();
                let is_selected = idx == selected_idx;
                let bg = if is_selected {
                    surface_bg(t.bg_selection, cx)
                } else {
                    transparent_black()
                };

                let label = format!("{} ({})", i18n!(cx, &meta.i18n_key), meta.name);
                let desc = i18n!(cx, meta.tier.label_key());

                div()
                    .id(ElementId::Name(format!("slash-skill-{}", meta.name).into()))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(SPACE_MD)
                    .py(SPACE_SM)
                    .rounded(RADIUS_MD)
                    .bg(bg)
                    .cursor_pointer()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(ui_text_md(cx))
                                    .text_color(rgb(t.text_primary))
                                    .child(label),
                            )
                            .child(
                                div()
                                    .text_size(ui_text_sm(cx))
                                    .text_color(rgb(t.text_muted))
                                    .child(desc),
                            ),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _window, cx| {
                            this.ai_slash_menu_open = false;
                            if let Some(ref input) = this.chat_input {
                                input.update(cx, |s, cx| s.set_value(&format!("/{} ", name), cx));
                            }
                            cx.notify();
                        }),
                    )
                    .into_any_element()
            })
            .collect();

        let list_container = div()
            .id("slash-popover-wrap")
            .relative()
            .w_full()
            .child(
                div()
                    .id("slash-popover-scroll")
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .p(SPACE_XS)
                    .max_h(px(200.0))
                    .overflow_y_scroll()
                    .track_scroll(&self.ai_slash_scroll_handle)
                    .children(items),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .left_0()
                    .child(
                        Scrollbar::new(&self.ai_slash_scroll_handle)
                            .axis(ScrollbarAxis::Vertical)
                            .scrollbar_show(ScrollbarShow::Always),
                    ),
            );

        let popover_box = dropdown_overlay("slash-popover-box", &t, cx).child(list_container);

        Some(dropdown_anchored_above(bounds, popover_box))
    }

    /// 当前选中的模型配置（供 Skills / Agent 模式复用）。
    fn current_model_config(&self, cx: &App) -> AiModelConfig {
        let s = settings_entity(cx).read(cx).settings.clone();
        s.ai_models
            .iter()
            .find(|m| Some(&m.id) == self.ai_selected_model_id.as_ref())
            .or_else(|| {
                s.ai_default_model_id
                    .as_ref()
                    .and_then(|id| s.ai_models.iter().find(|m| &m.id == id))
            })
            .or_else(|| s.ai_models.iter().find(|m| m.enabled))
            .cloned()
            .unwrap_or_else(|| {
                AiModelConfig::new(String::new(), String::new(), String::new(), String::new())
            })
    }

    fn new_ai_chat(&mut self, cx: &mut Context<Self>) {
        self.ai_stream_rx = None;
        self.ai_streaming_index = None;
        self._ai_stream_task = None;
        self.ai_agent_rx = None;
        self.ai_tool_rx = None;
        self.ai_agent_registry = None;
        self._ai_agent_task = None;
        self._ai_consume_task = None;
        self.ai_current_request_id = None;
        self.ai_pending_queue.clear();
        self.messages.clear();
        self.attachments.clear();
        self.ai_history_open = false;
        self.ai_history.clear();
        self.push_message(ChatMessage {
            is_user: false,
            text: i18n!(cx, "ai_assistant.welcome"),
            streaming: false,
            document_views: std::cell::RefCell::new(Vec::new()),
            tool_call: None,
            thinking: None,
            attachments: Vec::new(),

            quote: None,
        });
        let idx = self.messages.len() - 1;
        self.update_message_input_states(idx, cx);
        self.save_current_sessions_to_disk();
        cx.notify();
    }

    fn clear_ai_chat(&mut self, cx: &mut Context<Self>) {
        self.ai_stream_rx = None;
        self.ai_streaming_index = None;
        self._ai_stream_task = None;
        self.ai_agent_rx = None;
        self.ai_tool_rx = None;
        self.ai_agent_registry = None;
        self._ai_agent_task = None;
        self._ai_consume_task = None;
        self.ai_current_request_id = None;
        self.ai_pending_queue.clear();
        self.messages.clear();
        self.attachments.clear();
        self.ai_history_open = false;
        self.ai_history.clear();
        self.push_message(ChatMessage {
            is_user: false,
            text: i18n!(cx, "ai_assistant.cleared"),
            streaming: false,
            document_views: std::cell::RefCell::new(Vec::new()),
            tool_call: None,
            thinking: None,
            attachments: Vec::new(),

            quote: None,
        });
        let idx = self.messages.len() - 1;
        self.update_message_input_states(idx, cx);
        self.save_current_sessions_to_disk();
        cx.notify();
    }

    fn refresh_ai(&mut self, cx: &mut Context<Self>) {
        let text = self.ai_client.refresh_context(cx);
        self.push_message(ChatMessage {
            is_user: false,
            text,
            streaming: false,
            document_views: std::cell::RefCell::new(Vec::new()),
            tool_call: None,
            thinking: None,
            attachments: Vec::new(),

            quote: None,
        });
        let idx = self.messages.len() - 1;
        self.update_message_input_states(idx, cx);
        cx.notify();
    }

    fn toggle_ai_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ai_search_open = !self.ai_search_open;
        if self.ai_search_open {
            let input = self.ai_search_input.get_or_insert_with(|| {
                let input = cx.new(|cx| {
                    InputState::new(cx)
                        .placeholder(i18n!(cx, "ai_assistant.search_placeholder"))
                });
                let input_clone = input.clone();
                cx.subscribe(
                    &input_clone,
                    |this: &mut Self, _, _: &velowork_ui::input::InputEvent, cx| {
                        this.ai_search_flat_index = None;
                        cx.notify();
                    },
                )
                .detach();
                input
            });
            input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(cx);
            });
        } else if let Some(ref input) = self.ai_search_input {
            input.update(cx, |input, cx| input.set_value("", cx));
        }
        cx.notify();
    }

    /// 关闭 AI 助手右键菜单并清空状态（`open_ai_context_menu` 的 `on_close` 通过此公开方法跨模块清理）。
    pub fn close_ai_context_menu(&mut self, cx: &mut Context<Self>) {
        self.ai_context_menu = None;
        cx.notify();
    }

    fn render_ai_assistant(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);
        let this = cx.entity().clone();

        let chat_input = self
            .chat_input
            .get_or_insert_with(|| {
                let input = cx.new(|cx| {
                    InputState::new(cx)
                        .multiline()
                        .wrap(true)
                        .fill_height(true)
                        .placeholder(i18n!(cx, "ai_assistant.title"))
                });
                let input_clone = input.clone();
                cx.subscribe(
                    &input_clone,
                    |this: &mut Self, _, _: &velowork_ui::input::InputEvent, cx| {
                        let val = this
                            .chat_input
                            .as_ref()
                            .map(|i| i.read(cx).text().to_string())
                            .unwrap_or_default();
                        if val.starts_with('/') && !val.contains(' ') {
                            this.ai_slash_menu_open = true;
                            let count = this.filtered_slash_skills(cx).len();
                            if count > 0 {
                                this.ai_slash_selected_index =
                                    this.ai_slash_selected_index.min(count - 1);
                            } else {
                                this.ai_slash_selected_index = 0;
                            }
                            this.ai_slash_scroll_handle
                                .scroll_to_item(this.ai_slash_selected_index);
                        } else {
                            this.ai_slash_menu_open = false;
                        }
                        cx.notify();
                    },
                )
                .detach();
                input
            })
            .clone();

        if self.ai_search_open && self.ai_search_input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(i18n!(cx, "ai_assistant.search_placeholder"))
            });
            let input_clone = input.clone();
            cx.subscribe(
                &input_clone,
                |this: &mut Self, _, _: &velowork_ui::input::InputEvent, cx| {
                    this.ai_search_flat_index = None;
                    cx.notify();
                },
            )
            .detach();
            self.ai_search_input = Some(input);
        }

        let search_query = if self.ai_search_open {
            self.ai_search_input
                .as_ref()
                .map(|i| i.read(cx).text().to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let case_sensitive = self.ai_search_case_sensitive;
        let use_regex = self.ai_search_use_regex;
        let current_match = self.ai_search_flat_index;

        let messages: Vec<ChatMessage> = self.messages.iter().cloned().collect();

        let ai_configured = {
            let s = settings_entity(cx).read(cx).settings.clone();
            s.ai_enabled && s.ai_models.iter().any(|m| m.enabled)
        };

        if !ai_configured {
            return v_flex()
                .track_focus(&self.focus_handle)
                .key_context("AiAssistantPanel")
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .px(SPACE_XL)
                .child(
                    div()
                        .px(SPACE_XL)
                        .py(SPACE_XL)
                        .rounded(px(6.0))
                        .bg(p.surface_raised)
                        .border_1()
                        .border_color(p.surface_accent.opacity(0.4))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(SPACE_MD)
                        .child(
                            div()
                                .text_size(ui_text_md(cx))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.text_secondary)
                                .child(i18n!(cx, "ai_assistant.no_model")),
                        )
                        .child(
                            div()
                                .text_size(ui_text_md(cx))
                                .text_color(p.text_muted)
                                .child(i18n!(cx, "ai_assistant.no_model_hint")),
                        )
                        .child(
                            div()
                                .id("ai-go-settings")
                                .mt(SPACE_XS)
                                .px(SPACE_LG)
                                .py(SPACE_XS)
                                .rounded(RADIUS_STD)
                                .bg(p.surface_accent)
                                .text_size(ui_text_md(cx))
                                .text_color(p.text_on_accent)
                                .cursor_pointer()
                                .stateful_behavior(HoverBehavior {
                                    hover_bg: p.surface_accent_hover,
                                    ..Default::default()
                                })
                                .child(i18n!(cx, "ai_assistant.go_settings"))
                                .on_click(cx.listener(|_, _, window, cx| {
                                    window.dispatch_action(
                                        Box::new(crate::keybindings::ShowAiSettings),
                                        cx,
                                    );
                                })),
                        ),
                )
                .into_any();
        }

        let is_custom_titlebar = if cfg!(target_os = "macos") {
            settings_entity(cx).read(cx).settings.titlebar_style
                == velowork_workspace::settings::TitlebarStyle::Custom
        } else {
            matches!(
                window.window_decorations(),
                gpui::Decorations::Client { .. }
            )
        };
        let window_corner_radius = settings_entity(cx).read(cx).settings.window_corner_radius;
        let is_maximized = window.is_maximized();
        let is_fullscreen = window.is_fullscreen();
        let has_rounded_corners =
            is_custom_titlebar && !is_maximized && !is_fullscreen && window_corner_radius > 0.0;
        let radius = px(window_corner_radius);

        v_flex()
            .track_focus(&self.focus_handle)
            .key_context("AiAssistantPanel")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .overflow_hidden()
            .when(has_rounded_corners, |d| d.rounded_br(radius))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let cmd_or_ctrl = event.keystroke.modifiers.platform || event.keystroke.modifiers.control;
                if cmd_or_ctrl && event.keystroke.key.as_str() == "f" {
                    this.ai_search_open = true;
                    let input = this.ai_search_input.get_or_insert_with(|| {
                        let input = cx.new(|cx| {
                            InputState::new(cx)
                                .placeholder(i18n!(cx, "ai_assistant.search_placeholder"))
                        });
                        let input_clone = input.clone();
                        cx.subscribe(
                            &input_clone,
                            |this: &mut Self, _, _: &velowork_ui::input::InputEvent, cx| {
                                this.ai_search_flat_index = None;
                                cx.notify();
                            },
                        )
                        .detach();
                        input
                    });
                    input.update(cx, |inp, cx| {
                        inp.focus(window, cx);
                        inp.select_all(cx);
                    });
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                if event.keystroke.key.as_str() == "escape" && this.ai_search_open {
                    this.ai_search_open = false;
                    if let Some(ref input) = this.ai_search_input {
                        input.update(cx, |inp, cx| inp.set_value("", cx));
                    }
                    if let Some(ref chat_input) = this.chat_input {
                        chat_input.update(cx, |inp, cx| inp.focus(window, cx));
                    } else {
                        window.focus(&this.focus_handle, cx);
                    }
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(
                h_flex()
                    .h(px(velowork_ui::tab_height(cx)))
                    .px(ui_space_md(cx))
                    .border_b_1()
                    .border_color(rgb(t.border))
                    .items_center()
                    .gap(ui_space_xs(cx))
                    .child(self.ai_icon_btn(
                        "ai-new-chat",
                        AppIcon::Plus,
                        i18n!(cx, "ai_assistant.new_chat"),
                        &t,
                        cx,
                        |this, _, cx| this.new_ai_chat(cx),
                    ))
                    .child(self.ai_icon_btn(
                        "ai-clear-chat",
                        AppIcon::Trash,
                        i18n!(cx, "ai_assistant.clear_tooltip"),
                        &t,
                        cx,
                        |this, _, cx| this.clear_ai_chat(cx),
                    ))
                    .child(self.ai_icon_btn(
                        "ai-refresh",
                        AppIcon::Refresh,
                        i18n!(cx, "common.refresh"),
                        &t,
                        cx,
                        |this, _, cx| this.refresh_ai(cx),
                    ))
                    .child(div().w(px(1.0)).h(ICON_STD).bg(rgb(t.border)).mx(ui_space_xs(cx)))
                    .child(self.ai_icon_btn(
                        "ai-search",
                        AppIcon::Search,
                        i18n!(cx, "ai_assistant.search"),
                        &t,
                        cx,
                        |this, window, cx| this.toggle_ai_search(window, cx),
                    ))
                    .child(div().flex_1())
                    .child({
                        let used_tokens = self.current_session_tokens();
                        let max_tokens = settings_entity(cx).read(cx).settings.ai_max_context_tokens;
                        let token_text = format!("{} / {} T", used_tokens, max_tokens);
                        let tooltip_text = i18n!(cx, "ai_assistant.token_usage_hint");
                        div()
                            .id("ai-token-usage-badge")
                            .px(SPACE_XS)
                            .py(px(2.0))
                            .rounded(RADIUS_STD)
                            .bg(surface_bg(t.bg_hover, cx))
                            .child(
                                div()
                                    .text_size(ui_text_xs(cx))
                                    .text_color(rgb(t.text_muted))
                                    .child(token_text),
                            )
                            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(tooltip_text.clone())).into())
                    }),
            )
            .child({
                let panel_entity = cx.entity().clone();
                div()
                    .id("ai-chat-scroll-region")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .on_hover(move |hovered, _window, cx| {
                        panel_entity.update(cx, |this, cx| {
                            if this.ai_scrollbar_hovered.get() != *hovered {
                                this.ai_scrollbar_hovered.set(*hovered);
                                cx.notify();
                            }
                        });
                    })
                    .child(
                        div()
                            .id("ai-messages-container")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.ai_scroll_handle)
                            .flex()
                            .flex_col()
                            .px(ui_space_md(cx))
                            .py(ui_space_md(cx))
                            .gap(ui_space_sm(cx))
                            .children({
                                let panel_entity = cx.entity().clone();
                                // 预提取已复制索引，避免在 render_ai_message 中调用 entity.read()
                                // 触发 GPUI 实体借出冲突（entity_map.rs:164）。
                                let copied_indices: Vec<usize> =
                                    self.ai_copy_done_indices.borrow().clone();
                                let mut msg_elements: Vec<AnyElement> = Vec::new();
                                let mut flat = 0usize;
                                let total_msgs = messages.len();
                                let frame = self.animation_frame;
                                // 保证逐行显示动画状态向量与消息数对齐：新消息以当前帧为起始。
                                {
                                    let mut starts = self.ai_reveal_start.borrow_mut();
                                    let mut revealed = self.ai_reveal_revealed.borrow_mut();
                                    while starts.len() < total_msgs {
                                        starts.push(frame);
                                        revealed.push(0);
                                    }
                                }
                                // 归并同一轮 AI 回复中的多次工具调用：工具调用消息不再单独成气泡，
                                // 而是挂到最近的 AI 回复消息（`current_ai_idx`）下，渲染为可折叠的工具组。
                                let mut current_ai_idx: Option<usize> = None;
                                let mut tool_calls_by_ai: std::collections::HashMap<
                                    usize,
                                    Vec<ToolCallCardData>,
                                > = std::collections::HashMap::new();
                                let expanded_tools = self.ai_expanded_tools.borrow();
                                for (mi, msg) in messages.iter().enumerate() {
                                    // 独立的工具调用消息（来自 Agent 事件流）：收集到归属 AI 消息，跳过单独渲染。
                                    if !msg.is_user && msg.tool_call.is_some() && msg.text.trim().is_empty() {
                                        let owner = current_ai_idx.unwrap_or(mi);
                                        if let Some(tc) = &msg.tool_call {
                                            tool_calls_by_ai
                                                .entry(owner)
                                                .or_default()
                                                .push(tc.clone());
                                        }
                                        continue;
                                    }
                                    if msg.is_user {
                                        current_ai_idx = None;
                                    } else {
                                        current_ai_idx = Some(mi);
                                    }
                                    let attached_tool_calls =
                                        tool_calls_by_ai.remove(&mi).unwrap_or_default();
                                    let copied = copied_indices.contains(&mi);
                                    // 计算本条 AI 消息已显示的文本行数（逐行显示动画）：
                                    // 含代码块或工具调用的消息直接完整渲染（跳过逐行动画）。
                                    let revealed_lines = if !msg.is_user {
                                        self.ai_revealed_lines(mi, msg, frame)
                                    } else {
                                        0usize
                                    };
                                    // 记录已显示行数，供 update_message_input_states 与动画判断复用。
                                    let reveal_changed = {
                                        let mut revealed = self.ai_reveal_revealed.borrow_mut();
                                        let changed =
                                            revealed.get(mi).copied().unwrap_or(0) < revealed_lines;
                                        if changed {
                                            revealed[mi] = revealed_lines;
                                        }
                                        changed
                                    };
                                    let (el, cnt) = render_ai_message(
                                        msg,
                                        &t,
                                        window,
                                        cx,
                                        &self.focus_manager,
                                        &self.workspace,
                                        &self.terminals,
                                        &search_query,
                                        case_sensitive,
                                        use_regex,
                                        mi,
                                        flat,
                                        current_match,
                                        &self.ai_expanded_quotes.borrow(),
                                        &expanded_tools,
                                        &attached_tool_calls,
                                        &panel_entity,
                                        self.ai_editing_index,
                                        self.ai_edit_input.clone(),
                                        copied,
                                        frame,
                                        revealed_lines,
                                        reveal_changed,
                                    );
                                    flat += cnt;
                                    // 用户消息发送后即刻完整渲染（无逐字 / 渐入动画）；
                                    // AI 消息整体直接显示，内容行由逐行动画控制节奏。
                                    msg_elements.push(el);
                                }
                                // ── 智能自动滚动 ──
                                // 动画 / 流式期间平滑跟随最新内容；用户上滚查看历史时立即暂停，
                                // 重新滚动至底部后恢复自动跟随。
                                {
                                    let max = self.ai_scroll_handle.max_offset();
                                    let cur = self.ai_scroll_handle.offset();
                                    let prev = self.ai_prev_scroll_offset.get();
                                    let at_bottom = (cur.y + max.y).abs() <= px(4.0);

                                    if self.ai_autoscroll.get() {
                                        // 偏移量增大（向上）且已离开底部区域，说明用户主动上滚查看历史，立即暂停自动跟随。
                                        if cur.y > prev.y + px(0.5) && !at_bottom {
                                            self.ai_autoscroll.set(false);
                                        }
                                    } else if at_bottom {
                                        // 用户重新滚动至底部，恢复自动跟随。
                                        self.ai_autoscroll.set(true);
                                    }

                                    if self.ai_autoscroll.get() {
                                        let bottom = -max.y;
                                        let dist = bottom - cur.y;
                                        if dist.abs() > px(0.5) {
                                            // 平滑插值跟随最新内容，避免新消息被遮挡。
                                            let new_y = cur.y + dist * 0.35;
                                            self.ai_scroll_handle.set_offset(Point {
                                                x: px(0.0),
                                                y: new_y,
                                            });
                                        }
                                    }
                                    self.ai_prev_scroll_offset
                                        .set(self.ai_scroll_handle.offset());
                                }
                                msg_elements
                            }),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right_0()
                            .left_0()
                            .child(
                                Scrollbar::new(&self.ai_scroll_handle)
                                    .axis(ScrollbarAxis::Vertical)
                                    .scrollbar_show(if self.ai_scrollbar_hovered.get() {
                                        ScrollbarShow::Always
                                    } else {
                                        ScrollbarShow::Never
                                    }),
                            ),
                    )
            })
            // 统一圆角容器：拖拽手柄 + 输入框（无边框）+ 底部工具栏（模型 / 权限 / 发送）
            .child({
                let input_height = px(self.ai_input_area_height);
                // ── 顶部拖拽手柄 ──
                let drag_entity = cx.entity().downgrade();
                let drag_entity_for_handle = drag_entity.clone();
                let resize_handle =
                    ResizeHandle::new(true, t.border, t.border_active, move |pos, app_cx| {
                        if let Some(entity) = drag_entity_for_handle.upgrade() {
                            entity.update(app_cx, |this, cx| {
                                this.ai_input_resize_dragging = Some(AiInputResizeDrag {
                                    start_y: f32::from(pos.y),
                                    start_height: this.ai_input_area_height,
                                });
                                cx.notify();
                            });
                        }
                    });

                div()
                    .id("ai-input-area")
                    .relative()
                    .flex_shrink_0()
                    .mx(ui_space_sm(cx))
                    .mb(ui_space_sm(cx))
                    .h(input_height)
                    .rounded(RADIUS_LG)
                    .border_1()
                    .border_color(rgb(t.border))
                    .bg(surface_bg(t.bg_secondary, cx))
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            if let Some(ref input) = this.chat_input {
                                input.update(cx, |input, cx| {
                                    input.focus(window, cx);
                                });
                            }
                            this.close_history(cx);
                        }),
                    )
                    // 追踪输入框区域 bounds，用于把历史下拉锚定在其正上方且等宽。
                    .child(
                        canvas(
                            {
                                let this = this.clone();
                                move |b: Bounds<Pixels>, _window: &mut Window, cx: &mut App| {
                                    this.update(cx, |this, _cx| {
                                        this.ai_input_area_bounds = Some(b);
                                    });
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    // ── 拖拽调整高度手柄（顶部边缘） ──
                    .child(resize_handle)
                    // ── 顶部工具栏：附件 | 历史 ──
                    .child(
                        h_flex()
                            .flex_shrink_0()
                            .w_full()
                            .px(px(10.0))
                            .pt(ui_space_sm(cx))
                            .pb(px(2.0))
                            .items_center()
                            .gap(ui_space_sm(cx))
                            .child(self.ai_icon_btn(
                                "ai-attach-btn",
                                AppIcon::NewFile,
                                i18n!(cx, "ai_assistant.attach"),
                                &t,
                                cx,
                                |this, window, cx| this.attach_files(window, cx),
                            ))
                            .child(self.ai_icon_btn(
                                "ai-history-btn",
                                AppIcon::MonitorClock,
                                i18n!(cx, "ai_assistant.history"),
                                &t,
                                cx,
                                |this, window, cx| this.toggle_history(window, cx),
                            )),
                    )
                    // ── 附件预览行（有附件时显示）──
                    .when(!self.attachments.is_empty(), |d| {
                        d.child(self.render_attachment_preview(cx))
                    })
                    // ── 终端「AI 解读」引用块（有引用时显示，差异化背景色）──
                    .when(self.ai_quote.is_some(), |d| {
                        d.child(self.render_ai_quote(cx))
                    })
                    // ── 待发送消息队列（生成中用户输入新消息入队后显示，当前生成结束后自动发送）──
                    .when(!self.ai_pending_queue.is_empty(), |d| {
                        d.child(self.render_pending_queue(cx))
                    })
                    // ── 输入区域（无边框，与容器融为一体，flex_1 填充剩余空间）──
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .key_context("AiChatInput")
                            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                                if event.keystroke.key.as_str() == "enter"
                                    && event.keystroke.modifiers.control
                                {
                                    this.on_send_button(window, cx);
                                    cx.stop_propagation();
                                }
                            }))
                            .child(
                                div()
                                    .id("ai-input-wrapper")
                                    .w_full()
                                    .h_full()
                                    .pl(ui_space_lg(cx))
                                    .py(px(4.0))
                                    .child(
                                        Input::new(&chat_input)
                                            .borderless(true)
                                            .fill_height()
                                    ),
                            ),
                    )
                    // ── 底部工具栏：模型 ▼ | 权限 ▼ | ▶ 发送 ──
                    .child(
                        h_flex()
                            .flex_shrink_0()
                            .w_full()
                            .px(px(10.0))
                            .pb(ui_space_md(cx))
                            .items_center()
                            .justify_between()
                            .child(
                                h_flex()
                                    .gap(ui_space_sm(cx))
                                    .child(self.render_ai_model_dropdown(cx))
                                    .child(self.render_perm_dropdown(cx)),
                            )
                            .child(
                                velowork_ui::icon_button::icon_button_sized(
                                    "ai-send-icon",
                                    self.ai_send_button_icon(cx),
                                    24.0,
                                    14.0,
                                    &t,
                                )
                                .tooltip({
                                    let tip = self.ai_send_button_tooltip(cx);
                                    move |_, cx| {
                                        cx.new(|_| Tooltip::new(tip.clone())).into()
                                    }
                                })
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.on_send_button(window, cx);
                                    },
                                )),
                            ),
                    )
                    // ── 拖拽 mouse-move / mouse-up 追踪 ──
                    .child(
                        canvas(|_bounds, _window, _cx| {}, {
                            let ent = drag_entity.clone();
                            move |_bounds, _prepaint, window, _cx| {
                                let ent_move = ent.clone();
                                window.on_mouse_event(move |e: &MouseMoveEvent, phase, _window, cx| {
                                    if phase != DispatchPhase::Bubble {
                                        return;
                                    }
                                    if let Some(entity) = ent_move.upgrade() {
                                        entity.update(cx, |this, cx| {
                                            if let Some(drag) = this.ai_input_resize_dragging {
                                                // 向上拖 = start_y > current_y = 增大高度
                                                let delta = drag.start_y - f32::from(e.position.y);
                                                let new_h = (drag.start_height + delta)
                                                    .clamp(AI_INPUT_AREA_MIN_HEIGHT, AI_INPUT_AREA_MAX_HEIGHT);
                                                this.ai_input_area_height = new_h;
                                                cx.notify();
                                            }
                                        });
                                    }
                                });
                                let ent_up = ent.clone();
                                window.on_mouse_event(move |e: &MouseUpEvent, phase, _window, cx| {
                                    if phase != DispatchPhase::Bubble {
                                        return;
                                    }
                                    if e.button != MouseButton::Left {
                                        return;
                                    }
                                    if let Some(entity) = ent_up.upgrade() {
                                        entity.update(cx, |this, cx| {
                                            if this.ai_input_resize_dragging.is_some() {
                                                this.ai_input_resize_dragging = None;
                                                cx.notify();
                                            }
                                        });
                                    }
                                });
                            }
                        })
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(SPACE_XS),
                    )
            })
            .when(
                self.ai_history_open && self.ai_input_area_bounds.is_some(),
                |d| {
                    let b = self.ai_input_area_bounds.unwrap();
                    // 全窗口透明遮罩：点击外侧自动收起历史面板。
                    let size = window.window_bounds().get_bounds().size;
                    let backdrop = deferred(
                        anchored()
                            .position(Point::default())
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .w(size.width)
                                    .h(size.height)
                                    .on_mouse_down(MouseButton::Left, {
                                        let this = this.clone();
                                        move |_, _window, cx| {
                                            this.update(cx, |panel, cx| panel.close_history(cx));
                                        }
                                    }),
                            ),
                    );
                    d.child(backdrop)
                        .child(dropdown_anchored_above(b, self.render_history_list(cx)))
                },
            )
            .when(
                self.ai_slash_menu_open && self.ai_input_area_bounds.is_some(),
                |d| {
                    if let Some(menu) = self.render_slash_menu(cx) {
                        d.child(menu)
                    } else {
                        d
                    }
                },
            )
            .when(self.ai_search_open, |d| {
                let p = SemanticPalette::from_context(cx);
                let case_sensitive = self.ai_search_case_sensitive;
                let is_regex = self.ai_search_use_regex;
                let search_q = self
                    .ai_search_input
                    .as_ref()
                    .map(|i| i.read(cx).text().to_string())
                    .unwrap_or_default();
                let cs = self.ai_search_case_sensitive;
                let total_matches: usize = if search_q.is_empty() {
                    0
                } else {
                    self.messages
                        .iter()
                        .map(|m| find_match_ranges(&m.text, &search_q, cs, is_regex).len())
                        .sum()
                };
                let match_text = if total_matches == 0 {
                    "0/0".to_string()
                } else {
                    let current_idx = self.ai_search_flat_index.map(|i| i + 1).unwrap_or(1);
                    format!("{}/{}", current_idx, total_matches)
                };

                let case_tip = i18n!(cx, "terminal.search_case_sensitive");
                let regex_tip = i18n!(cx, "terminal.search_regex");
                let prev_tip = i18n!(cx, "terminal.search_prev");
                let next_tip = i18n!(cx, "terminal.search_next");
                let close_tip = i18n!(cx, "terminal.search_close");

                d.child(
                    div()
                        .id("ai-search-bar")
                        .occlude()
                        .absolute()
                        .top(px(velowork_ui::tab_height(cx)) + ui_space_sm(cx))
                        .right(px(24.0))
                        .h(px(38.0))
                        .px(ui_space_md(cx))
                        .flex()
                        .items_center()
                        .gap(ui_space_sm(cx))
                        .bg(p.surface_raised)
                        .border_1()
                        .border_color(rgb(t.border))
                        .rounded(RADIUS_LG)
                        .shadow_xl()
                        .max_w(relative(0.9))
                        .when(use_custom_ui_font(cx), |d| {
                            d.font_family(ui_font_family(cx))
                        })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .on_mouse_down(MouseButton::Right, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .on_mouse_move(|_, _, cx| {
                            cx.stop_propagation();
                        })
                        .on_scroll_wheel(|_, _, cx| {
                            cx.stop_propagation();
                        })
                        // 外层边框组合：输入框 + 大小写/正则图标，图标内嵌于输入框右侧
                        .child(
                            div()
                                .id("ai-search-input-group")
                                .h(px(28.0))
                                .w(px(260.0))
                                .flex()
                                .items_center()
                                .bg(p.surface_base)
                                .border_1()
                                .border_color(rgb(t.border))
                                .rounded(RADIUS_MD)
                                .child(
                                    div()
                                        .id("ai-search-input-wrapper")
                                        .key_context("AiSearchBar")
                                        .flex_1()
                                        .h_full()
                                        .flex()
                                        .items_center()
                                        .child(if let Some(ref search_input) = self.ai_search_input {
                                            Input::new(search_input).borderless(true).into_any_element()
                                        } else {
                                            div().into_any_element()
                                        })
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation();
                                        })
                                        .on_key_down(cx.listener(
                                            |this, event: &KeyDownEvent, window, cx| {
                                                if event.keystroke.key.as_str() == "escape" {
                                                    cx.stop_propagation();
                                                    this.ai_search_open = false;
                                                    if let Some(ref input) = this.ai_search_input {
                                                        input.update(cx, |inp, cx| inp.set_value("", cx));
                                                    }
                                                    if let Some(ref chat_input) = this.chat_input {
                                                        chat_input.update(cx, |inp, cx| inp.focus(window, cx));
                                                    } else {
                                                        window.focus(&this.focus_handle, cx);
                                                    }
                                                    cx.notify();
                                                    return;
                                                }
                                                if event.keystroke.key.as_str() == "enter" {
                                                    cx.stop_propagation();
                                                    if event.keystroke.modifiers.shift {
                                                        this.search_prev(cx);
                                                    } else {
                                                        this.search_next(cx);
                                                    }
                                                }
                                            },
                                        )),
                                )
                                .child({
                                    let cs = case_sensitive;
                                    let case_tip = case_tip.clone();
                                    div()
                                        .id("ai-search-case-btn")
                                        .cursor_pointer()
                                        .w(px(24.0))
                                        .h(px(24.0))
                                        .ml(px(2.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(RADIUS_MD)
                                        .when(cs, |s| s.bg(rgb(t.bg_selection)))
                                        .stateful_behavior(HoverBehavior {
                                            hover_bg: rgb(t.bg_hover).into(),
                                            ..Default::default()
                                        })
                                        .tooltip(move |_, cx| {
                                            let tip = case_tip.clone();
                                            cx.new(|_| Tooltip::new(tip)).into()
                                        })
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation();
                                        })
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.ai_search_case_sensitive =
                                                !this.ai_search_case_sensitive;
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .text_size(ui_text_md(cx))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(if cs {
                                                    p.text_primary
                                                } else {
                                                    p.text_secondary
                                                })
                                                .child("Aa"),
                                        )
                                })
                                .child({
                                    let rx = is_regex;
                                    let regex_tip = regex_tip.clone();
                                    div()
                                        .id("ai-search-regex-btn")
                                        .cursor_pointer()
                                        .w(px(24.0))
                                        .h(px(24.0))
                                        .ml(px(2.0))
                                        .mr(px(2.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(RADIUS_MD)
                                        .when(rx, |s| s.bg(rgb(t.bg_selection)))
                                        .stateful_behavior(HoverBehavior {
                                            hover_bg: rgb(t.bg_hover).into(),
                                            ..Default::default()
                                        })
                                        .tooltip(move |_, cx| {
                                            let tip = regex_tip.clone();
                                            cx.new(|_| Tooltip::new(tip)).into()
                                        })
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation();
                                        })
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.ai_search_use_regex = !this.ai_search_use_regex;
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .text_size(ui_text_md(cx))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(if rx {
                                                    p.text_primary
                                                } else {
                                                    p.text_secondary
                                                })
                                                .child(".*"),
                                        )
                                }),
                        )
                        .child(
                            div()
                                .text_size(ui_text_md(cx))
                                .text_color(p.text_secondary)
                                .min_w(px(40.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(match_text),
                        )
                        .child({
                            let prev_tip = prev_tip.clone();
                            div()
                                .id("ai-search-prev-btn")
                                .cursor_pointer()
                                .w(px(26.0))
                                .h(px(26.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(RADIUS_MD)
                                .stateful_behavior(HoverBehavior {
                                    hover_bg: rgb(t.bg_hover).into(),
                                    ..Default::default()
                                })
                                .tooltip(move |_, cx| {
                                    let tip = prev_tip.clone();
                                    cx.new(|_| Tooltip::new(tip)).into()
                                })
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.search_prev(cx);
                                }))
                                .child(
                                    AppIcon::ChevronUp
                                        .size(ICON_MD)
                                        .text_color(p.text_secondary),
                                )
                        })
                        .child({
                            let next_tip = next_tip.clone();
                            div()
                                .id("ai-search-next-btn")
                                .cursor_pointer()
                                .w(px(26.0))
                                .h(px(26.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(RADIUS_MD)
                                .stateful_behavior(HoverBehavior {
                                    hover_bg: rgb(t.bg_hover).into(),
                                    ..Default::default()
                                })
                                .tooltip(move |_, cx| {
                                    let tip = next_tip.clone();
                                    cx.new(|_| Tooltip::new(tip)).into()
                                })
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.search_next(cx);
                                }))
                                .child(
                                    AppIcon::ChevronDown
                                        .size(ICON_MD)
                                        .text_color(p.text_secondary),
                                )
                        })
                        .child({
                            let close_tip = close_tip.clone();
                            div()
                                .id("ai-search-close-btn")
                                .flex_shrink_0()
                                .cursor_pointer()
                                .w(px(26.0))
                                .h(px(26.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(RADIUS_MD)
                                .stateful_behavior(HoverBehavior {
                                    hover_bg: rgba(0xf14c4c44).into(),
                                    ..Default::default()
                                })
                                .tooltip(move |_, cx| {
                                    let tip = close_tip.clone();
                                    cx.new(|_| Tooltip::new(tip)).into()
                                })
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.ai_search_open = false;
                                    cx.notify();
                                }))
                                .child(AppIcon::Close.size(ICON_MD).text_color(p.text_secondary))
                        }),
                )
            })
            .when_some(self.ai_context_menu.clone(), |d, menu| {
                d.child(menu)
            })
            .into_any()
    }
}

impl AiAssistantPanel {
    /// Check whether AI functionality is enabled and at least one model is configured and enabled.
    pub fn is_ai_configured(&self, cx: &App) -> bool {
        let s = settings_entity(cx).read(cx).settings.clone();
        s.ai_enabled && s.ai_models.iter().any(|m| m.enabled)
    }
}

impl Focusable for AiAssistantPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.is_ai_configured(cx) {
            if let Some(ref input) = self.chat_input {
                return input.read(cx).focus_handle(cx);
            }
        }
        self.focus_handle.clone()
    }
}

impl Panel for AiAssistantPanel {
    fn metadata(&self, cx: &App) -> PanelInfo {
        PanelInfo::new(
            "ai_assistant",
            i18n!(cx, "ai_assistant.title"),
            AppIcon::AiAssistant,
            velowork_ui::dock::types::PanelKind::Custom,
        )
    }

    fn custom_actions(&self, _cx: &App) -> Vec<PanelAction> {
        Vec::new()
    }

    fn on_open(&mut self, cx: &mut Context<Self>) {
        self.check_and_switch_project(cx);
    }

    fn on_show(&mut self, cx: &mut Context<Self>) {
        self.check_and_switch_project(cx);
    }

    fn on_tab_changed(&mut self, cx: &mut Context<Self>) {
        self.check_and_switch_project(cx);
    }

    fn focus_handle(&self, cx: &App) -> Option<FocusHandle> {
        if self.is_ai_configured(cx) {
            if let Some(ref input) = self.chat_input {
                return Some(input.read(cx).focus_handle(cx));
            }
        }
        Some(self.focus_handle.clone())
    }
}

impl Render for AiAssistantPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.check_and_switch_project(cx);
        if self.is_ai_configured(cx) && self.focus_handle.is_focused(window) {
            if let Some(ref chat_input) = self.chat_input {
                chat_input.update(cx, |inp, cx| inp.focus(window, cx));
            }
        }
        self.render_ai_assistant(window, cx)
    }
}

fn find_match_ranges(
    text: &str,
    query: &str,
    case_sensitive: bool,
    use_regex: bool,
) -> Vec<std::ops::Range<usize>> {
    if use_regex {
        if let Ok(re) = regex::Regex::new(query) {
            re.find_iter(text).map(|m| m.range()).collect()
        } else {
            Vec::new()
        }
    } else {
        let text_lower = text.to_lowercase();
        let query_lower = query.to_lowercase();
        let text = if case_sensitive {
            text
        } else {
            text_lower.as_str()
        };
        let query = if case_sensitive {
            query
        } else {
            query_lower.as_str()
        };
        let mut ranges = Vec::new();
        let mut start = 0;
        while start < text.len() {
            match text[start..].find(query) {
                Some(rel) => {
                    let idx = start + rel;
                    ranges.push(idx..idx + query.len());
                    start = idx + query.len();
                }
                None => break,
            }
        }
        ranges
    }
}
fn render_ai_message(
    msg: &ChatMessage,
    t: &ThemeColors,
    window: &mut Window,
    cx: &mut App,
    focus_manager: &Entity<FocusManager>,
    workspace: &Entity<Workspace>,
    terminals: &TerminalsRegistry,
    search_query: &str,
    case_sensitive: bool,
    use_regex: bool,
    msg_index: usize,
    mut flat_idx: usize,
    current_match: Option<usize>,
    expanded_quotes: &[usize],
    expanded_tools: &[usize],
    attached_tool_calls: &[ToolCallCardData],
    panel_entity: &Entity<AiAssistantPanel>,
    ai_editing_index: Option<usize>,
    ai_edit_input: Option<Entity<InputState>>,
    copied: bool,
    // 当前动画帧，用于加载动画与头像呼吸效果。
    frame: u64,
    // 本条 AI 消息已显示的文本行数（逐行显示动画）。
    revealed_lines: usize,
    // 已显示行数本帧是否增加（决定是否重写 markdown 视图内容）。
    reveal_changed: bool,
) -> (AnyElement, usize) {
    let is_user = msg.is_user;

    // Whole-message search matches, used for the bubble-border highlight.
    // `flat_idx` is the running count of matches in earlier messages, so the
    // current-match index stays consistent across the whole conversation.
    let matches = if !search_query.is_empty() {
        find_match_ranges(&msg.text, search_query, case_sensitive, use_regex)
    } else {
        Vec::new()
    };
    let is_highlighted = matches
        .iter()
        .enumerate()
        .any(|(i, _)| current_match == Some(flat_idx + i));

    let mut out: Vec<AnyElement> = Vec::new();

    if is_user {
        // 渲染随此条消息提交的文件 / 图片附件芯片。
        if !msg.attachments.is_empty() {
            let chips = msg
                .attachments
                .iter()
                .map(|att| {
                    let name = att.name.clone();
                    let is_image = att.is_image;
                    let icon_path = if is_image {
                        AppIcon::File
                    } else {
                        AppIcon::File
                    };
                    h_flex()
                        .id(ElementId::Name(format!("msg-att-chip-{}", name).into()))
                        .items_center()
                        .gap(SPACE_XS)
                        .px(px(6.0))
                        .py(px(2.0))
                        .rounded(px(4.0))
                        .bg(surface_bg(t.bg_hover, cx))
                        .border_1()
                        .border_color(rgb(t.border))
                        .child(icon_path.size(px(11.0)).text_color(rgb(t.text_muted)))
                        .child({
                            let n = name.clone();
                            div()
                                .text_size(ui_text_xs(cx))
                                .text_color(rgb(t.text_secondary))
                                .child(n)
                                .into_any_element()
                        })
                        .tooltip(move |_, cx| {
                            let __tip = name.clone();
                            cx.new(|_| Tooltip::new(__tip)).into()
                        })
                })
                .collect::<Vec<_>>();
            out.push(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(SPACE_XS)
                    .mb(SPACE_XS)
                    .children(chips)
                    .into_any_element(),
            );
        }
        // 终端「AI 解读」引用：聊天界面中默认折叠（单行截断），点击可展开查看完整内容。
        if let Some(ref q) = msg.quote {
            if !q.trim().is_empty() {
                let expanded = expanded_quotes.contains(&msg_index);
                let entity = panel_entity.clone();
                let quote_text = q.clone();
                let expand_label = if expanded {
                    i18n!(cx, "ai_assistant.quote_collapse")
                } else {
                    i18n!(cx, "ai_assistant.quote_expand")
                };
                out.push(
                    div()
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(with_alpha(t.text_muted, 0.3))
                        .bg(with_alpha(t.bg_secondary, 0.4))
                        .overflow_hidden()
                        .child(
                            div()
                                .id(("ai-quote-card", msg_index))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(SPACE_SM)
                                .px(px(10.0))
                                .py(SPACE_SM)
                                .cursor_pointer()
                                .on_click({
                                    let entity = entity.clone();
                                    move |_ev, _window, cx| {
                                        entity.update(cx, |this, cx| {
                                            let mut v = this.ai_expanded_quotes.borrow_mut();
                                            if let Some(pos) =
                                                v.iter().position(|x| *x == msg_index)
                                            {
                                                v.remove(pos);
                                            } else {
                                                v.push(msg_index);
                                            }
                                            cx.notify();
                                        });
                                    }
                                })
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .text_size(ui_text_md(cx))
                                        .text_color(rgb(t.text_muted))
                                        .when(!expanded, |d| {
                                            d.truncate().whitespace_nowrap().child(
                                                quote_text.lines().collect::<Vec<_>>().join(" "),
                                            )
                                        })
                                        .when(expanded, |d| d.child(quote_text.clone())),
                                )
                                .child(
                                    div()
                                        .text_size(ui_text_xs(cx))
                                        .text_color(rgb(t.text_muted))
                                        .child(expand_label),
                                ),
                        )
                        .into_any_element(),
                );
            }
        }
        let lines: Vec<&str> = msg.text.split('\n').collect();
        let mut buf: Vec<String> = Vec::new();
        let mut code: Vec<String> = Vec::new();
        let mut in_code = false;
        let mut code_lang = String::new();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            if line.trim_start().starts_with("```") {
                if in_code {
                    out.push(code_block_with_actions(
                        &code.join("\n"),
                        &code_lang,
                        t,
                        cx,
                        focus_manager,
                        workspace,
                        terminals,
                    ));
                    code.clear();
                    code_lang.clear();
                    in_code = false;
                } else {
                    if !buf.is_empty() {
                        let (el, cnt) = text_bubble(
                            &buf.join("\n"),
                            is_user,
                            t,
                            window,
                            cx,
                            search_query,
                            case_sensitive,
                            use_regex,
                            msg_index,
                            flat_idx,
                            current_match,
                            panel_entity,
                        );
                        flat_idx += cnt;
                        out.push(el);
                        buf.clear();
                    }
                    in_code = true;
                    code_lang = line
                        .trim_start()
                        .trim_start_matches("```")
                        .trim()
                        .to_string();
                }
            } else if in_code {
                code.push(line.to_string());
            } else {
                buf.push(line.to_string());
            }
            i += 1;
        }
        if in_code {
            if !code.is_empty() {
                out.push(code_block_with_actions(
                    &code.join("\n"),
                    &code_lang,
                    t,
                    cx,
                    focus_manager,
                    workspace,
                    terminals,
                ));
            }
        } else if !buf.is_empty() {
            let (el, _) = text_bubble(
                &buf.join("\n"),
                is_user,
                t,
                window,
                cx,
                search_query,
                case_sensitive,
                use_regex,
                msg_index,
                flat_idx,
                current_match,
                panel_entity,
            );
            out.push(el);
        }
    } else {
        // AI (bot) reply.
        let panel_entity_clone = panel_entity.clone();
        let text_clone = msg.text.clone();

        // 收集气泡内子元素：纯文本按 markdown 渲染，工具调用渲染为专属卡片，
        // 二者在气泡内清晰分离且视觉统一。
        let mut children: Vec<AnyElement> = Vec::new();

        // 加载状态：流式中且无内容时显示 spinner，避免空白消息框。
        let is_loading = msg.streaming && msg.text.is_empty() && msg.tool_call.is_none();

        // 思考过程：Agent 工具调用前的推理内容，可折叠展示。
        if let Some(ref thinking) = msg.thinking {
            if !thinking.trim().is_empty() {
                children.push(thinking_block(thinking, t, cx).into_any_element());
            }
        }

        if is_loading {
            children.push(loading_indicator(t, cx, frame).into_any_element());
        } else {
            // 1. 如果有结构化工具调用（且 name 非空），渲染工具卡片
            if let Some(tc) = &msg.tool_call {
                if !tc.name.is_empty() {
                    children.push(tool_call_card(
                        tc,
                        t,
                        cx,
                        focus_manager,
                        workspace,
                        terminals,
                    ));
                }
            }

            // 2. 渲染正文文本 (正文中可能解析出 XML 工具调用，split_tool_calls 处理并将其与纯文本分离)
            if !msg.text.is_empty() {
                let mut text_seg_idx = 0usize;
                let mut text_line_offset = 0usize;
                for seg in split_tool_calls(&msg.text) {
                    match seg {
                        ToolSegment::Text(s) => {
                            if s.trim().is_empty() {
                                text_line_offset += s.split('\n').count();
                                continue;
                            }
                            let seg_lines = s.split('\n').count();
                            // 逐行显示：仅展示已揭示行，其余暂隐藏，确保每行完整出现后再显示下一行。
                            let reveal_in_seg = revealed_lines
                                .saturating_sub(text_line_offset)
                                .min(seg_lines);
                            if reveal_in_seg > 0 {
                                let revealed_text: String = s
                                    .split('\n')
                                    .take(reveal_in_seg)
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                let views = msg.document_views.borrow();
                                if let Some(doc_view) = views.get(text_seg_idx) {
                                    // 非流式历史消息或揭示行数变化时更新 markdown 内容
                                    if !msg.streaming || reveal_changed {
                                        doc_view.update(cx, |v, cx| {
                                            v.set_content(&revealed_text, cx);
                                        });
                                    }
                                    children.push(doc_view.clone().into_any_element());
                                } else {
                                    let fallback_el = div()
                                        .text_size(ui_text_md(cx))
                                        .text_color(rgb(t.text_primary))
                                        .child(revealed_text.clone());
                                    children.push(fallback_el.into_any_element());
                                }
                            }
                            text_line_offset += seg_lines;
                            text_seg_idx += 1;
                        }
                        ToolSegment::Tool { kind, name, body } => {
                            let params = parse_tool_body(&body);
                            let card = ToolCallCardData {
                                kind,
                                name,
                                body,
                                params,
                            };
                            children.push(tool_call_card(
                                &card,
                                t,
                                cx,
                                focus_manager,
                                workspace,
                                terminals,
                            ));
                        }
                    }
                }
            }
        }

        if children.is_empty() && !msg.text.is_empty() {
            children.push(
                div()
                    .text_size(ui_text_md(cx))
                    .text_color(rgb(t.text_primary))
                    .child(msg.text.clone())
                    .into_any_element(),
            );
        }

        // 归并到本气泡的多次工具调用：默认折叠显示摘要，点击展开查看每个详情卡片。
        if !attached_tool_calls.is_empty() {
            let expanded = expanded_tools.contains(&msg_index);
            let count = attached_tool_calls.len();
            let names: Vec<String> = attached_tool_calls
                .iter()
                .map(|tc| {
                    if tc.kind == ToolCallKind::Result {
                        i18n!(cx, "ai_assistant.tool_result").to_string()
                    } else if tc.name.is_empty() {
                        i18n!(cx, "ai_assistant.tool_use").to_string()
                    } else {
                        tc.name.clone()
                    }
                })
                .collect();
            let tool_entity = panel_entity.clone();
            let toggle_label = if expanded {
                i18n!(cx, "ai_assistant.tool_calls_expanded")
            } else {
                i18n!(cx, "ai_assistant.tool_calls_collapsed").replace("{count}", &count.to_string())
            };
            let mut group = div()
                .mt(SPACE_SM)
                .rounded(RADIUS_LG)
                .border_1()
                .border_color(rgb(t.border))
                .bg(with_alpha(t.bg_secondary, 0.4))
                .overflow_hidden()
                .child(
                    div()
                        .id(("ai-tool-group", msg_index))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(SPACE_SM)
                        .px(px(10.0))
                        .py(SPACE_SM)
                        .cursor_pointer()
                        .on_click({
                            let tool_entity = tool_entity.clone();
                            move |_ev, _window, cx| {
                                tool_entity.update(cx, |this, cx| {
                                    let mut v = this.ai_expanded_tools.borrow_mut();
                                    if let Some(pos) = v.iter().position(|x| *x == msg_index) {
                                        v.remove(pos);
                                    } else {
                                        v.push(msg_index);
                                    }
                                    cx.notify();
                                });
                            }
                        })
                        .child(
                            h_flex()
                                .items_center()
                                .gap(SPACE_SM)
                                .child(
                                    if expanded {
                                        AppIcon::ChevronDown
                                    } else {
                                        AppIcon::ChevronRight
                                    }
                                    .size(px(13.0))
                                    .flex_shrink_0()
                                    .text_color(rgb(t.text_muted)),
                                )
                                .child(
                                    div()
                                        .text_size(ui_text_md(cx))
                                        .text_color(rgb(t.text_primary))
                                        .child(toggle_label),
                                ),
                        )
                        .child(
                            div()
                                .text_size(ui_text_md(cx))
                                .text_color(rgb(t.text_muted))
                                .child(names.join("、 ")),
                        ),
                );
            if expanded {
                group = group.child(
                    v_flex()
                        .gap(SPACE_SM)
                        .px(px(10.0))
                        .pb(SPACE_MD)
                        .children(
                            attached_tool_calls
                                .iter()
                                .map(|tc| tool_call_card(tc, t, cx, focus_manager, workspace, terminals)),
                        ),
                );
            }
            children.push(group.into_any_element());
        }

        let bubble = div()
            .relative()
            .max_w(relative(1.0))
            .min_w(px(20.0))
            .px(SPACE_LG)
            .py(SPACE_MD)
            .rounded(RADIUS_LG)
            .bg(with_alpha(t.bg_secondary, 0.4))
            .border_1()
            .border_color(rgb(t.border))
            .when(is_highlighted, |s| s.border_color(rgb(t.border_active)))
            .child(v_flex().gap(SPACE_XS).children(children))
            .on_mouse_down(
                MouseButton::Right,
                move |event: &MouseDownEvent, window, cx| {
                    let panel = panel_entity_clone.clone();
                    let selection = panel.read(cx).get_active_selection_text(cx);
                    let registry = panel.read(cx).overlay_manager.read(cx).overlay_registry();
                    let menu = open_ai_context_menu(
                        panel,
                        event.position,
                        selection,
                        text_clone.clone(),
                        Some(registry),
                        window,
                        cx,
                    );
                    panel_entity_clone.update(cx, |panel, cx| {
                        panel.ai_context_menu = Some(menu);
                        cx.notify();
                    });
                },
            );
        out.push(bubble.into_any_element());
    }

    let group_name = format!("ai-msg-{}", msg_index);
    let editing = ai_editing_index.map(|i| i == msg_index).unwrap_or(false);

    // Copy button (copies the whole message text, shows ✓ when done).
    let copy_btn = msg_copy_btn(
        msg_index,
        msg.text.clone(),
        panel_entity.clone(),
        copied,
        t,
        cx,
    );

    if is_user {
        if editing {
            if let Some(ref edit_input) = ai_edit_input {
                let panel_send = panel_entity.clone();
                let send_btn = button_primary(
                    format!("ai-edit-send-{}", msg_index),
                    i18n!(cx, "ai_assistant.send"),
                    &t,
                )
                .small()
                .icon_left(AppIcon::Send)
                .on_click(move |_ev, _window, cx| {
                    let _ = panel_send.update(cx, |panel, cx| panel.confirm_edit(cx));
                });
                let panel_cancel = panel_entity.clone();
                let cancel_btn = Button::new(format!("ai-edit-cancel-{}", msg_index), &t)
                    .default()
                    .small()
                    .label(i18n!(cx, "common.cancel"))
                    .on_click(move |_ev, _window, cx| {
                        let _ = panel_cancel.update(cx, |panel, cx| panel.cancel_edit(cx));
                    });

                return (
                    v_flex()
                        .w_full()
                        .items_end()
                        .gap(SPACE_SM)
                        .child(
                            div()
                                .relative()
                                .w_full()
                                .max_w(relative(0.8))
                                .key_context("AiEditInput")
                                .on_key_down({
                                    let panel_entity = panel_entity.clone();
                                    move |event: &KeyDownEvent, _window, cx| {
                                        if event.keystroke.key.as_str() == "enter"
                                            && !event.keystroke.modifiers.control
                                        {
                                            let _ = panel_entity.update(cx, |panel, cx| {
                                                panel.confirm_edit(cx);
                                            });
                                            cx.stop_propagation();
                                        }
                                    }
                                })
                                .child(
                                    div()
                                        .id("ai-edit-input-wrapper")
                                        .w_full()
                                        .min_h(px(60.0))
                                        .child(Input::new(edit_input)),
                                 ),
                        )
                        .child(
                            h_flex()
                                .justify_end()
                                .gap(SPACE_SM)
                                .child(copy_btn)
                                .child(send_btn)
                                .child(cancel_btn),
                        )
                        .into_any_element(),
                    matches.len(),
                );
            }
        }

        // Normal user message: right-aligned bubble, hover actions on the right.
        let edit_label = i18n!(cx, "common.edit");
        return (
            v_flex()
                .w_full()
                .gap(SPACE_SM)
                .group(group_name.clone())
                .child(
                    div()
                        .self_end()
                        .w_full()
                        .max_w(relative(0.8))
                        .mr(SPACE_MD)
                        .flex()
                        .flex_col()
                        .gap(px(10.0))
                        .children(out),
                )
                .child(
                    h_flex()
                        .justify_end()
                        .gap(SPACE_SM)
                        .opacity(0.0)
                        .group_hover(group_name.clone(), |s| s.opacity(1.0))
                        .child(copy_btn)
                        .child({
                            let panel_edit = panel_entity.clone();
                            Button::new(format!("ai-edit-{}", msg_index), &t)
                                .text()
                                .small()
                                .icon_left(AppIcon::Edit)
                                .tooltip(edit_label.clone())
                                .on_click(move |_ev, window, cx| {
                                    let _ = panel_edit.update(cx, |panel, cx| {
                                        panel.start_edit(msg_index, window, cx);
                                    });
                                })
                        }),
                )
                .into_any_element(),
            matches.len(),
        );
    } else {
        // AI reply: avatar + name header above the bubble, hover copy on the left.
        let name_label = i18n!(cx, "ai_assistant.title");
        return (
            v_flex()
                .w_full()
                .gap(SPACE_SM)
                .group(group_name.clone())
                .child(
                    h_flex()
                        .gap(SPACE_MD)
                        .items_center()
                        .child(ai_avatar(t, msg.streaming, frame))
                        .child(
                            div()
                                .text_size(ui_text_md(cx))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(t.text_secondary))
                                .child(name_label),
                        ),
                )
                .child(
                    div()
                        .max_w(relative(0.8))
                        .flex()
                        .flex_col()
                        .gap(px(10.0))
                        .children(out),
                )
                .child(
                    h_flex()
                        .justify_start()
                        .gap(SPACE_SM)
                        .opacity(0.0)
                        .group_hover(group_name.clone(), |s| s.opacity(1.0))
                        .child(copy_btn),
                )
                .into_any_element(),
            matches.len(),
        );
    }
}

/// A small circular avatar shown above AI assistant replies.
/// 当 AI 正在回复（streaming）时，头像以呼吸节奏轻微明暗起伏，直观传达「工作中」。
fn ai_avatar(t: &ThemeColors, streaming: bool, frame: u64) -> impl IntoElement {
    let opacity = if streaming {
        let wave = (frame as f32 / 30.0 * std::f32::consts::PI * 2.0).sin();
        0.55 + 0.45 * ((wave + 1.0) / 2.0)
    } else {
        1.0
    };
    div()
        .w(px(28.0))
        .h(px(28.0))
        .rounded(px(14.0))
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(t.accent))
        .opacity(opacity)
        .child(
            AppIcon::AiAssistant
                .size(px(18.0))
                .text_color(rgb(0xffffff)),
        )
}

// ── Loading Indicator ────────────────────────────────────────────────────

/// 加载状态指示器：三个错相位呼吸跳动的圆点 + 文案，直观表达「等待回复中」。
fn loading_indicator(t: &ThemeColors, cx: &App, frame: u64) -> impl IntoElement {
    let label = i18n!(cx, "ai_assistant.thinking");
    let dots = (0..3).map(|i| {
        // 每个圆点相位错开，形成波浪式呼吸效果。
        let phase = (frame + i * 10) % 30;
        let wave = (phase as f32 / 30.0 * std::f32::consts::PI * 2.0).sin();
        let opacity = 0.35 + 0.65 * ((wave + 1.0) / 2.0);
        div()
            .w(px(7.0))
            .h(px(7.0))
            .rounded(px(3.5))
            .bg(rgb(t.accent))
            .opacity(opacity)
    });
    div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .py(SPACE_SM)
        .child(h_flex().gap(px(5.0)).items_center().children(dots))
        .child(
            div()
                .text_size(ui_text_md(cx))
                .text_color(rgb(t.text_muted))
                .child(label),
        )
}

// ── Thinking Block ───────────────────────────────────────────────────────

/// 可折叠的思考过程展示区块。
fn thinking_block(content: &str, t: &ThemeColors, cx: &App) -> impl IntoElement {
    let header_label = i18n!(cx, "ai_assistant.thinking_process");
    div()
        .rounded(px(6.0))
        .border_1()
        .border_color(with_alpha(t.text_muted, 0.3))
        .bg(with_alpha(t.bg_secondary, 0.4))
        .overflow_hidden()
        .child(
            // 思考过程 header（点击可折叠，默认展开）
            div()
                .flex()
                .items_center()
                .gap(SPACE_SM)
                .px(px(10.0))
                .py(SPACE_SM)
                .cursor_pointer()
                .child(
                    div()
                        .text_size(ui_text_md(cx))
                        .text_color(rgb(t.text_muted))
                        .font_weight(FontWeight::MEDIUM)
                        .child(header_label),
                ),
        )
        .child(
            // 思考内容（默认折叠）
            div().px(px(10.0)).pb(SPACE_MD).child(
                div()
                    .text_size(ui_text_md(cx))
                    .text_color(with_alpha(t.text_secondary, 0.8))
                    .child(content.to_string()),
            ),
        )
}

/// A hover-revealed "copy" action button for a chat message. Copies the
/// provided text (the whole message) to the clipboard.
fn msg_copy_btn(
    msg_index: usize,
    copy_text: String,
    panel_entity: Entity<AiAssistantPanel>,
    copied: bool,
    t: &ThemeColors,
    cx: &App,
) -> impl IntoElement {
    let label = if copied {
        i18n!(cx, "ai_assistant.copy_done")
    } else {
        i18n!(cx, "common.copy")
    };
    let icon_path = if copied {
        AppIcon::Check
    } else {
        AppIcon::Copy
    };
    Button::new(format!("ai-copy-{}", msg_index), &t)
        .text()
        .small()
        .icon_left(icon_path)
        .tooltip(label.clone())
        .on_click({
            let entity = panel_entity.clone();
            move |_ev, _window, cx| {
                let _ = entity.update(cx, |this, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy_text.clone()));
                    let mut v = this.ai_copy_done_indices.borrow_mut();
                    if !v.contains(&msg_index) {
                        v.push(msg_index);
                    }
                    cx.notify();
                });
            }
        })
}

fn text_bubble(
    text: &str,
    is_user: bool,
    t: &ThemeColors,
    _window: &mut Window,
    cx: &mut App,
    search_query: &str,
    case_sensitive: bool,
    use_regex: bool,
    _msg_index: usize,
    flat_idx: usize,
    current_match: Option<usize>,
    panel_entity: &Entity<AiAssistantPanel>,
) -> (AnyElement, usize) {
    let bg_color = surface_bg(t.bg_hover, cx);
    let text_color = if is_user {
        rgb(0xffffff)
    } else {
        rgb(t.text_primary)
    };

    let matches = if !search_query.is_empty() {
        find_match_ranges(text, search_query, case_sensitive, use_regex)
    } else {
        Vec::new()
    };

    let is_highlighted = matches
        .iter()
        .enumerate()
        .any(|(i, _)| current_match == Some(flat_idx + i));

    let mut highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    for (i, range) in matches.iter().enumerate() {
        if !text.is_char_boundary(range.start) || !text.is_char_boundary(range.end) {
            continue;
        }
        let is_current = current_match == Some(flat_idx + i);
        highlights.push((
            range.clone(),
            HighlightStyle {
                background_color: Some(if is_current {
                    rgb(t.bg_selection).into()
                } else {
                    rgb(t.bg_hover).into()
                }),
                ..Default::default()
            },
        ));
    }

    let panel_entity_clone = panel_entity.clone();
    let text_clone = text.to_string();
    let bubble = div()
        .relative()
        .max_w(relative(1.0))
        .min_w(px(20.0))
        .px(SPACE_LG)
        .py(SPACE_MD)
        .rounded(RADIUS_LG)
        .bg(bg_color)
        .border_1()
        .border_color(rgb(t.border))
        .when(is_highlighted, |s| s.border_color(rgb(t.border_active)))
        .when(use_custom_markdown_font(cx), |s| {
            s.font_family(markdown_font_family(cx))
        })
        .child(
            div()
                .text_color(text_color)
                .text_size(ui_text_md(cx))
                .child(StyledText::new(text.to_string()).with_highlights(highlights)),
        )
        .on_mouse_down(MouseButton::Right, {
            let panel_entity_clone = panel_entity_clone.clone();
            let text_clone = text_clone.clone();
            move |event: &MouseDownEvent, window, cx| {
                let panel = panel_entity_clone.clone();
                let selection = panel.read(cx).get_active_selection_text(cx);
                let registry = panel.read(cx).overlay_manager.read(cx).overlay_registry();
                let menu = open_ai_context_menu(
                    panel,
                    event.position,
                    selection,
                    text_clone.clone(),
                    Some(registry),
                    window,
                    cx,
                );
                panel_entity_clone.update(cx, |panel, cx| {
                    panel.ai_context_menu = Some(menu);
                    cx.notify();
                });
            }
        });

    (bubble.into_any_element(), matches.len())
}

fn code_block_with_actions(
    code: &str,
    lang: &str,
    t: &ThemeColors,
    cx: &mut App,
    focus_manager: &Entity<FocusManager>,
    workspace: &Entity<Workspace>,
    terminals: &TerminalsRegistry,
) -> AnyElement {
    let code_clone = code.to_string();
    let code_clone2 = code_clone.clone();
    let fm = focus_manager.clone();
    let ws = workspace.clone();
    let terms = terminals.clone();

    div()
        .relative()
        .max_w(relative(1.0))
        .rounded(RADIUS_LG)
        .overflow_hidden()
        .border_1()
        .border_color(rgb(t.border))
        .child(
            div()
                .px(SPACE_MD)
                .py(SPACE_XS)
                .bg(rgb(t.bg_header))
                .border_b_1()
                .border_color(rgb(t.border))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(ui_text_md(cx))
                        .text_color(rgb(t.text_muted))
                        .child(lang.to_string()),
                )
                .child(
                    h_flex()
                        .gap(SPACE_XS)
                        .child(
                            Button::new("code-copy-btn", &t)
                                .text()
                                .small()
                                .label(i18n!(cx, "common.copy"))
                                .on_click(move |_ev, _window, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        code_clone.clone(),
                                    ));
                                }),
                        )
                        .child(
                            button_primary("code-send-btn", i18n!(cx, "ai_assistant.send"), &t)
                                .small()
                                .on_click(move |_ev, _window, cx| {
                                    send_command_to_focused_terminal(
                                        &fm,
                                        &ws,
                                        &terms,
                                        &code_clone2,
                                        cx,
                                    );
                                }),
                        ),
                ),
        )
        .child(
            div()
                .px(SPACE_MD)
                .py(SPACE_MD)
                .bg(rgb(t.bg_panel))
                .font_family(mono_font_family(cx))
                .text_size(ui_text_md(cx))
                .text_color(rgb(t.text_primary))
                .child(code.to_string()),
        )
        .into_any_element()
}

/// 只读代码块（仅复制，无「发送到终端」），用于工具结果与无命令的回退展示。
fn result_code_block(code: &str, lang: &str, t: &ThemeColors, cx: &mut App) -> AnyElement {
    let code_clone = code.to_string();
    div()
        .relative()
        .max_w(relative(1.0))
        .rounded(RADIUS_LG)
        .overflow_hidden()
        .border_1()
        .border_color(rgb(t.border))
        .child(
            div()
                .px(SPACE_MD)
                .py(SPACE_XS)
                .bg(rgb(t.bg_header))
                .border_b_1()
                .border_color(rgb(t.border))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(ui_text_md(cx))
                        .text_color(rgb(t.text_muted))
                        .child(lang.to_string()),
                )
                .child(
                    Button::new("code-copy-btn", &t)
                        .text()
                        .small()
                        .label(i18n!(cx, "common.copy"))
                        .on_click(move |_ev, _window, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                code_clone.clone(),
                            ));
                        }),
                ),
        )
        .child(
            div()
                .px(SPACE_MD)
                .py(SPACE_MD)
                .bg(rgb(t.bg_panel))
                .font_family(mono_font_family(cx))
                .text_size(ui_text_md(cx))
                .text_color(rgb(t.text_primary))
                .child(code.to_string()),
        )
        .into_any_element()
}

/// 清除纯文本片段中残留的 XML/HTML 格式工具调用标记标签。
fn clean_stray_tags(text: &str) -> String {
    static RE_STRIP: OnceLock<Regex> = OnceLock::new();
    let re = RE_STRIP.get_or_init(|| {
        Regex::new(r"(?s)</?tool_call>|</?tool_result>|</?function(?:_result|=([^>\s]+))?>")
            .unwrap()
    });
    re.replace_all(text, "").to_string()
}

/// 将助手消息正文拆分为纯文本片段与工具调用片段并清除多余残留的 XML/HTML 工具标签。
///
/// 兼容多种模型工具调用格式：
/// 1. JSON 包裹格式 — `<tool_call>{"name": "...", "arguments": {...}}</tool_call>`
/// 2. XML 函数标签格式 — `<function=NAME>...</function>` 或
///    `<tool_call><function=NAME>...</function></tool_call>`
/// 3. 结果标签 — `<tool_result>...</tool_result>` 和 `<function_result>...</function_result>`
fn split_tool_calls(text: &str) -> Vec<ToolSegment> {
    // 收集所有匹配块的 (起始, 结束, 种类, 名称, 体内容)
    let mut blocks: Vec<(usize, usize, ToolCallKind, String, String)> = Vec::new();

    // ── 第一步：扫描 <tool_call>...内文...</tool_call> 包裹块 ──
    let re_wrapped = wrapped_tool_call_regex();
    for cap in re_wrapped.captures_iter(text) {
        let Some(whole) = cap.get(0) else { continue };
        let Some(inner_match) = cap.get(1) else { continue };
        let inner = inner_match.as_str().trim().to_string();

        // 内部是 JSON 格式：{"name": "...", "arguments": {...}}
        if inner.starts_with('{') {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&inner) {
                let name = val
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let body = val
                    .get("arguments")
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                blocks.push((whole.start(), whole.end(), ToolCallKind::Use, name, body));
            } else {
                // JSON 解析失败 → 回退为整段文本
                blocks.push((
                    whole.start(),
                    whole.end(),
                    ToolCallKind::Use,
                    String::new(),
                    inner,
                ));
            }
        }
        // 内部是 XML 函数标签格式
        else if let Some(cap_fn) = inner_fn_regex().captures(&inner) {
            let is_result = cap_fn.get(1).is_none();
            let name = cap_fn
                .get(1)
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default();
            let body = cap_fn
                .get(2)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            let kind = if is_result {
                ToolCallKind::Result
            } else {
                ToolCallKind::Use
            };
            blocks.push((whole.start(), whole.end(), kind, name, body));
        }
    }

    // ── 第二步：扫描未被上方包裹的裸 <function=...> / <function_result> 标签 ──
    let re_naked = naked_fn_regex();
    for cap in re_naked.captures_iter(text) {
        let Some(whole) = cap.get(0) else { continue };
        // 跳过已经在上一步包裹块内的部分
        if blocks
            .iter()
            .any(|(s, e, ..)| *s <= whole.start() && whole.start() < *e)
        {
            continue;
        }
        let is_result = cap.get(1).is_none();
        let name = cap
            .get(1)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        let body = cap
            .get(2)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        let kind = if is_result {
            ToolCallKind::Result
        } else {
            ToolCallKind::Use
        };
        blocks.push((whole.start(), whole.end(), kind, name, body));
    }

    // ── 第三步：扫描 <tool_result>...</tool_result> 标签 ──
    let re_tool_result = tool_result_regex();
    for cap in re_tool_result.captures_iter(text) {
        let Some(whole) = cap.get(0) else { continue };
        let Some(body_match) = cap.get(1) else { continue };
        // 跳过已经匹配过的区域
        if blocks
            .iter()
            .any(|(s, e, ..)| *s <= whole.start() && whole.start() < *e)
        {
            continue;
        }
        let body = body_match.as_str().to_string();
        blocks.push((
            whole.start(),
            whole.end(),
            ToolCallKind::Result,
            String::new(),
            body,
        ));
    }

    // ── 按出现位置排序，组装为 ToolSegment 序列 ──
    blocks.sort_by_key(|m| m.0);

    let mut segs: Vec<ToolSegment> = Vec::new();
    let mut last = 0usize;
    for (start, end, kind, name, body) in blocks {
        if start > last {
            let plain = clean_stray_tags(&text[last..start]);
            if !plain.trim().is_empty() {
                segs.push(ToolSegment::Text(plain));
            }
        }
        segs.push(ToolSegment::Tool { kind, name, body });
        last = end;
    }
    if last < text.len() {
        let plain = clean_stray_tags(&text[last..]);
        if !plain.trim().is_empty() {
            segs.push(ToolSegment::Text(plain));
        }
    }
    if segs.is_empty() {
        let plain = clean_stray_tags(text);
        if !plain.trim().is_empty() {
            segs.push(ToolSegment::Text(plain));
        }
    }
    segs
}

/// 匹配 `<tool_call>任意内文</tool_call>`，提取内文。
fn wrapped_tool_call_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)<tool_call>\s*(.*?)\s*</tool_call>").unwrap())
}

/// 在内文中匹配 `<function=NAME>...</function>` 或 `<function_result>...</function_result>`。
fn inner_fn_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?s)<function(?:_result|=([^>\s]+))>(.*?)</function(?:_result)?>").unwrap()
    })
}

/// 匹配未包裹的裸函数标签（与 `inner_fn_regex` 模式相同）。
fn naked_fn_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?s)<function(?:_result|=([^>\s]+))>(.*?)</function(?:_result)?>").unwrap()
    })
}

/// 匹配 `<tool_result>...</tool_result>` 标签。
fn tool_result_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)<tool_result>(.*?)</tool_result>").unwrap())
}

/// 解析工具调用体：优先 JSON 对象，其次 `<parameter=NAME>VALUE</parameter>`，
/// 否则整段作为原始内容返回。
fn parse_tool_body(body: &str) -> Vec<(String, String)> {
    let body = body.trim();
    if body.is_empty() {
        return Vec::new();
    }
    // 1) JSON 对象：平铺为一层 key/value。
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(obj) = v.as_object() {
            let mut params = Vec::new();
            for (k, val) in obj {
                let s = match val {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                params.push((k.clone(), s));
            }
            if !params.is_empty() {
                return params;
            }
        }
    }
    // 2) `<parameter=NAME>VALUE</parameter>` 标签。
    let pre = parameter_regex();
    let mut params = Vec::new();
    for cap in pre.captures_iter(body) {
        let name = cap
            .get(1)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        let val = cap
            .get(2)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        params.push((name, val));
    }
    if !params.is_empty() {
        return params;
    }
    // 3) 回退：整段作为未命名参数。
    vec![("".to_string(), body.to_string())]
}

fn parameter_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)<parameter=([^>]+)>(.*?)</parameter>").unwrap())
}

/// 渲染一条工具调用 / 工具结果为气泡内的专属卡片。
///
/// - 工具调用（`Use`）：头部显示图标 + 「工具调用」+ 函数名；若参数含 `command`
///   则渲染为可执行的命令代码块（带复制 / 发送到终端），其余参数以列表展示。
/// - 工具结果（`Result`）：头部显示图标 + 「工具结果」，正文以只读代码块展示。
fn tool_call_card(
    data: &ToolCallCardData,
    t: &ThemeColors,
    cx: &mut App,
    focus_manager: &Entity<FocusManager>,
    workspace: &Entity<Workspace>,
    terminals: &TerminalsRegistry,
) -> AnyElement {
    let is_result = data.kind == ToolCallKind::Result;
    let header_label = if is_result {
        i18n!(cx, "ai_assistant.tool_result")
    } else {
        i18n!(cx, "ai_assistant.tool_use")
    };

    let header = h_flex()
        .items_center()
        .gap(SPACE_SM)
        .child(
            AppIcon::Terminal
                .size(px(13.0))
                .flex_shrink_0()
                .text_color(rgb(t.text_muted)),
        )
        .child(
            div()
                .text_size(ui_text_md(cx))
                .text_color(rgb(t.text_muted))
                .child(header_label),
        )
        .when(!is_result && !data.name.is_empty(), |s| {
            s.child(
                div()
                    .text_size(ui_text_md(cx))
                    .text_color(rgb(t.text_primary))
                    .child(data.name.clone()),
            )
        });

    let mut card_children: Vec<AnyElement> = Vec::new();
    card_children.push(header.into_any_element());

    if is_result {
        card_children.push(result_code_block(&data.body, "result", t, cx));
    } else {
        // 命令类工具：把 command 参数渲染为可执行的代码块。
        let command = data
            .params
            .iter()
            .find(|(k, _)| k == "command")
            .map(|(_, v)| v.clone());
        if let Some(cmd) = &command {
            if !cmd.trim().is_empty() {
                card_children.push(code_block_with_actions(
                    cmd,
                    "bash",
                    t,
                    cx,
                    focus_manager,
                    workspace,
                    terminals,
                ));
            }
        }
        // 其余参数（排除已展示的 command）。
        let extra: Vec<&(String, String)> =
            data.params.iter().filter(|(k, _)| k != "command").collect();
        if !extra.is_empty() {
            let params_label = i18n!(cx, "ai_assistant.tool_params");
            let mut list = v_flex().gap(px(2.0)).child(
                div()
                    .text_size(ui_text_md(cx))
                    .text_color(rgb(t.text_muted))
                    .child(params_label),
            );
            for (k, v) in extra {
                list = list.child(
                    div()
                        .text_size(ui_text_md(cx))
                        .text_color(rgb(t.text_primary))
                        .child(format!("{}: {}", k, v)),
                );
            }
            card_children.push(list.into_any_element());
        } else if command.is_none() {
            // 无结构化参数时回退展示原始 body（JSON / XML 等）。
            card_children.push(result_code_block(&data.body, "json", t, cx));
        }
    }

    div()
        .relative()
        .w_full()
        .rounded(RADIUS_LG)
        .overflow_hidden()
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg_panel))
        .child(
            v_flex()
                .gap(SPACE_SM)
                .px(px(10.0))
                .py(SPACE_MD)
                .children(card_children),
        )
        .into_any_element()
}

/// Register AI Assistant panel to the right toolbar.
pub fn register_toolbar_panel(registry: &mut velowork_ui::dock::RightToolbarRegistry) {
    registry.register(velowork_ui::dock::ToolbarPanelSpec {
        id: "ai_assistant".to_string(),
        icon: velowork_ui::icon::AppIcon::AiAssistant,
        title_key: "dock.panel.ai_assistant".to_string(),
        order: 10,
        is_visible: std::sync::Arc::new(|cx| {
            crate::settings::settings_entity(cx).read(cx).settings.ai_enabled
        }),
        factory: std::sync::Arc::new(|ctx, _window, cx| {
            let app_ctx = ctx
                .downcast_ref::<super::AppPanelCreationContext>()
                .expect("AppPanelCreationContext required");
            let workspace = app_ctx.workspace.clone();
            let focus_manager = app_ctx.focus_manager.clone();
            let terminals = app_ctx.terminals.clone();
            let overlay_manager = app_ctx.overlay_manager.clone();
            let p = cx.new(|cx| {
                AiAssistantPanel::new(workspace, focus_manager, terminals, overlay_manager, cx)
            });
            velowork_ui::dock::AnyPanel::new(p)
        }),
    });
}

