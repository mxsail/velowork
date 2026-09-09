//! Theme module — re-exports from velowork-theme crate.

// Re-export everything from velowork-theme
#[allow(unused_imports)]
pub use velowork_theme::{
    ThemeColors, ThemeInfo, ColorTheme, FolderColor,
    DARK_THEME, LIGHT_THEME, PASTEL_DARK_THEME, HIGH_CONTRAST_THEME,
    with_alpha, ansi_to_hsla_palette,
    AppTheme, GlobalTheme, theme_entity,
    CustomThemeConfig, CustomThemeColors, get_themes_dir, load_custom_themes,
    surface_bg, surface_bg_min, surface_bg_t, bg_opacity, readable_text,
};

use gpui::*;

/// Get the current theme colors from the global theme entity (uses preview if active).
/// This is the desktop app's theme() — reads from GlobalTheme entity directly.
/// Different from velowork_theme::theme() which uses GlobalThemeProvider function pointer.
pub fn theme(cx: &App) -> ThemeColors {
    cx.global::<GlobalTheme>().0.read(cx).display_colors()
}
