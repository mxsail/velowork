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
#[allow(dead_code)]
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
    /// Last global background opacity seen, so we only touch the compositor
    /// when transparency actually changes.
    last_bg_opacity: f32,
    /// Last rounded-corner state seen. Tracked separately from `last_bg_opacity`
    /// so toggling rounded corners (or maximizing the window) re-applies the
    /// correct `WindowBackgroundAppearance` instead of getting stuck on a
    /// previously set `Transparent` surface.
    last_has_rounded_corners: bool,
    /// Last corner radius published to `WindowCornerRadius` so `modal_backdrop`
    /// can clip its dimming mask to the rounded window corners.
    last_modal_corner_radius: f32,
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
            last_bg_opacity: -1.0,
            last_has_rounded_corners: false,
            last_modal_corner_radius: -1.0,
        }
    }
}

impl Render for SimpleRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let decorations = window.window_decorations();
        let t = theme(cx);

        let is_custom_titlebar = if cfg!(target_os = "macos") {
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

        // Like Zed: tell gpui how far the visible frame is inset from the
        // surface edge. On Wayland this shrinks the xdg window geometry (and
        // the opaque region) to the inner frame so the compositor aligns
        // snapping/maximizing/effects with the rounded frame instead of the
        // rectangular surface.
        match decorations {
            Decorations::Client { .. } if has_rounded_corners => {
                window.set_client_inset(SHADOW_SIZE)
            }
            _ => window.set_client_inset(px(0.0)),
        }

        // Keep the compositor's backdrop blur/transparency in sync.
        // When custom titlebar with rounded corners (or transparency) is active,
        // we set WindowBackgroundAppearance to Transparent or Blurred so the OS
        // window compositor renders smooth alpha-clipped corners without sharp dark tips.
        let opacity = bg_opacity(cx);
        let target_appearance = if opacity < 1.0 {
            WindowBackgroundAppearance::Blurred
        } else if has_rounded_corners {
            WindowBackgroundAppearance::Transparent
        } else {
            WindowBackgroundAppearance::Opaque
        };

        if self.last_bg_opacity != opacity || self.last_has_rounded_corners != has_rounded_corners {
            window.set_background_appearance(target_appearance);
            self.last_bg_opacity = opacity;
            self.last_has_rounded_corners = has_rounded_corners;
        }

        let content_bg = if opacity >= 1.0 {
            with_alpha(t.bg_primary, 1.0)
        } else {
            with_alpha(t.bg_primary, 0.0)
        };

        // Resize detection zone: the shadow ring when present, otherwise a thin
        // strip just inside the window edge.
        let resize_zone = if has_rounded_corners {
            SHADOW_SIZE
        } else {
            RESIZE_EDGE_SIZE
        };

        let radius = px(window_corner_radius);
        let entity = cx.entity();

        // Inner frame — Zed's content wrapper: border + rounded corners on
        // non-tiled edges, drop shadow only when not tiled at all.
        let inner_frame = div()
            .id("window-frame")
            .size_full()
            .map(|d| {
                if has_rounded_corners {
                    d.when(!(tiling.top || tiling.left), |d| d.rounded_tl(radius))
                        .when(!(tiling.top || tiling.right), |d| d.rounded_tr(radius))
                        .when(!(tiling.bottom || tiling.left), |d| d.rounded_bl(radius))
                        .when(!(tiling.bottom || tiling.right), |d| d.rounded_br(radius))
                        .when(!tiling.is_tiled(), |d| {
                            // Same shadow as Zed: centered, blur = ring / 2, so
                            // it always fits inside the transparent ring. GPUI
                            // paints drop shadows with the element's corner
                            // radii, so it follows `window_corner_radius`.
                            d.shadow(vec![BoxShadow {
                                color: black().opacity(0.4),
                                offset: point(px(0.0), px(0.0)),
                                blur_radius: SHADOW_SIZE / 2.0,
                                spread_radius: px(0.0),
                                inset: false,
                            }])
                        })
                        .bg(content_bg)
                        .overflow_hidden()
                } else {
                    d.bg(content_bg)
                }
            })
            .on_mouse_move(|_e, _window, cx| {
                // Content area: stop propagation so the backdrop's resize-edge
                // tracking only reacts inside the shadow ring (Zed does the same).
                cx.stop_propagation();
            })
            .child(self.view.clone());

        // Backdrop — Zed's `window-backdrop`: transparent ring that hosts the
        // shadow and the resize mouse handlers.
        div()
            .id("simple-root")
            .size_full()
            // Apply a custom UI font family only when one is explicitly selected;
            // "System" (the default) lets GPUI use the native OS UI font.
            .when(use_custom_ui_font(cx), |div| div.font_family(ui_font_family(cx)))
            .map(|d| match decorations {
                Decorations::Server => {
                    if has_rounded_corners {
                        d.bg(transparent_black())
                    } else {
                        d.bg(content_bg)
                    }
                }
                Decorations::Client { .. } => d
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
                                // Re-render so the cursor-style canvas below picks
                                // up the new hover edge (same as Zed's notify).
                                entity.update(cx, |_, cx| cx.notify());
                            }
                        }
                    })
                    .on_mouse_down(MouseButton::Left, move |e, window, _cx| {
                        let size = window.window_bounds().get_bounds().size;
                        let Some(edge) = resize_edge(e.position, resize_zone, size, tiling)
                        else {
                            return;
                        };
                        window.start_window_resize(edge);
                    }),
            })
            .child(inner_frame)
            // Cursor-style canvas — Zed's third layer: a full-window hitbox that
            // switches the cursor while hovering a resize edge.
            .when(matches!(decorations, Decorations::Client { .. }), |d| {
                d.child(
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
                            let Some(edge) = resize_edge(mouse, resize_zone, size, tiling)
                            else {
                                return;
                            };
                            cx.set_global(GlobalResizeEdge(edge));
                            window.set_cursor_style(
                                match edge {
                                    ResizeEdge::Top | ResizeEdge::Bottom => {
                                        CursorStyle::ResizeUpDown
                                    }
                                    ResizeEdge::Left | ResizeEdge::Right => {
                                        CursorStyle::ResizeLeftRight
                                    }
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
                )
            })
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
