//! Tab bar rendering and management

mod shell_selector;

use crate::ActionDispatch;
use crate::actions::{Cancel, Search};
use crate::layout::layout_container::{
    LayoutContainer, is_renaming, rename_input,
};
use crate::layout::pane_drag::{PaneDrag, PaneDragView};
use crate::elements::terminal_element::TerminalElement;
use crate::terminal_view_settings;
use gpui::prelude::*;
use gpui::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use velowork_i18n::i18n;
use velowork_terminal::shell_config::ShellType;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::header_buttons::{HeaderAction, header_button_base};
use velowork_ui::icon::AppIcon;
use velowork_ui::menu::{PopupMenu, PopupMenuDirection, PopupMenuItem};
use velowork_ui::overlay_menu::{
    OverlayMenu, OverlayMenuAction, OverlayMenuDirection, OverlayMenuEntry, OverlayMenuItem,
};
use velowork_ui::scrollable::{Scrollbar, ScrollbarShow};
use velowork_ui::tab::{tab_height, tab_style};
use velowork_ui::theme::{bg_opacity, surface_bg_t, theme, with_alpha};
use velowork_ui::motion::ease_out_panel;
use velowork_ui::tokens::{
    RADIUS_CARD, RADIUS_MD, RADIUS_STD, RADIUS_XS, SPACE_MD,
    SPACE_SM, SPACE_XS, ui_icon_std_ts, ui_text_md, ui_text_sm,
};
use velowork_ui::input::Input;
use velowork_ui::{h_flex, v_flex};
use velowork_workspace::settings::TabWidthMode;
use velowork_workspace::state::{LayoutNode, SplitDirection};
use velowork_workspace::stores::GlobalSessionStore;

use crate::layout::session_labels::{duplicate_session_suffixes, terminal_base_name};

/// Context for tab action button closures.
#[derive(Clone)]
#[allow(dead_code)]
pub(super) struct TabActionContext<D: ActionDispatch> {
    pub workspace: Entity<velowork_workspace::state::Workspace>,
    pub project_id: String,
    pub layout_path: Vec<usize>,
    pub active_tab: usize,
    pub standalone: bool,
    pub action_dispatcher: Option<D>,
}

impl<D: ActionDispatch + Send + Sync> LayoutContainer<D> {
    pub(super) fn start_drop_animation(&mut self, tab_index: usize, cx: &mut Context<Self>) {
        self.drop_animation = Some((tab_index, 1.0));
        cx.notify();

        cx.spawn(async move |this: WeakEntity<LayoutContainer<D>>, cx| {
            let duration_ms = 200;
            let frame_time_ms = 33;
            let steps = duration_ms / frame_time_ms;
            let step_duration = std::time::Duration::from_millis(frame_time_ms as u64);

            for i in 1..=steps {
                smol::Timer::after(step_duration).await;

                let t = i as f32 / steps as f32;
                let progress = 1.0 - velowork_ui::motion::ease_out_cubic(t);

                let result = this.update(cx, |this, cx| {
                    if let Some((idx, _)) = this.drop_animation {
                        this.drop_animation = Some((idx, progress));
                        cx.notify();
                    }
                });
                if result.is_err() {
                    break;
                }
            }

            let _ = this.update(cx, |this, cx| {
                this.drop_animation = None;
                cx.notify();
            });
        })
        .detach();
    }

    /// Open the header "more" dropdown (anchored to the more button). It
    /// currently exposes "detach to window" — mirroring the dock's more menu —
    /// so the two tab bars share one consistent action layout.
    fn open_header_more_menu(
        &mut self,
        ctx: TabActionContext<D>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(at) = self.more_menu_toggle_guard.take() {
            if at.elapsed() < std::time::Duration::from_millis(350) {
                return;
            }
        }

        if let Some(ctx) = self.more_menu.take() {
            ctx.update(cx, |m, cx| {
                m.set_on_close(None);
                m.close(window, cx);
            });
            cx.notify();
            return;
        }

        let ctx_split_v = ctx.clone();
        let ctx_split_h = ctx.clone();
        let mut items = Vec::new();

        // 1. 垂直分屏
        items.push(
            PopupMenuItem::item(
                "split-v",
                i18n!(cx, "terminal.split_vertical"),
                move |_window, cx| {
                    if let Some(ref dispatcher) = ctx_split_v.action_dispatcher {
                        dispatcher.dispatch(
                            velowork_core::api::ActionRequest::SplitTerminal {
                                project_id: ctx_split_v.project_id.clone(),
                                path: ctx_split_v.layout_path.clone(),
                                direction: SplitDirection::Vertical,
                            },
                            cx,
                        );
                    }
                },
            )
            .icon(AppIcon::SplitVertical),
        );

        // 2. 水平分屏
        items.push(
            PopupMenuItem::item(
                "split-h",
                i18n!(cx, "terminal.split_horizontal"),
                move |_window, cx| {
                    if let Some(ref dispatcher) = ctx_split_h.action_dispatcher {
                        dispatcher.dispatch(
                            velowork_core::api::ActionRequest::SplitTerminal {
                                project_id: ctx_split_h.project_id.clone(),
                                path: ctx_split_h.layout_path.clone(),
                                direction: SplitDirection::Horizontal,
                            },
                            cx,
                        );
                    }
                },
            )
            .icon(AppIcon::SplitHorizontal),
        );

        let this_weak = cx.entity().downgrade();
        let bounds = self.more_button_bounds.borrow().clone();
        let overlay_registry = self.overlay_registry.clone();
        let menu = cx.new(move |cx| {
            PopupMenu::new(cx, items, bounds.origin, overlay_registry, None)
                .trigger_bounds(bounds)
                .direction(PopupMenuDirection::Below)
                .min_width(px(120.0))
        });

        let menu_id = menu.entity_id();
        let on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync> = Arc::new(move |_, cx| {
            if let Some(this) = this_weak.upgrade() {
                let _ = this.update(cx, |this, cx| {
                    if this.more_menu.as_ref().map(|m| m.entity_id()) == Some(menu_id) {
                        this.more_menu = None;
                        this.more_menu_toggle_guard = Some(std::time::Instant::now());
                        cx.notify();
                    }
                });
            }
        });
        menu.update(cx, |m, _| m.set_on_close(Some(on_close)));

        let focus_handle = menu.read(cx).focus_handle.clone();
        window.focus(&focus_handle, cx);
        self.more_menu = Some(menu);
        cx.notify();
    }

    pub(super) fn render_tab_action_buttons(
        &self,
        ctx: TabActionContext<D>,
        terminal_id: Option<String>,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let id_suffix = format!("tabs-{:?}", ctx.layout_path);

        let supports_buffer_capture = self.backend.supports_buffer_capture();
        let backend_for_export = self.backend.clone();
        let terminal_id_for_export = terminal_id.clone();

        let scale = velowork_ui::tokens::ui_scale_factor(cx);

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(SPACE_SM)

            .when(matches!(self.get_active_shell_type(ctx.active_tab, cx), ShellType::Custom { ref path, .. } if path == "ssh"), |el| {
                let project_id = ctx.project_id.clone();
                let request_broker = self.request_broker.clone();

                el.child(
                    header_button_base(HeaderAction::Sftp, &id_suffix, &t, None, None, cx)
                        .on_click(move |_, _window, cx| {
                            request_broker.update(cx, |broker, cx| {
                                broker.push_overlay_request(
                                    velowork_workspace::requests::OverlayRequest::Project(
                                        velowork_workspace::requests::ProjectOverlay {
                                            project_id: project_id.clone(),
                                            kind: velowork_workspace::requests::ProjectOverlayKind::ToggleSftpPanel,
                                        },
                                    ),
                                    cx,
                                );
                            });
                        }),
                )
            })
            .child(
                header_button_base(
                    HeaderAction::Search,
                    &id_suffix,
                    &t,
                    Some(i18n!(cx, "terminal.search").into()),
                    Some(Box::new(Search)),
                    cx,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    if let Some(pane) = this.active_terminal_pane(cx) {
                        pane.update(cx, |pane, cx| pane.toggle_search(window, cx));
                    }
                })),
            )
            .when(supports_buffer_capture, |el| {
                el.child(
                    header_button_base(HeaderAction::ExportBuffer, &id_suffix, &t, None, None, cx)
                        .on_click(move |_, _window, cx| {
                            if let Some(ref tid) = terminal_id_for_export
                                && let Some(path) = backend_for_export.capture_buffer(tid) {
                                    cx.write_to_clipboard(ClipboardItem::new_string(path.display().to_string()));
                                    log::info!("[views:terminal] Buffer exported | path={} (copied to clipboard)", path.display());
                                }
                        }),
                )
            })
            // “更多” 菜单按钮：包含“垂直分屏”、“水平分屏”等子项，与 dock 栏全局统一。
            .child({
                let ctx_for_more = ctx.clone();
                let more_button_bounds = self.more_button_bounds.clone();
                header_button_base(
                    HeaderAction::MoreMenu,
                    &id_suffix,
                    &t,
                    None,
                    None,
                    cx,
                )
                .child(
                    canvas(
                        {
                            let more_button_bounds = more_button_bounds.clone();
                            move |bounds, _window, _cx| {
                                *more_button_bounds.borrow_mut() = bounds;
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_header_more_menu(ctx_for_more.clone(), window, cx);
                }))
            })
            // 分割线：与 dock 栏右侧操作区保持一致的视觉分隔（操作按钮 → 更多 → 分割线 → 全屏）。
            .child(
                div()
                    .id(ElementId::Name(format!("action-divider-{}", id_suffix).into()))
                    .h(px(14.0 * scale))
                    .w(px(1.0))
                    .mx(px(2.0))
                    .bg(p.border_subtle),
            )
            // 全屏（zoom）状态下，将“全屏展开”按钮替换为“收起全屏”按钮；
            // 顶栏（tab bar）始终保留，仅内容区放大为当前终端。
            .child({
                let is_fullscreen = terminal_id.as_ref().is_some_and(|tid| {
                    self.focus_manager
                        .read(cx)
                        .is_terminal_fullscreened(&self.project_id, tid)
                });
                if is_fullscreen {
                    let exit_tooltip = i18n!(cx, "dock.action.exit_fullscreen");
                    let ctx_exit = ctx.clone();
                    header_button_base(
                        HeaderAction::ExitZoom,
                        &id_suffix,
                        &t,
                        Some(exit_tooltip.into()),
                        None,
                        cx,
                    )
                    .on_click(move |_, _window, cx| {
                        if let Some(ref dispatcher) = ctx_exit.action_dispatcher {
                            dispatcher.dispatch(velowork_core::api::ActionRequest::SetFullscreen {
                                project_id: ctx_exit.project_id.clone(),
                                terminal_id: None,
                                window: None,
                            }, cx);
                        }
                    })
                } else {
                    let fullscreen_tooltip = i18n!(cx, "dock.action.expand");
                    let ctx_fs = ctx.clone();
                    let terminal_id_for_fs = terminal_id.clone();
                    header_button_base(
                        HeaderAction::Fullscreen,
                        &id_suffix,
                        &t,
                        Some(fullscreen_tooltip.into()),
                        None,
                        cx,
                    )
                    .on_click(move |_, _window, cx| {
                        if let Some(ref tid) = terminal_id_for_fs
                            && let Some(ref dispatcher) = ctx_fs.action_dispatcher {
                                dispatcher.dispatch(velowork_core::api::ActionRequest::SetFullscreen {
                                    project_id: ctx_fs.project_id.clone(),
                                    terminal_id: Some(tid.clone()),
                                    window: None,
                                }, cx);
                            }
                    })
                }
            })
    }

    pub(super) fn render_tabs(
        &mut self,
        children: &[LayoutNode],
        active_tab: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        log::debug!(
            "[tabs:render] project_id={} path={:?} active_tab={} num_children={}",
            self.project_id, self.layout_path, active_tab, children.len()
        );
        if let Some(zoomed_idx) = self.find_zoomed_child_index(children, cx) {
            let mut child_path = self.layout_path.clone();
            child_path.push(zoomed_idx);

            let visible_paths = HashSet::from([child_path.clone()]);
            self.deregister_child_resize_viewers_except(&visible_paths, cx);

            let container = self
                .child_containers
                .entry(child_path.clone())
                .or_insert_with(|| {
                    cx.new(|_cx| {
                        LayoutContainer::new(
                            self.workspace.clone(),
                            self.focus_manager.clone(),
                            self.request_broker.clone(),
                            self.window_id,
                            self.project_id.clone(),
                            self.project_path.clone(),
                            child_path.clone(),
                            self.backend.clone(),
                            self.terminals.clone(),
                            self.active_drag.clone(),
                            self.action_dispatcher.clone(),
                        )
                    })
                })
                .clone();

            if let Some(reg) = self.overlay_registry.clone() {
                container.update(cx, |c, _cx| c.set_overlay_registry(reg));
            }

            // 全屏（zoom）模式下保留顶部 tab bar（仅内容区放大为该终端），
            // 由 tab bar 右侧的“收起全屏”按钮退出全屏。
            return v_flex()
                .size_full()
                .child(self.render_tab_bar(children, zoomed_idx, false, window, cx))
                .child(
                    div()
                        .id("console")
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .relative()
                        .child(
                            AnyView::from(container).cached(StyleRefinement::default().size_full()),
                        ),
                );
        }

        let visible_indices: Vec<usize> = children
            .iter()
            .enumerate()
            .filter(|(_, child)| !child.is_all_hidden())
            .map(|(i, _)| i)
            .collect();

        let enable_animations = cx
            .try_global::<velowork_app_core::settings::GlobalSettings>()
            .map(|g| g.0.read(cx).settings.enable_animations)
            .unwrap_or(true);

        let parent_bounds = *self.container_bounds_ref.borrow();
        let w = f32::from(parent_bounds.size.width).max(300.0);
        let h = f32::from(parent_bounds.size.height).max(200.0);
        let bar_h = f32::from(super::layout_container::compute_tab_bar_height(cx));
        let content_h = (h - bar_h).max(100.0);
        let dock_x = 16.0f32;

        let bounds_canvas = canvas(
            {
                let container_bounds_ref = self.container_bounds_ref.clone();
                move |bounds, _window, _cx| {
                    *container_bounds_ref.borrow_mut() = bounds;
                }
            },
            |_bounds, _prepaint, _window, _cx| {},
        )
        .absolute()
        .size_full();

        let make_ghost_collapsing_cards = |this: &Self, cx: &App| -> Vec<AnyElement> {
            let mut cards = Vec::new();
            if enable_animations {
                let t = theme(cx);
                for (i, child) in children.iter().enumerate() {
                    if let LayoutNode::Terminal { terminal_id: Some(tid), .. } = child {
                        if let Some(&(start, seq)) = this.collapsing_tabs.get(tid) {
                            if start.elapsed() < std::time::Duration::from_millis(280) {
                                let mut child_path = this.layout_path.clone();
                                child_path.push(i);
                                let terminal_pane = this.child_containers.get(&child_path)
                                    .and_then(|c| c.read(cx).terminal_pane.clone());
                                let terminal_arc = terminal_pane.as_ref().and_then(|p| p.read(cx).terminal_arc());
                                let zoom_level = this.workspace.read(cx).get_terminal_zoom(&this.project_id, &child_path);
                                let preview_el = terminal_arc.clone().map(|term| {
                                    TerminalElement::preview(term, cx.focus_handle()).with_zoom(zoom_level)
                                });
                                let gutter_el = terminal_arc.as_ref().and_then(|term| {
                                    crate::layout::terminal_pane::render_line_numbers_gutter(term, zoom_level, None, cx)
                                });

                                let ghost_card = div()
                                    .id(ElementId::Name(format!("tabs-ghost-card-{}-{}", tid, seq).into()))
                                    .bg(surface_bg_t(t.bg_panel, &t))
                                    .shadow_lg()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .size_full()
                                            .relative()
                                            .flex()
                                            .flex_row()
                                            .when_some(gutter_el, |el, g| el.child(g))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .h_full()
                                                    .p(SPACE_XS)
                                                    .relative()
                                                    .when_some(preview_el, |d, el| {
                                                        d.child(el)
                                                    }),
                                            ),
                                    );

                                let tid_str = tid.clone();
                                let contracting_el = ghost_card.with_animation(
                                    format!("tabs-ghost-contract-{}-{}", tid_str, seq),
                                    Animation::new(std::time::Duration::from_millis(280))
                                        .with_easing(ease_out_panel),
                                    move |this, delta| {
                                        let t = delta;
                                        if t >= 0.98 {
                                            this.absolute()
                                                .left(px(dock_x))
                                                .bottom(px(16.0))
                                                .w(px(w * 0.18))
                                                .h(px(content_h * 0.18))
                                                .opacity(0.0)
                                        } else {
                                            let scale = 1.0 - 0.82 * t;
                                            let cur_w = (w * scale).max(10.0);
                                            let cur_h = (content_h * scale).max(10.0);
                                            let target_x = dock_x;
                                            let target_y = (content_h - cur_h - 16.0).max(0.0);
                                            let cur_x = target_x * t;
                                            let cur_y = target_y * t;
                                            let cur_radius = (6.0 + 6.0 * t).min(12.0);
                                            let fade = if t >= 0.65 {
                                                ((1.0 - t) / 0.35).clamp(0.0, 1.0)
                                            } else {
                                                1.0
                                            };
                                            this.absolute()
                                                .left(px(cur_x))
                                                .top(px(cur_y))
                                                .w(px(cur_w))
                                                .h(px(cur_h))
                                                .rounded(px(cur_radius))
                                                .shadow_lg()
                                                .overflow_hidden()
                                                .opacity(fade)
                                        }
                                    },
                                );
                                cards.push(contracting_el.into_any_element());
                            }
                        }
                    }
                }
            }
            cards
        };

        let make_ghost_expanding_cards = |this: &Self, cx: &App| -> Vec<AnyElement> {
            let mut cards = Vec::new();
            if enable_animations {
                let render_settings = crate::terminal_view_settings(cx);
                let defaults = render_settings.terminal_defaults();
                let default_opts = velowork_state::SessionTerminalOptions::default();
                let effective_config = velowork_terminal::resolve_effective_terminal_config(&default_opts, &defaults);
                let term_palette = velowork_core::theme::get_terminal_palette_with_custom(
                    &effective_config.color_scheme,
                    &render_settings.custom_terminal_color_schemes,
                );
                let image_set = render_settings
                    .terminal_background_image
                    .as_ref()
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false);
                let base_alpha = if image_set { 0.0 } else { velowork_ui::theme::bg_opacity(cx) };
                let pane_bg = if base_alpha > 0.0 {
                    velowork_ui::theme::with_alpha(term_palette.background, base_alpha)
                } else {
                    gpui::transparent_black()
                };

                let ghost_bounds = Some(Bounds {
                    origin: Point::default(),
                    size: Size {
                        width: px(w),
                        height: px(content_h),
                    },
                });

                for (i, child) in children.iter().enumerate() {
                    if let LayoutNode::Terminal { terminal_id: Some(tid), .. } = child {
                        if let Some(&(_start, seq)) = this.restoring_tabs.get(tid) {
                            let mut child_path = this.layout_path.clone();
                            child_path.push(i);
                            let terminal_pane = this.child_containers.get(&child_path)
                                .and_then(|c| c.read(cx).terminal_pane.clone());
                            let terminal_arc = terminal_pane.as_ref().and_then(|p| p.read(cx).terminal_arc());
                            let zoom_level = this.workspace.read(cx).get_terminal_zoom(&this.project_id, &child_path);
                            let preview_el = terminal_arc.clone().map(|term| {
                                TerminalElement::preview(term, cx.focus_handle()).with_zoom(zoom_level)
                            });
                            let gutter_el = terminal_arc.as_ref().and_then(|term| {
                                crate::layout::terminal_pane::render_line_numbers_gutter(term, zoom_level, ghost_bounds, cx)
                            });

                            let ghost_card = div()
                                .id(ElementId::Name(format!("tabs-ghost-expand-card-{}-{}", tid, seq).into()))
                                .bg(pane_bg)
                                .overflow_hidden()
                                .child(
                                    div()
                                        .size_full()
                                        .relative()
                                        .flex()
                                        .flex_row()
                                        .when_some(gutter_el, |el, g| el.child(g))
                                        .child(
                                            div()
                                                .flex_1()
                                                .h_full()
                                                .p(SPACE_XS)
                                                .relative()
                                                .when_some(preview_el, |d, el| {
                                                    d.child(el)
                                                }),
                                        ),
                                );

                            let tid_str = tid.clone();
                            let expanding_el = ghost_card.with_animation(
                                format!("tabs-ghost-expand-{}-{}", tid_str, seq),
                                Animation::new(std::time::Duration::from_millis(300))
                                    .with_easing(ease_out_panel),
                                move |this, delta| {
                                    let t = delta.clamp(0.0, 1.0);
                                    let scale = 0.18 + 0.82 * t;
                                    let cur_w = (w * scale).max(10.0);
                                    let cur_h = (content_h * scale).max(10.0);
                                    let target_x = dock_x;
                                    let target_y = (content_h - cur_h - 16.0).max(0.0);
                                    let cur_x = target_x * (1.0 - t);
                                    let cur_y = target_y * (1.0 - t);
                                    let cur_radius = (12.0 * (1.0 - t)).max(0.0);
                                    let fade = if t < 0.25 {
                                        (t / 0.25).clamp(0.0, 1.0)
                                    } else {
                                        1.0
                                    };
                                    this.absolute()
                                        .left(px(cur_x))
                                        .top(px(cur_y))
                                        .w(px(cur_w))
                                        .h(px(cur_h))
                                        .rounded(px(cur_radius))
                                        .when(t < 0.95, |d| d.shadow_lg())
                                        .overflow_hidden()
                                        .opacity(fade)
                                },
                            );
                            cards.push(expanding_el.into_any_element());
                        }
                    }
                }
            }
            cards
        };

        if visible_indices.is_empty() {
            let tab_bar = self.render_tab_bar(children, active_tab, false, window, cx);
            let welcome_view = self.render_welcome_empty_state(window, cx);
            let ghost_cards = make_ghost_collapsing_cards(self, cx);
            let expanding_cards = make_ghost_expanding_cards(self, cx);
            return v_flex()
                .size_full()
                .relative()
                .child(bounds_canvas)
                .child(tab_bar)
                .child(
                    div()
                        .id("console")
                        .flex_1()
                        .size_full()
                        .overflow_hidden()
                        .relative()
                        .child(welcome_view)
                        .children(ghost_cards)
                        .children(expanding_cards),
                );
        }

        let effective_active_tab = if visible_indices.contains(&active_tab) {
            active_tab
        } else {
            *visible_indices
                .iter()
                .min_by_key(|&&i| (i as isize - active_tab as isize).abs())
                .unwrap_or(&visible_indices[0])
        };

        let num_children = children.len();
        let valid_paths: HashSet<Vec<usize>> = (0..num_children)
            .map(|i| {
                let mut path = self.layout_path.clone();
                path.push(i);
                path
            })
            .collect();
        let active_path = {
            let mut path = self.layout_path.clone();
            path.push(effective_active_tab);
            path
        };
        let visible_paths = HashSet::from([active_path]);
        self.deregister_child_resize_viewers_except(&visible_paths, cx);
        self.child_containers
            .retain(|path, _| valid_paths.contains(path));

        // Deregister pane map entries for inactive tabs so stale entries
        // don't interfere with spatial navigation
        let mut path = self.layout_path.clone();
        let base_len = path.len();
        for i in 0..num_children {
            if i != effective_active_tab {
                path.truncate(base_len);
                path.push(i);
                crate::layout::navigation::deregister_pane_bounds(
                    self.window_id,
                    &self.project_id,
                    &path,
                );
            }
        }

        let tab_bar = self.render_tab_bar(children, effective_active_tab, false, window, cx);
        let ghost_cards = make_ghost_collapsing_cards(self, cx);
        let expanding_cards = make_ghost_expanding_cards(self, cx);

        v_flex()
            .size_full()
            .relative()
            .child(bounds_canvas)
            .child(tab_bar)
            .child(
                div()
                    .id("console")
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .relative()
                    .child({
                        let mut child_path = self.layout_path.clone();
                        child_path.push(effective_active_tab);

                        let container = self
                            .child_containers
                            .entry(child_path.clone())
                            .or_insert_with(|| {
                                cx.new(|_cx| {
                                    LayoutContainer::new(
                                        self.workspace.clone(),
                                        self.focus_manager.clone(),
                                        self.request_broker.clone(),
                                        self.window_id,
                                        self.project_id.clone(),
                                        self.project_path.clone(),
                                        child_path.clone(),
                                        self.backend.clone(),
                                        self.terminals.clone(),
                                        self.active_drag.clone(),
                                        self.action_dispatcher.clone(),
                                    )
                                })
                            })
                            .clone();

                        *container.read(cx).container_bounds_ref.borrow_mut() = Bounds {
                            origin: parent_bounds.origin,
                            size: Size {
                                width: px(w),
                                height: px(content_h),
                            },
                        };

                        if let Some(reg) = self.overlay_registry.clone() {
                            container.update(cx, |c, _cx| c.set_overlay_registry(reg));
                        }

                        let is_active_restoring = children.get(effective_active_tab).and_then(|child| {
                            if let LayoutNode::Terminal { terminal_id: Some(tid), .. } = child {
                                Some(self.restoring_tabs.contains_key(tid))
                            } else {
                                None
                            }
                        }).unwrap_or(false);

                        div()
                            .size_full()
                            .when(is_active_restoring, |d| d.opacity(0.0))
                            .child(container.clone())
                    })
                    .children(ghost_cards)
                    .children(expanding_cards),
            )
    }

    pub(super) fn render_standalone_tab_bar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let node = {
            let ws = self.workspace.read(cx);
            self.get_layout(ws).cloned()
        };

        let children: &[LayoutNode] = match node {
            Some(ref n @ LayoutNode::Terminal { .. }) => std::slice::from_ref(n),
            _ => &[],
        };

        self.render_tab_bar(children, 0, true, window, cx)
    }

    /// Resolve the SSH connection display name for a terminal's shell.
    ///
    /// For `ssh` custom shells the saved session id is passed through the
    /// `--id`/`--session-id` flag; we look it up in the workspace's SSH
    /// session tree (loaded in memory at startup) and return its
    /// user-facing `name`. Falls back to the `user@host` argument when no
    /// saved session matches. Returns `None` for non-SSH shells.
    /// Compute the display label for a tab.
    ///
    /// Priority: real SSH connection name > terminal display name
    /// (custom name / OSC title / directory) > numbered default.
    ///
    /// When the same SSH session is opened in several tabs of this bar, the 2nd
    /// and later occurrences get a `:N` suffix (1-based) so they stay
    /// distinguishable: 名字, 名字:2, 名字:3, ...
    fn tab_display_label(
        &self,
        children: &[LayoutNode],
        index: usize,
        cx: &App,
        suffixes: &HashMap<String, usize>,
    ) -> String {
        let child = &children[index];
        let (terminal_id, shell_type) = match child {
            LayoutNode::Terminal {
                terminal_id,
                shell_type,
                ..
            } => (terminal_id.clone(), Some(shell_type.clone())),
            _ => (None, None),
        };

        let index_label = format!("{} {}", i18n!(cx, "terminal.tab"), index + 1);
        if matches!(shell_type, Some(velowork_core::shell::ShellType::Welcome)) {
            return i18n!(cx, "welcome.title");
        }

        let base = if let (Some(tid), Some(st)) = (terminal_id.as_ref(), shell_type.as_ref()) {
            if let Some(project) = self.workspace.read(cx).project(&self.project_id) {
                let osc_title = self.terminals.lock().get(tid).and_then(|t| t.title());
                let store = cx.global::<GlobalSessionStore>().0.read(cx);
                terminal_base_name(
                    tid.as_str(),
                    st,
                    self.backend.is_remote(),
                    osc_title.as_deref(),
                    project,
                    &store,
                )
            } else {
                index_label.clone()
            }
        } else {
            index_label.clone()
        };

        // Apply the single, global duplicate-session numbering so the tab strip
        // and the tab-list dropdown show the exact same `:N` suffix as the
        // command panel's target host list.
        if let Some(ref tid) = terminal_id {
            if let Some(&suffix) = suffixes.get(tid) {
                if suffix > 1 {
                    return format!("{}:{}", base, suffix);
                }
            }
        }

        base
    }

    /// Leading icon for a tab. Distinguishes local vs remote sessions and
    /// conveys connection status purely through the icon's color — there is no
    /// separate status dot:
    ///
    /// * Remote sessions (ssh shell or remote backend) → `server.svg`.
    /// * Local terminals → `terminal.svg` (a `>_` prompt glyph).
    ///
    /// Color: `error` (red) when the connection is lost, otherwise `success`
    /// (green, connected). The color reflects connection status only and does
    /// not depend on whether the tab is the active one.
    fn render_tab_icon_element(
        &self,
        icon: AppIcon,
        color: Hsla,
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .flex_shrink_0()
            .size(ui_icon_std_ts(cx))
            .flex()
            .items_center()
            .justify_center()
            .child(
                icon
                    .size(ui_icon_std_ts(cx))
                    .flex_shrink_0()
                    .text_color(color),
            )
            .into_any_element()
    }

    fn render_tab_bar(
        &mut self,
        children: &[LayoutNode],
        active_tab: usize,
        standalone: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);
        let workspace = self.workspace.clone();
        let project_id = self.project_id.clone();
        let layout_path = self.layout_path.clone();
        let num_children = children.len();

        let drop_animation = self.drop_animation;

        let terminals = self.terminals.clone();
        let workspace_reader = self.workspace.read(cx);
        let project = workspace_reader.project(&self.project_id);
        let _project_for_names = project.cloned();
        let tab_width_mode = velowork_app_core::settings::settings(cx).tab_width_mode;
        let enable_tab_preview = velowork_app_core::settings::settings(cx).enable_tab_preview;

        // Single global duplicate-session numbering, shared with the tab-list
        // dropdown and the command panel's target host list.
        let suffixes = duplicate_session_suffixes(&self.workspace.read(cx));

        let enable_animations = cx
            .try_global::<velowork_app_core::settings::GlobalSettings>()
            .map(|g| g.0.read(cx).settings.enable_animations)
            .unwrap_or(true);

        let current_all_ids: HashSet<String> = children
            .iter()
            .filter_map(|child| {
                if let LayoutNode::Terminal { terminal_id: Some(id), .. } = child {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();

        let current_minimized_ids: HashSet<String> = children
            .iter()
            .filter_map(|child| {
                if let LayoutNode::Terminal { terminal_id: Some(id), minimized: true, .. } = child {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();

        let current_visible_ids: HashSet<String> = children
            .iter()
            .filter_map(|child| {
                if !child.is_all_hidden()
                    && let LayoutNode::Terminal { terminal_id: Some(id), .. } = child
                {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();

        if enable_animations && self.has_initialized_tabs {
            // 1. 只有上一帧已存在且未最小化，当前帧变为最小化的 Tab，才触发收缩动画
            for id in &current_minimized_ids {
                if self.prev_all_tab_ids.contains(id)
                    && !self.prev_minimized_tab_ids.contains(id)
                    && !self.collapsing_tabs.contains_key(id)
                {
                    self.tab_anim_seq = self.tab_anim_seq.wrapping_add(1);
                    let seq = self.tab_anim_seq;
                    self.restoring_tabs.remove(id);
                    self.collapsing_tabs.insert(id.clone(), (std::time::Instant::now(), seq));
                    let entity = cx.entity().downgrade();
                    let tid_clone = id.clone();
                    cx.spawn(async move |_, cx| {
                        smol::Timer::after(std::time::Duration::from_millis(290)).await;
                        let _ = entity.update(cx, |this, cx| {
                            this.collapsing_tabs.remove(&tid_clone);
                            cx.notify();
                        });
                    }).detach();
                }
            }

            // 2. 只有上一帧处于最小化，当前帧恢复可见的 Tab，才触发还原展开动画
            //    全新创建的 Tab（上一帧不在 prev_all_tab_ids 和 prev_minimized_tab_ids 中）绝对不触发
            for cur_id in &current_visible_ids {
                if self.prev_minimized_tab_ids.contains(cur_id)
                    && !self.restoring_tabs.contains_key(cur_id)
                {
                    self.tab_anim_seq = self.tab_anim_seq.wrapping_add(1);
                    let seq = self.tab_anim_seq;
                    self.collapsing_tabs.remove(cur_id);
                    self.restoring_tabs.insert(cur_id.clone(), (std::time::Instant::now(), seq));
                    let entity = cx.entity().downgrade();
                    let tid_clone = cur_id.clone();
                    cx.spawn(async move |_, cx| {
                        smol::Timer::after(std::time::Duration::from_millis(300)).await;
                        let _ = entity.update(cx, |this, cx| {
                            this.restoring_tabs.remove(&tid_clone);
                            for child in this.child_containers.values() {
                                child.update(cx, |_, cx| cx.notify());
                            }
                            cx.notify();
                        });
                    }).detach();
                }
            }
        }
        self.prev_visible_tab_ids = current_visible_ids;
        self.prev_all_tab_ids = current_all_ids;
        self.prev_minimized_tab_ids = current_minimized_ids;
        self.has_initialized_tabs = true;

        let this_weak = cx.entity().downgrade();
        self.tab_bounds.borrow_mut().retain(|k, _| *k < children.len());
        let active_preview_props: std::rc::Rc<std::cell::RefCell<Option<velowork_ui::TerminalPreviewProps>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        let tab_elements: Vec<_> = children
            .iter()
            .enumerate()
            .filter(|(_, child)| {
                if !child.is_all_hidden() {
                    return true;
                }
                if let LayoutNode::Terminal { terminal_id: Some(id), .. } = child {
                    if let Some((start, _)) = self.collapsing_tabs.get(id) {
                        return start.elapsed() < std::time::Duration::from_millis(260);
                    }
                }
                false
            })
            .map(|(i, child)| {
            let is_active = i == active_tab;
            let workspace = workspace.clone();
            let project_id = project_id.clone();
            let project_id_for_drag = project_id.clone();
            let project_id_for_drop = project_id.clone();
            let layout_path = layout_path.clone();
            let layout_path_for_drag = layout_path.clone();
            let layout_path_for_drop = layout_path.clone();

            let terminal_id = match child {
                LayoutNode::Terminal { terminal_id: Some(id), .. } => Some(id.clone()),
                _ => None,
            };

            let idle_label = terminal_id.as_ref().and_then(|tid| {
                let guard = terminals.lock();
                guard.get(tid).and_then(|t| {
                    if t.is_waiting_for_input() {
                        Some(t.idle_duration_display())
                    } else {
                        None
                    }
                })
            });

            let connection_lost = terminal_id
                .as_ref()
                .map_or(false, |tid| self.is_terminal_connection_lost(tid, cx));

            let (is_remote_session, _shell_short) = match child {
                LayoutNode::Terminal { shell_type, .. } => {
                    let remote =
                        shell_type.is_remote() || self.backend.is_remote();
                    let short = if remote {
                        String::new()
                    } else {
                        shell_type.local_shell_name()
                    };
                    (remote, short)
                }
                _ => (false, "?".to_string()),
            };

            let tab_label = self.tab_display_label(children, i, cx, &suffixes);
            let is_renaming_this = terminal_id.as_ref().is_some_and(|tid| {
                is_renaming(&self.tab_rename_state, tid)
            });

            let has_drop_animation = drop_animation.map(|(idx, _)| idx == i).unwrap_or(false);
            let animation_progress = drop_animation
                .filter(|(idx, _)| *idx == i)
                .map(|(_, p)| p)
                .unwrap_or(0.0);

            let status_text = if connection_lost {
                i18n!(cx, "status.connection_lost")
            } else if is_remote_session {
                format!("{} (SSH)", i18n!(cx, "terminal.connection_ok"))
            } else {
                i18n!(cx, "terminal.connection_ok")
            };
            let tab_tooltip_text = if let Some(ref idle) = idle_label {
                format!("{} | {} | Idle: {}", tab_label, status_text, idle)
            } else {
                format!("{} | {}", tab_label, status_text)
            };

            let tab_element = tab_style(
                div().id(ElementId::Name(format!("tab-{}-{:?}", i, layout_path).into())),
                &t,
                is_active,
                cx,
            );

            let tab_element = match tab_width_mode {
                TabWidthMode::Compact => {
                    // 紧凑模式：所有标签固定更紧凑的统一宽度 (Padding 8px, 固定 85px)
                    tab_element
                        .px(SPACE_MD)
                        .w(px(85.0))
                        .flex_shrink_0()
                }
                TabWidthMode::Equal => {
                    // 等宽模式：所有标签固定统一宽度 (Padding 8px, 固定 155px)
                    tab_element
                        .px(SPACE_MD)
                        .w(px(155.0))
                        .flex_shrink_0()
                }
                TabWidthMode::TitleLength => {
                    // 标题自适应模式：自动适配标题长度，无 min/max 限制
                    tab_element
                        .px(SPACE_MD)
                        .flex_shrink_0()
                }
            };

            let is_welcome = matches!(
                child,
                LayoutNode::Terminal {
                    shell_type: velowork_core::shell::ShellType::Welcome,
                    ..
                }
            );
            let (preview_icon, protocol_badge, connection_info, preview_icon_color) = match child {
                LayoutNode::Terminal { shell_type, .. } => {
                    if is_welcome {
                        (
                            AppIcon::AiAssistant,
                            Some(i18n!(cx, "welcome.title")),
                            Some(i18n!(cx, "welcome.subtitle")),
                            p.surface_accent,
                        )
                    } else if let velowork_core::shell::ShellType::Custom { path, args } = shell_type {
                        if path == "ssh" {
                            let store = cx.global::<GlobalSessionStore>().0.read(cx);
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
                            let conn_info = if let Some(sid) = session_id {
                                if let Some(session) = store.find_session(&sid) {
                                    let user_prefix = if !session.username.is_empty() {
                                        format!("{}@", session.username)
                                    } else {
                                        String::new()
                                    };
                                    let port_suffix = if session.port != 22 && session.port != 0 {
                                        format!(":{}", session.port)
                                    } else {
                                        String::new()
                                    };
                                    Some(format!("{}{}{}", user_prefix, session.host, port_suffix))
                                } else {
                                    host_arg
                                }
                            } else {
                                host_arg
                            };
                            (
                                AppIcon::Server,
                                Some("SSH".to_string()),
                                conn_info,
                                if connection_lost { p.status_error } else { p.status_success },
                            )
                        } else if path == "serial" {
                            let store = cx.global::<GlobalSessionStore>().0.read(cx);
                            let mut session_id = None;
                            let mut port_arg = None;
                            let mut baud_arg = None;
                            let mut i = 0;
                            while i < args.len() {
                                if args[i] == "--id" && i + 1 < args.len() {
                                    session_id = Some(args[i + 1].clone());
                                    i += 2;
                                } else if args[i] == "--port" && i + 1 < args.len() {
                                    port_arg = Some(args[i + 1].clone());
                                    i += 2;
                                } else if args[i] == "--baud" && i + 1 < args.len() {
                                    baud_arg = Some(args[i + 1].clone());
                                    i += 2;
                                } else {
                                    i += 1;
                                }
                            }
                            let conn_info = if let Some(sid) = session_id {
                                if let Some(session) = store.find_session(&sid) {
                                    let port_str = session.serial_port.as_deref().unwrap_or(port_arg.as_deref().unwrap_or(""));
                                    Some(format!("{} · {} baud", port_str, session.serial_baud_rate))
                                } else {
                                    port_arg.map(|p| format!("{} · {} baud", p, baud_arg.unwrap_or_else(|| "115200".to_string())))
                                }
                            } else {
                                port_arg.map(|p| format!("{} · {} baud", p, baud_arg.unwrap_or_else(|| "115200".to_string())))
                            };
                            (
                                AppIcon::Serial,
                                Some("Serial".to_string()),
                                conn_info,
                                if connection_lost { p.status_error } else { p.status_warning },
                            )
                        } else if path == "telnet" {
                            let store = cx.global::<GlobalSessionStore>().0.read(cx);
                            let mut session_id = None;
                            let mut host_arg = None;
                            let mut port_arg = None;
                            let mut i = 0;
                            while i < args.len() {
                                if args[i] == "--id" && i + 1 < args.len() {
                                    session_id = Some(args[i + 1].clone());
                                    i += 2;
                                } else if args[i] == "--host" && i + 1 < args.len() {
                                    host_arg = Some(args[i + 1].clone());
                                    i += 2;
                                } else if args[i] == "--port" && i + 1 < args.len() {
                                    port_arg = Some(args[i + 1].clone());
                                    i += 2;
                                } else {
                                    i += 1;
                                }
                            }
                            let conn_info = if let Some(sid) = session_id {
                                if let Some(session) = store.find_session(&sid) {
                                    let host = session.telnet_host.as_deref().unwrap_or(session.host.as_str());
                                    let port = if session.telnet_port > 0 { session.telnet_port } else { 23 };
                                    Some(format!("{}:{}", host, port))
                                } else if let Some(host) = host_arg {
                                    Some(format!("{}:{}", host, port_arg.unwrap_or_else(|| "23".to_string())))
                                } else {
                                    None
                                }
                            } else if let Some(host) = host_arg {
                                Some(format!("{}:{}", host, port_arg.unwrap_or_else(|| "23".to_string())))
                            } else {
                                None
                            };
                            (
                                AppIcon::Telnet,
                                Some("Telnet".to_string()),
                                conn_info,
                                if connection_lost { p.status_error } else { p.status_info },
                            )
                        } else {
                            (
                                if is_remote_session { AppIcon::Server } else { AppIcon::Terminal },
                                Some(if is_remote_session { "SSH".to_string() } else { "Local".to_string() }),
                                Some(path.clone()),
                                if connection_lost { p.status_error } else { p.status_success },
                            )
                        }
                    } else {
                        (
                            if is_remote_session { AppIcon::Server } else { AppIcon::Terminal },
                            Some(if is_remote_session { "SSH".to_string() } else { "Local".to_string() }),
                            Some(shell_type.display_name()),
                            if connection_lost { p.status_error } else { p.status_success },
                        )
                    }
                }
                _ => (
                    AppIcon::Terminal,
                    Some("Local".to_string()),
                    None,
                    if connection_lost { p.status_error } else { p.status_success },
                ),
            };

            let preview_snapshot = terminal_id.as_ref().and_then(|tid| {
                let guard = terminals.lock();
                guard.get(tid).map(|t| t.preview_snapshot(16))
            });

            let terminal_font = crate::terminal_view_settings(cx).font_family.clone();

            let term_palette = terminal_id
                .as_ref()
                .and_then(|tid| {
                    let guard = terminals.lock();
                    guard.get(tid).and_then(|t| t.palette())
                })
                .unwrap_or_else(|| {
                    let tvs = crate::terminal_view_settings(cx);
                    velowork_core::theme::get_terminal_palette_with_custom(
                        &tvs.color_scheme,
                        &tvs.custom_terminal_color_schemes,
                    )
                });

            let background_builder = {
                let anim_id: SharedString = format!("tab-preview-bg-{}", i).into();
                Some(std::sync::Arc::new(move |cx: &App| {
                    crate::background_cache::terminal_background_element(cx, anim_id.clone(), false, f32::from(RADIUS_MD))
                }) as std::sync::Arc<dyn Fn(&App) -> Option<AnyElement> + Send + Sync>)
            };

            let preview_props = velowork_ui::TerminalPreviewProps {
                title: tab_label.clone(),
                icon: preview_icon,
                icon_color: Some(preview_icon_color),
                is_remote: is_remote_session,
                status_text: Some(status_text.clone()),
                idle_text: idle_label.clone(),
                is_disconnected: connection_lost,
                is_welcome,
                connection_info,
                protocol_badge,
                snapshot: preview_snapshot,
                font_family: Some(terminal_font),
                palette: Some(term_palette),
                background_builder,
            };

            let is_preview_target = enable_tab_preview && !is_active && self.preview_tab == Some(i) && self.preview_opened;
            if is_preview_target {
                *active_preview_props.borrow_mut() = Some(preview_props.clone());
            }

            let tab_element = if enable_tab_preview && !is_active {
                tab_element
            } else {
                tab_element.tooltip(move |_, cx| {
                    cx.new(|_| velowork_ui::tooltip::Tooltip::new(tab_tooltip_text.clone())).into()
                })
            };

            let bounds_map = self.tab_bounds.clone();
            let tab_idx = i;

            let tab_item = tab_element
                .child(
                    canvas(
                        move |bounds, _window, _cx| {
                            bounds_map.borrow_mut().insert(tab_idx, bounds);
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                .overflow_hidden()
                .when(has_drop_animation, |d| {
                    let glow_alpha = animation_progress * 0.5;
                    d.bg(with_alpha(t.border_active, glow_alpha))
                        .border_1()
                        .border_color(with_alpha(t.border_active, animation_progress * 0.9))
                        .rounded(RADIUS_STD)
                })
                .on_hover({
                    let weak = this_weak.clone();
                    move |&hovered, _window, cx| {
                        if let Some(this) = weak.upgrade() {
                            this.update(cx, |this, cx| {
                                let hovered_tab_changed = match (hovered, this.hovered_tab) {
                                    (true, Some(cur)) if cur == i => false,
                                    (false, Some(cur)) if cur == i => {
                                        this.hovered_tab = None;
                                        true
                                    }
                                    (true, _) => {
                                        this.hovered_tab = Some(i);
                                        true
                                    }
                                    (false, _) => false,
                                };

                                let preview_enabled = enable_tab_preview && !is_active;
                                if preview_enabled {
                                    if hovered {
                                        if this.preview_opened {
                                            if this.preview_tab != Some(i) {
                                                this.preview_tab = Some(i);
                                                this.preview_is_fresh = false;
                                                this.preview_seq = this.preview_seq.wrapping_add(1);
                                                cx.notify();
                                            }
                                        } else {
                                            this.preview_tab = Some(i);
                                            this.preview_is_fresh = true;
                                            this.preview_seq = this.preview_seq.wrapping_add(1);
                                            let seq = this.preview_seq;
                                            let entity = weak.clone();
                                            cx.spawn(async move |_, cx| {
                                                smol::Timer::after(std::time::Duration::from_millis(200)).await;
                                                let _ = entity.update(cx, |this, cx| {
                                                    if this.preview_seq == seq && this.preview_tab == Some(i) {
                                                        this.preview_opened = true;
                                                        cx.notify();
                                                    }
                                                });
                                            }).detach();
                                        }
                                    } else if this.preview_tab == Some(i) {
                                        this.preview_seq = this.preview_seq.wrapping_add(1);
                                        let seq = this.preview_seq;
                                        let entity = weak.clone();
                                        cx.spawn(async move |_, cx| {
                                            smol::Timer::after(std::time::Duration::from_millis(80)).await;
                                            let _ = entity.update(cx, |this, cx| {
                                                if this.preview_seq == seq {
                                                    this.preview_tab = None;
                                                    this.preview_opened = false;
                                                    this.preview_is_fresh = true;
                                                    cx.notify();
                                                }
                                            });
                                        }).detach();
                                    }
                                }

                                if hovered_tab_changed {
                                    cx.notify();
                                }
                            });
                        }
                    }
                })
                .child({
                    let rename_input_elem = if is_renaming_this {
                        rename_input(&self.tab_rename_state)
                    } else {
                        None
                    };

                    let is_tab_hovered = self.hovered_tab == Some(i);
                    let should_show_close = is_tab_hovered && !is_renaming_this && terminal_id.is_some();
                    let left_element = if should_show_close {
                        let tid = terminal_id.clone().unwrap_or_default();
                        let project_id_for_tab_close = project_id.clone();
                        let dispatcher_for_tab_close = self.action_dispatcher.clone();
                        let close_tab_label = i18n!(cx, "terminal.close_tab");
                        let weak_close = this_weak.clone();

                        div()
                            .id(ElementId::Name(format!("tab-close-{}-{:?}", i, layout_path).into()))
                            .group("tab-close-btn")
                            .flex_shrink_0()
                            .size(ui_icon_std_ts(cx))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(RADIUS_XS)
                            .hover(|s| s.bg(rgba(0xf14c4c99)))
                            .tooltip(move |_, cx| {
                                cx.new(|_| velowork_ui::tooltip::Tooltip::new(close_tab_label.clone())).into()
                            })
                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .on_click(move |_, _window, cx| {
                                cx.stop_propagation();
                                if let Some(this) = weak_close.upgrade() {
                                    this.update(cx, |this, _| {
                                        this.hovered_tab = None;
                                        this.preview_tab = None;
                                        this.preview_opened = false;
                                        this.preview_seq = this.preview_seq.wrapping_add(1);
                                    });
                                }
                                if let Some(ref dispatcher) = dispatcher_for_tab_close {
                                    dispatcher.dispatch(
                                        velowork_core::api::ActionRequest::CloseTerminal {
                                            project_id: project_id_for_tab_close.clone(),
                                            terminal_id: tid.clone(),
                                        },
                                        cx,
                                    );
                                }
                            })
                            .child(
                                AppIcon::Close
                                    .size(ui_icon_std_ts(cx) - px(2.0))
                                    .text_color(p.text_muted)
                                    .group_hover("tab-close-btn", |s| s.text_color(p.text_primary)),
                            )
                            .into_any_element()
                    } else {
                        self.render_tab_icon_element(
                            preview_icon,
                            preview_icon_color,
                            cx,
                        )
                    };

                    h_flex()
                        .items_center()
                        .flex_1()
                        .min_w_0()
                        .gap(SPACE_XS)
                        .overflow_hidden()
                        .child(left_element)
                        .child({
                            if let Some(input) = rename_input_elem {
                                div()
                                    .id(format!("tab-rename-{}", i))
                                    .key_context("TerminalRename")
                                    .flex_1()
                                    .min_w(px(60.0))
                                    .child(
                                        Input::new(input)
                                            .compact()
                                            .h(px(20.0))
                                            .text_size(ui_text_md(cx)),
                                    )
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .on_click(|_, _window, cx| {
                                        cx.stop_propagation();
                                    })
                                    .on_action(cx.listener(|this, _: &Cancel, _window, cx| {
                                        this.cancel_tab_rename(cx);
                                    }))
                                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                                        cx.stop_propagation();
                                        if event.keystroke.key.as_str() == "enter" {
                                            this.finish_tab_rename(cx);
                                        }
                                    }))
                                    .into_any_element()
                            } else {
                                div()
                                    .text_size(ui_text_md(cx))
                                    .when(
                                        tab_width_mode != TabWidthMode::TitleLength,
                                        |d| d.flex_1().min_w_0().truncate(),
                                    )
                                    .child(tab_label.clone())
                                    .into_any_element()
                            }
                        })
                        .children(idle_label.as_ref().filter(|_| !is_renaming_this).map(|d| {
                            div().text_size(ui_text_sm(cx)).text_color(rgb(t.text_muted)).child(d.clone())
                        }))
                        .into_any_element()
                })
                .on_mouse_down(MouseButton::Right, {
                    let project_id = project_id.clone();
                    let layout_path = layout_path.clone();
                    cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                        this.request_broker.update(cx, |broker, cx| {
                            broker.push_overlay_request(
                                velowork_workspace::requests::OverlayRequest::Project(velowork_workspace::requests::ProjectOverlay {
                                    project_id: project_id.clone(),
                                    kind: velowork_workspace::requests::ProjectOverlayKind::TabContextMenu {
                                        tab_index: i,
                                        num_tabs: num_children,
                                        layout_path: layout_path.clone(),
                                        position: event.position,
                                    },
                                }),
                                cx,
                            );
                        });
                        cx.stop_propagation();
                    })
                })
                .on_mouse_down(MouseButton::Middle, {
                    let project_id = project_id.clone();
                    let terminal_id = terminal_id.clone();
                    let action_dispatcher = self.action_dispatcher.clone();
                    let weak_middle = this_weak.clone();
                    cx.listener(move |_this, _event: &MouseDownEvent, _window, cx| {
                        if let Some(this) = weak_middle.upgrade() {
                            this.update(cx, |this, _| {
                                this.preview_tab = None;
                                this.preview_opened = false;
                                this.preview_seq = this.preview_seq.wrapping_add(1);
                            });
                        }
                        if let Some(ref tid) = terminal_id
                            && let Some(ref dispatcher) = action_dispatcher {
                                dispatcher.dispatch(velowork_core::api::ActionRequest::CloseTerminal {
                                    project_id: project_id.clone(),
                                    terminal_id: tid.clone(),
                                }, cx);
                            }
                        cx.stop_propagation();
                    })
                })
                .when_some(terminal_id.clone(), |el, tid| {
                    let terminal_path = if standalone {
                        layout_path_for_drag.clone()
                    } else {
                        let mut p = layout_path_for_drag.clone();
                        p.push(i);
                        p
                    };
                    el.on_drag(
                        PaneDrag {
                            project_id: project_id_for_drag.clone(),
                            layout_path: terminal_path,
                            terminal_id: tid,
                            terminal_name: tab_label.clone(),
                        },
                        move |drag, _position, _window, cx| {
                            cx.new(|_| PaneDragView::new(drag.terminal_name.clone()))
                        },
                    )
                })
                .drag_over::<PaneDrag>({
                    let active_drag = self.active_drag.clone();
                    move |style, _, _, _| {
                        if active_drag.borrow().is_some() {
                            return style;
                        }
                        style
                            .border_l(px(3.0))
                            .border_color(rgb(t.border_active))
                            .bg(with_alpha(t.border_active, 0.15))
                    }
                })
                .on_drop(cx.listener({
                    let active_drag = self.active_drag.clone();
                    let dispatcher_for_drop = self.action_dispatcher.clone();
                    let target_tid = terminal_id.clone();
                    move |this, drag: &PaneDrag, _window, cx| {
                        if active_drag.borrow().is_some() {
                            return;
                        }

                        if standalone {
                            if let (Some(target_tid), Some(ref dispatcher)) = (target_tid.as_ref(), dispatcher_for_drop.as_ref()) {
                                if &drag.terminal_id != target_tid {
                                    dispatcher.dispatch(velowork_core::api::ActionRequest::MovePaneTo {
                                        project_id: drag.project_id.clone(),
                                        terminal_id: drag.terminal_id.clone(),
                                        target_project_id: project_id_for_drop.clone(),
                                        target_terminal_id: (*target_tid).clone(),
                                        zone: "center".to_string(),
                                    }, cx);
                                }
                            }
                            return;
                        }

                        let drag_parent = &drag.layout_path[..drag.layout_path.len().saturating_sub(1)];
                        let drag_tab_index = drag.layout_path.last().copied();

                        if drag.project_id == project_id_for_drop
                            && drag_parent == layout_path_for_drop.as_slice()
                        {
                            if let Some(from_index) = drag_tab_index
                                && from_index != i {
                                    let target_index = if from_index < i { i - 1 } else { i };
                                    if let Some(ref dispatcher) = dispatcher_for_drop {
                                        dispatcher.dispatch(velowork_core::api::ActionRequest::MoveTab {
                                            project_id: project_id_for_drop.clone(),
                                            path: layout_path_for_drop.clone(),
                                            from_index,
                                            to_index: i,
                                        }, cx);
                                    }
                                    this.start_drop_animation(target_index, cx);
                                }
                        } else if let Some(ref dispatcher) = dispatcher_for_drop {
                            dispatcher.dispatch(velowork_core::api::ActionRequest::MoveTerminalToTabGroup {
                                project_id: drag.project_id.clone(),
                                terminal_id: drag.terminal_id.clone(),
                                target_path: layout_path_for_drop.clone(),
                                position: Some(i),
                                target_project_id: Some(project_id_for_drop.clone()),
                            }, cx);
                        }
                    }
                }))
                .on_click({
                    let workspace = workspace.clone();
                    let project_id = project_id.clone();
                    let layout_path = layout_path.clone();
                    let terminal_id = terminal_id.clone();
                    let tab_label = tab_label.clone();
                    let dispatcher_for_click = self.action_dispatcher.clone();
                    cx.listener(move |this, _, window, cx| {
                        this.preview_tab = None;
                        this.preview_opened = false;
                        this.preview_seq = this.preview_seq.wrapping_add(1);

                        let is_double_click = this.tab_click_detector.check(i);

                        if this.tab_rename_state.is_some() && !is_double_click {
                            let is_renaming_this = terminal_id.as_ref().is_some_and(|tid| {
                                is_renaming(&this.tab_rename_state, tid)
                            });
                            if !is_renaming_this {
                                this.cancel_tab_rename(cx);
                            }
                        }

                        if !standalone
                            && let Some(ref dispatcher) = dispatcher_for_click {
                                dispatcher.dispatch(velowork_core::api::ActionRequest::SetActiveTab {
                                    project_id: project_id.clone(),
                                    path: layout_path.clone(),
                                    index: i,
                                }, cx);
                            }

                        let terminal_path = if standalone {
                            layout_path.clone()
                        } else {
                            let mut p = layout_path.clone();
                            p.push(i);
                            p
                        };
                        let workspace_clone = workspace.clone();
                        let pid = project_id.clone();
                        let tpath = terminal_path.clone();
                        this.focus_manager.update(cx, |fm, cx| {
                            workspace_clone.update(cx, |ws, cx| {
                                ws.set_focused_terminal(fm, pid.clone(), tpath.clone(), cx);
                            });
                            cx.notify();
                        });

                        let pane_map = crate::layout::navigation::get_pane_map(this.window_id);
                        if let Some(pane) = pane_map.find_pane(&pid, &terminal_path) {
                            if let Some(ref handle) = pane.focus_handle {
                                handle.focus(window, cx);
                            }
                        } else {
                            let window_id = this.window_id;
                            let pid_clone = pid.clone();
                            let tpath_clone = terminal_path.clone();
                            window.on_next_frame(move |window, cx| {
                                let pane_map = crate::layout::navigation::get_pane_map(window_id);
                                if let Some(pane) = pane_map.find_pane(&pid_clone, &tpath_clone)
                                    && let Some(ref handle) = pane.focus_handle {
                                        handle.focus(window, cx);
                                    }
                            });
                        }

                        if is_double_click
                            && let Some(ref tid) = terminal_id {
                                this.start_tab_rename(tid.clone(), tab_label.clone(), window, cx);
                            }
                    })
                });

            let collapsing_info = terminal_id
                .as_ref()
                .and_then(|tid| self.collapsing_tabs.get(tid).copied());
            let restoring_info = terminal_id
                .as_ref()
                .and_then(|tid| self.restoring_tabs.get(tid).copied());

            let base_tab_w = match tab_width_mode {
                TabWidthMode::Compact => 85.0f32,
                TabWidthMode::Equal => 155.0f32,
                TabWidthMode::TitleLength => 140.0f32,
            };

            let is_hidden = child.is_all_hidden();
            if enable_animations && is_hidden && let Some((_, seq)) = collapsing_info {
                let tid = terminal_id.clone().unwrap_or_default();
                div()
                    .id(ElementId::Name(format!("tab-wrapper-collapse-{}-{}", tid, seq).into()))
                    .with_animation(
                        format!("tab-collapse-{}-{}", tid, seq),
                        Animation::new(std::time::Duration::from_millis(280))
                            .with_easing(velowork_ui::motion::ease_tab_collapse),
                        move |this, delta| {
                            let t = delta;
                            let cur_w = (base_tab_w * (1.0 - t)).max(0.0);
                            this.w(px(cur_w))
                                .max_w(px(cur_w))
                                .flex_shrink_0()
                                .opacity((1.0 - t).max(0.0))
                                .overflow_hidden()
                        },
                    )
                    .child(tab_item.w_full())
                    .into_any_element()
            } else if enable_animations && !is_hidden && let Some((_, seq)) = restoring_info {
                let tid = terminal_id.clone().unwrap_or_default();
                div()
                    .id(ElementId::Name(format!("tab-wrapper-expand-{}-{}", tid, seq).into()))
                    .with_animation(
                        format!("tab-expand-{}-{}", tid, seq),
                        Animation::new(std::time::Duration::from_millis(280))
                            .with_easing(velowork_ui::motion::ease_tab_expand),
                        move |this, delta| {
                            let t = delta;
                            let cur_w = (base_tab_w * t).min(base_tab_w);
                            this.w(px(cur_w))
                                .max_w(px(cur_w))
                                .flex_shrink_0()
                                .opacity(t.min(1.0))
                                .overflow_hidden()
                        },
                    )
                    .child(tab_item.w_full())
                    .into_any_element()
            } else {
                tab_item.into_any_element()
            }
        }).collect();

        let project_id_for_new = self.project_id.clone();
        let layout_path_for_new = self.layout_path.clone();
        let dispatcher_for_new = self.action_dispatcher.clone();

        let mut end_drop_zone = div()
            .id(ElementId::Name(
                format!("tab-end-drop-{:?}", self.layout_path).into(),
            ))
            .flex_1()
            .flex_shrink_0()
            .h_full()
            .min_w(px(20.0))
            .on_click(cx.listener(move |this, _, _window, cx| {
                if this.empty_area_click_detector.check(())
                    && let Some(ref dispatcher) = dispatcher_for_new
                {
                    log::debug!(
                        "[tabs:add_welcome_tab] project_id={} path={:?}",
                        project_id_for_new, layout_path_for_new
                    );
                    dispatcher.add_tab_with_shell(
                        &project_id_for_new,
                        &layout_path_for_new,
                        velowork_core::shell::ShellType::Welcome,
                        !standalone,
                        cx,
                    );
                }
            }));

        if !standalone {
            let active_drag_for_end_hover = self.active_drag.clone();
            let active_drag_for_end_drop = self.active_drag.clone();
            let project_id_for_end = self.project_id.clone();
            let layout_path_for_end = self.layout_path.clone();
            let dispatcher_for_end = self.action_dispatcher.clone();

            end_drop_zone = end_drop_zone
                .drag_over::<PaneDrag>(move |style, _, _, _| {
                    if active_drag_for_end_hover.borrow().is_some() {
                        return style;
                    }
                    style
                        .border_l(px(3.0))
                        .border_color(rgb(t.border_active))
                        .bg(with_alpha(t.border_active, 0.1))
                })
                .on_drop(cx.listener(move |this, drag: &PaneDrag, _window, cx| {
                    if active_drag_for_end_drop.borrow().is_some() {
                        return;
                    }

                    let drag_parent = &drag.layout_path[..drag.layout_path.len().saturating_sub(1)];
                    let drag_tab_index = drag.layout_path.last().copied();

                    if drag.project_id == project_id_for_end
                        && drag_parent == layout_path_for_end.as_slice()
                    {
                        if let Some(from_index) = drag_tab_index {
                            let target_index = num_children;
                            if from_index != target_index - 1 {
                                if let Some(ref dispatcher) = dispatcher_for_end {
                                    dispatcher.dispatch(
                                        velowork_core::api::ActionRequest::MoveTab {
                                            project_id: project_id_for_end.clone(),
                                            path: layout_path_for_end.clone(),
                                            from_index,
                                            to_index: target_index,
                                        },
                                        cx,
                                    );
                                }
                                this.start_drop_animation(num_children - 1, cx);
                            }
                        }
                    } else if let Some(ref dispatcher) = dispatcher_for_end {
                        dispatcher.dispatch(
                            velowork_core::api::ActionRequest::MoveTerminalToTabGroup {
                                project_id: drag.project_id.clone(),
                                terminal_id: drag.terminal_id.clone(),
                                target_path: layout_path_for_end.clone(),
                                position: None,
                                target_project_id: Some(project_id_for_end.clone()),
                            },
                            cx,
                        );
                    }
                }));
        }

        let action_ctx = TabActionContext {
            workspace: self.workspace.clone(),
            project_id: self.project_id.clone(),
            layout_path: self.layout_path.clone(),
            active_tab,
            standalone,
            action_dispatcher: self.action_dispatcher.clone(),
        };

        let terminal_id_for_actions = if standalone {
            match children.first() {
                Some(LayoutNode::Terminal { terminal_id, .. }) => terminal_id.clone(),
                _ => None,
            }
        } else {
            self.get_active_terminal_id(active_tab, cx)
        };

        let action_buttons =
            self.render_tab_action_buttons(action_ctx, terminal_id_for_actions.clone(), cx);

        // Only show the shell-selector button when the *active* tab is a local
        // terminal. A remote backend (whole terminal connected to a host) or an
        // `ssh` shell inside a local backend both run their shell remotely, where
        // switching the local shell is meaningless, so the button is hidden there.
        let active_shell = self.get_active_shell_type(active_tab, cx);
        let show_shell = terminal_view_settings(cx).show_shell_selector
            && !self.backend.is_remote()
            && !active_shell.is_remote();

        if self.last_scrolled_to_tab != Some(active_tab) {
            self.tab_scroll_handle.scroll_to_item(active_tab);
            self.last_scrolled_to_tab = Some(active_tab);
        }

        let add_tab_id_suffix = format!("tabbar-{:?}", self.layout_path);
        let tab_dropdown_id_suffix = format!("tab-list-{:?}", self.layout_path);

        // Single global duplicate-session numbering, shared with the tab strip
        // and the command panel's target host list.
        let suffixes = duplicate_session_suffixes(&self.workspace.read(cx));

        // Flat list of (index, label, terminal_id, icon, shell_short) for
        // the searchable tab-list dropdown.
        let tab_infos: Vec<(usize, String, Option<String>, AppIcon, String)> = children
            .iter()
            .enumerate()
            .filter(|(_, child)| !child.is_all_hidden())
            .map(|(i, child)| {
                let tid = match child {
                    LayoutNode::Terminal { terminal_id, .. } => terminal_id.clone(),
                    _ => None,
                };
                let (icon, shell_short) = match child {
                    LayoutNode::Terminal { shell_type, .. } => {
                        let icon = match shell_type {
                            velowork_core::shell::ShellType::Custom { path, .. } if path == "serial" => AppIcon::Serial,
                            velowork_core::shell::ShellType::Custom { path, .. } if path == "telnet" => AppIcon::Telnet,
                            velowork_core::shell::ShellType::Custom { path, .. } if path == "ssh" => AppIcon::Server,
                            _ if shell_type.is_remote() || self.backend.is_remote() => AppIcon::Server,
                            _ => AppIcon::Terminal,
                        };
                        let short = if shell_type.is_remote() || self.backend.is_remote() {
                            String::new()
                        } else {
                            shell_type.local_shell_name()
                        };
                        (icon, short)
                    }
                    _ => (AppIcon::Terminal, "?".to_string()),
                };
                (
                    i,
                    self.tab_display_label(children, i, cx, &suffixes),
                    tid,
                    icon,
                    shell_short,
                )
            })
            .collect();

        let tab_list_btn_bounds = self.tab_list_btn_bounds.clone();

        let item_height = px((tab_height(cx) - 6.0).max(24.0));
        let bar_height = super::layout_container::compute_tab_bar_height(cx);
        let image_set = terminal_view_settings(cx)
            .terminal_background_image
            .as_ref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);

        let is_fullscreen = self.focus_manager.read(cx).has_fullscreen();
        let in_split = self.is_project_split(cx);
        let has_top_corners = !is_fullscreen && !in_split;

        let tab_bar_bg = if image_set {
            with_alpha(t.bg_header, 0.85 * bg_opacity(cx))
        } else {
            p.surface_header
        };

        let preview_overlay = if let (Some(preview_idx), Some(props)) = (self.preview_tab, active_preview_props.borrow_mut().take()) {
            let bounds = self.tab_bounds.borrow().get(&preview_idx).copied();
            bounds.map(|b| {
                let enable_animations = velowork_app_core::settings::settings(cx).enable_animations;
                let is_fresh = self.preview_is_fresh;
                let anim_id: SharedString = format!("tab-preview-overlay-{:?}-{}", self.layout_path, preview_idx).into();
                velowork_ui::render_anchored_preview(
                    anim_id,
                    props,
                    b,
                    velowork_ui::TabPreviewPlacement::Below,
                    is_fresh,
                    enable_animations,
                    Some(window.viewport_size()),
                    cx,
                )
            })
        } else {
            None
        };

        div()
            .group("tab-bar-row")
            .h(bar_height)
            .pl(SPACE_SM)
            .pr(SPACE_SM)
            .pt(SPACE_SM)
            .pb(SPACE_SM)
            .flex()
            .items_center()
            .gap(SPACE_SM)
            .relative()
            .when(has_top_corners, |d| d.rounded_t(RADIUS_CARD))
            .bg(tab_bar_bg)
            .child(
                // Tab-list toggle button pinned to the front (leftmost) of the row.
                // A full-size canvas captures the button's global bounds so the
                // dropdown can be anchored precisely below it.
                div().flex_shrink_0().child({
                    let tab_infos = tab_infos.clone();
                    let tab_list_btn_bounds = tab_list_btn_bounds.clone();
                    header_button_base(
                        HeaderAction::TabList,
                        &tab_dropdown_id_suffix,
                        &t,
                        None,
                        None,
                        cx,
                    )
                    .child(
                        canvas(
                            {
                                let tab_list_btn_bounds = tab_list_btn_bounds.clone();
                                move |bounds, _window, _cx| {
                                    *tab_list_btn_bounds.borrow_mut() = bounds;
                                }
                            },
                            |_bounds, _prepaint, _window, _cx| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .on_click({
                        let tab_infos = tab_infos.clone();
                        cx.listener(move |this, _, window, cx| {
                            this.toggle_tab_dropdown(
                                tab_infos.clone(),
                                active_tab,
                                standalone,
                                window,
                                cx,
                            );
                        })
                    })
                }),
            )
            .child(
                div()
                    .id(ElementId::Name(
                        format!("tab-scroll-container-{:?}", self.layout_path).into(),
                    ))
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h(item_height)
                    .child(
                        div()
                            .id(ElementId::Name(
                                format!("tab-scroll-{:?}", self.layout_path).into(),
                            ))
                            .size_full()
                            .flex()
                            .items_center()
                            .gap(SPACE_SM)
                            .overflow_x_scroll()
                            .track_scroll(&self.tab_scroll_handle)
                            .children(tab_elements)
                            .child(end_drop_zone),
                    )
                    .child(
                        Scrollbar::horizontal(&self.tab_scroll_handle)
                            .scrollbar_show(ScrollbarShow::Hover),
                    ),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(SPACE_SM)
                    .child(
                        header_button_base(
                            HeaderAction::AddTab,
                            &add_tab_id_suffix,
                            &t,
                            None,
                            None,
                            cx,
                        )
                        .on_click({
                            let project_id = self.project_id.clone();
                            let layout_path = self.layout_path.clone();
                            let action_dispatcher = self.action_dispatcher.clone();
                            move |_, _window, cx| {
                                log::debug!(
                                    "[tabs:add_tab] project_id={} path={:?} standalone={}",
                                    project_id, layout_path, standalone
                                );
                                if let Some(ref dispatcher) = action_dispatcher {
                                    dispatcher.add_tab(&project_id, &layout_path, !standalone, cx);
                                }
                            }
                        }),
                    )
                    .when(show_shell, |el| {
                        el.child(self.render_shell_indicator(active_tab, cx))
                    })
                    .child(action_buttons),
            )
            .when_some(self.tab_list_menu.clone(), |this, menu| {
                this.child(
                    div()
                        .id(ElementId::Name(
                            format!("tab-list-menu-container-{:?}", self.layout_path).into(),
                        ))
                        .absolute()
                        .child(menu),
                )
            })
            // 终端顶部 “更多” 下拉菜单，与 dock 栏保持统一 PopupMenu 实现
            .when_some(self.more_menu.clone(), |this, menu| {
                this.child(
                    div()
                        .id(ElementId::Name(
                            format!("more-menu-container-{:?}", self.layout_path).into(),
                        ))
                        .absolute()
                        .child(menu),
                )
            })
            .when_some(preview_overlay, |this, overlay| {
                this.child(overlay)
            })
    }

    /// Toggle the searchable tab-list dropdown menu (`OverlayMenu`).
    fn toggle_tab_dropdown(
        &mut self,
        tab_infos: Vec<(usize, String, Option<String>, AppIcon, String)>,
        active_tab: usize,
        standalone: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(at) = self.tab_list_toggle_guard.take() {
            if at.elapsed() < std::time::Duration::from_millis(350) {
                return;
            }
        }

        if let Some(menu) = self.tab_list_menu.take() {
            menu.update(cx, |m, cx| {
                m.set_on_close(None);
                m.close(window, cx);
            });
            cx.notify();
            return;
        }

        let mut menu_items = Vec::new();
        let project_id = self.project_id.clone();
        let layout_path = self.layout_path.clone();
        let action_dispatcher = self.action_dispatcher.clone();
        let workspace = self.workspace.clone();
        let focus_manager = self.focus_manager.clone();
        let this_weak = cx.entity().downgrade();

        for (i, label, tid, icon, _shell_short) in tab_infos {
            let click_project_id = project_id.clone();
            let click_layout_path = layout_path.clone();
            let click_dispatcher = action_dispatcher.clone();
            let tid_for_focus = tid.clone();

            let close_project_id = project_id.clone();
            let close_dispatcher = action_dispatcher.clone();
            let tid_for_close = tid.clone();

            let ws = workspace.clone();
            let fm = focus_manager.clone();

            let trailing_actions = if let Some(ref tid) = tid_for_close {
                let tid_close = tid.clone();
                let pid_close = close_project_id.clone();
                let disp_close = close_dispatcher.clone();
                let close_tip = i18n!(cx, "terminal.close_tab");
                vec![OverlayMenuAction {
                    id: "close".to_string(),
                    icon: AppIcon::Close,
                    tooltip: Some(close_tip),
                    action: Arc::new(move |_window, cx| {
                        if let Some(ref dispatcher) = disp_close {
                            dispatcher.dispatch(
                                velowork_core::api::ActionRequest::CloseTerminal {
                                    project_id: pid_close.clone(),
                                    terminal_id: tid_close.clone(),
                                },
                                cx,
                            );
                        }
                    }),
                }]
            } else {
                Vec::new()
            };

            menu_items.push(OverlayMenuEntry::Item(OverlayMenuItem {
                id: format!("tab-{}", i),
                icon: Some(icon),
                label,
                color_dot: None,
                shortcut: None,
                action: Arc::new(move |_, cx| {
                    if !standalone && let Some(ref dispatcher) = click_dispatcher {
                        dispatcher.dispatch(
                            velowork_core::api::ActionRequest::SetActiveTab {
                                project_id: click_project_id.clone(),
                                path: click_layout_path.clone(),
                                index: i,
                            },
                            cx,
                        );
                    }

                    if tid_for_focus.is_some() {
                        let terminal_path = if standalone {
                            click_layout_path.clone()
                        } else {
                            let mut p = click_layout_path.clone();
                            p.push(i);
                            p
                        };
                        let ws = ws.clone();
                        let fm = fm.clone();
                        let pid = click_project_id.clone();
                        fm.update(cx, |fm, cx| {
                            ws.update(cx, |ws, cx| {
                                ws.set_focused_terminal(fm, pid, terminal_path, cx);
                            });
                            cx.notify();
                        });
                    }
                }),
                trailing_actions,
            }));
        }

        let search_placeholder = i18n!(cx, "terminal.tab_search_placeholder");
        let bounds = *self.tab_list_btn_bounds.borrow();
        let overlay_registry = self.overlay_registry.clone();

        let this_weak_close = this_weak.clone();
        let menu = cx.new(move |cx| {
            OverlayMenu::new(
                cx,
                menu_items,
                Some(search_placeholder),
                bounds,
                Some(Arc::new(move |_, cx| {
                    if let Some(this) = this_weak_close.upgrade() {
                        let _ = this.update(cx, |this, cx| {
                            this.tab_list_menu = None;
                            this.tab_list_toggle_guard = Some(std::time::Instant::now());
                            cx.notify();
                        });
                    }
                })),
                overlay_registry,
                SharedString::from("terminal-tab-list-menu"),
            )
            .with_direction(OverlayMenuDirection::Below)
            .with_min_width(px(260.0))
            .with_active_id(Some(format!("tab-{}", active_tab)))
            .with_text_size(ui_text_md(cx))
            .with_instant_open(true)
        });

        let focus_handle = menu.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
        self.tab_list_menu = Some(menu);
        cx.notify();
    }
}
