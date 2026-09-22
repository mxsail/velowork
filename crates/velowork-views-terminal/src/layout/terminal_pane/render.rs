//! Render implementation for TerminalPane.

use crate::ActionDispatch;
use velowork_core::api::ActionRequest;
use crate::actions::{
    AddTab, CloseSearch, CloseTerminal, Copy, DuplicateChannel, DuplicateSession, FocusDown,
    FocusLeft, FocusNextTerminal, FocusPrevTerminal, FocusRight, FocusUp, JumpToNextPrompt,
    JumpToPreviousPrompt, MinimizeTerminal, Paste, ReconnectTerminal, ResetZoom, Search,
    SearchNext, SearchPrev, SendBacktab, SendEscape, SendTab, SplitHorizontal, SplitVertical,
    ToggleFullscreen, ZoomIn, ZoomOut,
};
use velowork_i18n::i18n;
use crate::terminal_view_settings;
use velowork_ui::theme::theme;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::icon::AppIcon;
use velowork_ui::tooltip::Tooltip;
use velowork_ui::tokens::{
    SPACE_XS, SPACE_SM, SPACE_MD, RADIUS_CARD, RADIUS_STD, RADIUS_XS, ICON_SM, ui_text_md,
};
use crate::layout::navigation::NavigationDirection;
use velowork_workspace::state::{LayoutNode, SplitDirection};
use gpui::prelude::FluentBuilder;
use gpui::*;
use std::time::Duration;

use super::TerminalPane;

impl<D: ActionDispatch + Send + Sync> Render for TerminalPane<D> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let tvs = terminal_view_settings(cx);

        let default_session_options = velowork_state::SessionTerminalOptions::default();
        let session_id = self
            .terminal_id
            .as_deref()
            .and_then(|tid| self.backend.get_ssh_session_id(tid))
            .or_else(|| match &self.shell_type {
                velowork_core::shell::ShellType::Custom { path, args } if path == "ssh" => {
                    velowork_terminal::pty_manager::parse_ssh_args(args)
                        .and_then(|(_, _, _, _, sid, _)| sid)
                }
                velowork_core::shell::ShellType::Custom { path, args } if path == "serial" => {
                    velowork_terminal::pty_manager::parse_serial_args(args)
                        .and_then(|(_, _, sid)| sid)
                }
                velowork_core::shell::ShellType::Custom { path, args } if path == "telnet" => {
                    velowork_terminal::pty_manager::parse_telnet_args(args)
                        .and_then(|(_, _, sid)| sid)
                }
                velowork_core::shell::ShellType::Custom { path, args } if path == "local" => {
                    velowork_terminal::pty_manager::parse_local_args(args)
                }
                _ => None,
            });

        let session_options = session_id.as_deref().and_then(|sid| {
            cx.try_global::<velowork_workspace::stores::GlobalSessionStore>()
                .and_then(|s| s.0.read(cx).find_session(sid).map(|s| s.terminal.clone()))
        });
        let session_options_ref = session_options.as_ref().unwrap_or(&default_session_options);
        let defaults = tvs.terminal_defaults();
        let resolved_config =
            velowork_terminal::resolve_effective_terminal_config(session_options_ref, &defaults);

        if let Some(ref terminal) = self.terminal {
            terminal.update_bell_config(resolved_config.bell_style, resolved_config.bell_cooldown_ms);
        }

        let is_windowed = !window.is_maximized() && !window.is_fullscreen();
        let is_custom_titlebar = velowork_ui::decorations::is_custom_titlebar(window, cx);
        let corner_radius = velowork_ui::decorations::get_window_corner_radius(cx);
        let has_rounded_corners = is_custom_titlebar && is_windowed && corner_radius > 0.0;
        let is_fullscreen = self.focus_manager.read(cx).has_fullscreen();
        let in_split = self.is_project_split(cx);

        let base_card_r = if is_fullscreen {
            if has_rounded_corners {
                corner_radius
            } else {
                0.0
            }
        } else {
            f32::from(RADIUS_CARD)
        };

        let (bottom_left_r, bottom_right_r) = if is_fullscreen {
            (base_card_r, base_card_r)
        } else if !in_split {
            (base_card_r, base_card_r)
        } else {
            let (on_bl, on_br) = self.check_bottom_corners_in_layout(cx);
            (
                if on_bl { base_card_r } else { 0.0 },
                if on_br { base_card_r } else { 0.0 },
            )
        };

        let is_conn_lost = self.is_connection_lost(cx);
        let resolved_config_clone = resolved_config.clone();
        self.content.update(cx, |content, _| {
            content.set_connection_lost(is_conn_lost);
            content.set_resolved_config(resolved_config_clone);
            content.set_bottom_corner_radii(bottom_left_r, bottom_right_r);
        });

        // Keep the shared background-image cache in sync with the current setting.
        // Idempotent: only (re)decodes when the configured path actually changes.
        if let Some(cache) = crate::terminal_background_cache(cx) {
            let path = tvs.terminal_background_image.clone();
            let blur = tvs.terminal_background_image_blur;
            cache.update(cx, |cache, cx| cache.ensure_loaded(path, blur, cx));
        }

        // Refresh search results if terminal content changed (scroll, new output)
        self.search_bar.update(cx, |bar, cx| bar.refresh_if_needed(cx));
        if self.minimized || self.detached {
            self.deregister_resize_viewer(cx);
        }

        let focus_handle = self.focus_handle.clone();
        let id_suffix = self.id_suffix();

        let search_active = self.search_bar.read(cx).is_active();
        let is_welcome = self.shell_type == velowork_core::shell::ShellType::Welcome;

        let is_focused = if is_welcome {
            self.quick_connect_input.read(cx).focus_handle(cx).is_focused(window)
        } else {
            focus_handle.is_focused(window)
        };

        let has_bell = self.terminal.as_ref().is_some_and(|t| t.has_bell());
        if is_focused && has_bell
            && let Some(ref terminal) = self.terminal {
                terminal.clear_bell();
            }

        let has_notification = self.terminal.as_ref().is_some_and(|t| t.has_notification());
        if is_focused && has_notification
            && let Some(ref terminal) = self.terminal {
                terminal.clear_notification();
            }

        if is_focused
            && let Some(ref terminal) = self.terminal
                && terminal.is_waiting_for_input() {
                    terminal.clear_waiting();
                }

        if self.was_focused && !is_focused
            && let Some(ref terminal) = self.terminal {
                terminal.mark_as_viewed();
            }
        self.was_focused = is_focused;

        let show_focused_border = terminal_view_settings(cx).show_focused_border;
        let is_waiting = !is_focused && self.terminal.as_ref()
            .is_some_and(|t| t.is_waiting_for_input());

        // Focus-loss styling. Two cases:
        // * No split (single terminal): focus loss does nothing special — keep
        //   the previous behavior (border only when focused w/ setting, or for
        //   bell/notification/waiting). No overlay/mask is ever painted.
        // * Split present: the *active* (focused) pane gets an accent border to
        //   mark it, but only when the "show focus border" setting is on; when
        //   off it falls back to the idle border. *Inactive* panes always fall
        //   back to the default idle border (only bell/notification/waiting still
        //   raise attention). This replaces the old unfocused fog mask.
        let in_split = self.is_project_split(cx);
        let border_color;
        let show_border = if in_split {
            if is_focused && show_focused_border {
                border_color = p.status_info;
                true
            } else {
                border_color = p.text_muted;
                has_bell || has_notification || is_waiting
            }
        } else {
            border_color = if is_focused && show_focused_border {
                p.border_active
            } else if has_bell || has_notification {
                p.status_warning
            } else {
                p.text_muted
            };
            (is_focused && show_focused_border) || has_bell || has_notification || is_waiting
        };

        let is_log_recording = self.terminal.as_ref().is_some_and(|t| t.is_log_recording()) || self.log_toolbar_exiting;
        if is_log_recording {
            self.ensure_log_timer(cx);
            if self.log_toolbar_start_time.is_none() && !self.log_toolbar_exiting {
                self.log_toolbar_start_time = Some(std::time::Instant::now());
                let this_entity = cx.entity().downgrade();
                cx.spawn(async move |_, cx| {
                    smol::Timer::after(Duration::from_millis(280)).await;
                    let _ = this_entity.update(cx, |_, cx| cx.notify());
                    smol::Timer::after(Duration::from_millis(160)).await;
                    let _ = this_entity.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
        } else if !self.log_toolbar_exiting {
            self.log_toolbar_start_time = None;
            self.log_toolbar_exit_start_time = None;
        }

        let this_entity_bounds = cx.entity().downgrade();
        let this_window_id = self.window_id;
        let this_project_id = self.project_id.clone();
        let this_layout_path = self.layout_path.clone();
        let this_quick_connect_fh = self.quick_connect_input.read(cx).focus_handle(cx);
        let this_is_welcome = is_welcome;
        let this_focus_handle = self.focus_handle.clone();
        let pane_bounds_tracker = canvas(
            move |bounds, _window, cx| {
                if let Some(entity) = this_entity_bounds.upgrade() {
                    entity.update(cx, |this, _| {
                        this.pane_bounds = Some(bounds);
                    });
                }
                let fh = if this_is_welcome {
                    Some(this_quick_connect_fh.clone())
                } else {
                    Some(this_focus_handle.clone())
                };
                crate::layout::navigation::register_pane_bounds(
                    this_window_id,
                    this_project_id.clone(),
                    this_layout_path.clone(),
                    bounds,
                    fh,
                );
            },
            |_, _, _, _| {},
        );

        // 全屏（zoom）状态不再渲染独立的 zoom 顶栏；保留主 tab bar，
        // 由 tab bar 右侧的“收起全屏”按钮退出全屏。
        div()
            .id(format!("terminal-pane-main-{}", id_suffix))
            .track_focus(&focus_handle)
            .key_context("TerminalPane")
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .min_w_0()
            .group("terminal-pane")
            .relative()
            .overflow_hidden()
            .child(pane_bounds_tracker.absolute().inset_0())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                    if this.shell_type == velowork_core::shell::ShellType::Welcome {
                        this.quick_connect_input.update(cx, |inp, cx| inp.focus(window, cx));
                    } else {
                        window.focus(&this.focus_handle, cx);
                    }
                    let project_id = this.project_id.clone();
                    let layout_path = this.layout_path.clone();
                    let workspace = this.workspace.clone();
                    this.focus_manager.update(cx, |fm, cx| {
                        workspace.update(cx, |ws, cx| {
                            ws.set_focused_terminal(fm, project_id, layout_path, cx);
                        });
                        cx.notify();
                    });
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                if let Some(offset_in_toolbar) = this.log_toolbar_mouse_offset {
                    if let Some(pane_bounds) = this.pane_bounds {
                        let rel_x = event.position.x - offset_in_toolbar.x - pane_bounds.origin.x;
                        let rel_y = event.position.y - offset_in_toolbar.y - pane_bounds.origin.y;
                        let toolbar_w = this.log_toolbar_bounds.map(|b| b.size.width).unwrap_or(px(280.0));
                        let toolbar_h = this.log_toolbar_bounds.map(|b| b.size.height).unwrap_or(px(36.0));

                        let max_x = (pane_bounds.size.width - toolbar_w).max(px(0.0));
                        let max_y = (pane_bounds.size.height - toolbar_h).max(px(0.0));

                        let clamped_x = rel_x.clamp(px(0.0), max_x);
                        let clamped_y = rel_y.clamp(px(0.0), max_y);

                        this.log_toolbar_pos = Some(point(clamped_x, clamped_y));
                        cx.notify();
                    }
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                if this.log_toolbar_mouse_offset.is_some() {
                    this.log_toolbar_mouse_offset = None;
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &SplitVertical, _window, cx| { this.handle_split(SplitDirection::Vertical, cx); }))
            .on_action(cx.listener(|this, _: &SplitHorizontal, _window, cx| { this.handle_split(SplitDirection::Horizontal, cx); }))
            .on_action(cx.listener(|this, _: &AddTab, _window, cx| { this.handle_add_tab(cx); }))
            .on_action(cx.listener(|this, _: &DuplicateSession, _window, cx| { this.handle_duplicate_session(cx); }))
            .on_action(cx.listener(|this, _: &DuplicateChannel, _window, cx| { this.handle_duplicate_channel(cx); }))
            .on_action(cx.listener(|this, _: &ReconnectTerminal, _window, cx| { this.handle_reconnect(cx); }))
            .on_action(cx.listener(|this, _: &CloseTerminal, _window, cx| { this.handle_close(cx); }))
            .on_action(cx.listener(|this, _: &MinimizeTerminal, _window, cx| { this.handle_minimize(cx); }))
            .on_action(cx.listener(|this, _: &Copy, _window, cx| { this.handle_copy(cx); }))
            .on_action(cx.listener(|this, _: &Paste, _window, cx| { this.handle_paste(cx); }))
            .on_action(cx.listener(|this, _: &Search, window, cx| { if !this.search_bar.read(cx).is_active() { this.start_search(window, cx); } }))
            .on_action(cx.listener(|this, _: &CloseSearch, window, cx| { if this.search_bar.read(cx).is_active() { this.close_search(window, cx); } }))
            .on_action(cx.listener(|this, _: &SearchNext, _window, cx| { this.next_match(cx); }))
            .on_action(cx.listener(|this, _: &SearchPrev, _window, cx| { this.prev_match(cx); }))
            .on_action(cx.listener(|this, _: &JumpToPreviousPrompt, _window, cx| { this.handle_jump_prev_prompt(cx); }))
            .on_action(cx.listener(|this, _: &JumpToNextPrompt, _window, cx| { this.handle_jump_next_prompt(cx); }))
            .on_action(cx.listener(|this, _: &FocusLeft, window, cx| { this.handle_navigation(NavigationDirection::Left, window, cx); }))
            .on_action(cx.listener(|this, _: &FocusRight, window, cx| { this.handle_navigation(NavigationDirection::Right, window, cx); }))
            .on_action(cx.listener(|this, _: &FocusUp, window, cx| { this.handle_navigation(NavigationDirection::Up, window, cx); }))
            .on_action(cx.listener(|this, _: &FocusDown, window, cx| { this.handle_navigation(NavigationDirection::Down, window, cx); }))
            .on_action(cx.listener(|this, _: &FocusNextTerminal, window, cx| { this.handle_sequential_navigation(true, window, cx); }))
            .on_action(cx.listener(|this, _: &FocusPrevTerminal, window, cx| { this.handle_sequential_navigation(false, window, cx); }))
            .on_action(cx.listener(|this, _: &SendTab, _window, _cx| { if let Some(ref terminal) = this.terminal { terminal.send_bytes(b"\t"); } }))
            .on_action(cx.listener(|this, _: &SendBacktab, _window, _cx| { if let Some(ref terminal) = this.terminal { terminal.send_bytes(b"\x1b[Z"); } }))
            .on_action(cx.listener(|this, _: &SendEscape, _window, _cx| { if let Some(ref terminal) = this.terminal { terminal.send_bytes(b"\x1b"); } }))
            .on_action(cx.listener(|_this, _: &ZoomIn, _window, cx| {
                let mut tvs = crate::terminal_view_settings(cx).clone();
                let new_font_size = (tvs.font_size + 1.0).clamp(8.0, 48.0);
                if (new_font_size - tvs.font_size).abs() >= 0.01 {
                    tvs.font_size = new_font_size;
                    crate::set_terminal_view_settings(&tvs, cx);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|_this, _: &ZoomOut, _window, cx| {
                let mut tvs = crate::terminal_view_settings(cx).clone();
                let new_font_size = (tvs.font_size - 1.0).clamp(8.0, 48.0);
                if (new_font_size - tvs.font_size).abs() >= 0.01 {
                    tvs.font_size = new_font_size;
                    crate::set_terminal_view_settings(&tvs, cx);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|_this, _: &ResetZoom, _window, cx| {
                let mut tvs = crate::terminal_view_settings(cx).clone();
                tvs.font_size = 14.0;
                crate::set_terminal_view_settings(&tvs, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleFullscreen, _window, cx| {
                let is_fullscreen = this.focus_manager.read(cx).has_fullscreen();
                if is_fullscreen {
                    let action = ActionRequest::SetFullscreen { project_id: this.project_id.clone(), terminal_id: None, window: None };
                    if let Some(ref dispatcher) = this.action_dispatcher { dispatcher.dispatch(action, cx); }
                } else {
                    this.handle_fullscreen(cx);
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.handle_key(event, window, cx);
            }))
            .on_click(cx.listener(|this, _, window, cx| {
                if this.shell_type == velowork_core::shell::ShellType::Welcome {
                    this.quick_connect_input.update(cx, |inp, cx| inp.focus(window, cx));
                } else {
                    window.focus(&this.focus_handle, cx);
                }
                let project_id = this.project_id.clone();
                let layout_path = this.layout_path.clone();
                let workspace = this.workspace.clone();
                this.focus_manager.update(cx, |fm, cx| {
                    fm.request_focus(velowork_workspace::focus::FocusLayer::Terminal);
                    workspace.update(cx, |ws, cx| {
                        ws.set_focused_terminal(fm, project_id, layout_path, cx);
                    });
                    cx.notify();
                });
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| { this.handle_file_drop(paths, cx); }))
            .when(!self.minimized && !self.detached, |el| {
                if self.shell_type == velowork_core::shell::ShellType::Welcome {
                    return el.child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .overflow_hidden()
                            .relative()
                            .child(self.render_welcome_state(window, cx)),
                    );
                }
                el.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .min_w_0()
                        .overflow_hidden()
                        .relative()
                        .child(AnyView::from(self.content.clone()).cached(
                            StyleRefinement::default().size_full()
                        ))
                    .when(self.history_popup_open && !self.history_popup_items.is_empty(), |el| {
                        let cursor_pos = self.content.read(cx).relative_cursor_position(cx);
                        let items = self.history_popup_items.clone();
                        let selected = self.history_popup_selected;
                        let self_entity = cx.entity().clone();
                        el.child(super::history_popup::render_history_popup(
                            &items,
                            selected,
                            cursor_pos,
                            &t,
                            move |idx, cx| {
                                self_entity.update(cx, |this, cx| {
                                    this.select_history_suggestion(idx, cx);
                                });
                            },
                            cx,
                        ))
                    })
                    .when(!self.history_popup_open && self.ghost_text.is_some(), |el| {
                        let cursor_pos = self.content.read(cx).relative_cursor_position(cx);
                        if let (Some(ghost), Some(pos)) = (self.ghost_text.as_ref(), cursor_pos) {
                            let tvs = crate::terminal_view_settings(cx);
                            let (_, cell_h) = self.terminal.as_ref().map(|t| t.cell_dimensions()).unwrap_or((0.0, 0.0));
                            let fallback_h = (tvs.font_size * tvs.line_height).max(12.0);
                            let actual_cell_h = px(if cell_h > 0.0 { cell_h } else { fallback_h });
                            el.child(super::ghost_text::render_ghost_text_overlay(
                                ghost,
                                pos,
                                actual_cell_h,
                                px(tvs.font_size),
                                &tvs.font_family,
                                &t,
                                cx,
                            ))
                        } else {
                            el
                        }
                    })
                    .when(is_conn_lost, |el| {
                        let is_reconnecting = self.is_reconnecting;
                        let conn_lost_text = if is_reconnecting {
                            i18n!(cx, "status.reconnecting")
                        } else {
                            i18n!(cx, "status.connection_lost")
                        };
                        let reconnect_text = if is_reconnecting {
                            i18n!(cx, "status.reconnecting")
                        } else {
                            i18n!(cx, "common.action.reconnect")
                        };
                        let tooltip_text = if is_reconnecting {
                            i18n!(cx, "status.reconnecting")
                        } else {
                            i18n!(cx, "common.action.reconnect")
                        };
                        let banner_border = if is_reconnecting {
                            p.status_warning
                        } else {
                            p.status_error
                        };
                        let button_bg = if is_reconnecting {
                            p.status_warning
                        } else {
                            p.status_error
                        };
                        let tid_str = self.terminal_id.as_deref().unwrap_or("pane");
                        let anim_id = format!("terminal-reconnect-spinner-{}", tid_str);

                        el.child(
                            div()
                                .absolute()
                                .top(SPACE_SM)
                                .right(SPACE_MD)
                                .flex()
                                .items_center()
                                .gap(SPACE_MD)
                                .pl(SPACE_MD)
                                .pr(px(4.0))
                                .py(px(4.0))
                                .rounded(RADIUS_STD)
                                .bg(p.surface_overlay)
                                .border_1()
                                .border_color(banner_border)
                                .shadow_sm()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(SPACE_XS)
                                        .child(
                                            div()
                                                .w(px(7.0))
                                                .h(px(7.0))
                                                .rounded_full()
                                                .bg(banner_border)
                                        )
                                        .child(
                                            div()
                                                .text_size(ui_text_md(cx))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(p.text_primary)
                                                .child(conn_lost_text)
                                        )
                                )
                                .child(
                                    div()
                                        .id(ElementId::Name(format!("btn-reconnect-{}", tid_str).into()))
                                        .flex()
                                        .items_center()
                                        .gap(SPACE_XS)
                                        .px(SPACE_SM)
                                        .py(px(4.0))
                                        .rounded(RADIUS_XS)
                                        .bg(button_bg)
                                        .when(!is_reconnecting, |s| {
                                            s.hover(|h| h.opacity(0.9))
                                                .active(|a| a.opacity(0.75))
                                                .cursor_pointer()
                                                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                                    this.handle_reconnect(cx);
                                                }))
                                        })
                                        .when(is_reconnecting, |s| {
                                            s.opacity(0.8).cursor_not_allowed()
                                        })
                                        .tooltip(move |_, cx| {
                                            let tip = tooltip_text.clone();
                                            cx.new(|_| Tooltip::new(tip)).into()
                                        })
                                        .text_size(ui_text_md(cx))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(rgb(0xffffff))
                                        .child(
                                            if is_reconnecting {
                                                div()
                                                    .id(ElementId::Name(anim_id.clone().into()))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .with_animation(
                                                        anim_id,
                                                        Animation::new(Duration::from_millis(1000)).repeat(),
                                                        move |this, delta| {
                                                            let angle = delta * std::f32::consts::TAU;
                                                            this.child(
                                                                AppIcon::Refresh
                                                                    .size(ICON_SM)
                                                                    .text_color(rgb(0xffffff))
                                                                    .with_transformation(Transformation::rotate(radians(angle))),
                                                            )
                                                        },
                                                    )
                                                    .into_any_element()
                                            } else {
                                                AppIcon::Refresh
                                                    .size(ICON_SM)
                                                    .text_color(rgb(0xffffff))
                                                    .into_any_element()
                                            }
                                        )
                                        .child(reconnect_text)
                                )
                        )
                    })
                    .when(show_border, |el| {
                        el.child(
                            div()
                                .absolute()
                                .inset_0()
                                .border_1()
                                .border_color(border_color)
                                .when(bottom_left_r > 0.0, |d| d.rounded_bl(px(bottom_left_r)))
                                .when(bottom_right_r > 0.0, |d| d.rounded_br(px(bottom_right_r))),
                        )
                    })
                    .when(is_log_recording, |el| {
                        el.child(self.render_log_recording_toolbar(cx))
                    }),
                )
            })
            .when(search_active, |el: Stateful<Div>| {
                el.child(self.search_bar.clone())
            })
            .into_any_element()
    }
}

impl<D: ActionDispatch + Send + Sync> TerminalPane<D> {
    pub(super) fn execute_welcome_action(
        &mut self,
        action: crate::welcome::WelcomeAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            crate::welcome::WelcomeAction::StartTerminal => {
                log::debug!(
                    "[terminal_pane:start_terminal] project_id={} path={:?}",
                    self.project_id, self.layout_path
                );
                self.start_terminal_with_shell(velowork_core::shell::ShellType::Default, cx);
            }
            crate::welcome::WelcomeAction::ConnectSession(session) => {
                self.connect_to_session(session, cx);
            }
            crate::welcome::WelcomeAction::NewSession => {
                window.dispatch_action(Box::new(crate::actions::NewSession), cx);
            }
            crate::welcome::WelcomeAction::AiAssistant => {
                window.dispatch_action(Box::new(crate::actions::ShowAiAssistant), cx);
            }
            crate::welcome::WelcomeAction::QuickCommands => {
                window.dispatch_action(Box::new(crate::actions::ShowQuickCommandsPanel), cx);
            }
            crate::welcome::WelcomeAction::ImportSessions => {
                self.request_broker.update(cx, |broker, cx| {
                    broker.push_overlay_request(
                        velowork_workspace::requests::OverlayRequest::ImportSessionsDialog,
                        cx,
                    );
                });
            }
        }
    }

    pub(super) fn render_welcome_state(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let shortcuts = crate::welcome::WelcomeShortcuts::from_cx(cx);

        crate::welcome::render_welcome_dashboard(
            &self.project_id,
            &self.quick_connect_input,
            self.welcome_selected_index,
            shortcuts,
            window,
            cx,
            cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if let Some(action) = crate::welcome::handle_welcome_key_down(
                    &this.project_id,
                    &this.quick_connect_input,
                    &mut this.welcome_selected_index,
                    event,
                    cx,
                ) {
                    this.execute_welcome_action(action, window, cx);
                }
                cx.notify();
            }),
            cx.listener(|this, _, window, cx| {
                if let Some(action) = crate::welcome::handle_welcome_quick_connect(
                    &this.project_id,
                    &this.quick_connect_input,
                    &mut this.welcome_selected_index,
                    cx,
                ) {
                    this.execute_welcome_action(action, window, cx);
                }
            }),
            cx.listener(|this, action: &crate::welcome::WelcomeAction, window, cx| {
                this.execute_welcome_action(action.clone(), window, cx);
            }),
        )
    }

    /// Start a terminal inside this pane with the given shell type, transitioning away from Welcome
    pub fn start_terminal_with_shell(
        &mut self,
        shell_type: velowork_core::shell::ShellType,
        cx: &mut Context<Self>,
    ) {
        self.shell_type = shell_type.clone();
        let project_id = self.project_id.clone();
        let layout_path = self.layout_path.clone();

        self.workspace.update(cx, |ws, cx| {
            ws.set_terminal_shell(&project_id, &layout_path, shell_type, cx);
        });

        self.create_new_terminal(cx);
        cx.notify();
    }

    /// Connect to a specific SSH/Telnet/Serial session inside this pane
    pub fn connect_to_session(
        &mut self,
        session: velowork_state::SshSession,
        cx: &mut Context<Self>,
    ) {
        let shell = crate::welcome::session_to_shell_type(&session);

        let session_id = session.id.clone();
        let conn_store = cx
            .try_global::<velowork_workspace::stores::GlobalConnectionStore>()
            .map(|c| c.0.clone());
        if let Some(conn_store) = conn_store {
            conn_store.update(cx, |store, cx| {
                store.mark_connected(&session_id, cx);
            });
        }

        self.start_terminal_with_shell(shell, cx);
    }

    /// Check if this pane is situated at the bottom-left and/or bottom-right corners of the layout tree.
    pub(super) fn check_bottom_corners_in_layout(&self, cx: &App) -> (bool, bool) {
        let ws = self.workspace.read(cx);
        let Some(project) = ws.project(&self.project_id) else {
            return (false, false);
        };
        let Some(ref root) = project.layout else {
            return (false, false);
        };
        check_node_bottom_corners(root, &self.layout_path, true, true, true)
    }
}

fn check_node_bottom_corners(
    node: &LayoutNode,
    path: &[usize],
    on_bottom: bool,
    on_left: bool,
    on_right: bool,
) -> (bool, bool) {
    if path.is_empty() {
        return (on_bottom && on_left, on_bottom && on_right);
    }
    let idx = path[0];
    let rest = &path[1..];
    match node {
        LayoutNode::Terminal { .. } => (on_bottom && on_left, on_bottom && on_right),
        LayoutNode::Tabs { children, .. } => {
            if let Some(child) = children.get(idx) {
                check_node_bottom_corners(child, rest, on_bottom, on_left, on_right)
            } else {
                (false, false)
            }
        }
        LayoutNode::Split {
            direction,
            children,
            ..
        } => {
            let Some(child) = children.get(idx) else {
                return (false, false);
            };
            let visible_indices: Vec<usize> = children
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.is_all_hidden())
                .map(|(i, _)| i)
                .collect();
            if visible_indices.is_empty() {
                return (false, false);
            }
            let is_first = visible_indices.first() == Some(&idx);
            let is_last = visible_indices.last() == Some(&idx);
            match direction {
                SplitDirection::Horizontal => {
                    let child_on_bottom = on_bottom && is_last;
                    check_node_bottom_corners(child, rest, child_on_bottom, on_left, on_right)
                }
                SplitDirection::Vertical => {
                    let child_on_left = on_left && is_first;
                    let child_on_right = on_right && is_last;
                    check_node_bottom_corners(child, rest, on_bottom, child_on_left, child_on_right)
                }
            }
        }
    }
}


