//! Right-panel command history list context menu.

use gpui::*;
use std::sync::Arc;
use velowork_i18n::i18n;
use velowork_ui::icon::AppIcon;
use velowork_ui::menu::{ContextMenu, PopupMenu, PopupMenuItem};
use velowork_ui::overlay::OverlayRegistry;
use velowork_workspace::repositories::HistoryEntry;

/// Request passed when showing the command history context menu.
pub struct CommandHistoryMenuRequest {
    pub position: Point<Pixels>,
    pub entry: HistoryEntry,
}

#[derive(Clone)]
pub enum CommandHistoryContextMenuEvent {
    Close,
    Execute { entry: HistoryEntry },
    SendToCommands { entry: HistoryEntry },
    AddToQuickCommands { entry: HistoryEntry },
    Copy { entry: HistoryEntry },
    SendToTerminal { entry: HistoryEntry },
    Delete { entry: HistoryEntry },
}

pub fn open_command_history_context_menu(
    request: CommandHistoryMenuRequest,
    overlay_registry: Option<Entity<OverlayRegistry>>,
    on_event: impl Fn(CommandHistoryContextMenuEvent, &mut App) + Send + Sync + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Entity<PopupMenu> {
    let on_event = Arc::new(on_event);
    let mut items = Vec::new();
    let entry = request.entry;

    // 1. 执行 (Execute)
    let e1 = entry.clone();
    let ev1 = on_event.clone();
    let label_exec = i18n!(cx, "command_history.menu_execute");
    items.push(
        PopupMenuItem::item("history-menu-exec", label_exec, move |_, cx| {
            ev1(CommandHistoryContextMenuEvent::Execute { entry: e1.clone() }, cx);
        })
        .icon(AppIcon::Play),
    );

    // 2. 发送到命令 (Send to Commands Panel)
    let e2 = entry.clone();
    let ev2 = on_event.clone();
    let label_send_cmd = i18n!(cx, "command_history.menu_send_to_commands");
    items.push(
        PopupMenuItem::item("history-menu-send-commands", label_send_cmd, move |_, cx| {
            ev2(CommandHistoryContextMenuEvent::SendToCommands { entry: e2.clone() }, cx);
        })
        .icon(AppIcon::CommandAction),
    );

    // 3. 添加到快捷指令 (Add to Quick Commands)
    let e3 = entry.clone();
    let ev3 = on_event.clone();
    let label_add_qc = i18n!(cx, "command_history.menu_add_to_quick_commands");
    items.push(
        PopupMenuItem::item("history-menu-add-qc", label_add_qc, move |_, cx| {
            ev3(CommandHistoryContextMenuEvent::AddToQuickCommands { entry: e3.clone() }, cx);
        })
        .icon(AppIcon::QuickCommand),
    );

    items.push(PopupMenuItem::separator());

    // 4. 复制 (Copy)
    let e4 = entry.clone();
    let ev4 = on_event.clone();
    let label_copy = i18n!(cx, "common.copy");
    items.push(
        PopupMenuItem::item("history-menu-copy", label_copy, move |_, cx| {
            ev4(CommandHistoryContextMenuEvent::Copy { entry: e4.clone() }, cx);
        })
        .icon(AppIcon::Copy),
    );

    // 5. 发送到终端 (Send to Terminal)
    let e5 = entry.clone();
    let ev5 = on_event.clone();
    let label_send_term = i18n!(cx, "command_history.menu_send_to_terminal");
    items.push(
        PopupMenuItem::item("history-menu-send-term", label_send_term, move |_, cx| {
            ev5(CommandHistoryContextMenuEvent::SendToTerminal { entry: e5.clone() }, cx);
        })
        .icon(AppIcon::Terminal),
    );

    items.push(PopupMenuItem::separator());

    // 6. 删除 (Delete - 警告色)
    let e6 = entry.clone();
    let ev6 = on_event.clone();
    let label_del = i18n!(cx, "common.delete");
    let t = velowork_ui::theme::theme(cx);
    items.push(
        PopupMenuItem::item("history-menu-delete", label_del, move |_, cx| {
            ev6(CommandHistoryContextMenuEvent::Delete { entry: e6.clone() }, cx);
        })
        .icon(AppIcon::Trash)
        .text_color(t.error),
    );

    let ev_on_close = on_event;
    let on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync> = Arc::new(move |_, cx| {
        ev_on_close(CommandHistoryContextMenuEvent::Close, cx);
    });

    ContextMenu::open(
        request.position,
        items,
        overlay_registry,
        Some(on_close),
        window,
        cx,
    )
}
