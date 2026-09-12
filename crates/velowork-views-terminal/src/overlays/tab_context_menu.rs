//! Context menu for tab bar (right-click on a tab).

use crate::actions::Cancel;
use velowork_ui::icon::AppIcon;
use velowork_ui::theme::theme;
use velowork_ui::menu::{context_menu_panel, menu_item, menu_item_with_shortcut, menu_item_disabled, menu_item_disabled_with_shortcut, menu_separator};
use velowork_i18n::i18n;
use gpui::prelude::*;
use gpui::*;

/// Event emitted by TabContextMenu
pub enum TabContextMenuEvent {
    Close,
    DuplicateSession { project_id: String, layout_path: Vec<usize>, tab_index: usize },
    DuplicateChannel { project_id: String, layout_path: Vec<usize>, tab_index: usize },
    Reconnect { project_id: String, layout_path: Vec<usize>, tab_index: usize },
    SplitHorizontal { project_id: String, layout_path: Vec<usize>, tab_index: usize },
    SplitVertical { project_id: String, layout_path: Vec<usize>, tab_index: usize },
    CloseTab { project_id: String, layout_path: Vec<usize>, tab_index: usize },
    CloseTabsToRight { project_id: String, layout_path: Vec<usize>, tab_index: usize },
    CloseOtherTabs { project_id: String, layout_path: Vec<usize>, tab_index: usize },
    CloseInactiveTabs { project_id: String, layout_path: Vec<usize> },
    SessionSettings { project_id: String, layout_path: Vec<usize>, tab_index: usize },
    ToggleMinimize { project_id: String, layout_path: Vec<usize>, tab_index: usize },
}

/// Context menu for tab bar
pub struct TabContextMenu {
    tab_index: usize,
    num_tabs: usize,
    project_id: String,
    layout_path: Vec<usize>,
    position: Point<Pixels>,
    is_ssh: bool,
    is_minimized: bool,
    minimize_shortcut: Option<SharedString>,
    focus_handle: FocusHandle,
}

impl TabContextMenu {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tab_index: usize,
        num_tabs: usize,
        project_id: String,
        layout_path: Vec<usize>,
        position: Point<Pixels>,
        is_ssh: bool,
        is_minimized: bool,
        minimize_shortcut: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        Self {
            tab_index,
            num_tabs,
            project_id,
            layout_path,
            position,
            is_ssh,
            is_minimized,
            minimize_shortcut,
            focus_handle,
        }
    }

    fn close(&self, cx: &mut Context<Self>) {
        cx.emit(TabContextMenuEvent::Close);
    }
}

impl EventEmitter<TabContextMenuEvent> for TabContextMenu {}

impl Render for TabContextMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);

        // Focus on first render
        if !self.focus_handle.is_focused(window) {
            window.focus(&self.focus_handle, cx);
        }

        let position = self.position;
        let has_other_tabs = self.num_tabs > 1;
        let has_tabs_to_right = self.tab_index < self.num_tabs.saturating_sub(1);

        let duplicate_session = i18n!(cx, "terminal.duplicate_session");
        let duplicate_channel = i18n!(cx, "terminal.duplicate_channel");
        let reconnect = i18n!(cx, "common.reconnect");
        let split_h = i18n!(cx, "terminal.split_horizontal");
        let split_v = i18n!(cx, "terminal.split_vertical");
        let minimize_tab = i18n!(cx, "terminal.minimize_tab");
        let restore_tab = i18n!(cx, "terminal.restore_tab");
        let close_current = i18n!(cx, "terminal.close_current_view");
        let close_to_right = i18n!(cx, "terminal.close_right_views");
        let close_others = i18n!(cx, "terminal.close_other_views");
        let close_inactive = i18n!(cx, "terminal.close_inactive_views");
        let session_settings = i18n!(cx, "terminal.session_settings");

        let dup_session_sc: SharedString = if cfg!(target_os = "macos") { "⌘⌥S" } else { "Ctrl+Alt+S" }.into();
        let dup_channel_sc: SharedString = if cfg!(target_os = "macos") { "⌘⌥C" } else { "Ctrl+Alt+C" }.into();
        let reconnect_sc: SharedString = if cfg!(target_os = "macos") { "⌘⌥R" } else { "Ctrl+Alt+R" }.into();
        let split_h_sc: SharedString = if cfg!(target_os = "macos") { "⌘⇧D" } else { "Ctrl+D" }.into();
        let split_v_sc: SharedString = if cfg!(target_os = "macos") { "⌘D" } else { "Ctrl+Shift+D" }.into();
        let close_current_sc: SharedString = if cfg!(target_os = "macos") { "⌘W" } else { "Ctrl+Shift+W" }.into();

        let panel = if self.is_minimized {
            context_menu_panel("tab-context-menu", &t, cx)
                // 置顶核心操作：还原标签页
                .child(
                    menu_item_with_shortcut(
                        "tab-ctx-restore",
                        AppIcon::ChevronUp,
                        &restore_tab,
                        self.minimize_shortcut.clone(),
                        &t,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        cx.emit(TabContextMenuEvent::ToggleMinimize {
                            project_id: this.project_id.clone(),
                            layout_path: this.layout_path.clone(),
                            tab_index: this.tab_index,
                        });
                    })),
                )
                .child(menu_separator(&t))
                // 常用会话操作：重新连接, 复制会话
                .child(
                    menu_item_with_shortcut(
                        "tab-ctx-reconnect",
                        AppIcon::Refresh,
                        &reconnect,
                        Some(reconnect_sc),
                        &t,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        cx.emit(TabContextMenuEvent::Reconnect {
                            project_id: this.project_id.clone(),
                            layout_path: this.layout_path.clone(),
                            tab_index: this.tab_index,
                        });
                    })),
                )
                .child(
                    menu_item_with_shortcut(
                        "tab-ctx-dup-session",
                        AppIcon::Copy,
                        &duplicate_session,
                        Some(dup_session_sc),
                        &t,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        cx.emit(TabContextMenuEvent::DuplicateSession {
                            project_id: this.project_id.clone(),
                            layout_path: this.layout_path.clone(),
                            tab_index: this.tab_index,
                        });
                    })),
                )
                .child(menu_separator(&t))
                // 关闭标签页
                .child(
                    menu_item_with_shortcut(
                        "tab-ctx-close-current",
                        AppIcon::Close,
                        &close_current,
                        Some(close_current_sc),
                        &t,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        cx.emit(TabContextMenuEvent::CloseTab {
                            project_id: this.project_id.clone(),
                            layout_path: this.layout_path.clone(),
                            tab_index: this.tab_index,
                        });
                    })),
                )
                .child(menu_separator(&t))
                // 会话设置
                .child(
                    menu_item("tab-ctx-session-settings", AppIcon::Settings, &session_settings, &t, cx)
                        .on_click(cx.listener(|this, _, _window, cx| {
                            cx.emit(TabContextMenuEvent::SessionSettings {
                                project_id: this.project_id.clone(),
                                layout_path: this.layout_path.clone(),
                                tab_index: this.tab_index,
                            });
                        })),
                )
        } else {
            context_menu_panel("tab-context-menu", &t, cx)
                // Group 1: 复制会话, 复制渠道, 重新连接
                .child(
                    menu_item_with_shortcut(
                        "tab-ctx-dup-session",
                        AppIcon::Copy,
                        &duplicate_session,
                        Some(dup_session_sc),
                        &t,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        cx.emit(TabContextMenuEvent::DuplicateSession {
                            project_id: this.project_id.clone(),
                            layout_path: this.layout_path.clone(),
                            tab_index: this.tab_index,
                        });
                    })),
                )
                .child(if self.is_ssh {
                    menu_item_with_shortcut(
                        "tab-ctx-dup-channel",
                        AppIcon::Transfer,
                        &duplicate_channel,
                        Some(dup_channel_sc.clone()),
                        &t,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        cx.emit(TabContextMenuEvent::DuplicateChannel {
                            project_id: this.project_id.clone(),
                            layout_path: this.layout_path.clone(),
                            tab_index: this.tab_index,
                        });
                    }))
                } else {
                    menu_item_disabled_with_shortcut("tab-ctx-dup-channel", AppIcon::Transfer, &duplicate_channel, Some(dup_channel_sc), &t, cx)
                })
                .child(
                    menu_item_with_shortcut(
                        "tab-ctx-reconnect",
                        AppIcon::Refresh,
                        &reconnect,
                        Some(reconnect_sc),
                        &t,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        cx.emit(TabContextMenuEvent::Reconnect {
                            project_id: this.project_id.clone(),
                            layout_path: this.layout_path.clone(),
                            tab_index: this.tab_index,
                        });
                    })),
                )
                .child(menu_separator(&t))
                // Group 2: 水平分屏, 垂直分屏, 最小化标签页
                .child(
                    menu_item_with_shortcut(
                        "tab-ctx-split-h",
                        AppIcon::SplitHorizontal,
                        &split_h,
                        Some(split_h_sc),
                        &t,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        cx.emit(TabContextMenuEvent::SplitHorizontal {
                            project_id: this.project_id.clone(),
                            layout_path: this.layout_path.clone(),
                            tab_index: this.tab_index,
                        });
                    })),
                )
                .child(
                    menu_item_with_shortcut(
                        "tab-ctx-split-v",
                        AppIcon::SplitVertical,
                        &split_v,
                        Some(split_v_sc),
                        &t,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        cx.emit(TabContextMenuEvent::SplitVertical {
                            project_id: this.project_id.clone(),
                            layout_path: this.layout_path.clone(),
                            tab_index: this.tab_index,
                        });
                    })),
                )
                .child(
                    menu_item_with_shortcut(
                        "tab-ctx-minimize",
                        AppIcon::Minimize,
                        &minimize_tab,
                        self.minimize_shortcut.clone(),
                        &t,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        cx.emit(TabContextMenuEvent::ToggleMinimize {
                            project_id: this.project_id.clone(),
                            layout_path: this.layout_path.clone(),
                            tab_index: this.tab_index,
                        });
                    })),
                )
                .child(menu_separator(&t))
                // Group 3: 关闭当前标签页, 关闭右侧标签页, 关闭其他标签页, 关闭已断开标签页
                .child(
                    menu_item_with_shortcut(
                        "tab-ctx-close-current",
                        AppIcon::Close,
                        &close_current,
                        Some(close_current_sc),
                        &t,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        cx.emit(TabContextMenuEvent::CloseTab {
                            project_id: this.project_id.clone(),
                            layout_path: this.layout_path.clone(),
                            tab_index: this.tab_index,
                        });
                    })),
                )
                .child(if has_tabs_to_right {
                    menu_item("tab-ctx-close-to-right", AppIcon::ChevronRight, &close_to_right, &t, cx)
                        .on_click(cx.listener(|this, _, _window, cx| {
                            cx.emit(TabContextMenuEvent::CloseTabsToRight {
                                project_id: this.project_id.clone(),
                                layout_path: this.layout_path.clone(),
                                tab_index: this.tab_index,
                            });
                        }))
                } else {
                    menu_item_disabled("tab-ctx-close-to-right", AppIcon::ChevronRight, &close_to_right, &t, cx)
                })
                .child(if has_other_tabs {
                    menu_item("tab-ctx-close-others", AppIcon::Close, &close_others, &t, cx)
                        .on_click(cx.listener(|this, _, _window, cx| {
                            cx.emit(TabContextMenuEvent::CloseOtherTabs {
                                project_id: this.project_id.clone(),
                                layout_path: this.layout_path.clone(),
                                tab_index: this.tab_index,
                            });
                        }))
                } else {
                    menu_item_disabled("tab-ctx-close-others", AppIcon::Close, &close_others, &t, cx)
                })
                .child(
                    menu_item("tab-ctx-close-inactive", AppIcon::Trash, &close_inactive, &t, cx)
                        .on_click(cx.listener(|this, _, _window, cx| {
                            cx.emit(TabContextMenuEvent::CloseInactiveTabs {
                                project_id: this.project_id.clone(),
                                layout_path: this.layout_path.clone(),
                            });
                        })),
                )
                .child(menu_separator(&t))
                // Group 4: 会话设置
                .child(
                    menu_item("tab-ctx-session-settings", AppIcon::Settings, &session_settings, &t, cx)
                        .on_click(cx.listener(|this, _, _window, cx| {
                            cx.emit(TabContextMenuEvent::SessionSettings {
                                project_id: this.project_id.clone(),
                                layout_path: this.layout_path.clone(),
                                tab_index: this.tab_index,
                            });
                        })),
                )
        };

        div()
            .track_focus(&self.focus_handle)
            .key_context("TabContextMenu")
            .on_action(cx.listener(|this, _: &Cancel, _window, cx| {
                this.close(cx);
            }))
            .absolute()
            .inset_0()
            .id("tab-context-menu-backdrop")
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _window, cx| {
                this.close(cx);
            }))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, _, _window, cx| {
                this.close(cx);
            }))
            .child(deferred(
                anchored()
                    .position(position)
                    .snap_to_window()
                    .child(panel),
            ))
    }
}

impl gpui::Focusable for TabContextMenu {
    fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}

