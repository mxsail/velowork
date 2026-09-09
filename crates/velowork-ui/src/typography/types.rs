//! Typography types and domain definitions for Velowork.

use gpui::{px, Pixels, SharedString};
use serde::{Deserialize, Serialize};

/// The 4 distinct font categories in Velowork.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FontDomain {
    /// Non-terminal UI (Buttons, Sidebar, Panels, Menus, Dialogs, Controls)
    Ui,
    /// Code snippets, Log viewers, Labels
    Monospace,
    /// AI Chat Responses, Help Documentation, Markdown Viewers
    Markdown,
    /// Terminal PTY Buffer Canvas (Strictly Isolated)
    Terminal,
}

/// A resolved font family and size ready for rendering.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedFont {
    pub family: SharedString,
    pub size: Pixels,
}

impl ResolvedFont {
    pub fn new(family: impl Into<SharedString>, size: Pixels) -> Self {
        Self {
            family: family.into(),
            size,
        }
    }
}

/// Typography specification for all 4 font domains.
#[derive(Debug, Clone, PartialEq)]
pub struct Typography {
    pub ui_font: SharedString,
    pub ui_size: Pixels,
    pub mono_font: SharedString,
    pub mono_size: Pixels,
    pub markdown_font: SharedString,
    pub markdown_size: Pixels,
    pub terminal_font: SharedString,
    pub terminal_size: Pixels,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            ui_font: SharedString::from("System Default"),
            ui_size: px(13.0),
            mono_font: SharedString::from("JetBrains Mono"),
            mono_size: px(13.0),
            markdown_font: SharedString::from("System Default"),
            markdown_size: px(13.0),
            terminal_font: SharedString::from("JetBrains Mono"),
            terminal_size: px(14.0),
        }
    }
}
