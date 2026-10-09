//! Simple root wrapper with client-side decorations (rounded corners, shadow,
//! resize handling), ported from Zed's `workspace::client_side_decorations`.
//!
//! Key insight from Zed: on Wayland the window must call
//! `window.set_client_inset(shadow_size)` so gpui shrinks the xdg window
//! geometry to the inner frame (excluding the transparent shadow ring). The
//! compositor (KWin etc.) then treats the inner frame as the window — snapping,
//! maximizing and its own effects all align with the rounded frame instead of
//! the rectangular surface, which is what previously produced square tips
//! outside the rounded corners.

use gpui::{
    AnyView, Bounds, BoxShadow, Context, CursorStyle, Decorations, Global, Hitbox, HitboxBehavior,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Point, Render, ResizeEdge,
    Size, Styled, Tiling, Window, WindowBackgroundAppearance, black, canvas, div,
    point, prelude::FluentBuilder, px, size, transparent_black,
};
use velowork_ui::theme::{bg_opacity, theme, with_alpha};
use velowork_ui::tokens::{ui_font_family, use_custom_ui_font};
use crate::theme::theme_entity;

/// Width of the transparent shadow ring around the rounded frame. Mirrors
/// Zed's `theme::CLIENT_SIDE_DECORATION_SHADOW` (10px). This MUST match the
/// value passed to `window.set_client_inset` so the xdg window geometry lines
/// up exactly with the visible frame.
const SHADOW_SIZE: Pixels = px(10.0);

/// Frame border width, like Zed's `BORDER_SIZE`.
const BORDER_SIZE: Pixels = px(1.0);

/// Edge detection zone size for CSD resize when no shadow ring is shown
/// (custom titlebar with rounded corners disabled).
const RESIZE_EDGE_SIZE: Pixels = px(8.0);

/// Last hovered resize edge, used to refresh the resize cursor (same pattern
/// as Zed's `GlobalResizeEdge`).
struct GlobalResizeEdge(ResizeEdge);
impl Global for GlobalResizeEdge {}

/// Simple root view wrapper that provides CSD rounded corners, drop shadow and
/// resize edges, following Zed's implementation.
pub struct SimpleRoot {
    view: AnyView,
    /// Last window background appearance seen, so we only reconfigure the compositor
    /// when the underlying composition or material actually changes.
    last_appearance: Option<WindowBackgroundAppearance>,
    /// Last rounded-corner state seen. Tracked separately so toggling rounded corners
    /// (or maximizing the window) re-applies the correct window appearance.
    last_has_rounded_corners: bool,
    /// Last corner radius published to `WindowCornerRadius` so `modal_backdrop`
    /// can clip its dimming mask to the rounded window corners.
    last_modal_corner_radius: f32,
    /// Last theme brightness (dark mode) seen, so DWM Immersive Dark Mode stays
    /// in sync when switching between dark and light themes.
    last_is_dark: Option<bool>,
    /// Last custom titlebar state seen, so DWM non-client rendering policy disables
    /// duplicate system caption buttons when custom titlebar is active.
    last_is_custom_titlebar: Option<bool>,
}

impl SimpleRoot {
    pub fn new(view: impl Into<AnyView>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Re-render this detached window when the global theme changes so a
        // color-schema / palette switch made in the main window (or here) shows
        // up immediately instead of requiring a restart.
        cx.observe(&theme_entity(cx), |_this, _theme, cx| cx.notify())
            .detach();

        if let Some(global_settings) = cx.try_global::<crate::settings::GlobalSettings>().map(|g| g.0.clone()) {
            cx.observe(&global_settings, |_this, _settings, cx| cx.notify())
                .detach();
        }

        // 注册定时空闲内存整理（每 30 秒自动清理 jemalloc 无用脏页并归还给操作系统 RSS）
        cx.spawn(async move |_this, _cx| loop {
            smol::Timer::after(std::time::Duration::from_secs(30)).await;
            velowork_core::memory::trim_process_memory();
        })
        .detach();

        Self {
            view: view.into(),
            last_appearance: None,
            last_has_rounded_corners: false,
            last_modal_corner_radius: -1.0,
            last_is_dark: None,
            last_is_custom_titlebar: None,
        }
    }
}

impl Render for SimpleRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let decorations = window.window_decorations();
        let t = theme(cx);

        let is_custom_titlebar = if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
            crate::settings::settings_entity(cx)
                .read(cx)
                .settings
                .titlebar_style == velowork_workspace::settings::TitlebarStyle::Custom
        } else {
            matches!(decorations, Decorations::Client { .. })
        };

        let window_corner_radius = crate::settings::settings_entity(cx)
            .read(cx)
            .settings
            .window_corner_radius;

        let is_maximized = window.is_maximized();
        let has_rounded_corners = is_custom_titlebar && !is_maximized && window_corner_radius > 0.0;

        // Publish the active corner radius so `modal_backdrop` clips its
        // full-window dimming mask to the rounded window corners (GPUI's
        // `overflow_hidden` never clips the rounded shape).
        let modal_corner_radius = if has_rounded_corners { window_corner_radius } else { 0.0 };
        if self.last_modal_corner_radius != modal_corner_radius {
            cx.set_global(velowork_ui::WindowCornerRadius(modal_corner_radius));
            self.last_modal_corner_radius = modal_corner_radius;
        }

        let tiling = match decorations {
            Decorations::Server => Tiling::default(),
            Decorations::Client { tiling } => tiling,
        };

        // Client-side drop shadow ring is ONLY used on Linux Wayland/X11 CSD.
        // On Windows and macOS, the OS window manager/DWM provides the native shadow,
        // native 1px border, and handles edge resizing natively.
        let is_csd_shadow = cfg!(target_os = "linux") && matches!(decorations, Decorations::Client { .. });

        // Tell gpui how far the visible frame is inset from the surface edge.
        // On Wayland this shrinks the xdg window geometry to the inner frame.
        if is_csd_shadow && has_rounded_corners {
            window.set_client_inset(SHADOW_SIZE);
        } else {
            window.set_client_inset(px(0.0));
        }

        // Keep the compositor's backdrop blur/transparency in sync.
        let target_appearance = crate::settings::window_background_appearance(
            &crate::settings::settings_entity(cx).read(cx).settings,
        );

        let is_dark = t.is_dark();

        if self.last_appearance != Some(target_appearance)
            || self.last_has_rounded_corners != has_rounded_corners
            || self.last_is_dark != Some(is_dark)
            || self.last_is_custom_titlebar != Some(is_custom_titlebar)
        {
            log::info!(
                "[simple_root] Appearance change: rounded={}, is_dark={}, custom_titlebar={}, target={:?}",
                has_rounded_corners,
                is_dark,
                is_custom_titlebar,
                target_appearance
            );
            window.set_background_appearance(target_appearance);
            crate::platform::windows::sync_window_dwm_frame(
                window,
                target_appearance != WindowBackgroundAppearance::Opaque,
                is_dark,
                is_custom_titlebar,
            );

            self.last_appearance = Some(target_appearance);
            self.last_has_rounded_corners = has_rounded_corners;
            self.last_is_dark = Some(is_dark);
            self.last_is_custom_titlebar = Some(is_custom_titlebar);
        }

        let opacity = bg_opacity(cx);
        let content_bg = if opacity >= 1.0 {
            with_alpha(t.bg_primary, 1.0)
        } else {
            with_alpha(t.bg_primary, 0.0)
        };

        let radius = px(window_corner_radius);
        let entity = cx.entity();

        // 复合自适应边缘设计（仅在 Linux CSD 绘制阴影与模拟边框）：
        let border_alpha = if opacity < 1.0 {
            (opacity + (1.0 - opacity) * 0.32).min(1.0)
        } else {
            1.0
        };
        let border_color = if opacity < 1.0 {
            with_alpha(t.border, border_alpha)
        } else {
            with_alpha(t.border, 0.6)
        };

        let (shadow_opacity, shadow_blur) = if opacity < 1.0 {
            let factor = (0.35 + 0.65 * opacity).clamp(0.0, 1.0);
            (0.4 * factor, (SHADOW_SIZE / 2.0) * factor)
        } else {
            (0.4, SHADOW_SIZE / 2.0)
        };

        // Inner frame — content wrapper:
        // On Linux CSD: border + rounded corners on non-tiled edges + drop shadow when not tiled.
        // On Windows/macOS: rounded corners on non-tiled edges when configured, zero artificial shadow/border (OS DWM handles it).
        let inner_frame = div()
            .id("window-frame")
            .size_full()
            .map(|d| {
                if has_rounded_corners {
                    let mut frame = d
                        .when(!(tiling.top || tiling.left), |d| d.rounded_tl(radius))
                        .when(!(tiling.top || tiling.right), |d| d.rounded_tr(radius))
                        .when(!(tiling.bottom || tiling.left), |d| d.rounded_bl(radius))
                        .when(!(tiling.bottom || tiling.right), |d| d.rounded_br(radius))
                        .bg(content_bg)
                        .overflow_hidden();

                    if is_csd_shadow {
                        frame = frame
                            .border_color(border_color)
                            .when(!tiling.top, |d| d.border_t(BORDER_SIZE))
                            .when(!tiling.bottom, |d| d.border_b(BORDER_SIZE))
                            .when(!tiling.left, |d| d.border_l(BORDER_SIZE))
                            .when(!tiling.right, |d| d.border_r(BORDER_SIZE))
                            .when(!tiling.is_tiled(), |d| {
                                d.shadow(vec![BoxShadow {
                                    color: black().opacity(shadow_opacity),
                                    offset: point(px(0.0), px(0.0)),
                                    blur_radius: shadow_blur,
                                    spread_radius: px(0.0),
                                    inset: false,
                                }])
                            });
                    }
                    frame
                } else {
                    d.bg(content_bg)
                }
            })
            .when(is_csd_shadow, |d| {
                d.on_mouse_move(|_e, _window, cx| {
                    cx.stop_propagation();
                })
            })
            .child(self.view.clone());

        // Backdrop — transparent ring that hosts the shadow and the resize mouse handlers on Linux CSD.
        // On Windows and macOS, zero padding is applied so the frame perfectly matches the OS window.
        let mut root_div = div()
            .id("simple-root")
            .size_full()
            .when(use_custom_ui_font(cx), |div| div.font_family(ui_font_family(cx)));

        if is_csd_shadow {
            let resize_zone = if has_rounded_corners {
                SHADOW_SIZE
            } else {
                RESIZE_EDGE_SIZE
            };

            root_div = root_div
                .bg(transparent_black())
                .when(has_rounded_corners, |d| {
                    d.when(!tiling.top, |d| d.pt(SHADOW_SIZE))
                        .when(!tiling.bottom, |d| d.pb(SHADOW_SIZE))
                        .when(!tiling.left, |d| d.pl(SHADOW_SIZE))
                        .when(!tiling.right, |d| d.pr(SHADOW_SIZE))
                })
                .on_mouse_move({
                    let entity = entity.clone();
                    move |e, window, cx| {
                        let size = window.window_bounds().get_bounds().size;
                        let new_edge = resize_edge(e.position, resize_zone, size, tiling);
                        let old_edge = cx.try_global::<GlobalResizeEdge>().map(|e| e.0);
                        if new_edge != old_edge {
                            entity.update(cx, |_, cx| cx.notify());
                        }
                    }
                })
                .on_mouse_down(MouseButton::Left, move |e, window, _cx| {
                    let size = window.window_bounds().get_bounds().size;
                    let Some(edge) = resize_edge(e.position, resize_zone, size, tiling) else {
                        return;
                    };
                    window.start_window_resize(edge);
                });
        } else {
            root_div = root_div.bg(transparent_black());
        }

        root_div = root_div.child(inner_frame);

        if is_csd_shadow {
            let resize_zone = if has_rounded_corners {
                SHADOW_SIZE
            } else {
                RESIZE_EDGE_SIZE
            };

            root_div = root_div.child(
                canvas(
                    |_bounds, window, _cx| {
                        window.insert_hitbox(
                            Bounds::new(
                                point(px(0.0), px(0.0)),
                                window.window_bounds().get_bounds().size,
                            ),
                            HitboxBehavior::Normal,
                        )
                    },
                    move |_bounds, hitbox: Hitbox, window, cx| {
                        let mouse = window.mouse_position();
                        let size = window.window_bounds().get_bounds().size;
                        let Some(edge) = resize_edge(mouse, resize_zone, size, tiling) else {
                            return;
                        };
                        cx.set_global(GlobalResizeEdge(edge));
                        window.set_cursor_style(
                            match edge {
                                ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
                                ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
                                ResizeEdge::TopLeft | ResizeEdge::BottomRight => {
                                    CursorStyle::ResizeUpLeftDownRight
                                }
                                ResizeEdge::TopRight | ResizeEdge::BottomLeft => {
                                    CursorStyle::ResizeUpRightDownLeft
                                }
                            },
                            &hitbox,
                        );
                    },
                )
                .size_full()
                .absolute(),
            );
        }

        root_div
    }
}

/// Which resize edge (if any) the position is over. Ported from Zed's
/// `workspace::resize_edge`: corner zones are `1.5 * shadow_size` squares,
/// edge strips are `shadow_size` wide, and tiled edges are excluded.
fn resize_edge(
    pos: Point<Pixels>,
    shadow_size: Pixels,
    window_size: Size<Pixels>,
    tiling: Tiling,
) -> Option<ResizeEdge> {
    let bounds = Bounds::new(Point::default(), window_size).inset(shadow_size * 1.5);
    if bounds.contains(&pos) {
        return None;
    }

    let corner_size = size(shadow_size * 1.5, shadow_size * 1.5);
    let top_left_bounds = Bounds::new(Point::new(px(0.0), px(0.0)), corner_size);
    if !tiling.top && top_left_bounds.contains(&pos) {
        return Some(ResizeEdge::TopLeft);
    }

    let top_right_bounds = Bounds::new(
        Point::new(window_size.width - corner_size.width, px(0.0)),
        corner_size,
    );
    if !tiling.top && top_right_bounds.contains(&pos) {
        return Some(ResizeEdge::TopRight);
    }

    let bottom_left_bounds = Bounds::new(
        Point::new(px(0.0), window_size.height - corner_size.height),
        corner_size,
    );
    if !tiling.bottom && bottom_left_bounds.contains(&pos) {
        return Some(ResizeEdge::BottomLeft);
    }

    let bottom_right_bounds = Bounds::new(
        Point::new(
            window_size.width - corner_size.width,
            window_size.height - corner_size.height,
        ),
        corner_size,
    );
    if !tiling.bottom && bottom_right_bounds.contains(&pos) {
        return Some(ResizeEdge::BottomRight);
    }

    if !tiling.top && pos.y < shadow_size {
        Some(ResizeEdge::Top)
    } else if !tiling.bottom && pos.y > window_size.height - shadow_size {
        Some(ResizeEdge::Bottom)
    } else if !tiling.left && pos.x < shadow_size {
        Some(ResizeEdge::Left)
    } else if !tiling.right && pos.x > window_size.width - shadow_size {
        Some(ResizeEdge::Right)
    } else {
        None
    }
}
