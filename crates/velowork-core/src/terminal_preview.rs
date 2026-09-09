use alacritty_terminal::vte::ansi::Color;

/// A styled segment of text in a terminal preview.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TerminalPreviewSpan {
    pub text: String,
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

/// A single rendered line in a terminal preview.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TerminalPreviewLine {
    pub spans: Vec<TerminalPreviewSpan>,
}

/// Snapshot of the terminal's visible content for preview.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TerminalPreviewSnapshot {
    pub lines: Vec<TerminalPreviewLine>,
    pub cols: usize,
    pub rows: usize,
    pub is_empty: bool,
}
