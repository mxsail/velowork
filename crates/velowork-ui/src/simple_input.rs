use crate::design::appearance::{ControlAppearance, ControlSize, ControlVariant};
use crate::design::semantic::SemanticPalette;
use crate::icon::AppIcon;
use crate::input::focus_ring_shadows;
use crate::theme::{surface_bg_t, theme};
use crate::tokens::{RADIUS_STD, RADIUS_XS};
use crate::tooltip::Tooltip;
use crate::text_utils::{find_word_boundaries, is_word_char};
use gpui::prelude::*;
use gpui::*;
use unicode_segmentation::UnicodeSegmentation;
use velowork_i18n::i18n;

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Event emitted when input value changes
pub struct InputChangedEvent;

/// Event emitted when input is clicked/focused
#[derive(Clone)]
pub struct InputFocusedEvent;

/// Standard input event enum compatible with view subscriptions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputEvent {
    Change,
    PressEnter,
    Focus,
    Blur,
}

/// Result of key handling
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyHandled {
    /// Key was handled, stop propagation
    Handled,
    /// Key was not handled, let parent handle
    NotHandled,
    /// Key was ignored, don't stop propagation
    Ignored,
}

/// Result of key interceptor callback
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyInterceptResult {
    /// Keystroke unhandled by interceptor; input proceeds with default handling
    Unhandled,
    /// Keystroke fully handled; input stops propagation and does not edit text
    Handled,
    /// Keystroke handled and requests updating the input value
    SetValue(String),
}

#[derive(Clone)]
struct HistoryEntry {
    value: String,
    cursor_position: usize,
    selection: Option<Range<usize>>,
}

/// Internal layout and geometry cache populated during prepaint/paint without triggering GPUI entity dirty notifications.
#[derive(Default)]
pub(crate) struct SimpleInputLayoutCache {
    pub layouts: Vec<ShapedLine>,
    pub bounds: Option<Bounds<Pixels>>,
    pub row_ranges: Vec<Range<usize>>,
    pub line_height: Pixels,
    pub font_size: Pixels,
    pub scroll_offset: Pixels,
    pub scroll_offset_y: Pixels,
}

/// A simple text input state that handles selection, cursor, IME, and scrolling
pub struct SimpleInputState {
    focus_handle: FocusHandle,
    value: String,
    placeholder: String,
    cursor_position: usize, // UTF-8 byte offset
    selection: Option<Range<usize>>, // UTF-8 byte range
    selection_reversed: bool,
    cursor_visible: bool,
    _blink_task: Option<Task<()>>,
    icon: Option<AppIcon>,
    highlight_vars: bool,
    syntax_language: Option<String>,
    multiline: bool,
    wrap: bool, // For multiline inputs: word-wrap long lines (true) or keep each
    // logical line on a single visual row with horizontal scrolling (false).
    // Code/command-style inputs with a line gutter should set this to false so
    // that continuous paths/commands are never broken across visual rows.
    multiline_visible_rows: usize, // Visible rows for multiline inputs (ignored in fill_height mode)
    fill_height: bool, // When true, the element fills parent container height instead of using a fixed row count
    container_height: bool, // When true (single-line), skip fixed height and let parent container control sizing
    password: bool,
    show_gutter: bool,
    text_align: TextAlign,
    marked_range: Option<Range<usize>>, // UTF-8 byte range

    // When true (multiline), the viewport auto-follows the cursor so newly
    // typed lines stay visible ("新增行始终可见"). Manual scrolling (wheel or
    // scrollbar drag) sets this false so the user can freely review earlier
    // content ("随时回溯"); any edit or cursor move re-enables it via
    // `reset_cursor_blink`.
    follow_cursor: bool,

    // Vertical scrollbar drag state (multiline only). When true we are actively
    // dragging the scrollbar, so the viewport stays put instead of auto-following
    // the cursor.
    is_scrollbar_dragging: bool,

    // Undo/Redo stacks
    undo_stack: Vec<HistoryEntry>,
    redo_stack: Vec<HistoryEntry>,
    last_undo_value: String,
    last_undo_time: Option<Instant>,

    // Mouse drag selection
    is_selecting: bool,
    select_anchor: usize, // UTF-8 byte offset
    last_mouse_position: Option<Point<Pixels>>,
    _drag_scroll_task: Option<Task<()>>,
    submit_on_enter: bool,
    read_only: bool,
    allow_clear: bool,
    search_highlights: Vec<Range<usize>>,
    search_current_match: Option<Range<usize>>,

    // Layout representation for hit-testing and painting (cached via RefCell)
    layout_cache: RefCell<SimpleInputLayoutCache>,
    #[allow(clippy::type_complexity)]
    key_interceptor: Option<std::sync::Arc<dyn Fn(&KeyDownEvent, &str, &mut Context<Self>) -> KeyInterceptResult + 'static>>,
    digits_only: bool,
    max_length: Option<usize>,
    max_number: Option<u64>,
    content_padding: Option<Pixels>,
    auto_height: bool,
}

impl SimpleInputState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();

        // Start cursor blink task
        let blink_task = cx.spawn(async move |this: WeakEntity<SimpleInputState>, cx| {
            loop {
                smol::Timer::after(Duration::from_millis(530)).await;
                let done = this
                    .update(&mut *cx, |state, cx| {
                        if state.read_only {
                            return;
                        }
                        state.cursor_visible = !state.cursor_visible;
                        cx.notify();
                    })
                    .is_err();
                if done {
                    break;
                }
            }
        });

        Self {
            focus_handle,
            value: String::new(),
            placeholder: String::new(),
            cursor_position: 0,
            selection: None,
            selection_reversed: false,
            cursor_visible: true,
            _blink_task: Some(blink_task),
            icon: None,
            highlight_vars: false,
            syntax_language: None,
            multiline: false,
            wrap: true,
            multiline_visible_rows: 4,
            fill_height: false,
            container_height: false,
            password: false,
            show_gutter: false,
            text_align: TextAlign::Left,
            marked_range: None,
            follow_cursor: true,
            is_scrollbar_dragging: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_undo_value: String::new(),
            last_undo_time: None,
            is_selecting: false,
            select_anchor: 0,
            last_mouse_position: None,
            _drag_scroll_task: None,
            submit_on_enter: false,
            read_only: false,
            allow_clear: false,
            search_highlights: Vec::new(),
            search_current_match: None,
            layout_cache: RefCell::new(SimpleInputLayoutCache {
                line_height: px(18.0),
                font_size: px(12.0),
                ..Default::default()
            }),
            key_interceptor: None,
            digits_only: false,
            max_length: None,
            max_number: None,
            content_padding: None,
            auto_height: false,
        }
    }

    pub fn auto_height(mut self, auto: bool) -> Self {
        self.auto_height = auto;
        self
    }

    pub fn set_auto_height(&mut self, auto: bool) {
        self.auto_height = auto;
    }

    pub fn content_padding(mut self, pad: impl Into<Pixels>) -> Self {
        self.content_padding = Some(pad.into());
        self
    }

    pub fn set_content_padding(&mut self, pad: impl Into<Pixels>) {
        self.content_padding = Some(pad.into());
    }

    pub fn digits_only(mut self, val: bool) -> Self {
        self.digits_only = val;
        self
    }

    pub fn set_digits_only(&mut self, val: bool) {
        self.digits_only = val;
    }

    pub fn max_length(mut self, max: usize) -> Self {
        self.max_length = Some(max);
        self
    }

    pub fn set_max_length(&mut self, max: Option<usize>) {
        self.max_length = max;
    }

    pub fn max_number(mut self, max: u64) -> Self {
        self.max_number = Some(max);
        self
    }

    pub fn set_max_number(&mut self, max: Option<u64>) {
        self.max_number = max;
    }

    pub fn sanitize_input_text(&self, text: &str) -> String {
        let mut result = text.to_string();
        if self.digits_only {
            result = result.chars().filter(|c| c.is_ascii_digit()).collect();
        }
        if let Some(max) = self.max_length {
            result = result.chars().take(max).collect();
        }
        if let Some(max_num) = self.max_number {
            if let Ok(num) = result.parse::<u64>() {
                if num > max_num {
                    result = max_num.to_string();
                }
            }
        }
        result
    }

    pub fn set_key_interceptor<F>(&mut self, handler: F)
    where
        F: Fn(&KeyDownEvent, &str, &mut Context<Self>) -> KeyInterceptResult + 'static,
    {
        self.key_interceptor = Some(std::sync::Arc::new(handler));
    }

    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn submit_on_enter(mut self, submit: bool) -> Self {
        self.submit_on_enter = submit;
        self
    }

    pub fn read_only(mut self, ro: bool) -> Self {
        self.read_only = ro;
        self
    }

    pub fn set_read_only(&mut self, ro: bool) {
        self.read_only = ro;
    }

    /// When true, displays a clear icon on the right side when the input contains text.
    pub fn allow_clear(mut self, allow: bool) -> Self {
        self.allow_clear = allow;
        self
    }

    /// Alias for [`allow_clear`].
    #[allow(non_snake_case)]
    pub fn allowClear(self, allow: bool) -> Self {
        self.allow_clear(allow)
    }

    pub fn set_allow_clear(&mut self, allow: bool, cx: &mut Context<Self>) {
        self.allow_clear = allow;
        cx.notify();
    }

    /// Alias for [`set_allow_clear`].
    #[allow(non_snake_case)]
    pub fn set_allowClear(&mut self, allow: bool, cx: &mut Context<Self>) {
        self.set_allow_clear(allow, cx);
    }

    pub fn is_allow_clear(&self) -> bool {
        self.allow_clear
    }

    pub fn set_search_highlights(&mut self, highlights: Vec<Range<usize>>, current_match: Option<Range<usize>>) {
        self.search_highlights = highlights;
        self.search_current_match = current_match;
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selection.as_ref().map(|sel| {
            self.value[sel.clone()].to_string()
        })
    }

    pub fn last_bounds(&self) -> Option<Bounds<Pixels>> {
        self.layout_cache.borrow().bounds
    }

    pub fn set_selection(&mut self, range: Option<Range<usize>>, reversed: bool, cx: &mut Context<Self>) {
        self.selection = range;
        self.selection_reversed = reversed;
        cx.notify();
    }

    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if self.selection.is_some() {
            self.selection = None;
            cx.notify();
        }
    }

    pub fn set_placeholder(&mut self, placeholder: impl Into<String>) {
        self.placeholder = placeholder.into();
    }

    pub fn get_placeholder(&self) -> &str {
        &self.placeholder
    }

    pub fn default_value(mut self, value: impl Into<String>) -> Self {
        let v = self.sanitize_input_text(&value.into());
        self.cursor_position = v.len();
        self.value = v;
        self
    }

    pub fn icon(mut self, icon: impl Into<AppIcon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn highlight_vars(mut self) -> Self {
        self.highlight_vars = true;
        self
    }

    pub fn syntax_language(mut self, lang: Option<&str>) -> Self {
        self.syntax_language = lang.map(|s| s.to_string());
        self
    }

    pub fn set_syntax_language(&mut self, lang: Option<&str>, cx: &mut Context<Self>) {
        self.syntax_language = lang.map(|s| s.to_string());
        cx.notify();
    }

    pub fn multiline(mut self) -> Self {
        self.multiline = true;
        self
    }

    /// For multiline inputs only: when `wrap` is false, each logical line is
    /// kept on a single visual row and overflows horizontally (the element
    /// already scrolls the text horizontally). Use this for code/command-style
    /// inputs (e.g. ones with a line-number gutter) so that continuous
    /// content such as `cd /home/user` is never broken across visual rows.
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    pub fn set_wrap(&mut self, wrap: bool) {
        self.wrap = wrap;
    }

    /// Set the number of visible rows for a multiline input. The element gets a
    /// definite height of `rows * line_height` so it never collapses to zero
    /// height (which would make it unclickable). Content beyond this height
    /// scrolls internally.
    pub fn multiline_rows(mut self, rows: usize) -> Self {
        self.multiline_visible_rows = rows.max(1);
        self
    }

    /// When true (and multiline), the element fills its parent container's height
    /// instead of computing a fixed pixel height from `multiline_visible_rows`.
    /// Use this when the input is placed inside a resizable wrapper.
    pub fn fill_height(mut self, fill: bool) -> Self {
        self.fill_height = fill;
        self
    }

    /// When true (and multiline), the element fills its parent container's height
    /// instead of computing a fixed pixel height from `multiline_visible_rows`.
    /// Use this when the input is placed inside a resizable wrapper.
    pub fn set_fill_height(&mut self, fill: bool) {
        self.fill_height = fill;
    }

    /// When true, single-line inputs skip the built-in fixed height and
    /// fill the parent container height instead.
    pub fn set_container_height(&mut self, val: bool) {
        self.container_height = val;
    }

    pub fn set_show_gutter(&mut self, show: bool) {
        self.show_gutter = show;
    }

    pub fn masked(mut self, masked: bool) -> Self {
        self.password = masked;
        self
    }

    pub fn cleanable(mut self, cleanable: bool) -> Self {
        self.allow_clear = cleanable;
        self
    }

    pub fn password(mut self) -> Self {
        self.password = true;
        self
    }

    /// 运行时切换密码掩码状态（用于「显示/隐藏密码」切换按钮）。
    pub fn set_password(&mut self, password: bool) {
        self.password = password;
    }

    pub fn is_password(&self) -> bool {
        self.password
    }

    pub fn is_masked(&self) -> bool {
        self.password
    }

    pub fn text_align(mut self, align: TextAlign) -> Self {
        self.text_align = align;
        self
    }

    pub fn set_text_align(&mut self, align: TextAlign, cx: &mut Context<Self>) {
        self.text_align = align;
        cx.notify();
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn text(&self) -> &str {
        &self.value
    }

    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.set_value(text, cx);
    }

    pub fn scroll_offset_y(&self) -> Pixels {
        self.layout_cache.borrow().scroll_offset_y
    }

    pub fn last_line_height(&self) -> Pixels {
        self.layout_cache.borrow().line_height
    }

    pub fn current_line(&self) -> usize {
        let text_up_to_cursor = if self.cursor_position <= self.value.len() {
            &self.value[..self.cursor_position]
        } else {
            &self.value
        };
        text_up_to_cursor.split('\n').count()
    }

    pub fn is_multiline(&self) -> bool {
        self.multiline
    }

    pub fn set_value(&mut self, value: impl Into<String>, cx: &mut Context<Self>) {
        let v = self.sanitize_input_text(&value.into());
        if v == self.value {
            return;
        }
        self.value = v;
        self.cursor_position = self.value.len();
        self.selection = None;
        self.selection_reversed = false;
        self.marked_range = None;
        self.layout_cache.borrow_mut().scroll_offset = px(0.0);
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.last_undo_value = self.value.clone();
        self.last_undo_time = None;
        self.layout_cache.borrow_mut().row_ranges.clear();
        self.emit_change(cx);
        cx.notify();
    }

    fn emit_change(&self, cx: &mut Context<Self>) {
        cx.emit(InputChangedEvent);
        cx.emit(InputEvent::Change);
    }

    pub fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        if !self.value.is_empty() {
            self.selection = Some(0..self.value.len());
            self.selection_reversed = false;
            self.reset_cursor_blink();
            cx.notify();
        }
    }

    fn reset_cursor_blink(&mut self) {
        self.cursor_visible = true;
        // Any edit or cursor movement re-enables auto-follow so freshly typed
        // content scrolls back into view even if the user had scrolled away.
        self.follow_cursor = true;
    }

    // Offset translators
    fn offset_to_utf16(&self, byte_offset: usize) -> usize {
        let byte_offset = byte_offset.min(self.value.len());
        self.value[..byte_offset].chars().map(char::len_utf16).sum()
    }

    fn offset_from_utf16(&self, utf16_offset: usize) -> usize {
        let mut utf16_count = 0;
        let mut byte_offset = 0;
        for c in self.value.chars() {
            if utf16_count >= utf16_offset {
                break;
            }
            utf16_count += c.len_utf16();
            byte_offset += c.len_utf8();
        }
        byte_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    fn previous_grapheme_boundary(&self, byte_offset: usize) -> usize {
        let byte_offset = byte_offset.min(self.value.len());
        self.value
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < byte_offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_grapheme_boundary(&self, byte_offset: usize) -> usize {
        let byte_offset = byte_offset.min(self.value.len());
        self.value
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > byte_offset).then_some(idx))
            .unwrap_or(self.value.len())
    }

    // Undo / Redo mechanism
    fn push_undo_checkpoint(&mut self) {
        let entry = HistoryEntry {
            value: self.value.clone(),
            cursor_position: self.cursor_position,
            selection: self.selection.clone(),
        };
        if self.undo_stack.last().map(|e| &e.value) != Some(&self.value) {
            self.undo_stack.push(entry);
            if self.undo_stack.len() > 200 {
                self.undo_stack.remove(0);
            }
        }
        self.redo_stack.clear();
        self.last_undo_value = self.value.clone();
        self.last_undo_time = Some(Instant::now());
    }

    fn check_group_typing(&mut self) {
        let now = Instant::now();
        let should_checkpoint = if let Some(last_time) = self.last_undo_time {
            now.duration_since(last_time) > Duration::from_millis(1000)
        } else {
            true
        };

        if should_checkpoint {
            self.push_undo_checkpoint();
        } else {
            self.last_undo_time = Some(now);
        }
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        if let Some(entry) = self.undo_stack.pop() {
            let redo_entry = HistoryEntry {
                value: self.value.clone(),
                cursor_position: self.cursor_position,
                selection: self.selection.clone(),
            };
            self.redo_stack.push(redo_entry);

            self.value = entry.value;
            self.cursor_position = entry.cursor_position;
            self.selection = entry.selection;
            self.selection_reversed = false;
            self.marked_range = None;
            self.last_undo_value = self.value.clone();
            self.last_undo_time = None;

            self.emit_change(cx);
            cx.notify();
        }
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        if let Some(entry) = self.redo_stack.pop() {
            let undo_entry = HistoryEntry {
                value: self.value.clone(),
                cursor_position: self.cursor_position,
                selection: self.selection.clone(),
            };
            self.undo_stack.push(undo_entry);

            self.value = entry.value;
            self.cursor_position = entry.cursor_position;
            self.selection = entry.selection;
            self.selection_reversed = false;
            self.marked_range = None;
            self.last_undo_value = self.value.clone();
            self.last_undo_time = None;

            self.emit_change(cx);
            cx.notify();
        }
    }

    fn insert_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }

        let mut filtered_text = text.to_string();
        if self.digits_only {
            filtered_text = filtered_text.chars().filter(|c| c.is_ascii_digit()).collect();
            if filtered_text.is_empty() && !text.is_empty() {
                return;
            }
        }

        let selected_len = self.selection.as_ref().map(|r| r.len()).unwrap_or(0);
        if let Some(max) = self.max_length {
            let current_len = self.value.chars().count();
            let avail = max.saturating_sub(current_len.saturating_sub(selected_len));
            if avail == 0 {
                return;
            }
            filtered_text = filtered_text.chars().take(avail).collect();
        }

        if let Some(max_num) = self.max_number {
            let mut test_val = self.value.clone();
            if let Some(ref range) = self.selection {
                test_val.replace_range(range.clone(), &filtered_text);
            } else {
                let pos = clamp_to_char_boundary(&test_val, self.cursor_position.min(test_val.len()));
                test_val.insert_str(pos, &filtered_text);
            }
            if let Ok(num) = test_val.parse::<u64>() {
                if num > max_num {
                    self.set_value(max_num.to_string(), cx);
                    return;
                }
            }
        }

        if self.selection.is_some() {
            self.push_undo_checkpoint();
            let range = self.selection.take().unwrap();
            self.value.replace_range(range.clone(), "");
            self.cursor_position = range.start;
        } else {
            self.check_group_typing();
        }

        self.cursor_position = clamp_to_char_boundary(&self.value, self.cursor_position.min(self.value.len()));
        self.value.insert_str(self.cursor_position, &filtered_text);
        self.cursor_position += filtered_text.len();
        self.marked_range = None;
        self.reset_cursor_blink();
        self.emit_change(cx);
        cx.notify();
    }

    fn delete_backward(&mut self, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if self.selection.is_some() {
            self.push_undo_checkpoint();
            let range = self.selection.take().unwrap();
            self.value.replace_range(range.clone(), "");
            self.cursor_position = range.start;
            self.reset_cursor_blink();
            self.emit_change(cx);
            cx.notify();
            return;
        }

        if self.cursor_position > 0 {
            self.check_group_typing();
            let prev = self.previous_grapheme_boundary(self.cursor_position);
            self.value.replace_range(prev..self.cursor_position, "");
            self.cursor_position = prev;
            self.reset_cursor_blink();
            self.emit_change(cx);
            cx.notify();
        }
    }

    fn delete_forward(&mut self, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if self.selection.is_some() {
            self.push_undo_checkpoint();
            let range = self.selection.take().unwrap();
            self.value.replace_range(range.clone(), "");
            self.cursor_position = range.start;
            self.reset_cursor_blink();
            self.emit_change(cx);
            cx.notify();
            return;
        }

        if self.cursor_position < self.value.len() {
            self.check_group_typing();
            let next = self.next_grapheme_boundary(self.cursor_position);
            self.value.replace_range(self.cursor_position..next, "");
            self.reset_cursor_blink();
            self.emit_change(cx);
            cx.notify();
        }
    }

    fn delete_word_backward(&mut self, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if self.selection.is_some() {
            self.push_undo_checkpoint();
            let range = self.selection.take().unwrap();
            self.value.replace_range(range.clone(), "");
            self.cursor_position = range.start;
            self.reset_cursor_blink();
            self.emit_change(cx);
            cx.notify();
            return;
        }

        if self.cursor_position > 0 {
            self.push_undo_checkpoint();
            let pos = self.word_boundary_left();
            self.value.replace_range(pos..self.cursor_position, "");
            self.cursor_position = pos;
            self.reset_cursor_blink();
            self.emit_change(cx);
            cx.notify();
        }
    }

    fn delete_word_forward(&mut self, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if self.selection.is_some() {
            self.push_undo_checkpoint();
            let range = self.selection.take().unwrap();
            self.value.replace_range(range.clone(), "");
            self.cursor_position = range.start;
            self.reset_cursor_blink();
            self.emit_change(cx);
            cx.notify();
            return;
        }

        if self.cursor_position < self.value.len() {
            self.push_undo_checkpoint();
            let pos = self.word_boundary_right();
            self.value.replace_range(self.cursor_position..pos, "");
            self.reset_cursor_blink();
            self.emit_change(cx);
            cx.notify();
        }
    }

    fn delete_to_start(&mut self, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        self.push_undo_checkpoint();
        let pos = self.cursor_offset();
        if pos > 0 {
            self.value.replace_range(0..pos, "");
            self.move_to(0);
            self.reset_cursor_blink();
            self.emit_change(cx);
            cx.notify();
        }
    }

    fn delete_to_end(&mut self, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        self.push_undo_checkpoint();
        let pos = self.cursor_offset();
        if pos < self.value.len() {
            self.value.replace_range(pos..self.value.len(), "");
            self.move_to(pos);
            self.reset_cursor_blink();
            self.emit_change(cx);
            cx.notify();
        }
    }

    // Word scan boundaries
    fn word_boundary_left(&self) -> usize {
        if self.cursor_position == 0 {
            return 0;
        }
        let text = &self.value;
        let mut pos = self.cursor_position;
        
        while pos > 0 {
            let prev = self.previous_grapheme_boundary(pos);
            let prev_char = text[prev..pos].chars().next().unwrap_or(' ');
            if prev_char.is_whitespace() {
                pos = prev;
            } else {
                break;
            }
        }
        
        if pos > 0 {
            let prev = self.previous_grapheme_boundary(pos);
            let prev_char = text[prev..pos].chars().next().unwrap_or(' ');
            let on_word = is_word_char(prev_char);
            
            while pos > 0 {
                let prev = self.previous_grapheme_boundary(pos);
                let prev_char = text[prev..pos].chars().next().unwrap_or(' ');
                if is_word_char(prev_char) == on_word && !prev_char.is_whitespace() {
                    pos = prev;
                } else {
                    break;
                }
            }
        }
        pos
    }

    fn word_boundary_right(&self) -> usize {
        let text = &self.value;
        let mut pos = self.cursor_position;
        let len = text.len();
        
        while pos < len {
            let next = self.next_grapheme_boundary(pos);
            let next_char = text[pos..next].chars().next().unwrap_or(' ');
            if next_char.is_whitespace() {
                pos = next;
            } else {
                break;
            }
        }
        
        if pos < len {
            let next = self.next_grapheme_boundary(pos);
            let next_char = text[pos..next].chars().next().unwrap_or(' ');
            let on_word = is_word_char(next_char);
            
            while pos < len {
                let next = self.next_grapheme_boundary(pos);
                let next_char = text[pos..next].chars().next().unwrap_or(' ');
                if is_word_char(next_char) == on_word && !next_char.is_whitespace() {
                    pos = next;
                } else {
                    break;
                }
            }
        }
        pos
    }

    // Cursor movement helpers
    fn cursor_offset(&self) -> usize {
        if let Some(ref sel) = self.selection {
            if self.selection_reversed {
                sel.start
            } else {
                sel.end
            }
        } else {
            self.cursor_position
        }
    }

    fn move_to(&mut self, offset: usize) {
        self.cursor_position = clamp_to_char_boundary(&self.value, offset.min(self.value.len()));
        self.selection = None;
        self.selection_reversed = false;
    }

    fn select_to(&mut self, offset: usize) {
        let offset = clamp_to_char_boundary(&self.value, offset.min(self.value.len()));
        let anchor = if let Some(ref sel) = self.selection {
            if self.selection_reversed {
                sel.end
            } else {
                sel.start
            }
        } else {
            self.cursor_position
        };

        if offset < anchor {
            self.selection = Some(offset..anchor);
            self.selection_reversed = true;
        } else if offset > anchor {
            self.selection = Some(anchor..offset);
            self.selection_reversed = false;
        } else {
            self.selection = None;
            self.cursor_position = anchor;
            self.selection_reversed = false;
        }
    }

    fn move_cursor_left(&mut self, extend_selection: bool, cx: &mut Context<Self>) {
        let current = self.cursor_offset();
        if current > 0 {
            let prev = self.previous_grapheme_boundary(current);
            if extend_selection {
                self.select_to(prev);
            } else {
                if self.selection.is_some() {
                    let start = self.selection.as_ref().unwrap().start;
                    self.move_to(start);
                } else {
                    self.move_to(prev);
                }
            }
            self.reset_cursor_blink();
            cx.notify();
        } else if !extend_selection && self.selection.is_some() {
            let start = self.selection.as_ref().unwrap().start;
            self.move_to(start);
            cx.notify();
        }
    }

    fn move_cursor_right(&mut self, extend_selection: bool, cx: &mut Context<Self>) {
        let current = self.cursor_offset();
        if current < self.value.len() {
            let next = self.next_grapheme_boundary(current);
            if extend_selection {
                self.select_to(next);
            } else {
                if self.selection.is_some() {
                    let end = self.selection.as_ref().unwrap().end;
                    self.move_to(end);
                } else {
                    self.move_to(next);
                }
            }
            self.reset_cursor_blink();
            cx.notify();
        } else if !extend_selection && self.selection.is_some() {
            let end = self.selection.as_ref().unwrap().end;
            self.move_to(end);
            cx.notify();
        }
    }

    fn move_word_left(&mut self, extend_selection: bool, cx: &mut Context<Self>) {
        let target = self.word_boundary_left();
        if extend_selection {
            self.select_to(target);
        } else {
            self.move_to(target);
        }
        self.reset_cursor_blink();
        cx.notify();
    }

    fn move_word_right(&mut self, extend_selection: bool, cx: &mut Context<Self>) {
        let target = self.word_boundary_right();
        if extend_selection {
            self.select_to(target);
        } else {
            self.move_to(target);
        }
        self.reset_cursor_blink();
        cx.notify();
    }

    fn move_to_start(&mut self, extend_selection: bool, cx: &mut Context<Self>) {
        if extend_selection {
            self.select_to(0);
        } else {
            self.move_to(0);
        }
        self.reset_cursor_blink();
        cx.notify();
    }

    fn move_to_end(&mut self, extend_selection: bool, cx: &mut Context<Self>) {
        let len = self.value.len();
        if extend_selection {
            self.select_to(len);
        } else {
            self.move_to(len);
        }
        self.reset_cursor_blink();
        cx.notify();
    }

    fn line_col_for_byte_offset(&self, offset: usize) -> (usize, usize) {
        Self::line_col_for_byte_offset_in(&self.value, offset)
    }

    fn line_col_for_byte_offset_in(text: &str, offset: usize) -> (usize, usize) {
        let offset = offset.min(text.len());
        let before = &text[..offset];
        let line_idx = before.matches('\n').count();
        let start_of_line = before.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
        let byte_col = offset - start_of_line;
        (line_idx, byte_col)
    }

    fn byte_offset_for_line_col(&self, line_idx: usize, byte_col: usize) -> usize {
        let lines: Vec<&str> = self.value.split('\n').collect();
        if line_idx >= lines.len() {
            return self.value.len();
        }
        
        let mut offset = 0;
        for i in 0..line_idx {
            offset += lines[i].len() + 1; // +1 for '\n'
        }
        
        let line = lines[line_idx];
        let col = clamp_to_char_boundary(line, byte_col.min(line.len()));
        offset + col
    }

    fn move_cursor_up(&mut self, extend_selection: bool, cx: &mut Context<Self>) {
        let current_offset = self.cursor_offset();
        let (line, col) = self.line_col_for_byte_offset(current_offset);
        if line > 0 {
            let target_offset = self.byte_offset_for_line_col(line - 1, col);
            if extend_selection {
                self.select_to(target_offset);
            } else {
                self.move_to(target_offset);
            }
            self.reset_cursor_blink();
            cx.notify();
        }
    }

    fn move_cursor_down(&mut self, extend_selection: bool, cx: &mut Context<Self>) {
        let current_offset = self.cursor_offset();
        let (line, col) = self.line_col_for_byte_offset(current_offset);
        let total_lines = self.value.split('\n').count();
        if line + 1 < total_lines {
            let target_offset = self.byte_offset_for_line_col(line + 1, col);
            if extend_selection {
                self.select_to(target_offset);
            } else {
                self.move_to(target_offset);
            }
            self.reset_cursor_blink();
            cx.notify();
        }
    }

    fn normalized_insert_text(&self, text: &str) -> String {
        if self.multiline {
            text.to_string()
        } else {
            text.replace('\n', " ")
        }
    }

    fn copy_to_clipboard(&self, cx: &mut Context<Self>) {
        if self.password {
            return;
        }
        if let Some(ref sel) = self.selection {
            let text = &self.value[sel.clone()];
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
        }
    }

    fn paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if let Some(clipboard_item) = cx.read_from_clipboard() {
            if let Some(text) = clipboard_item.text() {
                let inserted = self.normalized_insert_text(&text);
                self.insert_text(&inserted, cx);
            }
        }
    }

    fn cut_to_clipboard(&mut self, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if self.password {
            return;
        }
        if let Some(ref sel) = self.selection {
            let text = &self.value[sel.clone()];
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
            self.insert_text("", cx);
        }
    }

    // Hit-testing / Mouse position
    pub fn char_position_for_mouse(&self, position: Point<Pixels>) -> usize {
        let Some(bounds) = self.last_bounds() else {
            return self.value.len();
        };

        let translate_offset = |display_offset: usize| -> usize {
            if self.password {
                let char_index = display_offset / 3;
                self.byte_position_for_char(char_index)
            } else {
                display_offset
            }
        };
        
        if self.multiline {
            let cache = self.layout_cache.borrow();
            let line_height = cache.line_height.max(px(1.0));
            let click_y = position.y - bounds.top() + cache.scroll_offset_y;
            let mut clicked_row = ((click_y / line_height).floor().max(0.0) as usize)
                .min(cache.row_ranges.len().saturating_sub(1));

            if !cache.row_ranges.is_empty() {
                // If the click is below all rows (in trailing empty space),
                // place the cursor at the very end of the text.
                if click_y >= line_height * (cache.row_ranges.len() as f32) {
                    return translate_offset(self.value.len());
                }
                clicked_row = clicked_row.min(cache.layouts.len().saturating_sub(1));
                if clicked_row < cache.layouts.len() {
                    let line = &cache.layouts[clicked_row];
                    let r = &cache.row_ranges[clicked_row];
                    let relative_x = position.x - bounds.left();
                    let local = line.closest_index_for_x(relative_x).min(line.text.len());
                    let display_offset = r.start + local;
                    return translate_offset(display_offset.min(self.value.len()));
                }
            }
            self.value.len()
        } else {
            let cache = self.layout_cache.borrow();
            if let Some(line) = cache.layouts.first() {
                let text_x_offset = if self.text_align == TextAlign::Center {
                    ((bounds.size.width - line.width()) * 0.5).max(px(0.0))
                } else {
                    px(0.0)
                };
                let relative_x = (position.x - bounds.left() - text_x_offset + cache.scroll_offset).max(px(0.0));
                let display_offset = line.closest_index_for_x(relative_x).min(line.len());
                translate_offset(display_offset)
            } else {
                self.value.len()
            }
        }
    }

    fn select_word_at(&mut self, pos: usize, cx: &mut Context<Self>) {
        let (start, end) = find_word_boundaries(&self.value, pos);
        if start != end {
            self.selection = Some(start..end);
            self.selection_reversed = false;
        } else {
            self.selection = None;
            self.cursor_position = pos;
        }
        self.reset_cursor_blink();
        cx.notify();
    }

    fn select_line_at(&mut self, pos: usize, cx: &mut Context<Self>) {
        if self.multiline {
            let (line_idx, _) = self.line_col_for_byte_offset(pos);
            let lines: Vec<&str> = self.value.split('\n').collect();
            if line_idx < lines.len() {
                let mut start = 0;
                for i in 0..line_idx {
                    start += lines[i].len() + 1;
                }
                let end = start + lines[line_idx].len();
                self.selection = Some(start..end);
                self.selection_reversed = false;
            }
        } else {
            self.select_all(cx);
        }
        self.reset_cursor_blink();
        cx.notify();
    }

    fn byte_position_for_char(&self, char_pos: usize) -> usize {
        self.value
            .char_indices()
            .nth(char_pos)
            .map(|(i, _)| i)
            .unwrap_or(self.value.len())
    }

    fn start_drag_scroll_task(&mut self, cx: &mut Context<Self>) {
        self._drag_scroll_task = Some(cx.spawn(async move |this: WeakEntity<SimpleInputState>, cx| {
            loop {
                smol::Timer::after(Duration::from_millis(50)).await;
                let done = this.update(&mut *cx, |state, cx| {
                    if !state.is_selecting {
                        return true; // Stop task
                    }
                    
                    let bounds_opt = state.layout_cache.borrow().bounds;
                    if let (Some(pos), Some(bounds)) = (state.last_mouse_position, bounds_opt) {
                        let mut scrolled = false;
                        let scroll_speed = px(12.0);
                        
                        if pos.x > bounds.right() {
                            state.layout_cache.borrow_mut().scroll_offset += scroll_speed;
                            scrolled = true;
                        } else if pos.x < bounds.left() {
                            let mut cache = state.layout_cache.borrow_mut();
                            cache.scroll_offset = (cache.scroll_offset - scroll_speed).max(px(0.0));
                            scrolled = true;
                        }
                        
                        if scrolled {
                            let char_pos = state.char_position_for_mouse(pos);
                            state.select_to(char_pos);
                            state.reset_cursor_blink();
                            cx.notify();
                        }
                    }
                    false
                }).unwrap_or(true);
                
                if done {
                    break;
                }
            }
        }));
    }

    // ── Vertical scrollbar (multiline) ────────────────────────────────────
    /// Jump-and-drag scroll (matches the project's other scrollbars): set the
    /// vertical scroll offset so the thumb aligns with `pointer_y` within the
    /// scrollbar track `bounds`. Used for both clicking the track and dragging
    /// the thumb. `follow_cursor` is turned off so the viewport stays where the
    /// user puts it (they can freely review earlier content); any edit / cursor
    /// move re-enables follow via `reset_cursor_blink`.
    fn scroll_to_y_from_pointer(
        &mut self,
        bounds: &Bounds<Pixels>,
        pointer_y: f32,
        cx: &mut Context<Self>,
    ) {
        let (line_height, rows) = {
            let cache = self.layout_cache.borrow();
            (f32::from(cache.line_height.max(px(1.0))), cache.row_ranges.len().max(1))
        };
        let content = line_height * rows as f32;
        let viewport = f32::from(bounds.size.height);
        let max_scroll = (content - viewport).max(1.0);
        // Nothing to scroll: ignore.
        if content <= viewport {
            return;
        }
        let origin_y = f32::from(bounds.origin.y);
        let ratio = ((pointer_y - origin_y) / viewport).clamp(0.0, 1.0);
        self.layout_cache.borrow_mut().scroll_offset_y = px(ratio * max_scroll);
        self.is_scrollbar_dragging = true;
        self.follow_cursor = false;
        cx.notify();
    }

    /// End an active scrollbar drag.
    fn end_scrollbar_drag(&mut self, cx: &mut Context<Self>) {
        if self.is_scrollbar_dragging {
            self.is_scrollbar_dragging = false;
            cx.notify();
        }
    }

    // Keystroke mapping
    fn handle_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> KeyHandled {
        if let Some(interceptor) = self.key_interceptor.clone() {
            match (interceptor)(event, &self.value, cx) {
                KeyInterceptResult::Handled => return KeyHandled::Handled,
                KeyInterceptResult::SetValue(new_val) => {
                    self.set_value(new_val, cx);
                    return KeyHandled::Handled;
                }
                KeyInterceptResult::Unhandled => {}
            }
        }

        let key = event.keystroke.key.as_str();
        let modifiers = &event.keystroke.modifiers;
        let shift = modifiers.shift;

        match key {
            "backspace" => {
                let is_word_mod = if cfg!(target_os = "macos") { modifiers.alt } else { modifiers.control };
                if is_word_mod {
                    self.delete_word_backward(cx);
                } else if modifiers.platform && cfg!(target_os = "macos") {
                    self.delete_to_start(cx);
                } else {
                    self.delete_backward(cx);
                }
                return KeyHandled::Handled;
            }
            "delete" => {
                let is_word_mod = if cfg!(target_os = "macos") { modifiers.alt } else { modifiers.control };
                if is_word_mod {
                    self.delete_word_forward(cx);
                } else if modifiers.platform && cfg!(target_os = "macos") {
                    self.delete_to_end(cx);
                } else {
                    self.delete_forward(cx);
                }
                return KeyHandled::Handled;
            }
            "left" => {
                let is_word_mod = if cfg!(target_os = "macos") { modifiers.alt } else { modifiers.control };
                if is_word_mod {
                    self.move_word_left(shift, cx);
                } else if modifiers.platform {
                    self.move_to_start(shift, cx);
                } else {
                    self.move_cursor_left(shift, cx);
                }
                return KeyHandled::Handled;
            }
            "right" => {
                let is_word_mod = if cfg!(target_os = "macos") { modifiers.alt } else { modifiers.control };
                if is_word_mod {
                    self.move_word_right(shift, cx);
                } else if modifiers.platform {
                    self.move_to_end(shift, cx);
                } else {
                    self.move_cursor_right(shift, cx);
                }
                return KeyHandled::Handled;
            }
            "up" if self.multiline => {
                self.move_cursor_up(shift, cx);
                return KeyHandled::Handled;
            }
            "down" if self.multiline => {
                self.move_cursor_down(shift, cx);
                return KeyHandled::Handled;
            }
            "home" => {
                self.move_to_start(shift, cx);
                return KeyHandled::Handled;
            }
            "end" => {
                self.move_to_end(shift, cx);
                return KeyHandled::Handled;
            }
            "z" if (modifiers.platform || modifiers.control) && !shift => {
                self.undo(cx);
                return KeyHandled::Handled;
            }
            "y" if (modifiers.platform || modifiers.control) => {
                self.redo(cx);
                return KeyHandled::Handled;
            }
            "z" if (modifiers.platform || modifiers.control) && shift => {
                self.redo(cx);
                return KeyHandled::Handled;
            }
            "a" | "A" if modifiers.platform || modifiers.control => {
                self.select_all(cx);
                return KeyHandled::Handled;
            }
            "c" | "C" if modifiers.platform || modifiers.control => {
                if self.selection.is_some() {
                    self.copy_to_clipboard(cx);
                    return KeyHandled::Handled;
                }
                return KeyHandled::NotHandled;
            }
            "v" | "V" if modifiers.platform || modifiers.control => {
                self.paste_from_clipboard(cx);
                return KeyHandled::Handled;
            }
            "x" | "X" if modifiers.platform || modifiers.control => {
                if self.selection.is_some() {
                    self.cut_to_clipboard(cx);
                    return KeyHandled::Handled;
                }
                return KeyHandled::NotHandled;
            }
            "escape" => {
                if self.selection.is_some() {
                    self.selection = None;
                    cx.notify();
                    return KeyHandled::Handled;
                }
                return KeyHandled::NotHandled;
            }
            "enter" => {
                if self.multiline {
                    if self.submit_on_enter && !modifiers.shift {
                        cx.emit(InputEvent::PressEnter);
                        return KeyHandled::Handled;
                    }
                    self.insert_text("\n", cx);
                    return KeyHandled::Handled;
                }
                cx.emit(InputEvent::PressEnter);
                return KeyHandled::Handled;
            }
            "tab" => {
                return KeyHandled::NotHandled;
            }
            "shift" | "control" | "alt" | "meta" | "capslock"
            | "f1" | "f2" | "f3" | "f4" | "f5" | "f6" | "f7" | "f8" | "f9" | "f10" | "f11"
            | "f12" | "pageup" | "pagedown" => {
                return KeyHandled::Ignored;
            }
            _ => {}
        }
        
        // Handle character input via key_char (it's a String, not a char)
        if let Some(ref s) = event.keystroke.key_char {
            if self.read_only {
                return KeyHandled::Ignored;
            }
            // Skip control characters (except for normal space/printable)
            if !s.is_empty() && !s.chars().next().is_none_or(|c| c.is_control() && c != ' ') {
                self.insert_text(s, cx);
                return KeyHandled::Handled;
            }
        }

        KeyHandled::Ignored
    }
}

// IME support implementation
impl EntityInputHandler for SimpleInputState {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        
        // For password fields, return bullets to prevent clipboard exposure
        if self.password {
            let char_count = self.value[range].chars().count();
            Some("•".repeat(char_count))
        } else {
            Some(self.value[range].to_string())
        }
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let sel = self.selection.clone().unwrap_or(self.cursor_position..self.cursor_position);
        Some(UTF16Selection {
            range: self.range_to_utf16(&sel),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        self.selection = None;
        self.selection_reversed = false;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }

        // The range passed by the platform (IME) is in UTF-16 units, matching
        // `selected_text_range`/`marked_text_range`. Convert it to UTF-8 byte
        // offsets before using it against `self.value`. Without this conversion,
        // CJK/emoji text (where UTF-16 length != UTF-8 byte length) lands the
        // range inside a multi-byte character and panics with
        // "start of range should be a character boundary".
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or_else(|| {
                self.selection.clone().unwrap_or(self.cursor_position..self.cursor_position)
            });

        self.push_undo_checkpoint();

        let mut filtered_text = new_text.to_string();
        if self.digits_only {
            filtered_text = filtered_text.chars().filter(|c| c.is_ascii_digit()).collect();
        }
        if let Some(max) = self.max_length {
            let current_len = self.value.chars().count();
            let replaced_chars = self.value[range.start.min(self.value.len())..range.end.min(self.value.len())].chars().count();
            let avail = max.saturating_sub(current_len.saturating_sub(replaced_chars));
            filtered_text = filtered_text.chars().take(avail).collect();
        }
        if let Some(max_num) = self.max_number {
            let mut test_val = self.value.clone();
            let start = clamp_to_char_boundary(&test_val, range.start.min(test_val.len()));
            let end = clamp_to_char_boundary(&test_val, range.end.min(test_val.len()));
            test_val.replace_range(start..end, &filtered_text);
            if let Ok(num) = test_val.parse::<u64>() {
                if num > max_num {
                    self.set_value(max_num.to_string(), cx);
                    return;
                }
            }
        }

        let start = clamp_to_char_boundary(&self.value, range.start.min(self.value.len()));
        let end = clamp_to_char_boundary(&self.value, range.end.min(self.value.len()));
        self.value.replace_range(start..end, &filtered_text);

        // `start` is a char boundary and `new_text` is a valid string, so
        // `start + new_text.len()` is also a char boundary (no drift on CJK/emoji).
        self.cursor_position = start + filtered_text.len();
        self.selection = None;
        self.selection_reversed = false;
        self.marked_range = None;

        self.reset_cursor_blink();
        self.emit_change(cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or(self.marked_range.clone())
            .unwrap_or_else(|| {
                self.selection.clone().unwrap_or(self.cursor_position..self.cursor_position)
            });

        if self.marked_range.is_none() {
            self.push_undo_checkpoint();
        }

        let start = clamp_to_char_boundary(&self.value, range.start.min(self.value.len()));
        let end = clamp_to_char_boundary(&self.value, range.end.min(self.value.len()));
        self.value.replace_range(start..end, new_text);

        let new_text_len = new_text.len();
        self.marked_range = if new_text_len == 0 {
            None
        } else {
            Some(start..start + new_text_len)
        };

        if let Some(selected_utf16) = new_selected_range_utf16 {
            let text_val = new_text.to_string();
            let end_rel = offset_from_utf16_in(&text_val, selected_utf16.end);
            let sel_end = clamp_to_char_boundary(&self.value, start + end_rel);

            self.selection = None;
            self.cursor_position = sel_end;
            self.selection_reversed = false;
        } else {
            self.selection = None;
            self.cursor_position = start + new_text_len;
            self.selection_reversed = false;
        }

        self.reset_cursor_blink();
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let start = range.start;

        let translate_offset = |offset: usize| -> usize {
            if self.password {
                let offset = offset.min(self.value.len());
                let char_count = self.value[..offset].chars().count();
                char_count * 3
            } else {
                offset
            }
        };

        let start = translate_offset(start);
        let cache = self.layout_cache.borrow();
        
        if self.multiline {
            let mut row = 0usize;
            for (i, r) in cache.row_ranges.iter().enumerate() {
                if start <= r.end {
                    row = i;
                    break;
                }
                row = i;
            }
            let layout = cache.layouts.get(row)?;
            let local = start.saturating_sub(cache.row_ranges[row].start);
            let relative_x = layout.x_for_index(local);
            let line_height = cache.line_height.max(window.line_height());
            let font_size = cache.font_size.max(px(1.0));
            let y_pos = bounds.top() + line_height * (row as f32) + (line_height - font_size) * 0.5 - cache.scroll_offset_y;

            Some(Bounds::new(
                point(bounds.left() + relative_x, y_pos),
                size(px(1.0), font_size),
            ))
        } else {
            let layout = cache.layouts.first()?;
            let relative_x = layout.x_for_index(start);
            let line_height = window.line_height();
            let font_size = cache.font_size.max(px(1.0));
            let y_pos = bounds.top() + (line_height - font_size) * 0.5;

            Some(Bounds::new(
                point(bounds.left() + relative_x - cache.scroll_offset, y_pos),
                size(px(1.0), font_size),
            ))
        }
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let char_pos = self.char_position_for_mouse(point);
        Some(self.offset_to_utf16(char_pos))
    }
}

/// Clamp a byte `idx` down to the nearest valid UTF-8 character boundary at or
/// before it. The result is always safe to pass to `String::replace_range`,
/// `insert_str`, `remove`, etc. Any code that computes a byte offset that may
/// fall inside a multi-byte character (CJK, emoji, combining marks, IME) must
/// route it through this helper instead of using `idx.min(s.len())`.
fn clamp_to_char_boundary(s: &str, mut idx: usize) -> usize {
    let len = s.len();
    if idx >= len {
        return len;
    }
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn offset_from_utf16_in(text: &str, utf16_offset: usize) -> usize {
    let mut utf16_count = 0;
    let mut byte_offset = 0;
    for c in text.chars() {
        if utf16_count >= utf16_offset {
            break;
        }
        utf16_count += c.len_utf16();
        byte_offset += c.len_utf8();
    }
    byte_offset
}

fn split_run_by_highlights(
    run: TextRun,
    run_start: usize,
    search_highlights: &[Range<usize>],
    search_current_match: &Option<Range<usize>>,
    palette: &SemanticPalette,
) -> Vec<TextRun> {
    let run_end = run_start + run.len;
    let mut sub_runs = Vec::new();
    
    let mut boundaries = vec![run_start, run_end];
    
    for range in search_highlights {
        let start = range.start.max(run_start).min(run_end);
        let end = range.end.max(run_start).min(run_end);
        if start < end {
            boundaries.push(start);
            boundaries.push(end);
        }
    }
    
    if let Some(range) = search_current_match {
        let start = range.start.max(run_start).min(run_end);
        let end = range.end.max(run_start).min(run_end);
        if start < end {
            boundaries.push(start);
            boundaries.push(end);
        }
    }
    
    boundaries.sort();
    boundaries.dedup();
    
    for i in 0..boundaries.len() - 1 {
        let start = boundaries[i];
        let end = boundaries[i + 1];
        if start >= end { continue; }
        
        let mid = (start + end) / 2;
        
        let mut bg_color = None;
        if let Some(range) = search_current_match {
            if mid >= range.start && mid < range.end {
                bg_color = Some(palette.editor_search_current);
            }
        }
        if bg_color.is_none() {
            for range in search_highlights {
                if mid >= range.start && mid < range.end {
                    bg_color = Some(palette.editor_search_match);
                    break;
                }
            }
        }
        
        sub_runs.push(TextRun {
            len: end - start,
            font: run.font.clone(),
            color: run.color,
            background_color: bg_color.or(run.background_color),
            underline: run.underline.clone(),
            strikethrough: run.strikethrough.clone(),
        });
    }
    
    sub_runs
}

// Helpers for styled text runs
fn build_runs(
    line_text: &str,
    line_start_byte: usize,
    marked_range: &Option<Range<usize>>,
    highlight_vars: bool,
    syntax_language: Option<&str>,
    is_dark: bool,
    search_highlights: &[Range<usize>],
    search_current_match: &Option<Range<usize>>,
    text_style: &gpui::TextStyle,
    palette: &SemanticPalette,
) -> Vec<TextRun> {
    let default_color: Hsla = palette.text_primary;
    let var_color: Hsla = palette.editor_variable;
    
    let mut runs = Vec::new();
    let line_len = line_text.len();
    if line_len == 0 {
        return runs;
    }
    
    let mut segments: Vec<(Range<usize>, Hsla)> = Vec::new();
    if let Some(lang) = syntax_language {
        let spans = crate::syntax::highlight_text(line_text, lang, is_dark);
        if spans.is_empty() {
            segments.push((0..line_len, default_color));
        } else {
            for span in spans {
                let col: Hsla = span.color.into();
                segments.push((span.range, col));
            }
        }
    } else {
        segments.push((0..line_len, default_color));
    }

    if highlight_vars {
        let mut var_ranges = Vec::new();
        let bytes = line_text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'{' {
                if let Some(close) = line_text[i..].find('}') {
                    let end = i + close + 1;
                    var_ranges.push(i..end);
                    i = end;
                    continue;
                }
            }
            i += 1;
        }

        if !var_ranges.is_empty() {
            let mut new_segments = Vec::new();
            for (seg_range, seg_color) in segments {
                let mut cur = seg_range.start;
                for vr in &var_ranges {
                    if vr.end <= cur || vr.start >= seg_range.end {
                        continue;
                    }
                    if vr.start > cur {
                        new_segments.push((cur..vr.start.min(seg_range.end), seg_color));
                    }
                    let overlap_start = cur.max(vr.start);
                    let overlap_end = seg_range.end.min(vr.end);
                    if overlap_start < overlap_end {
                        new_segments.push((overlap_start..overlap_end, var_color));
                        cur = overlap_end;
                    }
                }
                if cur < seg_range.end {
                    new_segments.push((cur..seg_range.end, seg_color));
                }
            }
            segments = new_segments;
        }
    }
    
    for (range, run_color) in segments {
        let global_start = line_start_byte + range.start;
        let global_end = line_start_byte + range.end;
        
        if let Some(marked) = marked_range {
            let intersect_start = global_start.max(marked.start);
            let intersect_end = global_end.min(marked.end);
            
            if intersect_start < intersect_end {
                if global_start < intersect_start {
                    runs.push(TextRun {
                        len: intersect_start - global_start,
                        font: text_style.font(),
                        color: run_color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    });
                }
                runs.push(TextRun {
                    len: intersect_end - intersect_start,
                    font: text_style.font(),
                    color: run_color,
                    background_color: Some(palette.editor_ime_marked),
                    underline: Some(UnderlineStyle {
                        color: Some(Hsla::default()),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    strikethrough: None,
                });
                if intersect_end < global_end {
                    runs.push(TextRun {
                        len: global_end - intersect_end,
                        font: text_style.font(),
                        color: run_color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    });
                }
            } else {
                runs.push(TextRun {
                    len: range.len(),
                    font: text_style.font(),
                    color: run_color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                });
            }
        } else {
            runs.push(TextRun {
                len: range.len(),
                font: text_style.font(),
                color: run_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }
    }

    let mut final_runs = Vec::new();
    let mut current_offset = line_start_byte;
    for run in runs {
        let run_len = run.len;
        final_runs.extend(split_run_by_highlights(
            run,
            current_offset,
            search_highlights,
            search_current_match,
            palette,
        ));
        current_offset += run_len;
    }
    final_runs
}

// Custom text viewport drawing element
struct TextInputElement {
    state: Entity<SimpleInputState>,
}

impl IntoElement for TextInputElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

struct PrepaintState {
    lines: Vec<ShapedLine>,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
}

impl Element for TextInputElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let state = self.state.read(cx);
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.flex_grow = 1.0;
        style.flex_shrink = 1.0;
        
        let line_height = window.line_height();
        if !state.multiline {
            style.size.height = line_height.into();
        } else {
            // Fill the parent's definite height so the element never collapses
            // to zero (the parent already reserves `visible_rows * line_height`).
            style.size.height = relative(1.).into();
        }
        
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let (value, placeholder, is_focused, cursor_offset, marked_range, selection, highlight_vars, syntax_language, multiline, auto_height, read_only, wrap, follow_cursor, scroll_offset, scroll_offset_y, password, search_highlights, search_current_match) = {
            let input = self.state.read(cx);
            let is_focused = input.focus_handle.is_focused(window);
            let cache = input.layout_cache.borrow();
            (
                input.value.clone(),
                input.placeholder.clone(),
                is_focused,
                input.cursor_offset(),
                input.marked_range.clone(),
                input.selection.clone(),
                input.highlight_vars,
                input.syntax_language.clone(),
                input.multiline,
                input.auto_height,
                input.read_only,
                input.wrap,
                input.follow_cursor,
                cache.scroll_offset,
                cache.scroll_offset_y,
                input.password,
                input.search_highlights.clone(),
                input.search_current_match.clone(),
            )
        };
        let text_style = window.text_style();
        let line_height = window.line_height();
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let is_dark = t.is_dark();
        let syntax_lang = if password { None } else { syntax_language.as_deref() };
        
        let mut shaped_lines = Vec::new();
        let mut cursor_quad = None;
        let mut selection_quads = Vec::new();
        
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let show_placeholder = value.is_empty();

        let display_value = if password && !value.is_empty() {
            "•".repeat(value.chars().count())
        } else {
            value.clone()
        };

        let translate_offset = |offset: usize| -> usize {
            if password {
                let offset = offset.min(value.len());
                let char_count = value[..offset].chars().count();
                // 掩码字符 `•` 为 3 字节 UTF-8 字符 (U+2022)
                char_count * "•".len()
            } else {
                offset
            }
        };

        let effective_font_size = if password && !value.is_empty() {
            font_size * 1.25
        } else {
            font_size
        };

        let cursor_offset = translate_offset(cursor_offset);
        let marked_range = marked_range.map(|r| translate_offset(r.start)..translate_offset(r.end));
        let selection = selection.map(|r| translate_offset(r.start)..translate_offset(r.end));
        let mut row_ranges: Vec<Range<usize>> = Vec::new();
        
        if show_placeholder {
            let run = TextRun {
                len: placeholder.len(),
                font: text_style.font(),
                color: p.text_muted,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let line = window.text_system().shape_line(placeholder.into(), font_size, &[run], None);
            shaped_lines.push(line);
            row_ranges.push(0..0);
        } else {
            let wrap_width = if multiline && wrap {
                // Reserve right-side padding so wrapped lines fold *before* they
                // reach the right border (instead of only at it), clearing the
                // 12px scrollbar and leaving a comfortable margin. Adaptive to the
                // panel/dock width because `bounds.size.width` is re-read on every
                // paint, so a resize that squeezes the input re-wraps immediately.
                Some((bounds.size.width - px(8.0)).max(px(60.0)))
            } else {
                None
            };
            let lines_str: Vec<&str> = display_value.split('\n').collect();
            let mut line_start_byte = 0;

            for line_str in lines_str.iter() {
                let display_str: &str = if line_str.is_empty() {
                    "\u{200B}"
                } else {
                    *line_str
                };

                let runs = build_runs(
                    display_str,
                    line_start_byte,
                    &marked_range,
                    highlight_vars,
                    syntax_lang,
                    is_dark,
                    &search_highlights,
                    &search_current_match,
                    &text_style,
                    &p,
                );
                let line = window.text_system().shape_line(display_str.into(), effective_font_size, &runs, None);

                if let Some(ww) = wrap_width {
                    // Wrap the logical line into visual rows that fit within ww.
                    let n = line_str.len();
                    if n == 0 {
                        shaped_lines.push(line);
                        row_ranges.push(line_start_byte..line_start_byte);
                    } else {
                        let mut xs: Vec<f32> = Vec::with_capacity(n + 1);
                        for i in 0..=n {
                            xs.push(f32::from(line.x_for_index(i)));
                        }
                        let width_of = |s: usize, e: usize| xs[e] - xs[s];
                        let ww_f = f32::from(ww);

                        let mut s = 0usize;
                        let mut i = 0usize;
                        while s < n {
                            let mut last_space: Option<usize> = None;
                            let mut overflow = false;
                            while i < n {
                                let ch = line_str.as_bytes()[i];
                                if width_of(s, i + 1) > ww_f {
                                    overflow = true;
                                    break;
                                }
                                if ch == b' ' {
                                    last_space = Some(i + 1);
                                }
                                i += 1;
                            }

                            let mut break_at = if overflow { i } else { n };
                            if overflow {
                                if let Some(ls) = last_space {
                                    if ls > s {
                                        break_at = ls;
                                    }
                                }
                            }
                            let break_at = clamp_to_char_boundary(line_str, break_at);
                            let break_at = if break_at <= s {
                                let next_char = line_str[s..].chars().next().map(|c| s + c.len_utf8()).unwrap_or(s + 1);
                                clamp_to_char_boundary(line_str, next_char)
                            } else {
                                break_at
                            };

                            let sub = &line_str[s..break_at];
                            let sub_start = line_start_byte + s;
                            let sub_runs = build_runs(
                                sub,
                                sub_start,
                                &marked_range,
                                highlight_vars,
                                syntax_lang,
                                is_dark,
                                &search_highlights,
                                &search_current_match,
                                &text_style,
                                &p,
                            );
                            let sub_line = window.text_system().shape_line(sub.into(), effective_font_size, &sub_runs, None);
                            shaped_lines.push(sub_line);
                            row_ranges.push(sub_start..sub_start + sub.len());
                            s = break_at;
                            i = s;
                        }
                    }
                } else {
                    shaped_lines.push(line);
                    row_ranges.push(line_start_byte..line_start_byte + line_str.len());
                }
                line_start_byte += line_str.len() + 1;
            }
        }
        
        // Calculate scroll offsets
        let mut final_scroll_offset = scroll_offset;
        let mut vscroll = scroll_offset_y;
        if !multiline && !shaped_lines.is_empty() {
            let line = &shaped_lines[0];
            let cursor_x = line.x_for_index(cursor_offset);
            // Small left padding so the cursor/start of text isn't glued to the left edge.
            // Right side uses 0 padding so text can fill all the way to the right edge.
            let left_padding = px(4.0).min(bounds.size.width * 0.1).max(px(2.0));
            let right_padding = px(0.0);

            if cursor_x - final_scroll_offset < left_padding {
                final_scroll_offset = (cursor_x - left_padding).max(px(0.0));
            } else if cursor_x - final_scroll_offset > bounds.size.width - right_padding {
                final_scroll_offset = cursor_x - bounds.size.width + right_padding;
            }

            let max_scroll = (line.width() - bounds.size.width + right_padding).max(px(0.0));
            final_scroll_offset = final_scroll_offset.clamp(px(0.0), max_scroll);
        } else if multiline {
            let visible_h = bounds.size.height;
            let total_h = line_height * (shaped_lines.len() as f32);
            if auto_height || read_only || total_h <= visible_h + px(1.5) {
                vscroll = px(0.0);
            } else if follow_cursor {
                let mut cursor_row = 0usize;
                for (i, r) in row_ranges.iter().enumerate() {
                    if cursor_offset <= r.end {
                        cursor_row = i;
                        break;
                    }
                    cursor_row = i;
                }
                let cursor_y = line_height * (cursor_row as f32);
                if cursor_y - vscroll < px(0.0) {
                    vscroll = cursor_y;
                } else if cursor_y + line_height - vscroll > visible_h {
                    vscroll = (cursor_y + line_height - visible_h).min(cursor_y);
                }
            }
            let max_scroll_y = if auto_height || read_only {
                px(0.0)
            } else {
                (total_h - visible_h).max(px(0.0))
            };
            vscroll = vscroll.clamp(px(0.0), max_scroll_y);
        }
        
        // Calculate cursor visual bounds
        if is_focused && !self.state.read(cx).read_only {
            let cursor_visible = self.state.read(cx).cursor_visible;

            if cursor_visible {
                if multiline {
                    let mut cursor_row = 0usize;
                    for (i, r) in row_ranges.iter().enumerate() {
                        if cursor_offset <= r.end {
                            cursor_row = i;
                            break;
                        }
                        cursor_row = i;
                    }
                    if cursor_row < shaped_lines.len() {
                        let line = &shaped_lines[cursor_row];
                        let row = &row_ranges[cursor_row];
                        let local = cursor_offset.saturating_sub(row.start);
                        let cursor_x = line.x_for_index(local);
                        let y_pos = bounds.top() + line_height * (cursor_row as f32) - vscroll;
                        let cursor_h = line_height;

                        cursor_quad = Some(fill(
                            Bounds::new(
                                point(bounds.left() + cursor_x, y_pos),
                                size(px(1.5), cursor_h),
                            ),
                            p.editor_cursor,
                        ));
                    }
                } else {
                    let line = &shaped_lines[0];
                    let text_x_offset = if self.state.read(cx).text_align == TextAlign::Center {
                        ((bounds.size.width - line.width()) * 0.5).max(px(0.0))
                    } else {
                        px(0.0)
                    };
                    let cursor_x = text_x_offset + line.x_for_index(cursor_offset);
                    let box_v_offset = ((bounds.size.height - line_height) * 0.5).max(px(0.0));
                    let cursor_h = line_height;
                    let y_offset = box_v_offset;
                    
                    cursor_quad = Some(fill(
                        Bounds::new(
                            point(bounds.left() + cursor_x - final_scroll_offset, bounds.top() + y_offset),
                            size(px(1.5), cursor_h),
                        ),
                        p.editor_cursor,
                    ));
                }
            }
        }
        
        // Calculate search highlights visual bounds
        if multiline {
            for (row_idx, line) in shaped_lines.iter().enumerate() {
                if row_idx >= row_ranges.len() { continue; }
                let row = &row_ranges[row_idx];
                let y_pos = bounds.top() + line_height * (row_idx as f32) - vscroll;
                
                // Normal search highlights
                for range in &search_highlights {
                    let intersect_start = range.start.max(row.start);
                    let intersect_end = range.end.min(row.end);
                    if intersect_start < intersect_end {
                        let start_col = intersect_start - row.start;
                        let end_col = intersect_end - row.start;
                        let start_x = line.x_for_index(start_col);
                        let end_x = line.x_for_index(end_col);
                        selection_quads.push(fill(
                            Bounds::from_corners(
                                point(bounds.left() + start_x, y_pos),
                                point(bounds.left() + end_x, y_pos + line_height),
                            ),
                            p.editor_search_match,
                        ));
                    }
                }
                
                // Current search match
                if let Some(ref range) = search_current_match {
                    let intersect_start = range.start.max(row.start);
                    let intersect_end = range.end.min(row.end);
                    if intersect_start < intersect_end {
                        let start_col = intersect_start - row.start;
                        let end_col = intersect_end - row.start;
                        let start_x = line.x_for_index(start_col);
                        let end_x = line.x_for_index(end_col);
                        selection_quads.push(fill(
                            Bounds::from_corners(
                                point(bounds.left() + start_x, y_pos),
                                point(bounds.left() + end_x, y_pos + line_height),
                            ),
                            p.editor_search_current,
                        ));
                    }
                }
            }
        } else if !shaped_lines.is_empty() {
            let line = &shaped_lines[0];
            for range in &search_highlights {
                let intersect_start = range.start;
                let intersect_end = range.end;
                if intersect_start < intersect_end {
                    let start_x = line.x_for_index(intersect_start);
                    let end_x = line.x_for_index(intersect_end);
                    selection_quads.push(fill(
                        Bounds::from_corners(
                            point(bounds.left() + start_x - final_scroll_offset, bounds.top()),
                            point(bounds.left() + end_x - final_scroll_offset, bounds.bottom()),
                        ),
                        p.editor_search_match,
                    ));
                }
            }
            if let Some(ref range) = search_current_match {
                let intersect_start = range.start;
                let intersect_end = range.end;
                if intersect_start < intersect_end {
                    let start_x = line.x_for_index(intersect_start);
                    let end_x = line.x_for_index(intersect_end);
                    selection_quads.push(fill(
                        Bounds::from_corners(
                            point(bounds.left() + start_x - final_scroll_offset, bounds.top()),
                            point(bounds.left() + end_x - final_scroll_offset, bounds.bottom()),
                        ),
                        p.editor_search_current,
                    ));
                }
            }
        }

        // Calculate selection visual bounds
        if let Some(ref sel) = selection {
            if multiline {
                for (row_idx, line) in shaped_lines.iter().enumerate() {
                    if row_idx >= row_ranges.len() { continue; }
                    let row = &row_ranges[row_idx];
                    let current_line_start = row.start;
                    let current_line_end = row.end;

                    let intersect_start = sel.start.max(current_line_start);
                    let intersect_end = sel.end.min(current_line_end);

                    if intersect_start < intersect_end {
                        let start_col = intersect_start - current_line_start;
                        let end_col = intersect_end - current_line_start;
                        let start_x = line.x_for_index(start_col);
                        let end_x = line.x_for_index(end_col);
                        let y_pos = bounds.top() + line_height * (row_idx as f32) - vscroll;

                        selection_quads.push(fill(
                            Bounds::from_corners(
                                point(bounds.left() + start_x, y_pos),
                                point(bounds.left() + end_x, y_pos + line_height),
                            ),
                            p.editor_selection,
                        ));
                    }
                }
            } else if !shaped_lines.is_empty() {
                let line = &shaped_lines[0];
                let text_x_offset = if self.state.read(cx).text_align == TextAlign::Center {
                    ((bounds.size.width - line.width()) * 0.5).max(px(0.0))
                } else {
                    px(0.0)
                };
                let start_x = text_x_offset + line.x_for_index(sel.start);
                let end_x = text_x_offset + line.x_for_index(sel.end);
                let box_v_offset = ((bounds.size.height - line_height) * 0.5).max(px(0.0));
                
                selection_quads.push(fill(
                    Bounds::from_corners(
                        point(bounds.left() + start_x - final_scroll_offset, bounds.top() + box_v_offset),
                        point(bounds.left() + end_x - final_scroll_offset, bounds.top() + box_v_offset + line_height),
                    ),
                    p.editor_selection,
                ));
            }
        }
        
        {
            let input = self.state.read(cx);
            let mut cache = input.layout_cache.borrow_mut();
            cache.row_ranges = row_ranges;
            cache.layouts = shaped_lines.clone();
            cache.scroll_offset = final_scroll_offset;
            cache.scroll_offset_y = vscroll;
            cache.line_height = line_height;
            cache.font_size = font_size;
            cache.bounds = Some(bounds);
        }
        
        PrepaintState {
            lines: shaped_lines,
            cursor: cursor_quad,
            selections: selection_quads,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (focus_handle, multiline, text_align) = {
            let state = self.state.read(cx);
            (state.focus_handle.clone(), state.multiline, state.text_align)
        };
        let (scroll_offset, vscroll, line_height) = {
            let cache = self.state.read(cx).layout_cache.borrow();
            let lh = if multiline {
                cache.line_height.max(px(1.0))
            } else {
                window.line_height()
            };
            (cache.scroll_offset, cache.scroll_offset_y, lh)
        };

        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.state.clone()),
            cx,
        );

        if multiline {
            let clip_bounds = Bounds {
                origin: bounds.origin,
                size: size(bounds.size.width, bounds.size.height + px(3.0)),
            };
            window.paint_layer(clip_bounds, |window| {
                for selection_quad in prepaint.selections.drain(..) {
                    window.paint_quad(selection_quad);
                }
                for (line_idx, line) in prepaint.lines.drain(..).enumerate() {
                    let y_pos = bounds.top() + line_height * (line_idx as f32) - vscroll;
                    line.paint(
                        point(bounds.left(), y_pos),
                        line_height,
                        gpui::TextAlign::Left,
                        None,
                        window,
                        cx,
                    ).ok();
                }
                if let Some(cursor) = prepaint.cursor.take() {
                    window.paint_quad(cursor);
                }
            });
        } else {
            for selection_quad in prepaint.selections.drain(..) {
                window.paint_quad(selection_quad);
            }
            if let Some(line) = prepaint.lines.pop() {
                let text_x_offset = if text_align == TextAlign::Center {
                    ((bounds.size.width - line.width()) * 0.5).max(px(0.0))
                } else {
                    px(0.0)
                };
                let box_v_offset = ((bounds.size.height - line_height) * 0.5).max(px(0.0));
                line.paint(
                    point(bounds.left() + text_x_offset - scroll_offset, bounds.top() + box_v_offset),
                    line_height,
                    text_align,
                    None,
                    window,
                    cx,
                ).ok();
            }
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }
        
        self.state.read(cx).layout_cache.borrow_mut().bounds = Some(bounds);
    }
}

impl Render for SimpleInputState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_theme(&t);
        let focus_handle = self.focus_handle.clone();
        let icon = self.icon.clone();
        let multiline = self.multiline;

        // Captured scrollbar track bounds (window-space), written by the scrollbar
        // `canvas` on every paint so geometry / drag are always in sync — including
        // while the parent bottom panel is being resized (the input fills its
        // height, so the track grows/shrinks with it).
        let track_bounds: Rc<RefCell<Option<Bounds<Pixels>>>> =
            Rc::new(RefCell::new(None));

        // Multiline inputs get a definite pixel height so the element never
        // collapses to zero height (which would make it unclickable / impossible
        // to focus when wrapped inside auto-height parents).
        //
        // The declared height is `rows * line_height + 2 * padding` so that the
        // *content* box (what the inner text element fills via `relative(1.0)`)
        // equals exactly `rows * line_height`. Omitting the padding here is what
        // previously clipped the last visible row.
        let effective_rows = if self.auto_height {
            let shaped_rows = self.layout_cache.borrow().row_ranges.len();
            if shaped_rows > 0 {
                shaped_rows
            } else {
                self.value.lines().count().max(1)
            }
        } else {
            self.multiline_visible_rows
        };
        let pad = self.content_padding.unwrap_or(px(4.0));
        let line_height = if self.layout_cache.borrow().line_height > px(1.0) {
            self.layout_cache.borrow().line_height
        } else {
            window.line_height()
        };
        let multiline_height = if multiline && !self.fill_height {
            let slack = if self.auto_height || pad > px(0.0) { px(2.0) } else { px(0.0) };
            Some(line_height * (effective_rows as f32) + pad * 2.0 + slack)
        } else {
            None
        };

        let show_clear = self.allow_clear && !self.value.is_empty() && !self.read_only;

        div()
            .id(ElementId::from(&focus_handle))
            .track_focus(&focus_handle)
            .relative()
            .flex()
            .when(multiline, |d| d.items_start())
            .when(!multiline, |d| d.items_center().h_full().overflow_hidden())
            .gap(px(6.0))
            .w_full()
            .when_some(multiline_height, |d, h| d.h(h).py(pad).overflow_hidden())
            .when(multiline && self.fill_height, |d| d.flex_1().h_full().min_h(px(0.0)).overflow_hidden())
            .when_some(self.content_padding, |d, p| d.px(p))
            .when(self.content_padding.is_none(), |d| {
                d.when(self.show_gutter, |d| d.pr(px(8.0)))
                    .when(!self.show_gutter && !show_clear, |d| d.px(px(8.0)))
                    .when(!self.show_gutter && show_clear, |d| d.pl(px(8.0)).pr(px(4.0)))
            })
            .cursor_text()
            .when(multiline, |d| {
                d.on_scroll_wheel(cx.listener(move |this, event: &ScrollWheelEvent, _window, cx| {
                    let line_height = this.layout_cache.borrow().line_height.max(px(1.0));
                    // `pixel_delta` converts line-based wheel deltas using the row
                    // height. Wheel-down yields a negative `delta.y`, so subtract
                    // to move the viewport down (reveal later content). The final
                    // range is clamped in `prepaint`.
                    let dy = event.delta.pixel_delta(line_height).y;
                    let mut cache = this.layout_cache.borrow_mut();
                    cache.scroll_offset_y = (cache.scroll_offset_y - dy).max(px(0.0));
                    // Manual scroll: stop auto-following the cursor so the user can
                    // review earlier content; typing re-enables it.
                    this.follow_cursor = false;
                    cx.notify();
                }))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.focus(window, cx);
                    cx.emit(InputFocusedEvent);
                    let pos = this.char_position_for_mouse(event.position);
                    this.last_mouse_position = Some(event.position);

                    if event.click_count >= 3 {
                        this.is_selecting = false;
                        this.select_line_at(pos, cx);
                    } else if event.click_count == 2 {
                        this.is_selecting = false;
                        this.select_word_at(pos, cx);
                    } else {
                        if event.modifiers.shift {
                            this.select_to(pos);
                        } else {
                            this.move_to(pos);
                        }
                        this.is_selecting = true;
                        this.select_anchor = pos;
                        this.reset_cursor_blink();
                        this.start_drag_scroll_task(cx);
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                this.last_mouse_position = Some(event.position);
                if this.is_selecting {
                    if event.pressed_button != Some(MouseButton::Left) {
                        this.is_selecting = false;
                        return;
                    }
                    let pos = this.char_position_for_mouse(event.position);
                    this.select_to(pos);
                    this.reset_cursor_blink();
                    cx.notify();
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                if this.is_scrollbar_dragging {
                    this.end_scrollbar_drag(cx);
                }
                this.is_selecting = false;
                this.last_mouse_position = None;
                this._drag_scroll_task = None;
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if this.handle_key_down(event, cx) == KeyHandled::Handled {
                    cx.stop_propagation();
                }
            }))
            .when_some(icon, |d, icon_path| {
                d.child(icon_path.size(px(12.0)).text_color(p.text_muted))
            })
            .when(self.show_gutter, |d| {
                let line_count = self.value.split('\n').count().max(1);
                let active_line = self.current_line();
                // Width sized from the digit count so large line numbers never
                // clip. Uses the actual text font size for the estimate.
                let digits = line_count.to_string().len().max(2) as f32;
                let fs = f32::from(self.layout_cache.borrow().font_size.max(px(11.0)));
                let gutter_w = (digits * fs * 0.62 + 12.0).max(24.0);
                let this_entity = cx.entity();
                let text_muted = p.text_muted;
                let text_primary = p.text_primary;
                let border = p.border_subtle;

                d.child(
                    div()
                    .w(px(gutter_w))
                    .h_full()
                    .overflow_hidden()
                    .border_r_1()
                    .border_color(border)
                        .child(
                            // Paint the line numbers exactly like the text body:
                            // shaped with the *same* font + font size, and painted
                            // via `ShapedLine::paint` at the *same* per-visual-row
                            // `y = line_height * row - scroll` and *same* line
                            // height. 
                            canvas(
                                |_, _, _| {},
                                move |bounds: Bounds<Pixels>, _, window: &mut Window, cx: &mut App| {
                                    let (line_height, font_size, scroll, value, row_ranges) = {
                                        let this = this_entity.read(cx);
                                        let cache = this.layout_cache.borrow();
                                        (
                                            cache.line_height.max(px(1.0)),
                                            cache.font_size.max(px(1.0)),
                                            cache.scroll_offset_y,
                                            this.value.clone(),
                                            cache.row_ranges.clone(),
                                        )
                                    };
                                    let font = window.text_style().font();
                                    let top = bounds.origin.y;
                                    let vis_top = top;
                                    let vis_bottom = top + bounds.size.height;

                                    let mut logical = 0usize;
                                    for (i, r) in row_ranges.iter().enumerate() {
                                        // A visual row starts a new logical line
                                        // when its byte range begins right at the
                                        // start of the text or just after a '\n'.
                                        // Wrapped continuation rows get no number.
                                        let is_line_start = r.start == 0
                                            || (r.start > 0
                                                && value.as_bytes().get(r.start - 1) == Some(&b'\n'));
                                        if !is_line_start {
                                            continue;
                                        }
                                        logical += 1;
                                        let y = top + line_height * (i as f32) - scroll;
                                        // Skip rows scrolled out of view.
                                        if y + line_height < vis_top || y > vis_bottom {
                                            continue;
                                        }
                                        let is_active = logical == active_line;
                                        let color: Hsla = if is_active {
                                            text_primary
                                        } else {
                                            text_muted
                                        };
                                        let mut f = font.clone();
                                        if is_active {
                                            f.weight = FontWeight::BOLD;
                                        }
                                        let s = logical.to_string();
                                        let run = TextRun {
                                            len: s.len(),
                                            font: f,
                                            color,
                                            background_color: None,
                                            underline: None,
                                            strikethrough: None,
                                        };
                                        let shaped = window
                                            .text_system()
                                            .shape_line(s.into(), font_size, &[run], None);
                                        // Right-align within the gutter (6px padding).
                                        let x = bounds.origin.x + bounds.size.width
                                            - px(6.0)
                                            - shaped.width;
                                        shaped
                                            .paint(
                                                point(x, y),
                                                line_height,
                                                gpui::TextAlign::Left,
                                                None,
                                                window,
                                                cx,
                                            )
                                            .ok();
                                    }
                                },
                            )
                            .size_full(),
                        )
                )
            })
            .child(TextInputElement { state: cx.entity() })
            .when(show_clear, |d| {
                let this_entity = cx.entity();
                let clear_tip = velowork_i18n::i18n!(cx, "common.clear_content");
                d.child(
                    div()
                        .id("simple-input-clear-btn")
                        .flex_shrink_0()
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(2.0))
                        .rounded(RADIUS_STD)
                        .hover(|s| s.bg(surface_bg_t(t.bg_hover, &t)))
                        .tooltip(move |_, cx| {
                            let tip = clear_tip.clone();
                            cx.new(|_| Tooltip::new(tip)).into()
                        })
                        .on_mouse_down(MouseButton::Left, move |_event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this_entity.update(cx, |this, cx| {
                                this.set_value("", cx);
                                this.focus(window, cx);
                            });
                        })
                        .child(AppIcon::Close.size(px(12.0)).text_color(p.text_muted)),
                )
            })
            // Vertical scrollbar overlay (multiline). The track's real bounds are
            // captured into `track_bounds` by the `canvas` on every paint, so the
            // thumb and drag stay correct even while the bottom panel is resized
            // (the input fills its height, so the track grows/shrinks with it).
            // Clicking / dragging anywhere on the track jumps + drags the thumb
            // (jump-and-drag), matching the rest of the app's scrollbars.
            .when(multiline && !self.auto_height, |d| {
                let this_entity = cx.entity();
                let sb = t.text_muted;
                let sbh = t.text_secondary;
                d.child(
                    div()
                        .id("simple-input-scrollbar")
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .right_0()
                        .w(px(12.0))
                        .cursor(CursorStyle::Arrow)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener({
                                let tb = track_bounds.clone();
                                move |this, event: &MouseDownEvent, _window, cx| {
                                    if let Some(bounds) = *tb.borrow() {
                                        this.scroll_to_y_from_pointer(&bounds, f32::from(event.position.y), cx);
                                    }
                                    cx.stop_propagation();
                                }
                            }),
                        )
                        .on_mouse_move(cx.listener({
                            let tb = track_bounds.clone();
                            move |this, event: &MouseMoveEvent, _window, cx| {
                                if !this.is_scrollbar_dragging {
                                    return;
                                }
                                if let Some(bounds) = *tb.borrow() {
                                    this.scroll_to_y_from_pointer(&bounds, f32::from(event.position.y), cx);
                                }
                            }
                        }))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                                this.end_scrollbar_drag(cx);
                            }),
                        )
                        .child(
                            canvas(
                                {
                                    let tb = track_bounds.clone();
                                    move |bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut App| {
                                        *tb.borrow_mut() = Some(bounds);
                                    }
                                },
                                move |bounds, _, window, cx| {
                                    let this = this_entity.read(cx);
                                    let cache = this.layout_cache.borrow();
                                    let line_height = f32::from(cache.line_height.max(px(1.0)));
                                    let rows = cache.row_ranges.len().max(1);
                                    let content = line_height * rows as f32;
                                    let viewport = f32::from(bounds.size.height);
                                    if viewport <= 0.0 || content <= viewport + 1.0 {
                                        return;
                                    }
                                    let thumb = (viewport / content * viewport).max(20.0).min(viewport);
                                    let max_scroll = (content - viewport).max(1.0);
                                    let ratio = (f32::from(cache.scroll_offset_y) / max_scroll).clamp(0.0, 1.0);
                                    let pos = ratio * (viewport - thumb);
                                    let color = if this.is_scrollbar_dragging { sbh } else { sb };
                                    let thumb_bounds = Bounds {
                                        origin: point(bounds.origin.x + px(3.0), bounds.origin.y + px(pos)),
                                        size: size(px(6.0), px(thumb)),
                                    };
                                    window.paint_quad(fill(thumb_bounds, rgb(color)).corner_radii(px(3.0)));
                                },
                            )
                            .size_full(),
                        ),
                )
            })
    }
}

impl Focusable for SimpleInputState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<InputChangedEvent> for SimpleInputState {}
impl EventEmitter<InputFocusedEvent> for SimpleInputState {}
impl EventEmitter<InputEvent> for SimpleInputState {}

#[derive(IntoElement)]
pub struct SimpleInput {
    state: Entity<SimpleInputState>,
    appearance: bool,
    size: ControlSize,
    search: bool,
    mask_toggle: bool,
    prefix: Option<AnyElement>,
    suffix: Option<AnyElement>,
    text_size: Option<Pixels>,
    line_height: Option<Pixels>,
    fill_height: bool,
    container_height: bool,
    custom_width: Option<Pixels>,
    custom_height: Option<Pixels>,
    custom_bg: Option<Hsla>,
    custom_border_color: Option<Hsla>,
    custom_text_color: Option<Hsla>,
    focus_ring: bool,
    cleanable: Option<bool>,
}

impl SimpleInput {
    pub fn new(state: &Entity<SimpleInputState>) -> Self {
        Self {
            state: state.clone(),
            appearance: true,
            size: ControlSize::Default,
            search: false,
            mask_toggle: false,
            prefix: None,
            suffix: None,
            text_size: None,
            line_height: None,
            fill_height: false,
            container_height: false,
            custom_width: None,
            custom_height: None,
            custom_bg: None,
            custom_border_color: None,
            custom_text_color: None,
            focus_ring: true,
            cleanable: None,
        }
    }

    /// Set appearance style: true for bordered container (default), false for borderless.
    pub fn appearance(mut self, appearance: bool) -> Self {
        self.appearance = appearance;
        self
    }

    /// Set borderless mode. `.borderless(true)` removes container background, border, and focus ring.
    pub fn borderless(mut self, borderless: bool) -> Self {
        self.appearance = !borderless;
        self
    }

    /// Set search input mode. When true, automatically displays a search icon prefix
    /// and enables clearable text behavior.
    pub fn search(mut self, search: bool) -> Self {
        self.search = search;
        self
    }

    /// Enable or disable password mask toggle (eye icon on the right).
    pub fn mask_toggle(mut self, toggle: bool) -> Self {
        self.mask_toggle = toggle;
        self
    }

    /// Shortcut for password inputs with eye toggle enabled.
    pub fn password(mut self, password: bool) -> Self {
        self.mask_toggle = password;
        self
    }

    /// Use compact control size.
    pub fn compact(mut self) -> Self {
        self.size = ControlSize::Compact;
        self
    }

    /// Explicitly set the control size.
    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    /// Set leading element (e.g. search icon).
    pub fn prefix(mut self, prefix: impl IntoElement) -> Self {
        self.prefix = Some(prefix.into_any_element());
        self
    }

    /// Set trailing element.
    pub fn suffix(mut self, suffix: impl IntoElement) -> Self {
        self.suffix = Some(suffix.into_any_element());
        self
    }

    pub fn text_size(mut self, size: Pixels) -> Self {
        self.text_size = Some(size);
        self
    }

    /// Override the line height used for this input's text.
    pub fn line_height(mut self, height: Pixels) -> Self {
        self.line_height = Some(height);
        self
    }

    /// Marks this input to fill the parent container's height (for resizable textareas).
    pub fn fill_height(mut self) -> Self {
        self.fill_height = true;
        self
    }

    /// Skip the built-in single-line fixed height; let the parent container control the height.
    pub fn container_height(mut self) -> Self {
        self.container_height = true;
        self
    }

    pub fn w(mut self, width: impl Into<Pixels>) -> Self {
        self.custom_width = Some(width.into());
        self
    }

    pub fn h(mut self, height: impl Into<Pixels>) -> Self {
        self.custom_height = Some(height.into());
        self
    }

    pub fn bg(mut self, bg: impl Into<Hsla>) -> Self {
        self.custom_bg = Some(bg.into());
        self
    }

    pub fn border_color(mut self, border: impl Into<Hsla>) -> Self {
        self.custom_border_color = Some(border.into());
        self
    }

    pub fn text_color(mut self, color: impl Into<Hsla>) -> Self {
        self.custom_text_color = Some(color.into());
        self
    }

    pub fn focus_ring(mut self, focus_ring: bool) -> Self {
        self.focus_ring = focus_ring;
        self
    }

    /// When true, enables the clear icon on the right side when text is present.
    pub fn allow_clear(self, allow: bool, cx: &mut App) -> Self {
        self.state.update(cx, |s, _| s.allow_clear = allow);
        self
    }

    /// Alias for [`allow_clear`].
    #[allow(non_snake_case)]
    pub fn allowClear(self, allow: bool, cx: &mut App) -> Self {
        self.allow_clear(allow, cx)
    }

    pub fn cleanable(mut self, cleanable: bool) -> Self {
        self.cleanable = Some(cleanable);
        self
    }

    pub fn content_type(mut self, ct: impl std::fmt::Debug) -> Self {
        let debug_str = format!("{:?}", ct);
        if debug_str.contains("Password") {
            self.mask_toggle = true;
        }
        self
    }
}

impl RenderOnce for SimpleInput {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        if let Some(cleanable) = self.cleanable {
            self.state.update(cx, |s, _| s.allow_clear = cleanable);
        } else if self.search {
            self.state.update(cx, |s, _| s.allow_clear = true);
        }
        if self.fill_height {
            self.state.update(cx, |s, _| s.set_fill_height(true));
        }
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);
        let density = crate::tokens::get_ui_density(cx);
        let scale = crate::tokens::ui_text_scale(cx);
        let geom = ControlAppearance::resolve(
            self.size,
            ControlVariant::Secondary,
            &p,
            density,
            scale,
        );
        let is_multiline = self.state.read(cx).is_multiline();
        let is_read_only = self.state.read(cx).read_only;
        let focus_handle = self.state.read(cx).focus_handle(cx);
        let ring = focus_ring_shadows(&t);
        let text_col = self.custom_text_color.unwrap_or(p.text_primary);

        let prefix_element = if let Some(pfx) = self.prefix {
            Some(pfx)
        } else if self.search {
            Some(
                AppIcon::Search
                    .svg()
                    .size(geom.icon_size)
                    .text_color(p.text_muted)
                    .into_any_element(),
            )
        } else {
            None
        };

        let mask_toggle_btn = if self.mask_toggle {
            let is_masked = self.state.read(cx).is_password();
            let toggle_icon = if is_masked {
                AppIcon::Eye
            } else {
                AppIcon::EyeOff
            };
            let tip_text = if is_masked {
                i18n!(cx, "common.show_password")
            } else {
                i18n!(cx, "common.hide_password")
            };
            let state_clone = self.state.clone();

            Some(
                div()
                    .id("input-mask-toggle-btn")
                    .cursor_pointer()
                    .flex_shrink_0()
                    .w(px(20.0))
                    .h(px(20.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(RADIUS_XS)
                    .text_color(p.text_muted)
                    .hover(move |s| s.text_color(p.text_primary).bg(geom.bg_hover))
                    .tooltip(move |_, cx| {
                        let __tip = tip_text.clone();
                        cx.new(|_| Tooltip::new(__tip)).into()
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_click(move |_, _window, cx| {
                        state_clone.update(cx, |s, cx| {
                            s.set_password(!s.is_password());
                            cx.notify();
                        });
                    })
                    .child(
                        toggle_icon
                            .svg()
                            .size(geom.icon_size)
                            .text_color(p.text_muted),
                    )
                    .into_any_element(),
            )
        } else {
            None
        };

        let suffix_element = match (self.suffix, mask_toggle_btn) {
            (Some(sfx), Some(toggle_btn)) => Some(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(sfx)
                    .child(toggle_btn)
                    .into_any_element(),
            ),
            (Some(sfx), None) => Some(
                div()
                    .flex_shrink_0()
                    .child(sfx)
                    .into_any_element(),
            ),
            (None, Some(toggle_btn)) => Some(
                div()
                    .flex_shrink_0()
                    .child(toggle_btn)
                    .into_any_element(),
            ),
            (None, None) => None,
        };

        let is_focused = focus_handle.is_focused(_window);
        let has_custom_border = self.custom_border_color.is_some();
        let has_custom_bg = self.custom_bg.is_some();
        let bg_col = self.custom_bg.unwrap_or(if is_focused {
            p.surface_hover
        } else {
            geom.bg
        });
        let border_col = self.custom_border_color.unwrap_or(if is_focused {
            p.border_active
        } else {
            geom.border_color
        });

        if self.appearance {
            div()
                .on_mouse_down(MouseButton::Left, {
                    let state = self.state.clone();
                    move |_, window, cx| {
                        cx.stop_propagation();
                        state.update(cx, |s, cx| s.focus(window, cx));
                    }
                })
                .flex()
                .when(!is_multiline && !self.fill_height, |d| d.items_center())
                .when(is_multiline || self.fill_height, |d| d.items_stretch())
                .when_some(self.custom_width, |d, w| d.w(w).flex_shrink_0())
                .when(self.custom_width.is_none(), |d| d.w_full())
                .when(is_multiline || self.fill_height, |d| {
                    d.flex_1().h_full().min_h(self.custom_height.unwrap_or(geom.height))
                })
                .when(!is_multiline && !self.fill_height && !self.container_height, |d| {
                    d.h(self.custom_height.unwrap_or(geom.height))
                })
                .when(self.container_height, |d| d.h_full())
                .bg(bg_col)
                .text_color(text_col)
                .border_1()
                .border_color(border_col)
                .rounded(geom.radius)
                .when(!is_focused && !is_read_only && !has_custom_border, |d| {
                    d.hover(move |s| {
                        let mut st = s.border_color(p.surface_accent.opacity(0.6));
                        if !has_custom_bg {
                            st = st.bg(p.surface_hover);
                        }
                        st
                    })
                })
                .track_focus(&focus_handle)
                .when(self.focus_ring && !is_read_only, |d| {
                    d.focus(move |s| {
                        let mut st = s;
                        if !has_custom_border {
                            st = st.border_color(p.border_active);
                        }
                        if !has_custom_bg {
                            st = st.bg(p.surface_hover);
                        }
                        st.shadow(ring)
                    })
                })
                .when_some(self.text_size, |d, size| d.text_size(size))
                .when_some(self.line_height, |d, lh| d.line_height(lh))
                .when_some(prefix_element, |d, pfx| {
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .pl(px(8.0))
                            .child(pfx),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .h_full()
                        .flex()
                        .overflow_hidden()
                        .child(self.state),
                )
                .when_some(suffix_element, |d, sfx| {
                    d.child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .pr(px(6.0))
                            .child(sfx),
                    )
                })
        } else {
            div()
                .on_mouse_down(MouseButton::Left, {
                    let state = self.state.clone();
                    move |_, window, cx| {
                        cx.stop_propagation();
                        state.update(cx, |s, cx| s.focus(window, cx));
                    }
                })
                .flex()
                .when(!is_multiline && !self.fill_height, |d| d.items_center())
                .when(is_multiline || self.fill_height, |d| d.items_stretch())
                .when_some(self.custom_width, |d, w| d.w(w).flex_shrink_0())
                .when(self.custom_width.is_none(), |d| d.w_full())
                .when(self.fill_height, |d| d.flex_1().h_full().min_h(px(0.0)))
                .when_some(self.custom_height, |d, h| d.h(h))
                .text_color(text_col)
                .when_some(self.text_size, |d, size| d.text_size(size))
                .when_some(self.line_height, |d, lh| d.line_height(lh))
                .when_some(prefix_element, |d, pfx| d.child(pfx))
                .child(
                    div()
                        .flex_1()
                        .when(self.fill_height, |d| d.h_full())
                        .flex()
                        .child(self.state),
                )
                .when_some(suffix_element, |d, sfx| d.child(sfx))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{clamp_to_char_boundary, SimpleInputState};

    #[test]
    fn test_clamp_to_char_boundary() {
        let s = "Hello世界";
        assert_eq!(clamp_to_char_boundary(s, 0), 0);
        assert_eq!(clamp_to_char_boundary(s, 5), 5); // start of 世界
        assert_eq!(clamp_to_char_boundary(s, 6), 5); // inside 世
        assert_eq!(clamp_to_char_boundary(s, 8), 8); // start of 界
        assert_eq!(clamp_to_char_boundary(s, 11), 11);
    }

    #[gpui::test]
    fn test_allow_clear_toggle(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        let input = cx.new(|cx| SimpleInputState::new(cx).allow_clear(true).default_value("hello"));
        assert!(input.read_with(cx, |this, _| this.is_allow_clear()));
        assert_eq!(input.read_with(cx, |this, _| this.value().to_string()), "hello");

        input.update(cx, |this, cx| {
            this.set_value("", cx);
        });
        assert_eq!(input.read_with(cx, |this, _| this.value().to_string()), "");
    }

    #[gpui::test]
    fn test_password_and_mask_toggle(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        let input = cx.new(|cx| SimpleInputState::new(cx).masked(true));
        input.update(cx, |this, _| {
            this.set_password(false);
        });
        assert!(!input.read_with(cx, |this, _| this.is_password()));
    }

    #[gpui::test]
    fn test_syntax_language(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        let input = cx.new(|cx| SimpleInputState::new(cx).syntax_language(Some("bash")).default_value("echo 'hello'"));
        assert_eq!(input.read_with(cx, |this, _| this.syntax_language.clone()), Some("bash".to_string()));

        input.update(cx, |this, cx| {
            this.set_syntax_language(Some("json"), cx);
        });
        assert_eq!(input.read_with(cx, |this, _| this.syntax_language.clone()), Some("json".to_string()));
    }

    #[gpui::test]
    fn test_port_input_validation(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        let input = cx.new(|cx| {
            SimpleInputState::new(cx)
                .digits_only(true)
                .max_length(5)
                .max_number(65535)
                .default_value("80abc")
        });
        // Non-digits stripped
        assert_eq!(input.read_with(cx, |this, _| this.value().to_string()), "80");

        // Insert digits
        input.update(cx, |this, cx| {
            this.insert_text("80", cx);
        });
        assert_eq!(input.read_with(cx, |this, _| this.value().to_string()), "8080");

        // Insert non-digits ignored
        input.update(cx, |this, cx| {
            this.insert_text("xyz!@", cx);
        });
        assert_eq!(input.read_with(cx, |this, _| this.value().to_string()), "8080");

        // Over 65535 clamped
        input.update(cx, |this, cx| {
            this.set_value("99999", cx);
        });
        assert_eq!(input.read_with(cx, |this, _| this.value().to_string()), "65535");

        // Length limit
        input.update(cx, |this, cx| {
            this.set_value("12345678", cx);
        });
        assert_eq!(input.read_with(cx, |this, _| this.value().to_string()), "12345");
    }

    #[gpui::test]
    fn test_auto_height_and_read_only_flags(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        let input = cx.new(|cx| {
            SimpleInputState::new(cx)
                .multiline()
                .auto_height(true)
                .read_only(true)
                .default_value("sudo du -xhd 1 / | sort -hr")
        });
        input.read_with(cx, |this, _| {
            assert!(this.auto_height);
            assert!(this.read_only);
            assert_eq!(this.layout_cache.borrow().scroll_offset_y, gpui::px(0.0));
        });
    }

    #[gpui::test]
    fn test_copy_key_handled_only_when_selection_exists(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        use super::KeyHandled;
        let input = cx.new(|cx| SimpleInputState::new(cx).default_value("hello world"));

        // Without selection, Ctrl+C should be NotHandled so parent elements can handle it
        let not_handled = input.update(cx, |this, cx| {
            let ev = gpui::KeyDownEvent {
                keystroke: gpui::Keystroke::parse("ctrl-c").expect("valid keystroke"),
                is_held: false,
                prefer_character_input: false,
            };
            this.handle_key_down(&ev, cx)
        });
        assert_eq!(not_handled, KeyHandled::NotHandled);

        // With selection, Ctrl+C should be Handled
        let handled = input.update(cx, |this, cx| {
            this.select_all(cx);
            let ev = gpui::KeyDownEvent {
                keystroke: gpui::Keystroke::parse("ctrl-c").expect("valid keystroke"),
                is_held: false,
                prefer_character_input: false,
            };
            this.handle_key_down(&ev, cx)
        });
        assert_eq!(handled, KeyHandled::Handled);
    }
}

