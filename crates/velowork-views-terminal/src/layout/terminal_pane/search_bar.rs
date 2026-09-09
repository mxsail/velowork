//! Search bar component for terminal pane.

use crate::elements::terminal_element::SearchMatch;
use velowork_ui::icon::AppIcon;
use crate::actions::CloseSearch;
use velowork_terminal::terminal::Terminal;
use velowork_ui::theme::theme;
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::tokens::{ui_font_family, use_custom_ui_font, ui_text_md, ICON_MD, SPACE_SM, SPACE_MD, RADIUS_MD, RADIUS_LG};
use velowork_ui::input::{Input, InputEvent, InputState};
use velowork_ui::tooltip::Tooltip;
use velowork_workspace::focus::FocusManager;
use velowork_workspace::state::Workspace;
use velowork_i18n::i18n;
use gpui::prelude::FluentBuilder;
use gpui::*;
use velowork_ui::icon_button::icon_button_sized_px;
use std::sync::Arc;

#[derive(Clone)]
pub enum SearchBarEvent {
    Closed,
    MatchesChanged(Arc<Vec<SearchMatch>>, Option<usize>),
}

impl EventEmitter<SearchBarEvent> for SearchBar {}

pub struct SearchBar {
    workspace: Entity<Workspace>,
    focus_manager: Entity<FocusManager>,
    terminal: Option<Arc<Terminal>>,
    input: Option<Entity<InputState>>,
    matches: Arc<Vec<SearchMatch>>,
    current_match_index: Option<usize>,
    case_sensitive: bool,
    use_regex: bool,
    is_active: bool,
    last_search_generation: u64,
}

impl SearchBar {
    pub fn new(workspace: Entity<Workspace>, focus_manager: Entity<FocusManager>, _cx: &mut Context<Self>) -> Self {
        Self {
            workspace,
            focus_manager,
            terminal: None,
            input: None,
            matches: Arc::new(Vec::new()),
            current_match_index: None,
            case_sensitive: false,
            use_regex: false,
            is_active: false,
            last_search_generation: 0,
        }
    }

    pub fn set_terminal(&mut self, terminal: Option<Arc<Terminal>>) {
        self.terminal = terminal;
    }

    pub fn is_active(&self) -> bool {
        self.is_active
    }

    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.is_active = true;
        let input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder(i18n!(cx, "terminal.search_placeholder"))
        });
        input.update(cx, |input, cx| {
            input.focus(window, cx);
        });
        cx.subscribe(&input, |this: &mut Self, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                this.perform_search(cx);
            }
        }).detach();
        self.input = Some(input);
        self.matches = Arc::new(Vec::new());
        self.current_match_index = None;

        let workspace = self.workspace.clone();
        self.focus_manager.update(cx, |fm, cx| {
            workspace.update(cx, |ws, cx| ws.clear_focused_terminal(fm, cx));
            cx.notify();
        });
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.is_active = false;
        self.input = None;
        self.matches = Arc::new(Vec::new());
        self.current_match_index = None;

        let workspace = self.workspace.clone();
        self.focus_manager.update(cx, |fm, cx| {
            workspace.update(cx, |ws, cx| ws.restore_focused_terminal(fm, cx));
            cx.notify();
        });
        cx.emit(SearchBarEvent::Closed);
        cx.notify();
    }

    pub fn perform_search(&mut self, cx: &mut Context<Self>) {
        let query = self.input.as_ref().map(|i| i.read(cx).text().to_string()).unwrap_or_default();

        if let Some(ref terminal) = self.terminal {
            self.last_search_generation = terminal.content_generation();
            let matches = terminal.search_grid(&query, self.case_sensitive, self.use_regex);
            let search_matches: Vec<SearchMatch> = matches
                .into_iter()
                .map(|(line, col, len)| SearchMatch { line, col, len })
                .collect();

            self.current_match_index = if !search_matches.is_empty() { Some(0) } else { None };
            self.matches = Arc::new(search_matches);

            cx.emit(SearchBarEvent::MatchesChanged(
                self.matches.clone(),
                self.current_match_index,
            ));
        }
        cx.notify();
    }

    /// Re-run search if terminal content has changed since last search.
    pub fn refresh_if_needed(&mut self, cx: &mut Context<Self>) {
        if !self.is_active { return; }
        if let Some(ref terminal) = self.terminal {
            let current_gen = terminal.content_generation();
            if current_gen != self.last_search_generation {
                self.perform_search(cx);
            }
        }
    }

    fn toggle_case_sensitive(&mut self, cx: &mut Context<Self>) {
        self.case_sensitive = !self.case_sensitive;
        self.perform_search(cx);
    }

    fn toggle_regex(&mut self, cx: &mut Context<Self>) {
        self.use_regex = !self.use_regex;
        self.perform_search(cx);
    }

    pub fn next_match(&mut self, cx: &mut Context<Self>) {
        if self.matches.is_empty() { return; }
        let next_idx = match self.current_match_index {
            Some(idx) => (idx + 1) % self.matches.len(),
            None => 0,
        };
        self.current_match_index = Some(next_idx);
        self.scroll_to_current_match();
        cx.emit(SearchBarEvent::MatchesChanged(self.matches.clone(), self.current_match_index));
        cx.notify();
    }

    pub fn prev_match(&mut self, cx: &mut Context<Self>) {
        if self.matches.is_empty() { return; }
        let prev_idx = match self.current_match_index {
            Some(idx) => { if idx == 0 { self.matches.len() - 1 } else { idx - 1 } }
            None => self.matches.len() - 1,
        };
        self.current_match_index = Some(prev_idx);
        self.scroll_to_current_match();
        cx.emit(SearchBarEvent::MatchesChanged(self.matches.clone(), self.current_match_index));
        cx.notify();
    }

    fn scroll_to_current_match(&self) {
        if let (Some(idx), Some(terminal)) = (self.current_match_index, &self.terminal)
            && let Some(search_match) = self.matches.get(idx) {
                let screen_lines = terminal.screen_lines() as i32;
                let display_offset = terminal.display_offset() as i32;
                // Convert absolute grid line to visual line
                let visual_line = search_match.line + display_offset;
                if visual_line < 0 || visual_line >= screen_lines {
                    let target_visible_line = screen_lines / 2;
                    let scroll_delta = target_visible_line - visual_line;
                    if scroll_delta > 0 { terminal.scroll_up(scroll_delta); }
                    else if scroll_delta < 0 { terminal.scroll_down(-scroll_delta); }
                }
            }
    }

    fn handle_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if event.keystroke.key.as_str() == "enter" {
            if event.keystroke.modifiers.shift { self.prev_match(cx); }
            else { self.next_match(cx); }
        }
    }
}

impl Render for SearchBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let match_count = self.matches.len();
        let current_idx = self.current_match_index.map(|i| i + 1).unwrap_or(0);
        let match_text = if match_count > 0 { format!("{}/{}", current_idx, match_count) } else { "0/0".to_string() };
        let case_sensitive = self.case_sensitive;
        let is_regex = self.use_regex;

        let case_tip = i18n!(cx, "terminal.search_case_sensitive");
        let regex_tip = i18n!(cx, "terminal.search_regex");
        let prev_tip = i18n!(cx, "terminal.search_prev");
        let next_tip = i18n!(cx, "terminal.search_next");
        let close_tip = i18n!(cx, "terminal.search_close");

        div()
            .id("search-bar")
            .absolute()
            .top(SPACE_MD)
            .right(px(24.0))
            .h(px(38.0))
            .px(SPACE_MD)
            .flex()
            .items_center()
            .gap(SPACE_SM)
            .bg(p.surface_raised)
            .border_1()
            .border_color(rgb(t.border))
            .rounded(RADIUS_LG)
            .shadow_xl()
            .max_w(relative(0.9))
            .when(use_custom_ui_font(cx), |d| d.font_family(ui_font_family(cx)))
            // 外层边框组合：输入框 + 大小写/正则图标，图标内嵌于输入框右侧
            .child(
                div()
                    .id("search-input-group")
                    .h(px(28.0))
                    .w(px(260.0))
                    .flex()
                    .items_center()
                    .bg(p.surface_base)
                    .border_1()
                    .border_color(rgb(t.border))
                    .rounded(RADIUS_MD)
                    .child(
                        if let Some(ref input) = self.input {
                            div()
                                .id("search-input-wrapper")
                                .key_context("SearchBar")
                                .flex_1()
                                .h_full()
                                .child(Input::new(input).borderless(true))
                                .on_mouse_down(MouseButton::Left, |_, _, cx| { cx.stop_propagation(); })
                                .on_action(cx.listener(|this, _: &CloseSearch, _window, cx| { this.close(cx); }))
                                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                                    cx.stop_propagation();
                                    this.handle_key_down(event, cx);
                                }))
                                .into_any_element()
                        } else {
                            div().flex_1().into_any_element()
                        },
                    )
                    .child(
                        div()
                            .id("search-case-sensitive-btn")
                            .cursor_pointer()
                            .w(px(24.0))
                            .h(px(24.0))
                            .ml(px(2.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(RADIUS_MD)
                            .when(case_sensitive, |s| s.bg(rgb(t.bg_selection)))
                            .hover(|s| s.bg(rgb(t.bg_hover)))
                            .tooltip(move |_, cx| {
                                let tip = case_tip.clone();
                                cx.new(|_| Tooltip::new(tip)).into()
                            })
                            .on_mouse_down(MouseButton::Left, |_, _, cx| { cx.stop_propagation(); })
                            .on_click(cx.listener(|this, _, _window, cx| { this.toggle_case_sensitive(cx); }))
                            .child(
                                div()
                                    .text_size(ui_text_md(cx))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(if case_sensitive { p.text_primary } else { p.text_secondary })
                                    .child("Aa"),
                            ),
                    )
                    .child(
                        div()
                            .id("search-regex-btn")
                            .cursor_pointer()
                            .w(px(24.0))
                            .h(px(24.0))
                            .ml(px(2.0))
                            .mr(px(2.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(RADIUS_MD)
                            .when(is_regex, |s| s.bg(rgb(t.bg_selection)))
                            .hover(|s| s.bg(rgb(t.bg_hover)))
                            .tooltip(move |_, cx| {
                                let tip = regex_tip.clone();
                                cx.new(|_| Tooltip::new(tip)).into()
                            })
                            .on_mouse_down(MouseButton::Left, |_, _, cx| { cx.stop_propagation(); })
                            .on_click(cx.listener(|this, _, _window, cx| { this.toggle_regex(cx); }))
                            .child(
                                div()
                                    .text_size(ui_text_md(cx))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(if is_regex { p.text_primary } else { p.text_secondary })
                                    .child(".*"),
                            ),
                    )
            )
            .child(
                div()
                    .text_size(ui_text_md(cx))
                    .text_color(p.text_secondary)
                    .min_w(px(40.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(match_text),
            )
            .child(
                icon_button_sized_px("search-prev-btn", AppIcon::ChevronUp, px(26.0), ICON_MD, &t)
                    .tooltip(move |_, cx| {
                        let tip = prev_tip.clone();
                        cx.new(|_| Tooltip::new(tip)).into()
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| { cx.stop_propagation(); })
                    .on_click(cx.listener(|this, _, _window, cx| { this.prev_match(cx); })),
            )
            .child(
                icon_button_sized_px("search-next-btn", AppIcon::ChevronDown, px(26.0), ICON_MD, &t)
                    .tooltip(move |_, cx| {
                        let tip = next_tip.clone();
                        cx.new(|_| Tooltip::new(tip)).into()
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| { cx.stop_propagation(); })
                    .on_click(cx.listener(|this, _, _window, cx| { this.next_match(cx); })),
            )
            .child(
                div()
                    .id("search-close-btn")
                    .flex_shrink_0()
                    .cursor_pointer()
                    .w(px(26.0))
                    .h(px(26.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(RADIUS_MD)
                    .hover(|s| s.bg(rgba(0xf14c4c44)))
                    .tooltip(move |_, cx| {
                        let tip = close_tip.clone();
                        cx.new(|_| Tooltip::new(tip)).into()
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| { cx.stop_propagation(); })
                    .on_click(cx.listener(|this, _, _window, cx| { this.close(cx); }))
                    .child(
                        AppIcon::Close
                            .size(ICON_MD)
                            .text_color(p.text_secondary),
                    ),
            )
    }
}

