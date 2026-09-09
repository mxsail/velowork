//! DocumentView GPUI component rendering document block list and managing selection drag logic.

use gpui::*;
use gpui::prelude::*;
use velowork_ui::simple_input::{SimpleInput, SimpleInputState};
use velowork_ui::theme::theme;
use velowork_ui::tokens::{markdown_font_family, mono_font_family, ui_text_md, ui_text_xl, use_custom_markdown_font};
use velowork_ui::{h_flex, v_flex};

use crate::clipboard::export_selection_to_markdown;
use crate::document::{Document, DocumentBlock};
use crate::position::DocumentPosition;
use crate::selection::DocumentSelection;

/// GPUI View wrapping the parsed document tree.
pub struct DocumentView {
    document: Document,
    block_states: Vec<Entity<SimpleInputState>>,
    selection: DocumentSelection,
    active_drag_start: Option<DocumentPosition>,
}

/// Selection event emitted by the DocumentView.
#[derive(Clone, Debug)]
pub enum DocumentViewEvent {
    SelectionStarted,
}

impl EventEmitter<DocumentViewEvent> for DocumentView {}

impl DocumentView {
    /// Create a new DocumentView.
    pub fn new(content: &str, cx: &mut Context<Self>) -> Self {
        let doc = Document::parse(content);
        let mut block_states = Vec::new();
        for block in &doc.blocks {
            let state = cx.new(|cx| {
                SimpleInputState::new(cx)
                    .multiline()
                    .wrap(true)
                    .read_only(true)
                    .default_value(block.plain_text())
            });
            block_states.push(state);
        }

        Self {
            document: doc,
            block_states,
            selection: DocumentSelection::default(),
            active_drag_start: None,
        }
    }

    /// Update the text content of the document.
    pub fn set_content(&mut self, content: &str, cx: &mut Context<Self>) {
        let doc = Document::parse(content);

        while self.block_states.len() < doc.blocks.len() {
            let state = cx.new(|cx| {
                SimpleInputState::new(cx)
                    .multiline()
                    .wrap(true)
                    .read_only(true)
            });
            self.block_states.push(state);
        }
        self.block_states.truncate(doc.blocks.len());

        for (i, block) in doc.blocks.iter().enumerate() {
            let val = block.plain_text().to_string();
            self.block_states[i].update(cx, |s, cx| {
                s.set_value(val, cx);
            });
        }

        self.document = doc;
        self.selection.clear();
        cx.notify();
    }

    /// Returns the active selection formatted as Markdown.
    pub fn get_selected_markdown(&self) -> Option<String> {
        let (start, end) = self.selection.normalized()?;
        if self.selection.is_empty() {
            return None;
        }
        Some(export_selection_to_markdown(&self.document, start, end))
    }

    /// Returns the active selection as plain text.
    pub fn get_selected_text(&self, _cx: &App) -> Option<String> {
        let (start, end) = self.selection.normalized()?;
        if self.selection.is_empty() {
            return None;
        }
        let mut out = String::new();
        for i in start.block_index..=end.block_index {
            if i >= self.document.blocks.len() {
                break;
            }
            let block = &self.document.blocks[i];
            let plain = block.plain_text();
            let len = plain.chars().count();
            let start_char = if i == start.block_index { start.char_offset } else { 0 };
            let end_char = if i == end.block_index { end.char_offset } else { len };
            if start_char < end_char {
                if i > start.block_index {
                    out.push('\n');
                }
                let substring: String = plain.chars().skip(start_char).take(end_char - start_char).collect();
                out.push_str(&substring);
            }
        }
        Some(out)
    }

    /// Clear current selections.
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.selection.clear();
        for state in &self.block_states {
            state.update(cx, |s, cx| {
                s.clear_selection(cx);
            });
        }
        cx.notify();
    }

    fn update_selections(&mut self, cx: &mut Context<Self>) {
        let norm = self.selection.normalized();
        for i in 0..self.block_states.len() {
            let plain = self.document.blocks[i].plain_text();
            let len = plain.chars().count();

            let local_range = if let Some((start, end)) = norm {
                if i < start.block_index || i > end.block_index {
                    None
                } else {
                    let start_char = if i == start.block_index { start.char_offset } else { 0 };
                    let end_char = if i == end.block_index { end.char_offset } else { len };
                    if start_char < end_char {
                        let start_byte = char_offset_to_byte_offset(plain, start_char);
                        let end_byte = char_offset_to_byte_offset(plain, end_char);
                        Some(start_byte..end_byte)
                    } else {
                        None
                    }
                }
            } else {
                None
            };

            self.block_states[i].update(cx, |s, cx| {
                s.set_selection(local_range, false, cx);
            });
        }
    }

    fn hit_test_block(&self, position: Point<Pixels>, cx: &App) -> Option<(usize, usize)> {
        for (i, state) in self.block_states.iter().enumerate() {
            let s = state.read(cx);
            if let Some(bounds) = s.last_bounds() {
                if bounds.contains(&position) {
                    let text = s.value();
                    let byte_offset = s.char_position_for_mouse(position);
                    let char_offset = byte_offset_to_char_offset(text, byte_offset);
                    return Some((i, char_offset));
                }
            }
        }

        if let Some(first_state) = self.block_states.first() {
            if let Some(bounds) = first_state.read(cx).last_bounds() {
                if position.y < bounds.top() {
                    return Some((0, 0));
                }
            }
        }

        if let Some(last_state) = self.block_states.last() {
            if let Some(bounds) = last_state.read(cx).last_bounds() {
                if position.y > bounds.bottom() {
                    let last_idx = self.block_states.len() - 1;
                    let text = last_state.read(cx).value();
                    return Some((last_idx, text.chars().count()));
                }
            }
        }

        None
    }
}

fn char_offset_to_byte_offset(s: &str, char_offset: usize) -> usize {
    let mut byte_offset = 0;
    for (i, c) in s.char_indices().enumerate() {
        if i == char_offset {
            return c.0;
        }
        byte_offset = c.0 + c.1.len_utf8();
    }
    byte_offset
}

fn byte_offset_to_char_offset(s: &str, byte_offset: usize) -> usize {
    for (i, c) in s.char_indices().enumerate() {
        if c.0 >= byte_offset {
            return i;
        }
    }
    s.chars().count()
}

impl Render for DocumentView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let this_weak = cx.entity().downgrade();

        let mut children = Vec::new();
        for (i, block) in self.document.blocks.iter().enumerate() {
            let state = &self.block_states[i];

            let block_element = match block {
                DocumentBlock::Paragraph { .. } => {
                    SimpleInput::new(state)
                        .text_size(ui_text_md(cx))
                        .line_height(ui_text_md(cx) * 1.618)
                        .into_any_element()
                }
                DocumentBlock::Heading { level, .. } => {
                    let size = match level {
                        1 => ui_text_xl(cx),
                        2 => ui_text_md(cx),
                        _ => ui_text_md(cx),
                    };
                    SimpleInput::new(state)
                        .text_size(size)
                        .line_height(size * 1.3)
                        .into_any_element()
                }
                DocumentBlock::CodeBlock { language, .. } => {
                    let lang = language.as_deref().unwrap_or("text");
                    v_flex()
                        .w_full()
                        .rounded(px(6.0))
                        .bg(rgb(t.bg_panel))
                        .border_1()
                        .border_color(rgb(t.border))
                        .overflow_hidden()
                        .child(
                            h_flex()
                                .justify_between()
                                .bg(rgb(t.bg_hover))
                                .px(px(8.0))
                                .py(px(4.0))
                                .border_b_1()
                                .border_color(rgb(t.border))
                                .child(
                                    div()
                                        .text_size(ui_text_md(cx))
                                        .text_color(rgb(t.text_muted))
                                        .child(lang.to_string())
                                )
                        )
                        .child(
                            div()
                                .p(px(8.0))
                                .font_family(mono_font_family(cx))
                                .child(
                                    SimpleInput::new(state)
                                        .text_size(ui_text_md(cx))
                                )
                        )
                        .into_any_element()
                }
                DocumentBlock::Blockquote { .. } => {
                    h_flex()
                        .w_full()
                        .gap(px(8.0))
                        .child(
                            div()
                                .w(px(3.0))
                                .bg(rgb(t.border_active))
                                .rounded(px(1.5))
                        )
                        .child(
                            div()
                                .flex_1()
                                .child(
                                    SimpleInput::new(state)
                                        .text_size(ui_text_md(cx))
                                        .line_height(ui_text_md(cx) * 1.4)
                                )
                        )
                        .into_any_element()
                }
                DocumentBlock::ListItem { ordered, depth, .. } => {
                    let bullet = if *ordered { "1. " } else { "• " };
                    let indent = depth * 12;
                    h_flex()
                        .w_full()
                        .pl(px(indent as f32))
                        .child(
                            div()
                                .w(px(16.0))
                                .text_align(TextAlign::Right)
                                .text_size(ui_text_md(cx))
                                .text_color(rgb(t.text_muted))
                                .child(bullet.to_string())
                        )
                        .child(
                            div()
                                .flex_1()
                                .child(
                                    SimpleInput::new(state)
                                        .text_size(ui_text_md(cx))
                                        .line_height(ui_text_md(cx) * 1.2)
                                )
                        )
                        .into_any_element()
                }
                DocumentBlock::HorizontalRule => {
                    div()
                        .h(px(1.0))
                        .bg(rgb(t.border))
                        .my(px(8.0))
                        .into_any_element()
                }
                DocumentBlock::Frontmatter { .. } => {
                    v_flex()
                        .w_full()
                        .p(px(8.0))
                        .bg(rgb(t.bg_panel))
                        .border_1()
                        .border_color(rgb(t.border))
                        .rounded(px(4.0))
                        .child(
                            SimpleInput::new(state)
                                .text_size(ui_text_md(cx))
                        )
                        .into_any_element()
                }
                DocumentBlock::Table { .. } => {
                    div()
                        .w_full()
                        .p(px(8.0))
                        .bg(rgb(t.bg_panel))
                        .border_1()
                        .border_color(rgb(t.border))
                        .rounded(px(4.0))
                        .font_family(mono_font_family(cx))
                        .child(
                            SimpleInput::new(state)
                                .text_size(ui_text_md(cx))
                        )
                        .into_any_element()
                }
            };

            children.push(block_element);
        }

        let this_weak_down = this_weak.clone();
        let this_weak_move = this_weak.clone();
        let this_weak_up = this_weak.clone();

        div()
            .id("document-view-container")
            .w_full()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, _window, cx| {
                if let Some(this) = this_weak_down.upgrade() {
                    this.update(cx, |this, cx| {
                        this.clear_selection(cx);
                        cx.emit(DocumentViewEvent::SelectionStarted);
                        let pos = event.position;
                        if let Some((idx, char_offset)) = this.hit_test_block(pos, cx) {
                            let doc_pos = DocumentPosition::new(idx, char_offset);
                            this.selection.start = Some(doc_pos);
                            this.selection.end = Some(doc_pos);
                            this.selection.is_selecting = true;
                            this.active_drag_start = Some(doc_pos);
                            this.update_selections(cx);
                        }
                    });
                }
            })
            .on_mouse_move(move |event: &MouseMoveEvent, _window, cx| {
                if let Some(this) = this_weak_move.upgrade() {
                    this.update(cx, |this, cx| {
                        if this.selection.is_selecting {
                            let pos = event.position;
                            if let Some((idx, char_offset)) = this.hit_test_block(pos, cx) {
                                let doc_pos = DocumentPosition::new(idx, char_offset);
                                this.selection.end = Some(doc_pos);
                                this.update_selections(cx);
                            }
                        }
                    });
                }
            })
            .on_mouse_up(MouseButton::Left, move |_event: &MouseUpEvent, _window, cx| {
                if let Some(this) = this_weak_up.upgrade() {
                    this.update(cx, |this, cx| {
                        this.selection.is_selecting = false;
                        this.active_drag_start = None;
                        cx.notify();
                    });
                }
            })
            .when(use_custom_markdown_font(cx), |d| d.font_family(markdown_font_family(cx)))
            .children(children)
    }
}
