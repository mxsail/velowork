//! DocumentView GPUI component rendering document block list and managing selection drag logic.

use gpui::*;
use gpui::prelude::*;
use velowork_i18n::i18n;
use velowork_ui::icon::AppIcon;
use velowork_ui::simple_input::{SimpleInput, SimpleInputState};
use velowork_ui::theme::theme;
use velowork_ui::tokens::{markdown_font_family, mono_font_family, ui_text_md, ui_text_xl, use_custom_markdown_font};
use velowork_ui::{h_flex, v_flex, SemanticPalette};

use crate::clipboard::export_selection_to_markdown;
use crate::document::{Document, DocumentBlock};
use crate::position::DocumentPosition;
use crate::selection::DocumentSelection;

/// GPUI View wrapping the parsed document tree.
pub struct DocumentView {
    raw_content: String,
    document: Document,
    block_states: Vec<Entity<SimpleInputState>>,
    selection: DocumentSelection,
    active_drag_start: Option<DocumentPosition>,
    copied_code_index: Option<usize>,
}

/// Selection event emitted by the DocumentView.
#[derive(Clone, Debug)]
pub enum DocumentViewEvent {
    SelectionStarted,
}

impl EventEmitter<DocumentViewEvent> for DocumentView {}

impl DocumentView {
    fn create_block_state(block: &DocumentBlock, cx: &mut Context<Self>) -> Entity<SimpleInputState> {
        let (wrap, syntax_language) = match block {
            DocumentBlock::CodeBlock { language, .. } => (false, language.as_deref()),
            DocumentBlock::Table { .. } => (false, None),
            _ => (true, None),
        };
        let text_value = match block {
            DocumentBlock::CodeBlock { code, .. } => code.trim_end_matches(['\r', '\n']),
            _ => block.plain_text(),
        };
        cx.new(|cx| {
            SimpleInputState::new(cx)
                .multiline()
                .auto_height(true)
                .wrap(wrap)
                .read_only(true)
                .content_padding(px(0.0))
                .syntax_language(syntax_language)
                .default_value(text_value)
        })
    }

    /// Create a new DocumentView.
    pub fn new(content: &str, cx: &mut Context<Self>) -> Self {
        let doc = Document::parse(content);
        let mut block_states = Vec::new();
        for block in &doc.blocks {
            block_states.push(Self::create_block_state(block, cx));
        }

        Self {
            raw_content: content.to_string(),
            document: doc,
            block_states,
            selection: DocumentSelection::default(),
            active_drag_start: None,
            copied_code_index: None,
        }
    }

    /// Returns the raw markdown string that this document view is representing.
    pub fn raw_content(&self) -> &str {
        &self.raw_content
    }

    /// Update the text content of the document.
    pub fn set_content(&mut self, content: &str, cx: &mut Context<Self>) {
        if self.raw_content == content {
            return;
        }
        self.raw_content = content.to_string();
        let doc = Document::parse(content);

        while self.block_states.len() < doc.blocks.len() {
            let i = self.block_states.len();
            self.block_states.push(Self::create_block_state(&doc.blocks[i], cx));
        }
        self.block_states.truncate(doc.blocks.len());

        for (i, block) in doc.blocks.iter().enumerate() {
            let val = match block {
                DocumentBlock::CodeBlock { code, .. } => code.trim_end_matches(['\r', '\n']).to_string(),
                _ => block.plain_text().to_string(),
            };
            let (wrap, syntax_language) = match block {
                DocumentBlock::CodeBlock { language, .. } => (false, language.as_deref()),
                DocumentBlock::Table { .. } => (false, None),
                _ => (true, None),
            };
            self.block_states[i].update(cx, |s, cx| {
                s.set_wrap(wrap);
                s.set_syntax_language(syntax_language, cx);
                s.set_auto_height(true);
                s.set_content_padding(px(0.0));
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
                        .borderless(true)
                        .text_size(ui_text_md(cx))
                        .line_height(ui_text_md(cx) * 1.5)
                        .into_any_element()
                }
                DocumentBlock::Heading { level, .. } => {
                    let size = match level {
                        1 => ui_text_xl(cx),
                        2 => ui_text_md(cx) * 1.15,
                        _ => ui_text_md(cx),
                    };
                    SimpleInput::new(state)
                        .borderless(true)
                        .text_size(size)
                        .into_any_element()
                }
                DocumentBlock::CodeBlock { language, code, depth } => {
                    let lang = language.as_deref().unwrap_or("");
                    let is_copied = self.copied_code_index == Some(i);
                    let copy_label = if is_copied {
                        i18n!(cx, "ai.copied")
                    } else {
                        i18n!(cx, "common.action.copy")
                    };
                    let copy_icon = if is_copied {
                        AppIcon::Check
                    } else {
                        AppIcon::Copy
                    };
                    let code_str = code.trim_end_matches(['\r', '\n']).to_string();
                    let block_idx = i;
                    let indent = depth * 16;
                    let p = SemanticPalette::from_context(cx);

                    div()
                        .w_full()
                        .when(indent > 0, |d| d.pl(px(indent as f32)))
                        .child(
                            div()
                                .relative()
                                .w_full()
                                .rounded(px(6.0))
                                .bg(p.surface_raised)
                                .border_1()
                                .border_color(p.border_subtle)
                                .overflow_hidden()
                                .child(
                                    div()
                                        .p(px(8.0))
                                        .pr(px(68.0))
                                        .font_family(mono_font_family(cx))
                                        .child(
                                            SimpleInput::new(state)
                                                .borderless(true)
                                                .text_size(ui_text_md(cx))
                                                .line_height(ui_text_md(cx) * 1.4)
                                        )
                                )
                                .child(
                                    h_flex()
                                        .absolute()
                                        .top(px(6.0))
                                        .right(px(6.0))
                                        .items_center()
                                        .gap(px(6.0))
                                        .when(!lang.is_empty() && lang != "text", |d| {
                                            d.child(
                                                div()
                                                    .text_size(px(10.5))
                                                    .text_color(rgb(t.text_muted))
                                                    .child(lang.to_string())
                                            )
                                        })
                                        .child(
                                            div()
                                                .id(SharedString::from(format!("code-copy-{}", i)))
                                                .cursor_pointer()
                                                .flex()
                                                .items_center()
                                                .gap(px(4.0))
                                                .px(px(6.0))
                                                .py(px(2.0))
                                                .rounded(px(4.0))
                                                .bg(p.surface_card)
                                                .border_1()
                                                .border_color(p.border_subtle)
                                                .hover(|s| s.bg(p.surface_hover))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    cx.write_to_clipboard(ClipboardItem::new_string(code_str.clone()));
                                                    this.copied_code_index = Some(block_idx);
                                                    cx.notify();
                                                    cx.spawn(async move |this: WeakEntity<Self>, cx| {
                                                        smol::Timer::after(std::time::Duration::from_secs(2)).await;
                                                        let _ = this.update(cx, |this, cx| {
                                                            if this.copied_code_index == Some(block_idx) {
                                                                this.copied_code_index = None;
                                                                cx.notify();
                                                            }
                                                        });
                                                    }).detach();
                                                }))
                                                .child(
                                                    copy_icon
                                                        .size(px(11.0))
                                                        .text_color(if is_copied { rgb(t.accent) } else { rgb(t.text_muted) })
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(10.5))
                                                        .text_color(if is_copied { rgb(t.accent) } else { rgb(t.text_muted) })
                                                        .child(copy_label)
                                                )
                                        )
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
                                        .borderless(true)
                                        .text_size(ui_text_md(cx))
                                        .line_height(ui_text_md(cx) * 1.5)
                                )
                        )
                        .into_any_element()
                }
                DocumentBlock::ListItem { ordered, depth, index, .. } => {
                    let marker_str = if let Some(idx) = index {
                        format!("{}.", idx)
                    } else if *ordered {
                        "1.".to_string()
                    } else {
                        "•".to_string()
                    };
                    let indent = depth * 16;
                    let marker_w = if *ordered || index.is_some() { px(20.0) } else { px(12.0) };
                    let lh = ui_text_md(cx) * 1.5;
                    h_flex()
                        .w_full()
                        .items_start()
                        .pl(px(indent as f32))
                        .child(
                            div()
                                .min_w(marker_w)
                                .mr(px(4.0))
                                .text_align(TextAlign::Right)
                                .text_size(ui_text_md(cx))
                                .line_height(lh)
                                .text_color(rgb(t.text_muted))
                                .child(marker_str)
                        )
                        .child(
                            div()
                                .flex_1()
                                .child(
                                    SimpleInput::new(state)
                                        .borderless(true)
                                        .text_size(ui_text_md(cx))
                                        .line_height(lh)
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
                    let p = SemanticPalette::from_context(cx);
                    v_flex()
                        .w_full()
                        .p(px(8.0))
                        .bg(p.surface_raised)
                        .border_1()
                        .border_color(p.border_subtle)
                        .rounded(px(4.0))
                        .child(
                            SimpleInput::new(state)
                                .borderless(true)
                                .text_size(ui_text_md(cx))
                        )
                        .into_any_element()
                }
                DocumentBlock::Table { .. } => {
                    let p = SemanticPalette::from_context(cx);
                    div()
                        .w_full()
                        .p(px(8.0))
                        .bg(p.surface_raised)
                        .border_1()
                        .border_color(p.border_subtle)
                        .rounded(px(4.0))
                        .font_family(mono_font_family(cx))
                        .child(
                            SimpleInput::new(state)
                                .borderless(true)
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
