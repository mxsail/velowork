use serde::{Deserialize, Serialize};

/// Color theme (palette) selection.
///
/// Unlike the legacy `ThemeMode`, this type no longer carries an `Auto`
/// variant: whether the *dark* or *light* palette is active is decided by the
/// `ColorSchema` appearance setting (Dark / Light / System) plus the system
/// appearance. The two are kept independent so each appearance mode can have
/// its own dedicated palette (`dark_color_theme` / `light_color_theme`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ColorTheme {
    #[default]
    Dark,
    Light,
    PastelDark,
    HighContrast,
    /// Custom theme loaded from configuration
    Custom,
}

impl ColorTheme {
    pub fn translation_key(self) -> &'static str {
        match self {
            ColorTheme::Dark => "settings.color_theme.dark",
            ColorTheme::Light => "settings.color_theme.light",
            ColorTheme::PastelDark => "settings.color_theme.pastel_dark",
            ColorTheme::HighContrast => "settings.color_theme.high_contrast",
            ColorTheme::Custom => "settings.color_theme.custom",
        }
    }

    pub fn all_variants() -> &'static [ColorTheme] {
        &[
            ColorTheme::Dark,
            ColorTheme::Light,
            ColorTheme::PastelDark,
            ColorTheme::HighContrast,
            ColorTheme::Custom,
        ]
    }
}

/// Appearance mode (dark / light / follow system).
///
/// This decides *which* palette (`dark_color_theme` or `light_color_theme`) is
/// shown. `System` follows the OS appearance at runtime, so the active
/// appearance (and therefore the active palette) can change without the user
/// touching a theme setting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorSchema {
    #[default]
    Dark,
    Light,
    System,
}

/// Interface density level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UiDensity {
    /// Compact layout — reduces height by 4 px per control.
    Compact,
    /// Standard layout (default).
    #[default]
    Default,
    /// Comfortable layout — adds 4 px per control.
    Comfortable,
}

impl UiDensity {
    /// Returns the height offset (in logical pixels) applied to controls.
    pub fn height_offset(self) -> f32 {
        match self {
            UiDensity::Compact => -4.0,
            UiDensity::Default => 0.0,
            UiDensity::Comfortable => 4.0,
        }
    }

    /// Returns the proportional spacing scale factor for margins, paddings, and gaps.
    #[inline]
    pub fn spacing_factor(self) -> f32 {
        match self {
            UiDensity::Compact => 0.80,
            UiDensity::Default => 1.00,
            UiDensity::Comfortable => 1.25,
        }
    }
}

impl ColorSchema {
    pub fn display_name(self) -> &'static str {
        match self {
            ColorSchema::Dark => "Dark",
            ColorSchema::Light => "Light",
            ColorSchema::System => "System",
        }
    }

    pub fn translation_key(self) -> &'static str {
        match self {
            ColorSchema::Dark => "settings.color_schema.dark",
            ColorSchema::Light => "settings.color_schema.light",
            ColorSchema::System => "settings.color_schema.system",
        }
    }

    pub fn all_variants() -> &'static [ColorSchema] {
        &[ColorSchema::Dark, ColorSchema::Light, ColorSchema::System]
    }
}

/// Folder color options for projects
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FolderColor {
    #[default]
    Default,
    Red,
    Orange,
    Yellow,
    Lime,
    Green,
    Teal,
    Cyan,
    Blue,
    Indigo,
    Purple,
    Pink,
}

impl FolderColor {
    /// Get all folder color variants for UI
    pub fn all() -> &'static [FolderColor] {
        &[
            FolderColor::Default,
            FolderColor::Red,
            FolderColor::Orange,
            FolderColor::Yellow,
            FolderColor::Lime,
            FolderColor::Green,
            FolderColor::Teal,
            FolderColor::Cyan,
            FolderColor::Blue,
            FolderColor::Indigo,
            FolderColor::Purple,
            FolderColor::Pink,
        ]
    }
}

/// Available built-in themes
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub is_dark: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ui_density_spacing_and_height_factors() {
        assert_eq!(UiDensity::Compact.height_offset(), -4.0);
        assert_eq!(UiDensity::Compact.spacing_factor(), 0.80);

        assert_eq!(UiDensity::Default.height_offset(), 0.0);
        assert_eq!(UiDensity::Default.spacing_factor(), 1.00);

        assert_eq!(UiDensity::Comfortable.height_offset(), 4.0);
        assert_eq!(UiDensity::Comfortable.spacing_factor(), 1.25);
    }
}
