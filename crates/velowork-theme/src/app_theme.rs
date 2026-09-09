use gpui::*;
use velowork_core::theme::{
    ColorSchema, ColorTheme, ThemeColors, DARK_THEME, LIGHT_THEME, PASTEL_DARK_THEME, HIGH_CONTRAST_THEME,
};
use crate::load_custom_themes;

/// Global theme state.
///
/// The active palette is resolved from two independent pieces of state:
/// * `schema` (`ColorSchema`) — the user's appearance preference
///   (Dark / Light / System).
/// * `system_is_dark` — the OS-level dark/light signal, only consulted when
///   `schema` is `System`.
///
/// `dark_color_theme` / `light_color_theme` are the dedicated palettes for
/// each appearance. When the resolved appearance is dark we use
/// `dark_color_theme`, otherwise `light_color_theme`. This removes the old
/// ambiguity where a single `ThemeMode::Auto` both followed the system *and*
/// selected a palette.
pub struct AppTheme {
    pub dark_color_theme: ColorTheme,
    pub light_color_theme: ColorTheme,
    pub schema: ColorSchema,
    pub colors: ThemeColors,
    system_is_dark: bool,
    /// Custom theme colors (when a palette is `Custom`).
    custom_colors: Option<ThemeColors>,
    /// Preview colors for live preview (temporarily overrides colors).
    preview_colors: Option<ThemeColors>,
    /// Global background opacity (0.0 - 1.0) driven by the `bg_opacity`
    /// system setting. Consumed by `surface_bg` / `readable_text` so every
    /// surface re-renders when the user changes transparency.
    pub opacity: f32,
}

impl AppTheme {
    pub fn new(
        dark_color_theme: ColorTheme,
        light_color_theme: ColorTheme,
        schema: ColorSchema,
        custom_theme_id: Option<&str>,
        system_is_dark: bool,
    ) -> Self {
        let mut this = Self {
            dark_color_theme,
            light_color_theme,
            schema,
            colors: DARK_THEME,
            system_is_dark,
            custom_colors: None,
            preview_colors: None,
            opacity: 1.0,
        };
        if dark_color_theme == ColorTheme::Custom || light_color_theme == ColorTheme::Custom {
            this.load_custom(custom_theme_id);
        }
        this.update_colors();
        this
    }

    /// Update the global background opacity (clamped to a sane range).
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.1, 1.0);
    }

    /// Resolved effective appearance: dark when forced dark, light when forced
    /// light, or follows the OS when `schema` is `System`.
    pub fn effective_is_dark(&self) -> bool {
        match self.schema {
            ColorSchema::System => self.system_is_dark,
            ColorSchema::Dark => true,
            ColorSchema::Light => false,
        }
    }

    /// The palette that should currently render, based on the active appearance.
    pub fn active_color_theme(&self) -> ColorTheme {
        if self.effective_is_dark() {
            self.dark_color_theme
        } else {
            self.light_color_theme
        }
    }

    fn colors_for(ct: ColorTheme, custom: Option<ThemeColors>) -> ThemeColors {
        match ct {
            ColorTheme::Dark => DARK_THEME,
            ColorTheme::Light => LIGHT_THEME,
            ColorTheme::PastelDark => PASTEL_DARK_THEME,
            ColorTheme::HighContrast => HIGH_CONTRAST_THEME,
            ColorTheme::Custom => custom.unwrap_or(DARK_THEME),
        }
    }

    /// Load the persisted custom theme's colors (if either palette is `Custom`).
    fn load_custom(&mut self, custom_theme_id: Option<&str>) {
        if let Some(id) = custom_theme_id {
            for (info, colors) in load_custom_themes() {
                if info.id == format!("custom:{}", id) {
                    self.custom_colors = Some(colors);
                    break;
                }
            }
        }
    }

    pub fn set_dark_color_theme(&mut self, ct: ColorTheme) {
        self.dark_color_theme = ct;
        self.update_colors();
    }

    pub fn set_light_color_theme(&mut self, ct: ColorTheme) {
        self.light_color_theme = ct;
        self.update_colors();
    }

    pub fn set_schema(&mut self, schema: ColorSchema) {
        self.schema = schema;
        self.update_colors();
    }

    pub fn set_system_appearance(&mut self, is_dark: bool) {
        self.system_is_dark = is_dark;
        if self.schema == ColorSchema::System {
            self.update_colors();
        }
    }

    /// Set custom theme colors (applies immediately when a palette is `Custom`).
    pub fn set_custom_colors(&mut self, colors: ThemeColors) {
        self.custom_colors = Some(colors);
        self.update_colors();
    }

    /// Set preview colors temporarily (for live preview).
    pub fn set_preview(&mut self, ct: ColorTheme) {
        self.preview_colors = Some(Self::colors_for(ct, self.custom_colors));
    }

    /// Set preview colors directly (for custom themes).
    pub fn set_preview_colors(&mut self, colors: ThemeColors) {
        self.preview_colors = Some(colors);
    }

    /// Clear preview and restore actual theme.
    pub fn clear_preview(&mut self) {
        self.preview_colors = None;
    }

    /// Get the current display colors (preview if set, otherwise actual).
    pub fn display_colors(&self) -> ThemeColors {
        self.preview_colors.unwrap_or(self.colors)
    }

    /// Re-sync the appearance preference, both palettes, and custom theme from
    /// persisted settings. Driven by the settings observer in `init_theme`.
    pub fn sync_from_settings(
        &mut self,
        dark: ColorTheme,
        light: ColorTheme,
        schema: ColorSchema,
        custom_id: Option<&str>,
    ) {
        self.dark_color_theme = dark;
        self.light_color_theme = light;
        self.schema = schema;
        if dark == ColorTheme::Custom || light == ColorTheme::Custom {
            self.load_custom(custom_id);
        }
        self.update_colors();
    }

    fn update_colors(&mut self) {
        self.colors = Self::colors_for(self.active_color_theme(), self.custom_colors);
    }
}

/// Wrapper for global theme entity
pub struct GlobalTheme(pub Entity<AppTheme>);

impl Global for GlobalTheme {}

/// Get the theme entity for observation
pub fn theme_entity(cx: &App) -> Entity<AppTheme> {
    cx.global::<GlobalTheme>().0.clone()
}
