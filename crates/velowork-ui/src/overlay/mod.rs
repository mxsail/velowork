//! Overlay helpers for Velowork UI components.

mod animated_modal;
mod legacy;
mod modal;

pub use animated_modal::*;
pub use legacy::*;
pub use modal::{
    chrome_button, modal_body, modal_frame, ActionKind, BadgeTone, DetachAffordance, ModalAction,
    ModalActions, ModalBadge, ModalFooter, ModalHeader, OverlayAction, WindowCornerRadius,
    fullscreen_overlay, fullscreen_panel, window_drag_spacer, detached_needs_controls,
    window_min_max_controls, modal_backdrop, modal_content, modal_header,
};

use gpui::App;

pub use crate::overlay_registry::{ClosePolicy, OverlayId, OverlayInfo, OverlayRegistry};

/// 弹窗外侧遮罩点击行为策略
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OutsideClickBehavior {
    /// 点击外侧立即平滑关闭（适用于 A 类轻量选择器、B 类只读信息）
    #[default]
    Dismiss,
    /// 点击外侧完全阻断并消耗事件（适用于 C 类高危确认、D 类表单配置、E 类进行中任务）
    Block,
}

/// 弹窗 ESC 按键响应策略
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EscKeyBehavior {
    /// ESC 立即无条件关闭
    #[default]
    Immediate,
    /// ESC 触发安全取消（适用于 ConfirmDialog）
    SafeCancel,
    /// 表单为脏时弹出二次确认，干净时立即关闭
    ConfirmIfDirty,
    /// 由弹窗内部自行分层消费
    Custom,
}

/// 脏状态检查接口：供所有含编辑输入的弹窗实现
pub trait DirtyAware {
    /// 检查当前弹窗是否包含未保存的编辑内容
    fn is_dirty(&self, _cx: &App) -> bool {
        false
    }
}

/// Unified result of a modal / overlay interaction.
#[derive(Clone, Debug, PartialEq)]
pub enum ModalResult<T = ()> {
    /// The primary action succeeded, carrying its payload.
    Confirm(T),
    /// The user cancelled (Cancel button / Escape on a form).
    Cancel,
    /// The user dismissed via ✕ / click-outside (no intent to confirm).
    Close,
    /// The overlay auto-dismissed (e.g. a toast timeout).
    Timeout,
}
