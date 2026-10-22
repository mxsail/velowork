//! Terminal pane navigation, search, and key handling.

use alacritty_terminal::grid::Dimensions;
use crate::ActionDispatch;
use velowork_terminal::input::{KeyEvent, KeyModifiers, key_to_bytes};
use crate::layout::navigation::{get_pane_map, PaneBounds, NavigationDirection};
use velowork_workspace::state::LayoutNode;
use gpui::*;
use std::sync::Arc;

use super::TerminalPane;

impl<D: ActionDispatch + Send + Sync> TerminalPane<D> {
    /// Try to switch to an adjacent tab within a Tabs node.
    /// Returns true if a tab switch happened, false if not in a tab group.
    fn try_switch_tab(&mut self, next: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.layout_path.is_empty() {
            return false;
        }

        let parent_path = &self.layout_path[..self.layout_path.len() - 1];
        let current_tab_index = self.layout_path[self.layout_path.len() - 1];

        let tab_count = {
            let ws = self.workspace.read(cx);
            ws.project(&self.project_id).and_then(|p| {
                p.layout.as_ref().and_then(|layout| {
                    layout.get_at_path(parent_path).and_then(|node| match node {
                        LayoutNode::Tabs { children, .. } => Some(children.len()),
                        _ => None,
                    })
                })
            })
        };

        let num_tabs = match tab_count.filter(|&n| n > 1) {
            Some(n) => n,
            None => return false,
        };

        let new_tab = if next {
            (current_tab_index + 1) % num_tabs
        } else {
            (current_tab_index + num_tabs - 1) % num_tabs
        };

        let project_id = self.project_id.clone();
        let mut new_layout_path = parent_path.to_vec();
        new_layout_path.push(new_tab);

        let workspace = self.workspace.clone();
        self.focus_manager.update(cx, |fm, cx| {
            workspace.update(cx, |ws, cx| {
                ws.set_active_tab(&project_id, parent_path, new_tab, cx);
                ws.set_focused_terminal(fm, project_id.clone(), new_layout_path.clone(), cx);
            });
            cx.notify();
        });

        // Direct focus transfer if already in pane map
        let pane_map = get_pane_map(self.window_id);
        if let Some(target) = pane_map.find_pane(&project_id, &new_layout_path) {
            if let Some(ref fh) = target.focus_handle {
                window.focus(fh, cx);
            }
        }

        true
    }

    pub(super) fn handle_navigation(
        &mut self,
        direction: NavigationDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Left/Right: try switching tabs first, fall through to spatial nav at edges
        if matches!(direction, NavigationDirection::Left | NavigationDirection::Right) {
            let next = matches!(direction, NavigationDirection::Right);
            if self.try_switch_tab(next, window, cx) {
                return;
            }
        }

        let pane_map = get_pane_map(self.window_id);

        let source = match pane_map.find_pane(&self.project_id, &self.layout_path) {
            Some(pane) => pane.clone(),
            None => return,
        };

        if let Some(target) = pane_map.find_nearest_in_direction(&source, direction) {
            self.focus_target(target, window, cx);
        }
    }

    pub(super) fn handle_sequential_navigation(
        &mut self,
        next: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_zoomed(cx) {
            if next {
                self.handle_zoom_next_terminal(cx);
            } else {
                self.handle_zoom_prev_terminal(cx);
            }
            return;
        }

        if self.try_switch_tab(next, window, cx) {
            return;
        }

        let pane_map = get_pane_map(self.window_id);

        let source = match pane_map.find_pane(&self.project_id, &self.layout_path) {
            Some(pane) => pane.clone(),
            None => return,
        };

        let target = if next {
            pane_map.find_next_pane(&source)
        } else {
            pane_map.find_prev_pane(&source)
        };

        if let Some(ref target) = target {
            self.focus_target(target, window, cx);
        }
    }

    fn focus_target(&self, target: &PaneBounds, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ref fh) = target.focus_handle {
            window.focus(fh, cx);
        }
        let target_project = target.project_id.clone();
        let target_path = target.layout_path.clone();
        let workspace = self.workspace.clone();
        self.focus_manager.update(cx, |fm, cx| {
            workspace.update(cx, |ws, cx| {
                ws.set_focused_terminal(fm, target_project, target_path, cx);
            });
            cx.notify();
        });
    }

    pub(crate) fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_bar.read(cx).is_active() {
            self.close_search(window, cx);
        } else {
            self.start_search(window, cx);
        }
    }

    pub(crate) fn start_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_bar.update(cx, |search_bar, cx| {
            search_bar.open(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_bar.update(cx, |search_bar, cx| {
            search_bar.close(cx);
        });
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    pub(super) fn next_match(&mut self, cx: &mut Context<Self>) {
        self.search_bar.update(cx, |search_bar, cx| {
            search_bar.next_match(cx);
        });
    }

    pub(super) fn prev_match(&mut self, cx: &mut Context<Self>) {
        self.search_bar.update(cx, |search_bar, cx| {
            search_bar.prev_match(cx);
        });
    }

    pub(super) fn update_history_popup(&mut self, cx: &mut Context<Self>) {
        if self.snippet_state.is_some() {
            self.history_popup_open = false;
            self.history_popup_items.clear();
            self.history_popup_selected = None;
            return;
        }

        let s = crate::terminal_view_settings(cx);
        if !s.command_history_auto_completion || self.is_cursor_at_auth_prompt() {
            self.history_popup_open = false;
            self.history_popup_items.clear();
            self.history_popup_selected = None;
            return;
        }

        if let Some(ref term) = self.terminal {
            if term.is_alt_screen() || term.is_mouse_mode() {
                self.history_popup_open = false;
                self.history_popup_items.clear();
                self.history_popup_selected = None;
                return;
            }
        }

        let query = self.input_line_buffer.trim();
        if query.is_empty() {
            self.history_popup_open = false;
            self.history_popup_items.clear();
            self.history_popup_selected = None;
            return;
        }

        let mut results = Vec::new();

        // 1. Match quick commands
        let empty_nodes = Vec::new();
        let global_settings = cx.try_global::<velowork_app_core::settings::GlobalSettings>();
        let qc_nodes = if let Some(global) = global_settings {
            let app_settings = global.0.read(cx);
            app_settings
                .settings
                .quick_commands_for_project(Some(&self.project_id))
                .to_vec()
        } else {
            empty_nodes
        };
        let matched_qcs = velowork_workspace::quick_commands::qc_match_commands(&qc_nodes, query, 5);
        for qc in matched_qcs {
            let effective_vars = qc.effective_variables();
            if let velowork_workspace::quick_commands::QuickCommandNode::Command {
                id,
                name,
                command,
                ..
            } = qc
            {
                results.push(super::history_popup::TerminalCompletionItem::QuickCommand {
                    id,
                    name,
                    command,
                    variables: effective_vars,
                });
            }
        }

        // 2. Match history entries
        let history_limit = 10usize.saturating_sub(results.len()).max(3);
        let entries = super::history_cache::HistoryCache::match_suggestions(
            &self.project_id,
            query,
            history_limit,
        );
        for entry in entries {
            results.push(super::history_popup::TerminalCompletionItem::History(entry));
        }

        if !results.is_empty() {
            self.history_popup_items = results;
            self.history_popup_open = true;
            if let Some(sel) = self.history_popup_selected {
                if sel >= self.history_popup_items.len() {
                    self.history_popup_selected = None;
                }
            }
            cx.notify();
            return;
        }

        self.history_popup_open = false;
        self.history_popup_items.clear();
        self.history_popup_selected = None;
        cx.notify();
    }

    pub(super) fn record_terminal_history(&mut self, cmd: &str, cx: &App) {
        let s = crate::terminal_view_settings(cx);
        if !velowork_core::security::SecretRedactor::should_record_to_history(
            cmd,
            &s.command_history_ignored_commands,
            s.command_history_ignore_space,
        ) {
            return;
        }

        let trimmed = cmd.trim();
        if trimmed.is_empty() || is_auth_prompt_line(trimmed) {
            return;
        }

        let sanitized = velowork_core::security::SecretRedactor::redact_command(trimmed);
        let cmd_to_save = sanitized.trim().to_string();
        if cmd_to_save.is_empty() {
            return;
        }

        // 1. Immediately update in-memory cache for instant subsequent completions
        super::history_cache::HistoryCache::record_command(&self.project_id, &cmd_to_save);

        // 2. Persist to SQLite asynchronously in background (non-blocking for UI thread)
        let project_id = self.project_id.clone();
        let max_count = s.command_history_max_count;
        let retention_days = s.command_history_retention_days;
        cx.background_executor()
            .spawn(async move {
                if let Some(db) = velowork_core::storage::database() {
                    let repo = velowork_workspace::repositories::HistoryRepository::new(db);
                    let _ = repo.record_project_command(
                        &project_id,
                        &cmd_to_save,
                        max_count,
                        retention_days,
                    );
                }
            })
            .detach();
    }

    /// Checks whether the terminal cursor is currently on (or right below) an interactive password/auth prompt.
    pub(super) fn is_cursor_at_auth_prompt(&self) -> bool {
        let Some(ref terminal) = self.terminal else {
            return false;
        };

        terminal.inspect_term(|term| {
            if term.mode().contains(alacritty_terminal::term::TermMode::ALT_SCREEN) {
                return false;
            }

            let grid = term.grid();
            let cursor_point = grid.cursor.point;
            let cols = grid.columns();
            let cursor_line_idx = cursor_point.line.0;
            let topmost = grid.topmost_line().0;

            // Check the current cursor line and up to 1 preceding line
            for l in (cursor_line_idx.saturating_sub(1)..=cursor_line_idx).rev() {
                if l < topmost {
                    continue;
                }
                let line_point = alacritty_terminal::index::Line(l);
                let mut line_str = String::with_capacity(cols);
                for c in 0..cols {
                    let cell_char = grid[alacritty_terminal::index::Point::new(line_point, alacritty_terminal::index::Column(c))].c;
                    if cell_char != '\0' {
                        line_str.push(cell_char);
                    }
                }
                let trimmed = line_str.trim();
                if !trimmed.is_empty() && is_auth_prompt_line(trimmed) {
                    return true;
                }
            }

            false
        })
    }

    /// Extract the full executed command from the terminal content grid at the cursor line.
    /// This captures full arguments (including tab completions, path expansions, pasted arguments)
    /// while strictly isolating and stripping any shell prompt prefix.
    pub(super) fn extract_command_to_record(&self) -> Option<String> {
        if self.is_cursor_at_auth_prompt() {
            return None;
        }

        let terminal = self.terminal.as_ref()?;
        let input_buf_trimmed = self.input_line_buffer.trim().to_string();

        terminal.inspect_term(|term| {
            if term.mode().contains(alacritty_terminal::term::TermMode::ALT_SCREEN) {
                return None;
            }

            let grid = term.grid();
            let cursor_point = grid.cursor.point;
            let cols = grid.columns();
            let cursor_line_idx = cursor_point.line.0;
            let topmost = grid.topmost_line().0;

            // Reconstruct logical line spanning wrapped rows
            let mut start_line = cursor_line_idx;
            while start_line > topmost {
                let prev_line = alacritty_terminal::index::Line(start_line - 1);
                let last_col = alacritty_terminal::index::Column(cols - 1);
                if grid[alacritty_terminal::index::Point::new(prev_line, last_col)]
                    .flags
                    .contains(alacritty_terminal::term::cell::Flags::WRAPLINE)
                {
                    start_line -= 1;
                } else {
                    break;
                }
            }

            let mut full_logical_line = String::new();
            for l in start_line..=cursor_line_idx {
                let line_point = alacritty_terminal::index::Line(l);
                let mut line_str = String::with_capacity(cols);
                for c in 0..cols {
                    let cell_char = grid[alacritty_terminal::index::Point::new(line_point, alacritty_terminal::index::Column(c))].c;
                    if cell_char == '\0' {
                        line_str.push(' ');
                    } else {
                        line_str.push(cell_char);
                    }
                }

                let is_wrapped = if l < cursor_line_idx {
                    let last_col = alacritty_terminal::index::Column(cols - 1);
                    grid[alacritty_terminal::index::Point::new(line_point, last_col)]
                        .flags
                        .contains(alacritty_terminal::term::cell::Flags::WRAPLINE)
                } else {
                    false
                };

                if is_wrapped {
                    full_logical_line.push_str(&line_str);
                } else {
                    full_logical_line.push_str(line_str.trim_end());
                }
            }

            let full_trimmed = full_logical_line.trim();
            if full_trimmed.is_empty() || is_auth_prompt_line(full_trimmed) {
                return None;
            }

            // Strategy 1: Match against input_line_buffer.
            // If the user typed a command prefix (e.g. "less"), find where that command starts
            // and take the remainder of the line to capture tab-completed/pasted arguments.
            if !input_buf_trimmed.is_empty() {
                if let Some(first_word) = input_buf_trimmed.split_whitespace().next() {
                    let mut search_start = 0;
                    let mut best_match: Option<&str> = None;
                    while let Some(rel_idx) = full_trimmed[search_start..].find(first_word) {
                        let idx = search_start + rel_idx;
                        let is_valid_start = idx == 0 || {
                            let prev = full_trimmed[..idx].chars().next_back().unwrap_or(' ');
                            prev.is_whitespace()
                                || prev == '$'
                                || prev == '#'
                                || prev == '>'
                                || prev == '%'
                                || prev == ']'
                                || prev == ':'
                                || prev == '❯'
                                || prev == '➜'
                                || prev == '»'
                        };
                        if is_valid_start {
                            let candidate = full_trimmed[idx..].trim();
                            if candidate.starts_with(first_word) {
                                best_match = Some(candidate);
                            }
                        }
                        search_start = idx + first_word.len();
                    }
                    if let Some(m) = best_match {
                        return Some(m.to_string());
                    }
                }
            }

            // Strategy 2: Strip standard shell prompt markers.
            if let Some(cmd) = strip_shell_prompt(full_trimmed) {
                return Some(cmd.to_string());
            }

            None
        })
    }

    pub(super) fn select_history_suggestion(&mut self, idx: usize, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if idx < self.history_popup_items.len() {
            let item = self.history_popup_items[idx].clone();
            match item {
                super::history_popup::TerminalCompletionItem::History(entry) => {
                    if let Some(terminal) = self.terminal.clone() {
                        terminal.send_bytes(b"\x15"); // Ctrl+U clear line
                        terminal.send_bytes(entry.command.as_bytes());
                    }
                    self.input_line_buffer = entry.command;
                    self.history_popup_open = false;
                    self.history_popup_items.clear();
                    self.history_popup_selected = None;
                    cx.notify();
                }
                super::history_popup::TerminalCompletionItem::QuickCommand {
                    id,
                    name,
                    command,
                    variables,
                } => {
                    if variables.is_empty() {
                        if let Some(terminal) = self.terminal.clone() {
                            terminal.send_bytes(b"\x15"); // Ctrl+U clear line
                            terminal.send_bytes(command.as_bytes());
                        }
                        self.input_line_buffer = command;
                        self.history_popup_open = false;
                        self.history_popup_items.clear();
                        self.history_popup_selected = None;
                        cx.notify();
                    } else {
                        self.enter_snippet_mode(id, name, command, variables, window, cx);
                    }
                }
            }
        }
    }

    pub(super) fn enter_snippet_mode(
        &mut self,
        id: String,
        name: String,
        command: String,
        variables: Vec<velowork_workspace::quick_commands::QuickCommandVar>,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        let state = super::snippet_bar::TerminalSnippetState::new(
            id,
            name,
            command,
            variables,
            self.input_line_buffer.clone(),
            cx,
        );

        // 订阅每一个变量槽输入框的事件（内容变化触发实时预览重绘，Enter/Escape 控制生命周期）
        for slot in &state.slots {
            let input = slot.input.clone();
            cx.subscribe(&input, |this, _, event: &velowork_ui::input::InputEvent, cx| {
                match event {
                    velowork_ui::input::InputEvent::Change => {
                        if let Some(ref mut st) = this.snippet_state {
                            st.sync_modified_status(cx);
                        }
                        cx.notify();
                    }
                    velowork_ui::input::InputEvent::PressEnter => {
                        this.submit_snippet(false, cx);
                    }
                    velowork_ui::input::InputEvent::PressEscape => {
                        this.exit_snippet_mode(cx);
                    }
                    _ => {}
                }
            })
            .detach();
        }

        // 默认将焦点置于第一个输入框并全选默认值（支持打字直接覆盖与鼠标点击微调）
        if let Some(win) = window {
            if let Some(first_slot) = state.slots.first() {
                first_slot.input.update(cx, |inp, cx| {
                    inp.select_all(cx);
                    inp.focus(win, cx);
                });
            }
        }

        self.snippet_state = Some(state);
        self.history_popup_open = false;
        self.history_popup_items.clear();
        self.history_popup_selected = None;

        if let Some(ref term) = self.terminal {
            // 设置防御性输入拦截器，彻底防止底层终端接收多余击键
            term.set_text_input_interceptor(Some(Arc::new(|_, _, _| {})));
        }

        cx.notify();
    }

    pub(super) fn submit_snippet(&mut self, auto_run: bool, cx: &mut Context<Self>) {
        if let Some(ref snippet) = self.snippet_state {
            let final_cmd = snippet.render_interpolated_command(cx);
            self.exit_snippet_mode(cx);
            if let Some(terminal) = self.terminal.clone() {
                terminal.send_bytes(b"\x15"); // Ctrl+U clear query text
                terminal.send_bytes(final_cmd.as_bytes());
                if auto_run {
                    terminal.send_bytes(b"\n");
                }
            }
            self.input_line_buffer = if auto_run { String::new() } else { final_cmd };
            cx.notify();
        }
    }

    pub(super) fn exit_snippet_mode(&mut self, cx: &mut Context<Self>) {
        if self.snippet_state.is_some() {
            self.snippet_state = None;
            if let Some(ref term) = self.terminal {
                term.set_text_input_interceptor(None);
            }
            cx.notify();
        }
    }

    pub(super) fn handle_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Defense-in-depth: never feed keystrokes to the PTY unless this pane
        // is the actually-focused element or contains the focused element.
        if !self.focus_handle.is_focused(window) && !self.focus_handle.contains_focused(window, cx) {
            log::warn!(
                "[TERMINAL-NAV] Dropped key {:?} because pane focus_handle is not focused on window",
                event.keystroke.key
            );
            return;
        }

        if self.is_connection_lost(cx) {
            log::debug!("[TERMINAL-NAV] Dropped key {:?} because connection is lost", event.keystroke.key);
            return;
        }

        // ZMODEM stream protection: intercept Ctrl+C/Esc while ZMODEM is active, and swallow other keystrokes
        if self.is_zmodem_active() {
            if (event.keystroke.key == "c" && event.keystroke.modifiers.control)
                || event.keystroke.key == "escape"
            {
                log::info!("[ZMODEM-NAV] Ctrl+C / Escape intercepted during active ZMODEM session, cancelling");
                self.cancel_zmodem(cx);
            }
            return;
        }

        if self.shell_type == velowork_core::shell::ShellType::Welcome {
            if let Some(action) = crate::welcome::handle_welcome_key_down(
                &self.project_id,
                &self.quick_connect_input,
                &mut self.welcome_selected_index,
                event,
                cx,
            ) {
                self.execute_welcome_action(action, window, cx);
            }
            cx.notify();
            return;
        }

        #[cfg(target_os = "windows")]
        if event.keystroke.key == "v"
            && event.keystroke.modifiers.control
            && !event.keystroke.modifiers.shift
            && !event.keystroke.modifiers.alt
            && !event.keystroke.modifiers.platform
        {
            self.handle_paste(cx);
            return;
        }

        // 0. Snippet 变量导航与填参模式拦截（优先级 3）
        if let Some(ref mut snippet) = self.snippet_state {
            if event.keystroke.key == "tab"
                && !event.keystroke.modifiers.control
                && !event.keystroke.modifiers.alt
                && !event.keystroke.modifiers.platform
            {
                if event.keystroke.modifiers.shift {
                    snippet.jump_prev(window, cx);
                } else {
                    snippet.jump_next(window, cx);
                }
                cx.notify();
                return;
            }

            if event.keystroke.key == "enter" {
                let auto_run = event.keystroke.modifiers.control || event.keystroke.modifiers.platform;
                self.submit_snippet(auto_run, cx);
                window.focus(&self.focus_handle, cx);
                return;
            }

            if event.keystroke.key == "escape" {
                self.exit_snippet_mode(cx);
                window.focus(&self.focus_handle, cx);
                return;
            }

            // 吞噬所有其他键盘输入，普通字符与输入法文本由 SimpleInputState 原生接管
            return;
        }

        // 历史命令与快捷指令智能匹配浮窗的键盘交互拦截
        if self.history_popup_open && !self.history_popup_items.is_empty() {
            if event.keystroke.key == "down"
                && !event.keystroke.modifiers.control
                && !event.keystroke.modifiers.alt
                && !event.keystroke.modifiers.platform
            {
                self.history_popup_selected = match self.history_popup_selected {
                    None => Some(0),
                    Some(i) => Some((i + 1).min(self.history_popup_items.len() - 1)),
                };
                cx.notify();
                return;
            }

            if event.keystroke.key == "up"
                && !event.keystroke.modifiers.control
                && !event.keystroke.modifiers.alt
                && !event.keystroke.modifiers.platform
            {
                self.history_popup_selected = match self.history_popup_selected {
                    Some(0) => None,
                    Some(i) => Some(i - 1),
                    None => None,
                };
                cx.notify();
                return;
            }

            if event.keystroke.key == "escape" {
                self.history_popup_open = false;
                self.history_popup_items.clear();
                self.history_popup_selected = None;
                cx.notify();
                return;
            }

            if (event.keystroke.key == "tab" || event.keystroke.key == "enter")
                && !event.keystroke.modifiers.control
                && !event.keystroke.modifiers.alt
                && !event.keystroke.modifiers.platform
            {
                let selected_idx = self.history_popup_selected.or_else(|| {
                    if event.keystroke.key == "tab" {
                        Some(0)
                    } else {
                        None
                    }
                });

                if let Some(idx) = selected_idx {
                    self.select_history_suggestion(idx, Some(window), cx);
                    return;
                }
            }
        }

        if let Some(terminal) = self.terminal.clone() {
            terminal.claim_resize_local();

            // Backspace with selection: delete selected text (only in plain shell)
            if event.keystroke.key == "backspace"
                && !event.keystroke.modifiers.control
                && !event.keystroke.modifiers.alt
                && !event.keystroke.modifiers.platform
                && terminal.has_selection()
                && !terminal.is_mouse_mode()
                && !terminal.is_alt_screen()
                && !terminal.has_running_child()
                && terminal.delete_selection() {
                    self.input_line_buffer.clear();
                    self.update_history_popup(cx);
                    return;
                }

            // Opt-in: Ctrl+C copies selection (and clears it) instead of sending SIGINT.
            if event.keystroke.key == "c"
                && event.keystroke.modifiers.control
                && !event.keystroke.modifiers.shift
                && !event.keystroke.modifiers.alt
                && !event.keystroke.modifiers.platform
                && crate::terminal_view_settings(cx).ctrl_c_copies_selection
                && let Some(text) = terminal.get_selected_text()
                && !text.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    terminal.clear_selection();
                    self.input_line_buffer.clear();
                    self.update_history_popup(cx);
                    cx.notify();
                    return;
                }

            // 维护输入缓冲与触发历史命令记录
            if event.keystroke.key == "enter" {
                log::debug!("[ZMODEM-NAV] clear_zmodem_cooldown on Enter key");
                self.clear_zmodem_cooldown();
                let is_auth = self.is_cursor_at_auth_prompt();
                if is_auth {
                    self.input_line_buffer.clear();
                    self.command_accumulator.clear();
                    self.history_popup_open = false;
                    self.history_popup_items.clear();
                    self.history_popup_selected = None;
                } else {
                    let current_line = self.extract_command_to_record();

                    if let Some(cmd_line) = current_line {
                        let trimmed = cmd_line.trim();
                        if !trimmed.is_empty() && !is_auth_prompt_line(trimmed) {
                            let ends_with_backslash = trimmed.ends_with('\\');
                            if ends_with_backslash {
                                // 续行符（\结尾）：累加到当前多行缓冲，暂不入库
                                self.command_accumulator.push(trimmed.to_string());
                            } else if !self.command_accumulator.is_empty() {
                                // 多行命令的末行：合并所有累积行作为整体入库
                                self.command_accumulator.push(trimmed.to_string());
                                let full_cmd = self.command_accumulator.join("\n");
                                self.record_terminal_history(&full_cmd, cx);
                                self.command_accumulator.clear();
                            } else {
                                // 普通单行命令
                                self.record_terminal_history(trimmed, cx);
                            }
                        }
                    } else if !self.command_accumulator.is_empty() {
                        // 空回车提交：将已累积的多行命令整体入库
                        let full_cmd = self.command_accumulator.join("\n");
                        self.record_terminal_history(&full_cmd, cx);
                        self.command_accumulator.clear();
                    }

                    self.input_line_buffer.clear();
                    self.history_popup_open = false;
                    self.history_popup_items.clear();
                    self.history_popup_selected = None;
                }
            } else if event.keystroke.key == "backspace" {
                self.input_line_buffer.pop();
                self.update_history_popup(cx);
            } else if (event.keystroke.key == "c" || event.keystroke.key == "u")
                && event.keystroke.modifiers.control
            {
                log::debug!("[ZMODEM-NAV] clear_zmodem_cooldown on Ctrl+{}", event.keystroke.key);
                self.clear_zmodem_cooldown();
                self.input_line_buffer.clear();
                self.command_accumulator.clear();
                self.history_popup_open = false;
                self.history_popup_items.clear();
                self.history_popup_selected = None;
            } else if !event.keystroke.modifiers.control
                && !event.keystroke.modifiers.alt
                && !event.keystroke.modifiers.platform
            {
                if let Some(ref ch) = event.keystroke.key_char {
                    if !ch.chars().any(|c| c.is_control()) {
                        self.input_line_buffer.push_str(ch);
                        self.update_history_popup(cx);
                    }
                } else if event.keystroke.key.chars().count() == 1 {
                    if let Some(c) = event.keystroke.key.chars().next() {
                        if !c.is_control() {
                            self.input_line_buffer.push(c);
                            self.update_history_popup(cx);
                        }
                    }
                }
            }

            let key_str = event.keystroke.key.as_str();
            let is_ctrl = event.keystroke.modifiers.control;
            if key_str == "backspace" {
                terminal.predict_backspace();
            } else if key_str == "escape"
                || key_str == "tab"
                || matches!(key_str, "up" | "down" | "left" | "right" | "home" | "end" | "pageup" | "pagedown")
                || is_ctrl
            {
                terminal.clear_predictions();
            }

            let app_cursor_mode = terminal.is_app_cursor_mode();
            let key_event = KeyEvent {
                key: event.keystroke.key.clone(),
                key_char: event.keystroke.key_char.clone(),
                modifiers: KeyModifiers {
                    control: event.keystroke.modifiers.control,
                    shift: event.keystroke.modifiers.shift,
                    alt: event.keystroke.modifiers.alt,
                    platform: event.keystroke.modifiers.platform,
                },
            };
            if let Some(input) = key_to_bytes(&key_event, app_cursor_mode) {
                // If this is a fallback single-character key not handled by InputHandler
                if !is_ctrl
                    && !event.keystroke.modifiers.alt
                    && !event.keystroke.modifiers.platform
                    && key_event.key_char.is_none()
                    && key_str.len() == 1
                {
                    terminal.predict_input(key_str);
                }
                terminal.send_bytes(&input);
            }
        }
    }
}

pub use velowork_terminal::terminal::is_auth_prompt_line;

/// Strip common shell prompt prefixes (e.g. `user@host:path$ `, `user@host:path# `, `[user@host ~]$ `, `PS C:\> `, `❯ `, `➜ `, `» `)
fn strip_shell_prompt(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 1. Common explicit prompt delimiters with space or boundary
    let prompt_markers = [
        "]$ ", "]$",
        "]# ", "]#",
        ":~$ ", ":~$",
        ":~# ", ":~#",
        ":$ ", ":# ",
        "$ ", "# ", "% ", "> ", "❯ ", "» ",
        "\\$ ", "\\# ",
        ") ✗ ", "✗ ", ") ",
        "➜ ",
    ];

    let mut rightmost: Option<(usize, usize)> = None;
    for marker in prompt_markers {
        if let Some(idx) = trimmed.rfind(marker) {
            let end_idx = idx + marker.len();
            if rightmost.map_or(true, |(r_idx, r_len)| end_idx > r_idx + r_len) {
                let candidate = trimmed[end_idx..].trim();
                if !candidate.is_empty() {
                    rightmost = Some((idx, marker.len()));
                }
            }
        }
    }

    if let Some((idx, len)) = rightmost {
        let candidate = trimmed[idx + len..].trim();
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }

    // 2. PowerShell / Cmd prompt ending with '>'
    if let Some(idx) = trimmed.rfind('>') {
        let candidate = trimmed[idx + 1..].trim();
        if !candidate.is_empty() {
            return Some(candidate);
        }
    }

    // 3. User@host format: "choi@fedora:~/Workspaces$ ls" or "root@srv:/var# ls"
    if let Some(at_idx) = trimmed.find('@') {
        if let Some(sub) = trimmed[at_idx..].find(|c| c == '$' || c == '#' || c == '%') {
            let actual_idx = at_idx + sub;
            let candidate = trimmed[actual_idx + 1..].trim();
            if !candidate.is_empty() {
                return Some(candidate);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{is_auth_prompt_line, strip_shell_prompt};

    #[test]
    fn test_is_auth_prompt_line() {
        // Common password prompts
        assert!(is_auth_prompt_line("user@192.168.1.1's password:"));
        assert!(is_auth_prompt_line("[sudo] password for choi:"));
        assert!(is_auth_prompt_line("Password:"));
        assert!(is_auth_prompt_line("password: "));
        assert!(is_auth_prompt_line("Enter password:"));
        assert!(is_auth_prompt_line("Password for 'https://github.com':"));
        assert!(is_auth_prompt_line("Enter passphrase for key '/home/choi/.ssh/id_rsa':"));
        assert!(is_auth_prompt_line("Passphrase:"));
        assert!(is_auth_prompt_line("Current password:"));
        assert!(is_auth_prompt_line("New password:"));
        assert!(is_auth_prompt_line("Retype new password:"));

        // Chinese prompts
        assert!(is_auth_prompt_line("请输入密码："));
        assert!(is_auth_prompt_line("密码："));
        assert!(is_auth_prompt_line("密码:"));
        assert!(is_auth_prompt_line("请输入口令："));
        assert!(is_auth_prompt_line("口令:"));
        assert!(is_auth_prompt_line("请输入动态口令:"));
        assert!(is_auth_prompt_line("验证码："));

        // 2FA / OTP / Token / PIN
        assert!(is_auth_prompt_line("Verification code:"));
        assert!(is_auth_prompt_line("OTP:"));
        assert!(is_auth_prompt_line("2FA Token:"));
        assert!(is_auth_prompt_line("Google Authenticator Code:"));
        assert!(is_auth_prompt_line("Enter PIN for token:"));

        // Normal commands containing the word password should NOT be treated as auth prompts
        assert!(!is_auth_prompt_line("choi@ubuntu:~$ echo password"));
        assert!(!is_auth_prompt_line("choi@ubuntu:~$ grep password /etc/pam.d/common-auth"));
        assert!(!is_auth_prompt_line("root@server:# cat /etc/passwd"));
        assert!(!is_auth_prompt_line("➜  velowork git:(main) ✗ cargo check"));
        assert!(!is_auth_prompt_line(""));
    }

    #[test]
    fn test_strip_shell_prompt() {
        assert_eq!(
            strip_shell_prompt("choi@ubuntu:~$ less /home/choi/Workspaces/velowork/crates/file.rs"),
            Some("less /home/choi/Workspaces/velowork/crates/file.rs")
        );
        assert_eq!(
            strip_shell_prompt("[choi@fedora velowork]$ less 不存在的目录"),
            Some("less 不存在的目录")
        );
        assert_eq!(
            strip_shell_prompt("root@server:/var/log# tail -f syslog"),
            Some("tail -f syslog")
        );
        assert_eq!(
            strip_shell_prompt("PS C:\\Users\\Administrator> Get-Service"),
            Some("Get-Service")
        );
        assert_eq!(
            strip_shell_prompt("➜  velowork git:(main) ✗ cargo build"),
            Some("cargo build")
        );
        assert_eq!(
            strip_shell_prompt("❯ npm run dev"),
            Some("npm run dev")
        );
        assert_eq!(
            strip_shell_prompt("mysql> SELECT * FROM users;"),
            Some("SELECT * FROM users;")
        );
        assert_eq!(
            strip_shell_prompt(""),
            None
        );
    }
}
