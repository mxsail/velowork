use velowork_terminal::terminal::{Terminal, TerminalTransport};
use velowork_terminal::TerminalsRegistry;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::theme::{theme, surface_bg_t};
use velowork_ui::tokens::{ui_text, ui_text_ms, SPACE_XS, SPACE_SM, SPACE_MD, SPACE_LG, RADIUS_STD};
use velowork_ui::icon::AppIcon;
use velowork_ui::chip::shell_indicator_chip;
use velowork_ui::header_buttons::{header_button_base, HeaderAction};
use velowork_workspace::state::Workspace;
use crate::layout::terminal_pane::{TerminalContent, TerminalContentEvent};
use crate::overlays::terminal_overlay_utils::{
    create_terminal_content, get_or_create_terminal, handle_pending_focus, handle_terminal_key_input,
};
use velowork_i18n::i18n;
use gpui::*;
use velowork_ui::{h_flex, v_flex};
use velowork_ui::decorations::{WindowButtonPosition, WindowDecorationConfig};
use velowork_ui::design::appearance::{ControlAppearance, ControlSize, ControlVariant};
use velowork_ui::overlay_registry::OverlayRegistry;
use crate::layout::layout_container::render_terminal_shared_background;
use crate::overlays::terminal_context_menu::{open_terminal_context_menu, TerminalContextMenuEvent};
use velowork_ui::title_bar::render_window_controls;
use gpui::prelude::FluentBuilder;
use std::sync::Arc;

/// Build terminal context menu./// Detached terminal window view
pub struct DetachedTerminalView {
    workspace: Entity<Workspace>,
    terminal: Arc<Terminal>,
    terminal_id: String,
    focus_handle: FocusHandle,
    pending_focus: bool,
    /// Flag to track if we should close the window
    should_close: bool,
    /// Terminal content view (handles selection, context menu, etc.)
    content: Entity<TerminalContent>,
    overlay_registry: Option<Entity<OverlayRegistry>>,
    active_menu: Option<AnyView>,
}

impl DetachedTerminalView {
    pub fn new(
        workspace: Entity<Workspace>,
        terminal_id: String,
        transport: Arc<dyn TerminalTransport>,
        terminals: TerminalsRegistry,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();

        // Get project info from workspace
        let (project_id, layout_path, project_path) = {
            let ws = workspace.read(cx);
            let mut found_project_id = String::new();
            let mut found_layout_path = vec![];
            let mut found_project_path = String::new();

            for project in ws.projects() {
                if let Some(layout) = &project.layout
                    && let Some(path) = layout.find_terminal_path(&terminal_id) {
                        found_project_id = project.id.clone();
                        found_layout_path = path;
                        found_project_path = project.path.clone();
                        break;
                    }
            }
            (found_project_id, found_layout_path, found_project_path)
        };

        // Get or create terminal from registry
        let terminal = get_or_create_terminal(&terminal_id, &transport, &terminals, &project_path);

        // Create terminal content view
        let content = create_terminal_content(
            cx,
            focus_handle.clone(),
            project_id,
            layout_path,
            workspace.clone(),
            terminal.clone(),
        );

        // Subscribe to right-click context menu request from terminal content
        cx.subscribe(&content, |this, _content, event: &TerminalContentEvent, cx| {
            match event {
                TerminalContentEvent::RequestContextMenu { position, has_selection, link_url } => {
                    this.show_context_menu(*position, *has_selection, link_url.clone(), cx);
                }
            }
        })
        .detach();

        // Observe workspace for changes (to detect when re-attached)
        let terminal_id_for_observer = terminal_id.clone();
        cx.observe(&workspace, move |this, workspace, cx| {
            let ws = workspace.read(cx);
            // Check if terminal is still detached
            let is_still_detached = ws.is_terminal_detached(&terminal_id_for_observer);
            if !is_still_detached && !this.should_close {
                // Terminal was re-attached, close the window
                this.should_close = true;
                cx.notify();
            }
        })
        .detach();

        // Refresh timer - checks terminal dirty flag and notifies only when content changed
        let terminal_for_refresh = terminal.clone();
        cx.spawn(async move |this: WeakEntity<DetachedTerminalView>, cx| {
            loop {
                smol::Timer::after(std::time::Duration::from_millis(8)).await; // ~120fps check rate

                // Only notify if terminal has new content
                if terminal_for_refresh.take_dirty() {
                    let should_continue = this.update(cx, |this, cx| {
                        if this.should_close {
                            return false;
                        }
                        cx.notify();
                        true
                    });
                    match should_continue {
                        Ok(true) => continue,
                        _ => break,
                    }
                } else {
                    // Check if view still exists
                    let should_continue = this.update(cx, |this, _| !this.should_close);
                    match should_continue {
                        Ok(true) => continue,
                        _ => break,
                    }
                }
            }
        })
        .detach();

        Self {
            workspace,
            terminal,
            terminal_id,
            focus_handle,
            pending_focus: true,
            should_close: false,
            content,
            overlay_registry: None,
            active_menu: None,
        }
    }

    pub fn set_overlay_registry(&mut self, registry: Entity<OverlayRegistry>) {
        self.overlay_registry = Some(registry);
    }

    fn show_context_menu(
        &mut self,
        position: Point<Pixels>,
        has_selection: bool,
        link_url: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let (project_id, layout_path) = {
            let ws = self.workspace.read(cx);
            let mut pid = String::new();
            let mut path = vec![];
            for p in ws.projects() {
                if let Some(layout) = &p.layout
                    && let Some(p_path) = layout.find_terminal_path(&self.terminal_id)
                {
                    pid = p.id.clone();
                    path = p_path;
                    break;
                }
            }
            (pid, path)
        };

        let this_weak = cx.entity().downgrade();
        let settings = velowork_app_core::settings::settings_entity(cx).read(cx).settings.clone();
        let ai_enabled = settings.ai_enabled;
        let search_engines: Vec<velowork_workspace::settings::SearchEngineConfig> = settings
            .search_engines
            .iter()
            .filter(|e| e.enabled)
            .cloned()
            .collect();
        let selection = self.terminal.get_selected_text().unwrap_or_default();
        let menu = open_terminal_context_menu(
            self.terminal_id.clone(),
            project_id,
            layout_path,
            position,
            has_selection,
            selection,
            link_url,
            false,
            false,
            false,
            false,
            ai_enabled,
            search_engines,
            self.overlay_registry.clone(),
            move |event, cx| {
                if let Some(this) = this_weak.upgrade() {
                    this.update(cx, |this, cx| {
                        match event {
                            TerminalContextMenuEvent::Close => {
                                this.active_menu = None;
                                cx.notify();
                            }
                            TerminalContextMenuEvent::Copy { .. } => {
                                if let Some(text) = this.terminal.get_selected_text() {
                                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                                }
                                this.active_menu = None;
                                cx.notify();
                            }
                            TerminalContextMenuEvent::Paste { .. } => {
                                if let Some(item) = cx.read_from_clipboard() {
                                    if let Some(text) = item.text() {
                                        this.terminal.send_paste(&text);
                                    }
                                }
                                this.active_menu = None;
                                cx.notify();
                            }
                            TerminalContextMenuEvent::SelectAll { .. } => {
                                this.terminal.select_all();
                                this.active_menu = None;
                                cx.notify();
                            }
                            TerminalContextMenuEvent::Clear { .. } | TerminalContextMenuEvent::ClearScrollback { .. } => {
                                this.terminal.process_output(b"\x1b[3J");
                                this.terminal.scroll_to_bottom();
                                this.active_menu = None;
                                cx.notify();
                            }
                            TerminalContextMenuEvent::SearchWithEngine { url, .. } => {
                                // URL is already fully built (with encoded query) at
                                // menu-open time. Open it directly.
                                crate::layout::terminal_pane::url_detector::UrlDetector::open_url(&url);
                                this.active_menu = None;
                                cx.notify();
                            }
                            TerminalContextMenuEvent::ZmodemUpload { .. } => {
                                this.terminal.send_input("rz\r");
                                this.active_menu = None;
                                cx.notify();
                            }
                            _ => {
                                this.active_menu = None;
                                cx.notify();
                            }
                        }
                    });
                }
            },
            cx,
        );

        self.active_menu = Some(menu.into());
        cx.notify();
    }

    fn handle_key(&mut self, event: &KeyDownEvent, _cx: &mut Context<Self>) {
        // Forward keys to terminal
        handle_terminal_key_input(&self.terminal, event);
    }

    fn handle_reattach(&mut self, cx: &mut Context<Self>) {
        let terminal_id = self.terminal_id.clone();
        self.workspace.update(cx, |ws, cx| {
            ws.attach_terminal(&terminal_id, cx);
        });
    }
}

pub fn resolve_ssh_connection_name(shell_type: &velowork_terminal::shell_config::ShellType, cx: &App) -> Option<String> {
    let velowork_terminal::shell_config::ShellType::Custom { path, args } = shell_type else {
        return None;
    };
    if path != "ssh" {
        return None;
    }

    let mut session_id = None;
    let mut host_arg = None;
    let mut i = 0;
    while i < args.len() {
        if (args[i] == "--id" || args[i] == "--session-id") && i + 1 < args.len() {
            session_id = Some(args[i + 1].clone());
            i += 2;
        } else if (args[i] == "-p" || args[i] == "-i") && i + 1 < args.len() {
            i += 2;
        } else if !args[i].starts_with('-') {
            host_arg = Some(args[i].clone());
            i += 1;
        } else {
            i += 1;
        }
    }

    if let Some(sid) = session_id {
        if let Some(store) = cx.try_global::<velowork_workspace::stores::GlobalSessionStore>() {
            if let Some(session) = store.0.read(cx).find_session(&sid) {
                if !session.name.is_empty() {
                    return Some(session.name.clone());
                }
            }
        }
    }
    host_arg
}

impl Render for DetachedTerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Close window when terminal is re-attached
        if self.should_close {
            window.remove_window();
            // Return empty element while closing
            return div().into_any_element();
        }

        handle_pending_focus(&mut self.pending_focus, &self.focus_handle, window, cx);

        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let focus_handle = self.focus_handle.clone();

        let (is_custom_titlebar, window_corner_radius) = if let Some(global) = cx.try_global::<velowork_app_core::settings::GlobalSettings>() {
            let custom = if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
                global.0.read(cx).settings.titlebar_style == velowork_workspace::settings::TitlebarStyle::Custom
            } else {
                matches!(window.window_decorations(), Decorations::Client { .. })
            };
            (
                custom,
                global.0.read(cx).settings.window_corner_radius,
            )
        } else {
            (
                if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
                    true
                } else {
                    matches!(window.window_decorations(), Decorations::Client { .. })
                },
                8.0,
            )
        };

        let is_maximized = window.is_maximized();
        let is_fullscreen = window.is_fullscreen();
        let has_rounded_corners = is_custom_titlebar && !is_maximized && !is_fullscreen && window_corner_radius > 0.0;
        let corner_r = px(if has_rounded_corners { window_corner_radius } else { 0.0 });

        let (terminal_name, shell_type, is_ssh) = {
            let ws = self.workspace.read(cx);
            let mut resolved_name = None;
            let mut st = velowork_terminal::shell_config::ShellType::Default;
            for p in ws.projects() {
                if let Some(layout) = &p.layout
                    && let Some(path) = layout.find_terminal_path(&self.terminal_id)
                    && let Some(node) = layout.get_at_path(&path)
                    && let velowork_workspace::state::LayoutNode::Terminal { shell_type, .. } = node
                {
                    st = shell_type.clone();
                    if shell_type.is_remote() {
                        resolved_name = Some(shell_type.local_shell_name());
                        break;
                    }
                    if let Some(custom_name) = p.terminal_names.get(&self.terminal_id) {
                        resolved_name = Some(custom_name.clone());
                        break;
                    }
                    if !shell_type.is_remote() {
                        resolved_name = Some(shell_type.local_shell_name());
                        break;
                    }
                    let osc_title = self.terminal.title();
                    resolved_name = Some(p.terminal_display_name(&self.terminal_id, osc_title));
                    break;
                }
            }
            let is_remote = st.is_remote();
            (
                resolved_name.unwrap_or_else(|| {
                    self.terminal.title().unwrap_or_else(|| "Terminal".to_string())
                }),
                st,
                is_remote,
            )
        };

        let needs_controls = is_custom_titlebar && velowork_ui::overlay::detached_needs_controls(window);

        let _traffic_light_padding = if cfg!(target_os = "macos") && !needs_controls {
            px(80.0)
        } else {
            px(12.0)
        };

        let _action_ap = ControlAppearance::resolve(
            ControlSize::Compact,
            ControlVariant::Ghost,
            &p,
            velowork_ui::tokens::get_ui_density(cx),
            velowork_ui::tokens::ui_text_scale(cx),
        );

        let scale = velowork_ui::tokens::ui_scale_factor(cx);

        let app_settings = cx.try_global::<velowork_app_core::settings::GlobalSettings>()
            .map(|g| g.0.read(cx).settings.clone());

        let titlebar_preset = app_settings.as_ref().map(|s| s.titlebar_preset).unwrap_or_default();
        let titlebar_position = app_settings.as_ref().map(|s| s.titlebar_position).unwrap_or_default();
        let titlebar_gap = app_settings.as_ref().map(|s| s.window_control_button_gap).unwrap_or_else(|| velowork_ui::decorations::get_window_control_button_gap(cx));
        let titlebar_margin = app_settings.as_ref().map(|s| s.window_control_margin).unwrap_or_else(|| velowork_ui::decorations::get_window_control_margin(cx));
        let titlebar_height_val = app_settings.as_ref().map(|s| s.titlebar_height).unwrap_or_else(|| velowork_ui::decorations::get_titlebar_height(cx));
        let titlebar_icon_sz = app_settings.as_ref().map(|s| s.window_control_icon_size).unwrap_or_else(|| velowork_ui::decorations::get_window_control_icon_size(cx));

        let style_override = match titlebar_preset {
            velowork_workspace::settings::CustomTitlebarPreset::Auto => None,
            velowork_workspace::settings::CustomTitlebarPreset::MacOS => Some(velowork_ui::decorations::WindowControlStyle::MacOS),
            velowork_workspace::settings::CustomTitlebarPreset::Windows11 => Some(velowork_ui::decorations::WindowControlStyle::Windows11),
            velowork_workspace::settings::CustomTitlebarPreset::LinuxCSD => Some(velowork_ui::decorations::WindowControlStyle::LinuxCSD),
            velowork_workspace::settings::CustomTitlebarPreset::KDEBreeze => Some(velowork_ui::decorations::WindowControlStyle::KDEBreeze),
        };
        let pos_override = match titlebar_position {
            velowork_workspace::settings::CustomTitlebarPosition::Auto => None,
            velowork_workspace::settings::CustomTitlebarPosition::Left => Some(velowork_ui::decorations::WindowButtonPosition::Left),
            velowork_workspace::settings::CustomTitlebarPosition::Right => Some(velowork_ui::decorations::WindowButtonPosition::Right),
        };

        let decoration_config = WindowDecorationConfig::from_custom(
            style_override,
            pos_override,
            Some(titlebar_gap),
            Some(titlebar_margin),
        );

        let is_left_controls = decoration_config.position == WindowButtonPosition::Left;
        let is_right_controls = decoration_config.position == WindowButtonPosition::Right;

        let custom_margin_px = titlebar_margin * scale;
        let left_padding = if is_left_controls {
            px(custom_margin_px)
        } else if cfg!(target_os = "macos") && !needs_controls {
            px(80.0 * scale)
        } else {
            px(12.0 * scale)
        };
        let right_padding = if is_right_controls {
            px(custom_margin_px)
        } else {
            SPACE_LG
        };

        let left_controls = if needs_controls && is_left_controls {
            Some(render_window_controls(
                "detached-term-ctrl-left",
                window,
                &decoration_config,
                Some(titlebar_icon_sz),
                None,
                &t,
                cx,
            ))
        } else {
            None
        };

        let right_controls = if needs_controls && is_right_controls {
            Some(render_window_controls(
                "detached-term-ctrl-right",
                window,
                &decoration_config,
                Some(titlebar_icon_sz),
                None,
                &t,
                cx,
            ))
        } else {
            None
        };

        // 1. Window Header Bar (Draggable window title, re-attach button, CSD window controls)
        let top_titlebar = if is_custom_titlebar {
            div()
                .h(px(titlebar_height_val * scale))
                .pl(left_padding)
                .pr(right_padding)
                .flex()
                .items_center()
                .justify_between()
                .bg(rgb(t.bg_header))
                .border_b_1()
                .border_color(p.border_subtle)
                .when(has_rounded_corners, |d| d.rounded_t(corner_r))
                .window_control_area(WindowControlArea::Drag)
                .when(cfg!(target_os = "linux"), |d| {
                    d.on_mouse_down(MouseButton::Left, |_, window, _cx| {
                        window.start_window_move();
                    })
                })
                .child(
                    h_flex()
                        .gap(SPACE_SM)
                        .items_center()
                        .children(left_controls)
                        .child(
                            if is_ssh {
                                AppIcon::Server.size(px(14.0)).text_color(rgb(t.accent))
                            } else {
                                AppIcon::Terminal.size(px(14.0)).text_color(p.text_secondary)
                            }
                        )
                        .child(
                            div()
                                .text_size(ui_text(13.0, cx))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.text_primary)
                                .child(terminal_name.clone()),
                        )
                        .child(
                            div()
                                .px(SPACE_XS)
                                .py(px(1.0))
                                .rounded(px(4.0))
                                .bg(surface_bg_t(t.bg_selection, &t))
                                .text_size(ui_text_ms(cx))
                                .text_color(rgb(t.accent))
                                .child(i18n!(cx, "terminal.detached")),
                        )
                )
                .child(
                    h_flex()
                        .gap(SPACE_MD)
                        .items_center()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .child({
                            let action_ap = ControlAppearance::resolve(
                                ControlSize::Compact,
                                ControlVariant::Ghost,
                                &p,
                                velowork_ui::tokens::get_ui_density(cx),
                                velowork_ui::tokens::ui_text_scale(cx),
                            );
                            let action_pad_x = px((f32::from(action_ap.height) - f32::from(action_ap.icon_size)) / 2.0);
                            let reattach_tip = i18n!(cx, "dock.panel.attach");
                            div()
                                .id("reattach-btn")
                                .group("reattach-btn")
                                .cursor_pointer()
                                .h(action_ap.height)
                                .px(action_pad_x)
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(action_ap.radius)
                                .hover(move |s| s.bg(p.surface_hover))
                                .child(
                                    AppIcon::Attach
                                        .size(action_ap.icon_size)
                                        .text_color(p.text_secondary)
                                        .group_hover("reattach-btn", |s| s.text_color(p.text_primary)),
                                )
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.handle_reattach(cx);
                                }))
                                .tooltip(move |_, cx| {
                                    cx.new(|_| velowork_ui::tooltip::Tooltip::new(reattach_tip.clone()))
                                        .into()
                                })
                        })
                        .when_some(right_controls, |d, controls| d.child(controls)),
                )
                .into_any_element()
        } else {
            div().into_any_element()
        };

        // 2. Terminal Tab Bar & Action Toolbar

        let tab_toolbar = div()
            .h(px(34.0))
            .px(SPACE_MD)
            .flex()
            .items_center()
            .justify_between()
            .bg(surface_bg_t(t.bg_panel, &t))
            .border_b_1()
            .border_color(p.border_subtle)
            .child(
                h_flex()
                    .gap(SPACE_SM)
                    .items_center()
                    .child(
                        h_flex()
                            .h(px(26.0))
                            .px(SPACE_MD)
                            .gap(SPACE_XS)
                            .items_center()
                            .rounded(RADIUS_STD)
                            .bg(p.surface_base)
                            .border_1()
                            .border_color(rgb(t.accent))
                            .child(
                                if is_ssh {
                                    AppIcon::Server.size(px(13.0)).text_color(rgb(t.accent))
                                } else {
                                    AppIcon::Terminal.size(px(13.0)).text_color(p.text_primary)
                                }
                            )
                            .child(
                                div()
                                    .text_size(ui_text(12.0, cx))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.text_primary)
                                    .child(terminal_name.clone()),
                            )
                    )
                    .child(shell_indicator_chip("detached-shell", shell_type.local_shell_name(), &t, cx))
                    .child(
                        header_button_base(HeaderAction::AddTab, "detached-add-tab", &t, None, None, cx)
                            .on_click(cx.listener(|this, _, window, cx| {
                                let ws_add_tab = this.workspace.clone();
                                this.handle_reattach(cx);
                                ws_add_tab.update(cx, |ws, cx| {
                                    if let Some(p) = ws.projects().first() {
                                        let mut fm = velowork_workspace::focus::FocusManager::new();
                                        ws.add_terminal(&mut fm, &p.id.clone(), cx);
                                    }
                                });
                                window.activate_window();
                            }))
                    )
            );

        v_flex()
            .id("detached-terminal-main")
            .track_focus(&focus_handle)
            .key_context("DetachedTerminal")
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                window.focus(&this.focus_handle, cx);
            }))
            .on_key_down(cx.listener(|this, event, _window, cx| {
                this.handle_key(event, cx);
            }))
            .size_full()
            .bg(p.surface_base)
            .when(has_rounded_corners, |d| d.rounded(corner_r).overflow_hidden())
            .child(top_titlebar)
            .child(tab_toolbar)
            .child(
                // Terminal content (reuses TerminalContent for selection, context menu, etc.)
                div()
                    .relative()
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .child(render_terminal_shared_background(cx, 0.0, false))
                    .child(AnyView::from(self.content.clone()).cached(
                        StyleRefinement::default().size_full()
                    )),
            )
            .when_some(self.active_menu.clone(), |this, menu| this.child(menu))
            .on_click(cx.listener(|this, _, window, cx| {
                window.focus(&this.focus_handle, cx);
            }))
            .into_any_element()
    }
}

impl gpui::Focusable for DetachedTerminalView {
    fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}
