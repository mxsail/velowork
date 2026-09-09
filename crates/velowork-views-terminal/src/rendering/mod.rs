//! Shared Terminal Rendering Subsystem.
//!
//! Provides decoupled render model, geometry calculation, paint options, and shared painter.

pub mod geometry;
pub mod model;
pub mod options;
pub mod painter;

pub use geometry::TerminalRenderGeometry;
pub use model::TerminalRenderModel;
pub use options::TerminalPaintOptions;
pub use painter::TerminalPainter;

use std::sync::Arc;
use alacritty_terminal::grid::Dimensions;
use gpui::*;
use velowork_core::theme::{DARK_PALETTE, ThemeColors};
use velowork_terminal::terminal::Terminal;
use velowork_workspace::settings::CursorShape;

/// Render an aspect-ratio-preserved thumbnail of the given terminal into `available_bounds`.
pub fn render_terminal_thumbnail(
    terminal: &Arc<Terminal>,
    available_bounds: Bounds<Pixels>,
    theme_colors: &ThemeColors,
    font: Font,
    font_bold: Font,
    font_italic: Font,
    font_bold_italic: Font,
    window: &mut Window,
    cx: &mut App,
) {
    terminal.with_content(|term| {
        let grid = term.grid();
        let cols = grid.columns();
        let screen_lines = grid.screen_lines();
        let display_offset = grid.display_offset() as i32;

        let geometry = TerminalRenderGeometry::fit_contain(
            available_bounds,
            cols,
            screen_lines,
            8.0,
            16.0,
        );

        let options = TerminalPaintOptions::thumbnail();
        let palette = DARK_PALETTE;

        let model = TerminalRenderModel {
            grid,
            cols,
            screen_lines,
            display_offset,
            cursor_point: Some(grid.cursor.point),
            cursor_shape: CursorShape::Block,
            cursor_visible: true,
            selection: None,
            search_matches: &[],
            current_match_index: None,
            url_matches: &[],
            hovered_url_group: None,
        };

        // 1. Fill container background
        window.paint_quad(fill(available_bounds, rgb(palette.background)));

        // 2. Paint grid with shared painter
        let painter = TerminalPainter {
            model: &model,
            geometry: &geometry,
            options: &options,
            palette: &palette,
            theme_colors,
            font,
            font_bold,
            font_italic,
            font_bold_italic,
        };

        painter.paint(window, cx);
    });
}
