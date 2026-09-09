use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{Color, NamedColor};
use gpui::*;
use velowork_core::theme::{TerminalPalette, ThemeColors};
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::theme::ansi_to_hsla_palette;
use velowork_workspace::settings::CursorShape;

use super::geometry::TerminalRenderGeometry;
use super::model::TerminalRenderModel;
use super::options::TerminalPaintOptions;
use crate::elements::terminal_rendering::{is_default_bg, BatchedTextRun, LayoutRect};

/// Shared Terminal Scene Painter responsible for rendering terminal cells, backgrounds, and cursor.
pub struct TerminalPainter<'a> {
    pub model: &'a TerminalRenderModel<'a>,
    pub geometry: &'a TerminalRenderGeometry,
    pub options: &'a TerminalPaintOptions,
    pub palette: &'a TerminalPalette,
    pub theme_colors: &'a ThemeColors,
    pub font: Font,
    pub font_bold: Font,
    pub font_italic: Font,
    pub font_bold_italic: Font,
}

impl<'a> TerminalPainter<'a> {
    /// Execute the full painting pass to the current GPUI window.
    pub fn paint(&self, window: &mut Window, cx: &mut App) {
        let (batched_runs, rects) = self.collect_runs_and_rects();

        // 1. Paint background rectangles
        self.paint_backgrounds(&rects, window);

        // 2. Paint search highlights
        if self.options.show_search_highlights && !self.model.search_matches.is_empty() {
            self.paint_search_highlights(window);
        }

        // 3. Paint URL hover underlines
        if self.options.show_url_decorations && !self.model.url_matches.is_empty() {
            self.paint_urls(window);
        }

        // 4. Paint text runs
        self.paint_text_runs(&batched_runs, window, cx);

        // 5. Paint cursor
        if self.options.show_cursor && self.model.cursor_visible {
            self.paint_cursor(window);
        }
    }

    fn collect_runs_and_rects(&self) -> (Vec<BatchedTextRun>, Vec<LayoutRect>) {
        let grid = self.model.grid;
        let screen_lines = self.model.screen_lines;
        let cols = self.model.cols;
        let display_offset = self.model.display_offset;
        let p = SemanticPalette::from_theme(self.theme_colors);

        let normalized_selection = if self.options.show_selection {
            self.model.selection.map(|((start_col, start_row), (end_col, end_row))| {
                if start_row < end_row || (start_row == end_row && start_col <= end_col) {
                    (start_row as i32, start_col as i32, end_row as i32, end_col as i32)
                } else {
                    (end_row as i32, end_col as i32, start_row as i32, start_col as i32)
                }
            })
        } else {
            None
        };

        let mut batched_runs: Vec<BatchedTextRun> = Vec::new();
        let mut rects: Vec<LayoutRect> = Vec::new();
        let mut current_batch: Option<BatchedTextRun> = None;
        let mut current_rect: Option<LayoutRect> = None;

        for row in 0..screen_lines {
            let visual_line = row as i32;
            let buffer_line = visual_line - display_offset;

            if let Some(batch) = current_batch.take() {
                batched_runs.push(batch);
            }
            if let Some(rect) = current_rect.take() {
                rects.push(rect);
            }

            for col in 0..cols {
                let cell_point = Point {
                    line: Line(buffer_line),
                    column: Column(col),
                };
                let cell = &grid[cell_point];
                let col_i32 = col as i32;

                let mut fg = cell.fg;
                let mut bg = cell.bg;

                if cell.flags.contains(Flags::BOLD) {
                    fg = match fg {
                        Color::Named(NamedColor::Black) => Color::Named(NamedColor::BrightBlack),
                        Color::Named(NamedColor::Red) => Color::Named(NamedColor::BrightRed),
                        Color::Named(NamedColor::Green) => Color::Named(NamedColor::BrightGreen),
                        Color::Named(NamedColor::Yellow) => Color::Named(NamedColor::BrightYellow),
                        Color::Named(NamedColor::Blue) => Color::Named(NamedColor::BrightBlue),
                        Color::Named(NamedColor::Magenta) => Color::Named(NamedColor::BrightMagenta),
                        Color::Named(NamedColor::Cyan) => Color::Named(NamedColor::BrightCyan),
                        Color::Named(NamedColor::White) => Color::Named(NamedColor::BrightWhite),
                        Color::Indexed(idx @ 0..=7) => Color::Indexed(idx + 8),
                        other => other,
                    };
                }

                if cell.flags.contains(Flags::INVERSE) {
                    std::mem::swap(&mut fg, &mut bg);
                }

                let is_selected = if let Some((start_row, start_col, end_row, end_col)) = normalized_selection {
                    if buffer_line >= start_row && buffer_line <= end_row {
                        if start_row == end_row {
                            col_i32 >= start_col && col_i32 <= end_col
                        } else if buffer_line == start_row {
                            col_i32 >= start_col
                        } else if buffer_line == end_row {
                            col_i32 <= end_col
                        } else {
                            true
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };

                let bg_color = if is_selected {
                    Some(self.palette.selection.map(rgb).map(Hsla::from).unwrap_or(p.editor_selection))
                } else if !is_default_bg(&bg, self.palette.background) {
                    Some(ansi_to_hsla_palette(self.palette, &bg))
                } else {
                    None
                };

                if let Some(color) = bg_color {
                    let can_extend = current_rect.as_ref().is_some_and(|rect| {
                        rect.line == visual_line
                            && rect.start_col + rect.num_cells as i32 == col_i32
                            && rect.color == color
                    });
                    if can_extend {
                        if let Some(rect) = current_rect.as_mut() {
                            rect.extend();
                        }
                    } else {
                        if let Some(prev) = current_rect.take() {
                            rects.push(prev);
                        }
                        current_rect = Some(LayoutRect::new(visual_line, col_i32, color));
                    }
                } else if let Some(rect) = current_rect.take() {
                    rects.push(rect);
                }

                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    continue;
                }
                if cell.c == ' ' && !cell.flags.intersects(Flags::UNDERLINE | Flags::STRIKEOUT) {
                    continue;
                }

                let mut fg_color = if is_selected {
                    rgb(self.palette.foreground).into()
                } else {
                    ansi_to_hsla_palette(self.palette, &fg)
                };

                if cell.flags.contains(Flags::DIM) && !cell.flags.contains(Flags::BOLD) {
                    fg_color.l = (fg_color.l * 0.66).clamp(0.0, 1.0);
                }

                let is_bold = cell.flags.contains(Flags::BOLD);
                let is_italic = cell.flags.contains(Flags::ITALIC);
                let font = match (is_bold, is_italic) {
                    (true, true) => self.font_bold_italic.clone(),
                    (true, false) => self.font_bold.clone(),
                    (false, true) => self.font_italic.clone(),
                    (false, false) => self.font.clone(),
                };

                let text_style = TextRun {
                    len: cell.c.len_utf8(),
                    font,
                    color: fg_color,
                    background_color: None,
                    underline: if cell.flags.intersects(Flags::ALL_UNDERLINES) {
                        let line_color = cell
                            .underline_color()
                            .map(|c| ansi_to_hsla_palette(self.palette, &c))
                            .unwrap_or(fg_color);
                        Some(UnderlineStyle {
                            color: Some(line_color),
                            thickness: px(1.0),
                            wavy: cell.flags.contains(Flags::UNDERCURL),
                        })
                    } else {
                        None
                    },
                    strikethrough: if cell.flags.contains(Flags::STRIKEOUT) {
                        Some(StrikethroughStyle {
                            color: Some(fg_color),
                            thickness: px(1.0),
                        })
                    } else {
                        None
                    },
                };

                if cell.flags.contains(Flags::WIDE_CHAR) {
                    if let Some(prev) = current_batch.take() {
                        batched_runs.push(prev);
                    }
                    batched_runs.push(BatchedTextRun::new_wide(
                        visual_line,
                        col_i32,
                        cell.c,
                        text_style,
                    ));
                } else {
                    let can_append = current_batch.as_ref().is_some_and(|batch| {
                        batch.can_append(&text_style, visual_line, col_i32)
                    });
                    if can_append {
                        if let Some(batch) = current_batch.as_mut() {
                            batch.append_char(cell.c);
                        }
                    } else {
                        if let Some(prev) = current_batch.take() {
                            batched_runs.push(prev);
                        }
                        current_batch = Some(BatchedTextRun::new(
                            visual_line,
                            col_i32,
                            cell.c,
                            text_style,
                        ));
                    }
                }
            }
        }

        if let Some(batch) = current_batch {
            batched_runs.push(batch);
        }
        if let Some(rect) = current_rect {
            rects.push(rect);
        }

        (batched_runs, rects)
    }

    fn paint_backgrounds(&self, rects: &[LayoutRect], window: &mut Window) {
        let origin = self.geometry.bounds.origin;
        let cell_width = self.geometry.cell_width;
        let line_height = self.geometry.line_height;

        for rect in rects {
            rect.paint(origin, cell_width, line_height, window);
        }
    }

    fn paint_text_runs(&self, batched_runs: &[BatchedTextRun], window: &mut Window, cx: &mut App) {
        let origin = self.geometry.bounds.origin;
        let cell_width = self.geometry.cell_width;
        let line_height = self.geometry.line_height;
        let font_size = self.geometry.font_size;

        for batch in batched_runs {
            batch.paint(origin, cell_width, line_height, font_size, window, cx);
        }
    }

    fn paint_search_highlights(&self, window: &mut Window) {
        let origin = self.geometry.bounds.origin;
        let cell_width_f = f32::from(self.geometry.cell_width);
        let line_height = self.geometry.line_height;
        let display_offset = self.model.display_offset;
        let screen_lines = self.model.screen_lines;
        let p = SemanticPalette::from_theme(self.theme_colors);

        for (idx, search_match) in self.model.search_matches.iter().enumerate() {
            let visual_line = search_match.line + display_offset;
            if visual_line < 0 || visual_line >= screen_lines as i32 {
                continue;
            }

            let is_current = self.model.current_match_index == Some(idx);
            let highlight_color = if is_current {
                let mut c = p.editor_search_current;
                c.a = 0.7;
                c
            } else {
                let mut c = p.editor_search_match;
                c.a = 0.5;
                c
            };

            let position = point(
                px((f32::from(origin.x) + search_match.col as f32 * cell_width_f).floor()),
                origin.y + line_height * visual_line as f32,
            );
            let size = size(
                px((cell_width_f * search_match.len as f32).ceil()),
                line_height,
            );

            window.paint_quad(fill(Bounds::new(position, size), highlight_color));
        }
    }

    fn paint_urls(&self, window: &mut Window) {
        let origin = self.geometry.bounds.origin;
        let cell_width_f = f32::from(self.geometry.cell_width);
        let line_height = self.geometry.line_height;
        let screen_lines = self.model.screen_lines;
        let t = self.theme_colors;

        for url_match in self.model.url_matches.iter() {
            let is_hovered = self.model.hovered_url_group == Some(url_match.link_group);
            if url_match.line < 0 || url_match.line >= screen_lines as i32 {
                continue;
            }

            let url_x = px((f32::from(origin.x) + url_match.col as f32 * cell_width_f).floor());
            let url_y = origin.y + line_height * url_match.line as f32;
            let url_width = px((cell_width_f * url_match.len as f32).ceil());

            if is_hovered {
                let hover_bg = Hsla::from(Rgba {
                    r: 0.0,
                    g: 0.48,
                    b: 0.8,
                    a: 0.2,
                });
                let hover_bounds = Bounds {
                    origin: point(url_x, url_y),
                    size: size(url_width, line_height),
                };
                window.paint_quad(fill(hover_bounds, hover_bg));

                let underline_color = rgb(t.border_active);
                let underline_y = url_y + line_height - px(2.0);
                let underline_bounds = Bounds {
                    origin: point(url_x, underline_y),
                    size: size(url_width, px(1.0)),
                };
                window.paint_quad(fill(underline_bounds, underline_color));
            } else {
                let underline_color = Hsla::from(Rgba {
                    r: 0.5,
                    g: 0.5,
                    b: 0.5,
                    a: 0.5,
                });
                let underline_y = url_y + line_height - px(2.0);
                let underline_bounds = Bounds {
                    origin: point(url_x, underline_y),
                    size: size(url_width, px(1.0)),
                };
                window.paint_quad(fill(underline_bounds, underline_color));
            }
        }
    }

    fn paint_cursor(&self, window: &mut Window) {
        let cursor_point = match self.model.cursor_point {
            Some(p) => p,
            None => return,
        };

        let visual_line = cursor_point.line.0 + self.model.display_offset;
        if visual_line < 0 || visual_line >= self.model.screen_lines as i32 {
            return;
        }

        let origin = self.geometry.bounds.origin;
        let cell_width = self.geometry.cell_width;
        let line_height = self.geometry.line_height;

        let cursor_x = px((f32::from(origin.x) + cursor_point.column.0 as f32 * f32::from(cell_width)).floor());
        let cursor_y = px((f32::from(origin.y) + visual_line as f32 * f32::from(line_height)).floor());
        let cursor_rgba = rgb(self.palette.cursor.unwrap_or(self.palette.foreground));
        let cursor_color = Hsla::from(Rgba {
            r: cursor_rgba.r,
            g: cursor_rgba.g,
            b: cursor_rgba.b,
            a: 0.8,
        });

        match self.model.cursor_shape {
            CursorShape::Block => {
                let cursor_bounds = Bounds {
                    origin: point(cursor_x, cursor_y),
                    size: size(cell_width, line_height),
                };
                window.paint_quad(fill(cursor_bounds, cursor_color));
            }
            CursorShape::Bar => {
                let cursor_bounds = Bounds {
                    origin: point(cursor_x, cursor_y),
                    size: size(px(2.0), line_height),
                };
                window.paint_quad(fill(cursor_bounds, cursor_color));
            }
            CursorShape::Underline => {
                let underline_height = (f32::from(line_height) * 0.15).max(2.0);
                let cursor_bounds = Bounds {
                    origin: point(cursor_x, cursor_y + line_height - px(underline_height)),
                    size: size(cell_width, px(underline_height)),
                };
                window.paint_quad(fill(cursor_bounds, cursor_color));
            }
        }
    }
}
