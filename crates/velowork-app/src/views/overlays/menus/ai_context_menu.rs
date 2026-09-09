//! AI 助手面板气泡/代码块的右键菜单（选中文字后的上下文菜单）。
//!
//! 采用与快捷指令面板一致的全局统一实现：底层使用 [`PopupMenu`] +
//! [`ContextMenu::open`]，菜单作为 overlay 注册，自动 `snap_to_window`
//! 回退到可视区域（解决边缘被遮挡），并通过 `occlude` + 点击外部关闭
//! 解决穿透问题。

use gpui::*;
use std::sync::Arc;
use velowork_i18n::i18n;
use velowork_ui::icon::AppIcon;
use velowork_ui::menu::{ContextMenu, PopupMenu, PopupMenuItem};
use velowork_ui::overlay::OverlayRegistry;

use crate::views::panels::ai_assistant_panel::AiAssistantPanel;

/// 打开 AI 助手的「复制选中文本 / 复制」右键菜单。
///
/// - `panel_entity`：AI 助手面板实体，`on_close` 时据此清空菜单字段。
/// - `position`：右键触发时的屏幕坐标，菜单内部会 `snap_to_window` 防止超出窗口。
/// - `selection_text`：当前终端真实选区（若存在），提供「复制选中文本」项。
/// - `message_text`：整条消息文本，提供「复制」项。
/// - `overlay_registry`：overlay 注册表，用于把菜单挂到全局 overlay 层。
pub fn open_ai_context_menu(
    panel_entity: Entity<AiAssistantPanel>,
    position: Point<Pixels>,
    selection_text: Option<String>,
    message_text: String,
    overlay_registry: Option<Entity<OverlayRegistry>>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<PopupMenu> {
    let mut items = Vec::new();

    // 仅保留一个「复制」项：有选区则复制选区，否则复制整条消息，
    // 避免「复制选中内容」与「复制」语义重叠、菜单项重复。
    let copy_text = selection_text.unwrap_or_else(|| message_text.clone());
    let label = i18n!(cx, "common.copy");
    items.push(
        PopupMenuItem::item("ai-cm-copy", label, move |_, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
        })
        .icon(AppIcon::Copy),
    );

    // 关闭时清空面板中的菜单字段，保持 render 与状态一致。
    // 通过面板公开的 `close_ai_context_menu` 方法跨模块清理状态。
    let weak = panel_entity.downgrade();
    let on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync> =
        Arc::new(move |_, cx| {
            if let Some(panel) = weak.upgrade() {
                panel.update(cx, |panel, cx| {
                    panel.close_ai_context_menu(cx);
                });
            }
        });

    ContextMenu::open(position, items, overlay_registry, Some(on_close), window, cx)
}
