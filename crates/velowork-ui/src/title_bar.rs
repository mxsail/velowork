//! Reusable TitleBar components for Velowork.

use crate::decorations::WindowControlStyle;
use crate::design::semantic::SemanticPalette;
use crate::icon::AppIcon;
use crate::theme::ThemeColors;
use crate::tokens::*;
use crate::tooltip::{Tooltip, TooltipDirection};
use gpui::prelude::*;
use gpui::*;
use velowork_i18n::i18n;

/// Window control button types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowControlType {
    Minimize,
    Maximize,
    Restore,
    Close,
}

impl WindowControlType {
    pub fn tooltip_key(self) -> &'static str {
        match self {
            WindowControlType::Minimize => "titlebar.minimize",
            WindowControlType::Maximize => "titlebar.maximize",
            WindowControlType::Restore => "titlebar.restore",
            WindowControlType::Close => "common.close",
        }
    }

    pub fn app_icon(self, style: WindowControlStyle) -> AppIcon {
        match style {
            WindowControlStyle::KDEBreeze => match self {
                WindowControlType::Minimize => AppIcon::WindowMinimizeBreeze,
                WindowControlType::Maximize => AppIcon::WindowMaximizeBreeze,
                WindowControlType::Restore => AppIcon::WindowRestoreBreeze,
                WindowControlType::Close => AppIcon::WindowCloseBreeze,
            },
            WindowControlStyle::Windows11 => match self {
                WindowControlType::Minimize => AppIcon::WindowMinimizeWin11,
                WindowControlType::Maximize => AppIcon::WindowMaximizeWin11,
                WindowControlType::Restore => AppIcon::WindowRestoreWin11,
                WindowControlType::Close => AppIcon::WindowCloseWin11,
            },
            WindowControlStyle::LinuxCSD | WindowControlStyle::MacOS => match self {
                WindowControlType::Minimize => AppIcon::WindowMinimizeCsd,
                WindowControlType::Maximize => AppIcon::WindowMaximizeCsd,
                WindowControlType::Restore => AppIcon::WindowRestoreCsd,
                WindowControlType::Close => AppIcon::WindowCloseCsd,
            },
        }
    }
}

/// Renders a single window control button (Minimize, Maximize, Restore, Close)
pub fn window_control_button(
    id: impl Into<ElementId>,
    control_type: WindowControlType,
    style: WindowControlStyle,
    t: &ThemeColors,
    cx: &mut App,
) -> Stateful<Div> {
    window_control_button_ex(id, control_type, style, None, t, cx)
}

/// Renders a single window control button with an optional custom icon size
pub fn window_control_button_ex(
    id: impl Into<ElementId>,
    control_type: WindowControlType,
    style: WindowControlStyle,
    custom_icon_size: Option<f32>,
    t: &ThemeColors,
    cx: &mut App,
) -> Stateful<Div> {
    let p = SemanticPalette::from_theme(t);
    let is_close = control_type == WindowControlType::Close;
    let tip_text = i18n!(cx, control_type.tooltip_key());

    let control_area = if cfg!(target_os = "windows") {
        Some(match control_type {
            WindowControlType::Minimize => WindowControlArea::Min,
            WindowControlType::Maximize | WindowControlType::Restore => WindowControlArea::Max,
            WindowControlType::Close => WindowControlArea::Close,
        })
    } else {
        None
    };

    let scale = ui_scale_factor(cx);

    let mut button = match style {
        WindowControlStyle::MacOS => {
            // macOS traffic light style
            let base_dot = custom_icon_size
                .unwrap_or_else(|| crate::decorations::get_window_control_icon_size(cx).min(16.0));
            let dot_size = px(base_dot * scale);
            let dot_color = match control_type {
                WindowControlType::Close => rgb(0xFF5F56),
                WindowControlType::Minimize => rgb(0xFFBD2E),
                WindowControlType::Maximize | WindowControlType::Restore => rgb(0x27C93F),
            };
            div()
                .id(id)
                .cursor_pointer()
                .w(dot_size)
                .h(dot_size)
                .rounded_full()
                .bg(dot_color)
                .flex()
                .items_center()
                .justify_center()
                .tooltip(move |_, cx| {
                    let tip = tip_text.clone();
                    cx.new(|_| Tooltip::new(tip).direction(TooltipDirection::Bottom))
                        .into()
                })
        }
        WindowControlStyle::Windows11
        | WindowControlStyle::LinuxCSD
        | WindowControlStyle::KDEBreeze => {
            let is_breeze = style == WindowControlStyle::KDEBreeze;
            let is_win11 = style == WindowControlStyle::Windows11;

            let (base_w, base_h) = style.button_size();
            let base_icon_sz = custom_icon_size
                .unwrap_or_else(|| crate::decorations::get_window_control_icon_size(cx));

            // Snap icon and button sizes to integer pixels to prevent subpixel rasterization blur
            let icon_f = (base_icon_sz * scale).round().max(8.0);

            let titlebar_h = crate::decorations::get_titlebar_height(cx);
            let (mut bw, mut bh) = if is_breeze {
                // KDE Breeze: button circle dynamically wraps the icon with a tight 1px margin halo
                let diameter = icon_f + 1.0;
                (diameter, diameter)
            } else if !is_win11 {
                // Linux CSD: rounded button adapts to icon size
                (
                    (f32::from(base_w) * scale).round().max(icon_f + 8.0),
                    (f32::from(base_h) * scale).round().max(icon_f + 8.0),
                )
            } else {
                // Windows 11: wide button adapting to titlebar height and icon size
                (
                    (f32::from(base_w) * scale).round().max(icon_f + 16.0),
                    titlebar_h.max(icon_f + 4.0),
                )
            };

            // Parity alignment: ensure (bw - icon_f) and (bh - icon_f) are even integers
            // for non-breeze styles so centering does not produce a 0.5px subpixel offset.
            if !is_breeze {
                if ((bw - icon_f) as i32) % 2 != 0 {
                    bw += 1.0;
                }
                if ((bh - icon_f) as i32) % 2 != 0 {
                    bh += 1.0;
                }
            }

            let icon_sz = px(icon_f);
            let btn_w = px(bw);
            let btn_h = px(bh);

            let app_icon = control_type.app_icon(style);
            let icon_elem = app_icon.size(icon_sz).text_color(p.text_primary);

            div()
                .id(id)
                .cursor_pointer()
                .w(btn_w)
                .h(btn_h)
                .flex()
                .items_center()
                .justify_center()
                .text_color(p.text_primary)
                .when(is_breeze, |d| d.rounded_full())
                .when(!is_win11 && !is_breeze, |d| {
                    d.rounded(px((6.0 * scale).round()))
                })
                .when(is_close, |d| {
                    d.hover(|s| s.bg(p.status_error).text_color(p.text_on_accent))
                })
                .when(!is_close, |d| {
                    d.hover(|s| s.bg(p.surface_hover).text_color(p.text_primary))
                })
                .child(icon_elem)
                .tooltip(move |_, cx| {
                    let tip = tip_text.clone();
                    cx.new(|_| Tooltip::new(tip).direction(TooltipDirection::Bottom))
                        .into()
                })
        }
    };

    if let Some(area) = control_area {
        button = button.occlude().window_control_area(area);
    }

    button
}

/// Renders a complete set of window control buttons according to `WindowDecorationConfig`.
/// Optionally handles custom close actions (e.g. re-attaching a detached dock panel).
pub fn render_window_controls(
    id_prefix: &str,
    window: &mut Window,
    config: &crate::decorations::WindowDecorationConfig,
    custom_icon_size: Option<f32>,
    custom_close: Option<std::sync::Arc<dyn Fn(&mut Window, &mut App)>>,
    t: &ThemeColors,
    cx: &mut App,
) -> impl IntoElement + use<> {
    let style = config.style;
    let gap = config.effective_gap(cx);
    let is_maximized = window.is_maximized();
    let is_fullscreen = window.is_fullscreen();

    let scale = ui_scale_factor(cx);
    let corner_radius_f = crate::decorations::get_window_corner_radius(cx);
    let has_rounded_corners = !is_maximized && !is_fullscreen && corner_radius_f > 0.0;
    let corner_radius = px((corner_radius_f * scale).round());

    let margin_val = config
        .custom_margin
        .unwrap_or_else(|| crate::decorations::get_window_control_margin(cx));
    let is_zero_margin = margin_val <= 0.5;
    let is_win11 = style == crate::decorations::WindowControlStyle::Windows11;

    let mut bar = crate::h_flex().gap(gap);
    let num_buttons = config.buttons.len();

    for (idx, &btn_type) in config.buttons.iter().enumerate() {
        let actual_type = match btn_type {
            WindowControlType::Maximize if is_maximized => WindowControlType::Restore,
            other => other,
        };

        let btn_id = ElementId::Name(format!("{}-{:?}-{}", id_prefix, actual_type, idx).into());
        let mut control_btn =
            window_control_button_ex(btn_id, actual_type, style, custom_icon_size, t, cx);

        // When margin is zero and the window has rounded corners, adapt outermost corner button
        // ONLY for Windows 11 full-bleed rectangular buttons to avoid deforming circular Breeze/macOS buttons.
        if is_win11 && is_zero_margin && has_rounded_corners {
            if config.position == crate::decorations::WindowButtonPosition::Left && idx == 0 {
                control_btn = control_btn.rounded_tl(corner_radius);
            } else if config.position == crate::decorations::WindowButtonPosition::Right
                && idx == num_buttons.saturating_sub(1)
            {
                control_btn = control_btn.rounded_tr(corner_radius);
            }
        }

        let custom_close = custom_close.clone();
        let bound_btn = control_btn
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                match actual_type {
                    WindowControlType::Minimize => window.minimize_window(),
                    WindowControlType::Maximize | WindowControlType::Restore => {
                        window.zoom_window();
                    }
                    WindowControlType::Close => {
                        if let Some(close_fn) = &custom_close {
                            close_fn(window, cx);
                        } else {
                            window.remove_window();
                        }
                    }
                }
            });

        bar = bar.child(bound_btn);
    }

    bar
}

/// Renders hamburger menu button (`☰`) with tooltip
pub fn hamburger_button(
    id: impl Into<ElementId>,
    is_active: bool,
    t: &ThemeColors,
    cx: &mut App,
) -> Stateful<Div> {
    let p = SemanticPalette::from_theme(t);
    let tip_text = i18n!(cx, "titlebar.menu_tooltip");

    div()
        .id(id)
        .cursor_pointer()
        .px(SPACE_MD)
        .py(SPACE_XS)
        .rounded(RADIUS_STD)
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .when(is_active, |d| d.bg(rgb(t.bg_hover)))
        .text_size(ui_text_xl(cx))
        .text_color(if is_active {
            rgb(t.accent).into()
        } else {
            p.text_secondary
        })
        .child("☰")
        .tooltip(move |_, cx| {
            let tip = tip_text.clone();
            cx.new(|_| Tooltip::new(tip).direction(TooltipDirection::Bottom))
                .into()
        })
}

/// Renders a top-level menu button (`File`, `Edit`, `View`, etc.)
pub fn top_menu_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    is_active: bool,
    t: &ThemeColors,
    cx: &mut App,
) -> Stateful<Div> {
    let p = SemanticPalette::from_theme(t);
    let label_str = label.into();
    div()
        .id(id)
        .cursor_pointer()
        .px(SPACE_MD)
        .py(SPACE_XS)
        .rounded(RADIUS_STD)
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .when(is_active, |d| d.bg(rgb(t.bg_hover)))
        .text_size(ui_text_sm(cx))
        .font_weight(if is_active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        })
        .text_color(if is_active {
            p.text_primary
        } else {
            p.text_secondary
        })
        .child(label_str)
}
