use crate::ai_runtime::AppAIRuntime;
use crate::settings::settings_entity;
use crate::views::overlays::overlay_manager::OverlayManager;
use gpui::prelude::*;
use gpui::*;
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
use velowork_ui::dropdown::{
    dropdown_anchored_above, dropdown_overlay,
};
use velowork_ui::icon::AppIcon;
use velowork_ui::menu::PopupMenu;
use crate::views::overlays::menus::ai_context_menu::open_ai_context_menu;
use velowork_ui::confirm_dialog::{ConfirmDialog, ConfirmDialogEvent};
use crate::views::overlays::dialogs::{AttachmentPreviewDialog, AttachmentPreviewDialogEvent};
use velowork_ui::design::appearance::{ControlSize, ControlVariant};
use velowork_ui::input::{focus_ring_shadows, Input, InputState, KeyInterceptResult};
use velowork_ui::overlay_registry::{ClosePolicy, OverlayInfo};
use velowork_ui::scrollable::{Scrollbar, ScrollbarAxis, ScrollbarShow};
use velowork_ui::select::{Select, SelectEvent, SelectOption, SelectPlacement, SelectState, SelectWidthMode};
use velowork_ui::simple_input::{InputEvent, SimpleInput, SimpleInputState};
use velowork_ui::theme::{ThemeColors, surface_bg, theme, with_alpha};
use velowork_ui::tokens::{
    elevation_menu_shadow, ui_space_sm, ui_space_xs,
    ICON_MD, ICON_MICRO, ICON_SM, ICON_STD, RADIUS_LG, RADIUS_MD, RADIUS_SM, RADIUS_STD, RADIUS_XS,
    SPACE_2XS, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XL, SPACE_XS,
    markdown_font_family, mono_font_family, ui_font_family, ui_text_md, ui_text_sm,
    ui_text_xs, use_custom_markdown_font, use_custom_ui_font,
};
use velowork_ui::tooltip::Tooltip;
use velowork_ui::{Button, ProgressRing, button_primary, format_token_count, h_flex, v_flex};
use velowork_workspace::focus::FocusManager;
use velowork_workspace::repositories::AiConversationRow;
use velowork_workspace::settings::AiModelConfig;
use velowork_workspace::state::Workspace;
use velowork_workspace::toast::{Toast, ToastManager};

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
    /// 图片专属：Base64 Data URL（如 "data:image/png;base64,..."）
    pub image_data_url: Option<String>,
    /// 是否已就绪（文本读取或图片 Base64 异步转码已完成，可安全提交发送）
    pub is_ready: bool,
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
                    image_data_url: None,
                    is_ready: true,
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
            active_conversation_id: None,
            messages: saved.messages.into_iter().map(ChatMessage::from_saved).collect(),
            ai_history: saved.ai_history,
            selected_model_id: saved.selected_model_id,
        }
    }
}

pub struct PagedLoadResult {
    pub messages: Vec<ChatMessage>,
    pub ai_history: Vec<(usize, String)>,
    pub selected_model_id: Option<String>,
    pub has_more: bool,
    pub oldest_rowid: Option<i64>,
    pub conv_id: String,
}

fn parse_message_row(m_row: velowork_workspace::repositories::AiMessageRow, repo: &velowork_workspace::repositories::AiRepository) -> ChatMessage {
    let mut quote = None;
    let mut thinking = None;
    let mut tool_call = None;
    let mut streaming = false;

    let mut attachments = Vec::new();
    if let Ok(att_rows) = repo.list_attachments(&m_row.id) {
        for att in att_rows {
            let p = std::path::PathBuf::from(&att.path);
            let is_img = is_image_path(&p);
            let text_content = None;
            attachments.push(ChatAttachment {
                name: p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                path: p,
                is_image: is_img,
                text_content,
                image_data_url: None,
                is_ready: true,
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

    ChatMessage {
        is_user: m_row.role == "user",
        text: m_row.content,
        streaming,
        document_views: std::cell::RefCell::new(Vec::new()),
        tool_call,
        thinking,
        attachments,
        quote,
    }
}

fn format_relative_time(iso_str: &str, cx: &App) -> String {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(iso_str) {
        let now = chrono::Utc::now();
        let duration = now.signed_duration_since(dt.with_timezone(&chrono::Utc));
        let secs = duration.num_seconds();
        if secs < 60 {
            i18n!(cx, "common.just_now")
        } else if secs < 3600 {
            format!("{}m", duration.num_minutes())
        } else if secs < 86400 {
            format!("{}h", duration.num_hours())
        } else if secs < 86400 * 7 {
            format!("{}d", duration.num_days())
        } else {
            dt.format("%m-%d").to_string()
        }
    } else {
        String::new()
    }
}

fn load_project_session_paged(
    pid: &str,
    target_conv_id: Option<&str>,
    before_rowid: Option<i64>,
    limit: usize,
) -> Option<PagedLoadResult> {
    let db = velowork_core::storage::database()?;
    let repo = velowork_workspace::repositories::AiRepository::new(db);

    let convs = if pid == "default" {
        repo.list_conversations(None).ok()?
    } else {
        repo.list_conversations(Some(pid)).ok()?
    };

    let conv = if let Some(target_id) = target_conv_id {
        convs.into_iter().find(|c| c.id == target_id)?
    } else {
        convs.into_iter().find(|c| {
            let c_pid = c.project_id.as_deref().unwrap_or("default");
            c_pid == pid
        })?
    };

    let (msgs_rows, oldest_rowid, has_more) = repo.list_messages_paged(&conv.id, before_rowid, limit).ok()?;
    log::debug!(
        "[AI DB Paged] Conversation {} for project {}, loaded: {}, has_more: {}, oldest_rowid: {:?}",
        conv.id, pid, msgs_rows.len(), has_more, oldest_rowid
    );

    let mut messages = Vec::new();
    let mut ai_history = Vec::new();
    for (idx, m_row) in msgs_rows.into_iter().enumerate() {
        if m_row.role == "user" {
            ai_history.push((idx, m_row.content.clone()));
        }
        messages.push(parse_message_row(m_row, &repo));
    }

    Some(PagedLoadResult {
        messages,
        ai_history,
        selected_model_id: conv.model,
        has_more,
        oldest_rowid,
        conv_id: conv.id,
    })
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
        messages.push(parse_message_row(m_row, &repo));
    }

    if messages.is_empty() {
        return None;
    }

    Some(ProjectChatSession {
        active_conversation_id: Some(conv.id),
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
            let mut images = Vec::new();
            for a in &m.attachments {
                if a.is_image {
                    if let Some(url) = &a.image_data_url {
                        images.push(url.clone());
                    }
                }
            }
            let mut simple = velowork_ai::SimpleChatMessage::new(m.is_user, text).with_images(images);
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

/// 判断是否为支持传入 Vision API 的光栅图片格式（PNG, JPG, WEBP, GIF）。
/// 注意：SVG 属于矢量代码，不在此列（SVG 自动分流为文本代码内联，防止 Vision API 报错）。
fn is_vision_image_path(p: &std::path::Path) -> bool {
    const VISION_IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| VISION_IMAGE_EXTS.contains(&e.as_str()))
}

/// 根据扩展名推导标准图片 MIME 类型
fn image_mime_type(p: &std::path::Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => "image/png",
    }
}

/// 常见黑名单二进制文件格式，严禁作为文本附件添加。
fn is_binary_blacklisted(p: &std::path::Path) -> bool {
    const BLACKLIST: &[&str] = &[
        "exe", "dll", "so", "dylib", "bin", "iso", "zip", "tar", "gz", "7z", "rar",
        "pdf", "docx", "xlsx", "pptx", "dmg", "pkg", "deb", "rpm", "class", "pyc",
        "o", "a", "wasm",
    ];
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| BLACKLIST.contains(&e.as_str()))
}

/// 安全读取文本/代码/SVG类附件内容：
/// 仅读取体量适中（<= 500KB）且可成功解码为 UTF-8 的文件，其余返回明确错误描述。
fn read_text_safe(p: &std::path::Path) -> Result<String, &'static str> {
    if is_binary_blacklisted(p) {
        return Err("暂不支持二进制或压缩包文件格式");
    }
    let Ok(meta) = std::fs::metadata(p) else {
        return Err("无法读取文件元数据");
    };
    if meta.len() > velowork_ai::MAX_TEXT_ATTACHMENT_SIZE as u64 {
        return Err("文本文件超出 500KB 大小限制");
    }
    let Ok(content) = std::fs::read_to_string(p) else {
        return Err("文件内容不是有效的 UTF-8 文本");
    };
    if content.trim().is_empty() {
        return Err("文件内容为空");
    }
    Ok(content)
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

/// 将文本/代码附件拼装为供模型消费的上下文段落。
/// 图片附件通过多模态 Vision 接口（Base64 Data URL）向模型传输真实像素，不再生成无意义的本地磁盘路径。
fn attachment_context(attachments: &[ChatAttachment]) -> String {
    let text_attachments: Vec<_> = attachments
        .iter()
        .filter(|a| !a.is_image && a.text_content.is_some())
        .collect();

    if text_attachments.is_empty() {
        return String::new();
    }
    let mut s = String::from("\n\n[附件上下文 / Attachments]\n");
    for a in text_attachments {
        s.push_str(&format!("--- 文件: {} (文本 Text)\n", a.name));
        if let Some(c) = &a.text_content {
            s.push_str(c);
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
    ai_sessions_search_scope_select: Entity<SelectState<bool>>,
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
    /// 虚拟化消息列表滚动状态 (gpui::list)
    list_state: ListState,
    /// 是否正在初次加载历史会话（显示居中 loading）
    loading_history: bool,
    /// 向上滚动是否还有更早的历史可分页加载
    has_more_history: bool,
    /// 已加载消息中最早消息的 SQLite rowid（用于向上分页查询游标）
    oldest_rowid: Option<i64>,
    /// 是否正在向上加载更早的历史消息（防止并发重复触发）
    loading_older: bool,
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
    /// 当前活跃的会话 ID
    active_conversation_id: Option<String>,
    /// 历史会话下拉 Popover 浮层是否展开
    ai_sessions_popover_open: bool,
    /// 缓存的当前项目下的历史会话列表
    ai_conversation_list: Vec<AiConversationRow>,
    /// 正在内联重命名标题的会话 ID
    renaming_conversation_id: Option<String>,
    /// 内联重命名的输入框状态
    rename_input_state: Option<Entity<SimpleInputState>>,
    /// 历史会话搜索关键字
    ai_sessions_search_query: String,
    /// 历史会话搜索模式：false 为仅搜标题，true 为消息全文检索
    ai_sessions_search_content: bool,
    /// 历史会话搜索输入框
    ai_sessions_search_input: Option<Entity<InputState>>,
    /// 历史会话搜索命中结果列表（当有搜索关键词时生效）
    ai_sessions_search_results: Option<Vec<velowork_workspace::repositories::AiConversationSearchResult>>,
    /// 历史会话搜索是否正在后台异步查询
    ai_sessions_searching: bool,
    /// 历史会话搜索请求代数（防止乱序覆盖）
    ai_sessions_search_generation: u64,
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
    /// 当前 AI 助手消息中的选区状态（消息索引、分段索引、字符起止偏移、提取文本）。
    ai_active_selection: Option<AiChatSelection>,
    /// 选区拖拽中的锚点状态。
    ai_selection_dragging: Option<AiSelectionDrag>,
}

/// AI 助手聊天气泡中的文本选区状态。
#[derive(Clone, Debug)]
pub struct AiChatSelection {
    pub msg_index: usize,
    pub seg_index: usize,
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// AI 助手聊天气泡中的文本选区拖拽锚点。
#[derive(Clone, Debug)]
pub struct AiSelectionDrag {
    pub msg_index: usize,
    pub seg_index: usize,
    pub anchor: usize,
    pub plain_text: String,
}

#[derive(Clone)]
pub struct ProjectChatSession {
    pub active_conversation_id: Option<String>,
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

/// 输入区域默认高度（像素）。增加 2 行高度（+48px），确保添加引用内容时无需手动拖拽也能自如多行输入。
const AI_INPUT_AREA_DEFAULT_HEIGHT: f32 = 208.0;
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
        let count = self.messages.len();
        self.list_state.splice(count - 1..count - 1, 1);
        if self.messages.len() > MAX_MEMORY_MESSAGES {
            let overflow = self.messages.len() - MAX_MEMORY_MESSAGES;
            self.messages.drain(0..overflow);
            self.list_state.splice(0..overflow, 0);
        }
    }
    pub fn new(
        workspace: Entity<Workspace>,
        focus_manager: Entity<FocusManager>,
        terminals: TerminalsRegistry,
        overlay_manager: Entity<OverlayManager>,
        cx: &mut Context<Self>,
    ) -> Self {
        let initial_pid = focus_manager.read(cx).active_project_id().cloned();
        let key = initial_pid.as_deref().unwrap_or("default").to_string();

        let project_chat_sessions = std::collections::HashMap::new();
        let messages = vec![ChatMessage {
            is_user: false,
            text: i18n!(cx, "ai.welcome"),
            streaming: false,
            document_views: std::cell::RefCell::new(Vec::new()),
            tool_call: None,
            thinking: None,
            attachments: Vec::new(),
            quote: None,
        }];
        let ai_history = Vec::new();
        let restored_model_id = None;

        let ai_client = AiClient::new(focus_manager.clone(), workspace.clone(), terminals.clone());

        let reg = overlay_manager.read(cx).overlay_registry();

        let ai_model_select = cx.new(|cx| {
            let mut s = SelectState::new(cx)
                .placeholder(i18n!(cx, "ai.model"))
                .placement(SelectPlacement::Above)
                .ghost(true)
                .size(ControlSize::Compact)
                .text_size(ui_text_md(cx))
                .width_mode(SelectWidthMode::ContentAdaptive);
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
                .placement(SelectPlacement::Above)
                .ghost(true)
                .size(ControlSize::Compact)
                .text_size(ui_text_md(cx));
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

        let ai_sessions_search_scope_select = cx.new(|cx| {
            let mut s = SelectState::new(cx)
                .options(vec![
                    SelectOption::new(false, i18n!(cx, "ai.search_scope_title")),
                    SelectOption::new(true, i18n!(cx, "ai.search_scope_content")),
                ])
                .selected(Some(false))
                .placement(SelectPlacement::Below)
                .width_mode(SelectWidthMode::ContentAdaptive)
                .size(ControlSize::Default)
                .text_size(ui_text_md(cx));
            s.set_overlay_registry(reg.clone());
            s
        });

        cx.subscribe(
            &ai_sessions_search_scope_select,
            |this, _, event: &SelectEvent<bool>, cx| {
                if let SelectEvent::Change(Some(content)) = event {
                    this.ai_sessions_search_content = *content;
                    this.refresh_sessions_search(cx);
                }
            },
        )
        .detach();

        let chat_input = cx.new(|cx| {
            let mut state = InputState::new(cx)
                .multiline()
                .submit_on_enter(true)
                .wrap(true)
                .fill_height(true)
                .placeholder(i18n!(cx, "terminal.inline_ai_follow_up_placeholder"));

            state.set_key_interceptor(|event, _val, cx| {
                let key = event.keystroke.key.as_str();
                let cmd_or_ctrl = event.keystroke.modifiers.platform || event.keystroke.modifiers.control;
                if cmd_or_ctrl && key.eq_ignore_ascii_case("v") {
                    if let Some(item) = cx.read_from_clipboard() {
                        let has_image = item.entries().iter().any(|e| matches!(e, ClipboardEntry::Image(_)));
                        let has_files = item.entries().iter().any(|e| matches!(e, ClipboardEntry::ExternalPaths(p) if !p.paths().is_empty()));
                        if has_image || has_files {
                            return KeyInterceptResult::NotHandled;
                        }
                    }
                }
                KeyInterceptResult::Unhandled
            });

            state
        });
        let chat_input_clone = chat_input.clone();
        cx.subscribe(
            &chat_input_clone,
            |this: &mut Self, _, event: &velowork_ui::input::InputEvent, cx| {
                if *event == velowork_ui::input::InputEvent::PressEnter {
                    if !this.ai_quote_editing && this.has_input_content(cx) {
                        this.on_send_button(cx);
                    }
                    return;
                }
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
            ai_sessions_search_scope_select,
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
            list_state: {
                let ls = ListState::new(0, ListAlignment::Top, px(2048.0));
                ls.set_follow_mode(FollowMode::Tail);
                ls
            },
            loading_history: true,
            has_more_history: false,
            oldest_rowid: None,
            loading_older: false,
            ai_history_scroll: ScrollHandle::new(),
            ai_context_menu: None,
            ai_editing_index: None,
            ai_edit_input: None,
            animation_frame: 0,
            ai_copy_done_indices: std::cell::RefCell::new(Vec::new()),
            ai_reveal_start: std::cell::RefCell::new(Vec::new()),
            ai_reveal_revealed: std::cell::RefCell::new(Vec::new()),
            attachments: Vec::new(),

            ai_history_open: false,
            ai_input_area_bounds: None,
            ai_history,
            current_project_id: initial_pid,
            active_conversation_id: None,
            ai_sessions_popover_open: false,
            ai_conversation_list: Vec::new(),
            renaming_conversation_id: None,
            rename_input_state: None,
            ai_sessions_search_query: String::new(),
            ai_sessions_search_content: false,
            ai_sessions_search_input: None,
            ai_sessions_search_results: None,
            ai_sessions_searching: false,
            ai_sessions_search_generation: 0,
            project_chat_sessions,
            ai_quote: None,
            ai_quote_editing: false,
            ai_quote_input: None,
            ai_expanded_quotes: std::cell::RefCell::new(Vec::new()),
            ai_expanded_tools: std::cell::RefCell::new(Vec::new()),
            ai_scrollbar_hovered: std::cell::Cell::new(false),
            ai_input_area_height: AI_INPUT_AREA_DEFAULT_HEIGHT,
            ai_input_resize_dragging: None,
            ai_active_selection: None,
            ai_selection_dragging: None,
        };

        let panel_weak = cx.entity().downgrade();
        panel.list_state.set_scroll_handler(move |event, _window, cx| {
            if event.visible_range.start <= 2
                && let Some(panel) = panel_weak.upgrade()
            {
                panel.update(cx, |this, cx| {
                    this.load_more_history(cx);
                });
            }
        });


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
        // 启动动画帧循环（~60fps，16ms）：在「AI 流式回复中」「仍有逐行显示未完成」
        // 或「处于加载动画态」时持续以 60fps 驱动重绘；滚动跟随完全由 gpui::ListState 原生处理。
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            loop {
                smol::Timer::after(std::time::Duration::from_millis(16)).await;
                let should_continue = this.update(cx, |this, cx| {
                    this.animation_frame = this.animation_frame.wrapping_add(1);

                    let total_msgs = this.messages.len();
                    let frame = this.animation_frame;
                    let mut needs_notify = false;
                    {
                        let mut revealed = this.ai_reveal_revealed.borrow_mut();
                        while revealed.len() < total_msgs {
                            revealed.push(0);
                        }
                        for (mi, msg) in this.messages.iter().enumerate() {
                            if !msg.is_user {
                                let rev_lines = this.ai_revealed_lines(mi, msg, frame);
                                if revealed[mi] < rev_lines {
                                    revealed[mi] = rev_lines;
                                    this.list_state.remeasure_items(mi..mi + 1);
                                    needs_notify = true;
                                }
                            }
                        }
                    }

                    // 仅在需要动画时通知重渲染；空闲时静默，零 CPU 占用。
                    if needs_notify || this.animation_active() || this.loading_history || this.loading_older {
                        cx.notify();
                    }
                    true
                }).unwrap_or(false);

                if !should_continue {
                    break;
                }
            }
        })
        .detach();

        // 异步后台首屏分页加载当前项目的历史会话（最新 15 条），绝不阻塞 UI 主线程展开动效
        let load_key = key.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let lk_query = load_key.clone();
            let paged = smol::unblock(move || {
                load_project_session_paged(&lk_query, None, None, 15)
            }).await;

            let _ = this.update(cx, |this, cx| {
                let cur_key = this.current_project_id.as_deref().unwrap_or("default");
                if cur_key == load_key {
                    this.loading_history = false;
                    if let Some(paged) = paged {
                        this.active_conversation_id = Some(paged.conv_id.clone());
                        if !paged.messages.is_empty() {
                            let has_user_msg = this.messages.iter().any(|m| m.is_user);
                            if has_user_msg {
                                // 智能合并保障：用户已在后台加载期间先行发送了新消息，将历史会话拼接到当前新消息之前
                                let new_msgs: Vec<ChatMessage> = this
                                    .messages
                                    .drain(..)
                                    .filter(|m| m.is_user || m.streaming || m.thinking.is_some() || m.tool_call.is_some())
                                    .collect();
                                let mut merged_messages = paged.messages;
                                let offset_idx = merged_messages.len();
                                merged_messages.extend(new_msgs);
                                this.messages = merged_messages;

                                let mut merged_history = paged.ai_history;
                                for (idx, msg) in this.messages.iter().enumerate().skip(offset_idx) {
                                    if msg.is_user {
                                        merged_history.push((idx, msg.text.clone()));
                                    }
                                }
                                this.ai_history = merged_history;
                            } else {
                                this.messages = paged.messages;
                                this.ai_history = paged.ai_history;
                            }
                            this.has_more_history = paged.has_more;
                            this.oldest_rowid = paged.oldest_rowid;
                        } else {
                            this.has_more_history = false;
                            this.oldest_rowid = None;
                        }
                        if let Some(m_id) = paged.selected_model_id {
                            this.ai_selected_model_id = Some(m_id.clone());
                            this.ai_model_select.update(cx, |s, cx| {
                                s.set_selected_value(Some(m_id), cx);
                            });
                        }
                    } else {
                        let new_id = format!("conv_{}_{}", load_key, chrono::Utc::now().timestamp_millis());
                        this.active_conversation_id = Some(new_id);
                        this.has_more_history = false;
                        this.oldest_rowid = None;
                    }
                    this.refresh_conversation_list();
                    this.list_state.reset(this.messages.len());
                    this.list_state.scroll_to_end();
                    this.update_all_message_input_states(cx);
                    cx.notify();
                }
            });
        })
        .detach();

        panel.list_state.reset(panel.messages.len());
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
                active_conversation_id: self.active_conversation_id.clone(),
                messages: std::mem::take(&mut self.messages),
                ai_history: std::mem::take(&mut self.ai_history),
                selected_model_id: self.ai_selected_model_id.clone(),
            };
            self.project_chat_sessions.insert(old_key, session);

            let new_key = active_pid.as_deref().unwrap_or("default").to_string();
            self.current_project_id = active_pid;
            self.ai_history_open = false;
            self.ai_sessions_popover_open = false;
            self.cancel_rename_conversation(cx);

            if let Some(sess) = self.project_chat_sessions.remove(&new_key) {
                self.active_conversation_id = sess.active_conversation_id;
                self.messages = sess.messages;
                self.ai_history = sess.ai_history;
                self.ai_selected_model_id = sess.selected_model_id.clone();
                if let Some(m_id) = sess.selected_model_id {
                    self.ai_model_select.update(cx, |s, cx| {
                        s.set_selected_value(Some(m_id), cx);
                    });
                }
                self.loading_history = false;
                self.list_state.reset(self.messages.len());
                self.list_state.scroll_to_end();
                self.update_all_message_input_states(cx);
                self.save_current_sessions_to_disk();
                self.refresh_conversation_list();
                cx.notify();
            } else {
                self.messages = vec![ChatMessage {
                    is_user: false,
                    text: i18n!(cx, "ai.welcome"),
                    streaming: false,
                    document_views: std::cell::RefCell::new(Vec::new()),
                    tool_call: None,
                    thinking: None,
                    attachments: Vec::new(),
                    quote: None,
                }];
                self.ai_history = Vec::new();
                self.loading_history = true;
                self.has_more_history = false;
                self.oldest_rowid = None;
                self.loading_older = false;
                self.list_state.reset(self.messages.len());
                self.update_all_message_input_states(cx);
                cx.notify();

                let load_key = new_key.clone();
                cx.spawn(async move |this: WeakEntity<Self>, cx| {
                    let lk_query = load_key.clone();
                    let paged = smol::unblock(move || {
                        load_project_session_paged(&lk_query, None, None, 15)
                    }).await;

                    let _ = this.update(cx, |this, cx| {
                        let cur_key = this.current_project_id.as_deref().unwrap_or("default");
                        if cur_key == load_key {
                            this.loading_history = false;
                            if let Some(paged) = paged {
                                this.active_conversation_id = Some(paged.conv_id.clone());
                                if !paged.messages.is_empty() {
                                    let has_user_msg = this.messages.iter().any(|m| m.is_user);
                                    if has_user_msg {
                                        let new_msgs: Vec<ChatMessage> = this
                                            .messages
                                            .drain(..)
                                            .filter(|m| m.is_user || m.streaming || m.thinking.is_some() || m.tool_call.is_some())
                                            .collect();
                                        let mut merged_messages = paged.messages;
                                        let offset_idx = merged_messages.len();
                                        merged_messages.extend(new_msgs);
                                        this.messages = merged_messages;

                                        let mut merged_history = paged.ai_history;
                                        for (idx, msg) in this.messages.iter().enumerate().skip(offset_idx) {
                                            if msg.is_user {
                                                merged_history.push((idx, msg.text.clone()));
                                            }
                                        }
                                        this.ai_history = merged_history;
                                    } else {
                                        this.messages = paged.messages;
                                        this.ai_history = paged.ai_history;
                                    }
                                    this.has_more_history = paged.has_more;
                                    this.oldest_rowid = paged.oldest_rowid;
                                } else {
                                    this.has_more_history = false;
                                    this.oldest_rowid = None;
                                }
                                if let Some(m_id) = paged.selected_model_id {
                                    this.ai_selected_model_id = Some(m_id.clone());
                                    this.ai_model_select.update(cx, |s, cx| {
                                        s.set_selected_value(Some(m_id), cx);
                                    });
                                }
                            } else {
                                let new_id = format!("conv_{}_{}", load_key, chrono::Utc::now().timestamp_millis());
                                this.active_conversation_id = Some(new_id);
                                this.has_more_history = false;
                                this.oldest_rowid = None;
                            }
                            this.refresh_conversation_list();
                            this.list_state.reset(this.messages.len());
                            this.list_state.scroll_to_end();
                            this.update_all_message_input_states(cx);
                            cx.notify();
                        }
                    });
                }).detach();
            }
        }
    }

    /// 云端数据恢复/同步后，从 SQLite 数据库异步重新加载所有 AI 对话会话
    pub fn reload_from_db(&mut self, cx: &mut Context<Self>) {
        let key = self
            .current_project_id
            .as_deref()
            .unwrap_or("default")
            .to_string();

        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let target_key = key.clone();
            let (loaded_sessions, cur_session) = smol::unblock(move || {
                let mut sessions = std::collections::HashMap::new();
                if let Some(db) = velowork_core::storage::database() {
                    let repo = velowork_workspace::repositories::AiRepository::new(db);
                    if let Ok(convs) = repo.list_conversations(None) {
                        for conv in convs {
                            let pid = conv.project_id.unwrap_or_else(|| "default".into());
                            if !sessions.contains_key(&pid) {
                                if let Some(sess) = load_single_project_session_from_db(&pid) {
                                    sessions.insert(pid, sess);
                                }
                            }
                        }
                    }
                }
                let cur = sessions.remove(&target_key).or_else(|| load_single_project_session_from_db(&target_key));
                (sessions, cur)
            }).await;

            let _ = this.update(cx, |this, cx| {
                this.project_chat_sessions = loaded_sessions;
                let cur_key = this.current_project_id.as_deref().unwrap_or("default");
                if cur_key == key {
                    if let Some(sess) = cur_session {
                        this.active_conversation_id = sess.active_conversation_id;
                        if !sess.messages.is_empty() {
                            this.messages = sess.messages;
                            this.ai_history = sess.ai_history;
                        }
                        if let Some(m_id) = sess.selected_model_id {
                            this.ai_selected_model_id = Some(m_id.clone());
                            this.ai_model_select.update(cx, |s, cx| {
                                s.set_selected_value(Some(m_id), cx);
                            });
                        }
                    } else {
                        this.messages = vec![ChatMessage {
                            is_user: false,
                            text: i18n!(cx, "ai.welcome"),
                            streaming: false,
                            document_views: std::cell::RefCell::new(Vec::new()),
                            tool_call: None,
                            thinking: None,
                            attachments: Vec::new(),
                            quote: None,
                        }];
                        this.ai_history = Vec::new();
                    }
                    this.refresh_conversation_list();
                    this.ai_history_open = false;
                    this.ai_sessions_popover_open = false;
                    this.list_state.reset(this.messages.len());
                    this.list_state.scroll_to_end();
                    this.update_all_message_input_states(cx);
                    cx.notify();
                }
            });
        }).detach();
    }

    pub fn save_current_sessions_to_disk(&self) {
        let active_pid = self
            .current_project_id
            .as_deref()
            .unwrap_or("default")
            .to_string();

        let current_session = ProjectChatSession {
            active_conversation_id: self.active_conversation_id.clone(),
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
                let conv_id = sess
                    .active_conversation_id
                    .clone()
                    .unwrap_or_else(|| format!("conv_{}", pid));

                if is_welcome(&sess) {
                    if let Ok(msgs) = repo.list_messages(&conv_id) {
                        if msgs.iter().any(|m| m.role == "user") {
                            log::info!("[AI Assistant] Skipping SQLite overwrite for conversation {} as DB has real messages", conv_id);
                            continue;
                        }
                    }
                }

                let existing = repo.get_conversation(&conv_id).ok().flatten();
                let existing_title = existing.as_ref().and_then(|c| c.title.clone());

                let auto_title = sess
                    .messages
                    .iter()
                    .find(|m| m.is_user)
                    .map(|m| {
                        let t = m.text.trim();
                        if t.chars().count() > 24 {
                            format!("{}...", t.chars().take(24).collect::<String>())
                        } else {
                            t.to_string()
                        }
                    });

                let title = existing_title.or(auto_title);
                let created_at = existing
                    .as_ref()
                    .map(|c| c.created_at.clone())
                    .unwrap_or_else(|| now_iso.clone());
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
        self.list_state.scroll_to_end();
    }

    fn scroll_to_message(&self, message_index: usize, _cx: &mut Context<Self>) {
        self.list_state.scroll_to_reveal_item(message_index);
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
            if m.is_user {
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

    fn update_message_input_states(&self, _msg_idx: usize, _cx: &mut Context<Self>) {}

    /// 向当前消息列表头部追加更早的历史消息（分页加载），并安全地重映射所有依赖消息索引的状态。
    fn prepend_older_messages(&mut self, older_messages: Vec<ChatMessage>) {
        let count = older_messages.len();
        if count == 0 {
            return;
        }

        // 1. 插入消息
        let mut new_messages = older_messages;
        new_messages.extend(std::mem::take(&mut self.messages));
        self.messages = new_messages;

        // 2. 偏移所有基于索引的状态
        if let Some(idx) = self.ai_streaming_index.as_mut() {
            *idx += count;
        }
        if let Some(idx) = self.ai_editing_index.as_mut() {
            *idx += count;
        }
        if let Some(sel) = self.ai_active_selection.as_mut() {
            sel.msg_index += count;
        }
        if let Some(drag) = self.ai_selection_dragging.as_mut() {
            drag.msg_index += count;
        }
        if let Some(s_idx) = self.ai_search_flat_index.as_mut() {
            *s_idx += count;
        }

        // 3. 偏移 RefCell 内部向量
        {
            let mut copy_done = self.ai_copy_done_indices.borrow_mut();
            for idx in copy_done.iter_mut() {
                *idx += count;
            }
        }
        {
            let mut quotes = self.ai_expanded_quotes.borrow_mut();
            for idx in quotes.iter_mut() {
                *idx += count;
            }
        }
        {
            let mut tools = self.ai_expanded_tools.borrow_mut();
            for idx in tools.iter_mut() {
                *idx += count;
            }
        }
        {
            let mut starts = self.ai_reveal_start.borrow_mut();
            let mut prepended_starts = vec![0u64; count];
            prepended_starts.extend(starts.drain(..));
            *starts = prepended_starts;

            let mut revealed = self.ai_reveal_revealed.borrow_mut();
            let mut prepended_revealed: Vec<usize> = self.messages[..count]
                .iter()
                .map(|m| ai_text_line_count(&m.text))
                .collect();
            prepended_revealed.extend(revealed.drain(..));
            *revealed = prepended_revealed;
        }

        // 4. 重构历史记录索引
        let mut new_history = Vec::new();
        for (idx, msg) in self.messages.iter().enumerate() {
            if msg.is_user {
                new_history.push((idx, msg.text.clone()));
            }
        }
        self.ai_history = new_history;

        // 5. 更新虚拟列表条目总数：在头部 splice
        self.list_state.splice(0..0, count);
    }

    /// 用户向上滚动触发：异步加载更早的历史消息（分页每次 10 条）
    fn load_more_history(&mut self, cx: &mut Context<Self>) {
        if self.loading_history || self.loading_older || !self.has_more_history {
            return;
        }
        // 当正在流式生成或处于编辑态时，暂不触发向上加载，避免索引并发错位
        if self.ai_streaming_index.is_some() || self.ai_editing_index.is_some() {
            return;
        }

        let pid = self
            .current_project_id
            .as_deref()
            .unwrap_or("default")
            .to_string();
        let target_conv = self.active_conversation_id.clone();
        let before_rowid = self.oldest_rowid;

        self.loading_older = true;
        cx.notify();

        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let res = smol::unblock(move || {
                load_project_session_paged(&pid, target_conv.as_deref(), before_rowid, 10)
            }).await;

            let _ = this.update(cx, |this, cx| {
                this.loading_older = false;
                if let Some(paged) = res {
                    if !paged.messages.is_empty() {
                        this.has_more_history = paged.has_more;
                        this.oldest_rowid = paged.oldest_rowid;
                        this.prepend_older_messages(paged.messages);
                    } else {
                        this.has_more_history = false;
                    }
                } else {
                    this.has_more_history = false;
                }
                cx.notify();
            });
        }).detach();
    }

    pub fn get_active_selection_text(&self, _cx: &App) -> Option<String> {
        self.ai_active_selection.as_ref().map(|s| s.text.clone())
    }

    pub fn get_selection_for_segment(&self, msg_idx: usize, seg_idx: usize) -> Option<&AiChatSelection> {
        self.ai_active_selection.as_ref().filter(|s| s.msg_index == msg_idx && s.seg_index == seg_idx)
    }

    pub fn get_selection_for_message(&self, msg_idx: usize) -> Option<String> {
        self.ai_active_selection.as_ref().filter(|s| s.msg_index == msg_idx).map(|s| s.text.clone())
    }

    pub fn handle_ai_selection_event(
        &mut self,
        msg_idx: usize,
        seg_idx: usize,
        event: velowork_markdown::MarkdownSelectionEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            velowork_markdown::MarkdownSelectionEvent::Start {
                offset,
                click_count,
                plain_text,
            } => {
                if click_count == 2 {
                    let (start, end) = velowork_markdown::find_word_boundaries(&plain_text, offset);
                    if start < end {
                        let selected_text: String =
                            plain_text.chars().skip(start).take(end - start).collect();
                        self.ai_active_selection = Some(AiChatSelection {
                            msg_index: msg_idx,
                            seg_index: seg_idx,
                            start,
                            end,
                            text: selected_text,
                        });
                        self.ai_selection_dragging = None;
                        cx.notify();
                        return;
                    }
                } else if click_count >= 3 {
                    let (start, end) = velowork_markdown::find_line_boundaries(&plain_text, offset);
                    if start < end {
                        let selected_text: String =
                            plain_text.chars().skip(start).take(end - start).collect();
                        self.ai_active_selection = Some(AiChatSelection {
                            msg_index: msg_idx,
                            seg_index: seg_idx,
                            start,
                            end,
                            text: selected_text,
                        });
                        self.ai_selection_dragging = None;
                        cx.notify();
                        return;
                    }
                }

                self.ai_selection_dragging = Some(AiSelectionDrag {
                    msg_index: msg_idx,
                    seg_index: seg_idx,
                    anchor: offset,
                    plain_text,
                });
                if self.ai_active_selection.is_some() {
                    self.ai_active_selection = None;
                    cx.notify();
                }
            }
            velowork_markdown::MarkdownSelectionEvent::Update { offset } => {
                let Some(ref drag) = self.ai_selection_dragging else {
                    return;
                };
                if drag.msg_index != msg_idx || drag.seg_index != seg_idx {
                    return;
                }
                let anchor = drag.anchor;
                let start = anchor.min(offset);
                let end = anchor.max(offset);
                if start < end {
                    let selected_text: String =
                        drag.plain_text.chars().skip(start).take(end - start).collect();
                    let changed = match &self.ai_active_selection {
                        Some(current) => {
                            current.msg_index != msg_idx
                                || current.seg_index != seg_idx
                                || current.start != start
                                || current.end != end
                                || current.text != selected_text
                        }
                        None => true,
                    };
                    if changed {
                        self.ai_active_selection = Some(AiChatSelection {
                            msg_index: msg_idx,
                            seg_index: seg_idx,
                            start,
                            end,
                            text: selected_text,
                        });
                        cx.notify();
                    }
                } else if self.ai_active_selection.is_some() {
                    self.ai_active_selection = None;
                    cx.notify();
                }
            }
            velowork_markdown::MarkdownSelectionEvent::End => {
                self.ai_selection_dragging = None;
            }
        }
    }

    pub fn clear_ai_selection(&mut self, cx: &mut Context<Self>) {
        if self.ai_active_selection.is_some() || self.ai_selection_dragging.is_some() {
            self.ai_active_selection = None;
            self.ai_selection_dragging = None;
            cx.notify();
        }
    }


    /// 打开系统文件选择框，将选中的本地文件 / 图片作为上下文附件加入输入框。
    fn attach_files(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(i18n!(cx, "ai.attach").into()),
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.attach_file_paths(paths, cx);
                });
            }
        })
        .detach();
    }

    /// 将指定的一组本地文件路径作为上下文附件加入输入框。
    pub(crate) fn attach_file_paths(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        let mut errors = Vec::new();
        let current_image_count = self.attachments.iter().filter(|a| a.is_image).count();
        let mut added_images = 0;

        for p in paths {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "附件".to_string());

            if is_vision_image_path(&p) {
                if current_image_count + added_images >= velowork_ai::MAX_IMAGES_PER_TURN {
                    errors.push(format!("图片 {name} 未添加：单轮最多支持 5 张图片"));
                    continue;
                }
                let Ok(meta) = std::fs::metadata(&p) else {
                    errors.push(format!("图片 {name} 无法读取元数据"));
                    continue;
                };
                if meta.len() > velowork_ai::MAX_IMAGE_ATTACHMENT_SIZE as u64 {
                    errors.push(format!("图片 {name} 超出 10MB 大小限制"));
                    continue;
                }

                // 预压入占位，标为未就绪 (is_ready = false)
                self.attachments.push(ChatAttachment {
                    path: p.clone(),
                    name: name.clone(),
                    is_image: true,
                    text_content: None,
                    image_data_url: None,
                    is_ready: false,
                });
                added_images += 1;

                // 调度后台异步线程读取与 Base64 编码，绝不阻塞 UI 主线程
                let p_clone = p.clone();
                let p_for_read = p.clone();
                cx.spawn(async move |this: WeakEntity<Self>, cx| {
                    let read_res = cx.background_executor().spawn(async move {
                        let bytes = std::fs::read(&p_for_read)?;
                        let mime = image_mime_type(&p_for_read);
                        use base64::Engine;
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        Ok::<_, std::io::Error>(format!("data:{mime};base64,{b64}"))
                    }).await;

                    let _ = this.update(cx, |this, cx| {
                        if let Ok(data_url) = read_res {
                            if let Some(att) = this.attachments.iter_mut().find(|a| a.path == p_clone) {
                                att.image_data_url = Some(data_url);
                                att.is_ready = true;
                            }
                        } else {
                            this.attachments.retain(|a| a.path != p_clone);
                        }
                        cx.notify();
                    });
                }).detach();
            } else {
                // 文本 / 代码 / SVG 文件处理
                match read_text_safe(&p) {
                    Ok(content) => {
                        self.attachments.push(ChatAttachment {
                            path: p,
                            name,
                            is_image: false,
                            text_content: Some(content),
                            image_data_url: None,
                            is_ready: true,
                        });
                    }
                    Err(err_reason) => {
                        errors.push(format!("{name}: {err_reason}"));
                    }
                }
            }
        }

        if !errors.is_empty() {
            let err_text = errors.join("\n");
            ToastManager::post(Toast::warning(err_text), cx);
        }

        cx.notify();
    }

    /// 将剪贴板中的截图或位图数据加入输入框附件。
    pub(crate) fn attach_clipboard_image(&mut self, image: gpui::Image, cx: &mut Context<Self>) {
        let current_image_count = self.attachments.iter().filter(|a| a.is_image).count();
        if current_image_count >= velowork_ai::MAX_IMAGES_PER_TURN {
            let err_text = format!("{}: 单轮最多支持 5 张图片", i18n!(cx, "ai.error"));
            ToastManager::post(Toast::warning(err_text), cx);
            return;
        }

        let bytes_len = image.bytes.len();
        if bytes_len > velowork_ai::MAX_IMAGE_ATTACHMENT_SIZE {
            let err_text = format!("{}: 粘贴图片超出 10MB 大小限制", i18n!(cx, "ai.error"));
            ToastManager::post(Toast::warning(err_text), cx);
            return;
        }

        // SVG 特殊分流：若复制的是 SVG 矢量，转为文本代码附件
        if image.format == gpui::ImageFormat::Svg {
            if let Ok(svg_content) = std::str::from_utf8(&image.bytes) {
                self.attachments.push(ChatAttachment {
                    path: std::path::PathBuf::from("clipboard.svg"),
                    name: "clipboard.svg".to_string(),
                    is_image: false,
                    text_content: Some(svg_content.to_string()),
                    image_data_url: None,
                    is_ready: true,
                });
                cx.notify();
                return;
            }
        }

        let ext = image.format.extension();
        let mime = image.format.mime_type().to_string();
        let cache_folder = velowork_core::profiles::cache_dir().join("attachments");
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let seq = (timestamp % 10000) as u32;
        let file_name = format!("paste_{}_{:08x}.{}", timestamp, (image.id & 0xffff_ffff) as u32, ext);
        let file_path = cache_folder.join(&file_name);
        let display_name = format!("截图_{:04}.{}", seq, ext);

        // 预压入占位，标为未就绪 (is_ready = false)
        self.attachments.push(ChatAttachment {
            path: file_path.clone(),
            name: display_name,
            is_image: true,
            text_content: None,
            image_data_url: None,
            is_ready: false,
        });
        cx.notify();

        // 调度后台异步线程写入本地缓存与 Base64 编码，绝不阻塞 UI 主线程
        let path_clone = file_path.clone();
        let path_for_write = file_path;
        let bytes = image.bytes;
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let write_res = cx.background_executor().spawn(async move {
                if let Some(parent) = path_for_write.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::write(&path_for_write, &bytes)?;
                use base64::Engine;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                Ok::<_, std::io::Error>(format!("data:{mime};base64,{b64}"))
            }).await;

            let _ = this.update(cx, |this, cx| {
                if let Ok(data_url) = write_res {
                    cx.remove_asset::<gpui::ImgResourceLoader>(&gpui::Resource::Path(path_clone.clone().into()));
                    if let Some(att) = this.attachments.iter_mut().find(|a| a.path == path_clone) {
                        att.image_data_url = Some(data_url);
                        att.is_ready = true;
                    }
                } else {
                    this.attachments.retain(|a| a.path != path_clone);
                    let err_text = format!("{}: 写入剪贴板图片缓存失败", i18n!(cx, "ai.error"));
                    this.push_message(ChatMessage {
                        is_user: false,
                        text: err_text,
                        streaming: false,
                        document_views: std::cell::RefCell::new(Vec::new()),
                        tool_call: None,
                        thinking: None,
                        quote: None,
                        attachments: Vec::new(),
                    });
                    this.scroll_to_bottom();
                }
                cx.notify();
            });
        }).detach();
    }

    /// 处理剪贴板粘贴（拦截 Ctrl+V / Cmd+V）：
    /// 1. 若剪贴板中含系统截图或位图数据（`ClipboardEntry::Image`），保存至安全缓存目录并挂载为图片附件；
    /// 2. 若剪贴板中含外部文件（`ClipboardEntry::ExternalPaths`），走附件通道挂载；
    /// 3. 若剪贴板同时含有非空文本，自动将其填入输入框（图文并茂）；
    /// 4. 若无图片及外部文件（纯文本），返回 false，交由原生输入框按普通文本粘贴处理。
    pub(crate) fn handle_clipboard_paste(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(item) = cx.read_from_clipboard() else {
            return false;
        };

        let mut pasted_image = None;
        let mut external_paths = Vec::new();
        let mut text_content = None;

        for entry in item.entries() {
            match entry {
                ClipboardEntry::Image(img) => {
                    if pasted_image.is_none() {
                        pasted_image = Some(img.clone());
                    }
                }
                ClipboardEntry::ExternalPaths(paths) => {
                    for p in paths.paths() {
                        external_paths.push(p.clone());
                    }
                }
                ClipboardEntry::String(s) => {
                    if text_content.is_none() && !s.text.trim().is_empty() {
                        text_content = Some(s.text.clone());
                    }
                }
            }
        }

        // 纯文本且无外部文件：交由原生 SimpleInput 粘贴
        if pasted_image.is_none() && external_paths.is_empty() {
            return false;
        }

        // 若同时有图文并茂中的文本，将其插入输入框
        if let Some(text) = text_content {
            if let Some(ref chat_input) = self.chat_input {
                chat_input.update(cx, |input, cx| {
                    input.insert_text(&text, cx);
                });
            }
        }

        // 处理外部文件
        if !external_paths.is_empty() {
            self.attach_file_paths(external_paths, cx);
        }

        // 处理剪贴板位图/系统截图
        if let Some(image) = pasted_image {
            self.attach_clipboard_image(image, cx);
        }

        true
    }

    /// 移除输入框中指定索引的附件。
    fn remove_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.attachments.len() {
            self.attachments.remove(index);
            cx.notify();
        }
    }

    /// 打开应用内附件预览弹窗（居中展示大图或文本/代码）。
    pub(crate) fn open_attachment_preview(&mut self, att: ChatAttachment, cx: &mut Context<Self>) {
        let dialog = cx.new(|cx| AttachmentPreviewDialog::new(att, cx));

        cx.subscribe(&dialog, move |this, _dialog, event: &AttachmentPreviewDialogEvent, cx| {
            match event {
                AttachmentPreviewDialogEvent::Close => {
                    this.overlay_manager.update(cx, |om, cx| om.close_modal(cx));
                }
            }
        })
        .detach();

        self.overlay_manager.update(cx, |om, cx| {
            om.open_modal(dialog, cx);
        });
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

    /// 点击历史列表中的某条消息：定位到聊天主区域该消息的原始位置。
    fn scroll_to_history_message(&mut self, index: usize, cx: &mut Context<Self>) {
        self.ai_history_open = false;
        self.list_state.scroll_to_reveal_item(index);
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

    /// Focus the chat input box.
    pub fn focus_input(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ref chat_input) = self.chat_input {
            chat_input.update(cx, |inp, cx| inp.focus(window, cx));
        }
    }

    /// Append a completed external conversation turn (from terminal inline AI popover)
    /// and persist it to the active project session.
    pub fn append_external_turn(
        &mut self,
        _project_id: &str,
        user_text: &str,
        quote: Option<&str>,
        assistant_reply: &str,
        cx: &mut Context<Self>,
    ) {
        if self.messages.len() == 1 && !self.messages[0].is_user {
            self.messages.clear();
            self.list_state.reset(0);
        }

        let user_msg = ChatMessage {
            is_user: true,
            text: user_text.to_string(),
            streaming: false,
            document_views: std::cell::RefCell::new(Vec::new()),
            tool_call: None,
            thinking: None,
            quote: quote.map(|q| q.to_string()),
            attachments: Vec::new(),
        };

        let assistant_doc_view =
            cx.new(|cx| velowork_markdown::widgets::DocumentView::new(assistant_reply, cx));
        let assistant_msg = ChatMessage {
            is_user: false,
            text: assistant_reply.to_string(),
            streaming: false,
            document_views: std::cell::RefCell::new(vec![assistant_doc_view]),
            tool_call: None,
            thinking: None,
            quote: None,
            attachments: Vec::new(),
        };

        self.push_message(user_msg);
        self.push_message(assistant_msg);
        self.save_current_sessions_to_disk();
        self.scroll_to_bottom();
        cx.notify();
    }

    /// Import multiple external conversation messages (from terminal inline AI popover)
    /// and persist them into the active panel session.
    pub fn import_external_messages(
        &mut self,
        _project_id: &str,
        messages: &[crate::views::ai::types::ChatMessage],
        cx: &mut Context<Self>,
    ) {
        if messages.is_empty() {
            return;
        }
        if self.messages.len() == 1 && !self.messages[0].is_user {
            self.messages.clear();
            self.list_state.reset(0);
        }
        for m in messages {
            if m.streaming && m.text.is_empty() {
                continue;
            }
            let doc_views = if !m.is_user && !m.text.is_empty() {
                let doc = cx.new(|cx| velowork_markdown::widgets::DocumentView::new(&m.text, cx));
                vec![doc]
            } else {
                Vec::new()
            };
            let panel_msg = ChatMessage {
                is_user: m.is_user,
                text: m.text.clone(),
                streaming: false,
                document_views: std::cell::RefCell::new(doc_views),
                tool_call: None,
                thinking: None,
                quote: m.quote.clone(),
                attachments: m.attachments.iter().map(|a| crate::views::panels::ai_assistant_panel::ChatAttachment {
                    path: a.path.clone(),
                    name: a.name.clone(),
                    is_image: a.is_image,
                    text_content: a.text_content.clone(),
                    image_data_url: a.image_data_url.clone(),
                    is_ready: a.is_ready,
                }).collect(),
            };
            self.push_message(panel_msg);
        }
        self.save_current_sessions_to_disk();
        self.scroll_to_bottom();
        cx.notify();
    }

    fn send_ai_message(&mut self, cx: &mut Context<Self>) {
        let input_text = self
            .chat_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string().trim().to_string())
            .unwrap_or_default();
        if (input_text.is_empty() && self.ai_quote.is_none() && self.attachments.is_empty()) || self.ai_streaming_index.is_some() {
            return;
        }

        // 检查是否有仍在后台转码中的图片附件，防止并发漏发
        if self.attachments.iter().any(|a| !a.is_ready) {
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
        self.scroll_to_bottom();
        self.save_current_sessions_to_disk();
        self.generate_ai_reply(cx);

        cx.notify();
    }

    /// 发送按钮的统一入口：根据当前是否正在生成以及输入框是否有内容，
    /// 在「发送新消息 / 终止生成 / 加入待发送队列」三种行为间切换。
    fn on_send_button(&mut self, cx: &mut Context<Self>) {
        // 引用文本正在编辑中时，严禁误触发发送，避免未保存的引用或未完成输入被意外提交。
        if self.ai_quote_editing {
            return;
        }
        // 附件仍在后台异步读取/转码中，拦截发送防止图片漏发
        if self.attachments.iter().any(|a| !a.is_ready) {
            return;
        }
        let is_streaming = self.ai_streaming_index.is_some();
        let has_input = self.has_input_content(cx);
        if is_streaming && !has_input {
            // 仅正在生成且无新输入：点击终止当前生成。
            self.stop_generation(cx);
        } else if is_streaming && has_input {
            // 正在生成且已输入文字：当前消息入队，待生成结束后自动发送。
            self.enqueue_current_input(cx);
        } else {
            // 空闲：直接发送。
            self.send_ai_message(cx);
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
    fn enqueue_current_input(&mut self, cx: &mut Context<Self>) {
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
            i18n!(cx, "ai.stop")
        } else if is_streaming && has_input {
            i18n!(cx, "ai.pending_send_now")
        } else {
            i18n!(cx, "ai.send")
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

            let rx = self.ai_client.stream_reply_simple(
                &model_config.base_url,
                &model_config.api_key,
                &model_config.model_id,
                None,
                &compressed,
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
                                loop {
                                    match rx.try_recv() {
                                        Ok(chunk) => match chunk {
                                            StreamChunk::Delta(text) => {
                                                if let Some(idx) = this.ai_streaming_index {
                                                    this.messages[idx].text.push_str(&text);
                                                    this.update_message_input_states(idx, cx);
                                                    this.list_state.remeasure_items(idx..idx + 1);
                                                }
                                            }
                                            StreamChunk::Done => {
                                                if let Some(idx) = this.ai_streaming_index {
                                                    this.messages[idx].streaming = false;
                                                    if this.messages[idx].text.is_empty() {
                                                        this.messages[idx].text = format!(
                                                            "{}: {}",
                                                            i18n!(cx, "ai.error"),
                                                            i18n!(cx, "ai.empty_response")
                                                        );
                                                    }
                                                    this.update_message_input_states(idx, cx);
                                                    let total = ai_text_line_count(&this.messages[idx].text);
                                                    {
                                                        let mut rev = this.ai_reveal_revealed.borrow_mut();
                                                        if idx < rev.len() {
                                                            rev[idx] = total;
                                                        }
                                                    }
                                                    this.list_state.remeasure_items(idx..idx + 1);
                                                }
                                                this.ai_streaming_index = None;
                                                this.save_current_sessions_to_disk();
                                                this.maybe_send_pending(cx);
                                                should_clear = true;
                                                done = true;
                                                break;
                                            }
                                            StreamChunk::Error(e) => {
                                                if let Some(idx) = this.ai_streaming_index {
                                                    let err_str = e.to_string();
                                                    let display_err = if err_str.contains("400") && (err_str.contains("content") || err_str.contains("image")) {
                                                        format!("{err_str}\n（提示：当前模型可能不支持视觉多模态输入，建议在顶部切换为 GPT-4o、Claude 3.5 Sonnet 或 Qwen-VL 等多模态模型）")
                                                    } else {
                                                        err_str
                                                    };
                                                    this.messages[idx].text = format!(
                                                        "{}: {}",
                                                        i18n!(cx, "ai.error"),
                                                        display_err
                                                    );
                                                    this.messages[idx].streaming = false;
                                                    this.update_message_input_states(idx, cx);
                                                    let total = ai_text_line_count(&this.messages[idx].text);
                                                    {
                                                        let mut rev = this.ai_reveal_revealed.borrow_mut();
                                                        if idx < rev.len() {
                                                            rev[idx] = total;
                                                        }
                                                    }
                                                    this.list_state.remeasure_items(idx..idx + 1);
                                                }
                                                this.ai_streaming_index = None;
                                                this.save_current_sessions_to_disk();
                                                this.maybe_send_pending(cx);
                                                should_clear = true;
                                                done = true;
                                                break;
                                            }
                                            StreamChunk::ToolCalls(_) => {
                                                // 纯聊天模式不使用工具，忽略。
                                            }
                                        },
                                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                            if let Some(idx) = this.ai_streaming_index {
                                                this.messages[idx].streaming = false;
                                                if this.messages[idx].text.is_empty() {
                                                    this.messages[idx].text = format!(
                                                        "{}: {}",
                                                        i18n!(cx, "ai.error"),
                                                        i18n!(cx, "ai.network_interrupted")
                                                    );
                                                }
                                                this.update_message_input_states(idx, cx);
                                                let total = ai_text_line_count(&this.messages[idx].text);
                                                {
                                                    let mut rev = this.ai_reveal_revealed.borrow_mut();
                                                    if idx < rev.len() {
                                                        rev[idx] = total;
                                                    }
                                                }
                                                this.list_state.remeasure_items(idx..idx + 1);
                                            }
                                            this.ai_streaming_index = None;
                                            this.save_current_sessions_to_disk();
                                            this.maybe_send_pending(cx);
                                            should_clear = true;
                                            done = true;
                                            break;
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
                let agent_messages = velowork_ai::simple_messages_to_api_values(
                    &compressed,
                    Some(&system),
                    1,
                );
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
                                                this.list_state.remeasure_items(idx..idx + 1);
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
                                                        this.list_state.remeasure_items(idx..idx + 1);
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
                                                this.list_state.remeasure_items(idx..idx + 1);
                                            }
                                            this.ai_streaming_index = None;
                                            done = true;
                                        }
                                        AgentEvent::Error(e) => {
                                            if let Some(idx) = this.ai_streaming_index {
                                                let display_err = if e.contains("400") && (e.contains("content") || e.contains("image")) {
                                                    format!("{e}\n（提示：当前模型可能不支持视觉多模态输入，建议在顶部切换为 GPT-4o、Claude 3.5 Sonnet 或 Qwen-VL 等多模态模型）")
                                                } else {
                                                    e
                                                };
                                                this.messages[idx].text = format!(
                                                    "{}: {}",
                                                    i18n!(cx, "ai.error"),
                                                    display_err
                                                );
                                                this.messages[idx].streaming = false;
                                                this.update_message_input_states(idx, cx);
                                                this.list_state.remeasure_items(idx..idx + 1);
                                            }
                                            this.ai_streaming_index = None;
                                            done = true;
                                        }
                                    },
                                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                        if let Some(idx) = this.ai_streaming_index {
                                            this.messages[idx].streaming = false;
                                            if this.messages[idx].text.is_empty() {
                                                this.messages[idx].text = format!(
                                                    "{}: {}",
                                                    i18n!(cx, "ai.error"),
                                                    i18n!(cx, "ai.network_interrupted")
                                                );
                                            }
                                            this.update_message_input_states(idx, cx);
                                            this.list_state.remeasure_items(idx..idx + 1);
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
                                        if this.messages[idx].text.is_empty() {
                                            this.messages[idx].text = format!(
                                                "{}: {}",
                                                i18n!(cx, "ai.error"),
                                                i18n!(cx, "ai.service_terminated")
                                            );
                                        }
                                        this.update_message_input_states(idx, cx);
                                        this.list_state.remeasure_items(idx..idx + 1);
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
                                format!("{}: {}", i18n!(cx, "ai.error"), e);
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
                    .placeholder(i18n!(cx, "ai.title"))
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
        let old_len = self.messages.len();
        self.messages[idx].text = new_text;
        self.update_message_input_states(idx, cx);
        // Remove any AI replies that followed this user message, then regenerate.
        self.messages.truncate(idx + 1);
        if old_len > idx + 1 {
            self.list_state.splice(idx + 1..old_len, 0);
        }
        self.ai_editing_index = None;
        self.scroll_to_bottom();
        self.generate_ai_reply(cx);
        cx.notify();
    }

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.ai_editing_index = None;
        cx.notify();
    }

    fn update_ai_search_matches(&mut self, cx: &mut Context<Self>) {
        let q = self
            .ai_search_input
            .as_ref()
            .map(|i| i.read(cx).text().to_string())
            .unwrap_or_default();
        if q.is_empty() {
            self.ai_search_flat_index = None;
            cx.notify();
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
        } else {
            self.ai_search_flat_index = Some(0);
            if let Some((mi, _range)) = matches.first() {
                self.scroll_to_message(*mi, cx);
            }
        }
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

    /// 输入框上方、附件标签行：以紧凑的 Tab 标签/Chip 样式展示已添加附件。
    /// 点击主体打开内置弹窗预览；明确点击右侧删除按钮才移除附件。
    fn render_attachment_preview(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = SemanticPalette::from_context(cx);
        let this = cx.entity();
        let attachments = self.attachments.clone();

        h_flex()
            .w_full()
            .flex_wrap()
            .px(SPACE_XS)
            .pb(SPACE_XS)
            .gap(SPACE_XS)
            .children(attachments.into_iter().enumerate().map(|(i, att)| {
                let this = this.clone();
                let att_path = att.path.clone();
                let is_image = att.is_image;
                let is_ready = att.is_ready;
                let name = att.name.clone();
                let remove_tip = i18n!(cx, "ai.attachment_remove");
                let preview_tip = format!("{}: {}", i18n!(cx, "ai.attachment_preview"), name);
                let att_for_preview = att.clone();
                let rm_group_id = SharedString::from(format!("ai-att-rm-{}", i));

                h_flex()
                    .id(ElementId::Name(format!("ai-att-tab-{}", i).into()))
                    .items_center()
                    .h(px(24.0))
                    .px(px(6.0))
                    .gap(px(5.0))
                    .rounded(RADIUS_STD)
                    .bg(p.surface_card)
                    .border_1()
                    .border_color(p.border_subtle)
                    .cursor_pointer()
                    .stateful_behavior(HoverBehavior {
                        hover_bg: p.surface_hover,
                        hover_border: Some(p.border_active),
                        ..Default::default()
                    })
                    // 左侧微型标识（16x16 / 12x12）
                    .child(if is_image {
                        if is_ready {
                            div()
                                .w(px(16.0))
                                .h(px(16.0))
                                .rounded(px(2.0))
                                .overflow_hidden()
                                .child(
                                    img(att_path)
                                        .w_full()
                                        .h_full()
                                        .object_fit(ObjectFit::Cover),
                                )
                                .into_any_element()
                        } else {
                            div()
                                .w(px(16.0))
                                .h(px(16.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    ProgressRing::new(0.5)
                                        .size(px(12.0))
                                        .stroke_width(px(1.5)),
                                )
                                .into_any_element()
                        }
                    } else {
                        AppIcon::File
                            .size(px(12.0))
                            .text_color(p.text_muted)
                            .into_any_element()
                    })
                    // 中间文件名
                    .child(
                        div()
                            .text_size(ui_text_xs(cx))
                            .text_color(p.text_secondary)
                            .max_w(px(160.0))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(name),
                    )
                    // 点击整个 Tab 预览
                    .on_click({
                        let _this = this.clone();
                        cx.listener(move |this, _, _window, cx| {
                            this.open_attachment_preview(att_for_preview.clone(), cx);
                        })
                    })
                    .tooltip(move |_, cx| {
                        let __tip = preview_tip.clone();
                        cx.new(|_| Tooltip::new(__tip)).into()
                    })
                    // 右侧关闭按钮（修复颜色看不见问题）
                    .child(
                        div()
                            .id(ElementId::Name(rm_group_id.clone()))
                            .group(rm_group_id.clone())
                            .w(px(16.0))
                            .h(px(16.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(RADIUS_SM)
                            .cursor_pointer()
                            .stateful_behavior(HoverBehavior {
                                hover_bg: p.status_error.opacity(0.18),
                                ..Default::default()
                            })
                            .child(
                                AppIcon::Close
                                    .size(px(10.0))
                                    .text_color(p.text_muted)
                                    .group_hover(rm_group_id, |s| s.text_color(p.status_error)),
                            )
                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .on_click({
                                let _this = this.clone();
                                cx.listener(move |this, _, _window, cx| {
                                    cx.stop_propagation();
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
        let edit_tip = i18n!(cx, "ai.quote_edit");
        let delete_tip = i18n!(cx, "ai.quote_delete");
        let save_tip = i18n!(cx, "ai.quote_save");

        div()
            .w_full()
            .px(SPACE_XS)
            .pb(SPACE_XS)
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .child(
                div()
                    .w_full()
                    .rounded(px(6.0))
                    .border_l_2()
                    .border_color(p.surface_accent)
                    .bg(p.surface_hover)
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .w_full()
                            .when(self.ai_quote_editing, |d| d.items_start())
                            .when(!self.ai_quote_editing, |d| d.items_center())
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
                                            .text_color(p.text_primary)
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
                                            |this, _window, cx| this.save_ai_quote(cx),
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
                                        |this, _window, cx| this.clear_ai_quote(cx),
                                    )),
                            ),
                    ),
            )
    }

    /// 引用内容编辑态的多行输入框。
    fn render_ai_quote_edit_input(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if event.keystroke.key.as_str() == "escape" {
                    this.cancel_edit_ai_quote(cx);
                    cx.stop_propagation();
                }
            }))
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
        let title = i18n!(cx, "ai.pending_queue");
        let remove_tip = i18n!(cx, "ai.pending_remove");

        div().w_full().px(SPACE_XS).pb(SPACE_XS).child(
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
                        i18n!(cx, "ai.quote")
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
        let is_new = self.ai_quote_input.is_none();
        let quote_input = self.ai_quote_input.get_or_insert_with(|| {
            cx.new(|cx| {
                InputState::new(cx)
                    .multiline()
                    .multiline_rows(3)
                    .wrap(true)
                    .submit_on_enter(true)
                    .pass_enter(false)
                    .placeholder(i18n!(cx, "ai.title"))
            })
        });
        if is_new {
            let quote_input_clone = quote_input.clone();
            cx.subscribe(
                &quote_input_clone,
                |this: &mut Self, _, event: &velowork_ui::input::InputEvent, cx| {
                    if *event == velowork_ui::input::InputEvent::PressEnter {
                        this.save_ai_quote(cx);
                    }
                },
            )
            .detach();
        }
        quote_input.update(cx, |input, cx| {
            input.set_value(&quote, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    /// 保存编辑态下的引用内容。
    fn save_ai_quote(&mut self, cx: &mut Context<Self>) {
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

    /// 取消编辑态下的引用内容，恢复原本的引用文本。
    fn cancel_edit_ai_quote(&mut self, cx: &mut Context<Self>) {
        self.ai_quote_editing = false;
        if let Some(ref input) = self.ai_quote_input {
            let orig = self.ai_quote.clone().unwrap_or_default();
            input.update(cx, |input, cx| input.set_value(&orig, cx));
        }
        cx.notify();
    }

    /// 删除引用块。
    fn clear_ai_quote(&mut self, cx: &mut Context<Self>) {
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

        let title = i18n!(cx, "ai.history");
        let close_tip = i18n!(cx, "common.action.close");

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
                .child(i18n!(cx, "ai.history_empty"))
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
                        .rounded(RADIUS_STD)
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
                        .gap(SPACE_2XS)
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
            state.set_text_size(Some(ui_text_md(cx)), cx);
            if state.selected_value() != selected.as_ref() {
                state.set_selected_value(selected, cx);
            }
        });

        div().min_w(px(80.0)).max_w(px(200.0)).child(Select::new(&self.ai_model_select))
    }

    fn render_perm_dropdown(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current_perm = self.ai_permission;
        self.ai_perm_select.update(cx, |state, cx| {
            state.set_text_size(Some(ui_text_md(cx)), cx);
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

    fn refresh_conversation_list(&mut self) {
        let pid = self.current_project_id.as_deref().unwrap_or("default");
        if let Some(db) = velowork_core::storage::database() {
            let repo = velowork_workspace::repositories::AiRepository::new(db);
            let convs = if pid == "default" {
                repo.list_conversations(None).unwrap_or_default()
            } else {
                repo.list_conversations(Some(pid)).unwrap_or_default()
            };
            self.ai_conversation_list = convs;
        }
    }

    fn toggle_sessions_popover(&mut self, cx: &mut Context<Self>) {
        if self.ai_sessions_popover_open {
            self.close_sessions_popover(cx);
        } else {
            self.ai_sessions_popover_open = true;
            self.cancel_rename_conversation(cx);
            self.init_sessions_search_input(cx);
            self.refresh_conversation_list();
            let reg = self.overlay_manager.read(cx).overlay_registry();
            let weak = cx.entity().downgrade();
            let close = std::sync::Arc::new(move |_: &mut Window, cx: &mut App| {
                if let Some(panel) = weak.upgrade() {
                    panel.update(cx, |this, cx| {
                        this.close_sessions_popover(cx);
                    });
                }
            });
            reg.update(cx, |r, _| {
                r.register(
                    OverlayInfo {
                        id: "ai-sessions-popover".into(),
                        bounds: Bounds::default(),
                        secondary_bounds: None,
                        close_policy: ClosePolicy::ClickOutside,
                        z_index: 2000,
                    },
                    close,
                );
            });
            cx.notify();
        }
    }

    fn init_sessions_search_input(&mut self, cx: &mut Context<Self>) {
        if self.ai_sessions_search_input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(i18n!(cx, "ai.search_sessions_placeholder"))
            });
            let input_clone = input.clone();
            cx.subscribe(
                &input_clone,
                |this: &mut Self, _, event: &velowork_ui::input::InputEvent, cx| {
                    if *event == velowork_ui::input::InputEvent::Change {
                        this.refresh_sessions_search(cx);
                    }
                },
            )
            .detach();
            self.ai_sessions_search_input = Some(input);
        }
    }

    fn refresh_sessions_search(&mut self, cx: &mut Context<Self>) {
        let q = self
            .ai_sessions_search_input
            .as_ref()
            .map(|i| i.read(cx).text().trim().to_string())
            .unwrap_or_default();
        self.ai_sessions_search_query = q.clone();

        if q.is_empty() {
            self.ai_sessions_search_results = None;
            self.ai_sessions_searching = false;
            cx.notify();
            return;
        }

        self.ai_sessions_search_generation = self.ai_sessions_search_generation.wrapping_add(1);
        let current_gen = self.ai_sessions_search_generation;
        let search_content = self.ai_sessions_search_content;
        let pid = self.current_project_id.as_deref().unwrap_or("default").to_string();

        self.ai_sessions_searching = true;
        cx.notify();

        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let results = smol::unblock(move || {
                let db = velowork_core::storage::database()?;
                let repo = velowork_workspace::repositories::AiRepository::new(db);
                let p = if pid == "default" { None } else { Some(pid.as_str()) };
                repo.search_conversations(p, &q, search_content).ok()
            })
            .await;

            let _ = this.update(cx, |this, cx| {
                if this.ai_sessions_search_generation != current_gen {
                    return;
                }
                this.ai_sessions_searching = false;
                this.ai_sessions_search_results = results;
                cx.notify();
            });
        })
        .detach();
    }

    fn close_sessions_popover(&mut self, cx: &mut Context<Self>) {
        if self.ai_sessions_popover_open {
            self.ai_sessions_popover_open = false;
            self.cancel_rename_conversation(cx);
            self.ai_sessions_search_query.clear();
            self.ai_sessions_search_results = None;
            self.ai_sessions_searching = false;
            if let Some(ref inp) = self.ai_sessions_search_input {
                inp.update(cx, |i, cx| i.set_value("", cx));
            }
            let reg = self.overlay_manager.read(cx).overlay_registry();
            reg.update(cx, |r, _| {
                r.unregister(&"ai-sessions-popover".into());
            });
            cx.notify();
        }
    }

    fn new_ai_chat(&mut self, cx: &mut Context<Self>) {
        let has_user_msg = self.messages.iter().any(|m| m.is_user);
        if !has_user_msg {
            self.close_sessions_popover(cx);
            return;
        }

        self.stop_generation(cx);
        self.ai_pending_queue.clear();
        self.save_current_sessions_to_disk();

        let pid = self.current_project_id.as_deref().unwrap_or("default").to_string();
        let new_conv_id = format!("conv_{}_{}", pid, chrono::Utc::now().timestamp_millis());
        self.active_conversation_id = Some(new_conv_id);

        self.messages.clear();
        self.list_state.reset(0);
        self.attachments.clear();
        self.ai_history_open = false;
        self.close_sessions_popover(cx);
        self.ai_history.clear();
        self.push_message(ChatMessage {
            is_user: false,
            text: i18n!(cx, "ai.welcome"),
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
        self.refresh_conversation_list();
        cx.notify();
    }

    fn switch_to_conversation(&mut self, target_id: &str, cx: &mut Context<Self>) {
        if self.active_conversation_id.as_deref() == Some(target_id) {
            self.close_sessions_popover(cx);
            return;
        }

        self.stop_generation(cx);
        self.ai_pending_queue.clear();
        self.save_current_sessions_to_disk();

        let target_conv_id = target_id.to_string();
        self.active_conversation_id = Some(target_conv_id.clone());
        self.close_sessions_popover(cx);
        self.loading_history = true;
        self.messages.clear();
        self.list_state.reset(0);
        cx.notify();

        let pid = self.current_project_id.as_deref().unwrap_or("default").to_string();
        let cid = target_conv_id.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let paged = smol::unblock(move || {
                load_project_session_paged(&pid, Some(&cid), None, 50)
            }).await;

            let _ = this.update(cx, |this, cx| {
                this.loading_history = false;
                if let Some(paged) = paged {
                    this.messages = paged.messages;
                    this.ai_history = paged.ai_history;
                    this.has_more_history = paged.has_more;
                    this.oldest_rowid = paged.oldest_rowid;
                    if let Some(m_id) = paged.selected_model_id {
                        this.ai_selected_model_id = Some(m_id.clone());
                        this.ai_model_select.update(cx, |s, cx| {
                            s.set_selected_value(Some(m_id), cx);
                        });
                    }
                }
                this.list_state.reset(this.messages.len());
                this.list_state.scroll_to_end();
                this.update_all_message_input_states(cx);
                this.refresh_conversation_list();
                if this.ai_search_open {
                    this.update_ai_search_matches(cx);
                }
                cx.notify();
            });
        }).detach();
    }

    fn switch_to_conversation_with_search(
        &mut self,
        target_id: &str,
        search_query: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let has_query = search_query.as_ref().map(|q| !q.trim().is_empty()).unwrap_or(false);
        let q = search_query.unwrap_or_default();
        self.switch_to_conversation(target_id, cx);
        if has_query {
            self.ai_search_open = true;
            if let Some(ref input) = self.ai_search_input {
                input.update(cx, |inp, cx| {
                    inp.set_value(&q, cx);
                });
            }
            self.update_ai_search_matches(cx);
        }
    }

    fn delete_conversation(&mut self, target_id: &str, cx: &mut Context<Self>) {
        if let Some(db) = velowork_core::storage::database() {
            let repo = velowork_workspace::repositories::AiRepository::new(db);
            let _ = repo.delete_messages(target_id);
            let _ = repo.delete_conversation(target_id);
        }

        self.ai_conversation_list.retain(|c| c.id != target_id);

        if self.active_conversation_id.as_deref() == Some(target_id) {
            if let Some(first_remaining) = self.ai_conversation_list.first() {
                let next_id = first_remaining.id.clone();
                self.switch_to_conversation(&next_id, cx);
            } else {
                self.messages.clear();
                self.list_state.reset(0);
                self.attachments.clear();
                self.ai_history.clear();
                let pid = self.current_project_id.as_deref().unwrap_or("default").to_string();
                let new_id = format!("conv_{}_{}", pid, chrono::Utc::now().timestamp_millis());
                self.active_conversation_id = Some(new_id);
                self.push_message(ChatMessage {
                    is_user: false,
                    text: i18n!(cx, "ai.welcome"),
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
                self.refresh_conversation_list();
            }
        }
        cx.notify();
    }

    fn confirm_clear_all_conversations(&mut self, cx: &mut Context<Self>) {
        let title = i18n!(cx, "ai.clear_all_history");
        let msg = i18n!(cx, "ai.clear_all_history_confirm");

        let dialog = cx.new(|cx| {
            ConfirmDialog::new(
                cx,
                title,
                msg,
                i18n!(cx, "common.action.confirm"),
                i18n!(cx, "common.action.cancel"),
                true,
                None,
                "ai-clear-all-sessions-confirm",
            )
        });

        cx.subscribe(&dialog, move |this, _dialog, event, cx| {
            if matches!(event, ConfirmDialogEvent::Confirmed { .. }) {
                this.clear_all_conversations(cx);
            }
            this.overlay_manager.update(cx, |om, cx| om.close_modal(cx));
        })
        .detach();

        self.overlay_manager.update(cx, |om, cx| {
            om.open_modal(dialog, cx);
        });
    }

    fn clear_all_conversations(&mut self, cx: &mut Context<Self>) {
        self.stop_generation(cx);
        self.ai_pending_queue.clear();
        let pid = self.current_project_id.as_deref().unwrap_or("default").to_string();

        if let Some(db) = velowork_core::storage::database() {
            let repo = velowork_workspace::repositories::AiRepository::new(db);
            for c in &self.ai_conversation_list {
                let _ = repo.delete_messages(&c.id);
                let _ = repo.delete_conversation(&c.id);
            }
        }

        self.ai_conversation_list.clear();
        self.messages.clear();
        self.list_state.reset(0);
        self.attachments.clear();
        self.ai_history.clear();
        let new_id = format!("conv_{}_{}", pid, chrono::Utc::now().timestamp_millis());
        self.active_conversation_id = Some(new_id);
        self.push_message(ChatMessage {
            is_user: false,
            text: i18n!(cx, "ai.welcome"),
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
        self.close_sessions_popover(cx);
    }

    fn start_renaming_conversation(&mut self, conv_id: &str, current_title: &str, cx: &mut Context<Self>) {
        self.renaming_conversation_id = Some(conv_id.to_string());
        let val = current_title.to_string();
        let state = cx.new(|cx| {
            SimpleInputState::new(cx)
                .default_value(&val)
                .submit_on_enter(true)
        });
        let state_clone = state.clone();
        cx.subscribe(
            &state_clone,
            |this: &mut Self, _, event: &InputEvent, cx| {
                match event {
                    InputEvent::PressEnter => {
                        this.submit_rename_conversation(cx);
                    }
                    InputEvent::Blur => {
                        this.submit_rename_conversation(cx);
                    }
                    _ => {}
                }
            },
        )
        .detach();
        self.rename_input_state = Some(state);
        cx.notify();
    }

    fn submit_rename_conversation(&mut self, cx: &mut Context<Self>) {
        if let (Some(conv_id), Some(ref input)) = (self.renaming_conversation_id.take(), self.rename_input_state.take()) {
            let new_title = input.read(cx).value().trim().to_string();
            if !new_title.is_empty() {
                if let Some(db) = velowork_core::storage::database() {
                    let repo = velowork_workspace::repositories::AiRepository::new(db);
                    if let Ok(Some(mut conv)) = repo.get_conversation(&conv_id) {
                        conv.title = Some(new_title.clone());
                        conv.updated_at = chrono::Utc::now().to_rfc3339();
                        let _ = repo.save_conversation(&conv);
                    }
                }
                if let Some(item) = self.ai_conversation_list.iter_mut().find(|c| c.id == conv_id) {
                    item.title = Some(new_title);
                }
            }
        }
        cx.notify();
    }

    fn cancel_rename_conversation(&mut self, cx: &mut Context<Self>) {
        self.renaming_conversation_id = None;
        self.rename_input_state = None;
        cx.notify();
    }

    fn render_sessions_popover(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = SemanticPalette::from_context(cx);
        let t = theme(cx);
        let is_search_focused = self
            .ai_sessions_search_input
            .as_ref()
            .map(|inp| inp.read(cx).focus_handle(cx).is_focused(window))
            .unwrap_or(false);
        let ring = focus_ring_shadows(&t);
        let active_id = self.active_conversation_id.clone();
        let renaming_id = self.renaming_conversation_id.clone();
        let conv_count = self.ai_conversation_list.len();
        let panel_weak = cx.entity().downgrade();

        let search_content = self.ai_sessions_search_content;
        self.ai_sessions_search_scope_select.update(cx, |state, cx| {
            state.set_options(
                vec![
                    SelectOption::new(false, i18n!(cx, "ai.search_scope_title")),
                    SelectOption::new(true, i18n!(cx, "ai.search_scope_content")),
                ],
                cx,
            );
            state.set_text_size(Some(ui_text_md(cx)), cx);
            if state.selected_value() != Some(&search_content) {
                state.set_selected_value(Some(search_content), cx);
            }
        });

        let backdrop = div()
            .id("ai-sessions-backdrop")
            .absolute()
            .inset_0()
            .on_mouse_down(MouseButton::Left, {
                let panel_weak = panel_weak.clone();
                move |_, _, cx| {
                    cx.stop_propagation();
                    if let Some(panel) = panel_weak.upgrade() {
                        panel.update(cx, |this, cx| {
                            this.close_sessions_popover(cx);
                        });
                    }
                }
            })
            .on_mouse_down(MouseButton::Right, {
                let panel_weak = panel_weak.clone();
                move |_, _, cx| {
                    cx.stop_propagation();
                    if let Some(panel) = panel_weak.upgrade() {
                        panel.update(cx, |this, cx| {
                            this.close_sessions_popover(cx);
                        });
                    }
                }
            });

        let list_items: Vec<AnyElement> = if let Some(ref search_results) = self.ai_sessions_search_results {
            if search_results.is_empty() {
                vec![
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .py(SPACE_LG)
                        .gap(SPACE_XS)
                        .child(AppIcon::Search.svg().size(ICON_MD).text_color(p.text_muted))
                        .child(
                            div()
                                .text_size(ui_text_md(cx))
                                .text_color(p.text_muted)
                                .child(i18n!(cx, "ai.no_sessions_found")),
                        )
                        .into_any_element(),
                ]
            } else {
                search_results
                    .iter()
                    .map(|res| {
                        let is_active = active_id.as_deref() == Some(&res.conversation.id);
                        let conv_id = res.conversation.id.clone();
                        let default_title = i18n!(cx, "ai.new_chat_title");
                        let title = res.conversation.title.as_deref().unwrap_or(&default_title);
                        let rel_time = format_relative_time(&res.conversation.updated_at, cx);
                        let title_str = title.to_string();
                        let snippet = res.matched_snippet.clone();
                        let group_name = SharedString::from(format!("session-search-row-{}", conv_id));
                        let row_id = SharedString::from(format!("ai-session-search-row-{}", conv_id));
                        let search_q = self.ai_sessions_search_query.clone();
                        let delete_tip: &'static str = Box::leak(i18n!(cx, "ai.delete_chat").into_boxed_str());

                        v_flex()
                            .id(row_id)
                            .group(group_name.clone())
                            .w_full()
                            .px(SPACE_XS)
                            .py(px(4.0))
                            .rounded(RADIUS_SM)
                            .cursor_pointer()
                            .gap(px(2.0))
                            .bg(if is_active { p.surface_hover } else { gpui::transparent_black() })
                            .hover(|s| s.bg(p.surface_hover))
                            .on_click({
                                let panel_weak = panel_weak.clone();
                                let cid = conv_id.clone();
                                let sq = search_q.clone();
                                move |_, _, cx| {
                                    if let Some(panel) = panel_weak.upgrade() {
                                        panel.update(cx, |this, cx| {
                                            this.switch_to_conversation_with_search(&cid, Some(sq.clone()), cx);
                                        });
                                    }
                                }
                            })
                            .child(
                                h_flex()
                                    .w_full()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        h_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .items_center()
                                            .gap(SPACE_SM)
                                            .child(
                                                div()
                                                    .size(px(6.0))
                                                    .rounded_full()
                                                    .bg(if is_active { p.surface_accent } else { gpui::transparent_black() })
                                                    .flex_shrink_0(),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .truncate()
                                                    .text_size(ui_text_md(cx))
                                                    .text_color(if is_active { p.text_primary } else { p.text_secondary })
                                                    .font_weight(if is_active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                                                    .child(title_str.clone()),
                                            )
                                            .when(!rel_time.is_empty(), |d| {
                                                d.child(
                                                    div()
                                                        .flex_shrink_0()
                                                        .text_size(ui_text_sm(cx))
                                                        .text_color(p.text_muted)
                                                        .child(rel_time),
                                                )
                                            }),
                                    )
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap(px(2.0))
                                            .opacity(0.0)
                                            .group_hover(group_name.clone(), |s| s.opacity(1.0))
                                            .child(
                                                div()
                                                    .id(SharedString::from(format!("session-search-delete-icon-{}", conv_id)))
                                                    .cursor_pointer()
                                                    .p(px(2.0))
                                                    .rounded(RADIUS_XS)
                                                    .hover(|s| s.bg(p.surface_hover))
                                                    .tooltip(move |_, cx| cx.new(|_| Tooltip::new(delete_tip)).into())
                                                    .child(AppIcon::Trash.svg().size(ICON_MICRO).text_color(p.status_error))
                                                    .on_click({
                                                        let panel_weak = panel_weak.clone();
                                                        let cid = conv_id.clone();
                                                        move |_, _, cx| {
                                                            cx.stop_propagation();
                                                            if let Some(panel) = panel_weak.upgrade() {
                                                                panel.update(cx, |this, cx| {
                                                                    this.delete_conversation(&cid, cx);
                                                                    this.refresh_sessions_search(cx);
                                                                });
                                                            }
                                                        }
                                                    }),
                                            ),
                                    ),
                            )
                            .when_some(snippet, |d, snip| {
                                let snippet_prefix = i18n!(cx, "ai.snippet_prefix");
                                d.child(
                                    div()
                                        .w_full()
                                        .ml(px(14.0))
                                        .px(px(4.0))
                                        .py(px(2.0))
                                        .rounded(RADIUS_XS)
                                        .bg(p.surface_base)
                                        .text_size(ui_text_xs(cx))
                                        .text_color(p.text_muted)
                                        .child(format!("{}: {}", snippet_prefix, snip.trim())),
                                )
                            })
                            .into_any_element()
                    })
                    .collect()
            }
        } else if conv_count == 0 {
            vec![
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .py(SPACE_MD)
                    .gap(SPACE_XS)
                    .child(AppIcon::History.svg().size(ICON_MD).text_color(p.text_muted))
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .text_color(p.text_muted)
                            .child(i18n!(cx, "ai.no_chat_history")),
                    )
                    .into_any_element(),
            ]
        } else {
            self.ai_conversation_list
                .iter()
                .map(|conv| {
                    let is_active = active_id.as_deref() == Some(&conv.id);
                    let is_renaming = renaming_id.as_deref() == Some(&conv.id);
                    let conv_id = conv.id.clone();
                    let default_title = i18n!(cx, "ai.new_chat_title");
                    let title = conv.title.as_deref().unwrap_or(&default_title);
                    let rel_time = format_relative_time(&conv.updated_at, cx);
                    let title_str = title.to_string();

                    if is_renaming {
                        h_flex()
                            .id(SharedString::from(format!("ai-session-renaming-{}", conv_id)))
                            .w_full()
                            .px(SPACE_XS)
                            .py(px(4.0))
                            .gap(px(2.0))
                            .items_center()
                            .when_some(self.rename_input_state.clone(), |d, input_st| {
                                d.child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(
                                            SimpleInput::new(&input_st)
                                                .size(ControlSize::Compact)
                                                .text_size(ui_text_md(cx)),
                                        ),
                                )
                            })
                            .child(
                                div()
                                    .id("session-rename-confirm-btn")
                                    .cursor_pointer()
                                    .p(px(2.0))
                                    .rounded(RADIUS_XS)
                                    .hover(|s| s.bg(p.surface_hover))
                                    .child(AppIcon::Check.svg().size(ICON_MICRO).text_color(p.status_success))
                                    .on_click({
                                        let panel_weak = panel_weak.clone();
                                        move |_, _, cx| {
                                            if let Some(panel) = panel_weak.upgrade() {
                                                panel.update(cx, |this, cx| {
                                                    this.submit_rename_conversation(cx);
                                                });
                                            }
                                        }
                                    }),
                            )
                            .child(
                                div()
                                    .id("session-rename-cancel-btn")
                                    .cursor_pointer()
                                    .p(px(2.0))
                                    .rounded(RADIUS_XS)
                                    .hover(|s| s.bg(p.surface_hover))
                                    .child(AppIcon::Close.svg().size(ICON_MICRO).text_color(p.text_muted))
                                    .on_click({
                                        let panel_weak = panel_weak.clone();
                                        move |_, _, cx| {
                                            if let Some(panel) = panel_weak.upgrade() {
                                                panel.update(cx, |this, cx| {
                                                    this.cancel_rename_conversation(cx);
                                                });
                                            }
                                        }
                                    }),
                            )
                            .into_any_element()
                    } else {
                        let group_name = SharedString::from(format!("session-row-{}", conv_id));
                        let row_id = SharedString::from(format!("ai-session-row-{}", conv_id));
                        let rename_tip: &'static str = Box::leak(i18n!(cx, "ai.rename_chat").into_boxed_str());
                        let delete_tip: &'static str = Box::leak(i18n!(cx, "ai.delete_chat").into_boxed_str());

                        h_flex()
                            .id(row_id)
                            .group(group_name.clone())
                            .w_full()
                            .px(SPACE_XS)
                            .py(px(4.0))
                            .rounded(RADIUS_SM)
                            .cursor_pointer()
                            .items_center()
                            .justify_between()
                            .bg(if is_active { p.surface_hover } else { gpui::transparent_black() })
                            .hover(|s| s.bg(p.surface_hover))
                            .on_click({
                                let panel_weak = panel_weak.clone();
                                let cid = conv_id.clone();
                                move |_, _, cx| {
                                    if let Some(panel) = panel_weak.upgrade() {
                                        panel.update(cx, |this, cx| {
                                            this.switch_to_conversation(&cid, cx);
                                        });
                                    }
                                }
                            })
                            .child(
                                h_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .items_center()
                                    .gap(SPACE_SM)
                                    .child(
                                        div()
                                            .size(px(6.0))
                                            .rounded_full()
                                            .bg(if is_active { p.surface_accent } else { gpui::transparent_black() })
                                            .flex_shrink_0(),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .truncate()
                                            .text_size(ui_text_md(cx))
                                            .text_color(if is_active { p.text_primary } else { p.text_secondary })
                                            .font_weight(if is_active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                                            .child(title_str.clone()),
                                    )
                                    .when(!rel_time.is_empty(), |d| {
                                        d.child(
                                            div()
                                                .flex_shrink_0()
                                                .text_size(ui_text_sm(cx))
                                                .text_color(p.text_muted)
                                                .child(rel_time),
                                        )
                                    }),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap(px(2.0))
                                    .opacity(0.0)
                                    .group_hover(group_name.clone(), |s| s.opacity(1.0))
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("session-rename-icon-{}", conv_id)))
                                            .cursor_pointer()
                                            .p(px(2.0))
                                            .rounded(RADIUS_XS)
                                            .hover(|s| s.bg(p.surface_hover))
                                            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(rename_tip)).into())
                                            .child(AppIcon::Edit.svg().size(ICON_MICRO).text_color(p.text_muted))
                                            .on_click({
                                                let panel_weak = panel_weak.clone();
                                                let cid = conv_id.clone();
                                                let t = title_str.clone();
                                                move |_, _, cx| {
                                                    cx.stop_propagation();
                                                    if let Some(panel) = panel_weak.upgrade() {
                                                        panel.update(cx, |this, cx| {
                                                            this.start_renaming_conversation(&cid, &t, cx);
                                                        });
                                                    }
                                                }
                                            }),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("session-delete-icon-{}", conv_id)))
                                            .cursor_pointer()
                                            .p(px(2.0))
                                            .rounded(RADIUS_XS)
                                            .hover(|s| s.bg(p.surface_hover))
                                            .tooltip(move |_, cx| cx.new(|_| Tooltip::new(delete_tip)).into())
                                            .child(AppIcon::Trash.svg().size(ICON_MICRO).text_color(p.status_error))
                                            .on_click({
                                                let panel_weak = panel_weak.clone();
                                                let cid = conv_id.clone();
                                                move |_, _, cx| {
                                                    cx.stop_propagation();
                                                    if let Some(panel) = panel_weak.upgrade() {
                                                        panel.update(cx, |this, cx| {
                                                            this.delete_conversation(&cid, cx);
                                                        });
                                                    }
                                                }
                                            }),
                                    ),
                            )
                            .into_any_element()
                    }
                })
                .collect()
        };

        let reg = self.overlay_manager.read(cx).overlay_registry();
        let card = div()
            .id("ai-sessions-popover-card")
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                canvas(
                    move |bounds: Bounds<Pixels>, _window: &mut Window, cx: &mut App| {
                        reg.update(cx, |r, _| {
                            r.set_bounds(&"ai-sessions-popover".into(), bounds);
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .absolute()
            .top(px(velowork_ui::tab_height(cx) + 4.0))
            .left(px(8.0))
            .right(px(8.0))
            .max_h(px(380.0))
            .flex()
            .flex_col()
            .bg(p.surface_raised)
            .border_1()
            .border_color(p.border_subtle)
            .rounded(RADIUS_MD)
            .shadow(elevation_menu_shadow())
            // Header
            .child(
                h_flex()
                    .h(px(34.0))
                    .px(SPACE_XS)
                    .border_b_1()
                    .border_color(p.border_subtle)
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(2.0))
                            .child(
                                AppIcon::History
                                    .svg()
                                    .size(ICON_SM)
                                    .text_color(p.text_secondary),
                            )
                            .child(
                                div()
                                    .text_size(ui_text_md(cx))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.text_primary)
                                    .child(if let Some(ref search_results) = self.ai_sessions_search_results {
                                        format!("{} ({})", i18n!(cx, "ai.chat_history"), search_results.len())
                                    } else {
                                        format!("{} ({})", i18n!(cx, "ai.chat_history"), conv_count)
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(2.0))
                            .when(conv_count > 0, |d| {
                                d.child(
                                    div()
                                        .id("ai-clear-all-sessions-btn")
                                        .cursor_pointer()
                                        .px(SPACE_XS)
                                        .py(px(2.0))
                                        .rounded(RADIUS_SM)
                                        .text_size(ui_text_md(cx))
                                        .text_color(p.text_muted)
                                        .hover(|s| s.text_color(p.status_error).bg(p.surface_hover))
                                        .child(i18n!(cx, "ai.clear_all_history"))
                                        .on_click({
                                            let panel_weak = panel_weak.clone();
                                            move |_, _, cx| {
                                                if let Some(panel) = panel_weak.upgrade() {
                                                    panel.update(cx, |this, cx| {
                                                        this.confirm_clear_all_conversations(cx);
                                                    });
                                                }
                                            }
                                        }),
                                )
                            })
                            .child(
                                div()
                                    .id("ai-popover-close-btn")
                                    .cursor_pointer()
                                    .p(px(2.0))
                                    .rounded(RADIUS_XS)
                                    .hover(|s| s.bg(p.surface_hover))
                                    .child(
                                        AppIcon::Close
                                            .svg()
                                            .size(ICON_MICRO)
                                            .text_color(p.text_muted),
                                    )
                                    .on_click({
                                        let panel_weak = panel_weak.clone();
                                        move |_, _, cx| {
                                            if let Some(panel) = panel_weak.upgrade() {
                                                panel.update(cx, |this, cx| {
                                                    this.close_sessions_popover(cx);
                                                });
                                            }
                                        }
                                    }),
                            ),
                    ),
            )
            // Sessions Search Bar
            .child({
                let has_search_query = !self.ai_sessions_search_query.is_empty();
                let panel_weak_search = panel_weak.clone();

                h_flex()
                    .h(px(36.0))
                    .px(SPACE_XS)
                    .py(SPACE_XS)
                    .gap(SPACE_XS)
                    .items_center()
                    .border_b_1()
                    .border_color(p.border_subtle)
                    .child(
                        h_flex()
                            .id("ai-sessions-search-input-group")
                            .flex_1()
                            .h(px(28.0))
                            .px(SPACE_XS)
                            .items_center()
                            .gap(px(4.0))
                            .rounded(RADIUS_STD)
                            .bg(if is_search_focused {
                                p.surface_hover
                            } else {
                                p.surface_card
                            })
                            .border_1()
                            .border_color(if is_search_focused {
                                p.border_active
                            } else {
                                p.border_subtle
                            })
                            .when(is_search_focused, |s| s.shadow(ring))
                            .when(!is_search_focused, |s| {
                                s.hover(|h| {
                                    h.border_color(p.surface_accent.opacity(0.6))
                                        .bg(p.surface_hover)
                                })
                            })
                            .child(AppIcon::Search.svg().size(ICON_MICRO).text_color(p.text_muted))
                            .child(
                                div()
                                    .id("ai-sessions-search-input-wrapper")
                                    .key_context("AiSessionsSearchBar")
                                    .flex_1()
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .child(if let Some(ref s_input) = self.ai_sessions_search_input {
                                        Input::new(s_input).borderless(true).text_size(ui_text_md(cx)).into_any_element()
                                    } else {
                                        div().into_any_element()
                                    })
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                            )
                            .when(has_search_query, |d| {
                                let panel_weak = panel_weak_search.clone();
                                d.child(
                                    div()
                                        .id("ai-sessions-search-clear")
                                        .cursor_pointer()
                                        .p(px(2.0))
                                        .rounded(RADIUS_XS)
                                        .hover(|s| s.bg(p.surface_hover))
                                        .child(AppIcon::Close.svg().size(ICON_MICRO).text_color(p.text_muted))
                                        .on_click(move |_, _, cx| {
                                            cx.stop_propagation();
                                            if let Some(panel) = panel_weak.upgrade() {
                                                panel.update(cx, |this, cx| {
                                                    if let Some(ref inp) = this.ai_sessions_search_input {
                                                        inp.update(cx, |i, cx| i.set_value("", cx));
                                                    }
                                                    this.refresh_sessions_search(cx);
                                                });
                                            }
                                        })
                                )
                            })
                    )
                    .child(
                        div()
                            .w(px(112.0))
                            .child(Select::new(&self.ai_sessions_search_scope_select))
                    )
            })
            // Scrollable List
            .child(
                div()
                    .id("ai-sessions-scroll-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .px(SPACE_XS)
                    .py(SPACE_XS)
                    .children(list_items),
            )
            // Footer
            .child(
                h_flex()
                    .px(SPACE_XS)
                    .py(SPACE_XS)
                    .border_t_1()
                    .border_color(p.border_subtle)
                    .child(
                        Button::new("ai-popover-footer-new-chat-btn", &t)
                            .label(format!("+ {}", i18n!(cx, "ai.new_chat")))
                            .small()
                            .text_size(ui_text_md(cx))
                            .variant(ControlVariant::Secondary)
                            .full_width(true)
                            .on_click({
                                let panel_weak = panel_weak.clone();
                                move |_, _, cx| {
                                    if let Some(panel) = panel_weak.upgrade() {
                                        panel.update(cx, |this, cx| {
                                            this.new_ai_chat(cx);
                                        });
                                    }
                                }
                            }),
                    ),
            );

        div()
            .id("ai-sessions-overlay-root")
            .absolute()
            .inset_0()
            .child(backdrop)
            .child(card)
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
        self.list_state.reset(0);
        self.attachments.clear();
        self.ai_history_open = false;
        self.ai_history.clear();
        self.push_message(ChatMessage {
            is_user: false,
            text: i18n!(cx, "ai.cleared"),
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
                        .placeholder(i18n!(cx, "ai.search_placeholder"))
                });
                let input_clone = input.clone();
                cx.subscribe(
                    &input_clone,
                    |this: &mut Self, _, event: &velowork_ui::input::InputEvent, cx| {
                        if *event == velowork_ui::input::InputEvent::PressEnter {
                            this.search_next(cx);
                        } else {
                            this.update_ai_search_matches(cx);
                        }
                    },
                )
                .detach();
                input
            });
            input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(cx);
            });
            self.update_ai_search_matches(cx);
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
                        .submit_on_enter(true)
                        .wrap(true)
                        .fill_height(true)
                        .placeholder(i18n!(cx, "terminal.inline_ai_follow_up_placeholder"))
                });
                let input_clone = input.clone();
                cx.subscribe(
                    &input_clone,
                    |this: &mut Self, _, event: &velowork_ui::input::InputEvent, cx| {
                        if *event == velowork_ui::input::InputEvent::PressEnter {
                            if !this.ai_quote_editing && this.has_input_content(cx) {
                                this.on_send_button(cx);
                            }
                            return;
                        }
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
                    .placeholder(i18n!(cx, "ai.search_placeholder"))
            });
            let input_clone = input.clone();
            cx.subscribe(
                &input_clone,
                |this: &mut Self, _, event: &velowork_ui::input::InputEvent, cx| {
                    if *event == velowork_ui::input::InputEvent::PressEnter {
                        this.search_next(cx);
                    } else {
                        this.update_ai_search_matches(cx);
                    }
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
                                .child(i18n!(cx, "ai.no_model")),
                        )
                        .child(
                            div()
                                .text_size(ui_text_md(cx))
                                .text_color(p.text_muted)
                                .child(i18n!(cx, "ai.no_model_hint")),
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
                                .child(i18n!(cx, "ai.go_settings"))
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

        let is_custom_titlebar = if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
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
                if cmd_or_ctrl && event.keystroke.key.eq_ignore_ascii_case("c") {
                    if let Some(text) = this.get_active_selection_text(cx) {
                        if !text.is_empty() {
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                            cx.stop_propagation();
                            return;
                        }
                    }
                }
                if cmd_or_ctrl && event.keystroke.key.eq_ignore_ascii_case("v") {
                    if this.handle_clipboard_paste(cx) {
                        cx.stop_propagation();
                        return;
                    }
                }
                if cmd_or_ctrl && event.keystroke.key.as_str() == "f" {

                    this.ai_search_open = true;
                    let input = this.ai_search_input.get_or_insert_with(|| {
                        let input = cx.new(|cx| {
                            InputState::new(cx)
                                .placeholder(i18n!(cx, "ai.search_placeholder"))
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
                if event.keystroke.key.as_str() == "escape" {
                    if this.renaming_conversation_id.is_some() {
                        this.cancel_rename_conversation(cx);
                        cx.stop_propagation();
                        cx.notify();
                        return;
                    }
                    if this.ai_sessions_popover_open {
                        this.close_sessions_popover(cx);
                        cx.stop_propagation();
                        cx.notify();
                        return;
                    }
                    if this.ai_search_open {
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
                        return;
                    }
                }
            }))
            .child(
                h_flex()
                    .h(px(velowork_ui::tab_height(cx)))
                    .px(ui_space_xs(cx))
                    .border_b_1()
                    .border_color(rgb(t.border))
                    .items_center()
                    .gap(ui_space_xs(cx))
                    .child(self.ai_icon_btn(
                        "ai-new-chat",
                        AppIcon::Plus,
                        i18n!(cx, "ai.new_chat"),
                        &t,
                        cx,
                        |this, _, cx| this.new_ai_chat(cx),
                    ))
                    .child(self.ai_icon_btn(
                        "ai-history-btn",
                        AppIcon::History,
                        i18n!(cx, "ai.chat_history"),
                        &t,
                        cx,
                        |this, _, cx| this.toggle_sessions_popover(cx),
                    ))
                    .child(self.ai_icon_btn(
                        "ai-clear-chat",
                        AppIcon::Trash,
                        i18n!(cx, "ai.clear_tooltip"),
                        &t,
                        cx,
                        |this, _, cx| this.clear_ai_chat(cx),
                    ))
                    .child(self.ai_icon_btn(
                        "ai-refresh",
                        AppIcon::Refresh,
                        i18n!(cx, "common.action.refresh"),
                        &t,
                        cx,
                        |this, _, cx| this.refresh_ai(cx),
                    ))
                    .child(div().w(px(1.0)).h(ICON_STD).bg(rgb(t.border)))
                    .child(self.ai_icon_btn(
                        "ai-search",
                        AppIcon::Search,
                        i18n!(cx, "ai.search"),
                        &t,
                        cx,
                        |this, window, cx| this.toggle_ai_search(window, cx),
                    ))
                    .child(div().flex_1())
                    .child({
                        let used_tokens = self.current_session_tokens();
                        let max_tokens = settings_entity(cx).read(cx).settings.ai_max_context_tokens.max(1);
                        let ratio = (used_tokens as f32 / max_tokens as f32).clamp(0.0, 1.0);
                        let pct = format!("{:.1}%", (used_tokens as f64 / max_tokens as f64) * 100.0);
                        let used_str = format_token_count(used_tokens);
                        let max_str = format_token_count(max_tokens);
                        let tooltip_text = format!(
                            "{} {}/{} {}",
                            pct,
                            used_str,
                            max_str,
                            i18n!(cx, "ai.context_used")
                        );
                        div()
                            .id("ai-token-usage-ring")
                            .size(px(24.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(RADIUS_STD)
                            .hover(|s| s.bg(surface_bg(t.bg_hover, cx)))
                            .child(
                                ProgressRing::new(ratio)
                                    .size(px(16.0))
                                    .stroke_width(px(2.0)),
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
                    .when(self.loading_history, |d| {
                        d.child(
                            div()
                                .id("ai-loading-container")
                                .size_full()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap(ui_space_sm(cx))
                                .child(
                                    velowork_ui::spinner::loading_spinner(
                                        "ai-history-loading-spinner",
                                        px(24.0),
                                        rgb(t.accent),
                                    ),
                                )
                                .child(
                                    div()
                                        .text_size(ui_text_sm(cx))
                                        .text_color(rgb(t.text_muted))
                                        .child(i18n!(cx, "common.state.loading")),
                                ),
                        )
                    })
                    .when(!self.loading_history, |d| {
                        let panel_entity = cx.entity().clone();
                        let copied_indices: Vec<usize> =
                            self.ai_copy_done_indices.borrow().clone();
                        let active_selection = self.ai_active_selection.clone();
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

                        // 预计算逐行显示信息
                        let mut reveal_info: Vec<(usize, bool)> = Vec::with_capacity(total_msgs);
                        {
                            let mut revealed = self.ai_reveal_revealed.borrow_mut();
                            for (mi, msg) in messages.iter().enumerate() {
                                let rev_lines = if !msg.is_user {
                                    self.ai_revealed_lines(mi, msg, frame)
                                } else {
                                    0usize
                                };
                                let changed = revealed.get(mi).copied().unwrap_or(0) < rev_lines;
                                if changed {
                                    revealed[mi] = rev_lines;
                                }
                                reveal_info.push((rev_lines, changed));
                            }
                        }

                        // 预计算搜索命中起始偏移
                        let mut msg_flat_offsets = Vec::with_capacity(total_msgs);
                        let mut running_flat = 0usize;
                        for msg in &messages {
                            msg_flat_offsets.push(running_flat);
                            if !search_query.is_empty() {
                                let cnt = find_match_ranges(&msg.text, &search_query, case_sensitive, use_regex).len();
                                running_flat += cnt;
                            }
                        }

                        // 归并同一轮 AI 回复中的多次工具调用
                        let mut current_ai_idx: Option<usize> = None;
                        let mut tool_calls_by_ai: std::collections::HashMap<
                            usize,
                            Vec<ToolCallCardData>,
                        > = std::collections::HashMap::new();
                        for (mi, msg) in messages.iter().enumerate() {
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
                        }

                        let expanded_quotes: Vec<usize> = self.ai_expanded_quotes.borrow().clone();
                        let expanded_tools: Vec<usize> = self.ai_expanded_tools.borrow().clone();
                        let focus_manager = self.focus_manager.clone();
                        let workspace = self.workspace.clone();
                        let terminals = self.terminals.clone();
                        let search_query_str = search_query.clone();
                        let editing_idx = self.ai_editing_index;
                        let edit_input = self.ai_edit_input.clone();
                        let t_theme = t;

                        let list_state = self.list_state.clone();
                        let messages_for_list = messages.clone();

                        let top_indicator = if self.loading_older {
                            Some(
                                h_flex()
                                    .w_full()
                                    .py(ui_space_xs(cx))
                                    .items_center()
                                    .justify_center()
                                    .gap(ui_space_xs(cx))
                                    .child(velowork_ui::spinner::loading_spinner(
                                        "ai-loading-older-spinner",
                                        px(14.0),
                                        rgb(t.accent),
                                    ))
                                    .child(
                                        div()
                                            .text_size(ui_text_xs(cx))
                                            .text_color(rgb(t.text_muted))
                                            .child(i18n!(cx, "common.state.loading")),
                                    ),
                            )
                        } else {
                            None
                        };

                        d.child(
                            div()
                                .id("ai-messages-container")
                                .size_full()
                                .px(ui_space_sm(cx))
                                .flex()
                                .flex_col()
                                .on_mouse_down(MouseButton::Left, {
                                    let panel_entity = panel_entity.clone();
                                    move |_ev, window, cx| {
                                        panel_entity.update(cx, |this, cx| {
                                            window.focus(&this.focus_handle, cx);
                                        });
                                    }
                                })
                                .children(top_indicator)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_h_0()
                                        .w_full()
                                        .child(
                                            list(list_state, move |ix, window, cx| {
                                                if ix >= messages_for_list.len() {
                                                    return div().into_any_element();
                                                }
                                                let msg = &messages_for_list[ix];
                                                if !msg.is_user && msg.tool_call.is_some() && msg.text.trim().is_empty() {
                                                    return div().h_0().overflow_hidden().into_any_element();
                                                }
                                                let copied = copied_indices.contains(&ix);
                                                let (revealed_lines, reveal_changed) = reveal_info.get(ix).copied().unwrap_or((0, false));
                                                let flat = msg_flat_offsets.get(ix).copied().unwrap_or(0);
                                                let empty_tools = Vec::new();
                                                let attached_tool_calls = tool_calls_by_ai.get(&ix).unwrap_or(&empty_tools);

                                                let (el, _cnt) = render_ai_message(
                                                    msg,
                                                    &t_theme,
                                                    window,
                                                    cx,
                                                    &focus_manager,
                                                    &workspace,
                                                    &terminals,
                                                    &search_query_str,
                                                    case_sensitive,
                                                    use_regex,
                                                    ix,
                                                    flat,
                                                    current_match,
                                                    &expanded_quotes,
                                                    &expanded_tools,
                                                    attached_tool_calls,
                                                    &panel_entity,
                                                    editing_idx,
                                                    edit_input.clone(),
                                                    copied,
                                                    &active_selection,
                                                    frame,
                                                    revealed_lines,
                                                    reveal_changed,
                                                );

                                                div()
                                                    .w_full()
                                                    .pb(ui_space_sm(cx))
                                                    .child(el)
                                                    .into_any_element()
                                            })
                                            .size_full()
                                            .py(ui_space_sm(cx)),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .right_0()
                                .left_0()
                                .child(
                                    Scrollbar::vertical(&self.list_state)
                                        .scrollbar_show(if self.ai_scrollbar_hovered.get() {
                                            ScrollbarShow::Always
                                        } else {
                                            ScrollbarShow::Never
                                        }),
                                ),
                        )
                    })
            })
            // 统一圆角容器：拖拽手柄 + 输入框（无边框）+ 底部工具栏（模型 / 权限 / 发送）
            .child({
                let input_height = px(self.ai_input_area_height);
                // ── 顶部拖拽手柄 ──
                let drag_entity = cx.entity().downgrade();

                div()
                    .id("ai-input-area")
                    .relative()
                    .flex_shrink_0()
                    .mx(SPACE_XS)
                    .mb(SPACE_XS)
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
                            if !this.ai_quote_editing {
                                if let Some(ref input) = this.chat_input {
                                    input.update(cx, |input, cx| {
                                        input.focus(window, cx);
                                    });
                                }
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
                    // ── 拖拽调整高度手柄（顶部边缘隐形热区，光标 ResizeUpDown，不挡圆角，无高亮线条） ──
                    .child(
                        div()
                            .id("ai-input-resize-handle")
                            .absolute()
                            .top_0()
                            .left(px(8.0))
                            .right(px(8.0))
                            .h(px(6.0))
                            .cursor(CursorStyle::ResizeUpDown)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, e: &MouseDownEvent, _window, cx| {
                                    this.ai_input_resize_dragging = Some(AiInputResizeDrag {
                                        start_y: f32::from(e.position.y),
                                        start_height: this.ai_input_area_height,
                                    });
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            ),
                    )
                    // ── 顶部工具栏：附件 | 历史 ──
                    .child(
                        h_flex()
                            .flex_shrink_0()
                            .w_full()
                            .px(SPACE_XS)
                            .pt(SPACE_XS)
                            .pb(px(2.0))
                            .items_center()
                            .gap(SPACE_XS)
                            .child(self.ai_icon_btn(
                                "ai-attach-btn",
                                AppIcon::NewFile,
                                i18n!(cx, "ai.attach"),
                                &t,
                                cx,
                                |this, window, cx| this.attach_files(window, cx),
                            ))
                            .child(self.ai_icon_btn(
                                "ai-history-btn",
                                AppIcon::MonitorClock,
                                i18n!(cx, "ai.history"),
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
                            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                                let is_newline = event.keystroke.modifiers.control
                                    || event.keystroke.modifiers.platform
                                    || event.keystroke.modifiers.shift;
                                if event.keystroke.key.as_str() == "enter" && !is_newline {
                                    if !this.ai_quote_editing && this.has_input_content(cx) {
                                        this.on_send_button(cx);
                                    }
                                    cx.stop_propagation();
                                    return;
                                }
                                let cmd_or_ctrl = event.keystroke.modifiers.platform || event.keystroke.modifiers.control;
                                if cmd_or_ctrl && event.keystroke.key.eq_ignore_ascii_case("c") {
                                    if let Some(text) = this.get_active_selection_text(cx) {
                                        if !text.is_empty() {
                                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                                            cx.stop_propagation();
                                            return;
                                        }
                                    }
                                }
                                if cmd_or_ctrl && event.keystroke.key.eq_ignore_ascii_case("v") {
                                    if this.handle_clipboard_paste(cx) {
                                        cx.stop_propagation();
                                        return;
                                    }
                                }
                            }))
                            .child(
                                div()
                                    .id("ai-input-wrapper")
                                    .w_full()
                                    .h_full()
                                    .px(SPACE_XS)
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
                            .px(SPACE_XS)
                            .pb(SPACE_XS)
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
                                    |this, _, _window, cx| {
                                        this.on_send_button(cx);
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
            .when(self.ai_sessions_popover_open, |d| {
                d.child(self.render_sessions_popover(window, cx))
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
                let is_search_focused = self
                    .ai_search_input
                    .as_ref()
                    .map(|inp| inp.read(cx).focus_handle(cx).is_focused(window))
                    .unwrap_or(false);
                let ring = focus_ring_shadows(&t);
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
                        .top(px(velowork_ui::tab_height(cx)) + ui_space_xs(cx))
                        .left(SPACE_XS)
                        .right(SPACE_XS)
                        .h(px(36.0))
                        .p(SPACE_XS)
                        .flex()
                        .items_center()
                        .gap(SPACE_XS)
                        .bg(p.surface_raised)
                        .border_1()
                        .border_color(p.border_subtle)
                        .rounded(RADIUS_LG)
                        .shadow_xl()
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
                                .flex_1()
                                .min_w(px(140.0))
                                .flex()
                                .items_center()
                                .rounded(RADIUS_STD)
                                .bg(if is_search_focused {
                                    p.surface_hover
                                } else {
                                    p.surface_card
                                })
                                .border_1()
                                .border_color(if is_search_focused {
                                    p.border_active
                                } else {
                                    p.border_subtle
                                })
                                .when(is_search_focused, |s| s.shadow(ring))
                                .when(!is_search_focused, |s| {
                                    s.hover(|h| {
                                        h.border_color(p.surface_accent.opacity(0.6))
                                            .bg(p.surface_hover)
                                    })
                                })
                                .child(
                                    div()
                                        .id("ai-search-input-wrapper")
                                        .key_context("AiSearchBar")
                                        .flex_1()
                                        .min_w(px(60.0))
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
                                        .flex_shrink_0()
                                        .cursor_pointer()
                                        .w(px(24.0))
                                        .h(px(24.0))
                                        .ml(px(2.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(RADIUS_STD)
                                        .when(cs, |s| {
                                            s.bg(p.text_primary.opacity(0.14))
                                                .border_1()
                                                .border_color(p.border_subtle)
                                                .text_color(p.text_primary)
                                                .hover(|h| h.bg(p.text_primary.opacity(0.18)))
                                        })
                                        .when(!cs, |s| {
                                            s.border_1()
                                                .border_color(gpui::transparent_black())
                                                .text_color(p.text_secondary)
                                                .hover(|h| {
                                                    h.bg(p.text_primary.opacity(0.10))
                                                        .text_color(p.text_primary)
                                                })
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
                                            this.update_ai_search_matches(cx);
                                        }))
                                        .child(
                                            div()
                                                .text_size(ui_text_md(cx))
                                                .font_weight(FontWeight::BOLD)
                                                .child("Aa"),
                                        )
                                })
                                .child({
                                    let rx = is_regex;
                                    let regex_tip = regex_tip.clone();
                                    div()
                                        .id("ai-search-regex-btn")
                                        .flex_shrink_0()
                                        .cursor_pointer()
                                        .w(px(24.0))
                                        .h(px(24.0))
                                        .ml(px(2.0))
                                        .mr(px(2.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(RADIUS_STD)
                                        .when(rx, |s| {
                                            s.bg(p.text_primary.opacity(0.14))
                                                .border_1()
                                                .border_color(p.border_subtle)
                                                .text_color(p.text_primary)
                                                .hover(|h| h.bg(p.text_primary.opacity(0.18)))
                                        })
                                        .when(!rx, |s| {
                                            s.border_1()
                                                .border_color(gpui::transparent_black())
                                                .text_color(p.text_secondary)
                                                .hover(|h| {
                                                    h.bg(p.text_primary.opacity(0.10))
                                                        .text_color(p.text_primary)
                                                })
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
                                            this.update_ai_search_matches(cx);
                                        }))
                                        .child(
                                            div()
                                                .text_size(ui_text_md(cx))
                                                .font_weight(FontWeight::BOLD)
                                                .child(".*"),
                                        )
                                }),
                        )
                        .child(
                            div()
                                .id("ai-search-match-count")
                                .flex_shrink_0()
                                .text_size(ui_text_sm(cx))
                                .text_color(p.text_secondary)
                                .min_w(px(30.0))
                                .px(px(2.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(match_text),
                        )
                        .child({
                            let prev_tip = prev_tip.clone();
                            div()
                                .id("ai-search-prev-btn")
                                .flex_shrink_0()
                                .cursor_pointer()
                                .w(px(28.0))
                                .h(px(28.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(RADIUS_STD)
                                .hover(|s| s.bg(p.surface_hover))
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
                                        .size(ICON_STD)
                                        .text_color(p.text_secondary),
                                )
                        })
                        .child({
                            let next_tip = next_tip.clone();
                            div()
                                .id("ai-search-next-btn")
                                .flex_shrink_0()
                                .cursor_pointer()
                                .w(px(28.0))
                                .h(px(28.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(RADIUS_STD)
                                .hover(|s| s.bg(p.surface_hover))
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
                                        .size(ICON_STD)
                                        .text_color(p.text_secondary),
                                )
                        })
                        .child({
                            let close_tip = close_tip.clone();
                            div()
                                .id("ai-search-close-btn")
                                .flex_shrink_0()
                                .cursor_pointer()
                                .w(px(28.0))
                                .h(px(28.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(RADIUS_STD)
                                .hover(|s| s.bg(p.surface_hover))
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
                                .child(AppIcon::Close.size(ICON_STD).text_color(p.text_secondary))
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
            i18n!(cx, "ai.title"),
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
    active_selection: &Option<AiChatSelection>,
    // 当前动画帧，用于加载动画与头像呼吸效果。
    frame: u64,
    // 本条 AI 消息已显示的文本行数（逐行显示动画）。
    revealed_lines: usize,
    // 已显示行数本帧是否增加（决定是否重写 markdown 视图内容）。
    _reveal_changed: bool,
) -> (AnyElement, usize) {
    let is_user = msg.is_user;
    let p = SemanticPalette::from_context(cx);

    // Whole-message search matches, used for the bubble-border highlight.
    // `flat_idx` is the running count of matches in earlier messages, so the
    // current-match index stays consistent across the whole conversation.
    let matches = if !search_query.is_empty() {
        find_match_ranges(&msg.text, search_query, case_sensitive, use_regex)
    } else {
        Vec::new()
    };
    let _is_highlighted = matches
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
                        AppIcon::Image
                    } else {
                        AppIcon::File
                    };
                    let panel_ent = panel_entity.clone();
                    let att_for_preview = att.clone();
                    let preview_tip = format!("{}: {}", i18n!(cx, "ai.attachment_preview"), name);
                    h_flex()
                        .id(ElementId::Name(format!("msg-att-chip-{}", name).into()))
                        .items_center()
                        .gap(SPACE_XS)
                        .px(px(6.0))
                        .py(px(2.0))
                        .rounded(px(4.0))
                        .bg(p.surface_hover)
                        .border_1()
                        .border_color(p.border_subtle)
                        .cursor_pointer()
                        .stateful_behavior(HoverBehavior {
                            hover_bg: p.surface_hover,
                            hover_border: Some(p.border_active),
                            ..Default::default()
                        })
                        .child(icon_path.size(px(11.0)).text_color(p.text_muted))
                        .child({
                            let n = name.clone();
                            div()
                                .text_size(ui_text_xs(cx))
                                .text_color(p.text_secondary)
                                .child(n)
                                .into_any_element()
                        })
                        .tooltip(move |_, cx| {
                            let __tip = preview_tip.clone();
                            cx.new(|_| Tooltip::new(__tip)).into()
                        })
                        .on_click(move |_, _window, cx| {
                            panel_ent.update(cx, |this, cx| {
                                this.open_attachment_preview(att_for_preview.clone(), cx);
                            });
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
                let on_toggle: Option<Arc<dyn Fn(usize, &mut Window, &mut App) + Send + Sync>> =
                    Some(Arc::new(move |msg_idx, _window, cx| {
                        let _ = entity.update(cx, |this, cx| {
                            let mut v = this.ai_expanded_quotes.borrow_mut();
                            if let Some(pos) = v.iter().position(|x| *x == msg_idx) {
                                v.remove(pos);
                            } else {
                                v.push(msg_idx);
                            }
                            cx.notify();
                        });
                    }));
                out.push(crate::views::ai::message_view::render_quote_capsule(
                    q,
                    msg_index,
                    expanded,
                    on_toggle,
                    &p,
                    cx,
                ));
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

        // 错误状态卡片：当 AI 回复内容为错误信息时，以显著红色告警卡片展示。
        let error_prefix = format!("{}:", i18n!(cx, "ai.error"));
        let is_error = msg.text.starts_with(&error_prefix);

        if is_error {
            let is_multiline = msg.text.contains('\n') || msg.text.chars().count() > 36;
            children.push(
                h_flex()
                    .w_full()
                    .gap(SPACE_SM)
                    .p(SPACE_SM)
                    .rounded(RADIUS_MD)
                    .bg(p.surface_danger.opacity(0.12))
                    .border_1()
                    .border_color(p.status_error)
                    .when(is_multiline, |d| d.items_start())
                    .when(!is_multiline, |d| d.items_center())
                    .child(
                        div()
                            .flex_shrink_0()
                            .when(is_multiline, |d| d.pt(px(2.0)))
                            .child(
                                AppIcon::Ban
                                    .size(px(14.0))
                                    .text_color(p.status_error),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(ui_text_md(cx))
                            .text_color(p.status_error)
                            .child(msg.text.clone()),
                    )
                    .into_any_element(),
            );
        } else if is_loading {
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
                                let mut revealed_text: String = s
                                    .split('\n')
                                    .take(reveal_in_seg)
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                let fence_count = revealed_text
                                    .lines()
                                    .filter(|l| l.trim_start().starts_with("```"))
                                    .count();
                                if fence_count % 2 != 0 {
                                    revealed_text.push_str("\n```");
                                }
                                let active_sel = active_selection
                                    .as_ref()
                                    .filter(|s| s.msg_index == msg_index && s.seg_index == text_seg_idx);
                                let sel_range = active_sel.map(|s| (s.start, s.end));
                                let panel_for_sel = panel_entity.clone();
                                let mut md_el = velowork_markdown::MarkdownElement::new(
                                    ElementId::from(format!("ai-md-{}-{}", msg_index, text_seg_idx)),
                                    &revealed_text,
                                )
                                .selection(sel_range)
                                .on_selection_event(move |ev, window, cx| {
                                    panel_for_sel.update(cx, |this, cx| {
                                        if let velowork_markdown::MarkdownSelectionEvent::Start { .. } = &ev {
                                            window.focus(&this.focus_handle, cx);
                                            if let Some(ref chat_input) = this.chat_input {
                                                chat_input.update(cx, |inp, cx| inp.clear_selection(cx));
                                            }
                                        }
                                        this.handle_ai_selection_event(msg_index, text_seg_idx, ev, cx);
                                    });
                                })
                                .on_url_click(move |url, _window, cx| {
                                    cx.open_url(url);
                                });

                                if !search_query.is_empty() {
                                    md_el = md_el.search(
                                        search_query,
                                        case_sensitive,
                                        use_regex,
                                        current_match,
                                        flat_idx,
                                    );
                                    let seg_cnt = find_match_ranges(&revealed_text, search_query, case_sensitive, use_regex).len();
                                    flat_idx += seg_cnt;
                                }

                                children.push(md_el.into_any_element());
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

        if children.is_empty() {
            if !msg.text.is_empty() {
                let (el, cnt) = text_bubble(
                    &msg.text,
                    false,
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
                let _ = cnt;
                children.push(el);
            } else if !msg.streaming {
                let err_msg = format!("{}: {}", i18n!(cx, "ai.error"), i18n!(cx, "ai.empty_response"));
                let is_multiline = err_msg.contains('\n') || err_msg.chars().count() > 36;
                children.push(
                    h_flex()
                        .w_full()
                        .gap(SPACE_SM)
                        .p(SPACE_SM)
                        .rounded(RADIUS_MD)
                        .bg(p.surface_danger.opacity(0.12))
                        .border_1()
                        .border_color(p.status_error)
                        .when(is_multiline, |d| d.items_start())
                        .when(!is_multiline, |d| d.items_center())
                        .child(
                            div()
                                .flex_shrink_0()
                                .when(is_multiline, |d| d.pt(px(2.0)))
                                .child(
                                    AppIcon::Ban
                                        .size(px(14.0))
                                        .text_color(p.status_error),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_size(ui_text_md(cx))
                                .text_color(p.status_error)
                                .child(err_msg),
                        )
                        .into_any_element(),
                );
            }
        }

        // 归并到本气泡的多次工具调用：默认折叠显示摘要，点击展开查看每个详情卡片。
        if !attached_tool_calls.is_empty() {
            let expanded = expanded_tools.contains(&msg_index);
            let count = attached_tool_calls.len();
            let names: Vec<String> = attached_tool_calls
                .iter()
                .map(|tc| {
                    if tc.kind == ToolCallKind::Result {
                        i18n!(cx, "ai.tool_result").to_string()
                    } else if tc.name.is_empty() {
                        i18n!(cx, "ai.tool_use").to_string()
                    } else {
                        tc.name.clone()
                    }
                })
                .collect();
            let tool_entity = panel_entity.clone();
            let toggle_label = if expanded {
                i18n!(cx, "ai.tool_calls_expanded")
            } else {
                i18n!(cx, "ai.tool_calls_collapsed").replace("{count}", &count.to_string())
            };
            let mut group = div()
                .mt(SPACE_SM)
                .rounded(RADIUS_LG)
                .border_1()
                .border_color(p.border_subtle)
                .bg(p.surface_raised)
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
            .w_full()
            .py(SPACE_XS)
            .child(v_flex().gap(SPACE_XS).children(children))
            .on_mouse_down(
                MouseButton::Right,
                move |event: &MouseDownEvent, window, cx| {
                    let panel = panel_entity_clone.clone();
                    let selection = panel
                        .read(cx)
                        .get_selection_for_message(msg_index)
                        .or_else(|| panel.read(cx).get_active_selection_text(cx));
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
                    i18n!(cx, "ai.send"),
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
                    .label(i18n!(cx, "common.action.cancel"))
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
        let edit_label = i18n!(cx, "common.action.edit");
        return (
            v_flex()
                .w_full()
                .gap(SPACE_SM)
                .group(group_name.clone())
                .child(
                    div()
                        .self_end()
                        .w_full()
                        .max_w(relative(0.92))
                        .p(SPACE_MD)
                        .rounded(RADIUS_LG)
                        .bg(p.surface_raised)
                        .border_1()
                        .border_color(p.border_subtle)
                        .flex()
                        .flex_col()
                        .gap(SPACE_SM)
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
        // AI reply: clean Antigravity style without avatar/name header, hover copy on the left.
        return (
            v_flex()
                .w_full()
                .gap(SPACE_SM)
                .group(group_name.clone())
                .child(
                    div()
                        .w_full()
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

// ── Loading Indicator ────────────────────────────────────────────────────

/// 加载状态指示器：三个错相位呼吸跳动的圆点 + 文案，直观表达「等待回复中」。
fn loading_indicator(_t: &ThemeColors, cx: &App, frame: u64) -> impl IntoElement {
    let p = SemanticPalette::from_context(cx);
    let label = i18n!(cx, "ai.thinking");
    let dots = (0..3).map(|i| {
        // 每个圆点相位错开，形成波浪式呼吸效果。
        let phase = (frame + i * 10) % 30;
        let wave = (phase as f32 / 30.0 * std::f32::consts::PI * 2.0).sin();
        let opacity = 0.35 + 0.65 * ((wave + 1.0) / 2.0);
        div()
            .w(px(7.0))
            .h(px(7.0))
            .rounded(px(3.5))
            .bg(p.surface_accent)
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
                .text_color(p.text_muted)
                .child(label),
        )
}

// ── Thinking Block ───────────────────────────────────────────────────────

/// 可折叠的思考过程展示区块。
fn thinking_block(content: &str, t: &ThemeColors, cx: &App) -> impl IntoElement {
    let header_label = i18n!(cx, "ai.thinking_process");
    let p = SemanticPalette::from_context(cx);
    div()
        .rounded(px(6.0))
        .border_1()
        .border_color(p.border_subtle)
        .bg(p.surface_raised)
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
        i18n!(cx, "ai.copy_done")
    } else {
        i18n!(cx, "common.action.copy")
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
    _is_user: bool,
    _t: &ThemeColors,
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
    let p = SemanticPalette::from_context(cx);

    let matches = if !search_query.is_empty() {
        find_match_ranges(text, search_query, case_sensitive, use_regex)
    } else {
        Vec::new()
    };

    let _is_highlighted = matches
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
                    p.surface_accent.into()
                } else {
                    p.surface_accent.opacity(0.28).into()
                }),
                color: if is_current {
                    Some(p.text_on_accent.into())
                } else {
                    None
                },
                font_weight: if is_current {
                    Some(FontWeight::BOLD)
                } else {
                    None
                },
                ..Default::default()
            },
        ));
    }

    let panel_entity_clone = panel_entity.clone();
    let text_clone = text.to_string();
    let bubble = div()
        .relative()
        .w_full()
        .when(use_custom_markdown_font(cx), |s| {
            s.font_family(markdown_font_family(cx))
        })
        .child(
            div()
                .text_color(p.text_primary)
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

    let p = SemanticPalette::from_context(cx);

    div()
        .relative()
        .max_w(relative(1.0))
        .rounded(RADIUS_LG)
        .overflow_hidden()
        .border_1()
        .border_color(p.border_subtle)
        .child(
            div()
                .px(SPACE_MD)
                .py(SPACE_XS)
                .bg(p.surface_header)
                .border_b_1()
                .border_color(p.border_subtle)
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
                                .label(i18n!(cx, "common.action.copy"))
                                .on_click(move |_ev, _window, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        code_clone.clone(),
                                    ));
                                }),
                        )
                        .child(
                            button_primary("code-send-btn", i18n!(cx, "ai.send"), &t)
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
                .bg(p.surface_raised)
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
    let p = SemanticPalette::from_context(cx);
    div()
        .relative()
        .max_w(relative(1.0))
        .rounded(RADIUS_LG)
        .overflow_hidden()
        .border_1()
        .border_color(p.border_subtle)
        .child(
            div()
                .px(SPACE_MD)
                .py(SPACE_XS)
                .bg(p.surface_header)
                .border_b_1()
                .border_color(p.border_subtle)
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
                        .label(i18n!(cx, "common.action.copy"))
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
                .bg(p.surface_raised)
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
        i18n!(cx, "ai.tool_result")
    } else {
        i18n!(cx, "ai.tool_use")
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
            let params_label = i18n!(cx, "ai.tool_params");
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

    let p = SemanticPalette::from_context(cx);
    div()
        .relative()
        .w_full()
        .rounded(RADIUS_LG)
        .overflow_hidden()
        .border_1()
        .border_color(p.border_subtle)
        .bg(p.surface_raised)
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
        title_key: "ai.title".to_string(),
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


