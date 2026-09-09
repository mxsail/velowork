//! Centralized resource system.
//!
//! Provides a single registry for terminal color schemes and other
//! non-icon asset references. Icons live in the unified `AppIcon`
//! catalog (`crate::icon`) — do not add icon paths here.

use gpui::Hsla;
use velowork_core::theme::TerminalPalette;

/// Terminal ANSI color scheme (8 standard colors).
#[derive(Debug, Clone, Copy)]
pub struct TerminalColorScheme {
    pub black: Hsla,
    pub red: Hsla,
    pub green: Hsla,
    pub yellow: Hsla,
    pub blue: Hsla,
    pub magenta: Hsla,
    pub cyan: Hsla,
    pub white: Hsla,
}

/// Helper: convert a `u32` hex color to `Hsla`.
fn to_hsla(color: u32) -> Hsla {
    gpui::rgb(color).into()
}

/// Resource resolution helpers.
pub struct Resources;

impl Resources {
    /// Build a `TerminalColorScheme` from a `TerminalPalette`.
    pub fn terminal_scheme(p: &TerminalPalette) -> TerminalColorScheme {
        TerminalColorScheme {
            black: to_hsla(p.black),
            red: to_hsla(p.red),
            green: to_hsla(p.green),
            yellow: to_hsla(p.yellow),
            blue: to_hsla(p.blue),
            magenta: to_hsla(p.magenta),
            cyan: to_hsla(p.cyan),
            white: to_hsla(p.white),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use velowork_core::theme::DARK_PALETTE;

    #[test]
    fn test_terminal_scheme_from_palette() {
        let scheme = Resources::terminal_scheme(&DARK_PALETTE);
        // Red should have nonzero saturation.
        assert!(scheme.red.s > 0.0);
    }
}
