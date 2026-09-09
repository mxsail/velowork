mod colors;
mod types;
pub mod color_schemes;

pub use colors::{
    ThemeColors, DARK_THEME, HIGH_CONTRAST_THEME, LIGHT_THEME, PASTEL_DARK_THEME,
};
pub use color_schemes::{
    built_in_color_scheme_names, get_terminal_palette, get_terminal_palette_with_custom,
    hex_to_u32, is_built_in_color_scheme, u32_to_hex, CustomTerminalColorScheme,
    TerminalPalette, BUILTIN_COLOR_SCHEMES, DARK_PALETTE, LIGHT_PALETTE,
};
pub use types::{ColorSchema, ColorTheme, FolderColor, ThemeInfo, UiDensity};
