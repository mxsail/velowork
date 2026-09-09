//! Custom theme configuration support
//!
//! Allows loading custom themes from JSON files in the themes directory.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use velowork_core::theme::{ThemeColors, ThemeInfo};

/// Custom theme configuration file format
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomThemeConfig {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub is_dark: bool,
    pub colors: CustomThemeColors,
}

/// Serializable theme colors with hex string format (33 UI colors)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomThemeColors {
    // Background colors
    #[serde(default = "default_bg_primary")]
    pub bg_primary: String,
    #[serde(default = "default_bg_secondary")]
    pub bg_secondary: String,
    #[serde(default = "default_bg_header")]
    pub bg_header: String,
    #[serde(default = "default_bg_panel")]
    pub bg_panel: String,
    #[serde(default = "default_bg_selection")]
    pub bg_selection: String,
    #[serde(default = "default_bg_hover")]
    pub bg_hover: String,

    // Semantic accent (design spec: #5B6BD6, hover #6E7BF0)
    #[serde(default = "default_accent")]
    pub accent: String,

    // Border colors
    #[serde(default = "default_border")]
    pub border: String,
    #[serde(default = "default_border_active")]
    pub border_active: String,

    // Text colors
    #[serde(default = "default_text_primary")]
    pub text_primary: String,
    #[serde(default = "default_text_secondary")]
    pub text_secondary: String,
    #[serde(default = "default_text_muted")]
    pub text_muted: String,

    // Status colors
    #[serde(default = "default_success")]
    pub success: String,
    #[serde(default = "default_warning")]
    pub warning: String,
    #[serde(default = "default_error")]
    pub error: String,

    // Folder colors
    #[serde(default = "default_folder_default")]
    pub folder_default: String,
    #[serde(default = "default_folder_red")]
    pub folder_red: String,
    #[serde(default = "default_folder_orange")]
    pub folder_orange: String,
    #[serde(default = "default_folder_yellow")]
    pub folder_yellow: String,
    #[serde(default = "default_folder_lime")]
    pub folder_lime: String,
    #[serde(default = "default_folder_green")]
    pub folder_green: String,
    #[serde(default = "default_folder_teal")]
    pub folder_teal: String,
    #[serde(default = "default_folder_cyan")]
    pub folder_cyan: String,
    #[serde(default = "default_folder_blue")]
    pub folder_blue: String,
    #[serde(default = "default_folder_indigo")]
    pub folder_indigo: String,
    #[serde(default = "default_folder_purple")]
    pub folder_purple: String,
    #[serde(default = "default_folder_pink")]
    pub folder_pink: String,
}

// Default color functions for serde (based on dark theme)
fn default_bg_primary() -> String { "#1e1e1e".to_string() }
fn default_bg_secondary() -> String { "#252526".to_string() }
fn default_bg_header() -> String { "#323233".to_string() }
fn default_bg_panel() -> String { "#202326".to_string() }
fn default_bg_selection() -> String { "#264f78".to_string() }
fn default_bg_hover() -> String { "#2a2d2e".to_string() }
fn default_accent() -> String { "#5B6BD6".to_string() }
fn default_border() -> String { "#252526".to_string() }
fn default_border_active() -> String { "#007acc".to_string() }
fn default_text_primary() -> String { "#cccccc".to_string() }
fn default_text_secondary() -> String { "#808080".to_string() }
fn default_text_muted() -> String { "#6a6a6a".to_string() }
fn default_success() -> String { "#4ec9b0".to_string() }
fn default_warning() -> String { "#dcdcaa".to_string() }
fn default_error() -> String { "#f44747".to_string() }
fn default_folder_default() -> String { "#8a9199".to_string() }
fn default_folder_red() -> String { "#e06c75".to_string() }
fn default_folder_orange() -> String { "#d19a66".to_string() }
fn default_folder_yellow() -> String { "#e5c07b".to_string() }
fn default_folder_lime() -> String { "#a3d955".to_string() }
fn default_folder_green() -> String { "#98c379".to_string() }
fn default_folder_teal() -> String { "#2fbda0".to_string() }
fn default_folder_cyan() -> String { "#56d7e5".to_string() }
fn default_folder_blue() -> String { "#61afef".to_string() }
fn default_folder_indigo() -> String { "#818cf8".to_string() }
fn default_folder_purple() -> String { "#c678dd".to_string() }
fn default_folder_pink() -> String { "#e06c9f".to_string() }

impl CustomThemeColors {
    /// Parse a hex color string (e.g., "#1e1e1e" or "1e1e1e") to u32
    fn parse_hex(s: &str) -> u32 {
        let s = s.trim_start_matches('#');
        u32::from_str_radix(s, 16).unwrap_or(0)
    }

    /// Format a u32 color as a `#rrggbb` hex string.
    fn to_hex(c: u32) -> String {
        format!("#{:06x}", c & 0x00ff_ffff)
    }

    /// Build hex-string colors from a [`ThemeColors`] (inverse of
    /// [`Self::to_theme_colors`]). Lets a built-in or live theme be emitted as
    /// an editable custom-theme blob (e.g. for `velowork theme show`).
    pub fn from_theme_colors(c: &ThemeColors) -> Self {
        let h = Self::to_hex;
        Self {
            bg_primary: h(c.bg_primary),
            bg_secondary: h(c.bg_secondary),
            bg_header: h(c.bg_header),
            bg_panel: h(c.bg_panel),
            bg_selection: h(c.bg_selection),
            bg_hover: h(c.bg_hover),
            accent: h(c.accent),
            border: h(c.border),
            border_active: h(c.border_active),
            text_primary: h(c.text_primary),
            text_secondary: h(c.text_secondary),
            text_muted: h(c.text_muted),
            success: h(c.success),
            warning: h(c.warning),
            error: h(c.error),
            folder_default: h(c.folder_default),
            folder_red: h(c.folder_red),
            folder_orange: h(c.folder_orange),
            folder_yellow: h(c.folder_yellow),
            folder_lime: h(c.folder_lime),
            folder_green: h(c.folder_green),
            folder_teal: h(c.folder_teal),
            folder_cyan: h(c.folder_cyan),
            folder_blue: h(c.folder_blue),
            folder_indigo: h(c.folder_indigo),
            folder_purple: h(c.folder_purple),
            folder_pink: h(c.folder_pink),
        }
    }

    /// Convert to ThemeColors
    pub fn to_theme_colors(&self) -> ThemeColors {
        ThemeColors {
            bg_primary: Self::parse_hex(&self.bg_primary),
            bg_secondary: Self::parse_hex(&self.bg_secondary),
            bg_header: Self::parse_hex(&self.bg_header),
            bg_panel: Self::parse_hex(&self.bg_panel),
            bg_selection: Self::parse_hex(&self.bg_selection),
            bg_hover: Self::parse_hex(&self.bg_hover),
            accent: Self::parse_hex(&self.accent),
            border: Self::parse_hex(&self.border),
            border_active: Self::parse_hex(&self.border_active),
            text_primary: Self::parse_hex(&self.text_primary),
            text_secondary: Self::parse_hex(&self.text_secondary),
            text_muted: Self::parse_hex(&self.text_muted),
            success: Self::parse_hex(&self.success),
            warning: Self::parse_hex(&self.warning),
            error: Self::parse_hex(&self.error),
            folder_default: Self::parse_hex(&self.folder_default),
            folder_red: Self::parse_hex(&self.folder_red),
            folder_orange: Self::parse_hex(&self.folder_orange),
            folder_yellow: Self::parse_hex(&self.folder_yellow),
            folder_lime: Self::parse_hex(&self.folder_lime),
            folder_green: Self::parse_hex(&self.folder_green),
            folder_teal: Self::parse_hex(&self.folder_teal),
            folder_cyan: Self::parse_hex(&self.folder_cyan),
            folder_blue: Self::parse_hex(&self.folder_blue),
            folder_indigo: Self::parse_hex(&self.folder_indigo),
            folder_purple: Self::parse_hex(&self.folder_purple),
            folder_pink: Self::parse_hex(&self.folder_pink),
        }
    }
}

/// Get path to custom themes directory
pub fn get_themes_dir() -> PathBuf {
    if let Some(p) = velowork_core::profiles::try_current() {
        p.themes_dir()
    } else {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("velowork")
            .join("themes")
    }
}

/// Load custom themes from the themes directory
pub fn load_custom_themes() -> Vec<(ThemeInfo, ThemeColors)> {
    let themes_dir = get_themes_dir();
    let mut custom_themes = Vec::new();

    if !themes_dir.exists() {
        // Create themes directory and example theme
        if let Err(e) = std::fs::create_dir_all(&themes_dir) {
            log::warn!("Failed to create themes directory: {}", e);
            return custom_themes;
        }

        // Write an example custom theme file
        let example_theme = CustomThemeConfig {
            name: "My Custom Theme".to_string(),
            description: "An example custom theme - modify colors as desired".to_string(),
            is_dark: true,
            colors: CustomThemeColors {
                bg_primary: "#1a1a1a".to_string(),
                bg_secondary: "#222222".to_string(),
                bg_header: "#282828".to_string(),
                bg_panel: "#202326".to_string(),
                bg_selection: "#363983".to_string(),
                bg_hover: "#303030".to_string(),
                accent: "#5B6BD6".to_string(),
                border: "#3a3a3a".to_string(),
                border_active: "#96cbfe".to_string(),
                text_primary: "#eeeeee".to_string(),
                text_secondary: "#999999".to_string(),
                text_muted: "#666666".to_string(),
                success: "#a8ff60".to_string(),
                warning: "#ffffb6".to_string(),
                error: "#ff6c60".to_string(),
                folder_default: "#a9b1d6".to_string(),
                folder_red: "#f7768e".to_string(),
                folder_orange: "#ff9e64".to_string(),
                folder_yellow: "#e0af68".to_string(),
                folder_lime: "#b8e655".to_string(),
                folder_green: "#9ece6a".to_string(),
                folder_teal: "#2ac3a2".to_string(),
                folder_cyan: "#67e8f9".to_string(),
                folder_blue: "#7dcfff".to_string(),
                folder_indigo: "#7f7ff5".to_string(),
                folder_purple: "#bb9af7".to_string(),
                folder_pink: "#f472b6".to_string(),
            },
        };

        let example_path = themes_dir.join("example-theme.json");
        if let Ok(content) = serde_json::to_string_pretty(&example_theme) {
            let _ = std::fs::write(&example_path, content);
        }
    }

    // Load all JSON files from themes directory
    if let Ok(entries) = std::fs::read_dir(&themes_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json")
                && let Ok(content) = std::fs::read_to_string(&path)
                && let Ok(config) = serde_json::from_str::<CustomThemeConfig>(&content) {
                    let theme_id = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("custom")
                        .to_string();

                    let info = ThemeInfo {
                        id: format!("custom:{}", theme_id),
                        name: config.name.clone(),
                        description: config.description.clone(),
                        is_dark: config.is_dark,
                    };
                    let colors = config.colors.to_theme_colors();
                    custom_themes.push((info, colors));
                }
        }
    }

    custom_themes
}
