use crate::keybindings::{
    About, AddTab, Cancel, CheckForUpdates, CloseWindow, CyclePanelNext, CyclePanelPrev,
    EqualizeLayout, FocusBottomDock, FocusCenterDock, FocusLeftDock, FocusRightDock, InstallUpdate,
    NewProject, NewSession, NewWindow, OpenSettingsFile, Quit, ShowAboutDialog, ShowAiAssistant,
    ShowAiSettings, ShowCommandPalette, ShowHelp, ShowHistoryPanel, ShowImportSessionDialog,
    ShowKeybindings, ShowLogConsole, ShowProfileManager, ShowProjectManageDialog,
    ShowQuickCommandsPanel, ShowServicesPanel, ShowSettings, ShowThemeSelector, ShowTunnelsPanel,
    ShowUpdateDialog, TerminalInlineAi, ToggleCommandsPanel, ToggleLeftDock, ToggleLeftDockAutoHide,
    TogglePaneSwitcher, ToggleRightDock, ToggleRightToolbar, ToggleSftpPanel,
};
use crate::settings::{open_settings_file, settings_entity};
use crate::theme::{surface_bg, theme};
use crate::ui::tokens::{
    RADIUS_CARD, ui_right_toolbar_width, ui_space_card_gap, ui_space_window_padding, ui_space_md, ui_text_md, ui_text_xl,
};
use crate::views::layout::navigation::{get_pane_map, prune_pane_map};
use crate::views::layout::split_pane::{DragState, compute_resize, render_project_divider};
use gpui::prelude::*;
use gpui::*;
use velowork_i18n::i18n;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::h_flex;
use velowork_ui::tooltip::Tooltip;

use super::WindowView;

impl WindowView {
    /// Normalize raw project widths to percentages summing to 100%.
    fn normalize_widths(raw_widths: &[f32]) -> Vec<f32> {
        let total: f32 = raw_widths.iter().sum();
        if total > 0.0 {
            raw_widths.iter().map(|w| w / total * 100.0).collect()
        } else {
            let n = raw_widths.len();
            vec![100.0 / n as f32; n]
        }
    }

    /// Convert normalized percentage widths to pixel widths.
    fn to_pixel_widths(widths: &[f32], container_width: f32, min_col_width: f32) -> Vec<f32> {
        let num_dividers = widths.len().saturating_sub(1) as f32;
        let available_width = (container_width - num_dividers * 1.0).max(0.0);
        widths
            .iter()
            .map(|w| (available_width * w / 100.0).max(min_col_width))
            .collect()
    }

    /// Scroll the projects grid horizontally to ensure the focused project column is visible.
    pub(super) fn scroll_to_focused_project(
        &self,
        focused_id: Option<&str>,
        center: bool,
        cx: &Context<Self>,
    ) {
        let focused_id = match focused_id {
            Some(id) => id,
            None => return,
        };

        let workspace = self.workspace.read(cx);
        let fm = self.focus_manager.read(cx);

        // Don't scroll when zoomed to a single project
        if fm.fullscreen_project_id().is_some() {
            return;
        }

        let visible_projects: Vec<String> = workspace
            .visible_projects(
                self.window_id,
                fm.focused_project_id(),
                fm.is_focus_individual(),
            )
            .iter()
            .map(|p| p.id.clone())
            .collect();
        let num_projects = visible_projects.len();
        if num_projects <= 1 {
            return;
        }

        // Find the focused project's index
        let focused_idx = match visible_projects.iter().position(|id| id == focused_id) {
            Some(idx) => idx,
            None => return,
        };

        let is_rows = workspace.project_layout_mode(self.window_id).is_rows();
        let settings = settings_entity(cx).read(cx).settings.clone();
        let container_size = {
            let b = self.projects_grid_bounds.borrow();
            f32::from(if is_rows { b.size.height } else { b.size.width })
        };

        let raw_widths: Vec<f32> = visible_projects
            .iter()
            .map(|id| workspace.get_project_width(self.window_id, id, num_projects))
            .collect();
        let widths = Self::normalize_widths(&raw_widths);
        let pixel_widths =
            Self::to_pixel_widths(&widths, container_size, settings.min_column_width);

        // Compute the leading edge (along the grid axis) of the focused project
        let mut col_lead: f32 = 0.0;
        for width in &pixel_widths[..focused_idx] {
            col_lead += width + 1.0; // +1 for divider
        }

        let current_offset_axis = {
            let o = self.projects_scroll_handle.offset();
            f32::from(if is_rows { o.y } else { o.x })
        };

        let new_offset = if center {
            // Center the focused project in the viewport
            let col_center = col_lead + pixel_widths[focused_idx] / 2.0;
            -(col_center - container_size / 2.0)
        } else {
            let col_trail = col_lead + pixel_widths[focused_idx];
            let viewport_lead = -current_offset_axis;
            let viewport_trail = viewport_lead + container_size;

            if col_lead < viewport_lead {
                -col_lead
            } else if col_trail > viewport_trail {
                -(col_trail - container_size)
            } else {
                return; // already visible
            }
        };

        let max_offset = self.projects_scroll_handle.max_offset();
        if is_rows {
            let clamped = new_offset.clamp(-f32::from(max_offset.y), 0.0);
            self.projects_scroll_handle
                .set_offset(point(px(0.0), px(clamped)));
        } else {
            let clamped = new_offset.clamp(-f32::from(max_offset.x), 0.0);
            self.projects_scroll_handle
                .set_offset(point(px(clamped), px(0.0)));
        }
    }

    /// Render the window-level bottom dock container inside the center column.
    /// The bottom dock is a single WindowView-owned instance shared across the
    /// whole center region; its content follows the focused terminal.
    pub(super) fn render_bottom_dock_container(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bottom_dock = self.bottom_dock.clone();
        let mode = bottom_dock.read(cx).mode;
        let is_maximized = mode == velowork_ui::dock::PanelMode::Maximized
            || mode == velowork_ui::dock::PanelMode::Fullscreen;

        let is_custom_titlebar = if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
            self.initial_titlebar_style == velowork_workspace::settings::TitlebarStyle::Custom
        } else {
            matches!(
                window.window_decorations(),
                gpui::Decorations::Client { .. }
            )
        };
        let window_corner_radius = settings_entity(cx).read(cx).settings.window_corner_radius;

        // Bottom dock sits inside center-column above the status bar, so it stays
        // square (0.0) when in normal mode. When maximized, it fills the full window
        // height and must match the configurable window corner radius.
        {
            let r = Some(if is_maximized && is_custom_titlebar {
                window_corner_radius
            } else {
                0.0
            });
            bottom_dock.update(cx, |p, _| {
                if p.window_corner_radius != r {
                    p.window_corner_radius = r;
                }
            });
        }

        let dock = bottom_dock.read(cx);
        let configured_height = dock.size;
        let animation = self.bottom_dock_ctrl.animation();
        let current_height = animation * configured_height;
        let should_render = animation > 0.001 || is_maximized;

        if !should_render {
            return div().into_any_element();
        }

        let t = theme(cx);
        let _palette = SemanticPalette::from_theme(&t);
        let mut container = div().id("bottom-dock-container").flex_shrink_0();

        if is_maximized {
            // When maximized, the bottom dock takes up all remaining space
            container = container
                .flex_1()
                .min_h_0()
                .when(is_custom_titlebar && window_corner_radius > 0.0, |d| {
                    d.rounded_bl(px(window_corner_radius))
                        .rounded_br(px(window_corner_radius))
                })
                .child(bottom_dock);
        } else {
            let is_animating = self.bottom_dock_ctrl.is_animating();
            let card_gap = f32::from(ui_space_card_gap(cx));
            let dynamic_gap = animation * card_gap;
            // Normal mode - render at animated height with smooth gap and physical drawer sliding
            container = container
                .h(px(current_height))
                .mt(px(dynamic_gap))
                .w_full()
                .relative()
                .when(is_animating, |d| d.overflow_hidden())
                .child(if is_animating {
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(configured_height))
                        .child(bottom_dock)
                } else {
                    div().size_full().child(bottom_dock)
                });
        }

        container.into_any_element()
    }

    pub(super) fn render_projects_grid(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        // Execute pending center-scroll (deferred from unfocus to let layout update first).
        // We wait until the scroll handle reports overflow (max_offset > 0), which means
        // the layout has been recalculated with all projects visible.
        if let Some(project_id) = self.pending_center_scroll.take() {
            let workspace = self.workspace.read(cx);
            let fm = self.focus_manager.read(cx);
            let num_visible = workspace
                .visible_projects(
                    self.window_id,
                    fm.focused_project_id(),
                    fm.is_focus_individual(),
                )
                .len();
            let is_zoomed = fm.focused_project_id().is_some();

            let is_rows = workspace.project_layout_mode(self.window_id).is_rows();
            let max_offset = self.projects_scroll_handle.max_offset();
            let axis_overflow = if is_rows { max_offset.y } else { max_offset.x };
            if is_zoomed || num_visible <= 1 {
                // Still zoomed or only one project — no centering needed
            } else if axis_overflow > px(0.0) {
                self.scroll_to_focused_project(Some(&project_id), true, cx);
            } else {
                // Layout hasn't updated yet — re-queue for next frame
                self.pending_center_scroll = Some(project_id);
                cx.notify();
            }
        }

        // Sync project columns to handle newly added projects
        self.sync_project_columns(cx);

        let visible_projects: Vec<_> = {
            let workspace = self.workspace.read(cx);
            let fm = self.focus_manager.read(cx);
            // When zoomed, show only the zoomed project's column
            if let Some(pid) = fm.fullscreen_project_id() {
                vec![pid.to_string()]
            } else {
                workspace
                    .visible_projects(
                        self.window_id,
                        fm.focused_project_id(),
                        fm.is_focus_individual(),
                    )
                    .iter()
                    .map(|p| p.id.clone())
                    .collect()
            }
        };

        let num_projects = visible_projects.len();

        // Evict stale pane map entries for projects no longer rendered
        // (e.g. worktree columns hidden in overview mode)
        {
            let visible_ids: std::collections::HashSet<&str> =
                visible_projects.iter().map(|s| s.as_str()).collect();
            prune_pane_map(self.window_id, &visible_ids);
        }

        // Empty state when folder filter yields no results
        if num_projects == 0 {
            let has_folder_filter = self
                .workspace
                .read(cx)
                .active_folder_filter(self.window_id)
                .is_some();
            if has_folder_filter {
                let t = theme(cx);
                let workspace = self.workspace.clone();
                let window_id = self.window_id;
                return div()
                    .id("projects-grid-empty")
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(ui_space_md(cx))
                    .child(
                        div()
                            .text_size(ui_text_xl(cx))
                            .text_color(rgb(t.text_muted))
                            .child(i18n!(cx, "window.no_projects_in_folder")),
                    )
                    .child(
                        div()
                            .id("clear-folder-filter")
                            .text_size(ui_text_md(cx))
                            .text_color(rgb(t.border_active))
                            .cursor_pointer()
                            .hover(|s| s.underline())
                            .child(i18n!(cx, "window.show_all_projects"))
                            .on_click(move |_, _window, cx| {
                                workspace.update(cx, |ws, cx| {
                                    ws.set_folder_filter(window_id, None, cx);
                                });
                            }),
                    )
                    .into_any_element();
            }
            // Empty state when every project is hidden in this window
            // (e.g. fresh extra window spawned via NewWindow). Per slice 05
            // criterion 4: a placeholder is rendered when hidden_project_ids
            // covers every project in the workspace.
            if !self.workspace.read(cx).projects().is_empty() {
                let t = theme(cx);
                return div()
                    .id("projects-grid-empty")
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(ui_space_md(cx))
                    .child(
                        div()
                            .text_size(ui_text_xl(cx))
                            .text_color(rgb(t.text_muted))
                            .child(i18n!(cx, "window.no_projects_in_window")),
                    )
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .text_color(rgb(t.text_muted))
                            .child(i18n!(cx, "window.click_sidebar_hint")),
                    )
                    .into_any_element();
            }
        }

        // Get widths for each project
        let settings = settings_entity(cx).read(cx).settings.clone();

        // Per-window orientation: columns (side by side) vs rows (stacked).
        let is_rows = self
            .workspace
            .read(cx)
            .project_layout_mode(self.window_id)
            .is_rows();

        let widths: Vec<f32> = if num_projects <= 1 {
            vec![100.0; num_projects]
        } else {
            let workspace = self.workspace.read(cx);
            let raw_widths: Vec<f32> = visible_projects
                .iter()
                .map(|id| workspace.get_project_width(self.window_id, id, num_projects))
                .collect();
            Self::normalize_widths(&raw_widths)
        };

        // Persistent bounds reference for resize calculation (survives across renders)
        let container_bounds = self.projects_grid_bounds.clone();

        // Compute pixel sizes from percentages, accounting for divider thickness.
        // The relevant axis is width for columns, height for rows.
        let measured_container_size = {
            let b = container_bounds.borrow();
            f32::from(if is_rows { b.size.height } else { b.size.width })
        };
        let dynamic_container_size = {
            let window_bounds = window.window_bounds().get_bounds();
            let card_gap = f32::from(ui_space_card_gap(cx));
            let win_pad = f32::from(ui_space_window_padding(cx));
            let total_axis = f32::from(if is_rows {
                window_bounds.size.height - px(62.0)
            } else {
                window_bounds.size.width
            });
            let left_sb = if self.left_dock_ctrl.should_render() {
                self.left_dock_ctrl.current_width() + self.left_dock_ctrl.animation() * card_gap
            } else {
                0.0
            };
            let right_sb = if self.right_dock_ctrl.should_render() {
                self.right_dock_ctrl.current_width() + self.right_dock_ctrl.animation() * card_gap
            } else {
                0.0
            };
            let right_tb = if settings_entity(cx).read(cx).settings.right_toolbar_open {
                f32::from(ui_right_toolbar_width(cx)) + card_gap
            } else {
                0.0
            };
            let padding_overhead = 2.0 * win_pad;
            (total_axis - left_sb - right_sb - right_tb - padding_overhead).max(200.0)
        };

        let is_any_dock_animating =
            self.left_dock_ctrl.is_animating() || self.right_dock_ctrl.is_animating();

        let container_size = if is_any_dock_animating {
            dynamic_container_size
        } else if measured_container_size > 0.0 {
            measured_container_size
        } else {
            dynamic_container_size
        };

        let pixel_widths =
            Self::to_pixel_widths(&widths, container_size, settings.min_column_width);

        // Project currently hovered in the Switch Project overlay (any window).
        // Its panel gets an accent ring here so a hover also reveals where the
        // project lives — across every window it is open in.
        let hovered_project = crate::views::overlays::project_hover::hovered_project(cx);
        let ring_color = theme(cx).border_active;

        let is_custom_titlebar = if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
            self.initial_titlebar_style == velowork_workspace::settings::TitlebarStyle::Custom
        } else {
            matches!(
                window.window_decorations(),
                gpui::Decorations::Client { .. }
            )
        };
        let window_corner_radius = settings.window_corner_radius;
        let is_maximized = window.is_maximized();
        let is_fullscreen = window.is_fullscreen();
        let has_rounded_corners =
            is_custom_titlebar && !is_maximized && !is_fullscreen && window_corner_radius > 0.0;
        let corner_radius = px(window_corner_radius);
        // Build interleaved columns and dividers
        let mut elements: Vec<AnyElement> = Vec::new();
        let is_single = num_projects == 1;

        for (i, project_id) in visible_projects.iter().enumerate() {
            let pixel_size = pixel_widths.get(i).copied().unwrap_or(200.0);
            let is_last = i == num_projects - 1;

            if let Some(col) = self.project_columns.get(project_id).cloned() {
                let is_hovered = hovered_project.as_deref() == Some(project_id.as_str());
                let terminal_fullscreen = self.focus_manager.read(cx).has_fullscreen();
                let palette = SemanticPalette::from_theme(&t);
                let col_element = div()
                    .overflow_hidden()
                    .when(!terminal_fullscreen, |d| d.rounded(RADIUS_CARD))
                    .when(terminal_fullscreen && has_rounded_corners, |d| {
                        d.rounded_bl(corner_radius).rounded_br(corner_radius)
                    })
                    .when(is_single, |d| d.flex_1().size_full())
                    .when(!is_single && is_rows, |d| {
                        if is_last {
                            d.flex_1().w_full().min_h(px(pixel_size))
                        } else {
                            d.h(px(pixel_size)).w_full().flex_shrink_0()
                        }
                    })
                    .when(!is_single && !is_rows, |d| {
                        if is_last {
                            d.flex_1().h_full().min_w(px(pixel_size))
                        } else {
                            d.w(px(pixel_size)).h_full().flex_shrink_0()
                        }
                    })
                    .relative()
                    .child(AnyView::from(col).cached(StyleRefinement::default().size_full()))
                    // Subtle panel border drawn as a non-occluding overlay so it is never
                    // obscured or blurred by child elements' antialiased solid backgrounds.
                    .when(!terminal_fullscreen, |d| {
                        d.child(
                            div()
                                .absolute()
                                .inset_0()
                                .rounded(RADIUS_CARD)
                                .border_1()
                                .border_color(palette.border_subtle),
                        )
                    })
                    // Accent ring drawn as a non-occluding overlay so it adds no
                    // layout shift and does not intercept clicks on the panel.
                    .when(is_hovered, |d| {
                        d.child(
                            div()
                                .absolute()
                                .inset_0()
                                .border_2()
                                .border_color(rgb(ring_color))
                                .rounded(RADIUS_CARD),
                        )
                    })
                    .into_any_element();

                elements.push(col_element);

                // Add divider after each project except the last
                if i < num_projects - 1 {
                    let min_col_width = settings_entity(cx).read(cx).settings.min_column_width;
                    let divider = render_project_divider(
                        self.window_id,
                        self.workspace.clone(),
                        i,
                        visible_projects.clone(),
                        container_bounds.clone(),
                        &self.active_drag,
                        min_col_width,
                        is_rows,
                        cx,
                    );
                    elements.push(divider.into_any_element());
                }
            }
        }

        let t = theme(cx);
        let scroll_handle = self.projects_scroll_handle.clone();
        let scrollbar_color = rgb(t.text_muted);
        let terminal_fullscreen = self.focus_manager.read(cx).has_fullscreen();

        let scroll_handle_for_wheel = self.projects_scroll_handle.clone();

        div()
            .id("projects-grid-wrapper")
            .flex_1()
            .h_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            // Clip overflow along the grid axis; the scrollbar drives it.
            .when(is_rows, |d| d.overflow_y_hidden())
            .when(!is_rows, |d| d.overflow_x_hidden())
            .relative()
            // No separate center background layer here on purpose: the terminal
            // area's own shared background (`render_terminal_shared_background`,
            // painted once by `ProjectColumn`) already paints `term_background`
            // + the optional image as a single translucent surface. Adding a
            // second `bg_primary` + image layer underneath would show THROUGH the
            // semi-transparent terminal
            // background and blend `term_background` with `bg_primary`, breaking
            // the global opacity (the area would no longer fade cleanly to the
            // desktop). The terminal background composites directly over the
            // window root, so any resize/dock-collapse gap briefly shows the same
            // backdrop the terminal area already reveals — no extra flash.
            // Scroll the project grid along its axis. Columns scroll
            // horizontally (shift+wheel or native horizontal wheel); rows
            // scroll vertically with the natural wheel.
            .on_scroll_wheel(
                cx.listener(move |_this, event: &ScrollWheelEvent, _window, cx| {
                    let delta = event.delta.pixel_delta(px(17.0));
                    let max_offset = scroll_handle_for_wheel.max_offset();
                    let current = scroll_handle_for_wheel.offset();
                    if is_rows {
                        let amount = if !delta.y.is_zero() { delta.y } else { delta.x };
                        if amount.is_zero() || max_offset.y <= px(2.0) {
                            return;
                        }
                        let new_y = (current.y + amount).clamp(-max_offset.y, px(0.0));
                        scroll_handle_for_wheel.set_offset(point(current.x, new_y));
                    } else {
                        let amount = if event.modifiers.shift {
                            if !delta.x.is_zero() { delta.x } else { delta.y }
                        } else if !delta.x.is_zero() {
                            delta.x
                        } else {
                            return;
                        };
                        if max_offset.x <= px(2.0) {
                            return;
                        }
                        let new_x = (current.x + amount).clamp(-max_offset.x, px(0.0));
                        scroll_handle_for_wheel.set_offset(point(new_x, current.y));
                    }
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("projects-grid")
                    .size_full()
                    .flex()
                    .overflow_hidden()
                    .when(terminal_fullscreen && has_rounded_corners, |d| {
                        d.rounded_bl(corner_radius).rounded_br(corner_radius)
                    })
                    .when(is_rows, |d| d.flex_col().overflow_y_hidden())
                    .when(!is_rows, |d| d.overflow_x_hidden())
                    .track_scroll(&self.projects_scroll_handle)
                    // Canvas to capture container bounds (updates persistent bounds for next render)
                    .child(
                        canvas(
                            {
                                let container_bounds = container_bounds.clone();
                                move |bounds, _window, _cx| {
                                    *container_bounds.borrow_mut() = bounds;
                                }
                            },
                            |_bounds, _prepaint, _window, _cx| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    // Mouse handlers are on root div - no need to duplicate here
                    .children(elements),
            )
            // Scrollbar overlay: along the bottom for columns, along the right
            // edge for rows. Drag state (`hscroll_*`) is axis-agnostic since
            // only one orientation is active at a time.
            .child({
                let hscroll_bounds = self.hscroll_bounds.clone();
                div()
                    .id("grid-scrollbar")
                    .absolute()
                    .when(is_rows, |d| d.top_0().bottom_0().right_0().w(px(6.0)))
                    .when(!is_rows, |d| d.bottom_0().left_0().right_0().h(px(6.0)))
                    .cursor(CursorStyle::Arrow)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                            let max_offset = this.projects_scroll_handle.max_offset();
                            let max = if is_rows { max_offset.y } else { max_offset.x };
                            if max <= px(2.0) {
                                return;
                            }
                            this.hscroll_dragging = true;
                            // Jump to clicked position
                            if let Some(bounds) = *this.hscroll_bounds.borrow() {
                                let (track, origin, pos) = if is_rows {
                                    (
                                        f32::from(bounds.size.height),
                                        f32::from(bounds.origin.y),
                                        f32::from(event.position.y),
                                    )
                                } else {
                                    (
                                        f32::from(bounds.size.width),
                                        f32::from(bounds.origin.x),
                                        f32::from(event.position.x),
                                    )
                                };
                                let ratio = ((pos - origin) / track).clamp(0.0, 1.0);
                                let new = -ratio * f32::from(max);
                                let off = if is_rows {
                                    point(px(0.0), px(new))
                                } else {
                                    point(px(new), px(0.0))
                                };
                                this.projects_scroll_handle.set_offset(off);
                            }
                            cx.notify();
                        }),
                    )
                    .on_mouse_move(
                        cx.listener(move |this, event: &MouseMoveEvent, _window, cx| {
                            if !this.hscroll_dragging {
                                return;
                            }
                            let max_offset = this.projects_scroll_handle.max_offset();
                            let max = if is_rows { max_offset.y } else { max_offset.x };
                            if max <= px(2.0) {
                                return;
                            }
                            if let Some(bounds) = *this.hscroll_bounds.borrow() {
                                let (track, origin, pos) = if is_rows {
                                    (
                                        f32::from(bounds.size.height),
                                        f32::from(bounds.origin.y),
                                        f32::from(event.position.y),
                                    )
                                } else {
                                    (
                                        f32::from(bounds.size.width),
                                        f32::from(bounds.origin.x),
                                        f32::from(event.position.x),
                                    )
                                };
                                let ratio = ((pos - origin) / track).clamp(0.0, 1.0);
                                let new = -ratio * f32::from(max);
                                let off = if is_rows {
                                    point(px(0.0), px(new))
                                } else {
                                    point(px(new), px(0.0))
                                };
                                this.projects_scroll_handle.set_offset(off);
                            }
                            cx.notify();
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                            if this.hscroll_dragging {
                                this.hscroll_dragging = false;
                                cx.notify();
                            }
                        }),
                    )
                    .child(
                        canvas(
                            {
                                let hscroll_bounds = hscroll_bounds.clone();
                                move |bounds, _window, _cx| {
                                    *hscroll_bounds.borrow_mut() = Some(bounds);
                                }
                            },
                            move |bounds, _, window, _cx| {
                                let max_scroll = scroll_handle.max_offset();
                                let max = if is_rows { max_scroll.y } else { max_scroll.x };
                                if max <= px(2.0) {
                                    return;
                                }
                                let offset = scroll_handle.offset();
                                let off = if is_rows { offset.y } else { offset.x };
                                let track = if is_rows {
                                    f32::from(bounds.size.height)
                                } else {
                                    f32::from(bounds.size.width)
                                };
                                let content = track + f32::from(max);
                                let thumb = (track / content * track).max(30.0);
                                let scroll_ratio = f32::from(-off) / f32::from(max);
                                let thumb_pos = scroll_ratio * (track - thumb);

                                let thumb_bounds = if is_rows {
                                    Bounds {
                                        origin: point(
                                            bounds.origin.x + px(1.0),
                                            bounds.origin.y + px(thumb_pos),
                                        ),
                                        size: size(px(4.0), px(thumb)),
                                    }
                                } else {
                                    Bounds {
                                        origin: point(
                                            bounds.origin.x + px(thumb_pos),
                                            bounds.origin.y + px(1.0),
                                        ),
                                        size: size(px(thumb), px(4.0)),
                                    }
                                };
                                window.paint_quad(
                                    fill(thumb_bounds, scrollbar_color).corner_radii(px(2.0)),
                                );
                            },
                        )
                        .size_full(),
                    )
            })
            .into_any_element()
    }
}

impl Render for WindowView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);

        // Deferred activation of the AI assistant panel with an injected terminal quote.
        // (Panel activation needs a Window, which is only available here in render.)
        if let Some(quote) = self.pending_ai_interpret.take() {
            // 右侧 dock 为按需创建（IDEA Tool Window 模式）：若用户从未展开过 AI
            // 面板，self.right_dock 为 None，必须先创建并展开 dock 实体，否则 quote
            // 会被 take 后直接丢弃。复用工具栏点击逻辑创建专属面板并展开。
            if self.right_dock.is_none() {
                self.handle_right_toolbar_click("ai_assistant", window, cx);
            }
            if let Some(dock) = self.right_dock.clone() {
                dock.update(cx, |dp, cx| {
                    dp.activate_or_add_panel("ai_assistant", window, cx);
                });
                if let Some(active) = dock.read(cx).active_tab() {
                    if let Ok(ai) = active
                        .view
                        .clone()
                        .downcast::<crate::views::panels::ai_assistant_panel::AiAssistantPanel>(
                    ) {
                        ai.update(cx, |ai, cx| ai.set_quote(quote, cx));
                    }
                }
            }
        }

        if self.pending_ai_open {
            self.pending_ai_open = false;
            if self.right_dock.is_none() {
                self.handle_right_toolbar_click("ai_assistant", window, cx);
            }
            if let Some(dock) = self.right_dock.clone() {
                dock.update(cx, |dp, cx| {
                    dp.activate_or_add_panel("ai_assistant", window, cx);
                });
                if let Some(active) = dock.read(cx).active_tab() {
                    if let Ok(ai) = active
                        .view
                        .clone()
                        .downcast::<crate::views::panels::ai_assistant_panel::AiAssistantPanel>(
                    ) {
                        ai.update(cx, |ai, cx| {
                            ai.focus_input(window, cx);
                        });
                    }
                }
            }
        }

        let is_custom_titlebar = if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
            self.initial_titlebar_style == velowork_workspace::settings::TitlebarStyle::Custom
        } else {
            matches!(
                window.window_decorations(),
                gpui::Decorations::Client { .. }
            )
        };

        // Get overlay visibility state from overlay manager
        let om = self.overlay_manager.read(cx);
        let has_context_menu = om.has_context_menu();
        let has_folder_context_menu = om.has_folder_context_menu();
        let has_terminal_context_menu = om.has_terminal_context_menu();
        let has_tab_context_menu = om.has_tab_context_menu();
        let has_quick_command_context_menu = om.has_quick_command_context_menu();
        let has_tunnel_context_menu = om.has_tunnel_context_menu();
        let has_service_context_menu = om.has_service_context_menu();
        let has_transfer_popup = om.has_transfer_popup();
        let has_terminal_ai_inline = om.has_terminal_ai_inline();

        // Get active drag for global mouse handling
        let active_drag = self.active_drag.clone();
        let workspace = self.workspace.clone();

        // Capture sidebar state for mouse move handler
        let sidebar_auto_hide = self.left_dock_ctrl.is_auto_hide();
        let sidebar_hover_shown = self.left_dock_ctrl.is_hover_shown();
        let current_sidebar_width = self.left_dock_ctrl.current_width();

        // Clone overlay_manager for action handlers
        let overlay_manager = self.overlay_manager.clone();

        let focus_handle = self.focus_handle.clone();

        // 渲染期物理焦点自愈状态机（事件驱动 + 状态转移 + Focus Watchdog 三重防线）：
        let has_modal = om.has_modal() || self.sidebar.read(cx).active_dialog.is_some();
        let modal_just_closed = self.last_had_modal && !has_modal;
        self.last_had_modal = has_modal;

        if self.needs_focus_restore || modal_just_closed {
            // 场景 C / D / E（弹窗关闭）：专用发起源精准原路归还 + 三级降级兜底链（支持嵌套父级弹窗）
            self.needs_focus_restore = false;
            self.restore_modal_closed_focus(window, cx);
        } else if has_modal {
            // 场景 B（弹窗打开中）：完全静默，绝不抢焦，保证弹窗首项 initial_focus_done 顺畅获焦
            self.needs_focus_restore = false;
        } else if !self.initial_focus_done {
            // 场景 A（冷启动首帧）：执行欢迎界面输入框 / 终端初次获焦
            self.initial_focus_done = true;
            self.focus_active_terminal(window, cx);
        } else if window.focused(cx).is_none() {
            // 场景 F（Focus Watchdog 意外失焦防护）：按意图层级联恢复，绝不盲目跨层劫持
            self.restore_focus_cascade(window, cx);
        }

        let window_corner_radius = settings_entity(cx).read(cx).settings.window_corner_radius;
        let is_maximized = window.is_maximized();
        let is_fullscreen = window.is_fullscreen();
        let has_rounded_corners =
            is_custom_titlebar && !is_maximized && !is_fullscreen && window_corner_radius > 0.0;
        let corner_radius = px(window_corner_radius);
        // Publish the active window-corner radius so `modal_backdrop` can clip
        // its full-window dimming mask to the rounded window corners (GPUI's
        // `overflow_hidden` never clips the rounded shape, so the mask must
        // round its own background). Only re-set the global when it changes.
        let modal_corner_radius = if has_rounded_corners {
            window_corner_radius
        } else {
            0.0
        };
        if self.last_modal_corner_radius != modal_corner_radius {
            cx.set_global(velowork_ui::WindowCornerRadius(modal_corner_radius));
            self.last_modal_corner_radius = modal_corner_radius;
        }
        // Minimum animated sidebar width required to actually cover the window's
        // bottom corner arc. While a sidebar strip is narrower than the corner
        // radius, its own `rounded_bl/br` quad gets clamped by GPUI
        // (`clamp_radii_for_quad_size`) into a near-square corner that paints
        // over the frame's rounded arc — showing a square tip during the
        // open/close animation. Below this width the strip must not paint its
        // background and the status bar must keep its own rounded corner.
        let _corner_cover_px = if has_rounded_corners {
            window_corner_radius
        } else {
            0.0
        };

        div()
            .id("root")
            .size_full()
            .flex()
            .flex_col()
            .text_size(ui_text_md(cx))
            .text_color(rgb(t.text_primary))
            .when(has_rounded_corners, |d| {
                d.rounded(corner_radius).overflow_hidden()
            })
            .bg(surface_bg(t.bg_primary, cx))
            .track_focus(&focus_handle)
            // Global mouse move handler for resize and auto-hide
            .on_mouse_move(cx.listener({
                let active_drag = active_drag.clone();
                let workspace = workspace.clone();
                move |this, event: &MouseMoveEvent, window, cx| {
                    // Handle resize drag
                    if let Some(ref state) = *active_drag.borrow() {
                        match state {
                            DragState::Sidebar => {
                                // Handle sidebar resize
                                let mouse_x = f32::from(event.position.x);
                                let min_size = this.left_dock.read(cx).required_min_size(cx);
                                let collapse_threshold = (min_size - 25.0).max(60.0);

                                if mouse_x < collapse_threshold {
                                    // User dragged past minimal width threshold: immediately collapse/hide (like VSCode)
                                    *active_drag.borrow_mut() = None;
                                    this.hide_left_dock(cx);
                                    this.focus_active_terminal(window, cx);
                                } else {
                                    let bounds = window.window_bounds().get_bounds();
                                    let win_w = f32::from(bounds.size.width);
                                    let window_limit = (win_w - 220.0).max(0.0);
                                    let ctx = velowork_workspace::dock_controller::DockResizeContext {
                                        content_min: Some(min_size),
                                        window_limit: Some(window_limit),
                                    };
                                    this.left_dock_ctrl.resize(
                                        mouse_x,
                                        &velowork_workspace::dock_controller::SIDEBAR_CONSTRAINTS,
                                        &ctx,
                                    );
                                    let width = this.left_dock_ctrl.width();
                                    this.left_dock.update(cx, |panel, cx| {
                                        panel.size = width;
                                        cx.notify();
                                    });
                                }
                                cx.notify();
                            }
                            DragState::RightSidebar => {
                                // Handle right sidebar resize
                                let bounds = window.window_bounds().get_bounds();
                                let win_w = f32::from(bounds.size.width);
                                let mouse_from_right = win_w - f32::from(event.position.x);
                                let min_size = this
                                    .right_dock
                                    .as_ref()
                                    .map(|d| d.read(cx).required_min_size(cx))
                                    .unwrap_or(velowork_workspace::dock_controller::SIDEBAR_CONSTRAINTS.min);
                                let collapse_threshold = (min_size - 25.0).max(60.0);

                                if mouse_from_right < collapse_threshold {
                                    // Immediately collapse right dock
                                    *active_drag.borrow_mut() = None;
                                    this.hide_right_dock(cx);
                                    this.focus_active_terminal(window, cx);
                                } else {
                                    let window_limit = (win_w - 220.0).max(0.0);
                                    let ctx = velowork_workspace::dock_controller::DockResizeContext {
                                        content_min: Some(min_size),
                                        window_limit: Some(window_limit),
                                    };
                                    this.right_dock_ctrl.resize(
                                        mouse_from_right,
                                        &velowork_workspace::dock_controller::SIDEBAR_CONSTRAINTS,
                                        &ctx,
                                    );
                                    let width = this.right_dock_ctrl.width();
                                    if let Some(dock) = this.right_dock.as_ref() {
                                        dock.update(cx, |panel, cx| {
                                            panel.size = width;
                                            cx.notify();
                                        });
                                    }
                                }
                                cx.notify();
                            }
                            _ => {
                                // Handle split and project column resize
                                compute_resize(
                                    this.window_id,
                                    event.position,
                                    state,
                                    &workspace,
                                    cx,
                                );
                                // Bypass all .cached() views so terminal elements
                                // repaint with new bounds during drag.
                                window.refresh();
                            }
                        }
                    }

                    // Handle auto-hide: check if mouse left the sidebar area
                    if sidebar_auto_hide && sidebar_hover_shown {
                        // Add small margin for smoother interaction
                        let hide_threshold = current_sidebar_width + 10.0;
                        if f32::from(event.position.x) > hide_threshold {
                            this.hide_sidebar_on_leave(cx);
                        }
                    }
                }
            }))
            // Global mouse up handler to end resize (registered via window event
            // to reliably fire regardless of which child element the cursor is over)
            .child(
                canvas(|_bounds, _window, _cx| {}, {
                    let active_drag = active_drag.clone();
                    let terminals = self.terminals.clone();
                    let workspace = workspace.clone();
                    let overlay_manager = self.overlay_manager.clone();
                    let this_weak = cx.entity().downgrade();
                    move |_bounds, _prepaint, window, _cx| {
                        let active_drag = active_drag.clone();
                        let terminals = terminals.clone();
                        let workspace = workspace.clone();
                        let this_weak = this_weak.clone();

                        // Route every left `MouseDown` (bubble phase) to the
                        // overlay registry so any `ClickOutside` overlay whose
                        // bounds don't contain the click is dismissed. Dock /
                        // panel code never participates. Re-registered every
                        // frame because `on_mouse_event` only lasts one frame.
                        let overlay_manager = overlay_manager.clone();
                        let this_weak_for_focus = this_weak.clone();
                        window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
                            if phase != DispatchPhase::Capture {
                                return;
                            }
                            overlay_manager.update(cx, |om, cx| {
                                if e.button == MouseButton::Left {
                                    om.record_click_origin(e.position);
                                }
                                om.handle_overlay_mouse_down(e.position, window, cx);
                            });
                            // 全局物理焦点失焦自愈兜底：若任何非预期路径导致焦点悬空，在交互发生时无感对齐
                            if window.focused(cx).is_none() {
                                if let Some(this) = this_weak_for_focus.upgrade() {
                                    this.update(cx, |this, cx| {
                                        this.focus_active_terminal(window, cx);
                                    });
                                }
                            }
                        });

                        window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
                            if phase == DispatchPhase::Bubble && e.button == MouseButton::Left {
                                let was_split_drag =
                                    matches!(*active_drag.borrow(), Some(DragState::Split { .. }));
                                let drag_state = active_drag.borrow().clone();
                                *active_drag.borrow_mut() = None;

                                match drag_state {
                                    Some(DragState::Sidebar) => {
                                        let mouse_x = f32::from(e.position.x);
                                        let min_size = this_weak
                                            .upgrade()
                                            .map(|t| t.read(cx).left_dock.read(cx).required_min_size(cx))
                                            .unwrap_or(140.0);
                                        let collapse_threshold = (min_size - 25.0).max(60.0);
                                        if mouse_x < collapse_threshold {
                                            if let Some(this) = this_weak.upgrade() {
                                                this.update(cx, |this, cx| {
                                                    this.hide_left_dock(cx);
                                                    this.focus_active_terminal(window, cx);
                                                });
                                            }
                                        }
                                    }
                                    Some(DragState::RightSidebar) => {
                                        let bounds = window.window_bounds().get_bounds();
                                        let win_w = f32::from(bounds.size.width);
                                        let mouse_from_right = win_w - f32::from(e.position.x);
                                        let min_size = this_weak
                                            .upgrade()
                                            .and_then(|t| {
                                                t.read(cx)
                                                    .right_dock
                                                    .as_ref()
                                                    .map(|d| d.read(cx).required_min_size(cx))
                                            })
                                            .unwrap_or(140.0);
                                        let collapse_threshold = (min_size - 25.0).max(60.0);
                                        if mouse_from_right < collapse_threshold {
                                            if let Some(this) = this_weak.upgrade() {
                                                this.update(cx, |this, cx| {
                                                    this.hide_right_dock(cx);
                                                    this.focus_active_terminal(window, cx);
                                                });
                                            }
                                        }
                                    }
                                    _ => {}
                                }

                                if drag_state.is_some() {
                                    let terminals_guard = terminals.lock();
                                    for terminal in terminals_guard.values() {
                                        terminal.flush_pending_resize();
                                    }
                                }

                                // Persist final split sizes (drag used ui_only notify)
                                if was_split_drag {
                                    workspace.update(cx, |ws, cx| {
                                        ws.notify_data(cx);
                                    });
                                }
                            }
                        });
                    }
                })
                .absolute()
                .size_full(),
            )
            // Handle left dock toggle action
            .on_action(cx.listener(|this, _: &ToggleLeftDock, window, cx| {
                this.toggle_left_dock_with_window(window, cx);
            }))
            // Handle right dock toggle action
            .on_action(cx.listener(|this, _: &ToggleRightDock, window, cx| {
                this.toggle_right_dock_with_window(window, cx);
            }))
            // Handle right toolbar toggle action
            .on_action(cx.listener(|this, _: &ToggleRightToolbar, _window, cx| {
                this.toggle_right_toolbar(cx);
            }))
            // Handle toggle SFTP panel action
            .on_action(cx.listener(|this, _: &ToggleSftpPanel, window, cx| {
                this.toggle_active_sftp_with_window(window, cx);
            }))
            // Handle toggle commands/hook panel action
            .on_action(cx.listener(|this, _: &ToggleCommandsPanel, window, cx| {
                this.toggle_active_commands_with_window(window, cx);
            }))
            // 面板开关 Action（Velowork 语义）：聚焦面板自动带出所属 dock
            .on_action(cx.listener(|this, _: &ShowTunnelsPanel, window, cx| {
                this.handle_right_toolbar_click("tunnels", window, cx);
            }))
            .on_action(cx.listener(|this, _: &ShowServicesPanel, window, cx| {
                this.handle_right_toolbar_click("services", window, cx);
            }))
            .on_action(cx.listener(|this, _: &ShowQuickCommandsPanel, window, cx| {
                this.handle_right_toolbar_click("quick_commands", window, cx);
            }))
            .on_action(cx.listener(|this, _: &ShowHistoryPanel, window, cx| {
                this.handle_right_toolbar_click("command_history", window, cx);
            }))
            .on_action(cx.listener(|this, _: &ShowAiAssistant, window, cx| {
                this.handle_right_toolbar_click("ai_assistant", window, cx);
            }))
            .on_action(cx.listener(|this, _: &TerminalInlineAi, window, cx| {
                this.handle_terminal_inline_ai(window, cx);
            }))
            // Handle toggle left dock auto-hide action
            .on_action(cx.listener(|this, _: &ToggleLeftDockAutoHide, _window, cx| {
                this.toggle_sidebar_auto_hide(cx);
            }))
            // Handle equalize layout action
            .on_action(cx.listener(|this, _: &EqualizeLayout, _window, cx| {
                let fm = this.focus_manager.read(cx).clone();
                this.workspace.update(cx, |ws, cx| {
                    // Equalize pane sizes in the focused terminal's parent split
                    ws.equalize_focused_split(&fm, cx);
                });
            }))
            // Spawn a new extra window onto the workspace. The data-layer
            // mutation pushes a fresh `WindowState` and bumps `data_version`
            // so the auto-save observer fires; the OS window itself opens
            // when the `Velowork` observer in `src/app/extras.rs` sees the new
            // `extra_windows` entry.
            .on_action(cx.listener(|this, _: &NewWindow, window, cx| {
                let bounds = window.window_bounds().get_bounds();
                let spawning_bounds = crate::workspace::state::WindowBounds {
                    origin_x: f32::from(bounds.origin.x),
                    origin_y: f32::from(bounds.origin.y),
                    width: f32::from(bounds.size.width),
                    height: f32::from(bounds.size.height),
                };
                this.workspace.update(cx, |ws, cx| {
                    ws.spawn_extra_window(Some(spawning_bounds), cx);
                });
            }))
            .on_action(
                cx.listener(|this, _: &CloseWindow, window, cx| {
                    this.request_close_window(window, cx);
                }),
            )
            .on_action(
                cx.listener(|this, _: &Quit, window, cx| {
                    this.request_quit(window, cx);
                }),
            )
            // Dock focus navigation (Keyboard-First idea, now wired through GPUI)
            .on_action(cx.listener(|this, _: &FocusLeftDock, window, cx| {
                this.focus_dock_at(0, window, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusCenterDock, window, cx| {
                this.focus_dock_at(1, window, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusBottomDock, window, cx| {
                this.focus_dock_at(2, window, cx);
            }))
            .on_action(cx.listener(|this, _: &FocusRightDock, window, cx| {
                this.focus_dock_at(3, window, cx);
            }))
            .on_action(cx.listener(|this, _: &CyclePanelNext, window, cx| {
                this.cycle_dock_index = (this.cycle_dock_index + 1) % 4;
                this.focus_dock_at(this.cycle_dock_index, window, cx);
            }))
            .on_action(cx.listener(|this, _: &CyclePanelPrev, window, cx| {
                this.cycle_dock_index = (this.cycle_dock_index + 3) % 4;
                this.focus_dock_at(this.cycle_dock_index, window, cx);
            }))
            // Handle show keybindings action
            .on_action(cx.listener({
                let overlay_manager = overlay_manager.clone();
                move |_this, _: &ShowKeybindings, _window, cx| {
                    overlay_manager.update(cx, |om, cx| om.toggle_keybindings_help(cx));
                }
            }))
            // Handle show theme selector action
            .on_action(cx.listener({
                let overlay_manager = overlay_manager.clone();
                move |_this, _: &ShowThemeSelector, window, cx| {
                    overlay_manager.update(cx, |om, cx| om.toggle_theme_selector_with_window(Some(window), cx));
                }
            }))
            // Handle show command palette action
            .on_action(cx.listener({
                let overlay_manager = overlay_manager.clone();
                move |_this, _: &ShowCommandPalette, _window, cx| {
                    overlay_manager.update(cx, |om, cx| om.toggle_command_palette(cx));
                }
            }))
            // Handle show settings panel action
            .on_action(cx.listener({
                let overlay_manager = overlay_manager.clone();
                move |_this, _: &ShowSettings, _window, cx| {
                    overlay_manager.update(cx, |om, cx| om.toggle_settings_panel(cx));
                }
            }))
            // Handle show AI settings action
            .on_action(cx.listener({
                let overlay_manager = overlay_manager.clone();
                move |_this, _: &ShowAiSettings, _window, cx| {
                    overlay_manager.update(cx, |om, cx| om.open_settings_panel_to(Some(crate::views::overlays::settings::settings_panel::SettingsCategory::AiAssistant), cx));
                }
            }))
            // Handle show update dialog action
            .on_action(cx.listener({
                let overlay_manager = overlay_manager.clone();
                move |_this, _: &ShowUpdateDialog, window, cx| {
                    overlay_manager.update(cx, |om, cx| om.toggle_update_dialog_with_window(Some(window), cx));
                }
            }))
            // Handle show help dialog action
            .on_action(cx.listener({
                let overlay_manager = overlay_manager.clone();
                move |_this, _: &ShowHelp, window, cx| {
                    overlay_manager.update(cx, |om, cx| om.toggle_help_dialog_with_window(Some(window), cx));
                }
            }))
            // Handle show about dialog action (About and ShowAboutDialog)
            .on_action(cx.listener({
                let overlay_manager = overlay_manager.clone();
                move |_this, _: &About, window, cx| {
                    overlay_manager.update(cx, |om, cx| om.toggle_about_dialog_with_window(Some(window), cx));
                }
            }))
            .on_action(cx.listener({
                let overlay_manager = overlay_manager.clone();
                move |_this, _: &ShowAboutDialog, window, cx| {
                    overlay_manager.update(cx, |om, cx| om.toggle_about_dialog_with_window(Some(window), cx));
                }
            }))
            // Handle global Cancel (escape) when a modal overlay or terminal inline AI is open
            .on_action(cx.listener({
                let overlay_manager = overlay_manager.clone();
                move |_this, _: &Cancel, _window, cx| {
                    if overlay_manager.read(cx).has_terminal_ai_inline() {
                        cx.stop_propagation();
                        overlay_manager.update(cx, |om, cx| om.dismiss_terminal_ai_inline(cx));
                        return;
                    }
                    if overlay_manager.read(cx).has_modal() {
                        cx.stop_propagation();
                        overlay_manager.update(cx, |om, cx| om.close_modal(cx));
                    }
                }
            }))
            // Handle show log console action
            .on_action(cx.listener({
                let overlay_manager = overlay_manager.clone();
                move |_this, _: &ShowLogConsole, _window, cx| {
                    overlay_manager.update(cx, |om, cx| om.toggle_log_console(cx));
                }
            }))
            // Handle show profile manager action
            .on_action(cx.listener({
                let overlay_manager = overlay_manager.clone();
                move |_this, _: &ShowProfileManager, _window, cx| {
                    overlay_manager.update(cx, |om, cx| om.toggle_profile_manager(cx));
                }
            }))
            // Handle add tab action globally (always creates a new tab)
            .on_action(cx.listener(|this, _: &AddTab, window, cx| {
                this.handle_global_add_tab(window, cx);
            }))
            // Handle new session action
            .on_action(cx.listener(|this, _: &NewSession, window, cx| {
                this.open_add_session_dialog_with_window(window, cx);
            }))
            // Handle new project action
            .on_action(cx.listener({
                let overlay_manager = overlay_manager.clone();
                move |_this, _: &NewProject, _window, cx| {
                    overlay_manager.update(cx, |om, cx| om.toggle_add_project_dialog(cx));
                }
            }))
            // Handle manage projects/sessions action
            .on_action(cx.listener({
                let overlay_manager = overlay_manager.clone();
                move |_this, _: &ShowProjectManageDialog, _window, cx| {
                    overlay_manager.update(cx, |om, cx| om.toggle_manage_projects_dialog(cx));
                }
            }))
            // Handle import sessions action
            .on_action(cx.listener({
                let overlay_manager = overlay_manager.clone();
                move |_this, _: &ShowImportSessionDialog, _window, cx| {
                    overlay_manager.update(cx, |om, cx| om.toggle_import_sessions_dialog(cx));
                }
            }))
            // Handle open settings file action
            .on_action(cx.listener(|_this, _: &OpenSettingsFile, _window, _cx| {
                open_settings_file();
            }))
            // Handle check for updates action
            .on_action(cx.listener(|_this, _: &CheckForUpdates, _window, cx| {
                if let Some(update_info) = cx.try_global::<velowork_updater::GlobalUpdateInfo>()
                {
                    let info = update_info.0.clone();

                    // Prevent concurrent manual checks
                    if !info.try_start_manual() {
                        return;
                    }

                    info.set_status(velowork_updater::UpdateStatus::Checking);
                    let token = info.current_token();
                    cx.notify();
                    cx.spawn(async move |this, cx| {
                        velowork_updater::orchestrator::run_manual_check(
                            info,
                            token,
                            cx,
                            move |cx| {
                                let _ = this.update(cx, |_, cx| cx.notify());
                            },
                        )
                        .await;
                    })
                    .detach();
                }
            }))
            // Handle install update action (dispatched from status bar)
            .on_action(cx.listener(|_this, _: &InstallUpdate, _window, cx| {
                if let Some(update_info) = cx.try_global::<velowork_updater::GlobalUpdateInfo>()
                {
                    let info = update_info.0.clone();
                    if let velowork_updater::UpdateStatus::Ready { version, path } =
                        info.status()
                    {
                        info.set_status(velowork_updater::UpdateStatus::Installing {
                            version: version.clone(),
                        });
                        cx.notify();
                        cx.spawn(async move |this, cx| {
                            velowork_updater::orchestrator::run_install(
                                info,
                                version,
                                path,
                                cx,
                                move |cx| {
                                    let _ = this.update(cx, |_, cx| cx.notify());
                                },
                            )
                            .await;
                        })
                        .detach();
                    }
                }
            }))
            // Handle toggle pane switcher action
            .on_action(cx.listener(|this, _: &TogglePaneSwitcher, _window, cx| {
                if this.pane_switch_active {
                    this.pane_switch_active = false;
                    this.pane_switcher_entity = None;
                } else {
                    this.pane_switch_active = true;
                    let pane_map = get_pane_map(this.window_id);
                    this.show_pane_switcher(pane_map, cx);
                }
                cx.notify();
            }))
            // Title bar at the top (rendered when custom titlebar is enabled)
            .when(
                is_custom_titlebar && (!cfg!(target_os = "macos") || !window.is_fullscreen()),
                |d| d.child(self.title_bar.clone()),
            )
            // Content root: wraps main content area, status bar, and content
            // modals. Its .relative() establishes the positioning context for
            // modal overlays so their .absolute().inset_0() covers only the
            // content area — never the Window Chrome (titlebar).
            .child(
                div()
                    .id("content-root")
                    .relative()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .overflow_hidden()
                    // Main content area
                    .child(
                        // Content below title bar
                        div()
                            .flex_1()
                            .flex()
                            .min_h_0()
                            .min_w_0()
                            .relative()
                    .when(!{
                        let is_left_dock_maximized = {
                            let mode = self.left_dock.read(cx).mode;
                            mode == velowork_ui::dock::PanelMode::Maximized
                                || mode == velowork_ui::dock::PanelMode::Fullscreen
                        };
                        let bottom_fullscreen = self.is_bottom_dock_fullscreen(cx);
                        let right_fullscreen = {
                            let mode = self
                                .right_dock
                                .as_ref()
                                .map(|d| d.read(cx).mode)
                                .unwrap_or(velowork_ui::dock::PanelMode::Normal);
                            mode == velowork_ui::dock::PanelMode::Maximized
                                || mode == velowork_ui::dock::PanelMode::Fullscreen
                        };
                        let terminal_fullscreen = self.focus_manager().read(cx).has_fullscreen();
                        is_left_dock_maximized || right_fullscreen || bottom_fullscreen || terminal_fullscreen
                    }, |d| {
                        let win_pad = ui_space_window_padding(cx);
                        d.px(win_pad)
                            .pt(win_pad)
                            .pb(win_pad)
                    })
                    // Hide left sidebar when right sidebar, bottom dock, or a
                    // terminal is fullscreen/maximized, or when left sidebar is closed.
                    .when(
                        !{
                            let mode = self
                                .right_dock
                                .as_ref()
                                .map(|d| d.read(cx).mode)
                                .unwrap_or(velowork_ui::dock::PanelMode::Normal);
                            let right_fullscreen = mode == velowork_ui::dock::PanelMode::Maximized
                                || mode == velowork_ui::dock::PanelMode::Fullscreen;
                            let bottom_fullscreen = self.is_bottom_dock_fullscreen(cx);
                            let terminal_fullscreen =
                                self.focus_manager().read(cx).has_fullscreen();
                            right_fullscreen || bottom_fullscreen || terminal_fullscreen
                        } && (self.left_dock_ctrl.should_render() || {
                            let mode = self.left_dock.read(cx).mode;
                            mode == velowork_ui::dock::PanelMode::Maximized
                                || mode == velowork_ui::dock::PanelMode::Fullscreen
                        }),
                        |d| {
                            let is_left_dock_maximized = {
                                let mode = self.left_dock.read(cx).mode;
                                mode == velowork_ui::dock::PanelMode::Maximized
                                    || mode == velowork_ui::dock::PanelMode::Fullscreen
                            };

                            let sidebar_width = self.left_dock_ctrl.current_width();
                            let configured_width = self.left_dock_ctrl.width();
                            let card_gap = f32::from(ui_space_card_gap(cx));
                            let left_dynamic_gap = self.left_dock_ctrl.animation() * card_gap;
                            let is_animating = self.left_dock_ctrl.is_animating();

                            if is_left_dock_maximized {
                                d.child(
                                    div()
                                        .id("sidebar-container")
                                        .flex_1()
                                        .h_full()
                                        .relative()
                                        .overflow_hidden()
                                        .when(has_rounded_corners, |d| {
                                            d.rounded_bl(corner_radius).rounded_br(corner_radius)
                                        })
                                        .child(self.left_dock.clone()),
                                )
                            } else {
                                d.child(
                                    div()
                                        .id("sidebar-container")
                                        .h_full()
                                        .w(px(sidebar_width))
                                        .mr(px(left_dynamic_gap))
                                        .flex_shrink_0()
                                        .relative()
                                        .when(is_animating, |d| d.overflow_hidden())
                                        .child(
                                            if is_animating {
                                                div()
                                                    .absolute()
                                                    .top_0()
                                                    .bottom_0()
                                                    .right_0()
                                                    .w(px(configured_width))
                                                    .child(self.left_dock.clone())
                                            } else {
                                                div()
                                                    .size_full()
                                                    .child(self.left_dock.clone())
                                            },
                                        ),
                                )
                            }
                        },
                    )
                    // Center column: main area + bottom dock (vertical layout)
                    .when(!{
                        let is_left_dock_maximized = {
                            let mode = self.left_dock.read(cx).mode;
                            mode == velowork_ui::dock::PanelMode::Maximized
                                || mode == velowork_ui::dock::PanelMode::Fullscreen
                        };
                        let right_fullscreen = {
                            let mode = self
                                .right_dock
                                .as_ref()
                                .map(|d| d.read(cx).mode)
                                .unwrap_or(velowork_ui::dock::PanelMode::Normal);
                            mode == velowork_ui::dock::PanelMode::Maximized
                                || mode == velowork_ui::dock::PanelMode::Fullscreen
                        };
                        is_left_dock_maximized || right_fullscreen
                    }, |d| {
                        let bottom_fullscreen = self.is_bottom_dock_fullscreen(cx);
                        let terminal_fullscreen = self.focus_manager().read(cx).has_fullscreen();
                        let show_bottom_dock = self.bottom_dock_ctrl.should_render() || bottom_fullscreen;

                        d.child(
                            div()
                                .id("center-column")
                                .flex_1()
                                .flex()
                                .flex_col()
                                .min_h_0()
                                .min_w_0()
                                .overflow_hidden()
                                // When bottom dock is fullscreen, hide projects grid
                                .when(!bottom_fullscreen, |d| {
                                    d.child(
                                        div()
                                            .id("projects-container")
                                            .flex_1()
                                            .min_h_0()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .child(self.render_projects_grid(window, cx)),
                                    )
                                })
                                // When a terminal is fullscreen, hide the bottom dock so the
                                // zoomed terminal can fill the whole center column.
                                .when(!terminal_fullscreen && show_bottom_dock, |d| {
                                    d.child(self.render_bottom_dock_container(window, cx))
                                })
                        )
                    })
                    // Right Sidebar & Toolbar container (conditional mount)
                    .when(
                        !{
                            let is_left_dock_maximized = {
                                let mode = self.left_dock.read(cx).mode;
                                mode == velowork_ui::dock::PanelMode::Maximized
                                    || mode == velowork_ui::dock::PanelMode::Fullscreen
                            };
                            let bottom_fullscreen = self.is_bottom_dock_fullscreen(cx);
                            let terminal_fullscreen =
                                self.focus_manager().read(cx).has_fullscreen();
                            is_left_dock_maximized || bottom_fullscreen || terminal_fullscreen
                        },
                        |d| {
                            let mode = self
                                .right_dock
                                .as_ref()
                                .map(|d| d.read(cx).mode)
                                .unwrap_or(velowork_ui::dock::PanelMode::Normal);
                            let is_right_dock_maximized = mode
                                == velowork_ui::dock::PanelMode::Maximized
                                || mode == velowork_ui::dock::PanelMode::Fullscreen;
                            let show_right_dock = self.right_dock_ctrl.should_render() || is_right_dock_maximized;
                            let show_right_toolbar = self.is_right_toolbar_open() && !is_right_dock_maximized;

                            if !show_right_dock && !show_right_toolbar {
                                return d;
                            }

                            if is_right_dock_maximized {
                                if let Some(ref dock) = self.right_dock {
                                    dock.update(cx, |p, _| {
                                        if p.max_render_size.is_some() {
                                            p.max_render_size = None;
                                        }
                                    });
                                }
                            }

                            let right_dock_width = self.right_dock_ctrl.current_width();
                            let configured_width = self.right_dock_ctrl.width();

                            let r_container = if let (true, Some(right_dock)) =
                                (show_right_dock, self.right_dock.clone())
                            {
                                if is_right_dock_maximized {
                                    Some(
                                        div()
                                            .id("right-sidebar-container")
                                            .flex_1()
                                            .w_full()
                                            .min_w_0()
                                            .h_full()
                                            .relative()
                                            .child(right_dock)
                                            .into_any_element(),
                                    )
                                } else {
                                    let is_animating = self.right_dock_ctrl.is_animating();
                                    let card_gap = f32::from(ui_space_card_gap(cx));
                                    let right_dynamic_gap = self.right_dock_ctrl.animation() * card_gap;
                                    Some(
                                        div()
                                            .id("right-sidebar-container")
                                            .h_full()
                                            .flex_shrink_0()
                                            .w(px(right_dock_width))
                                            .relative()
                                            .when(is_animating, |d| d.overflow_hidden())
                                            .when(show_right_toolbar, |d| d.mr(px(right_dynamic_gap)))
                                            .when(!show_right_toolbar, |d| d.ml(px(right_dynamic_gap)))
                                            .child(
                                                if is_animating {
                                                    div()
                                                        .absolute()
                                                        .top_0()
                                                        .bottom_0()
                                                        .right_0()
                                                        .w(px(configured_width))
                                                        .child(right_dock)
                                                } else {
                                                    div()
                                                        .size_full()
                                                        .child(right_dock)
                                                },
                                            )
                                            .into_any_element(),
                                    )
                                }
                            } else {
                                None
                            };

                            let right_toolbar = if show_right_toolbar {
                                let visible_panels =
                                    self.right_toolbar_registry.visible_panels(cx);

                                let is_enhanced_security = {
                                    let s = settings_entity(cx).read(cx).settings.clone();
                                    s.security.security_mode == "enhanced" || s.security.master_password_set
                                };

                                let palette = SemanticPalette::from_theme(&t);
                                Some(
                                    div()
                                        .id("right-toolbar")
                                        .w(ui_right_toolbar_width(cx))
                                        .h_full()
                                        .overflow_hidden()
                                        .rounded(RADIUS_CARD)
                                        .border_1()
                                        .border_color(palette.border_subtle)
                                        .bg(palette.surface_card)
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .py(ui_space_md(cx))
                                        .gap(ui_space_md(cx))
                                        .children(visible_panels.into_iter().map(|spec| {
                                            let p_id = &spec.id;
                                            let icon = spec.icon;
                                            let is_active =
                                                self.right_toolbar_active.as_deref() == Some(p_id.as_str());
                                            let tooltip_text = i18n!(cx, &spec.title_key);
                                            let p_id_str = p_id.clone();

                                            let btn_id = format!("right-tb-btn-{}", p_id);
                                            velowork_ui::icon_button::icon_button(btn_id, icon, &t, cx)
                                                .when(is_active, |s| {
                                                    s.bg(surface_bg(t.bg_selection, cx))
                                                })
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    this.handle_right_toolbar_click(
                                                        &p_id_str, window, cx,
                                                    );
                                                }))
                                                .tooltip(move |_, cx| {
                                                    let tip = tooltip_text.clone();
                                                    cx.new(|_| Tooltip::new(tip)).into()
                                                })
                                        }))
                                        .when(is_enhanced_security, |d| {
                                            let tooltip_text = i18n!(cx, "dock.panel.lock");
                                            d.child(div().flex_1())
                                                .child(
                                                    velowork_ui::icon_button::icon_button(
                                                        "right-tb-btn-lock",
                                                        velowork_ui::icon::AppIcon::Lock,
                                                        &t,
                                                        cx,
                                                    )
                                                    .on_click(cx.listener(move |_this, _, window, cx| {
                                                        window.dispatch_action(Box::new(crate::keybindings::LockApp), cx);
                                                    }))
                                                    .tooltip(move |_, cx| {
                                                        let tip = tooltip_text.clone();
                                                        cx.new(|_| Tooltip::new(tip)).into()
                                                    }),
                                                )
                                        })
                                        .into_any_element(),
                                )
                            } else {
                                None
                            };

                            match (r_container, right_toolbar) {
                                (Some(dock), Some(tb)) => d.child(
                                    h_flex()
                                        .h_full()
                                        .ml(ui_space_card_gap(cx))
                                        .child(dock)
                                        .child(tb),
                                ),
                                (Some(dock), None) => d.child(dock),
                                (None, Some(tb)) => d.child(
                                    div()
                                        .h_full()
                                        .ml(ui_space_card_gap(cx))
                                        .child(tb),
                                ),
                                (None, None) => d,
                            }
                        },
                    ),
                    ) // end of content-area .child()
                    // Status bar at the bottom (hidden when any panel/dock/terminal is fullscreen)
                    .when(!{
                        let is_left_dock_maximized = {
                            let mode = self.left_dock.read(cx).mode;
                            mode == velowork_ui::dock::PanelMode::Maximized
                                || mode == velowork_ui::dock::PanelMode::Fullscreen
                        };
                        let is_right_dock_maximized = {
                            let mode = self
                                .right_dock
                                .as_ref()
                                .map(|d| d.read(cx).mode)
                                .unwrap_or(velowork_ui::dock::PanelMode::Normal);
                            mode == velowork_ui::dock::PanelMode::Maximized
                                || mode == velowork_ui::dock::PanelMode::Fullscreen
                        };
                        let bottom_fullscreen = self.is_bottom_dock_fullscreen(cx);
                        let terminal_fullscreen = self.focus_manager().read(cx).has_fullscreen();
                        is_left_dock_maximized
                            || is_right_dock_maximized
                            || bottom_fullscreen
                            || terminal_fullscreen
                    }, |d| {
                        d.child(self.status_bar.clone())
                    })
                    // Content modal overlays — positioned relative to content-root,
                    // so they cover the content area + status bar but NOT the titlebar.
                    // Stacked modal overlays (renders bottom-to-top so child modals layer over parent modals)
                    .children(self.overlay_manager.read(cx).render_modals().into_iter().map(|modal| {
                        div()
                            .absolute()
                            .inset_0()
                            .when(has_rounded_corners, |d| {
                                d.rounded(corner_radius).overflow_hidden()
                            })
                            .child(modal)
                    }))
                    // Sidebar active dialog overlay (session add/edit)
                    .when_some(self.sidebar.read(cx).active_dialog.clone(), |d, dialog| {
                        let content = self.sidebar.update(cx, |panel, cx| {
                            panel
                                .render_dialog_overlay(&dialog, window, cx)
                                .into_any_element()
                        });
                        d.child(
                            div()
                                .absolute()
                                .inset_0()
                                .when(has_rounded_corners, |d| {
                                    d.rounded(corner_radius).overflow_hidden()
                                })
                                .child(content),
                        )
                    })
                    // Quick-command / Overlay-manager delete-confirmation dialog fallback (centered with backdrop).
                    .when_some(
                        self.overlay_manager.read(cx).confirm_dialog.clone(),
                        |d, dialog| {
                            d.child(
                                velowork_ui::overlay::modal_backdrop(
                                    "om-confirm-backdrop",
                                    &crate::theme::theme(cx),
                                    cx,
                                )
                                .child(dialog),
                            )
                        },
                    )
            ) // end of content-root .child()
            // App menu dropdown (renders on top of everything when custom titlebar menu is open)
            .when(
                is_custom_titlebar && self.title_bar.read(cx).is_menu_open(),
                |d| d.child(self.title_bar.update(cx, |tb, cx| tb.render_menu(cx))),
            )
            // Context menu overlay (positioned popup, separate from modals)
            .when(has_context_menu, |d| {
                d.children(self.overlay_manager.read(cx).render_context_menu())
            })
            // Folder context menu overlay (positioned popup, separate from modals)
            .when(has_folder_context_menu, |d| {
                d.children(self.overlay_manager.read(cx).render_folder_context_menu())
            })
            // Terminal context menu overlay (positioned popup)
            .when(has_terminal_context_menu, |d| {
                d.children(self.overlay_manager.read(cx).render_terminal_context_menu())
            })
            // Terminal AI inline toolbar / popover overlay
            .when(has_terminal_ai_inline, |d| {
                d.children(self.overlay_manager.read(cx).render_terminal_ai_inline())
            })
            // Tab context menu overlay (positioned popup)
            .when(has_tab_context_menu, |d| {
                d.children(self.overlay_manager.read(cx).render_tab_context_menu())
            })
            // Quick-command tree context menu overlay (positioned popup)
            .when(has_quick_command_context_menu, |d| {
                d.children(
                    self.overlay_manager
                        .read(cx)
                        .render_quick_command_context_menu(),
                )
            })
            // Tunnel tree context menu overlay (positioned popup)
            .when(has_tunnel_context_menu, |d| {
                d.children(self.overlay_manager.read(cx).render_tunnel_context_menu())
            })
            // Service tree context menu overlay (positioned popup)
            .when(has_service_context_menu, |d| {
                d.children(self.overlay_manager.read(cx).render_service_context_menu())
            })
            // Monitor popup (positioned popup above the status bar, rendered at
            // window level so anchored() uses true window coordinates)
            .when_some(
                self.status_bar.update(cx, |sb, cx| sb.render_monitor_popup(window, cx)),
                |d, popup| {
                    d.child(popup)
                },
            )
            // Transfer manager popup (positioned popup above the status bar)
            .when(has_transfer_popup, |d| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .when(has_rounded_corners, |d| {
                            d.rounded(corner_radius).overflow_hidden()
                        })
                        .children(self.overlay_manager.read(cx).render_transfer_popup()),
                )
            })
            // Pane switcher overlay (numbered pane badges)
            .when_some(self.pane_switcher_entity.clone(), |d, entity| {
                d.child(entity)
            })
            // Toast notifications (bottom-right, on top of everything including all deferred overlays)
            .child(self.toast_overlay.clone())
    }
}
