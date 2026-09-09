/// Terminal ANSI Color Schemes.
///
/// Provides 9 built-in color schemes for terminal cell rendering:
/// - Dark
/// - Light
/// - Solarized Dark
/// - Solarized Light
/// - Monokai
/// - Dracula
/// - Nord
/// - One Dark
/// - Gruvbox Dark

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TerminalPalette {
    pub black: u32,
    pub red: u32,
    pub green: u32,
    pub yellow: u32,
    pub blue: u32,
    pub magenta: u32,
    pub cyan: u32,
    pub white: u32,
    pub bright_black: u32,
    pub bright_red: u32,
    pub bright_green: u32,
    pub bright_yellow: u32,
    pub bright_blue: u32,
    pub bright_magenta: u32,
    pub bright_cyan: u32,
    pub bright_white: u32,
    pub foreground: u32,
    pub background: u32,
    #[serde(default)]
    pub cursor: Option<u32>,
    #[serde(default)]
    pub selection: Option<u32>,
}

impl Default for TerminalPalette {
    fn default() -> Self {
        DARK_PALETTE
    }
}

impl TerminalPalette {
    /// Get terminal color by ANSI name
    pub fn get_term_color(&self, named: &alacritty_terminal::vte::ansi::NamedColor) -> u32 {
        use alacritty_terminal::vte::ansi::NamedColor;
        match named {
            NamedColor::Black => self.black,
            NamedColor::Red => self.red,
            NamedColor::Green => self.green,
            NamedColor::Yellow => self.yellow,
            NamedColor::Blue => self.blue,
            NamedColor::Magenta => self.magenta,
            NamedColor::Cyan => self.cyan,
            NamedColor::White => self.white,
            NamedColor::BrightBlack => self.bright_black,
            NamedColor::BrightRed => self.bright_red,
            NamedColor::BrightGreen => self.bright_green,
            NamedColor::BrightYellow => self.bright_yellow,
            NamedColor::BrightBlue => self.bright_blue,
            NamedColor::BrightMagenta => self.bright_magenta,
            NamedColor::BrightCyan => self.bright_cyan,
            NamedColor::BrightWhite => self.bright_white,
            NamedColor::Foreground => self.foreground,
            NamedColor::Background => self.background,
            _ => self.foreground,
        }
    }

    /// Convert ANSI color to packed ARGB u32 (0xAARRGGBB) using this palette.
    pub fn ansi_to_argb(&self, color: &alacritty_terminal::vte::ansi::Color) -> u32 {
        use alacritty_terminal::vte::ansi::{Color, NamedColor};

        match color {
            Color::Named(named) => 0xFF000000 | self.get_term_color(named),
            Color::Spec(rgb) => {
                0xFF000000 | ((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | (rgb.b as u32)
            }
            Color::Indexed(idx) => {
                let idx = *idx as usize;
                if idx < 16 {
                    let named = match idx {
                        0 => NamedColor::Black,
                        1 => NamedColor::Red,
                        2 => NamedColor::Green,
                        3 => NamedColor::Yellow,
                        4 => NamedColor::Blue,
                        5 => NamedColor::Magenta,
                        6 => NamedColor::Cyan,
                        7 => NamedColor::White,
                        8 => NamedColor::BrightBlack,
                        9 => NamedColor::BrightRed,
                        10 => NamedColor::BrightGreen,
                        11 => NamedColor::BrightYellow,
                        12 => NamedColor::BrightBlue,
                        13 => NamedColor::BrightMagenta,
                        14 => NamedColor::BrightCyan,
                        15 => NamedColor::BrightWhite,
                        _ => NamedColor::Foreground,
                    };
                    0xFF000000 | self.get_term_color(&named)
                } else if idx < 232 {
                    let i = idx - 16;
                    let r = (i / 36) * 51;
                    let g = ((i / 6) % 6) * 51;
                    let b = (i % 6) * 51;
                    0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
                } else {
                    let gray = ((idx - 232) * 10 + 8) as u32;
                    0xFF000000 | (gray << 16) | (gray << 8) | gray
                }
            }
        }
    }
}

pub const DARK_PALETTE: TerminalPalette = TerminalPalette {
    black: 0x181920,
    red: 0xF87171,
    green: 0x34D399,
    yellow: 0xFBBF24,
    blue: 0x60A5FA,
    magenta: 0xA78BFA,
    cyan: 0x38BDF8,
    white: 0xEDEDF0,
    bright_black: 0x4B5563,
    bright_red: 0xFCA5A5,
    bright_green: 0x6EE7B7,
    bright_yellow: 0xFDE68A,
    bright_blue: 0x93C5FD,
    bright_magenta: 0xC4B5FD,
    bright_cyan: 0x7DD3FC,
    bright_white: 0xFFFFFF,
    foreground: 0xEDEDF0,
    background: 0x0F1015,
    cursor: Some(0x6366F1),
    selection: Some(0x282A3A),
};

pub const LIGHT_PALETTE: TerminalPalette = TerminalPalette {
    black: 0x1E293B,
    red: 0xDC2626,
    green: 0x16A34A,
    yellow: 0xCA8A04,
    blue: 0x2563EB,
    magenta: 0x9333EA,
    cyan: 0x0891B2,
    white: 0x64748B,
    bright_black: 0x475569,
    bright_red: 0xEF4444,
    bright_green: 0x22C55E,
    bright_yellow: 0xF59E0B,
    bright_blue: 0x3B82F6,
    bright_magenta: 0xA855F7,
    bright_cyan: 0x06B6D4,
    bright_white: 0x0F172A,
    foreground: 0x0F172A,
    background: 0xFFFFFF,
    cursor: Some(0x4F46E5),
    selection: Some(0xE0E7FF),
};

pub const SOLARIZED_DARK_PALETTE: TerminalPalette = TerminalPalette {
    black: 0x073642,
    red: 0xDC322F,
    green: 0x859900,
    yellow: 0xB58900,
    blue: 0x268BD2,
    magenta: 0xD33682,
    cyan: 0x2AA198,
    white: 0xEEE8D5,
    bright_black: 0x002B36,
    bright_red: 0xCB4B16,
    bright_green: 0x586E75,
    bright_yellow: 0x657B83,
    bright_blue: 0x839496,
    bright_magenta: 0x6C71C4,
    bright_cyan: 0x93A1A1,
    bright_white: 0xFDF6E3,
    foreground: 0x839496,
    background: 0x002B36,
    cursor: None,
    selection: None,
};

pub const SOLARIZED_LIGHT_PALETTE: TerminalPalette = TerminalPalette {
    black: 0x073642,
    red: 0xDC322F,
    green: 0x859900,
    yellow: 0xB58900,
    blue: 0x268BD2,
    magenta: 0xD33682,
    cyan: 0x2AA198,
    white: 0xEEE8D5,
    bright_black: 0x002B36,
    bright_red: 0xCB4B16,
    bright_green: 0x586E75,
    bright_yellow: 0x657B83,
    bright_blue: 0x839496,
    bright_magenta: 0x6C71C4,
    bright_cyan: 0x93A1A1,
    bright_white: 0xFDF6E3,
    foreground: 0x657B83,
    background: 0xFDF6E3,
    cursor: None,
    selection: None,
};

pub const MONOKAI_PALETTE: TerminalPalette = TerminalPalette {
    black: 0x272822,
    red: 0xF92672,
    green: 0xA6E22E,
    yellow: 0xE6DB74,
    blue: 0x66D9EF,
    magenta: 0xAE81FF,
    cyan: 0xA1EFE4,
    white: 0xF8F8F2,
    bright_black: 0x75715E,
    bright_red: 0xF92672,
    bright_green: 0xA6E22E,
    bright_yellow: 0xE6DB74,
    bright_blue: 0x66D9EF,
    bright_magenta: 0xAE81FF,
    bright_cyan: 0xA1EFE4,
    bright_white: 0xF9F8F5,
    foreground: 0xF8F8F2,
    background: 0x272822,
    cursor: None,
    selection: None,
};

pub const DRACULA_PALETTE: TerminalPalette = TerminalPalette {
    black: 0x21222C,
    red: 0xFF5555,
    green: 0x50FA7B,
    yellow: 0xF1FA8C,
    blue: 0xBD93F9,
    magenta: 0xFF79C6,
    cyan: 0x8BE9FD,
    white: 0xF8F8F2,
    bright_black: 0x6272A4,
    bright_red: 0xFF6E6E,
    bright_green: 0x69FF94,
    bright_yellow: 0xFFFA9E,
    bright_blue: 0xD6ACFF,
    bright_magenta: 0xFF92D0,
    bright_cyan: 0xA4FFFF,
    bright_white: 0xFFFFFF,
    foreground: 0xF8F8F2,
    background: 0x282A36,
    cursor: None,
    selection: None,
};

pub const NORD_PALETTE: TerminalPalette = TerminalPalette {
    black: 0x3B4252,
    red: 0xBF616A,
    green: 0xA3BE8C,
    yellow: 0xEBCB8B,
    blue: 0x81A1C1,
    magenta: 0xB48EAD,
    cyan: 0x88C0D0,
    white: 0xE5E9F0,
    bright_black: 0x4C566A,
    bright_red: 0xBF616A,
    bright_green: 0xA3BE8C,
    bright_yellow: 0xEBCB8B,
    bright_blue: 0x81A1C1,
    bright_magenta: 0xB48EAD,
    bright_cyan: 0x8FBCBB,
    bright_white: 0xECEFF4,
    foreground: 0xD8DEE9,
    background: 0x2E3440,
    cursor: None,
    selection: None,
};

pub const ONE_DARK_PALETTE: TerminalPalette = TerminalPalette {
    black: 0x1E222A,
    red: 0xE06C75,
    green: 0x98C379,
    yellow: 0xE5C07B,
    blue: 0x61AFEF,
    magenta: 0xC678DD,
    cyan: 0x56B6C2,
    white: 0xABB2BF,
    bright_black: 0x545862,
    bright_red: 0xE06C75,
    bright_green: 0x98C379,
    bright_yellow: 0xE5C07B,
    bright_blue: 0x61AFEF,
    bright_magenta: 0xC678DD,
    bright_cyan: 0x56B6C2,
    bright_white: 0xC8CCD4,
    foreground: 0xABB2BF,
    background: 0x282C34,
    cursor: None,
    selection: None,
};

pub const GRUVBOX_DARK_PALETTE: TerminalPalette = TerminalPalette {
    black: 0x282828,
    red: 0xCC241D,
    green: 0x98971A,
    yellow: 0xD79921,
    blue: 0x458588,
    magenta: 0xB16286,
    cyan: 0x689D6A,
    white: 0xA89984,
    bright_black: 0x928374,
    bright_red: 0xFB4934,
    bright_green: 0xB8BB26,
    bright_yellow: 0xFABD2F,
    bright_blue: 0x83A598,
    bright_magenta: 0xD3869B,
    bright_cyan: 0x8EC07C,
    bright_white: 0xEBDBB2,
    foreground: 0xEBDBB2,
    background: 0x282828,
    cursor: None,
    selection: None,
};

/// Custom terminal color scheme configuration
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CustomTerminalColorScheme {
    pub id: String,
    pub name: String,
    pub palette: TerminalPalette,
}

impl CustomTerminalColorScheme {
    pub fn new(id: impl Into<String>, name: impl Into<String>, palette: TerminalPalette) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            palette,
        }
    }
}

/// All built-in color scheme names
pub const BUILTIN_COLOR_SCHEMES: &[(&str, TerminalPalette)] = &[
    ("Dark", DARK_PALETTE),
    ("Light", LIGHT_PALETTE),
    ("Solarized Dark", SOLARIZED_DARK_PALETTE),
    ("Solarized Light", SOLARIZED_LIGHT_PALETTE),
    ("Monokai", MONOKAI_PALETTE),
    ("Dracula", DRACULA_PALETTE),
    ("Nord", NORD_PALETTE),
    ("One Dark", ONE_DARK_PALETTE),
    ("Gruvbox Dark", GRUVBOX_DARK_PALETTE),
];

/// Get list of built-in color scheme names
pub fn built_in_color_scheme_names() -> Vec<&'static str> {
    BUILTIN_COLOR_SCHEMES.iter().map(|(name, _)| *name).collect()
}

/// Check if a scheme name is built-in
pub fn is_built_in_color_scheme(name: &str) -> bool {
    BUILTIN_COLOR_SCHEMES.iter().any(|(n, _)| *n == name)
}

/// Get terminal palette by name (built-in only)
pub fn get_terminal_palette(name: &str) -> TerminalPalette {
    match name {
        "Dark" => DARK_PALETTE,
        "Light" => LIGHT_PALETTE,
        "Solarized Dark" => SOLARIZED_DARK_PALETTE,
        "Solarized Light" => SOLARIZED_LIGHT_PALETTE,
        "Monokai" => MONOKAI_PALETTE,
        "Dracula" => DRACULA_PALETTE,
        "Nord" => NORD_PALETTE,
        "One Dark" => ONE_DARK_PALETTE,
        "Gruvbox Dark" => GRUVBOX_DARK_PALETTE,
        _ => DARK_PALETTE,
    }
}

/// Get terminal palette by name or id, checking custom schemes first
pub fn get_terminal_palette_with_custom(
    name_or_id: &str,
    custom_schemes: &[CustomTerminalColorScheme],
) -> TerminalPalette {
    if let Some(custom) = custom_schemes.iter().find(|s| s.name == name_or_id || s.id == name_or_id) {
        return custom.palette;
    }
    get_terminal_palette(name_or_id)
}

/// Parse a hex color string (e.g., "#RRGGBB", "RRGGBB", "#RGB") into u32 (0x00RRGGBB)
pub fn hex_to_u32(hex: &str) -> Option<u32> {
    let clean = hex.trim().trim_start_matches('#');
    if clean.len() == 6 {
        u32::from_str_radix(clean, 16).ok()
    } else if clean.len() == 3 {
        let mut expanded = String::with_capacity(6);
        for ch in clean.chars() {
            expanded.push(ch);
            expanded.push(ch);
        }
        u32::from_str_radix(&expanded, 16).ok()
    } else {
        None
    }
}

/// Format a u32 (0x00RRGGBB) into a "#RRGGBB" uppercase hex string
pub fn u32_to_hex(color: u32) -> String {
    format!("#{:06X}", color & 0x00FFFFFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_conversion() {
        assert_eq!(hex_to_u32("#FFFFFF"), Some(0xFFFFFF));
        assert_eq!(hex_to_u32("000000"), Some(0x000000));
        assert_eq!(hex_to_u32("#FFF"), Some(0xFFFFFF));
        assert_eq!(hex_to_u32("#1e2"), Some(0x11EE22));
        assert_eq!(hex_to_u32("invalid"), None);
        assert_eq!(u32_to_hex(0xFF5533), "#FF5533");
        assert_eq!(u32_to_hex(0x000000), "#000000");
    }

    #[test]
    fn test_custom_scheme_resolution() {
        let custom = CustomTerminalColorScheme::new(
            "my_custom",
            "My Custom Theme",
            TerminalPalette {
                foreground: 0x123456,
                cursor: Some(0x654321),
                ..DARK_PALETTE
            },
        );
        let list = vec![custom];

        // Resolves custom by name
        let p = get_terminal_palette_with_custom("My Custom Theme", &list);
        assert_eq!(p.foreground, 0x123456);
        assert_eq!(p.cursor, Some(0x654321));

        // Resolves custom by id
        let p2 = get_terminal_palette_with_custom("my_custom", &list);
        assert_eq!(p2.foreground, 0x123456);

        // Fallback to built-in
        let p3 = get_terminal_palette_with_custom("Monokai", &list);
        assert_eq!(p3.foreground, MONOKAI_PALETTE.foreground);
    }
}
