use gpui::*;
use velowork_core::theme::ThemeColors;

/// Create an hsla color from a hex color with custom alpha.
pub fn with_alpha(hex: u32, alpha: f32) -> Hsla {
    let rgba = rgb(hex);
    Hsla::from(Rgba { a: alpha, ..rgba })
}

/// Global theme provider -- a function pointer that reads the current theme colors.
/// The host app registers this at startup; crate views call `theme()` to read colors.
pub struct GlobalThemeProvider(pub fn(&App) -> ThemeColors);

impl Global for GlobalThemeProvider {}

/// Get current theme colors.
/// Panics if `GlobalThemeProvider` has not been set by the host app.
pub fn theme(cx: &App) -> ThemeColors {
    (cx.global::<GlobalThemeProvider>().0)(cx)
}

/// Convert ANSI color to GPUI Hsla using a specific `TerminalPalette`.
pub fn ansi_to_hsla_palette(palette: &velowork_core::theme::color_schemes::TerminalPalette, color: &alacritty_terminal::vte::ansi::Color) -> Hsla {
    let argb = palette.ansi_to_argb(color);
    let r = ((argb >> 16) & 0xFF) as f32 / 255.0;
    let g = ((argb >> 8) & 0xFF) as f32 / 255.0;
    let b = (argb & 0xFF) as f32 / 255.0;
    Hsla::from(Rgba { r, g, b, a: 1.0 })
}

use crate::app_theme::GlobalTheme;

/// Current global background opacity (0.0 - 1.0), mirrored from the
/// `bg_opacity` system setting into `AppTheme.opacity`.
pub fn bg_opacity(cx: &App) -> f32 {
    cx.global::<GlobalTheme>().0.read(cx).opacity
}

/// Resolve a surface background color that respects the global background
/// opacity. Use this instead of `.bg(rgb(t.bg_xxx))` for any surface that
/// should become translucent when the user lowers transparency.
pub fn surface_bg(base: u32, cx: &App) -> Hsla {
    with_alpha(base, bg_opacity(cx))
}

/// Same as `surface_bg`, but enforces a minimum effective opacity. Use for
/// readability-critical surfaces (e.g. terminal content) so they never become
/// too transparent even at low global opacity settings.
pub fn surface_bg_min(base: u32, min_opacity: f32, cx: &App) -> Hsla {
    with_alpha(base, bg_opacity(cx).max(min_opacity))
}

/// Fallback helper for converting a raw hex color to `Hsla` without requiring `&App`.
/// Prefer `SemanticPalette` or `surface_bg(base, cx)` for proper opacity handling.
#[inline]
pub fn surface_bg_t(base: u32, _t: &ThemeColors) -> Hsla {
    with_alpha(base, 1.0)
}

// ----------------------------------------------------------------------------
// Readability helpers
// ----------------------------------------------------------------------------

/// Perceived luminance of an sRGB hex color (0.0 - 1.0), used for contrast math.
fn relative_luminance(hex: u32) -> f32 {
    let linear = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let r = linear(((hex >> 16) & 0xFF) as f32 / 255.0);
    let g = linear(((hex >> 8) & 0xFF) as f32 / 255.0);
    let b = linear((hex & 0xFF) as f32 / 255.0);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// WCAG contrast ratio between two sRGB hex colors (1.0 - 21.0).
pub fn contrast_ratio(fg: u32, bg: u32) -> f32 {
    let l1 = relative_luminance(fg);
    let l2 = relative_luminance(bg);
    let (light, dark) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (light + 0.05) / (dark + 0.05)
}

/// Linearly interpolate between two sRGB hex colors. `t` = 0 returns `a`,
/// `t` = 1 returns `b`.
fn mix_color(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let ar = (a >> 16) & 0xFF;
    let ag = (a >> 8) & 0xFF;
    let ab = a & 0xFF;
    let br = (b >> 16) & 0xFF;
    let bg = (b >> 8) & 0xFF;
    let bb = b & 0xFF;
    let r = (ar as f32 + (br as f32 - ar as f32) * t).round() as u32 & 0xFF;
    let g = (ag as f32 + (bg as f32 - ag as f32) * t).round() as u32 & 0xFF;
    let bl = (ab as f32 + (bb as f32 - ab as f32) * t).round() as u32 & 0xFF;
    (r << 16) | (g << 8) | bl
}

/// Whether a hex color reads as "dark" (used to pick the contrast extreme).
fn is_dark_color(hex: u32) -> bool {
    relative_luminance(hex) < 0.4
}

/// Return a text color that stays readable when painted on a (possibly
/// translucent) surface.
///
/// Because a translucent surface composites over an *unknown* backdrop
/// (the user's desktop), we cannot guarantee contrast against a specific
/// background. Instead we use a conservative, deterministic strategy:
///
/// 1. As global opacity drops, push the text color toward the theme's contrast
///    extreme (white on dark themes, black on light themes). This guarantees
///    the text remains solid and high-contrast regardless of what shows through.
/// 2. Enforce a WCAG AA floor (contrast ratio >= 4.5) against the *themed*
///    background at full opacity; if the boosted color still falls short, snap
///    fully to the extreme.
///
/// At `opacity == 1.0` the boost is zero, so text is unchanged from `fg`.
pub fn readable_text(bg: u32, fg: u32, cx: &App) -> Hsla {
    let op = bg_opacity(cx);
    let boost = (1.0 - op).clamp(0.0, 0.85);
    let extreme = if is_dark_color(bg) { 0xFFFFFF } else { 0x000000 };
    let mixed = mix_color(fg, extreme, boost);

    if contrast_ratio(mixed, bg) < 4.5 {
        with_alpha(extreme, 1.0)
    } else {
        with_alpha(mixed, 1.0)
    }
}

/// Convenience: resolve a readable default text color for a surface.
pub fn readable_text_on(bg: u32, cx: &App) -> Hsla {
    let fg = if is_dark_color(bg) { 0xE6E6E6 } else { 0x1A1A1A };
    readable_text(bg, fg, cx)
}
