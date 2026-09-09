pub use crate::workspace::toast::{Toast, ToastAction, ToastActionStyle, ToastLevel, ToastManager};

use crate::theme::theme;
use crate::ui::tokens::{
    ICON_SM, RADIUS_LG, RADIUS_MD, RADIUS_STD, SPACE_MD, SPACE_SM, SPACE_XS,
    ui_height_status_bar, ui_space_card_gap, ui_text_ms, ui_text_sm, ui_text_xs,
};
use gpui::prelude::FluentBuilder;
use gpui::*;
use std::time::Duration;
use velowork_i18n::i18n;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::icon::AppIcon;
use velowork_ui::scrollable::{Scrollbar, ScrollbarShow};
use velowork_ui::tooltip::Tooltip;

/// Emitted when a clickable toast action is clicked. The owning view (WindowView)
/// subscribes and routes it (e.g. soft-close undo / close-now).
#[derive(Clone, Debug)]
pub struct ToastActionEvent {
    pub toast_id: String,
    pub action_id: String,
}

/// Tick interval for the overlay's animation/prune loop
const TICK_INTERVAL: Duration = Duration::from_millis(50);

/// Duration of smooth entry animation (fade-in + slide-up)
const ENTER_ANIMATION_DURATION: Duration = Duration::from_millis(200);

/// Toast width
const TOAST_WIDTH: f32 = 330.0;

/// Accent stripe width
const ACCENT_WIDTH: f32 = 3.5;

trait ToastLevelExt {
    fn accent_color(self, t: &crate::theme::ThemeColors) -> u32;
}

impl ToastLevelExt for ToastLevel {
    fn accent_color(self, t: &crate::theme::ThemeColors) -> u32 {
        match self {
            ToastLevel::Success => t.success,
            ToastLevel::Error => t.error,
            ToastLevel::Warning => t.warning,
            ToastLevel::Info => t.accent,
        }
    }
}

/// Progress for entry transition (0.0 → 1.0 over ENTER_ANIMATION_DURATION)
fn toast_anim_progress(toast: &Toast) -> f32 {
    let elapsed = toast.created.elapsed();
    if elapsed >= ENTER_ANIMATION_DURATION {
        1.0
    } else {
        (elapsed.as_secs_f32() / ENTER_ANIMATION_DURATION.as_secs_f32()).clamp(0.0, 1.0)
    }
}

// ─── ToastOverlay (GPUI entity) ─────────────────────────────────────────────

pub struct ToastOverlay {
    toasts: Vec<Toast>,
    expanded: bool,
    is_hovered: bool,
    scroll_handle: ScrollHandle,
}

impl ToastOverlay {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // Start async tick loop for animations and expiry
        cx.spawn(async move |this: WeakEntity<ToastOverlay>, cx| {
            loop {
                smol::Timer::after(TICK_INTERVAL).await;

                let result = this.update(cx, |this, cx| {
                    if let Some(tm) = cx.try_global::<ToastManager>() {
                        // When hovered or expanded, pause auto-dismiss pruning so user can read,
                        // and shift created forward by TICK_INTERVAL so remaining TTL is preserved
                        let snapshot = if this.is_hovered || this.expanded {
                            let mut queue = tm.0.lock();
                            for toast in queue.iter_mut() {
                                toast.created += TICK_INTERVAL;
                            }
                            queue.clone()
                        } else {
                            tm.drain_snapshot()
                        };

                        if snapshot != this.toasts {
                            this.toasts = snapshot;
                            // Auto-collapse if count dropped to <= 3
                            if this.toasts.len() <= 3 {
                                this.expanded = false;
                            }
                            cx.notify();
                        }
                    }
                    // Re-render during active toasts (animations, countdown, expiry)
                    if !this.toasts.is_empty() {
                        cx.notify();
                    }
                });

                if result.is_err() {
                    break;
                }
            }
        })
        .detach();

        Self {
            toasts: Vec::new(),
            expanded: false,
            is_hovered: false,
            scroll_handle: ScrollHandle::new(),
        }
    }
}

impl EventEmitter<ToastActionEvent> for ToastOverlay {}

fn titlebar_height(window: &Window, cx: &App) -> f32 {
    let settings = crate::settings::settings_entity(cx).read(cx).settings.clone();
    let scale = velowork_ui::tokens::ui_scale_factor(cx);
    let is_custom_titlebar = if cfg!(target_os = "macos") {
        settings.titlebar_style == velowork_workspace::settings::TitlebarStyle::Custom
    } else {
        settings.titlebar_style == velowork_workspace::settings::TitlebarStyle::Custom
            || matches!(window.window_decorations(), Decorations::Client { .. })
    };
    if is_custom_titlebar && (!cfg!(target_os = "macos") || !window.is_fullscreen()) {
        settings.titlebar_height * scale
    } else {
        0.0
    }
}

impl Render for ToastOverlay {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.toasts.is_empty() {
            return div().into_any_element();
        }

        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);
        let text_size = ui_text_ms(cx);
        let detail_size = ui_text_xs(cx);
        let toasts = self.toasts.clone();
        let total_count = toasts.len();
        let is_stacked = !self.expanded && total_count > 3;

        let viewport_h = f32::from(window.viewport_size().height);
        let title_bar_h = titlebar_height(window, cx);
        // Ensure top of expanded panel never overlaps title bar, strictly leaving at least 16px gap below title bar
        let top_safe_limit = title_bar_h + 16.0;
        let status_bar_h = f32::from(ui_height_status_bar(cx));
        let card_gap = f32::from(ui_space_card_gap(cx));
        let bottom_offset = status_bar_h + card_gap;
        let max_panel_h = px((viewport_h - top_safe_limit - bottom_offset).max(150.0));
        let max_list_h = px((f32::from(max_panel_h) - 52.0).max(100.0));

        deferred(
            div()
                .occlude()
                .id("toast-overlay-container")
                .absolute()
                .bottom(px(bottom_offset)) // aligned with right toolbar above status bar
                .right(ui_space_card_gap(cx))
                .w(px(TOAST_WIDTH))
                .flex()
                .flex_col()
                .gap(SPACE_XS)
                .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                    this.is_hovered = *hovered;
                    cx.notify();
                }))
                .when(self.expanded, |container| {
                    // Expanded state: Header toolbar + scrollable list of all toasts
                    let collapse_tip = i18n!(cx, "toast.collapse");
                    let clear_tip = i18n!(cx, "toast.clear_all");

                    container
                        .on_mouse_down_out(cx.listener(|this, _ev, _w, cx| {
                            this.expanded = false;
                            this.is_hovered = false;
                            cx.notify();
                        }))
                        .max_h(max_panel_h)
                        .p(SPACE_SM)
                        .bg(p.surface_raised)
                        .border_1()
                        .border_color(p.border_subtle)
                        .rounded(RADIUS_LG)
                        .shadow_2xl()
                        // 1. Header Toolbar
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .pb(SPACE_XS)
                                .px(SPACE_XS)
                                .border_b_1()
                                .border_color(p.border_subtle)
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(SPACE_XS)
                                        .text_size(ui_text_sm(cx))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(p.text_primary)
                                        .child(i18n!(cx, "toast.title"))
                                        .child(
                                            div()
                                                .px(SPACE_XS)
                                                .py(px(1.0))
                                                .rounded_full()
                                                .bg(p.surface_selection)
                                                .text_size(ui_text_xs(cx))
                                                .text_color(p.text_secondary)
                                                .child(total_count.to_string()),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(SPACE_XS)
                                        // Clear all button
                                        .child(
                                            div()
                                                .id("toast-clear-all-btn")
                                                .cursor_pointer()
                                                .px(SPACE_XS)
                                                .py(px(2.0))
                                                .rounded(RADIUS_STD)
                                                .text_size(ui_text_xs(cx))
                                                .text_color(p.text_muted)
                                                .hover(|s| s.text_color(p.status_error).bg(p.surface_hover))
                                                .child(clear_tip)
                                                .on_click(cx.listener(|this, _, _w, cx| {
                                                    ToastManager::dismiss_all(cx);
                                                    this.expanded = false;
                                                    cx.notify();
                                                })),
                                        )
                                        // Collapse button
                                        .child(
                                            div()
                                                .id("toast-collapse-btn")
                                                .cursor_pointer()
                                                .p(px(2.0))
                                                .rounded(RADIUS_STD)
                                                .text_color(p.text_muted)
                                                .hover(|s| s.text_color(p.text_primary).bg(p.surface_hover))
                                                .tooltip(move |_, cx| {
                                                    cx.new(|_| Tooltip::new(collapse_tip.clone())).into()
                                                })
                                                .child(AppIcon::ChevronDown.size(ICON_SM))
                                                .on_click(cx.listener(|this, _, _w, cx| {
                                                    this.expanded = false;
                                                    cx.notify();
                                                })),
                                        ),
                                ),
                        )
                        // 2. Scrollable Toast List with on-demand vertical scrollbar
                        .child(
                            div()
                                .relative()
                                .w_full()
                                .overflow_hidden()
                                .child(
                                    div()
                                        .id("toast-scroll-list")
                                        .max_h(max_list_h)
                                        .w_full()
                                        .overflow_y_scroll()
                                        .track_scroll(&self.scroll_handle)
                                        .flex()
                                        .flex_col()
                                        .gap(SPACE_XS)
                                        .pt(SPACE_XS)
                                        .pr(SPACE_XS)
                                        .children(toasts.iter().rev().map(|toast| {
                                            render_single_toast(
                                                toast,
                                                &t,
                                                &p,
                                                text_size,
                                                detail_size,
                                                false,
                                                0,
                                                cx,
                                            )
                                        })),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .bottom_0()
                                        .right_0()
                                        .left_0()
                                        .child(
                                            Scrollbar::vertical(&self.scroll_handle)
                                                .scrollbar_show(ScrollbarShow::Hover),
                                        ),
                                ),
                        )
                })
                .when(!self.expanded && !is_stacked, |container| {
                    // Standard list for <= 3 toasts
                    container.children(toasts.iter().map(|toast| {
                        render_single_toast(
                            toast,
                            &t,
                            &p,
                            text_size,
                            detail_size,
                            false,
                            0,
                            cx,
                        )
                    }))
                })
                .when(is_stacked, |container| {
                    // Sonner-style stacked cards for > 3 toasts
                    let extra_count = total_count - 1;
                    let Some(latest_toast) = toasts.last() else {
                        return container;
                    };

                    container.child(
                        div()
                            .id("toast-stack-inner")
                            .relative()
                            .w_full()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _w, cx| {
                                this.expanded = true;
                                cx.notify();
                            }))
                            // Background Card Layer 3 (deepest)
                            .child(
                                div()
                                    .id("toast-stack-bg-3")
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _w, cx| {
                                        this.expanded = true;
                                        cx.notify();
                                    }))
                                    .absolute()
                                    .top(px(-12.0))
                                    .left(px(14.0))
                                    .right(px(14.0))
                                    .h(px(40.0))
                                    .bg(p.surface_raised)
                                    .border_1()
                                    .border_color(p.border_subtle)
                                    .rounded(RADIUS_LG)
                                    .shadow_sm()
                                    .opacity(0.40),
                            )
                            // Background Card Layer 2 (middle)
                            .child(
                                div()
                                    .id("toast-stack-bg-2")
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _w, cx| {
                                        this.expanded = true;
                                        cx.notify();
                                    }))
                                    .absolute()
                                    .top(px(-6.0))
                                    .left(px(7.0))
                                    .right(px(7.0))
                                    .h(px(44.0))
                                    .bg(p.surface_raised)
                                    .border_1()
                                    .border_color(p.border_subtle)
                                    .rounded(RADIUS_LG)
                                    .shadow_md()
                                    .opacity(0.70),
                            )
                            // Foreground Latest Card (Layer 1) with "+N" badge
                            .child(
                                render_single_toast(
                                    latest_toast,
                                    &t,
                                    &p,
                                    text_size,
                                    detail_size,
                                    true,
                                    extra_count,
                                    cx,
                                ),
                            ),
                    )
                }),
        )
        .into_any_element()
    }
}

/// Render a single toast card with full-height adaptive indicator stripe,
/// entry animation, and optional stack badge.
fn render_single_toast(
    toast: &Toast,
    t: &crate::theme::ThemeColors,
    p: &SemanticPalette,
    text_size: Pixels,
    detail_size: Pixels,
    is_stack_front: bool,
    extra_count: usize,
    cx: &mut Context<ToastOverlay>,
) -> AnyElement {
    let accent_color = toast.level.accent_color(t);
    let progress = toast_anim_progress(toast);
    let opacity = progress;
    let offset_y = (1.0 - progress) * 10.0;
    let toast_id = toast.id.clone();
    let has_countdown = !toast.actions.is_empty();
    let remaining = toast.remaining_fraction();
    let close_tip = i18n!(cx, "common.close");

    div()
        .id(SharedString::from(format!("toast-{}", toast.id)))
        .flex_shrink_0()
        .w_full()
        .min_h(px(40.0))
        .relative()
        .top(px(offset_y))
        .opacity(opacity)
        .bg(p.surface_raised)
        .border_1()
        .border_color(p.border_subtle)
        .rounded(RADIUS_LG)
        .shadow_xl()
        .flex()
        .flex_col()
        .justify_center()
        .overflow_hidden()
        .when(is_stack_front, |el| {
            el.cursor_pointer()
                .hover(move |s| s.bg(p.surface_hover))
                .on_click(cx.listener(|this, _, _w, cx| {
                    this.expanded = true;
                    cx.notify();
                }))
        })
        // Main row: Full-height adaptive accent stripe + content column
        .child(
            div()
                .flex()
                .flex_row()
                .w_full()
                .flex_1()
                .items_stretch()
                // Left accent indicator stripe (floating inset pill)
                .child(
                    div()
                        .py(SPACE_SM)
                        .pl(SPACE_SM)
                        .flex()
                        .items_stretch()
                        .flex_shrink_0()
                        .child(
                            div()
                                .w(px(ACCENT_WIDTH))
                                .self_stretch()
                                .rounded_full()
                                .bg(rgb(accent_color)),
                        ),
                )
                // Content column (message row + optional actions row)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .flex_1()
                        .overflow_x_hidden()
                        .gap(SPACE_XS)
                        .pl(SPACE_SM)
                        .pr(SPACE_MD)
                        .py(SPACE_SM)
                        // Message row
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(SPACE_SM)
                                // Message + optional detail line
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .overflow_x_hidden()
                                        .flex()
                                        .flex_col()
                                        .justify_center()
                                        .gap(px(1.0))
                                        .child(
                                            div()
                                                .whitespace_normal()
                                                .text_size(text_size)
                                                .font_weight(FontWeight::NORMAL)
                                                .text_color(p.text_primary)
                                                .child(toast.message.clone()),
                                        )
                                        .when_some(
                                            toast.detail.clone(),
                                            |el, detail| {
                                                el.child(
                                                    div()
                                                        .whitespace_normal()
                                                        .text_size(detail_size)
                                                        .text_color(p.text_muted)
                                                        .child(detail),
                                                )
                                            },
                                        ),
                                )
                                // Stack badge (if in collapsed stack mode)
                                .when(is_stack_front && extra_count > 0, |el| {
                                    el.child(
                                        div()
                                            .id("toast-stack-badge")
                                            .cursor_pointer()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(px(2.0))
                                            .px(SPACE_XS)
                                            .py(px(1.5))
                                            .rounded_full()
                                            .bg(p.surface_selection)
                                            .hover(move |s| s.bg(p.surface_card))
                                            .text_size(ui_text_xs(cx))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(t.accent))
                                            .child(format!("+{}", extra_count))
                                            .child(AppIcon::ChevronUp.size(ICON_SM))
                                            .on_click(cx.listener(|this, _, _w, cx| {
                                                this.expanded = true;
                                                cx.notify();
                                            })),
                                    )
                                })
                                // Dismiss button
                                .when(!has_countdown && !is_stack_front, |el| {
                                    el.child(
                                        div()
                                            .id(SharedString::from(format!(
                                                "toast-close-{}",
                                                toast.id
                                            )))
                                            .cursor_pointer()
                                            .flex_shrink_0()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(RADIUS_STD)
                                            .p(px(2.0))
                                            .hover(move |s| s.bg(p.surface_hover))
                                            .tooltip(move |_, cx| {
                                                cx.new(|_| {
                                                    Tooltip::new(close_tip.clone())
                                                })
                                                .into()
                                            })
                                            .child(
                                                AppIcon::Close
                                                    .size(ICON_SM)
                                                    .text_color(p.text_muted),
                                            )
                                            .on_click(move |_, _window, cx| {
                                                ToastManager::dismiss(&toast_id, cx);
                                            }),
                                    )
                                }),
                        )
                        // Actions row (Undo / Close now / …)
                        .when(has_countdown, |el| {
                            el.child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .justify_end()
                                    .gap(SPACE_XS)
                                    .children(toast.actions.iter().map(|action| {
                                        let toast_id = toast.id.clone();
                                        let action_id = action.id.clone();
                                        let on_click = cx.listener(
                                            move |_this, _ev: &ClickEvent, _window, cx| {
                                                cx.emit(ToastActionEvent {
                                                    toast_id: toast_id.clone(),
                                                    action_id: action_id.clone(),
                                                });
                                            },
                                        );
                                        action_button(
                                            &toast.id, action, t, p, text_size, on_click,
                                        )
                                    })),
                            )
                        }),
                ),
        )
        // Countdown progress bar (only for toasts with actions)
        .when(has_countdown, |el| {
            el.child(
                div()
                    .mx(SPACE_MD)
                    .mb(SPACE_XS)
                    .h(px(2.0))
                    .bg(p.border_subtle)
                    .rounded_full()
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .w(relative(remaining))
                            .bg(rgb(accent_color))
                            .rounded_full(),
                    ),
            )
        })
        .into_any_element()
}

/// Render a single clickable toast action button. The `on_click` handler is
/// built by the caller (via `cx.listener`) so this stays free of the context
/// lifetime.
fn action_button(
    toast_id: &str,
    action: &ToastAction,
    t: &crate::theme::ThemeColors,
    p: &SemanticPalette,
    text_size: Pixels,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label_color = match action.style {
        ToastActionStyle::Primary => t.accent,
        ToastActionStyle::Danger => t.error,
        ToastActionStyle::Default => t.text_secondary,
    };
    let hover_bg = p.surface_hover;

    div()
        .id(SharedString::from(format!(
            "toast-action-{}-{}",
            toast_id, action.id
        )))
        .cursor_pointer()
        .px(SPACE_SM)
        .py(px(2.0))
        .rounded(RADIUS_MD)
        .text_size(text_size)
        .text_color(rgb(label_color))
        .hover(move |s| s.bg(hover_bg))
        .child(action.label.clone())
        .on_click(on_click)
}

#[cfg(test)]
mod tests {
    use super::{Toast, ToastLevel, ToastManager};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_toast_expiry() {
        let toast = Toast::error("fail").with_ttl(Duration::from_millis(50));
        assert!(!toast.is_expired());
        thread::sleep(Duration::from_millis(60));
        assert!(toast.is_expired());
    }

    #[test]
    fn test_drain_snapshot_prunes_expired() {
        let tm = ToastManager::new();
        {
            let mut q = tm.0.lock();
            q.push(Toast::success("a"));
            q.push(Toast::error("b").with_ttl(Duration::from_millis(1)));
            q.push(Toast::warning("c"));
        }
        // Wait for the short-TTL toast to expire
        thread::sleep(Duration::from_millis(10));
        let snapshot = tm.drain_snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].message, "a");
        assert_eq!(snapshot[1].message, "c");
    }

    #[test]
    fn test_queue_cap() {
        let tm = ToastManager::new();
        {
            let mut q = tm.0.lock();
            for i in 0..25 {
                q.push(Toast::info(format!("msg-{}", i)));
            }
            while q.len() > 20 {
                q.remove(0);
            }
        }
        let q = tm.0.lock();
        assert_eq!(q.len(), 20);
        assert_eq!(q[0].message, "msg-5");
    }

    #[test]
    fn test_dismiss_by_id() {
        let tm = ToastManager::new();
        let ids: Vec<String>;
        {
            let mut q = tm.0.lock();
            q.push(Toast::success("a"));
            q.push(Toast::error("b"));
            q.push(Toast::warning("c"));
            ids = q.iter().map(|t| t.id.clone()).collect();
        }
        // Dismiss the middle toast
        tm.0.lock().retain(|t| t.id != ids[1]);
        let q = tm.0.lock();
        assert_eq!(q.len(), 2);
        assert_eq!(q[0].id, ids[0]);
        assert_eq!(q[1].id, ids[2]);
    }

    #[test]
    fn test_with_ttl_builder() {
        let toast = Toast::error("x").with_ttl(Duration::from_secs(30));
        assert_eq!(toast.ttl, Duration::from_secs(30));
        assert_eq!(toast.level, ToastLevel::Error);
    }
}
