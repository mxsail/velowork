//! Command History panel — lists and manages executed commands per project.

use gpui::prelude::*;
use gpui::*;
use std::sync::Arc;
use velowork_core::storage::database;
use velowork_i18n::i18n;
use velowork_terminal::TerminalsRegistry;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::dock::{Panel, PanelAction, PanelInfo, PanelKind, RightToolbarRegistry, ToolbarPanelSpec};
use velowork_ui::icon::AppIcon;
use velowork_ui::menu::PopupMenu;
use velowork_ui::overlay_registry::OverlayRegistry;
use velowork_ui::scrollable::Scrollbar;
use velowork_ui::input::InputState;
use velowork_ui::theme::{surface_bg_t, theme, with_alpha};
use velowork_ui::tokens::{
    ui_space_sm, ui_space_xs, ui_text_xs, ICON_STD, RADIUS_STD,
};
use velowork_ui::tooltip::Tooltip;
use velowork_ui::{h_flex, v_flex};
use velowork_workspace::focus::FocusManager;
use velowork_workspace::repositories::{HistoryEntry, HistoryRepository};
use velowork_workspace::state::Workspace;

use crate::settings::settings_entity;
use crate::views::overlays::dialogs::quick_command_dialog::QuickCommandDialogMode;
use crate::views::overlays::menus::command_history_context_menu::{
    open_command_history_context_menu, CommandHistoryContextMenuEvent, CommandHistoryMenuRequest,
};
use crate::views::overlays::overlay_manager::OverlayManager;
use crate::views::panels::quick_commands_panel::send_command_to_focused_terminal;
use crate::views::panels::registry::AppPanelCreationContext;

/// Send text into terminal directly at cursor without appending carriage return.
pub(crate) fn paste_command_to_focused_terminal(
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
            terminal.send_bytes(cmd.as_bytes());
        }
    }
}

pub struct CommandHistoryPanel {
    workspace: Entity<Workspace>,
    focus_manager: Entity<FocusManager>,
    terminals: TerminalsRegistry,
    overlay_manager: Entity<OverlayManager>,
    overlay_registry: Option<Entity<OverlayRegistry>>,
    focus_handle: FocusHandle,
    scroll_handle: ScrollHandle,

    search_input: Option<Entity<InputState>>,
    search_query: String,
    show_search: bool,

    selected_id: Option<i64>,
    items: Vec<HistoryEntry>,
    context_menu: Option<Entity<PopupMenu>>,
    selected_item_bounds: std::rc::Rc<std::cell::RefCell<Option<Bounds<Pixels>>>>,
}

impl CommandHistoryPanel {
    pub fn new(
        workspace: Entity<Workspace>,
        focus_manager: Entity<FocusManager>,
        terminals: TerminalsRegistry,
        overlay_manager: Entity<OverlayManager>,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let scroll_handle = ScrollHandle::new();

        // 监听焦点终端切换以更新对应项目命令历史
        cx.observe(&focus_manager, |this: &mut Self, _, cx| {
            this.reload_items(cx);
        })
        .detach();

        // 监听云端恢复/同步拉取完成通知以刷新命令历史
        if let Some(sync_store) = crate::sync_engine::sync_status_store(cx) {
            cx.observe(&sync_store, |this: &mut Self, _, cx| {
                this.reload_items(cx);
            })
            .detach();
        }

        let mut panel = Self {
            workspace,
            focus_manager,
            terminals,
            overlay_manager,
            overlay_registry: None,
            focus_handle,
            scroll_handle,
            search_input: None,
            search_query: String::new(),
            show_search: false,
            selected_id: None,
            items: Vec::new(),
            context_menu: None,
            selected_item_bounds: std::rc::Rc::new(std::cell::RefCell::new(None)),
        };

        cx.observe(&panel.overlay_manager, |_this: &mut Self, _om, cx| {
            cx.notify();
        })
        .detach();

        panel.reload_items(cx);
        panel
    }

    pub fn is_selection_active(&self, window: &Window, cx: &App) -> bool {
        let is_focused = self.focus_handle.is_focused(window);
        let has_my_context_menu = self.context_menu.is_some();
        let has_my_modal = self
            .overlay_manager
            .read(cx)
            .active_modal_belongs_to(&self.focus_handle);
        is_focused || has_my_context_menu || has_my_modal
    }

    pub fn set_overlay_registry(&mut self, reg: Entity<OverlayRegistry>) {
        self.overlay_registry = Some(reg);
    }

    fn current_project_id(&self, cx: &App) -> String {
        self.focus_manager
            .read(cx)
            .focused_terminal_state()
            .map(|s| s.project_id)
            .or_else(|| self.focus_manager.read(cx).active_project_id().cloned())
            .unwrap_or_else(|| "default".to_string())
    }

    pub fn reload_items(&mut self, cx: &mut Context<Self>) {
        let pid = self.current_project_id(cx);
        let query = &self.search_query;
        let query_opt = if query.trim().is_empty() {
            None
        } else {
            Some(query.trim().to_string())
        };

        if let Some(db) = database() {
            let repo = HistoryRepository::new(db);
            if let Ok(entries) = repo.list_by_project(&pid, query_opt.as_deref(), 1000) {
                self.items = entries;
                if let Some(sel) = self.selected_id {
                    if !self.items.iter().any(|it| it.id == sel) {
                        self.selected_id = None;
                    }
                }
                cx.notify();
            }
        }
    }

    fn record_and_execute(&mut self, cmd: &str, cx: &mut Context<Self>) {
        let pid = self.current_project_id(cx);
        let s = settings_entity(cx).read(cx).settings.clone();
        if let Some(db) = database() {
            let repo = HistoryRepository::new(db);
            let _ = repo.record_project_command(
                &pid,
                cmd,
                s.command_history_max_count,
                s.command_history_retention_days,
            );
        }
        send_command_to_focused_terminal(
            &self.focus_manager,
            &self.workspace,
            &self.terminals,
            cmd,
            cx,
        );
        self.reload_items(cx);
    }

    fn show_menu(
        &mut self,
        entry: HistoryEntry,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ctx) = self.context_menu.take() {
            ctx.update(cx, |m, cx| {
                m.set_on_close(None);
                m.close(window, cx);
            });
        }

        let request = CommandHistoryMenuRequest {
            position,
            entry,
        };

        let this_weak = cx.entity().downgrade();
        let this_weak_close = this_weak.clone();
        let menu = open_command_history_context_menu(
            request,
            self.overlay_registry.clone(),
            move |event, cx| {
                if let Some(this) = this_weak.upgrade() {
                    this.update(cx, |this, cx| {
                        this.handle_menu_event(event, cx);
                    });
                }
            },
            window,
            cx,
        );

        let menu_id = menu.entity_id();
        let origin = self.focus_handle.clone();
        let on_close = Arc::new(move |window: &mut Window, cx: &mut App| {
            if let Some(this) = this_weak_close.upgrade() {
                this.update(cx, |this, cx| {
                    if this.context_menu.as_ref().map(|m| m.entity_id()) == Some(menu_id) {
                        this.context_menu = None;
                        if window.focused(cx).is_none() {
                            window.focus(&origin, cx);
                        }
                        cx.notify();
                    }
                });
            }
        });
        menu.update(cx, |m, _| m.set_on_close(Some(on_close)));

        self.context_menu = Some(menu);
        cx.notify();
    }

    fn handle_menu_event(
        &mut self,
        event: CommandHistoryContextMenuEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            CommandHistoryContextMenuEvent::Close => {
                cx.notify();
            }
            CommandHistoryContextMenuEvent::Execute { entry } => {
                self.record_and_execute(&entry.command, cx);
            }
            CommandHistoryContextMenuEvent::SendToCommands { entry } => {
                let cmd = entry.command.clone();
                self.overlay_manager.update(cx, |om, cx| {
                    om.open_commands_panel_with(&cmd, cx);
                });
                cx.notify();
            }
            CommandHistoryContextMenuEvent::AddToQuickCommands { entry } => {
                let cmd = entry.command.clone();
                self.overlay_manager.update(cx, |om, cx| {
                    om.show_quick_command_dialog(
                        QuickCommandDialogMode::CreateCommand {
                            parent_id: None,
                            initial_content: Some(cmd),
                        },
                        cx,
                    );
                });
                cx.notify();
            }
            CommandHistoryContextMenuEvent::Copy { entry } => {
                cx.write_to_clipboard(ClipboardItem::new_string(entry.command));
            }
            CommandHistoryContextMenuEvent::SendToTerminal { entry } => {
                paste_command_to_focused_terminal(
                    &self.focus_manager,
                    &self.workspace,
                    &self.terminals,
                    &entry.command,
                    cx,
                );
            }
            CommandHistoryContextMenuEvent::Delete { entry } => {
                self.prompt_delete_entry(entry, cx);
            }
        }
    }

    fn prompt_delete_entry(&mut self, entry: HistoryEntry, cx: &mut Context<Self>) {
        let entry_id = entry.id;
        let self_entity = cx.entity().clone();
        let origin = self.focus_handle.clone();
        let panel = self.focus_handle.clone();
        let click_origin = self.selected_item_bounds.borrow().map(|b| b.center());
        self.overlay_manager.update(cx, |om, cx| {
            if let Some(pt) = click_origin {
                om.record_click_origin(pt);
            }
            om.show_command_history_delete_confirm_with_origin(
                entry.command.clone(),
                Some(origin),
                Some(panel),
                move |cx| {
                    self_entity.update(cx, |this, cx| {
                        this.delete_entry(entry_id, cx);
                    });
                },
                cx,
            );
        });
    }

    fn prompt_clear_all(&mut self, cx: &mut Context<Self>) {
        let self_entity = cx.entity().clone();
        self.overlay_manager.update(cx, |om, cx| {
            om.show_command_history_clear_confirm(
                move |cx| {
                    self_entity.update(cx, |this, cx| {
                        this.clear_all_history(cx);
                    });
                },
                cx,
            );
        });
    }

    fn delete_entry(&mut self, id: i64, cx: &mut Context<Self>) {
        if let Some(db) = database() {
            let repo = HistoryRepository::new(db);
            let _ = repo.delete_by_id(id);
        }
        if self.selected_id == Some(id) {
            self.selected_id = None;
        }
        self.reload_items(cx);
    }

    fn clear_all_history(&mut self, cx: &mut Context<Self>) {
        let pid = self.current_project_id(cx);
        if let Some(db) = database() {
            let repo = HistoryRepository::new(db);
            let _ = repo.clear_by_project(&pid);
        }
        self.selected_id = None;
        self.reload_items(cx);
    }

    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_search = !self.show_search;
        if !self.show_search {
            self.search_input = None;
            self.search_query.clear();
        } else {
            let input = self.search_input.get_or_insert_with(|| {
                let input = cx.new(|cx| {
                    InputState::new(cx)
                        .placeholder(i18n!(cx, "command_history.search_placeholder"))
                });
                let input_clone = input.clone();
                cx.subscribe(
                    &input_clone,
                    |this: &mut Self, _, _: &velowork_ui::input::InputEvent, cx| {
                        if let Some(inp) = this.search_input.as_ref() {
                            this.search_query = inp.read(cx).text().to_string();
                            this.reload_items(cx);
                        }
                    },
                )
                .detach();
                input
            });
            input.update(cx, |s, cx| {
                s.focus(window, cx);
                s.select_all(cx);
            });
        }
        self.reload_items(cx);
    }

    fn delete_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected_id {
            if let Some(entry) = self.items.iter().find(|it| it.id == id).cloned() {
                self.prompt_delete_entry(entry, cx);
            }
        }
    }
}

impl Panel for CommandHistoryPanel {
    fn metadata(&self, cx: &App) -> PanelInfo {
        PanelInfo::new(
            "command_history",
            i18n!(cx, "command_history.title"),
            AppIcon::MonitorClock,
            PanelKind::Custom,
        )
    }

    fn custom_actions(&self, _cx: &App) -> Vec<PanelAction> {
        Vec::new()
    }

    fn focus_handle(&self, _cx: &App) -> Option<FocusHandle> {
        Some(self.focus_handle.clone())
    }
}

impl Render for CommandHistoryPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);
        let is_panel_active = self.is_selection_active(window, cx);

        if self.show_search && self.search_input.is_none() {
            let input = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder(i18n!(cx, "command_history.search_placeholder"))
            });
            let input_clone = input.clone();
            cx.subscribe(&input_clone, |this: &mut Self, _, _: &velowork_ui::input::InputEvent, cx| {
                if let Some(inp) = this.search_input.as_ref() {
                    this.search_query = inp.read(cx).text().to_string();
                    this.reload_items(cx);
                }
            })
            .detach();
            self.search_input = Some(input);
        }

        let times_suffix = i18n!(cx, "command_history.executed_times");
        let empty_text = i18n!(cx, "command_history.empty_hint");
        let is_empty = self.items.is_empty();
        let ap = velowork_ui::tree::tree_row_appearance(&t, cx);

        let delete_tip = i18n!(cx, "command_history.delete_selected_tooltip");
        let clear_tip = i18n!(cx, "command_history.clear_all_tooltip");
        let search_tip = i18n!(cx, "command_history.search_tooltip");
        let tooltip_time_label = i18n!(cx, "command_history.tooltip_time");
        let tooltip_count_label = i18n!(cx, "command_history.tooltip_count");

        v_flex()
            .id("command-history-panel")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex_col()
            .min_h_0()
            .relative()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let cmd_or_ctrl = event.keystroke.modifiers.platform || event.keystroke.modifiers.control;
                if cmd_or_ctrl && event.keystroke.key.as_str() == "f" {
                    this.show_search = true;
                    let input = this.search_input.get_or_insert_with(|| {
                        let input = cx.new(|cx| {
                            InputState::new(cx)
                                .placeholder(i18n!(cx, "command_history.search_placeholder"))
                        });
                        let input_clone = input.clone();
                        cx.subscribe(
                            &input_clone,
                            |this: &mut Self, _, _: &velowork_ui::input::InputEvent, cx| {
                                if let Some(inp) = this.search_input.as_ref() {
                                    this.search_query = inp.read(cx).text().to_string();
                                    this.reload_items(cx);
                                }
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
                    if this.show_search {
                        this.show_search = false;
                        this.search_query.clear();
                        if let Some(ref input) = this.search_input {
                            input.update(cx, |inp, cx| inp.set_value("", cx));
                        }
                        this.reload_items(cx);
                        window.focus(&this.focus_handle, cx);
                        cx.stop_propagation();
                        cx.notify();
                        return;
                    }
                    if this.selected_id.is_some() {
                        this.selected_id = None;
                        cx.stop_propagation();
                        cx.notify();
                    }
                }

                match event.keystroke.key.as_str() {
                    "down" | "up" => {
                        if !this.focus_handle.is_focused(window) || this.items.is_empty() {
                            return;
                        }
                        let current_idx = this.selected_id.and_then(|id| this.items.iter().position(|it| it.id == id));
                        let new_idx = match current_idx {
                            None => 0,
                            Some(idx) => {
                                if event.keystroke.key == "down" {
                                    (idx + 1).min(this.items.len() - 1)
                                } else {
                                    idx.saturating_sub(1)
                                }
                            }
                        };
                        this.selected_id = Some(this.items[new_idx].id);
                        cx.stop_propagation();
                        cx.notify();
                    }
                    "enter" => {
                        if let Some(id) = this.selected_id {
                            if let Some(entry) = this.items.iter().find(|it| it.id == id).cloned() {
                                this.record_and_execute(&entry.command, cx);
                                cx.stop_propagation();
                            }
                        }
                    }
                    "delete" => {
                        if this.selected_id.is_some() && !this.show_search {
                            this.delete_selected(cx);
                            cx.stop_propagation();
                        }
                    }
                    _ => {}
                }
            }))
            // ── 工具栏 (参考 SSH 隧道面板独立一行) ──
            .child(
                h_flex()
                    .h(px(velowork_ui::tab_height(cx)))
                    .px(ui_space_xs(cx))
                    .border_b_1()
                    .border_color(p.border_subtle)
                    .items_center()
                    .gap(ui_space_xs(cx))
                    .child(
                        velowork_ui::icon_button::icon_button("btn-history-delete", AppIcon::Trash, &t, cx)
                            .tooltip(move |_, cx| {
                                let tip = delete_tip.clone();
                                cx.new(|_| Tooltip::new(tip)).into()
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.delete_selected(cx);
                            })),
                    )
                    .child(
                        velowork_ui::icon_button::icon_button("btn-history-clear", AppIcon::Eraser, &t, cx)
                            .tooltip(move |_, cx| {
                                let tip = clear_tip.clone();
                                cx.new(|_| Tooltip::new(tip)).into()
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.prompt_clear_all(cx);
                            })),
                    )
                    .child(div().w(px(1.0)).h(ICON_STD).bg(p.border_subtle))
                    .child(
                        velowork_ui::icon_button::icon_button("btn-history-search", AppIcon::Search, &t, cx)
                            .when(self.show_search, |b| b.bg(surface_bg_t(t.bg_hover, &t)))
                            .tooltip(move |_, cx| {
                                let tip = search_tip.clone();
                                cx.new(|_| Tooltip::new(tip)).into()
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_search(window, cx);
                            })),
                    ),
            )
            // ── 搜索框 ──
            .when(self.show_search, |d| {
                d.child(
                    div().px(ui_space_xs(cx)).py(ui_space_xs(cx)).when_some(self.search_input.as_ref(), |this, inp| {
                        this.child(velowork_ui::Input::new(inp).search(true))
                    }),
                )
            })
            // ── 历史命令列表区域 ──
            .child(
                div()
                    .relative()
                    .flex_1()
                    .w_full()
                    .overflow_hidden()
                    .child(
                        div()
                            .id("command-history-scroll")
                            .size_full()
                            .track_scroll(&self.scroll_handle)
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .py(ui_space_xs(cx))
                            .px(ui_space_xs(cx))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.selected_id = None;
                                window.focus(&this.focus_handle, cx);
                                cx.notify();
                            }))
                            .when(is_empty, |d| {
                                d.child(velowork_ui::empty_state::empty_state(empty_text, &t, cx))
                            })
                            .children({
                                *self.selected_item_bounds.borrow_mut() = None;
                                self.items.iter().map(|entry| {
                                let entry_clone = entry.clone();
                                let is_selected = is_panel_active && self.selected_id == Some(entry.id);
                                let row_id = format!("hist-row-{}", entry.id);

                                let tip_text = format!(
                                    "{}\n{}: {}\n{}: {} {}",
                                    entry.command,
                                    tooltip_time_label,
                                    format_full_timestamp(&entry.timestamp),
                                    tooltip_count_label,
                                    entry.execution_count,
                                    times_suffix,
                                );

                                h_flex()
                                    .id(ElementId::Name(row_id.into()))
                                    .w_full()
                                    .h(ap.height)
                                    .items_center()
                                    .justify_between()
                                    .px(ui_space_sm(cx))
                                    .rounded(RADIUS_STD)
                                    .gap(ui_space_sm(cx))
                                    .cursor_pointer()
                                    .border_1()
                                    .border_color(with_alpha(0x00000000, 0.0))
                                    .when(is_selected, |d| {
                                        let bounds_slot = self.selected_item_bounds.clone();
                                        d.border_color(rgb(t.border_active)).child(
                                            canvas(
                                                move |bounds, _, _| {
                                                    *bounds_slot.borrow_mut() = Some(bounds);
                                                },
                                                |_, _, _, _| {},
                                            )
                                            .absolute()
                                            .size_full(),
                                        )
                                    })
                                    .bg(if is_selected {
                                        surface_bg_t(t.bg_selection, &t)
                                    } else {
                                        gpui::transparent_black()
                                    })
                                    .hover(|s| {
                                        if !is_selected {
                                            s.bg(surface_bg_t(t.bg_hover, &t))
                                        } else {
                                            s
                                        }
                                    })
                                    .tooltip(move |_, cx| {
                                        let tip = tip_text.clone();
                                        cx.new(|_| Tooltip::new(tip)).into()
                                    })
                                    .on_click(cx.listener({
                                        let e = entry_clone.clone();
                                        move |this, event: &ClickEvent, window, cx| {
                                            window.focus(&this.focus_handle, cx);
                                            cx.stop_propagation();
                                            if event.click_count() == 2 {
                                                this.record_and_execute(&e.command, cx);
                                            } else {
                                                this.selected_id = Some(e.id);
                                                cx.notify();
                                            }
                                        }
                                    }))
                                    .on_mouse_down(MouseButton::Right, {
                                        let e = entry_clone.clone();
                                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                            cx.stop_propagation();
                                            this.selected_id = Some(e.id);
                                            window.focus(&this.focus_handle, cx);
                                            this.show_menu(e.clone(), event.position, window, cx);
                                        })
                                    })
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(ap.font_size)
                                            .text_color(if is_selected {
                                                rgb(t.text_primary)
                                            } else {
                                                rgb(t.text_secondary)
                                            })
                                            .font_family(".SystemMonoFont")
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .child(entry.command.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(ui_text_xs(cx))
                                            .text_color(rgb(t.text_muted))
                                            .flex_shrink_0()
                                            .child(format_short_timestamp(&entry.timestamp)),
                                    )
                                })
                            }),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right_0()
                            .left_0()
                            .child(Scrollbar::vertical(&self.scroll_handle)),
                    ),
            )
            .when_some(self.context_menu.clone(), |d, menu| d.child(menu))
    }
}

fn format_short_timestamp(ts: &str) -> String {
    if ts.len() >= 16 {
        ts[5..16].replace('T', " ")
    } else {
        ts.to_string()
    }
}

fn format_full_timestamp(ts: &str) -> String {
    if ts.len() >= 19 {
        ts[0..19].replace('T', " ")
    } else {
        ts.to_string()
    }
}

impl Focusable for CommandHistoryPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

pub fn register_toolbar_panel(registry: &mut RightToolbarRegistry) {
    registry.register(ToolbarPanelSpec {
        id: "command_history".to_string(),
        icon: AppIcon::MonitorClock,
        title_key: "command_history.title".to_string(),
        order: 25,
        is_visible: std::sync::Arc::new(|_cx| true),
        factory: std::sync::Arc::new(|ctx, _window, cx| {
            let app_ctx = ctx
                .downcast_ref::<AppPanelCreationContext>()
                .expect("AppPanelCreationContext required");
            let workspace = app_ctx.workspace.clone();
            let focus_manager = app_ctx.focus_manager.clone();
            let terminals = app_ctx.terminals.clone();
            let overlay_manager = app_ctx.overlay_manager.clone();
            let overlay_reg = app_ctx.overlay_registry.clone();
            let p = cx.new(|cx| {
                let mut panel = CommandHistoryPanel::new(
                    workspace,
                    focus_manager,
                    terminals,
                    overlay_manager,
                    cx,
                );
                panel.set_overlay_registry(overlay_reg);
                panel
            });

            velowork_ui::dock::AnyPanel::new(p)
        }),
    });
}
