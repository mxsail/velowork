//! Scrollbar component for terminal pane.

use velowork_terminal::terminal::Terminal;
use velowork_ui::theme::theme;
use velowork_ui::theme::with_alpha;
use gpui::*;
use std::sync::Arc;
use std::time::Instant;

const BUTTON_ZONE_HEIGHT: f32 = 24.0;

pub struct Scrollbar {
    terminal: Option<Arc<Terminal>>,
    dragging: bool,
    drag_start_y: Option<f32>,
    drag_start_offset: Option<usize>,
    last_activity: Instant,
    element_bounds: Option<Bounds<Pixels>>,
    hovered: bool,
    content_hovered: bool,
    fade_task: Option<gpui::Task<()>>,
}

impl Scrollbar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // 监听全局配置变化，以便在设置中切换滚动条模式时立即重绘生效
        cx.observe(&velowork_app_core::settings::settings_entity(cx), |_, _, cx| {
            cx.notify();
        })
        .detach();

        Self {
            terminal: None,
            dragging: false,
            drag_start_y: None,
            drag_start_offset: None,
            last_activity: Instant::now(),
            element_bounds: None,
            hovered: false,
            content_hovered: false,
            fade_task: None,
        }
    }

    pub fn set_terminal(&mut self, terminal: Option<Arc<Terminal>>) {
        self.terminal = terminal;
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    pub fn mark_activity(&mut self, cx: &mut Context<Self>) {
        self.last_activity = Instant::now();
        cx.notify();
        // 调度 350ms 后的淡出通知，使 Scrolling 模式在静止后自动隐藏
        self.fade_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(350))
                .await;
            let _ = this.update(cx, |_this, cx| {
                cx.notify();
            });
        }));
    }

    pub fn set_content_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.content_hovered != hovered {
            self.content_hovered = hovered;
            if hovered {
                self.last_activity = Instant::now();
            }
            cx.notify();
        }
    }

    fn should_show(&self, cx: &App) -> bool {
        let mode = velowork_app_core::settings::settings_entity(cx).read(cx).settings.terminal_scrollbar_show;
        match mode {
            velowork_ui::scrollable::ScrollbarShow::Never => false,
            velowork_ui::scrollable::ScrollbarShow::Always => true,
            velowork_ui::scrollable::ScrollbarShow::Hover => {
                self.hovered || self.content_hovered || self.dragging
            }
            velowork_ui::scrollable::ScrollbarShow::Scrolling => {
                if self.dragging || self.hovered || self.content_hovered {
                    return true;
                }
                self.last_activity.elapsed().as_millis() < 300
            }
        }
    }

    fn has_scroll_content(&self) -> bool {
        self.terminal
            .as_ref()
            .map(|t| {
                let (total, visible, _) = t.scroll_info();
                total > visible
            })
            .unwrap_or(false)
    }

    fn calculate_geometry(&self, content_height: f32) -> Option<(f32, f32)> {
        let track_height = content_height;
        let terminal = self.terminal.as_ref()?;
        let (total_lines, visible_lines, display_offset) = terminal.scroll_info();

        if total_lines <= visible_lines {
            return None;
        }

        let scrollable_lines = total_lines - visible_lines;
        let thumb_height = (visible_lines as f32 / total_lines as f32 * track_height).max(20.0);
        let available_space = track_height - thumb_height;
        let scroll_ratio = display_offset as f32 / scrollable_lines as f32;
        let thumb_y = (1.0 - scroll_ratio) * available_space;

        Some((thumb_y, thumb_height))
    }

    pub fn start_drag(&mut self, y: f32, cx: &mut Context<Self>) {
        if let Some(ref terminal) = self.terminal {
            self.dragging = true;
            self.drag_start_y = Some(y);
            self.drag_start_offset = Some(terminal.display_offset());
            self.last_activity = Instant::now();
            cx.notify();
        }
    }

    pub fn update_drag(&mut self, y: f32, content_height: f32, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }

        if let (Some(start_y), Some(start_offset), Some(terminal)) =
            (self.drag_start_y, self.drag_start_offset, &self.terminal)
        {
            let (total_lines, visible_lines, _) = terminal.scroll_info();
            if total_lines <= visible_lines {
                return;
            }

            let scrollable_lines = total_lines - visible_lines;
            let thumb_height = (visible_lines as f32 / total_lines as f32 * content_height).max(20.0);
            let available_space = (content_height - thumb_height).max(1.0);
            let delta_y = y - start_y;
            let lines_per_pixel = scrollable_lines as f32 / available_space;
            let delta_lines = (-delta_y * lines_per_pixel).round() as i32;

            let new_offset =
                (start_offset as i32 + delta_lines).clamp(0, scrollable_lines as i32) as usize;
            terminal.scroll_to(new_offset);

            self.last_activity = Instant::now();
            cx.notify();
        }
    }

    pub fn end_drag(&mut self, cx: &mut Context<Self>) {
        self.dragging = false;
        self.drag_start_y = None;
        self.drag_start_offset = None;
        cx.notify();
    }

    pub fn handle_click(&mut self, y: f32, content_height: f32, cx: &mut Context<Self>) {
        if let Some(ref terminal) = self.terminal {
            let (total_lines, visible_lines, _) = terminal.scroll_info();
            if total_lines <= visible_lines {
                return;
            }

            let scrollable_lines = total_lines - visible_lines;
            let thumb_height = (visible_lines as f32 / total_lines as f32 * content_height).max(20.0);
            let available_space = (content_height - thumb_height).max(1.0);
            let centered_y = (y - thumb_height / 2.0).clamp(0.0, available_space);
            let ratio = 1.0 - (centered_y / available_space);
            let new_offset = (ratio * scrollable_lines as f32).round() as usize;
            terminal.scroll_to(new_offset);

            self.last_activity = Instant::now();
            cx.notify();
        }
    }

    fn handle_mouse_down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.last_activity = Instant::now();

        if let Some(bounds) = self.element_bounds {
            let relative_y = f32::from(event.position.y) - f32::from(bounds.origin.y);
            let content_height = f32::from(bounds.size.height);

            if let Some((thumb_y, thumb_height)) = self.calculate_geometry(content_height) {
                if relative_y >= thumb_y && relative_y <= thumb_y + thumb_height {
                    self.start_drag(f32::from(event.position.y), cx);
                } else if !Self::is_in_zone(relative_y, content_height) {
                    self.handle_click(relative_y, content_height, cx);
                }
            }
        }
    }

    pub fn handle_mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let was_dragging = self.dragging;
        self.end_drag(cx);

        if was_dragging {
            return;
        }

        if let Some(bounds) = self.element_bounds {
            let relative_y = f32::from(event.position.y) - f32::from(bounds.origin.y);
            let content_height = f32::from(bounds.size.height);
            let zone = BUTTON_ZONE_HEIGHT.min(content_height / 4.0);

            if relative_y < zone {
                if let Some(ref terminal) = self.terminal {
                    let (total_lines, visible_lines, _) = terminal.scroll_info();
                    if total_lines > visible_lines {
                        terminal.scroll_to(total_lines - visible_lines);
                    }
                    self.last_activity = Instant::now();
                    cx.notify();
                }
            } else if relative_y > content_height - zone
                && let Some(ref terminal) = self.terminal {
                    terminal.scroll_to(0);
                    self.last_activity = Instant::now();
                    cx.notify();
                }
        }
    }

    fn is_in_zone(relative_y: f32, content_height: f32) -> bool {
        let zone = BUTTON_ZONE_HEIGHT.min(content_height / 4.0);
        relative_y < zone || relative_y > content_height - zone
    }
}

impl Render for Scrollbar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);

        if !self.has_scroll_content() {
            return div().into_any_element();
        }

        let mode = velowork_app_core::settings::settings_entity(cx)
            .read(cx)
            .settings
            .terminal_scrollbar_show;
        if mode == velowork_ui::scrollable::ScrollbarShow::Never {
            return div().id("scrollbar-hidden").into_any_element();
        }

        let visible = self.should_show(cx);
        let opacity = if visible { 1.0 } else { 0.0 };
        let terminal_clone = self.terminal.clone();
        let dragging = self.dragging;
        let hovered = self.hovered;

        let (thumb_color, thumb_width, thumb_inset, thumb_radius) = if dragging {
            (
                with_alpha(t.text_primary, 0.7),
                8.0,
                1.0,
                4.0,
            )
        } else if hovered {
            (
                with_alpha(t.text_primary, 0.6),
                8.0,
                1.0,
                4.0,
            )
        } else {
            (
                with_alpha(t.text_muted, 0.35),
                6.0,
                2.0,
                3.0,
            )
        };

        div()
            .id("scrollbar")
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(px(10.0))
            .opacity(opacity)
            .cursor(CursorStyle::Arrow)
            .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                this.hovered = *hovered;
                if *hovered {
                    this.last_activity = Instant::now();
                }
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                    this.handle_mouse_down(event, cx);
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                this.last_activity = Instant::now();
                if this.dragging
                    && let Some(bounds) = this.element_bounds {
                        let content_height = f32::from(bounds.size.height);
                        this.update_drag(f32::from(event.position.y), content_height, cx);
                    }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _window, cx| {
                    this.handle_mouse_up(event, cx);
                }),
            )
            .child(
                canvas(
                    {
                        let entity = cx.entity().downgrade();
                        move |bounds: Bounds<Pixels>, _window: &mut Window, cx: &mut App| {
                            if let Some(entity) = entity.upgrade() {
                                entity.update(cx, |this, _| {
                                    this.element_bounds = Some(bounds);
                                });
                            }
                        }
                    },
                    {
                        let entity = cx.entity().downgrade();
                        move |bounds: Bounds<Pixels>, _state: (), window: &mut Window, _cx: &mut App| {
                            if !visible {
                                return;
                            }
                            if let Some(ref terminal) = terminal_clone {
                                let (total_lines, visible_lines, display_offset) = terminal.scroll_info();
                                if total_lines > visible_lines {
                                    let track_height = f32::from(bounds.size.height);
                                    let scrollable_lines = total_lines - visible_lines;
                                    let thumb_height =
                                        (visible_lines as f32 / total_lines as f32 * track_height).max(20.0);
                                    let available_scroll_space = track_height - thumb_height;
                                    let scroll_ratio = display_offset as f32 / scrollable_lines as f32;
                                    let thumb_y = (1.0 - scroll_ratio) * available_scroll_space;

                                    let thumb_bounds = Bounds {
                                        origin: point(bounds.origin.x + px(thumb_inset), bounds.origin.y + px(thumb_y)),
                                        size: size(px(thumb_width), px(thumb_height)),
                                    };
                                    window.paint_quad(fill(thumb_bounds, thumb_color).corner_radii(px(thumb_radius)));
                                }
                            }

                            if dragging {
                                let entity = entity.clone();
                                let content_height = f32::from(bounds.size.height);
                                window.on_mouse_event({
                                    let entity = entity.clone();
                                    move |event: &MouseMoveEvent, phase, _window, cx| {
                                        if phase != DispatchPhase::Bubble {
                                            return;
                                        }
                                        if let Some(entity) = entity.upgrade() {
                                            entity.update(cx, |this, cx| {
                                                this.update_drag(f32::from(event.position.y), content_height, cx);
                                            });
                                        }
                                    }
                                });
                                window.on_mouse_event(move |_: &MouseUpEvent, phase, _window, cx| {
                                    if phase != DispatchPhase::Bubble {
                                        return;
                                    }
                                    if let Some(entity) = entity.upgrade() {
                                        entity.update(cx, |this, cx| {
                                            this.end_drag(cx);
                                        });
                                    }
                                });
                            }
                        }
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }
}
