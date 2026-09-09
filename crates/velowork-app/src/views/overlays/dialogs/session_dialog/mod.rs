//! 会话配置弹窗核心模块。

pub mod changeset;
pub mod inputs;
pub mod model;
pub mod parser;
pub mod render;
pub mod section;
pub mod validation;

pub use model::*;
pub use render::render_session_dialog;
pub use section::{BUILTIN_SECTIONS, SectionDescriptor, SshSection, visible_sections};

use gpui::*;
use velowork_i18n::i18n;
use velowork_state::SessionProtocol;

/// 获取会话弹窗标题（统一「新建/编辑 + 协议名称 + 会话」格式）
pub fn session_dialog_title(is_edit: bool, protocol: SessionProtocol, cx: &App) -> String {
    if is_edit {
        match protocol {
            SessionProtocol::Ssh => i18n!(cx, "ssh.dialog.edit_session_ssh"),
            SessionProtocol::Serial => i18n!(cx, "ssh.dialog.edit_session_serial"),
            SessionProtocol::Telnet => i18n!(cx, "ssh.dialog.edit_session_telnet"),
            SessionProtocol::Local => i18n!(cx, "ssh.dialog.edit_session_local"),
        }
    } else {
        match protocol {
            SessionProtocol::Ssh => i18n!(cx, "ssh.dialog.new_session_ssh"),
            SessionProtocol::Serial => i18n!(cx, "ssh.dialog.new_session_serial"),
            SessionProtocol::Telnet => i18n!(cx, "ssh.dialog.new_session_telnet"),
            SessionProtocol::Local => i18n!(cx, "ssh.dialog.new_session_local"),
        }
    }
}

/// 统一会话配置弹窗状态。
pub enum SessionDialogState {
    Session {
        model: Box<SessionDialogModel>,
    },
}

impl SessionDialogState {
    pub fn model(&self) -> &SessionDialogModel {
        match self {
            Self::Session { model } => model,
        }
    }

    pub fn model_mut(&mut self) -> &mut SessionDialogModel {
        match self {
            Self::Session { model } => model,
        }
    }

    /// 计算当前协议对话框的最佳尺寸。
    pub fn preferred_size(&self, win_size: Size<Pixels>) -> Size<Pixels> {
        let is_ssh = self.model().config.protocol == velowork_state::SessionProtocol::Ssh;
        if is_ssh {
            let w = px(900.0).min(win_size.width - px(48.0));
            let h = px(680.0).min(win_size.height - px(80.0)).max(px(240.0));
            Size::new(w, h)
        } else {
            let w = px(820.0).min(win_size.width - px(48.0));
            let h = px(560.0).min(win_size.height - px(80.0)).max(px(240.0));
            Size::new(w, h)
        }
    }
}

/// 分组注册表：持有纯元数据描述符。
pub struct SectionRegistry {
    pub descriptors: Vec<SectionDescriptor>,
}

impl Default for SectionRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

impl SectionRegistry {
    /// 注册内建 8 组。
    pub fn with_builtins() -> Self {
        Self {
            descriptors: BUILTIN_SECTIONS.to_vec(),
        }
    }

    /// 注册新分组（插件扩展点）。
    pub fn register(&mut self, descriptor: SectionDescriptor) {
        self.descriptors.push(descriptor);
    }

    pub fn iter(&self) -> impl Iterator<Item = &SectionDescriptor> {
        self.descriptors.iter()
    }

    pub fn get(&self, id: SshSection) -> Option<&SectionDescriptor> {
        self.descriptors.iter().find(|d| d.id == id)
    }

    /// 搜索命中的分组集合（空关键字 = 全部）。
    pub fn match_search(&self, keyword: &str) -> Vec<SshSection> {
        self.descriptors
            .iter()
            .filter(|d| d.matches_search(keyword))
            .map(|d| d.id)
            .collect()
    }
}
