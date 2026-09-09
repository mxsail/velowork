//! Theme helpers — re-exported from velowork-theme.
pub use velowork_theme::{
    // Core types (via velowork-theme which re-exports from velowork-core)
    FolderColor, ThemeColors, ThemeInfo, ColorTheme,
    DARK_THEME, HIGH_CONTRAST_THEME, LIGHT_THEME, PASTEL_DARK_THEME,
    // GPUI helpers
    with_alpha, ansi_to_hsla_palette, bg_opacity, GlobalThemeProvider, readable_text,
    readable_text_on, surface_bg, surface_bg_min, surface_bg_t, theme,
};
