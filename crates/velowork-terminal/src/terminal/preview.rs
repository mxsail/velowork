use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{Color, NamedColor};

pub use velowork_core::terminal_preview::{
    TerminalPreviewLine, TerminalPreviewSnapshot, TerminalPreviewSpan,
};

use super::Terminal;

impl Terminal {
    /// Capture a preview snapshot of the terminal's visible content.
    ///
    /// `max_lines` limits the maximum number of lines returned (e.g. 14~16 lines).
    /// Extracts the lines from the screen grid, preserving ANSI colors and attributes.
    pub fn preview_snapshot(&self, _max_lines: usize) -> TerminalPreviewSnapshot {
        self.with_content(|term| {
            let grid = term.grid();
            let screen_lines = grid.screen_lines();
            let cols = grid.columns();
            let default_fg = Color::Named(NamedColor::Foreground);
            let default_bg = Color::Named(NamedColor::Background);

            let mut all_lines: Vec<TerminalPreviewLine> = Vec::with_capacity(screen_lines);
            let mut first_non_empty: Option<usize> = None;
            let mut last_non_empty: Option<usize> = None;

            for row in 0..screen_lines as i32 {
                let mut spans: Vec<TerminalPreviewSpan> = Vec::new();
                let mut current_span: Option<TerminalPreviewSpan> = None;
                let mut has_text = false;

                let mut col_idx = 0usize;
                while col_idx < cols {
                    let cell = &grid[Point::new(Line(row), Column(col_idx))];

                    if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                        col_idx += 1;
                        continue;
                    }

                    let c = cell.c;
                    let is_space = c == '\0' || c == ' ';
                    if !is_space {
                        has_text = true;
                    }

                    let fg = if cell.fg == default_fg { None } else { Some(cell.fg) };
                    let bg = if cell.bg == default_bg { None } else { Some(cell.bg) };
                    let bold = cell.flags.contains(Flags::BOLD);
                    let dim = cell.flags.contains(Flags::DIM);
                    let italic = cell.flags.contains(Flags::ITALIC);
                    let underline = cell.flags.intersects(Flags::ALL_UNDERLINES);
                    let inverse = cell.flags.contains(Flags::INVERSE);

                    let ch = if c == '\0' { ' ' } else { c };

                    let can_merge = match &current_span {
                        Some(span) => {
                            span.fg == fg
                                && span.bg == bg
                                && span.bold == bold
                                && span.dim == dim
                                && span.italic == italic
                                && span.underline == underline
                                && span.inverse == inverse
                        }
                        None => false,
                    };

                    if can_merge {
                        if let Some(span) = current_span.as_mut() {
                            span.text.push(ch);
                        }
                    } else {
                        if let Some(prev) = current_span.take() {
                            spans.push(prev);
                        }
                        current_span = Some(TerminalPreviewSpan {
                            text: ch.to_string(),
                            fg,
                            bg,
                            bold,
                            dim,
                            italic,
                            underline,
                            inverse,
                        });
                    }

                    col_idx += 1;
                }

                if let Some(span) = current_span.take() {
                    spans.push(span);
                }

                // Right-trim trailing whitespace with default styling
                while let Some(last) = spans.last_mut() {
                    let trimmed = last.text.trim_end_matches(' ');
                    if trimmed.is_empty() && last.bg.is_none() {
                        spans.pop();
                    } else {
                        if trimmed.len() < last.text.len() && last.bg.is_none() {
                            last.text = trimmed.to_string();
                        }
                        break;
                    }
                }

                let line_idx = all_lines.len();
                if has_text {
                    if first_non_empty.is_none() {
                        first_non_empty = Some(line_idx);
                    }
                    last_non_empty = Some(line_idx);
                }

                all_lines.push(TerminalPreviewLine { spans });
            }

            let (lines, is_empty) = match (first_non_empty, last_non_empty) {
                (Some(_), Some(last)) => {
                    // Return all lines from top of screen up to last non-empty line (or full screen_lines)
                    let end = (last + 1).max(1).min(screen_lines);
                    let selected = all_lines[0..end].to_vec();
                    if selected.is_empty() {
                        (Vec::new(), true)
                    } else {
                        (selected, false)
                    }
                }
                _ => (Vec::new(), true),
            };

            TerminalPreviewSnapshot {
                lines,
                cols,
                rows: screen_lines,
                is_empty,
            }
        })
    }
}
