//! Semantic color palette.
//!
//! Bridges the raw `ThemeColors` (hex u32 values in `velowork-core`) into
//! GPUI-native `Hsla` tokens grouped by *purpose* — surfaces, accents, text,
//! borders, and status indicators. The palette is the single source of truth
//! for any code that resolves visual appearance.

use gpui::{App, Hsla};
use crate::theme::{ThemeColors, bg_opacity, theme, with_alpha};

/// Helper: convert a `u32` hex color to `Hsla`.
fn to_hsla(color: u32) -> Hsla {
    gpui::rgb(color).into()
}

/// Helper: transparent Hsla (fully invisible).
fn transparent() -> Hsla {
    gpui::hsla(0.0, 0.0, 0.0, 0.0)
}

/// Semantic color palette derived from a concrete `ThemeColors`.
///
/// Every field here has a clear role name so component code never reaches
/// back into the raw hex palette.
#[derive(Debug, Clone, Copy)]
pub struct SemanticPalette {
    // -- Surfaces --
    /// Base surface (e.g. the editor/terminal background).
    pub surface_base: Hsla,
    /// Raised surface (cards, secondary panels).
    pub surface_raised: Hsla,
    /// Card surface (subtle foreground-tinted surface overlay for frosted-glass card containers).
    pub surface_card: Hsla,
    /// Header surface (title bar, tab bar).
    pub surface_header: Hsla,
    /// Overlay surface (menus, dropdowns, modals, popovers).
    pub surface_overlay: Hsla,
    /// Hover state surface.
    pub surface_hover: Hsla,
    /// Selection state surface (list item selection).
    pub surface_selection: Hsla,
    /// Accent surface (primary action backgrounds).
    pub surface_accent: Hsla,
    /// Accent hover surface.
    pub surface_accent_hover: Hsla,
    /// Danger surface (destructive action backgrounds).
    pub surface_danger: Hsla,

    // -- Text --
    /// Primary text (headings, main labels).
    pub text_primary: Hsla,
    /// Secondary text (descriptions, hints).
    pub text_secondary: Hsla,
    /// Muted text (placeholders, disabled labels).
    pub text_muted: Hsla,
    /// Text on accent surfaces (buttons, badges).
    pub text_on_accent: Hsla,

    // -- Borders --
    /// Subtle border (cards, secondary inputs).
    pub border_subtle: Hsla,
    /// Active / focused border.
    pub border_active: Hsla,
    /// Transparent border placeholder.
    pub border_transparent: Hsla,

    // -- Editor (text-editing surfaces: inputs, terminals, viewers) --
    /// Text cursor / caret.
    pub editor_cursor: Hsla,
    /// Selected-text background.
    pub editor_selection: Hsla,
    /// Search match highlight background.
    pub editor_search_match: Hsla,
    /// Current (focused) search match background.
    pub editor_search_current: Hsla,
    /// IME composition (marked text) background.
    pub editor_ime_marked: Hsla,
    /// Inline variable / placeholder token highlight (e.g. `{var}`).
    pub editor_variable: Hsla,

    // -- Status / Severity --
    /// Success / healthy / running indicator.
    pub status_success: Hsla,
    /// Warning / degraded / paused indicator.
    pub status_warning: Hsla,
    /// Error / failed / stopped indicator.
    pub status_error: Hsla,
    /// Informational / in-progress (e.g. syncing) indicator.
    pub status_info: Hsla,
}

impl SemanticPalette {
    /// Derives a semantic palette from the given `ThemeColors` with custom background opacity.
    pub fn from_theme_with_opacity(t: &ThemeColors, opacity: f32) -> Self {
        let accent_alpha = (0.85 + 0.15 * opacity) * opacity;
        let surface_accent = with_alpha(t.accent, accent_alpha);
        let accent_hover = Hsla {
            l: if surface_accent.l > 0.5 {
                (surface_accent.l * 0.9).clamp(0.0, 1.0)
            } else {
                (surface_accent.l * 1.15).clamp(0.0, 1.0)
            },
            ..surface_accent
        };

        // Text on accent: high contrast foreground (white on dark accent, near-black on light accent)
        let is_dark_accent = {
            let r = ((t.accent >> 16) & 0xFF) as f32 / 255.0;
            let g = ((t.accent >> 8) & 0xFF) as f32 / 255.0;
            let b = (t.accent & 0xFF) as f32 / 255.0;
            let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            lum < 0.45
        };
        let text_on_accent = if is_dark_accent {
            to_hsla(0xFFFFFF)
        } else {
            to_hsla(0x1A1A1A)
        };

        Self {
            surface_base: with_alpha(t.bg_primary, opacity),
            surface_raised: with_alpha(t.bg_secondary, opacity),
            surface_card: with_alpha(t.text_primary, 0.035),
            surface_header: with_alpha(t.bg_header, opacity),
            surface_overlay: with_alpha(t.bg_primary, opacity),
            surface_hover: with_alpha(t.bg_hover, opacity),
            surface_selection: with_alpha(t.bg_selection, opacity),
            surface_accent,
            surface_accent_hover: accent_hover,
            surface_danger: with_alpha(t.error, accent_alpha),
            text_primary: to_hsla(t.text_primary),
            text_secondary: to_hsla(t.text_secondary),
            text_muted: to_hsla(t.text_muted),
            text_on_accent,
            border_subtle: with_alpha(t.border, 0.5),
            border_active: to_hsla(t.border_active),
            border_transparent: transparent(),
            editor_cursor: to_hsla(t.text_primary),
            editor_selection: to_hsla(t.bg_selection),
            editor_search_match: with_alpha(t.accent, 0.4),
            editor_search_current: surface_accent,
            editor_ime_marked: to_hsla(t.bg_selection),
            editor_variable: surface_accent,
            status_success: to_hsla(t.success),
            status_warning: to_hsla(t.warning),
            status_error: to_hsla(t.error),
            status_info: surface_accent,
        }
    }

    /// Derives a semantic palette from the given concrete `ThemeColors` (default opacity = 1.0).
    pub fn from_theme(t: &ThemeColors) -> Self {
        Self::from_theme_with_opacity(t, 1.0)
    }

    /// Derives a semantic palette directly from the current GPUI app context,
    /// automatically reading the active theme and background opacity setting.
    pub fn from_context(cx: &App) -> Self {
        let t = theme(cx);
        let opacity = bg_opacity(cx);
        Self::from_theme_with_opacity(&t, opacity)
    }

    /// Convenience: build the palette for the built-in dark theme.
    pub fn dark_default() -> Self {
        Self::from_theme(&crate::theme::DARK_THEME)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::DARK_THEME;

    #[test]
    fn test_palette_from_dark_theme() {
        let p = SemanticPalette::from_theme(&DARK_THEME);
        // Transparent border should have zero alpha.
        assert_eq!(p.border_transparent.a, 0.0);
        // Accent surface should not be transparent.
        assert!(p.surface_accent.a > 0.0);
        // Status colors should be opaque and distinct.
        assert!(p.status_success.a > 0.0);
        assert!(p.status_warning.a > 0.0);
        assert!(p.status_error.a > 0.0);
        assert!(p.status_info.a > 0.0);
        assert_ne!(p.status_success, p.status_error);
    }

    #[test]
    fn test_palette_with_opacity() {
        let p = SemanticPalette::from_theme_with_opacity(&DARK_THEME, 0.8);
        assert!((p.surface_base.a - 0.8).abs() < 0.01);
        assert!((p.surface_overlay.a - 0.8).abs() < 0.01);
    }

    #[test]
    fn test_dark_default_shortcut() {
        let a = SemanticPalette::dark_default();
        let b = SemanticPalette::from_theme(&DARK_THEME);
        // Both should produce the same accent surface.
        assert_eq!(a.surface_accent, b.surface_accent);
    }
}
