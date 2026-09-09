/// Configuration options determining which visual features to paint in a terminal scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalPaintOptions {
    pub show_cursor: bool,
    pub show_selection: bool,
    pub show_search_highlights: bool,
    pub show_url_decorations: bool,
}

impl TerminalPaintOptions {
    /// Full interactive options used for primary terminal panes.
    pub fn full(show_cursor: bool) -> Self {
        Self {
            show_cursor,
            show_selection: true,
            show_search_highlights: true,
            show_url_decorations: true,
        }
    }

    /// Lightweight options used for thumbnail / popover previews.
    pub fn thumbnail() -> Self {
        Self {
            show_cursor: true,
            show_selection: false,
            show_search_highlights: false,
            show_url_decorations: false,
        }
    }
}
