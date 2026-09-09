#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

// Re-export core theme types (source of truth is velowork-core)
pub use velowork_core::theme::{
    ThemeColors, ThemeInfo, ColorTheme, FolderColor,
    DARK_THEME, LIGHT_THEME, PASTEL_DARK_THEME, HIGH_CONTRAST_THEME,
};

pub mod custom;
mod gpui_helpers;
mod app_theme;

pub use gpui_helpers::{
    ansi_to_hsla_palette, bg_opacity, contrast_ratio, GlobalThemeProvider, readable_text,
    readable_text_on, surface_bg, surface_bg_min, surface_bg_t, theme, with_alpha,
};
pub use app_theme::{AppTheme, GlobalTheme, theme_entity};
pub use custom::{CustomThemeConfig, CustomThemeColors, get_themes_dir, load_custom_themes};
