use crate::design::semantic::SemanticPalette;
use crate::icon::AppIcon;
use crate::scrollable::{Scrollbar, ScrollbarShow};
use crate::tokens::{ui_text_md, ICON_STD, RADIUS_LG, RADIUS_SM, SPACE_MD, SPACE_XS};
use crate::Cancel;
use crate::input::{Input, InputEvent, InputState};
use gpui::prelude::*;
use gpui::*;
use std::path::PathBuf;
use std::sync::Arc;

/// A suggestion for path auto-completion
#[derive(Clone, Debug)]
pub struct PathSuggestion {
    /// Directory name only (for display)
    pub display_name: String,
    /// Complete path
    pub full_path: String,
    /// Whether this is a directory
    pub is_directory: bool,
    /// Whether this is the "select current folder" entry
    pub is_select_current: bool,
}

/// Event emitted when path value changes
#[derive(Clone, Debug)]
pub enum PathAutoCompleteEvent {
    Change(String),
}

/// Path auto-complete state wrapping InputState
pub struct PathAutoCompleteState {
    input: Entity<InputState>,
    placeholder: String,
    last_applied_placeholder: Option<String>,
    clearable: bool,
    suggestions: Vec<PathSuggestion>,
    selected_index: usize,
    show_suggestions: bool,
    focus_handle: FocusHandle,
    /// Scroll handle for suggestions dropdown (uniform list)
    pub uniform_scroll_handle: UniformListScrollHandle,
    /// When true, suppress the next InputEvent from triggering suggestions
    suppress_suggestions: bool,
}

impl EventEmitter<PathAutoCompleteEvent> for PathAutoCompleteState {}

impl PathAutoCompleteState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(cx).placeholder("Enter path...")
        });

        cx.subscribe(&input, |this, _input, _: &InputEvent, cx| {
            this.on_input_changed(cx);
        })
        .detach();

        let focus_handle = input.read(cx).focus_handle(cx);

        Self {
            input,
            placeholder: "Enter path...".to_string(),
            last_applied_placeholder: None,
            clearable: true,
            suggestions: Vec::new(),
            selected_index: 0,
            show_suggestions: false,
            focus_handle,
            uniform_scroll_handle: UniformListScrollHandle::new(),
            suppress_suggestions: false,
        }
    }

    pub fn set_placeholder(&mut self, placeholder: impl Into<String>) {
        self.placeholder = placeholder.into();
    }

    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    pub fn set_clearable(&mut self, clearable: bool) {
        self.clearable = clearable;
    }

    pub fn value(&self, cx: &App) -> String {
        self.input.read(cx).text().to_string()
    }

    #[allow(dead_code)]
    pub fn set_value(&mut self, value: impl Into<String>, cx: &mut Context<Self>) {
        let val: String = value.into();
        self.input.update(cx, |input, cx| {
            input.set_value(val, cx);
        });
        self.update_suggestions(cx);
    }

    /// Set value without triggering suggestions (e.g. from Browse picker)
    pub fn set_value_quiet(&mut self, value: impl Into<String>, cx: &mut Context<Self>) {
        self.suppress_suggestions = true;
        let val: String = value.into();
        self.input.update(cx, |input, cx| {
            input.set_value(val, cx);
        });
        self.hide_suggestions(cx);
    }

    #[allow(dead_code)]
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.focus(window, cx);
        });
    }

    #[allow(dead_code)]
    pub fn input(&self) -> &Entity<InputState> {
        &self.input
    }

    /// Returns true if suggestions should be shown
    pub fn has_suggestions(&self) -> bool {
        self.show_suggestions && !self.suggestions.is_empty()
    }

    /// Get the suggestions for external rendering
    pub fn suggestions(&self) -> &[PathSuggestion] {
        &self.suggestions
    }

    /// Get the selected index
    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// Get the uniform scroll handle for the suggestions dropdown
    pub fn uniform_scroll_handle(&self) -> &UniformListScrollHandle {
        &self.uniform_scroll_handle
    }

    /// Select a suggestion by index and complete it
    pub fn select_and_complete(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected_index = index;
        self.complete_selected(cx);
    }

    /// Expand ~ to home directory
    fn expand_path(path: &str) -> String {
        if path.starts_with('~')
            && let Some(home) = dirs::home_dir() {
                let rest = path.strip_prefix('~').unwrap_or("");
                return format!("{}{}", home.display(), rest);
            }
        path.to_string()
    }

    /// Get the directory to list and the prefix to filter by
    fn parse_path_for_completion(path: &str) -> (PathBuf, String) {
        let expanded = Self::expand_path(path);
        let path_buf = PathBuf::from(&expanded);

        if expanded.ends_with('/') || expanded.is_empty() {
            // List directory contents
            (path_buf, String::new())
        } else if path_buf.is_dir() {
            // If it's an existing directory without trailing slash, still list its contents
            (path_buf, String::new())
        } else {
            // Get parent directory and use filename as prefix filter
            let parent = path_buf.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
            let prefix = path_buf
                .file_name()
                .and_then(|n| n.to_str())
                .map(String::from)
                .unwrap_or_default();
            (parent, prefix)
        }
    }

    fn on_input_changed(&mut self, cx: &mut Context<Self>) {
        let val = self.value(cx);
        cx.emit(PathAutoCompleteEvent::Change(val));
        if self.suppress_suggestions {
            self.suppress_suggestions = false;
            return;
        }
        self.update_suggestions(cx);
    }

    fn update_suggestions(&mut self, cx: &mut Context<Self>) {
        let current_value = self.value(cx);

        // Don't show suggestions for empty input
        if current_value.is_empty() {
            self.suggestions.clear();
            self.show_suggestions = false;
            self.selected_index = 0;
            self.uniform_scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
            cx.notify();
            return;
        }

        let (dir_path, prefix) = Self::parse_path_for_completion(&current_value);

        // Listing a directory is blocking IO (slow on network mounts / huge dirs),
        // so run it off the main thread rather than per keystroke on the UI loop.
        let value_for_task = current_value.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let new_suggestions = cx
                .background_executor()
                .spawn(async move { Self::compute_suggestions(dir_path, prefix, value_for_task) })
                .await;

            let _ = this.update(cx, |this, cx| {
                // Guard against a stale result overwriting newer input: only apply
                // if the input hasn't changed since this lookup was kicked off.
                if this.value(cx) != current_value {
                    return;
                }
                this.suggestions = new_suggestions;
                this.show_suggestions = !this.suggestions.is_empty();
                this.selected_index = 0;
                this.uniform_scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            });
        })
        .detach();
    }

    /// Build the suggestion list for `current_value`. Performs blocking filesystem
    /// IO (`read_dir`, `is_dir`), so this must run off the GPUI main thread.
    fn compute_suggestions(dir_path: PathBuf, prefix: String, current_value: String) -> Vec<PathSuggestion> {
        let mut new_suggestions = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&dir_path) {
            for entry in entries.filter_map(|e| e.ok()) {
                let file_name = entry.file_name();
                let name = file_name.to_string_lossy().to_string();

                // Skip hidden files unless user typed a dot
                if name.starts_with('.') && !prefix.starts_with('.') {
                    continue;
                }

                // Filter by prefix (case-insensitive)
                if !prefix.is_empty() && !name.to_lowercase().starts_with(&prefix.to_lowercase()) {
                    continue;
                }

                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let full_path = entry.path().to_string_lossy().to_string();

                // Convert back to use ~ if the original path used it
                let display_full_path = if current_value.starts_with('~') {
                    if let Some(home) = dirs::home_dir() {
                        let home_str = home.to_string_lossy().to_string();
                        if full_path.starts_with(&home_str) {
                            format!("~{}", &full_path[home_str.len()..])
                        } else {
                            full_path.clone()
                        }
                    } else {
                        full_path.clone()
                    }
                } else {
                    full_path.clone()
                };

                new_suggestions.push(PathSuggestion {
                    display_name: name,
                    full_path: display_full_path,
                    is_directory: is_dir,
                    is_select_current: false,
                });
            }
        }

        // Sort: directories first, then alphabetically
        new_suggestions.sort_by(|a, b| {
            match (a.is_directory, b.is_directory) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()),
            }
        });

        // Limit suggestions to at most 50 items
        new_suggestions.truncate(50);

        // Add "Select this folder" at the top when the current path is a valid directory
        let expanded = Self::expand_path(&current_value);
        let expanded_path = PathBuf::from(&expanded);
        if expanded_path.is_dir() && !new_suggestions.is_empty() {
            new_suggestions.insert(0, PathSuggestion {
                display_name: "Select this folder".to_string(),
                full_path: current_value.clone(),
                is_directory: true,
                is_select_current: true,
            });
        }

        new_suggestions
    }

    fn complete_selected(&mut self, cx: &mut Context<Self>) {
        // Clone suggestion data before borrowing self mutably
        let suggestion_data = self.suggestions.get(self.selected_index)
            .map(|s| (s.full_path.clone(), s.is_directory, s.is_select_current));

        if let Some((full_path, is_directory, is_select_current)) = suggestion_data {
            // "Select this folder" — just confirm the current path
            if is_select_current {
                self.hide_suggestions(cx);
                return;
            }

            let mut path = full_path;
            if is_directory && !path.ends_with('/') {
                path.push('/');
            }
            self.suppress_suggestions = true;
            self.input.update(cx, |input, cx| {
                input.set_value(path.clone(), cx);
            });
            cx.emit(PathAutoCompleteEvent::Change(path.clone()));
            self.suggestions.clear();
            self.show_suggestions = false;
            self.selected_index = 0;
            self.uniform_scroll_handle.scroll_to_item(0, ScrollStrategy::Top);

            // If it's a directory, update suggestions for the new path
            if is_directory {
                self.update_suggestions(cx);
            }
        }
        cx.notify();
    }

    fn select_previous(&mut self, cx: &mut Context<Self>) {
        if self.suggestions.is_empty() {
            return;
        }
        if self.selected_index == 0 {
            self.selected_index = self.suggestions.len() - 1;
            self.uniform_scroll_handle.scroll_to_item(self.selected_index, ScrollStrategy::Bottom);
        } else {
            self.selected_index -= 1;
            self.uniform_scroll_handle.scroll_to_item(self.selected_index, ScrollStrategy::Top);
        }
        cx.notify();
    }

    fn select_next(&mut self, cx: &mut Context<Self>) {
        if self.suggestions.is_empty() {
            return;
        }
        if self.selected_index >= self.suggestions.len() - 1 {
            self.selected_index = 0;
            self.uniform_scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
        } else {
            self.selected_index += 1;
            self.uniform_scroll_handle.scroll_to_item(self.selected_index, ScrollStrategy::Bottom);
        }
        cx.notify();
    }

    pub fn hide_suggestions(&mut self, cx: &mut Context<Self>) {
        self.show_suggestions = false;
        self.suggestions.clear();
        self.selected_index = 0;
        self.uniform_scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn handle_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let key = event.keystroke.key.as_str();

        match key {
            "tab"
                if self.show_suggestions && !self.suggestions.is_empty() => {
                    self.complete_selected(cx);
                    return true;
                }
            "up"
                if self.show_suggestions => {
                    self.select_previous(cx);
                    return true;
                }
            "down"
                if self.show_suggestions => {
                    self.select_next(cx);
                    return true;
                }
            "escape"
                if self.show_suggestions => {
                    self.hide_suggestions(cx);
                    return true;
                }
            "enter"
                if self.show_suggestions && !self.suggestions.is_empty() => {
                    self.complete_selected(cx);
                    return true;
                }
            _ => {}
        }

        false
    }
}

impl Render for PathAutoCompleteState {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.last_applied_placeholder.as_deref() != Some(&self.placeholder) {
            let ph = self.placeholder.clone();
            self.last_applied_placeholder = Some(ph.clone());
            self.input.update(cx, |input, _| {
                input.set_placeholder(ph);
            });
        }

        let input_elem = Input::new(&self.input)
            .cleanable(self.clearable);

        div()
            .w_full()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &Cancel, _, cx| {
                if this.show_suggestions {
                    this.hide_suggestions(cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if this.handle_key_down(event, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(input_elem)
    }
}

impl Focusable for PathAutoCompleteState {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }
}

/// Render path-completion suggestions as a virtualized floating dropdown.
///
/// Limits display to at most 8 items at a time (approx 224px + padding), with
/// scrollbar and keyboard arrow navigation following.
pub fn render_path_suggestions(
    id: impl Into<SharedString>,
    input_state: &Entity<PathAutoCompleteState>,
    cx: &App,
) -> impl IntoElement {
    let p = SemanticPalette::from_context(cx);
    let pstate = input_state.read(cx);
    let suggestions = pstate.suggestions.clone();
    let selected_index = pstate.selected_index;
    let uniform_scroll_handle = pstate.uniform_scroll_handle.clone();
    let pstate_entity = input_state.clone();
    let count = suggestions.len();
    let visible_count = count.min(8);
    let list_h = px(28.0 * visible_count as f32);
    let show_scrollbar = count > 8;

    let id_str: SharedString = id.into();
    let id_str_closure = id_str.clone();
    let suggestions_rc = Arc::new(suggestions);

    let list_id = ElementId::Name(format!("{}-ulist", id_str).into());
    let ulist = uniform_list(
        list_id,
        count,
        move |range, _window, cx| {
            let p = SemanticPalette::from_context(cx);
            let mut items = Vec::with_capacity(range.len());
            for i in range {
                if let Some(s) = suggestions_rc.get(i) {
                    let is_selected = i == selected_index;
                    let pstate_entity = pstate_entity.clone();
                    let s_display_name = s.display_name.clone();
                    let icon = if s.is_select_current {
                        AppIcon::Check
                    } else if s.is_directory {
                        AppIcon::Folder
                    } else {
                        AppIcon::File
                    };

                    let item = div()
                        .id(ElementId::Name(format!("{}-sugg-{}", id_str_closure, i).into()))
                        .h(px(28.0))
                        .px(SPACE_MD)
                        .rounded(RADIUS_SM)
                        .cursor_pointer()
                        .when(is_selected, |d| d.bg(p.surface_selection))
                        .hover(|d| d.bg(p.surface_hover))
                        .flex()
                        .items_center()
                        .gap(SPACE_MD)
                        .child(
                            icon.size(ICON_STD)
                                .text_color(p.text_muted),
                        )
                        .child(
                            div()
                                .text_size(ui_text_md(cx))
                                .text_color(p.text_primary)
                                .truncate()
                                .child(s_display_name),
                        )
                        .on_mouse_down(MouseButton::Left, move |_, _window, cx| {
                            pstate_entity.update(cx, |st, cx| st.select_and_complete(i, cx));
                        });
                    items.push(item.into_any_element());
                }
            }
            items
        },
    )
    .track_scroll(&uniform_scroll_handle)
    .size_full();

    let scrollbar_id = ElementId::Name(format!("{}-scrollbar", id_str).into());
    let mut container = div()
        .id(ElementId::Name(id_str))
        .occlude()
        .w_full()
        .h(list_h + px(8.0))
        .bg(p.surface_overlay)
        .border_1()
        .border_color(p.border_subtle)
        .rounded(RADIUS_LG)
        .shadow_xl()
        .p(SPACE_XS)
        .relative()
        .on_scroll_wheel(|_, _, cx| {
            cx.stop_propagation();
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .child(ulist);

    if show_scrollbar {
        container = container.child(
            div()
                .absolute()
                .top(SPACE_XS)
                .bottom(SPACE_XS)
                .right(SPACE_XS)
                .w(px(12.0))
                .child(
                    Scrollbar::vertical(&uniform_scroll_handle)
                        .id(scrollbar_id)
                        .scrollbar_show(ScrollbarShow::Hover),
                ),
        );
    }

    container
}
