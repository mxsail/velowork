//! Generic wrapper for hosting any `Render + Focusable` entity in a separate OS
//! window.
//!
//! `DetachedOverlayView<T, E>` hosts an arbitrary entity (a detached terminal,
//! a detached dock panel, a file/diff viewer, …) in a dedicated OS window with
//! the app's standard client-drawn chrome. The wrapped entity is responsible
//! for its own header, drag area and re-attach UI; the wrapper only handles:
//! initial focus hand-off, repaint propagation, window-bounds persistence, and
//! closing the window.
//!
//! Closing happens in two ways:
//! * the wrapped content emits its `CloseEvent` (file/diff viewers) → the window
//!   closes, or
//! * the OS requests a close → `on_close` runs (dock panels re-attach here),
//!   then the window closes.
//!
//! Entities that don't emit a `CloseEvent` (terminals, dock panels) are hosted
//! through `DetachedHost<T>`, a zero-cost adapter that satisfies the
//! `EventEmitter` bound with a never-firing event.

use gpui::*;
use gpui::prelude::FluentBuilder;
use velowork_ui::overlay::CloseEvent;
use velowork_ui::theme::{surface_bg, theme};
use std::marker::PhantomData;
use std::sync::Arc;

/// A never-firing close event used by `DetachedHost`.
pub struct DetachedHostCloseEvent;

impl CloseEvent for DetachedHostCloseEvent {
    fn is_close(&self) -> bool {
        false
    }
}

/// Zero-cost adapter that lets any `Render + Focusable` entity be hosted by
/// `DetachedOverlayView` without the entity itself emitting a `CloseEvent`.
pub struct DetachedHost<T: Render + Focusable + 'static> {
    pub inner: Entity<T>,
}

impl<T: Render + Focusable + 'static> Render for DetachedHost<T> {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.inner.clone())
    }
}

impl<T: Render + Focusable + 'static> Focusable for DetachedHost<T> {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.inner.read(cx).focus_handle(cx)
    }
}

impl<T: Render + Focusable + 'static> EventEmitter<DetachedHostCloseEvent> for DetachedHost<T> {}

use velowork_ui::overlay_registry::OverlayRegistry;

pub struct DetachedOverlayView<T: Render + 'static, E: 'static> {
    content: Entity<T>,
    overlay_registry: Entity<OverlayRegistry>,
    #[allow(dead_code)]
    title: SharedString,
    focus_handle: FocusHandle,
    should_close: bool,
    hide_titlebar: bool,
    _phantom: PhantomData<E>,
}

impl<T, E> DetachedOverlayView<T, E>
where
    T: Render + Focusable + EventEmitter<E> + 'static,
    E: CloseEvent + 'static,
{
    pub fn new(
        content: Entity<T>,
        overlay_registry: Entity<OverlayRegistry>,
        title: impl Into<SharedString>,
        on_close: Option<Arc<dyn Fn(&mut Window, &mut gpui::App)>>,
        hide_titlebar: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();

        // Hand initial focus to the wrapped content off-render
        let content_focus = content.read(cx).focus_handle(cx);
        window.defer(cx, move |window, cx| {
            window.focus(&content_focus, cx);
        });

        // Close the window when the wrapped content emits its Close event
        // (file/diff viewers, etc.). Terminal/dock hosts emit no such event and
        // close themselves (or via `on_close`).
        cx.subscribe(&content, |this, _, event: &E, cx| {
            if event.is_close() {
                this.should_close = true;
                cx.notify();
            }
        })
        .detach();

        // Repaint when the wrapped content notifies (since we render it inside
        // our hierarchy, GPUI doesn't otherwise know to re-run our render fn).
        cx.observe(&content, |_, _, cx| {
            cx.notify();
        })
        .detach();

        // When the OS asks the window to close, run `on_close` (dock panels use
        // this to re-attach instead of closing) and then close the window.
        let close_handler = on_close.clone();
        window.on_window_should_close(cx, move |window, cx| {
            if let Some(handler) = &close_handler {
                handler(window, cx);
            }
            window.remove_window();
            true
        });

        // Persist window bounds + state (windowed/maximized/fullscreen) whenever
        // they change so the next detached window opens the same way.
        let title_for_persist = title.into();
        cx.observe_window_bounds(window, move |_this, window, cx| {
            use velowork_workspace::settings::{DetachedWindowBounds, DetachedWindowState};
            let wb = window.window_bounds();
            let bounds = wb.get_bounds();
            let state = match wb {
                WindowBounds::Windowed(_) => DetachedWindowState::Windowed,
                WindowBounds::Maximized(_) => DetachedWindowState::Maximized,
                WindowBounds::Fullscreen(_) => DetachedWindowState::Fullscreen,
            };
            let snapshot = DetachedWindowBounds {
                origin_x: f32::from(bounds.origin.x),
                origin_y: f32::from(bounds.origin.y),
                width: f32::from(bounds.size.width),
                height: f32::from(bounds.size.height),
                state,
            };
            if let Some(global) = cx.try_global::<crate::settings::GlobalSettings>() {
                global.0.clone().update(cx, |state, cx| {
                    state.set_detached_overlay_bounds(snapshot, cx);
                });
            }
        })
        .detach();

        Self {
            content,
            overlay_registry,
            title: title_for_persist,
            focus_handle,
            should_close: false,
            hide_titlebar,
            _phantom: PhantomData,
        }
    }
}

impl<T, E> Render for DetachedOverlayView<T, E>
where
    T: Render + Focusable + EventEmitter<E> + 'static,
    E: CloseEvent + 'static,
{
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Close the window when the content asked us to (via its CloseEvent).
        if self.should_close {
            window.remove_window();
            return div().into_any_element();
        }

        let t = theme(cx);
        let focus_handle = self.focus_handle.clone();
        let overlay_registry = self.overlay_registry.clone();

        let is_custom_titlebar = if self.hide_titlebar {
            false
        } else if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
            if let Some(global) = cx.try_global::<velowork_app_core::settings::GlobalSettings>() {
                global.0.read(cx).settings.titlebar_style == velowork_workspace::settings::TitlebarStyle::Custom
            } else {
                true
            }
        } else {
            matches!(window.window_decorations(), Decorations::Client { .. })
        };
        let window_corner_radius = if let Some(global) = cx.try_global::<velowork_app_core::settings::GlobalSettings>() {
            global.0.read(cx).settings.window_corner_radius
        } else {
            8.0
        };
        let is_maximized = window.is_maximized();
        let is_fullscreen = window.is_fullscreen();
        let has_rounded_corners = is_custom_titlebar && !is_maximized && !is_fullscreen && window_corner_radius > 0.0;
        let radius = px(window_corner_radius);

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

        let decoration_config = velowork_ui::decorations::WindowDecorationConfig::from_custom(
            style_override,
            pos_override,
            Some(titlebar_gap),
            Some(titlebar_margin),
        );

        let titlebar_element = if is_custom_titlebar && (!cfg!(target_os = "macos") || !is_fullscreen) {
            let scale = velowork_ui::tokens::ui_scale_factor(cx);
            let title_bar_height = px(titlebar_height_val * scale);
            let title_str = self.title.clone();
            let needs_controls = velowork_ui::overlay::detached_needs_controls(window);
            let is_left_controls = decoration_config.position == velowork_ui::decorations::WindowButtonPosition::Left;
            let is_right_controls = decoration_config.position == velowork_ui::decorations::WindowButtonPosition::Right;

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
                velowork_ui::tokens::SPACE_SM
            };

            let left_controls = if needs_controls && is_left_controls {
                Some(velowork_ui::title_bar::render_window_controls(
                    "detached-overlay-ctrl-left",
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
                Some(velowork_ui::title_bar::render_window_controls(
                    "detached-overlay-ctrl-right",
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
            let app_icon_sz = px((titlebar_height_val * 0.52).clamp(14.0, 24.0) * scale);
            let p = velowork_ui::design::semantic::SemanticPalette::from_context(cx);

            Some(
                div()
                    .id("detached-overlay-titlebar")
                    .h(title_bar_height)
                    .w_full()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(p.surface_card)
                    .border_b_1()
                    .border_color(p.border_subtle)
                    .when(has_rounded_corners, |d| d.rounded_tl(radius).rounded_tr(radius))
                    .window_control_area(WindowControlArea::Drag)
                    .when(cfg!(target_os = "linux"), |d| {
                        d.on_mouse_down(MouseButton::Left, |e, window, _| {
                            if e.click_count == 2 {
                                window.zoom_window();
                            } else {
                                window.start_window_move();
                            }
                        })
                    })
                    .child(
                        velowork_ui::h_flex()
                            .gap(velowork_ui::tokens::SPACE_SM)
                            .pl(left_padding)
                            .items_center()
                            .children(left_controls)
                            .child(
                                velowork_ui::h_flex()
                                    .id("detached-titlebar-app-icon")
                                    .items_center()
                                    .child(velowork_ui::brand_logo(app_icon_sz, window, cx)),
                            ),
                    )
                    .child(
                        div().flex_1().flex().items_center().justify_center().child(
                            velowork_ui::h_flex().gap(velowork_ui::tokens::SPACE_XS).items_center().child(
                                div()
                                    .text_size(velowork_ui::tokens::ui_text(13.0, cx))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(t.text_primary))
                                    .child(title_str),
                            ),
                        ),
                    )
                    .child(
                        velowork_ui::h_flex()
                            .pr(right_padding)
                            .items_center()
                            .children(right_controls),
                    ),
            )
        } else {
            None
        };

        div()
            .id("detached-overlay-root")
            .track_focus(&focus_handle)
            .key_context("DetachedOverlay")
            .size_full()
            .flex()
            .flex_col()
            .text_size(velowork_ui::tokens::ui_text_md(cx))
            .text_color(rgb(t.text_primary))
            .when(has_rounded_corners, |d| d.rounded(radius).overflow_hidden())
            .bg(surface_bg(t.bg_primary, cx))
            .child(
                canvas(
                    |_bounds, _window, _cx| {},
                    move |_bounds, _prepaint, window, _cx| {
                        let overlay_registry = overlay_registry.clone();
                        window.on_mouse_event(move |e: &MouseDownEvent, _phase, window, cx| {
                            if e.button == MouseButton::Left {
                                overlay_registry.update(cx, |r, cx| {
                                    r.handle_mouse_down(e.position, window, cx);
                                });
                            }
                        });
                    },
                )
                .absolute()
                .inset_0(),
            )
            .children(titlebar_element)
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .relative()
                    .when(has_rounded_corners, |d| d.rounded_b(radius).overflow_hidden())
                    .child(self.content.clone()),
            )
            .into_any_element()
    }
}

impl<T, E> Focusable for DetachedOverlayView<T, E>
where
    T: Render + Focusable + EventEmitter<E> + 'static,
    E: CloseEvent + 'static,
{
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
