//! Terminal content component.

use crate::elements::terminal_element::{
    deregister_resize_viewer as deregister_shared_resize_viewer, next_resize_viewer_id, LinkKind,
    SearchMatch, TerminalElement,
};
use crate::terminal_view_settings;
use velowork_terminal::terminal::Terminal;
use velowork_ui::theme::{bg_opacity, theme, with_alpha};
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::tokens::{SPACE_XS, SPACE_SM};
use velowork_workspace::state::{WindowId, Workspace};
use velowork_i18n::i18n;
use gpui::*;
use gpui::prelude::FluentBuilder;
use std::sync::Arc;
use std::time::{Duration, Instant};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};

use super::scrollbar::Scrollbar;
use super::url_detector::UrlDetector;

/// Events emitted by terminal content.
pub enum TerminalContentEvent {
    RequestContextMenu {
        position: Point<Pixels>,
        has_selection: bool,
        link_url: Option<String>,
    },
    ShowAiFloatingToolbar {
        position: Point<Pixels>,
        selection_text: String,
    },
    DismissAiFloatingToolbar,
}

/// Terminal content view handling display and mouse interactions.
pub struct TerminalContent {
    terminal: Option<Arc<Terminal>>,
    resize_viewer_id: u64,
    focus_handle: FocusHandle,
    url_detector: UrlDetector,
    scrollbar: Entity<Scrollbar>,
    is_selecting: bool,
    element_bounds: Option<Bounds<Pixels>>,
    last_click: Option<(Instant, usize, i32)>,
    click_count: u8,
    cursor_visible: bool,
    search_matches: Arc<Vec<SearchMatch>>,
    search_current_index: Option<usize>,
    project_id: String,
    layout_path: Vec<usize>,
    _window_id: Option<WindowId>,
    workspace: Entity<Workspace>,
    scroll_accumulator: f32,
    scroll_x: f32,
    mouse_down_cell: Option<(usize, i32)>,
    forwarded_button: Option<(u8, u8)>,
    is_connection_lost: bool,
    resolved_config: Option<velowork_terminal::ResolvedTerminalConfig>,
    bottom_left_radius: f32,
    bottom_right_radius: f32,
    /// Runs while a drag-selection is active, scrolling the viewport when the
    /// pointer is held past the top/bottom edge so selection can extend beyond
    /// the visible area (issue #132).
    autoscroll_task: Option<Task<()>>,
}

impl TerminalContent {
    pub fn new(
        focus_handle: FocusHandle,
        window_id: Option<WindowId>,
        project_id: String,
        layout_path: Vec<usize>,
        workspace: Entity<Workspace>,
        cx: &mut Context<Self>,
    ) -> Self {
        let scrollbar = cx.new(Scrollbar::new);
        Self {
            terminal: None,
            resize_viewer_id: next_resize_viewer_id(),
            focus_handle,
            url_detector: UrlDetector::new(),
            scrollbar,
            is_selecting: false,
            element_bounds: None,
            last_click: None,
            click_count: 0,
            cursor_visible: true,
            search_matches: Arc::new(Vec::new()),
            search_current_index: None,
            project_id,
            layout_path,
            _window_id: window_id,
            workspace,
            scroll_accumulator: 0.0,
            scroll_x: 0.0,
            mouse_down_cell: None,
            forwarded_button: None,
            is_connection_lost: false,
            resolved_config: None,
            bottom_left_radius: 0.0,
            bottom_right_radius: 0.0,
            autoscroll_task: None,
        }
    }

    pub fn set_bottom_corner_radii(&mut self, bl: f32, br: f32) {
        self.bottom_left_radius = bl;
        self.bottom_right_radius = br;
    }

    pub fn set_resolved_config(&mut self, config: velowork_terminal::ResolvedTerminalConfig) {
        self.resolved_config = Some(config);
    }

    pub fn effective_config(&self, cx: &App) -> velowork_terminal::ResolvedTerminalConfig {
        if let Some(ref config) = self.resolved_config {
            config.clone()
        } else {
            let defaults = crate::terminal_view_settings(cx).terminal_defaults();
            let default_opts = velowork_state::SessionTerminalOptions::default();
            velowork_terminal::resolve_effective_terminal_config(&default_opts, &defaults)
        }
    }

    fn mouse_modifier_bits(m: &Modifiers) -> u8 {
        let mut bits = 0u8;
        if m.shift {
            bits |= 4;
        }
        if m.alt {
            bits |= 8;
        }
        if m.control {
            bits |= 16;
        }
        bits
    }

    /// Forward a button press to the PTY if the app has mouse mode enabled.
    /// `button_code` is 0=left, 1=middle, 2=right. Returns true if forwarded.
    fn try_forward_mouse_press(
        &mut self,
        button_code: u8,
        event_position: Point<Pixels>,
        modifiers: &Modifiers,
    ) -> bool {
        let Some(terminal) = self.terminal.as_ref() else {
            return false;
        };
        if !terminal.is_mouse_mode() {
            return false;
        }
        // Shift bypasses mouse reporting so users can still select text
        // in apps like nano/tmux that capture mouse. Matches xterm/iTerm2/WezTerm.
        if modifiers.shift {
            return false;
        }
        let Some((col, row, _)) = self.pixel_to_cell(event_position, false) else {
            return false;
        };
        let mods = Self::mouse_modifier_bits(modifiers);
        terminal.send_mouse_button(button_code, true, col, row as usize, mods);
        self.forwarded_button = Some((button_code, mods));
        self.mouse_down_cell = None;
        self.is_selecting = false;
        true
    }

    /// Forward a button release to the PTY if that button's press was forwarded.
    /// Returns true if a release was sent (or the forwarded state was cleared).
    fn try_forward_mouse_release(
        &mut self,
        button_code: u8,
        event_position: Point<Pixels>,
        modifiers: &Modifiers,
    ) -> bool {
        let Some((forwarded, _)) = self.forwarded_button else {
            return false;
        };
        if forwarded != button_code {
            return false;
        }
        if let Some(terminal) = self.terminal.as_ref()
            && let Some((col, row, _)) = self.pixel_to_cell(event_position, false) {
                let mods = Self::mouse_modifier_bits(modifiers);
                terminal.send_mouse_button(button_code, false, col, row as usize, mods);
            }
        self.forwarded_button = None;
        self.mouse_down_cell = None;
        true
    }

    pub fn set_terminal(&mut self, terminal: Option<Arc<Terminal>>, cx: &mut Context<Self>) {
        if let Some(old_terminal) = self.terminal.as_ref() {
            let next_id = terminal.as_ref().map(|terminal| terminal.terminal_id.as_str());
            if next_id != Some(old_terminal.terminal_id.as_str()) {
                deregister_shared_resize_viewer(&old_terminal.terminal_id, self.resize_viewer_id);
                old_terminal.remove_focus_reporter(self.resize_viewer_id);
            }
        }
        self.terminal = terminal.clone();
        self.scrollbar.update(cx, |scrollbar, _| {
            scrollbar.set_terminal(terminal);
        });
        // The pane renders this view through GPUI's `.cached()` element, which
        // only re-renders when the entity is dirty. Without this notify the
        // terminal area keeps replaying the *previous* terminal's painted frame
        // until some unrelated update lands, showing a stale screen for one or
        // more frames right after a session connects.
        cx.notify();
    }

    pub(crate) fn deregister_resize_viewer(&mut self) {
        if let Some(terminal) = self.terminal.as_ref() {
            deregister_shared_resize_viewer(&terminal.terminal_id, self.resize_viewer_id);
        }
    }

    fn deregister_focus_reporter(&mut self) {
        if let Some(terminal) = self.terminal.as_ref() {
            terminal.remove_focus_reporter(self.resize_viewer_id);
        }
    }

    pub fn set_connection_lost(&mut self, connection_lost: bool) {
        if self.is_connection_lost != connection_lost {
            self.is_connection_lost = connection_lost;
        }
    }

    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.cursor_visible = visible;
    }

    pub fn set_search_highlights(
        &mut self,
        matches: Arc<Vec<SearchMatch>>,
        current_index: Option<usize>,
    ) {
        self.search_matches = matches;
        self.search_current_index = current_index;
    }

    /// Calculate the maximum horizontal scroll offset in pixels when wrap mode is NoWrap.
    ///
    /// If all content fits within the visible bounds (or wrap mode is not NoWrap),
    /// this returns 0.0, preventing horizontal scrolling when content does not exceed
    /// the visible range.
    pub fn max_scroll_x(&self, cx: &App) -> f32 {
        let tvs = crate::terminal_view_settings(cx);
        if tvs.wrap_mode != velowork_workspace::settings::WrapMode::NoWrap {
            return 0.0;
        }

        let Some(ref terminal) = self.terminal else {
            return 0.0;
        };
        let Some(bounds) = self.element_bounds else {
            return 0.0;
        };

        let available_width = f32::from(bounds.size.width);
        if available_width <= 0.0 {
            return 0.0;
        }

        let (cell_width, _) = terminal.cell_dimensions();
        if cell_width <= 0.0 {
            return 0.0;
        }

        let max_content_col = terminal.with_content(|term| {
            let grid = term.grid();
            let cols = grid.columns();
            let screen_lines = grid.screen_lines();
            if cols == 0 || screen_lines == 0 {
                return 0;
            }
            let display_offset = grid.display_offset() as i32;

            let mut max_col = 0usize;
            for row in 0..screen_lines {
                let visual_line = row as i32;
                let buffer_line = Line(visual_line - display_offset);
                for col in (0..cols).rev() {
                    let cell = &grid[alacritty_terminal::index::Point::new(buffer_line, Column(col))];
                    let has_char = cell.c != ' ' && cell.c != '\0';
                    let has_custom_bg = !matches!(
                        cell.bg,
                        alacritty_terminal::vte::ansi::Color::Named(
                            alacritty_terminal::vte::ansi::NamedColor::Background
                        )
                    );
                    if has_char || has_custom_bg {
                        max_col = max_col.max(col + 1);
                        break;
                    }
                }
            }

            if display_offset == 0 {
                max_col = max_col.max(grid.cursor.point.column.0 + 1);
            }

            max_col
        });

        compute_max_scroll_x(max_content_col, cell_width, available_width)
    }

    pub fn mark_scroll_activity(&mut self, cx: &mut Context<Self>) {
        self.scrollbar.update(cx, |scrollbar, cx| {
            scrollbar.mark_activity(cx);
        });
    }

    pub fn handle_scroll(
        &mut self,
        delta: f32,
        position: Point<Pixels>,
        shift: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(ref terminal) = self.terminal {
            let (cell_width, cell_height) = terminal.cell_dimensions();

            if terminal.is_mouse_mode() && !shift {
                self.scroll_accumulator += delta;
                let lines = (self.scroll_accumulator / cell_height) as i32;
                if lines != 0 {
                    self.scroll_accumulator -= lines as f32 * cell_height;
                    let (col, row) = self.pixel_to_cell_raw(position, cell_width, cell_height);
                    let button = if lines > 0 { 64u8 } else { 65u8 };
                    terminal.send_mouse_scroll(button, col, row, lines.unsigned_abs() as usize);
                }
            } else {
                self.scroll_accumulator += delta;
                let lines = (self.scroll_accumulator / cell_height) as i32;
                if lines != 0 {
                    self.scroll_accumulator -= lines as f32 * cell_height;
                    if lines > 0 {
                        terminal.scroll_up(lines);
                    } else {
                        terminal.scroll_down(-lines);
                    }
                }
            }
            self.mark_scroll_activity(cx);
            cx.notify();
        }
    }

    pub fn update_scrollbar_drag(&mut self, y: f32, cx: &mut Context<Self>) {
        if let Some(bounds) = self.element_bounds {
            let content_height = f32::from(bounds.size.height);
            self.scrollbar.update(cx, |scrollbar, cx| {
                scrollbar.update_drag(y, content_height, cx);
            });
        }
    }

    pub fn end_scrollbar_drag(&mut self, cx: &mut Context<Self>) {
        self.scrollbar.update(cx, |scrollbar, cx| {
            scrollbar.end_drag(cx);
        });
    }

    const TERMINAL_PADDING: f32 = 4.0;

    fn pixel_to_cell(&self, pos: Point<Pixels>, is_no_wrap: bool) -> Option<(usize, i32, alacritty_terminal::index::Side)> {
        let bounds = self.element_bounds?;
        let terminal = self.terminal.as_ref()?;
        let (cell_width, cell_height) = terminal.cell_dimensions();
        let x_offset = if is_no_wrap { self.scroll_x } else { 0.0 };

        let x = (f32::from(pos.x) - f32::from(bounds.origin.x) + x_offset - Self::TERMINAL_PADDING).max(0.0);
        let y = (f32::from(pos.y) - f32::from(bounds.origin.y) - Self::TERMINAL_PADDING).max(0.0);

        let col_exact = x / cell_width;
        let col = col_exact.floor() as usize;
        let row = (y / cell_height).floor() as i32;

        let size = terminal.resize_state.lock();
        let col = col.min(size.size.cols.saturating_sub(1) as usize);
        let row = row.min(size.size.rows.saturating_sub(1) as i32);

        let side = if col_exact.fract() < 0.5 {
            alacritty_terminal::index::Side::Left
        } else {
            alacritty_terminal::index::Side::Right
        };

        Some((col, row, side))
    }

    /// Calculate the current cursor's screen position or relative position.
    pub fn cursor_pixel_position(&self) -> Option<Point<Pixels>> {
        let terminal = self.terminal.as_ref()?;
        let bounds = self.element_bounds?;
        let (cell_width, cell_height) = terminal.cell_dimensions();
        terminal.with_content(|term| {
            let cursor_point = term.grid().cursor.point;
            let display_offset = term.grid().display_offset();
            let cursor_visual_line = cursor_point.line.0 + display_offset as i32;
            if cursor_visual_line >= 0 {
                let x = px(f32::from(bounds.origin.x) + cursor_point.column.0 as f32 * cell_width + Self::TERMINAL_PADDING);
                let y = px(f32::from(bounds.origin.y) + cursor_visual_line as f32 * cell_height + Self::TERMINAL_PADDING + cell_height);
                Some(point(x, y))
            } else {
                None
            }
        })
    }

    /// Calculate cursor position relative to this content container.
    pub fn relative_cursor_position(&self, cx: &App) -> Option<Point<Pixels>> {
        let terminal = self.terminal.as_ref()?;
        let (cell_width, cell_height) = terminal.cell_dimensions();
        let render_settings = crate::terminal_view_settings(cx);
        let effective_config = self.effective_config(cx);
        let zoom_level = self.workspace.read(cx).get_terminal_zoom(&self.project_id, &self.layout_path);
        let font_sz = effective_config.font_size * zoom_level * velowork_ui::tokens::ui_scale_factor(cx);
        let fallback_w = (font_sz * 0.6).max(1.0);
        let fallback_h = (font_sz * render_settings.line_height).max(1.0);
        let actual_cell_w = if cell_width > 0.0 { cell_width } else { fallback_w };
        let actual_cell_h = if cell_height > 0.0 { cell_height } else { fallback_h };

        let gutter_offset = if render_settings.show_line_numbers {
            let max_line_num = terminal.with_content(|term| term.grid().screen_lines());
            let max_digits = max_line_num.to_string().len().max(3);
            (max_digits as f32 * font_sz * 0.6 + 16.0).max(36.0)
        } else {
            0.0
        };

        terminal.with_content(|term| {
            let cursor_point = term.grid().cursor.point;
            let display_offset = term.grid().display_offset();
            let cursor_visual_line = cursor_point.line.0 + display_offset as i32;
            if cursor_visual_line >= 0 {
                let x = px(gutter_offset + Self::TERMINAL_PADDING + cursor_point.column.0 as f32 * actual_cell_w);
                let y = px(Self::TERMINAL_PADDING + cursor_visual_line as f32 * actual_cell_h + actual_cell_h);
                Some(point(x, y))
            } else {
                None
            }
        })
    }

    /// Calculate input start position (aligned with first character of current input buffer)
    pub fn relative_input_start_position(&self, input_chars_count: usize) -> Option<Point<Pixels>> {
        let terminal = self.terminal.as_ref()?;
        let (cell_width, cell_height) = terminal.cell_dimensions();
        terminal.with_content(|term| {
            let cursor_point = term.grid().cursor.point;
            let display_offset = term.grid().display_offset();
            let cursor_visual_line = cursor_point.line.0 + display_offset as i32;
            if cursor_visual_line >= 0 {
                let start_col = cursor_point.column.0.saturating_sub(input_chars_count);
                let x = px(start_col as f32 * cell_width + Self::TERMINAL_PADDING);
                let y = px(cursor_visual_line as f32 * cell_height + Self::TERMINAL_PADDING + cell_height);
                Some(point(x, y))
            } else {
                None
            }
        })
    }

    fn cell_at(&self, pos: Point<Pixels>, cx: &App) -> Option<(usize, i32, alacritty_terminal::index::Side)> {
        let tvs = crate::terminal_view_settings(cx);
        let is_no_wrap = tvs.wrap_mode == velowork_workspace::settings::WrapMode::NoWrap;
        self.pixel_to_cell(pos, is_no_wrap)
    }

    fn pixel_to_cell_raw(&self, pos: Point<Pixels>, cell_width: f32, cell_height: f32) -> (usize, usize) {
        if let Some(bounds) = self.element_bounds {
            let x = (f32::from(pos.x) - f32::from(bounds.origin.x) + self.scroll_x).max(0.0);
            let y = (f32::from(pos.y) - f32::from(bounds.origin.y)).max(0.0);
            ((x / cell_width) as usize, (y / cell_height) as usize)
        } else {
            (0, 0)
        }
    }

    fn handle_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);

        let Some((col, row, side)) = self.cell_at(event.position, cx) else {
            return;
        };
        self.mouse_down_cell = Some((col, row));

        if event.modifiers.platform || event.modifiers.control {
            if let Some(uri) = self.terminal.as_ref().and_then(|t| t.hyperlink_at(col, row)) {
                UrlDetector::open_url(&uri);
                self.mouse_down_cell = None;
                return;
            }
            if let Some(url_match) = self.url_detector.find_at(col, row) {
                match &url_match.kind {
                    LinkKind::Url => {
                        UrlDetector::open_url(&url_match.url);
                    }
                    LinkKind::FilePath { line, col } => {
                        let file_opener = terminal_view_settings(cx).file_opener.clone();
                        UrlDetector::open_file(&url_match.url, *line, *col, &file_opener);
                    }
                }
                self.mouse_down_cell = None;
                return;
            }
        }

        if self.try_forward_mouse_press(0, event.position, &event.modifiers) {
            cx.notify();
            return;
        }

        let now = Instant::now();

        let click_count = if let Some((last_time, last_col, last_row)) = self.last_click {
            let elapsed = now.duration_since(last_time).as_millis();
            let same_position =
                (col as i32 - last_col as i32).abs() <= 1 && (row - last_row).abs() <= 0;
            if elapsed < 400 && same_position {
                if self.click_count >= 3 { 1 } else { self.click_count + 1 }
            } else {
                1
            }
        } else {
            1
        };

        self.last_click = Some((now, col, row));
        self.click_count = click_count;

        let Some(terminal) = self.terminal.as_ref() else {
            return;
        };
        terminal.clear_selection();
        cx.emit(TerminalContentEvent::DismissAiFloatingToolbar);

        match click_count {
            2 => {
                terminal.start_word_selection(col, row);
                self.is_selecting = false;
            }
            3 => {
                terminal.start_line_selection(col, row);
                self.is_selecting = false;
            }
            _ => {
                terminal.start_selection(col, row, side);
                self.is_selecting = true;
            }
        }
        if self.is_selecting {
            self.start_autoscroll(window, cx);
        }
        cx.notify();
    }

    /// Spawn the drag-selection auto-scroll loop. It keeps the viewport
    /// scrolling toward the pointer while the user holds the mouse past the
    /// terminal's top/bottom edge — including when the pointer leaves the
    /// element entirely, where `on_mouse_move` no longer fires (issue #132).
    fn start_autoscroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.autoscroll_task = Some(cx.spawn_in(window, async move |this, cx| {
            let interval = Duration::from_millis(33);
            loop {
                smol::Timer::after(interval).await;
                match this.update_in(cx, |this, window, cx| this.autoscroll_tick(window, cx)) {
                    Ok(true) => {}
                    _ => break,
                }
            }
        }));
    }

    /// One tick of the auto-scroll loop. Returns `false` to stop the loop.
    /// Scrolls (and extends the selection) only while the pointer is past an
    /// edge; the in-bounds case is already handled by `handle_mouse_move`.
    fn autoscroll_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.is_selecting {
            return false;
        }
        let Some(bounds) = self.element_bounds else {
            return false;
        };
        let Some(terminal) = self.terminal.clone() else {
            return false;
        };

        let position = window.mouse_position();
        let (_cell_width, cell_height) = terminal.cell_dimensions();

        let top = f32::from(bounds.origin.y) + Self::TERMINAL_PADDING;
        let bottom =
            f32::from(bounds.origin.y) + f32::from(bounds.size.height) - Self::TERMINAL_PADDING;
        let y = f32::from(position.y);

        let lines = autoscroll_lines(y, top, bottom, cell_height);
        if lines == 0 {
            return true;
        }

        if lines > 0 {
            terminal.scroll_up(lines);
        } else if lines < 0 {
            terminal.scroll_down(-lines);
        }

        if let Some((col, row, side)) = self.cell_at(position, cx) {
            terminal.update_selection(col, row, side);
        }
        self.mark_scroll_activity(cx);
        cx.notify();
        true
    }

    fn handle_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if let Some((col, row, _side)) = self.cell_at(event.position, cx) {
            if self.url_detector.update_hover(col, row) {
                cx.notify();
            }
        } else if self.url_detector.clear_hover() {
            cx.notify();
        }

        if let Some((button, mods)) = self.forwarded_button {
            if let Some(ref terminal) = self.terminal
                && terminal.supports_mouse_drag()
                    && let Some((col, row, _side)) = self.cell_at(event.position, cx) {
                        terminal.send_mouse_drag(button, col, row as usize, mods);
                    }
            return;
        }

        if self.is_selecting {
            if event.pressed_button != Some(MouseButton::Left) {
                if let Some(ref terminal) = self.terminal {
                    terminal.end_selection();
                    if !terminal.has_selection()
                        || terminal.get_selected_text().map(|s| s.is_empty()).unwrap_or(true)
                    {
                        terminal.clear_selection();
                    }
                }
                self.is_selecting = false;
                cx.notify();
                return;
            }

            if let Some(ref terminal) = self.terminal
                && let Some((col, row, side)) = self.cell_at(event.position, cx) {
                    terminal.update_selection(col, row, side);
                    cx.notify();
                }
        }
    }

    fn handle_mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        if self.try_forward_mouse_release(0, event.position, &event.modifiers) {
            cx.notify();
            return;
        }

        if self.is_selecting
            && let Some(ref terminal) = self.terminal {
                terminal.end_selection();
                self.is_selecting = false;

                let empty_selection = !terminal.has_selection()
                    || terminal.get_selected_text().map(|s| s.is_empty()).unwrap_or(true);

                if empty_selection {
                    terminal.clear_selection();
                    cx.emit(TerminalContentEvent::DismissAiFloatingToolbar);

                    // Click-to-cursor: on a clean single click (no drag), move cursor
                    if self.click_count == 1
                        && let Some((col, row)) = self.mouse_down_cell.take()
                            && !terminal.is_mouse_mode() && !terminal.is_alt_screen() && !terminal.has_running_child() {
                                terminal.move_cursor_to_click(col, row);
                            }
                }
                cx.notify();
            }

        // Sync any non-empty selection to PRIMARY so middle-click paste works
        // for drag, double-click (word), and triple-click (line) selections.
        if let Some(ref terminal) = self.terminal
            && let Some(text) = terminal.get_selected_text()
                && !text.is_empty() {
                    #[cfg(target_os = "linux")]
                    cx.write_to_primary(ClipboardItem::new_string(text.clone()));

                    let tvs = crate::terminal_view_settings(cx);
                    if tvs.terminal_copy_on_select {
                        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                    }

                    if tvs.ai_enabled && tvs.terminal_ai_floating_toolbar_enabled {
                        let trimmed = text.trim();
                        if trimmed.chars().count() >= 2 {
                            cx.emit(TerminalContentEvent::ShowAiFloatingToolbar {
                                position: event.position,
                                selection_text: text,
                            });
                        }
                    }
                }

        self.mouse_down_cell = None;
    }
}

impl Render for TerminalContent {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let is_focused = self.focus_handle.is_focused(window);

        if let Some(ref terminal) = self.terminal {
            terminal.update_focus_reporter(self.resize_viewer_id, is_focused);
        }

        let settings = crate::terminal_view_settings(cx);
        let effective_config = self.effective_config(cx);

        if let Some(ref terminal) = self.terminal {
            terminal.set_palette(velowork_core::theme::get_terminal_palette_with_custom(
                &effective_config.color_scheme,
                &settings.custom_terminal_color_schemes,
            ));
            for text in terminal.take_pending_clipboard_writes() {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
        }
        let bg_tint = if settings.color_tinted_background {
            let ws = self.workspace.read(cx);
            ws.project(&self.project_id).and_then(|p| {
                let color = ws.effective_folder_color(p);
                if color != velowork_core::theme::FolderColor::Default {
                    Some(t.get_folder_color(color))
                } else {
                    None
                }
            })
        } else {
            None
        };

        self.url_detector.update_matches(&self.terminal);

        let Some(ref terminal) = self.terminal else {
            return div()
                .flex_1()
                .min_h(px(200.0))
                .flex()
                .items_center()
                .justify_center()
                .text_color(p.text_muted)
                .child(i18n!(cx, "terminal.creating"))
                .into_any_element();
        };

        let terminal_clone = terminal.clone();
        let focus_handle = self.focus_handle.clone();
        let zoom_level = self.workspace.read(cx).get_terminal_zoom(&self.project_id, &self.layout_path);

        let element_bounds_setter = {
            let entity = cx.entity().downgrade();
            move |bounds: Bounds<Pixels>, _window: &mut Window, cx: &mut App| {
                if let Some(entity) = entity.upgrade() {
                    entity.update(cx, |this, cx| {
                        this.element_bounds = Some(bounds);
                        let max_scroll = this.max_scroll_x(cx);
                        if this.scroll_x > max_scroll {
                            this.scroll_x = max_scroll;
                        }
                    });
                }
            }
        };

        let render_settings = crate::terminal_view_settings(cx);
        if render_settings.wrap_mode != velowork_workspace::settings::WrapMode::NoWrap {
            self.scroll_x = 0.0;
        } else {
            let max_scroll = self.max_scroll_x(cx);
            if self.scroll_x > max_scroll {
                self.scroll_x = max_scroll;
            }
        }
        let effective_config = self.effective_config(cx);
        let active_color_scheme = &effective_config.color_scheme;
        let term_palette = velowork_core::theme::get_terminal_palette_with_custom(
            active_color_scheme,
            &render_settings.custom_terminal_color_schemes,
        );
        let effective_bg = if let Some(tint) = bg_tint {
            velowork_ui::color_utils::tint_color(term_palette.background, tint, 0.18)
        } else {
            term_palette.background
        };
        let image_set = render_settings
            .terminal_background_image
            .as_ref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let base_alpha = if image_set { 0.0 } else { bg_opacity(cx) };
        let pane_bg = if base_alpha > 0.0 {
            with_alpha(effective_bg, base_alpha)
        } else {
            transparent_black()
        };

        div()
            .id("terminal-content")
            .size_full()
            .min_h_0()
            .relative()
            .bg(pane_bg)
            .when(self.bottom_left_radius > 0.0, |d| {
                d.rounded_bl(px(self.bottom_left_radius))
            })
            .when(self.bottom_right_radius > 0.0, |d| {
                d.rounded_br(px(self.bottom_right_radius))
            })
            .cursor(CursorStyle::Arrow)
            .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                let hovered = *hovered;
                this.scrollbar.update(cx, |scrollbar, cx| {
                    scrollbar.set_content_hovered(hovered, cx);
                });
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    if this.scrollbar.read(cx).is_dragging() {
                        this.end_scrollbar_drag(cx);
                        return;
                    }
                    this.handle_mouse_down(event, window, cx);
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                if this.scrollbar.read(cx).is_dragging() {
                    this.update_scrollbar_drag(f32::from(event.position.y), cx);
                    return;
                }
                this.handle_mouse_move(event, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _window, cx| {
                    if this.scrollbar.read(cx).is_dragging() {
                        this.end_scrollbar_drag(cx);
                        return;
                    }
                    this.handle_mouse_up(event, cx);
                }),
            )
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _window, cx| {
                cx.emit(TerminalContentEvent::DismissAiFloatingToolbar);
                let delta = event.delta.pixel_delta(px(17.0));
                let tvs = crate::terminal_view_settings(cx);
                if tvs.wrap_mode == velowork_workspace::settings::WrapMode::NoWrap
                    && (delta.x != px(0.0) || (event.modifiers.shift && delta.y != px(0.0)))
                {
                    let max_scroll_x = this.max_scroll_x(cx);
                    if max_scroll_x <= 0.0 {
                        if this.scroll_x != 0.0 {
                            this.scroll_x = 0.0;
                            cx.notify();
                        }
                        return;
                    }
                    let delta_val = if event.modifiers.shift { f32::from(delta.y) } else { f32::from(delta.x) };
                    let new_scroll_x = (this.scroll_x - delta_val).clamp(0.0, max_scroll_x);
                    if (new_scroll_x - this.scroll_x).abs() > 0.001 {
                        this.scroll_x = new_scroll_x;
                        cx.notify();
                    }
                    return;
                }
                if event.modifiers.shift {
                    return;
                }
                if event.modifiers.control {
                    let mut tvs = crate::terminal_view_settings(cx).clone();
                    let delta_y = f32::from(delta.y);
                    let size_delta = if delta_y > 0.0 { 1.0 } else { -1.0 };
                    let new_font_size = (tvs.font_size + size_delta).clamp(8.0, 48.0);
                    if (new_font_size - tvs.font_size).abs() >= 0.01 {
                        tvs.font_size = new_font_size;
                        crate::set_terminal_view_settings(&tvs, cx);
                        cx.notify();
                    }
                } else {
                    this.handle_scroll(f32::from(delta.y), event.position, event.modifiers.shift, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus_handle, cx);
                    if this.try_forward_mouse_press(2, event.position, &event.modifiers) {
                        cx.notify();
                        return;
                    }
                    let tvs = crate::terminal_view_settings(cx);
                    if tvs.terminal_right_click_paste && !event.modifiers.shift {
                        cx.emit(TerminalContentEvent::DismissAiFloatingToolbar);
                        if let Some(terminal) = &this.terminal {
                            let item = cx.read_from_clipboard();
                            if let Some(text) = item.and_then(|i| i.text()) {
                                terminal.send_paste(&text);
                            }
                        }
                        return;
                    }
                    cx.emit(TerminalContentEvent::DismissAiFloatingToolbar);
                    let has_selection = this.terminal.as_ref().map(|t| t.has_selection()).unwrap_or(false);
                    let link_url = this.cell_at(event.position, cx).and_then(|(col, row, _side)| {
                        this.url_detector.find_at(col, row)
                            .filter(|m| m.kind == LinkKind::Url)
                            .map(|m| m.url)
                    });
                    cx.emit(TerminalContentEvent::RequestContextMenu {
                        position: event.position,
                        has_selection,
                        link_url,
                    });
                }),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|this, event: &MouseUpEvent, _window, cx| {
                    if this.try_forward_mouse_release(2, event.position, &event.modifiers) {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                    if this.try_forward_mouse_press(1, event.position, &event.modifiers) {
                        cx.notify();
                    } else {
                        cx.emit(TerminalContentEvent::DismissAiFloatingToolbar);
                        #[cfg(target_os = "linux")]
                        if let Some(ref terminal) = this.terminal
                            && let Some(item) = cx.read_from_primary()
                            && let Some(text) = item.text()
                            && !text.is_empty()
                        {
                            terminal.send_paste(&text);
                        }
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|this, event: &MouseUpEvent, _window, cx| {
                    if this.try_forward_mouse_release(1, event.position, &event.modifiers) {
                        cx.notify();
                    }
                }),
            )
            .child({
                let gutter_el = render_line_numbers_gutter(&terminal, zoom_level, self.element_bounds, cx);
                div()
                    .size_full()
                    .flex()
                    .flex_row()
                    .when_some(gutter_el, |el, g| el.child(g))
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .p(SPACE_XS)
                            .relative()
                            .child(canvas(element_bounds_setter, |_, _, _, _| {}).absolute().size_full())
                            .child(
                                TerminalElement::new(terminal_clone, focus_handle, self.resize_viewer_id)
                                    .with_bottom_corner_radii(0.0, 0.0)
                                    .with_zoom(zoom_level)
                                    .with_scroll_x(self.scroll_x)
                                    .with_bg_tint(bg_tint)
                                    .with_search(self.search_matches.clone(), self.search_current_index)
                                    .with_urls(
                                        self.url_detector.matches_arc(),
                                        self.url_detector.hovered_group(),
                                    )
                                    .with_cursor_visible(self.cursor_visible && !self.is_connection_lost)
                                    .with_cursor_style(effective_config.cursor_shape)
                                    .with_font_family(effective_config.font_family.clone())
                                    .with_font_size(effective_config.font_size)
                                    .with_color_scheme(effective_config.color_scheme.clone()),
                            ),
                    )
            })
            .child(self.scrollbar.clone())
            .into_any_element()
    }
}

impl Drop for TerminalContent {
    fn drop(&mut self) {
        self.deregister_resize_viewer();
        self.deregister_focus_reporter();
    }
}

impl EventEmitter<TerminalContentEvent> for TerminalContent {}

/// Compute the maximum horizontal scroll offset in pixels for a given content width.
///
/// Returns 0.0 if the content fits within the available width, preventing rightward
/// horizontal scrolling into blank space.
pub(crate) fn compute_max_scroll_x(
    max_content_col: usize,
    cell_width: f32,
    available_width: f32,
) -> f32 {
    let content_width = max_content_col as f32 * cell_width;
    if content_width <= available_width {
        0.0
    } else {
        content_width - available_width + cell_width
    }
}

/// Render terminal line number gutter element.
/// Reused across `TerminalContent`, tab restore ghost cards, and standalone pane restore cards.
pub fn render_line_numbers_gutter(
    terminal: &Arc<Terminal>,
    zoom_level: f32,
    element_bounds: Option<Bounds<Pixels>>,
    cx: &App,
) -> Option<Div> {
    let render_settings = crate::terminal_view_settings(cx);
    if !render_settings.show_line_numbers {
        return None;
    }

    let rows = terminal.resize_state.lock().size.rows;
    let (line_labels, is_alt_screen, max_line_num) = terminal.with_content(|term| {
        if term.mode().contains(alacritty_terminal::term::TermMode::ALT_SCREEN) {
            return (Vec::new(), true, 0usize);
        }

        let grid = term.grid();
        let offset = grid.display_offset() as i32;
        let cols = grid.columns();
        let screen_lines = grid.screen_lines() as i32;

        let topmost = grid.topmost_line().0;

        let cursor_line = grid.cursor.point.line.0;
        let mut last_content_line = cursor_line.max(0);
        for row_idx in (0..screen_lines).rev() {
            let mut has_content = false;
            for col in 0..cols {
                let c = grid[alacritty_terminal::index::Point::new(
                    Line(row_idx),
                    Column(col),
                )]
                .c;
                if c != ' ' && c != '\0' {
                    has_content = true;
                    break;
                }
            }
            if has_content {
                last_content_line = last_content_line.max(row_idx);
                break;
            }
        }
        let last_active_line = last_content_line.max(cursor_line);

        let mut current_logical_line = 1usize;
        let mut labels = Vec::with_capacity(screen_lines as usize);
        let mut max_line = 1usize;

        for l in topmost..=last_active_line {
            let is_continuation = if l > topmost {
                let prev_line = Line(l - 1);
                let last_col = Column(cols - 1);
                let cell = &grid[alacritty_terminal::index::Point::new(prev_line, last_col)];
                cell.flags.contains(alacritty_terminal::term::cell::Flags::WRAPLINE)
            } else {
                false
            };

            if !is_continuation {
                current_logical_line += 1;
            }

            let visual_row = l + offset;
            if visual_row >= 0 && visual_row < screen_lines {
                let num = (current_logical_line - 1).max(1);
                let label = if is_continuation {
                    String::new()
                } else {
                    max_line = max_line.max(num);
                    format!("{}", num)
                };
                labels.push(label);
            }
        }

        (labels, false, max_line)
    });

    if is_alt_screen {
        return None;
    }

    let defaults = render_settings.terminal_defaults();
    let default_opts = velowork_state::SessionTerminalOptions::default();
    let effective_config = velowork_terminal::resolve_effective_terminal_config(&default_opts, &defaults);
    let font_sz = effective_config.font_size * zoom_level * velowork_ui::tokens::ui_scale_factor(cx);
    let lh = font_sz * render_settings.line_height;
    let font_family = effective_config.font_family.clone();
    let term_palette = velowork_core::theme::get_terminal_palette_with_custom(
        &effective_config.color_scheme,
        &render_settings.custom_terminal_color_schemes,
    );
    let line_num_color = with_alpha(term_palette.foreground, 0.45);

    let max_digits = max_line_num.to_string().len().max(3);
    let gutter_w = (max_digits as f32 * font_sz * 0.6 + 16.0).max(36.0);

    let effective_rows = if let Some(bounds) = element_bounds {
        if lh > 0.0 {
            ((f32::from(bounds.size.height) - 0.5) / lh).floor().max(1.0) as u16
        } else {
            rows
        }
    } else {
        rows.max(30)
    }.max(rows);

    Some(
        div()
            .flex_shrink_0()
            .w(px(gutter_w))
            .h_full()
            .pt(SPACE_XS)
            .pr(SPACE_SM)
            .flex()
            .flex_col()
            .overflow_hidden()
            .children((0..effective_rows).map(move |row| {
                let label = line_labels
                    .get(row as usize)
                    .cloned()
                    .unwrap_or_default();
                let font_family = font_family.clone();
                div()
                    .font_family(font_family)
                    .text_size(px(font_sz))
                    .line_height(px(lh))
                    .text_color(line_num_color)
                    .w_full()
                    .flex()
                    .justify_end()
                    .child(label)
            }))
    )
}

/// Lines to scroll for drag-selection auto-scroll, given the pointer's `y` and
/// the terminal content's `top`/`bottom` edges (all window-space pixels).
///
/// Returns 0 while the pointer is between the edges. Past an edge the magnitude
/// grows super-linearly with distance (a far drag scrolls fast) but is capped at
/// ±3 lines per tick so a flick can't jump pages. Positive scrolls up toward
/// history, negative scrolls down toward the prompt. Matches Zed's terminal.
fn autoscroll_lines(y: f32, top: f32, bottom: f32, cell_height: f32) -> i32 {
    if cell_height <= 0.0 {
        return 0;
    }
    let lines = if y < top {
        ((top - y).powf(1.1) / cell_height).ceil() as i32
    } else if y > bottom {
        -(((y - bottom).powf(1.1) / cell_height).ceil() as i32)
    } else {
        0
    };
    lines.clamp(-3, 3)
}

#[cfg(test)]
mod tests {
    use super::{autoscroll_lines, compute_max_scroll_x};

    const CELL: f32 = 16.0;
    const TOP: f32 = 100.0;
    const BOTTOM: f32 = 500.0;

    #[test]
    fn no_scroll_within_bounds() {
        assert_eq!(autoscroll_lines(TOP, TOP, BOTTOM, CELL), 0);
        assert_eq!(autoscroll_lines(300.0, TOP, BOTTOM, CELL), 0);
        assert_eq!(autoscroll_lines(BOTTOM, TOP, BOTTOM, CELL), 0);
    }

    #[test]
    fn scrolls_up_past_top_edge() {
        // Just above the top edge: one line toward history.
        assert_eq!(autoscroll_lines(TOP - 1.0, TOP, BOTTOM, CELL), 1);
        // Far above the top: clamped to the +3 ceiling.
        assert_eq!(autoscroll_lines(TOP - 1000.0, TOP, BOTTOM, CELL), 3);
    }

    #[test]
    fn scrolls_down_past_bottom_edge() {
        assert_eq!(autoscroll_lines(BOTTOM + 1.0, TOP, BOTTOM, CELL), -1);
        assert_eq!(autoscroll_lines(BOTTOM + 1000.0, TOP, BOTTOM, CELL), -3);
    }

    #[test]
    fn magnitude_is_clamped_to_three_lines() {
        for dy in [50.0_f32, 100.0, 500.0, 5000.0] {
            assert!((1..=3).contains(&autoscroll_lines(TOP - dy, TOP, BOTTOM, CELL)));
            assert!((-3..=-1).contains(&autoscroll_lines(BOTTOM + dy, TOP, BOTTOM, CELL)));
        }
    }

    #[test]
    fn zero_cell_height_is_safe() {
        assert_eq!(autoscroll_lines(TOP - 50.0, TOP, BOTTOM, 0.0), 0);
    }

    #[test]
    fn max_scroll_x_zero_when_content_fits_bounds() {
        // Content width (40 cols * 10.0 = 400.0) <= available width (800.0) -> no scroll
        assert_eq!(compute_max_scroll_x(40, 10.0, 800.0), 0.0);
        // Empty content -> no scroll
        assert_eq!(compute_max_scroll_x(0, 10.0, 800.0), 0.0);
        // Content exactly matches available width -> no scroll
        assert_eq!(compute_max_scroll_x(80, 10.0, 800.0), 0.0);
    }

    #[test]
    fn max_scroll_x_positive_when_content_exceeds_bounds() {
        // Content width (100 cols * 10.0 = 1000.0) > available width (800.0)
        // Excess = 1000.0 - 800.0 + 10.0 (padding) = 210.0
        assert_eq!(compute_max_scroll_x(100, 10.0, 800.0), 210.0);
    }
}
