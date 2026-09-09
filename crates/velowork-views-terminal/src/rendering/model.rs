use alacritty_terminal::grid::Grid;
use alacritty_terminal::index::Point;
use alacritty_terminal::term::cell::Cell;
use velowork_workspace::settings::CursorShape;

use crate::elements::terminal_element::{SearchMatch, URLMatch};

/// Viewport and state snapshot required for terminal scene rendering.
pub struct TerminalRenderModel<'a> {
    pub grid: &'a Grid<Cell>,
    pub cols: usize,
    pub screen_lines: usize,
    pub display_offset: i32,
    pub cursor_point: Option<Point>,
    pub cursor_shape: CursorShape,
    pub cursor_visible: bool,
    pub selection: Option<((usize, i32), (usize, i32))>,
    pub search_matches: &'a [SearchMatch],
    pub current_match_index: Option<usize>,
    pub url_matches: &'a [URLMatch],
    pub hovered_url_group: Option<usize>,
}
