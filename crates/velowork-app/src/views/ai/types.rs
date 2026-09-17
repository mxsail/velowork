//! Shared types for AI chat conversations across panels and overlays.

use std::cell::RefCell;
use std::path::PathBuf;
use gpui::Entity;
use serde::{Deserialize, Serialize};

/// 工具调用卡片的种类：模型发起调用 vs 工具返回结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolCallKind {
    Use,
    Result,
}

/// 一条结构化的工具调用 / 工具结果，渲染为气泡内的专属卡片。
#[derive(Clone, Debug)]
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
#[derive(Clone, Debug)]
pub enum ToolSegment {
    Text(String),
    Tool {
        kind: ToolCallKind,
        name: String,
        body: String,
    },
}

/// 输入框添加的上下文附件：本地文件或图片。
#[derive(Clone, Debug)]
pub struct ChatAttachment {
    /// 附件在磁盘上的绝对路径。
    pub path: PathBuf,
    /// 显示用文件名。
    pub name: String,
    /// 是否为图片（决定缩略图渲染方式）。
    pub is_image: bool,
    /// 文本类附件解析出的内容；图片类为 `None`。
    pub text_content: Option<String>,
}

/// 统一的高保真聊天消息结构体。
pub struct ChatMessage {
    pub is_user: bool,
    pub text: String,
    pub streaming: bool,
    pub document_views: RefCell<Vec<Entity<velowork_markdown::widgets::DocumentView>>>,
    /// 结构化工具调用卡片（若有），优先于 `text` 渲染为专属组件。
    pub tool_call: Option<ToolCallCardData>,
    /// Agent 模式的思考过程（工具调用前的推理内容），可折叠展示。
    pub thinking: Option<String>,
    /// 随本条消息发送的终端选区引用内容。
    pub quote: Option<String>,
    /// 随本条消息发送的上下文附件（本地文件 / 图片），仅在用户消息上非空。
    pub attachments: Vec<ChatAttachment>,
}

impl Clone for ChatMessage {
    fn clone(&self) -> Self {
        Self {
            is_user: self.is_user,
            text: self.text.clone(),
            streaming: self.streaming,
            document_views: RefCell::new(Vec::new()),
            tool_call: self.tool_call.clone(),
            thinking: self.thinking.clone(),
            quote: self.quote.clone(),
            attachments: self.attachments.clone(),
        }
    }
}

impl ChatMessage {
    pub fn new_user(text: String, quote: Option<String>, attachments: Vec<ChatAttachment>) -> Self {
        Self {
            is_user: true,
            text,
            streaming: false,
            document_views: RefCell::new(Vec::new()),
            tool_call: None,
            thinking: None,
            quote,
            attachments,
        }
    }

    pub fn new_assistant(text: String, streaming: bool) -> Self {
        Self {
            is_user: false,
            text,
            streaming,
            document_views: RefCell::new(Vec::new()),
            tool_call: None,
            thinking: None,
            quote: None,
            attachments: Vec::new(),
        }
    }

    /// 标准化生成用于发送给大模型 API 的消息正文。
    /// 当包含终端引用时，按规范 Markdown 代码块包裹并附加用户问题。
    pub fn api_content(&self) -> String {
        if let Some(ref q) = self.quote {
            let trimmed_quote = q.trim();
            if !trimmed_quote.is_empty() {
                if self.text.trim().is_empty() {
                    format!("终端选中文本：\n```\n{}\n```", trimmed_quote)
                } else {
                    format!(
                        "终端选中文本：\n```\n{}\n```\n\n用户问题：{}",
                        trimmed_quote,
                        self.text.trim()
                    )
                }
            } else {
                self.text.clone()
            }
        } else {
            self.text.clone()
        }
    }

    /// 判断消息是否为错误告警消息
    pub fn is_error_with_prefix(&self, prefix: &str) -> bool {
        self.text.starts_with(prefix)
    }

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
            document_views: RefCell::new(Vec::new()),
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

#[derive(Clone, Serialize, Deserialize, Default)]
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

#[derive(Clone, Serialize, Deserialize, Default)]
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

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct SavedChatAttachment {
    #[serde(default)]
    pub path: PathBuf,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub is_image: bool,
    #[serde(default)]
    pub text_content: Option<String>,
}
