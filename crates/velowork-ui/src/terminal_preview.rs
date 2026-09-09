//! Universal Terminal Preview Card Component.
//!
//! Renders a high-fidelity mini preview card of terminal contents when hovering
//! over terminal tabs or minimized capsules.

use gpui::prelude::*;
use gpui::*;
use velowork_core::terminal_preview::TerminalPreviewSnapshot;
use velowork_core::theme::DARK_PALETTE;
use velowork_i18n::i18n;

use crate::design::semantic::SemanticPalette;
use crate::icon::AppIcon;
use crate::theme::{ansi_to_hsla_palette, surface_bg_t, theme, with_alpha};
use crate::tokens::{
    ICON_LG, RADIUS_MD, RADIUS_XS, SPACE_2XS, SPACE_MD, SPACE_SM, SPACE_XS,
    mono_font_family, ui_icon_std_ts, ui_text_md, ui_text_sm, ui_text_xs,
};
use crate::{h_flex, v_flex};

/// Properties for constructing a terminal preview card.
#[derive(Clone)]
pub struct TerminalPreviewProps {
    pub title: String,
    pub icon: AppIcon,
    pub icon_color: Option<Hsla>,
    pub is_remote: bool,
    pub status_text: Option<String>,
    pub idle_text: Option<String>,
    pub is_disconnected: bool,
    pub is_welcome: bool,
    pub connection_info: Option<String>,
    pub protocol_badge: Option<String>,
    pub snapshot: Option<TerminalPreviewSnapshot>,
    pub font_family: Option<String>,
    pub palette: Option<velowork_core::theme::TerminalPalette>,
    pub background_builder: Option<std::sync::Arc<dyn Fn(&App) -> Option<AnyElement> + Send + Sync>>,
}

/// Construct a terminal preview card element.
pub fn terminal_preview_card(props: TerminalPreviewProps, cx: &mut App) -> impl IntoElement {
    let t = theme(cx);
    let p = SemanticPalette::from_theme(&t);

    let palette = props.palette.unwrap_or(DARK_PALETTE);

    let is_empty = props.snapshot.as_ref().map_or(true, |s| s.is_empty);

    let (card_width, viewport_height, content_aspect) = if let Some(ref snapshot) = props.snapshot {
        let total_cols = (snapshot.cols.max(1)) as f32;
        let total_rows = (snapshot.rows.max(snapshot.lines.len()).max(1)) as f32;
        let cell_aspect = 0.52f32; // cell_w / cell_h
        let aspect = (total_cols * cell_aspect) / total_rows;

        let base_h = 220.0f32;
        let raw_w = base_h * aspect;
        let clamped_w = raw_w.clamp(320.0, 520.0);
        let clamped_h = (clamped_w / aspect).clamp(180.0, 300.0);
        (px(clamped_w), px(clamped_h), aspect)
    } else {
        (px(440.0), px(220.0), 2.0f32)
    };

    let font_family_str = props
        .font_family
        .filter(|f| !f.is_empty() && f != "Auto" && f != "System Default")
        .unwrap_or_else(|| mono_font_family(cx));
    let font_family: SharedString = font_family_str.into();

    #[cfg(target_os = "macos")]
    let fallbacks = Some(FontFallbacks::from_fonts(vec![
        "JetBrains Mono".into(),
        "Menlo".into(),
        "SF Mono".into(),
        "Monaco".into(),
    ]));

    #[cfg(not(target_os = "macos"))]
    let fallbacks = Some(FontFallbacks::from_fonts(vec![
        "JetBrains Mono".into(),
        "DejaVu Sans Mono".into(),
        "Liberation Mono".into(),
        "Ubuntu Mono".into(),
        "Noto Sans Mono".into(),
        "monospace".into(),
    ]));

    let font = Font {
        family: font_family.clone(),
        features: FontFeatures::disable_ligatures(),
        fallbacks: fallbacks.clone(),
        weight: FontWeight::NORMAL,
        style: FontStyle::Normal,
    };
    let font_bold = Font {
        family: font_family.clone(),
        features: FontFeatures::disable_ligatures(),
        fallbacks: fallbacks.clone(),
        weight: FontWeight::BOLD,
        style: FontStyle::Normal,
    };
    let font_italic = Font {
        family: font_family.clone(),
        features: FontFeatures::disable_ligatures(),
        fallbacks: fallbacks.clone(),
        weight: FontWeight::NORMAL,
        style: FontStyle::Italic,
    };
    let font_bold_italic = Font {
        family: font_family.clone(),
        features: FontFeatures::disable_ligatures(),
        fallbacks,
        weight: FontWeight::BOLD,
        style: FontStyle::Italic,
    };

    v_flex()
        .w(card_width)
        .max_w(px(560.0))
        .rounded(RADIUS_MD)
        .bg(surface_bg_t(t.bg_panel, &t))
        .border_1()
        .border_color(rgb(t.border))
        .shadow(crate::tokens::elevation_menu_shadow())
        .overflow_hidden()
        // ── Card Window Header ───────────────────────────────────────
        .child(
            v_flex()
                .w_full()
                .px(SPACE_MD)
                .py(SPACE_SM)
                .gap(SPACE_2XS)
                .rounded_t(RADIUS_MD)
                .bg(surface_bg_t(t.bg_header, &t))
                .border_b_1()
                .border_color(p.border_subtle)
                // Top row: Icon + Title + Status Badges
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .gap(SPACE_SM)
                        .child(
                            h_flex()
                                .items_center()
                                .gap(SPACE_XS)
                                .flex_1()
                                .min_w_0()
                                .child(
                                    props
                                        .icon
                                        .size(ui_icon_std_ts(cx))
                                        .flex_shrink_0()
                                        .text_color(props.icon_color.unwrap_or(p.text_secondary)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(ui_text_md(cx))
                                        .text_color(rgb(t.text_primary))
                                        .child(props.title),
                                ),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap(SPACE_XS)
                                .flex_shrink_0()
                                .when_some(props.idle_text.as_ref(), |d, idle| {
                                    d.child(
                                        h_flex()
                                            .items_center()
                                            .px(SPACE_XS)
                                            .py(px(1.0))
                                            .rounded(RADIUS_XS)
                                            .bg(with_alpha(t.warning, 0.12))
                                            .border_1()
                                            .border_color(with_alpha(t.warning, 0.30))
                                            .text_size(ui_text_xs(cx))
                                            .text_color(rgb(t.warning))
                                            .child(format!("Idle: {}", idle)),
                                    )
                                })
                                .when(props.is_disconnected, |d| {
                                    d.child(
                                        h_flex()
                                            .items_center()
                                            .gap(SPACE_2XS)
                                            .px(SPACE_XS)
                                            .py(px(1.0))
                                            .rounded(RADIUS_XS)
                                            .bg(with_alpha(t.error, 0.15))
                                            .border_1()
                                            .border_color(with_alpha(t.error, 0.35))
                                            .text_color(rgb(t.error))
                                            .text_size(ui_text_xs(cx))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(
                                                div().size(px(6.0)).rounded_full().bg(rgb(t.error)),
                                            )
                                            .child(i18n!(cx, "status.connection_lost")),
                                    )
                                })
                                .when(!props.is_disconnected, |d| {
                                    let badge_str = props.protocol_badge.as_deref().unwrap_or(
                                        if props.is_welcome {
                                            "Welcome"
                                        } else if props.is_remote {
                                            "SSH"
                                        } else {
                                            "Local"
                                        }
                                    );
                                    let (bg_color, border_color, text_color, dot_color) = match badge_str {
                                        "SSH" => (
                                            Hsla { a: 0.15, ..p.status_success },
                                            Hsla { a: 0.35, ..p.status_success },
                                            p.status_success,
                                            p.status_success,
                                        ),
                                        "Serial" => (
                                            Hsla { a: 0.15, ..p.status_warning },
                                            Hsla { a: 0.35, ..p.status_warning },
                                            p.status_warning,
                                            p.status_warning,
                                        ),
                                        "Telnet" => (
                                            Hsla { a: 0.15, ..p.status_info },
                                            Hsla { a: 0.35, ..p.status_info },
                                            p.status_info,
                                            p.status_info,
                                        ),
                                        "Welcome" | "新标签页" => (
                                            with_alpha(t.accent, 0.15),
                                            with_alpha(t.accent, 0.35),
                                            with_alpha(t.accent, 1.0),
                                            with_alpha(t.accent, 1.0),
                                        ),
                                        _ => (
                                            with_alpha(t.text_primary, 0.08),
                                            with_alpha(t.text_primary, 0.18),
                                            p.text_secondary,
                                            p.status_success,
                                        ),
                                    };
                                    d.child(
                                        h_flex()
                                            .items_center()
                                            .gap(SPACE_2XS)
                                            .px(SPACE_XS)
                                            .py(px(1.0))
                                            .rounded(RADIUS_XS)
                                            .bg(bg_color)
                                            .border_1()
                                            .border_color(border_color)
                                            .text_color(text_color)
                                            .text_size(ui_text_xs(cx))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(
                                                div().size(px(6.0)).rounded_full().bg(dot_color),
                                            )
                                            .child(badge_str.to_string()),
                                    )
                                }),
                        ),
                )
                // Sub row: Detailed Connection Info (e.g. user@host:port)
                .when_some(props.connection_info.clone(), |d, info| {
                    d.child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .text_size(ui_text_xs(cx))
                            .text_color(p.text_muted)
                            .child(div().flex_1().min_w_0().truncate().child(info)),
                    )
                }),
        )
        // ── Card Screen Body (Terminal Viewport) ──────────────────────
        .child({
            let bg_layer = props
                .background_builder
                .as_ref()
                .and_then(|builder| builder(cx));
            let has_bg_layer = bg_layer.is_some();
            let bg_color = if props.is_welcome {
                surface_bg_t(t.bg_panel, &t)
            } else if has_bg_layer {
                transparent_black()
            } else {
                surface_bg_t(palette.background, &t)
            };

            let mut screen_body = div()
                .w_full()
                .h(viewport_height)
                .rounded_b(RADIUS_MD)
                .relative()
                .bg(bg_color)
                .overflow_hidden();

            if let Some(layer) = bg_layer {
                screen_body = screen_body.child(layer);
            }

            screen_body = screen_body.child(if props.is_welcome {
                    v_flex()
                        .size_full()
                        .p(SPACE_MD)
                        .gap(SPACE_SM)
                        .justify_between()
                        .child(
                            h_flex()
                                .items_center()
                                .gap(SPACE_SM)
                                .child(
                                    div()
                                        .size(px(28.0))
                                        .rounded(RADIUS_MD)
                                        .bg(with_alpha(t.accent, 0.15))
                                        .border_1()
                                        .border_color(with_alpha(t.accent, 0.30))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(AppIcon::AiAssistant.size(px(15.0)).text_color(rgb(t.accent))),
                                )
                                .child(
                                    v_flex()
                                        .gap(px(1.0))
                                        .child(
                                            div()
                                                .text_size(ui_text_sm(cx))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(p.text_primary)
                                                .child(i18n!(cx, "welcome.title")),
                                        )
                                        .child(
                                            div()
                                                .text_size(ui_text_xs(cx))
                                                .text_color(p.text_muted)
                                                .truncate()
                                                .child(i18n!(cx, "welcome.subtitle")),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .grid_cols(2)
                                .gap(SPACE_XS)
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap(SPACE_XS)
                                        .px(SPACE_SM)
                                        .py(SPACE_XS)
                                        .rounded(RADIUS_MD)
                                        .bg(with_alpha(t.bg_header, 0.7))
                                        .border_1()
                                        .border_color(p.border_subtle)
                                        .child(AppIcon::Terminal.size(px(13.0)).text_color(rgb(t.accent)))
                                        .child(
                                            div()
                                                .text_size(ui_text_xs(cx))
                                                .text_color(p.text_secondary)
                                                .truncate()
                                                .child(i18n!(cx, "welcome.start_terminal")),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap(SPACE_XS)
                                        .px(SPACE_SM)
                                        .py(SPACE_XS)
                                        .rounded(RADIUS_MD)
                                        .bg(with_alpha(t.bg_header, 0.7))
                                        .border_1()
                                        .border_color(p.border_subtle)
                                        .child(AppIcon::Server.size(px(13.0)).text_color(rgb(t.success)))
                                        .child(
                                            div()
                                                .text_size(ui_text_xs(cx))
                                                .text_color(p.text_secondary)
                                                .truncate()
                                                .child(i18n!(cx, "session.new_session")),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap(SPACE_XS)
                                        .px(SPACE_SM)
                                        .py(SPACE_XS)
                                        .rounded(RADIUS_MD)
                                        .bg(with_alpha(t.bg_header, 0.7))
                                        .border_1()
                                        .border_color(p.border_subtle)
                                        .child(AppIcon::Terminal.size(px(13.0)).text_color(rgb(t.warning)))
                                        .child(
                                            div()
                                                .text_size(ui_text_xs(cx))
                                                .text_color(p.text_secondary)
                                                .truncate()
                                                .child(i18n!(cx, "welcome.quick_commands")),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap(SPACE_XS)
                                        .px(SPACE_SM)
                                        .py(SPACE_XS)
                                        .rounded(RADIUS_MD)
                                        .bg(with_alpha(t.bg_header, 0.7))
                                        .border_1()
                                        .border_color(p.border_subtle)
                                        .child(AppIcon::AiAssistant.size(px(13.0)).text_color(rgb(t.accent)))
                                        .child(
                                            div()
                                                .text_size(ui_text_xs(cx))
                                                .text_color(p.text_secondary)
                                                .truncate()
                                                .child(i18n!(cx, "welcome.ai_assistant")),
                                        ),
                                ),
                        )
                        .into_any_element()
                } else if props.is_disconnected && is_empty {
                    v_flex()
                        .size_full()
                        .items_center()
                        .justify_center()
                        .gap(SPACE_SM)
                        .py(SPACE_MD)
                        .text_color(p.status_error)
                        .child(AppIcon::Unlink.size(ICON_LG).text_color(p.status_error))
                        .child(
                            div()
                                .text_size(ui_text_sm(cx))
                                .child(i18n!(cx, "terminal.preview_disconnected")),
                        )
                        .into_any_element()
                } else if is_empty {
                    v_flex()
                        .size_full()
                        .items_center()
                        .justify_center()
                        .gap(SPACE_SM)
                        .py(SPACE_MD)
                        .text_color(p.text_muted)
                        .child(AppIcon::Terminal.size(ICON_LG).text_color(p.text_muted))
                        .child(
                            div()
                                .text_size(ui_text_sm(cx))
                                .child(i18n!(cx, "terminal.preview_no_output")),
                        )
                        .into_any_element()
                } else {
                    let snapshot = props.snapshot.unwrap_or_default();
                    let palette = palette;

                    let font = font.clone();
                    let font_bold = font_bold.clone();
                    let font_italic = font_italic.clone();
                    let font_bold_italic = font_bold_italic.clone();

                    canvas(
                        |_bounds, _window, _cx| {},
                        move |bounds: Bounds<Pixels>, _, window: &mut Window, cx: &mut App| {
                            // Apply strict content mask to canvas bounds to prevent any drawing from overflowing
                            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                                let total_cols = (snapshot.cols.max(1)) as f32;
                                let total_rows =
                                    (snapshot.rows.max(snapshot.lines.len()).max(1)) as f32;

                                let pad_inset = 6.0;
                                let avail_w =
                                    (f32::from(bounds.size.width) - pad_inset * 2.0).max(10.0);
                                let avail_h =
                                    (f32::from(bounds.size.height) - pad_inset * 2.0).max(10.0);
                                let container_aspect = avail_w / avail_h;

                                let (final_w, final_h) = if content_aspect > container_aspect {
                                    let w = avail_w;
                                    let h = (w / content_aspect).min(avail_h);
                                    (w, h)
                                } else {
                                    let h = avail_h;
                                    let w = (h * content_aspect).min(avail_w);
                                    (w, h)
                                };

                                let pad_x = px((f32::from(bounds.size.width) - final_w) * 0.5);
                                let pad_y = px((f32::from(bounds.size.height) - final_h) * 0.5);

                                let cell_w = px(final_w / total_cols);
                                let cell_h = px(final_h / total_rows);

                                // Monospace font character advance is ~0.6 * font_size.
                                // Ensure font_size is proportionally bounded by cell_w and cell_h.
                                let font_size_w = f32::from(cell_w) * 1.55;
                                let font_size_h = f32::from(cell_h) * 0.80;
                                let font_size = px(font_size_w.min(font_size_h).max(3.5));

                                let origin =
                                    point(bounds.origin.x + pad_x, bounds.origin.y + pad_y);
                                let max_x = bounds.origin.x + bounds.size.width - px(4.0);
                                let max_y = bounds.origin.y + bounds.size.height - px(4.0);

                                for (r, line) in snapshot.lines.iter().enumerate() {
                                    let line_y = origin.y + r as f32 * cell_h;
                                    if line_y + cell_h > max_y {
                                        break;
                                    }

                                    let mut col_offset = 0usize;

                                    for span in &line.spans {
                                        let char_count = span.text.chars().count();
                                        let span_x = origin.x + col_offset as f32 * cell_w;
                                        let span_w = char_count as f32 * cell_w;

                                        if span_x >= max_x {
                                            break;
                                        }

                                        // 2. Paint background highlight rect
                                        if let Some(bg) = span.bg {
                                            let bg_color = ansi_to_hsla_palette(&palette, &bg);
                                            let clamped_w = span_w.min(max_x - span_x);
                                            let rect = Bounds::new(
                                                point(span_x, line_y),
                                                size(clamped_w, cell_h),
                                            );
                                            window.paint_quad(fill(rect, bg_color));
                                        }

                                        // 3. Paint REAL crisp glyph text runs with fixed-pitch spacing
                                        let fg_color = span
                                            .fg
                                            .map(|c| ansi_to_hsla_palette(&palette, &c))
                                            .unwrap_or_else(|| rgb(palette.foreground).into());

                                        let span_font = if span.bold && span.italic {
                                            font_bold_italic.clone()
                                        } else if span.bold {
                                            font_bold.clone()
                                        } else if span.italic {
                                            font_italic.clone()
                                        } else {
                                            font.clone()
                                        };

                                        let run_style = TextRun {
                                            len: span.text.len(),
                                            font: span_font,
                                            color: fg_color,
                                            background_color: None,
                                            underline: if span.underline {
                                                Some(UnderlineStyle {
                                                    color: Some(fg_color),
                                                    thickness: px(1.0),
                                                    wavy: false,
                                                })
                                            } else {
                                                None
                                            },
                                            strikethrough: None,
                                        };

                                        let _ = window
                                            .text_system()
                                            .shape_line(
                                                span.text.as_str().into(),
                                                font_size,
                                                &[run_style],
                                                Some(cell_w),
                                            )
                                            .paint(
                                                point(span_x, line_y),
                                                cell_h,
                                                TextAlign::Left,
                                                None,
                                                window,
                                                cx,
                                            );

                                        col_offset += char_count;
                                    }
                                }
                            });
                        },
                    )
                    .size_full()
                    .into_any_element()
                });
            screen_body
        })
}
