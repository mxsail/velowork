//! Generic Quick Picker / Command Palette component for searchable modal navigation.
//!
//! # Architecture & Responsibilities vs `Select`:
//! - **`QuickPicker`** ([`QuickPickerState`], [`QuickPickerConfig`]): For **Global Modal Command Palettes & Quick Open Dialogs**
//!   (e.g., Project Switcher `Ctrl+P`, Theme Selector `Ctrl+K Ctrl+T`, Shell Selector, Command Palette `Ctrl+Shift+P`).
//!   These are independent modal overlays centered or anchored at the top of the window with full keyboard navigation and search.
//! - **`Select`** ([`Select`](crate::select::Select)): For **In-Form Dropdown Controls** (e.g. settings form fields, baud rate selectors)
//!   which anchor below/above a specific trigger element in a form or dialog.

use gpui::prelude::FluentBuilder;
use gpui::*;

use velowork_i18n::{i18n, t_fmt};

/// Configuration for a quick picker modal.
#[derive(Clone)]
pub struct QuickPickerConfig {
    /// Width of the modal in pixels.
    pub width: f32,
    /// Maximum height of the modal in pixels.
    pub max_height: f32,
    /// Title shown in the modal header.
    pub title: String,
    /// Optional subtitle shown below the title.
    pub subtitle: Option<String>,
    /// Placeholder text for the search input. If None, search is disabled.
    pub search_placeholder: Option<String>,
    /// Message shown when the list is empty after filtering.
    pub empty_message: String,
    /// Keyboard hints shown in the footer as (key, description) pairs.
    pub keyboard_hints: Vec<(String, String)>,
    /// Whether to center the modal vertically (true) or position at top (false).
    pub centered: bool,
    /// Key context for keyboard shortcuts (e.g., "CommandPalette").
    pub key_context: String,
}

impl QuickPickerConfig {
    /// Create a new config with default values.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            width: 500.0,
            max_height: 450.0,
            title: title.into(),
            subtitle: None,
            search_placeholder: None,
            empty_message: "No items found".to_string(),
            keyboard_hints: vec![
                ("Enter".to_string(), "select".to_string()),
                ("Esc".to_string(), "close".to_string()),
            ],
            centered: false,
            key_context: "QuickPicker".to_string(),
        }
    }

    /// Set the subtitle.
    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Enable search with the given placeholder.
    pub fn searchable(mut self, placeholder: impl Into<String>) -> Self {
        self.search_placeholder = Some(placeholder.into());
        self
    }

    /// Set the modal dimensions.
    pub fn size(mut self, width: f32, max_height: f32) -> Self {
        self.width = width;
        self.max_height = max_height;
        self
    }

    /// Center the modal vertically.
    pub fn centered(mut self) -> Self {
        self.centered = true;
        self
    }

    /// Set the key context for keyboard shortcuts.
    pub fn key_context(mut self, context: impl Into<String>) -> Self {
        self.key_context = context.into();
        self
    }

    /// Set the empty message.
    pub fn empty_message(mut self, message: impl Into<String>) -> Self {
        self.empty_message = message.into();
        self
    }

    /// Set keyboard hints as (key, description) pairs.
    pub fn keyboard_hints(mut self, hints: Vec<(impl Into<String>, impl Into<String>)>) -> Self {
        self.keyboard_hints = hints
            .into_iter()
            .map(|(k, d)| (k.into(), d.into()))
            .collect();
        self
    }

    /// Check if search is enabled.
    pub fn has_search(&self) -> bool {
        self.search_placeholder.is_some()
    }
}

/// Result of filtering an item.
#[derive(Clone, Debug, Default)]
pub struct FilterResult {
    /// Original index in the items list.
    pub index: usize,
}

impl FilterResult {
    pub fn new(index: usize) -> Self {
        Self { index }
    }
}

/// Shared state for quick pickers.
pub struct QuickPickerState<T: Clone> {
    /// Focus handle for keyboard events.
    pub focus_handle: FocusHandle,
    /// Scroll handle for list scrolling.
    pub scroll_handle: ScrollHandle,
    /// All items in the list.
    pub items: Vec<T>,
    /// Filtered items (indices).
    pub filtered: Vec<FilterResult>,
    /// Currently selected index (into filtered list).
    pub selected_index: usize,
    /// Current search query.
    pub search_query: String,
    /// Configuration.
    pub config: QuickPickerConfig,
}

impl<T: Clone> QuickPickerState<T> {
    /// Create a new state with the given items and config.
    pub fn new(items: Vec<T>, config: QuickPickerConfig, cx: &mut App) -> Self {
        let filtered: Vec<FilterResult> = (0..items.len())
            .map(FilterResult::new)
            .collect();

        Self {
            focus_handle: cx.focus_handle(),
            scroll_handle: ScrollHandle::new(),
            items,
            filtered,
            selected_index: 0,
            search_query: String::new(),
            config,
        }
    }

    /// Create a new state with a pre-selected index.
    pub fn with_selected(items: Vec<T>, config: QuickPickerConfig, selected_index: usize, cx: &mut App) -> Self {
        let mut state = Self::new(items, config, cx);
        state.selected_index = selected_index.min(state.filtered.len().saturating_sub(1));
        state
    }

    /// Get the currently selected item, if any.
    pub fn selected_item(&self) -> Option<&T> {
        self.filtered
            .get(self.selected_index)
            .map(|f| &self.items[f.index])
    }

    /// Move selection up.
    pub fn select_prev(&mut self) -> bool {
        if self.selected_index > 0 {
            self.selected_index -= 1;
            self.scroll_to_selected();
            true
        } else {
            false
        }
    }

    /// Move selection down.
    pub fn select_next(&mut self) -> bool {
        if self.selected_index < self.filtered.len().saturating_sub(1) {
            self.selected_index += 1;
            self.scroll_to_selected();
            true
        } else {
            false
        }
    }

    /// Scroll to keep the selected item visible.
    pub fn scroll_to_selected(&self) {
        if !self.filtered.is_empty() {
            self.scroll_handle.scroll_to_item(self.selected_index);
        }
    }

    /// Add a character to the search query.
    pub fn push_search_char(&mut self, ch: char) {
        self.search_query.push(ch);
    }

    /// Remove the last character from the search query.
    pub fn pop_search_char(&mut self) -> bool {
        if !self.search_query.is_empty() {
            self.search_query.pop();
            true
        } else {
            false
        }
    }

    /// Update the filtered list and reset selection to first item.
    pub fn set_filtered(&mut self, filtered: Vec<FilterResult>) {
        self.filtered = filtered;
        self.selected_index = 0;
    }

    /// Check if the list is empty after filtering.
    pub fn is_empty(&self) -> bool {
        self.filtered.is_empty()
    }
}

/// Actions that can result from key handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickPickerAction {
    /// Close the overlay.
    Close,
    /// Move selection up.
    SelectPrev,
    /// Move selection down.
    SelectNext,
    /// Confirm the current selection.
    Confirm,
    /// Search query changed (character added or removed).
    QueryChanged,
    /// A custom action was triggered (e.g., "space" for toggle visibility).
    Custom(String),
    /// No action taken.
    None,
}

/// Characters allowed in search queries.
const SEARCH_CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 -_./";

/// Handle keyboard events for a quick picker.
pub fn handle_quick_picker_key<T: Clone>(
    state: &mut QuickPickerState<T>,
    event: &KeyDownEvent,
    extra_keys: &[(&str, &str)],
) -> QuickPickerAction {
    let key = event.keystroke.key.as_str();

    // Check extra keys first
    for &(k, action) in extra_keys {
        if key == k {
            return QuickPickerAction::Custom(action.to_string());
        }
    }

    match key {
        "escape" => QuickPickerAction::Close,
        "up" => {
            if state.select_prev() {
                QuickPickerAction::SelectPrev
            } else {
                QuickPickerAction::None
            }
        }
        "down" => {
            if state.select_next() {
                QuickPickerAction::SelectNext
            } else {
                QuickPickerAction::None
            }
        }
        "enter" => QuickPickerAction::Confirm,
        "backspace" => {
            if state.config.has_search() && state.pop_search_char() {
                QuickPickerAction::QueryChanged
            } else {
                QuickPickerAction::None
            }
        }
        key if key.len() == 1 && state.config.has_search() => {
            let Some(ch) = key.chars().next() else {
                return QuickPickerAction::None;
            };
            if SEARCH_CHARS.contains(ch) {
                state.push_search_char(ch);
                QuickPickerAction::QueryChanged
            } else {
                QuickPickerAction::None
            }
        }
        _ => QuickPickerAction::None,
    }
}

/// Navigation helper for uniform list overlays.
pub fn select_prev(selected_index: &mut usize, scroll_handle: &UniformListScrollHandle) -> bool {
    if *selected_index > 0 {
        *selected_index -= 1;
        scroll_handle.scroll_to_item(*selected_index, ScrollStrategy::Top);
        true
    } else {
        false
    }
}

/// Move selection down. Returns true if the selection changed.
pub fn select_next(
    selected_index: &mut usize,
    total: usize,
    scroll_handle: &UniformListScrollHandle,
) -> bool {
    if *selected_index < total.saturating_sub(1) {
        *selected_index += 1;
        scroll_handle.scroll_to_item(*selected_index, ScrollStrategy::Top);
        true
    } else {
        false
    }
}

/// Render a search input row with ">" prompt prefix and an InputState.
pub fn search_input_row(
    input: &Entity<crate::input::InputState>,
    t: &velowork_core::theme::ThemeColors,
    cx: &App,
) -> Div {
    use crate::input::Input;
    use crate::tokens::ui_text_ms;

    div()
        .px(px(12.0))
        .py(px(6.0))
        .border_b_1()
        .border_color(rgb(t.border))
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .text_size(ui_text_ms(cx))
                .text_color(rgb(t.text_muted))
                .child(">"),
        )
        .child(
            div()
                .flex_1()
                .child(Input::new(input)),
        )
}

/// Substring filter for list items.
pub fn substring_filter<T, F>(items: &[T], query: &str, get_fields: F) -> Vec<FilterResult>
where
    F: Fn(&T) -> Vec<String>,
{
    if query.is_empty() {
        return (0..items.len()).map(FilterResult::new).collect();
    }

    let query_lower = query.to_lowercase();
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            get_fields(item)
                .iter()
                .any(|field| field.to_lowercase().contains(&query_lower))
        })
        .map(|(i, _)| FilterResult::new(i))
        .collect()
}

/// Render a "filters" toggle button that opens a popover.
pub fn file_filter_button(
    id: impl Into<SharedString>,
    active_count: u8,
    t: &velowork_core::theme::ThemeColors,
    cx: &App,
    on_click: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    on_bounds: impl Fn(Bounds<Pixels>, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    use crate::tokens::{ui_text_sm, RADIUS_STD};

    div()
        .id(ElementId::Name(id.into()))
        .cursor_pointer()
        .px(px(8.0))
        .py(px(3.0))
        .rounded(RADIUS_STD)
        .text_size(ui_text_sm(cx))
        .font_weight(FontWeight::MEDIUM)
        .when(active_count > 0, |d: Stateful<Div>| {
            d.bg(rgb(t.border_active))
                .text_color(rgb(t.text_primary))
        })
        .when(active_count == 0, |d: Stateful<Div>| {
            d.bg(rgb(t.bg_secondary))
                .text_color(rgb(t.text_muted))
        })
        .hover(|s: StyleRefinement| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(MouseButton::Left, on_click)
        .child(if active_count > 0 {
            t_fmt(cx, "search.file_search.filter.filters_count", &[("count", &active_count.to_string())])
        } else {
            i18n!(cx, "search.file_search.filter.filters")
        })
        .child(canvas(on_bounds, |_, _, _, _| {}).absolute().size_full())
}

/// Render the file filter popover panel anchored below the given button bounds.
pub fn file_filter_popover(
    bounds: Bounds<Pixels>,
    show_ignored: bool,
    t: &velowork_core::theme::ThemeColors,
    cx: &App,
    on_toggle: impl Fn(&str, &mut Window, &mut App) + 'static,
) -> Deferred {
    use crate::popover::popover_panel;

    deferred(
        anchored()
            .position(point(bounds.origin.x, bounds.origin.y + bounds.size.height + px(2.0)))
            .snap_to_window()
            .child(
                popover_panel("filter-popover", t)
                    .min_w(px(180.0))
                    .py(px(4.0))
                    .child(
                        file_filter_option("ignored", &i18n!(cx, "search.file_search.filter.include_gitignored"), show_ignored, t, cx,
                            move |_, window, cx| on_toggle("ignored", window, cx))
                    )
            ),
    )
}

fn file_filter_option(
    id: &str,
    label: &str,
    active: bool,
    t: &velowork_core::theme::ThemeColors,
    cx: &App,
    on_click: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    use crate::tokens::{ui_text_sm, RADIUS_SM};

    div()
        .id(ElementId::Name(format!("filter-{}", id).into()))
        .mx(px(4.0))
        .px(px(10.0))
        .py(px(6.0))
        .rounded(RADIUS_SM)
        .cursor_pointer()
        .text_size(ui_text_sm(cx))
        .text_color(rgb(t.text_primary))
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .flex()
        .items_center()
        .justify_between()
        .child(label.to_string())
        .when(active, |d: Stateful<Div>| {
            d.child(
                div()
                    .text_size(ui_text_sm(cx))
                    .text_color(rgb(t.border_active))
                    .child("✓"),
            )
        })
        .on_mouse_down(MouseButton::Left, on_click)
}
