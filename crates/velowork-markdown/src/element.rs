//! Unified GPUI Element for rendering Markdown documents with full text selection support.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gpui::*;
use gpui::prelude::*;
use velowork_core::theme::ThemeColors;
use velowork_i18n::i18n;
use velowork_ui::icon::AppIcon;
use velowork_ui::theme::theme;
use velowork_ui::syntax::highlight_text;
use velowork_ui::tokens::{mono_font_family, ui_text_md, ui_text_sm, ui_text_xl};
use velowork_ui::{h_flex, v_flex, SemanticPalette};

use crate::types::{Inline, Node};
use crate::MarkdownDocument;

/// Callback type for URL clicks: (url, window, cx).
pub type UrlClickCallback = Arc<dyn Fn(&str, &mut Window, &mut App) + Send + Sync + 'static>;

/// Callback type for Code Copy clicks: (code, block_index, window, cx).
pub type CodeCopyCallback = Arc<dyn Fn(&str, usize, &mut Window, &mut App) + Send + Sync + 'static>;

/// Selection event from mouse interaction on MarkdownElement.
#[derive(Clone, Debug)]
pub enum MarkdownSelectionEvent {
    /// Mouse down at character offset with click count (1 = single, 2 = double, 3 = triple),
    /// passing the document's plain text representation.
    Start {
        offset: usize,
        click_count: usize,
        plain_text: String,
    },
    /// Mouse dragged to character offset.
    Update {
        offset: usize,
    },
    /// Mouse released.
    End,
}

/// Callback type for selection events.
pub type SelectionEventCallback = Arc<dyn Fn(MarkdownSelectionEvent, &mut Window, &mut App) + Send + Sync + 'static>;

/// Hit test info for a single rendered text block / code line.
#[derive(Clone)]
struct BlockHitTest {
    char_start: usize,
    char_end: usize,
    layout: TextLayout,
}

/// 搜索关键词高亮配置。
#[derive(Clone, Debug)]
pub struct MarkdownSearchConfig {
    pub query: String,
    pub case_sensitive: bool,
    pub use_regex: bool,
    pub current_match: Option<usize>,
    pub start_flat_index: usize,
}

/// A unified GPUI element that renders a complete Markdown document without
/// fragmenting paragraphs into separate input entities, with full selection support.
#[derive(IntoElement)]
pub struct MarkdownElement {
    id: ElementId,
    document: MarkdownDocument,
    copied_code_block: Option<usize>,
    selection: Option<(usize, usize)>,
    search_config: Option<MarkdownSearchConfig>,
    on_url_click: Option<UrlClickCallback>,
    on_copy_code: Option<CodeCopyCallback>,
    on_selection_event: Option<SelectionEventCallback>,
}

impl MarkdownElement {
    /// Create a new MarkdownElement by parsing a markdown text string.
    pub fn new(id: impl Into<ElementId>, content: &str) -> Self {
        let doc = MarkdownDocument::parse(content);
        Self {
            id: id.into(),
            document: doc,
            copied_code_block: None,
            selection: None,
            search_config: None,
            on_url_click: None,
            on_copy_code: None,
            on_selection_event: None,
        }
    }

    /// Create from a pre-parsed `MarkdownDocument`.
    pub fn from_document(id: impl Into<ElementId>, document: MarkdownDocument) -> Self {
        Self {
            id: id.into(),
            document,
            copied_code_block: None,
            selection: None,
            search_config: None,
            on_url_click: None,
            on_copy_code: None,
            on_selection_event: None,
        }
    }

    /// Set the active copied code block index for showing "Copied" feedback.
    pub fn copied_code_block(mut self, block_idx: Option<usize>) -> Self {
        self.copied_code_block = block_idx;
        self
    }

    /// Set character-level selection range `(start_char, end_char)` for highlighting.
    pub fn selection(mut self, selection: Option<(usize, usize)>) -> Self {
        self.selection = selection;
        self
    }

    /// 配置搜索高亮。
    pub fn search(
        mut self,
        query: impl Into<String>,
        case_sensitive: bool,
        use_regex: bool,
        current_match: Option<usize>,
        start_flat_index: usize,
    ) -> Self {
        let q = query.into();
        if !q.trim().is_empty() {
            self.search_config = Some(MarkdownSearchConfig {
                query: q,
                case_sensitive,
                use_regex,
                current_match,
                start_flat_index,
            });
        }
        self
    }

    /// Set a custom URL click handler. If unset, opens via `cx.open_url`.
    pub fn on_url_click(
        mut self,
        handler: impl Fn(&str, &mut Window, &mut App) + Send + Sync + 'static,
    ) -> Self {
        self.on_url_click = Some(Arc::new(handler));
        self
    }

    /// Set a code copy handler to observe or override code block copying.
    pub fn on_copy_code(
        mut self,
        handler: impl Fn(&str, usize, &mut Window, &mut App) + Send + Sync + 'static,
    ) -> Self {
        self.on_copy_code = Some(Arc::new(handler));
        self
    }

    /// Set a selection event handler for mouse drag / double click / triple click selection.
    pub fn on_selection_event(
        mut self,
        handler: impl Fn(MarkdownSelectionEvent, &mut Window, &mut App) + Send + Sync + 'static,
    ) -> Self {
        self.on_selection_event = Some(Arc::new(handler));
        self
    }
}

struct RunningSearchState<'a> {
    config: &'a MarkdownSearchConfig,
    running_index: usize,
}

fn find_search_matches(
    text: &str,
    query: &str,
    case_sensitive: bool,
    use_regex: bool,
) -> Vec<Range<usize>> {
    if query.is_empty() || text.is_empty() {
        return Vec::new();
    }
    if use_regex {
        if let Ok(re) = regex::Regex::new(query) {
            re.find_iter(text).map(|m| m.range()).collect()
        } else {
            Vec::new()
        }
    } else {
        let text_lower = text.to_lowercase();
        let query_lower = query.to_lowercase();
        let (haystack, needle) = if case_sensitive {
            (text, query)
        } else {
            (text_lower.as_str(), query_lower.as_str())
        };
        let mut ranges = Vec::new();
        let mut start = 0;
        while start < haystack.len() {
            if let Some(rel) = haystack[start..].find(needle) {
                let idx = start + rel;
                let end = idx + needle.len();
                if text.is_char_boundary(idx) && text.is_char_boundary(end) {
                    ranges.push(idx..end);
                }
                start = end.max(start + 1);
            } else {
                break;
            }
        }
        ranges
    }
}

fn search_highlights_for_text(
    text: &str,
    state: &mut Option<RunningSearchState>,
    t: &ThemeColors,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let Some(state) = state.as_mut() else {
        return Vec::new();
    };

    let ranges = find_search_matches(
        text,
        &state.config.query,
        state.config.case_sensitive,
        state.config.use_regex,
    );

    let mut out = Vec::with_capacity(ranges.len());
    for range in ranges {
        let global_match_idx = state.running_index;
        state.running_index += 1;

        let is_current = state.config.current_match == Some(global_match_idx);
        let style = if is_current {
            HighlightStyle {
                background_color: Some(rgb(t.accent).into()),
                color: Some(rgb(0xffffff).into()),
                font_weight: Some(FontWeight::BOLD),
                ..Default::default()
            }
        } else {
            HighlightStyle {
                background_color: Some(rgb(t.accent).opacity(0.28).into()),
                ..Default::default()
            }
        };
        out.push((range, style));
    }

    out
}

impl RenderOnce for MarkdownElement {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = theme(cx);
        let p = SemanticPalette::from_context(cx);

        let mut children: Vec<AnyElement> = Vec::new();
        let doc = Rc::new(self.document);
        let nodes = doc.nodes();
        let hit_test_blocks: Rc<RefCell<Vec<BlockHitTest>>> = Rc::new(RefCell::new(Vec::new()));

        let mut search_state = self.search_config.as_ref().map(|c| RunningSearchState {
            config: c,
            running_index: c.start_flat_index,
        });

        for (node_idx, node) in nodes.iter().enumerate() {
            let offset = doc.node_offsets.get(node_idx).copied().unwrap_or(0);
            let node_len = MarkdownDocument::node_text_length(node);

            let node_selection = self.selection.and_then(|(start, end)| {
                let norm_start = start.min(end);
                let norm_end = start.max(end);
                if norm_end <= offset || norm_start >= offset + node_len {
                    None
                } else {
                    Some((
                        norm_start.saturating_sub(offset),
                        (norm_end - offset).min(node_len),
                    ))
                }
            });

            let el = match node {
                Node::Heading { level, children: inlines } => {
                    render_heading(
                        *level,
                        inlines,
                        &t,
                        cx,
                        self.on_url_click.clone(),
                        node_idx,
                        offset,
                        node_selection,
                        &mut search_state,
                        &hit_test_blocks,
                    )
                }
                Node::Paragraph { children: inlines } => {
                    render_paragraph(
                        inlines,
                        &t,
                        cx,
                        self.on_url_click.clone(),
                        node_idx,
                        offset,
                        node_selection,
                        &mut search_state,
                        &hit_test_blocks,
                    )
                }
                Node::CodeBlock { language, code } => {
                    let is_copied = self.copied_code_block == Some(node_idx);
                    render_code_block(
                        node_idx,
                        offset,
                        language.as_deref(),
                        code,
                        &t,
                        &p,
                        cx,
                        is_copied,
                        self.on_copy_code.clone(),
                        node_selection,
                        &mut search_state,
                        &hit_test_blocks,
                    )
                }
                Node::List { ordered, items } => {
                    render_list(
                        *ordered,
                        items,
                        &t,
                        cx,
                        self.on_url_click.clone(),
                        node_idx,
                        offset,
                        node_selection,
                        &mut search_state,
                        &hit_test_blocks,
                    )
                }
                Node::Blockquote { children: inlines } => {
                    render_blockquote(
                        inlines,
                        &t,
                        cx,
                        self.on_url_click.clone(),
                        node_idx,
                        offset,
                        node_selection,
                        &mut search_state,
                        &hit_test_blocks,
                    )
                }
                Node::Table { headers, rows, col_widths } => {
                    MarkdownDocument::render_table_with_selection(
                        headers,
                        rows,
                        col_widths,
                        &t,
                        cx,
                        node_selection,
                    )
                    .into_any_element()
                }
                Node::HorizontalRule => {
                    div()
                        .w_full()
                        .h(px(1.0))
                        .bg(p.border_subtle)
                        .my(px(6.0))
                        .into_any_element()
                }
                Node::Frontmatter { block, .. } => {
                    MarkdownDocument::render_frontmatter(block, &t, cx).into_any_element()
                }
            };
            children.push(el);
        }

        let total_chars = doc.plain_text.chars().count();
        let on_selection_event = self.on_selection_event;

        v_flex()
            .id(self.id)
            .w_full()
            .gap(px(8.0))
            .when(on_selection_event.is_some(), |el| el.cursor_text())
            .children(children)
            .when_some(on_selection_event, |el, cb| {
                let cb_down = cb.clone();
                let cb_move = cb.clone();
                let cb_up = cb;
                let ht_down = hit_test_blocks.clone();
                let ht_move = hit_test_blocks;
                let doc_down = doc.clone();

                el.on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, window, cx| {
                    let offset = hit_test(&ht_down.borrow(), event.position, total_chars);
                    cb_down(
                        MarkdownSelectionEvent::Start {
                            offset,
                            click_count: event.click_count,
                            plain_text: doc_down.plain_text.clone(),
                        },
                        window,
                        cx,
                    );
                })
                .on_mouse_move(move |event: &MouseMoveEvent, window, cx| {
                    if event.dragging() {
                        let offset = hit_test(&ht_move.borrow(), event.position, total_chars);
                        cb_move(MarkdownSelectionEvent::Update { offset }, window, cx);
                    }
                })
                .on_mouse_up(MouseButton::Left, move |_event: &MouseUpEvent, window, cx| {
                    cb_up(MarkdownSelectionEvent::End, window, cx);
                })
            })
    }
}

/// Convert byte index within `text` to character count.
fn char_count_for_byte_offset(text: &str, byte_ix: usize) -> usize {
    let safe_byte = byte_ix.min(text.len());
    let mut boundary = safe_byte;
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    text[..boundary].chars().count()
}

/// Hit test a mouse position against registered text block layouts to find character offset.
fn hit_test(blocks: &[BlockHitTest], position: Point<Pixels>, total_chars: usize) -> usize {
    if blocks.is_empty() {
        return 0;
    }

    let mut valid_blocks: Vec<(&BlockHitTest, Bounds<Pixels>)> = Vec::with_capacity(blocks.len());
    for block in blocks {
        let bounds = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| block.layout.bounds()));
        if let Ok(b) = bounds {
            valid_blocks.push((block, b));
        }
    }

    if valid_blocks.is_empty() {
        return 0;
    }

    if position.y < valid_blocks[0].1.top() {
        return 0;
    }

    let last = &valid_blocks[valid_blocks.len() - 1];
    if position.y > last.1.bottom() {
        return total_chars;
    }

    for (i, (block, bounds)) in valid_blocks.iter().enumerate() {
        if position.y >= bounds.top() && position.y <= bounds.bottom() {
            let byte_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                block.layout.index_for_position(position)
            }));
            let byte_ix = match byte_res {
                Ok(Ok(ix)) | Ok(Err(ix)) => ix,
                Err(_) => 0,
            };
            let text_res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                block.layout.text()
            }));
            let text = text_res.unwrap_or_default();
            let char_offset_in_block = char_count_for_byte_offset(&text, byte_ix);
            return (block.char_start + char_offset_in_block).min(block.char_end);
        }

        if i + 1 < valid_blocks.len() {
            let next_bounds = &valid_blocks[i + 1].1;
            if position.y > bounds.bottom() && position.y < next_bounds.top() {
                let mid_y = (bounds.bottom() + next_bounds.top()) * 0.5;
                if position.y < mid_y {
                    return block.char_end;
                } else {
                    return valid_blocks[i + 1].0.char_start;
                }
            }
        }
    }

    let mut closest_offset = 0;
    let mut min_dist = px(f32::MAX);
    for (block, bounds) in &valid_blocks {
        let dist = if position.y < bounds.top() {
            bounds.top() - position.y
        } else if position.y > bounds.bottom() {
            position.y - bounds.bottom()
        } else {
            px(0.0)
        };
        if dist < min_dist {
            min_dist = dist;
            closest_offset = block.char_start;
        }
    }
    closest_offset
}



/// Recursively collect links and their byte ranges within inlines.
fn collect_inline_links(
    inlines: &[Inline],
    text: &str,
    char_offset: &mut usize,
    out: &mut Vec<(Range<usize>, String)>,
) {
    for inline in inlines {
        match inline {
            Inline::Text(s) => *char_offset += s.chars().count(),
            Inline::Code(c) => *char_offset += c.chars().count(),
            Inline::Bold(children) | Inline::Italic(children) | Inline::Strikethrough(children) => {
                collect_inline_links(children, text, char_offset, out);
            }
            Inline::Link { url, children } => {
                let start = *char_offset;
                collect_inline_links(children, text, char_offset, out);
                let end = *char_offset;
                let byte_start = MarkdownDocument::char_to_byte(text, start);
                let byte_end = MarkdownDocument::char_to_byte(text, end);
                out.push((byte_start..byte_end, url.clone()));
            }
        }
    }
}

/// Render a styled paragraph with optional interactive links.
#[allow(clippy::too_many_arguments)]
fn render_paragraph(
    inlines: &[Inline],
    t: &ThemeColors,
    cx: &App,
    on_url_click: Option<UrlClickCallback>,
    node_idx: usize,
    node_offset: usize,
    selection: Option<(usize, usize)>,
    search_state: &mut Option<RunningSearchState>,
    hit_tests: &Rc<RefCell<Vec<BlockHitTest>>>,
) -> AnyElement {
    let base = TextStyle {
        color: rgb(t.text_primary).into(),
        font_size: AbsoluteLength::from(ui_text_md(cx)),
        line_height: DefiniteLength::from(ui_text_md(cx) * 1.5),
        ..Default::default()
    };

    let flat_text = MarkdownDocument::render_inlines_as_text(inlines);
    let mut links = Vec::new();
    let mut char_offset = 0usize;
    collect_inline_links(inlines, &flat_text, &mut char_offset, &mut links);

    let search_hl = search_highlights_for_text(&flat_text, search_state, t);
    let styled = MarkdownDocument::build_inline_styled(inlines, &base, t, cx, selection, &search_hl);

    let inlines_char_len = flat_text.chars().count();
    hit_tests.borrow_mut().push(BlockHitTest {
        char_start: node_offset,
        char_end: node_offset + inlines_char_len,
        layout: styled.layout().clone(),
    });

    if links.is_empty() {
        div().w_full().child(styled).into_any_element()
    } else {
        let (ranges, urls): (Vec<_>, Vec<_>) = links.into_iter().unzip();
        InteractiveText::new(ElementId::from(format!("md-p-{}", node_idx)), styled)
            .on_click(ranges, move |ix, window, cx| {
                if let Some(url) = urls.get(ix) {
                    if let Some(ref handler) = on_url_click {
                        handler(url, window, cx);
                    } else {
                        cx.open_url(url);
                    }
                }
            })
            .into_any_element()
    }
}

/// Render a heading block (H1-H6).
#[allow(clippy::too_many_arguments)]
fn render_heading(
    level: u8,
    inlines: &[Inline],
    t: &ThemeColors,
    cx: &App,
    on_url_click: Option<UrlClickCallback>,
    node_idx: usize,
    node_offset: usize,
    selection: Option<(usize, usize)>,
    search_state: &mut Option<RunningSearchState>,
    hit_tests: &Rc<RefCell<Vec<BlockHitTest>>>,
) -> AnyElement {
    let (size, weight) = match level {
        1 => (ui_text_xl(cx) * 1.2, FontWeight::BOLD),
        2 => (ui_text_xl(cx), FontWeight::BOLD),
        3 => (ui_text_md(cx) * 1.15, FontWeight::SEMIBOLD),
        4 => (ui_text_md(cx) * 1.05, FontWeight::SEMIBOLD),
        _ => (ui_text_md(cx), FontWeight::MEDIUM),
    };

    let base = TextStyle {
        color: rgb(t.text_primary).into(),
        font_size: AbsoluteLength::from(size),
        font_weight: weight,
        line_height: DefiniteLength::from(size * 1.35),
        ..Default::default()
    };

    let flat_text = MarkdownDocument::render_inlines_as_text(inlines);
    let mut links = Vec::new();
    let mut char_offset = 0usize;
    collect_inline_links(inlines, &flat_text, &mut char_offset, &mut links);

    let search_hl = search_highlights_for_text(&flat_text, search_state, t);
    let styled = MarkdownDocument::build_inline_styled(inlines, &base, t, cx, selection, &search_hl);

    let inlines_char_len = flat_text.chars().count();
    hit_tests.borrow_mut().push(BlockHitTest {
        char_start: node_offset,
        char_end: node_offset + inlines_char_len,
        layout: styled.layout().clone(),
    });

    let heading_content = if links.is_empty() {
        styled.into_any_element()
    } else {
        let (ranges, urls): (Vec<_>, Vec<_>) = links.into_iter().unzip();
        InteractiveText::new(ElementId::from(format!("md-h-{}", node_idx)), styled)
            .on_click(ranges, move |ix, window, cx| {
                if let Some(url) = urls.get(ix) {
                    if let Some(ref handler) = on_url_click {
                        handler(url, window, cx);
                    } else {
                        cx.open_url(url);
                    }
                }
            })
            .into_any_element()
    };

    div()
        .w_full()
        .pt(if level <= 2 { px(6.0) } else { px(3.0) })
        .pb(px(2.0))
        .when(level <= 2, |d| {
            d.border_b_1()
                .border_color(rgb(t.border).opacity(0.5))
                .pb(px(4.0))
        })
        .child(heading_content)
        .into_any_element()
}

/// Render an ordered or unordered list.
#[allow(clippy::too_many_arguments)]
fn render_list(
    ordered: bool,
    items: &[Vec<Inline>],
    t: &ThemeColors,
    cx: &App,
    on_url_click: Option<UrlClickCallback>,
    node_idx: usize,
    node_offset: usize,
    selection: Option<(usize, usize)>,
    search_state: &mut Option<RunningSearchState>,
    hit_tests: &Rc<RefCell<Vec<BlockHitTest>>>,
) -> AnyElement {
    let base = TextStyle {
        color: rgb(t.text_primary).into(),
        font_size: AbsoluteLength::from(ui_text_md(cx)),
        line_height: DefiniteLength::from(ui_text_md(cx) * 1.45),
        ..Default::default()
    };

    let mut list_items = Vec::new();
    let mut item_offset = node_offset;
    let mut visible_count = 0usize;

    for (item_idx, inlines) in items.iter().enumerate() {
        let flat_text = MarkdownDocument::render_inlines_as_text(inlines);
        let item_char_len = flat_text.chars().count();
        if flat_text.trim().is_empty() {
            item_offset += item_char_len + 1;
            continue;
        }
        visible_count += 1;

        let item_end = item_offset + item_char_len;

        let item_sel = selection.and_then(|(s, e)| {
            let rel_offset = item_offset - node_offset;
            let rel_end = rel_offset + item_char_len;
            if e <= rel_offset || s >= rel_end {
                None
            } else {
                Some((
                    s.saturating_sub(rel_offset),
                    (e - rel_offset).min(item_char_len),
                ))
            }
        });

        let mut links = Vec::new();
        let mut char_offset = 0usize;
        collect_inline_links(inlines, &flat_text, &mut char_offset, &mut links);

        let search_hl = search_highlights_for_text(&flat_text, search_state, t);
        let styled = MarkdownDocument::build_inline_styled(inlines, &base, t, cx, item_sel, &search_hl);

        hit_tests.borrow_mut().push(BlockHitTest {
            char_start: item_offset,
            char_end: item_end,
            layout: styled.layout().clone(),
        });

        let item_content = if links.is_empty() {
            styled.into_any_element()
        } else {
            let (ranges, urls): (Vec<_>, Vec<_>) = links.into_iter().unzip();
            InteractiveText::new(
                ElementId::from(format!("md-li-{}-{}", node_idx, item_idx)),
                styled,
            )
            .on_click(ranges, {
                let on_url_click = on_url_click.clone();
                move |ix, window, cx| {
                    if let Some(url) = urls.get(ix) {
                        if let Some(ref handler) = on_url_click {
                            handler(url, window, cx);
                        } else {
                            cx.open_url(url);
                        }
                    }
                }
            })
            .into_any_element()
        };

        let bullet_str = if ordered {
            format!("{}.", visible_count)
        } else {
            "•".to_string()
        };

        list_items.push(
            h_flex()
                .w_full()
                .items_start()
                .gap(px(6.0))
                .child(
                    div()
                        .w(px(16.0))
                        .flex_shrink_0()
                        .text_color(rgb(t.text_muted))
                        .text_size(ui_text_md(cx))
                        .font_weight(if ordered {
                            FontWeight::MEDIUM
                        } else {
                            FontWeight::BOLD
                        })
                        .child(bullet_str),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .w_full()
                        .child(item_content),
                ),
        );

        item_offset += item_char_len + 1; // +1 for '\n'
    }

    if list_items.is_empty() {
        return div().into_any_element();
    }

    v_flex()
        .w_full()
        .gap(px(4.0))
        .children(list_items)
        .into_any_element()
}

/// Render a blockquote with left border accent.
#[allow(clippy::too_many_arguments)]
fn render_blockquote(
    inlines: &[Inline],
    t: &ThemeColors,
    cx: &App,
    on_url_click: Option<UrlClickCallback>,
    node_idx: usize,
    node_offset: usize,
    selection: Option<(usize, usize)>,
    search_state: &mut Option<RunningSearchState>,
    hit_tests: &Rc<RefCell<Vec<BlockHitTest>>>,
) -> AnyElement {
    let base = TextStyle {
        color: rgb(t.text_secondary).into(),
        font_size: AbsoluteLength::from(ui_text_md(cx)),
        font_style: FontStyle::Italic,
        line_height: DefiniteLength::from(ui_text_md(cx) * 1.5),
        ..Default::default()
    };

    let flat_text = MarkdownDocument::render_inlines_as_text(inlines);
    let inlines_char_len = flat_text.chars().count();
    let mut links = Vec::new();
    let mut char_offset = 0usize;
    collect_inline_links(inlines, &flat_text, &mut char_offset, &mut links);

    let search_hl = search_highlights_for_text(&flat_text, search_state, t);
    let styled = MarkdownDocument::build_inline_styled(inlines, &base, t, cx, selection, &search_hl);

    hit_tests.borrow_mut().push(BlockHitTest {
        char_start: node_offset,
        char_end: node_offset + inlines_char_len,
        layout: styled.layout().clone(),
    });

    let quote_content = if links.is_empty() {
        styled.into_any_element()
    } else {
        let (ranges, urls): (Vec<_>, Vec<_>) = links.into_iter().unzip();
        InteractiveText::new(ElementId::from(format!("md-bq-{}", node_idx)), styled)
            .on_click(ranges, move |ix, window, cx| {
                if let Some(url) = urls.get(ix) {
                    if let Some(ref handler) = on_url_click {
                        handler(url, window, cx);
                    } else {
                        cx.open_url(url);
                    }
                }
            })
            .into_any_element()
    };

    div()
        .w_full()
        .pl(px(10.0))
        .border_l_2()
        .border_color(rgb(t.accent).opacity(0.7))
        .child(quote_content)
        .into_any_element()
}

/// Render a first-class code block card with language header and copy button.
#[allow(clippy::too_many_arguments)]
fn render_code_block(
    block_idx: usize,
    node_offset: usize,
    language: Option<&str>,
    code: &str,
    t: &ThemeColors,
    p: &SemanticPalette,
    cx: &App,
    is_copied: bool,
    on_copy_code: Option<CodeCopyCallback>,
    selection: Option<(usize, usize)>,
    search_state: &mut Option<RunningSearchState>,
    hit_tests: &Rc<RefCell<Vec<BlockHitTest>>>,
) -> AnyElement {
    let normalized_code = code.replace("\r\n", "\n").replace('\r', "\n");
    let raw_code = normalized_code.trim_end_matches('\n').to_string();
    let lang = language.unwrap_or("").trim().to_lowercase();
    let display_lang = if !lang.is_empty() && lang != "text" {
        lang.to_uppercase()
    } else {
        "CODE".to_string()
    };

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

    let copy_action = {
        let code_clone = raw_code.clone();
        let on_copy_code = on_copy_code.clone();
        move |_event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
            cx.write_to_clipboard(ClipboardItem::new_string(code_clone.clone()));
            if let Some(ref handler) = on_copy_code {
                handler(&code_clone, block_idx, window, cx);
            }
            cx.stop_propagation();
        }
    };

    // Syntax highlighted code lines using syntect via velowork_ui::syntax
    let spans = highlight_text(&raw_code, &lang, t.is_dark());

    // Split spans by lines for clean multiline rendering
    let mut lines: Vec<Vec<(String, Rgba)>> = Vec::new();
    let mut current_line = Vec::new();
    for span in spans {
        let parts: Vec<&str> = span.text.split('\n').collect();
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                lines.push(std::mem::take(&mut current_line));
            }
            if !part.is_empty() {
                current_line.push((part.to_string(), span.color));
            }
        }
    }
    if !current_line.is_empty() || lines.is_empty() {
        lines.push(current_line);
    }

    let mut code_lines_div: Vec<AnyElement> = Vec::with_capacity(lines.len());
    let mut line_offset_in_block = 0usize;

    for line_spans in lines {
        let line_text: String = line_spans.iter().map(|(txt, _)| txt.as_str()).collect();
        let line_char_len = line_text.chars().count();
        let line_global_start = node_offset + line_offset_in_block;
        let line_global_end = line_global_start + line_char_len;

        let line_sel = selection.and_then(|(sel_start, sel_end)| {
            if sel_end <= line_offset_in_block || sel_start >= line_offset_in_block + line_char_len {
                None
            } else {
                Some((
                    sel_start.saturating_sub(line_offset_in_block),
                    (sel_end - line_offset_in_block).min(line_char_len),
                ))
            }
        });

        let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
        let mut byte_pos = 0usize;
        for (span_str, color) in &line_spans {
            let span_byte_len = span_str.len();
            highlights.push((
                byte_pos..byte_pos + span_byte_len,
                HighlightStyle {
                    color: Some((*color).into()),
                    ..Default::default()
                },

            ));
            byte_pos += span_byte_len;
        }

        let search_hl = search_highlights_for_text(&line_text, search_state, t);
        highlights.extend(search_hl);

        let sel_byte_range = line_sel.and_then(|(s_char, e_char)| {
            let s_byte = MarkdownDocument::char_to_byte(&line_text, s_char);
            let e_byte = MarkdownDocument::char_to_byte(&line_text, e_char);
            if e_byte > s_byte {
                Some(s_byte..e_byte)
            } else {
                None
            }
        });

        let selection_style = HighlightStyle {
            background_color: Some(rgba(0x3390ff40).into()),
            ..Default::default()
        };

        let base_style = TextStyle {
            font_family: mono_font_family(cx).into(),
            font_size: AbsoluteLength::from(ui_text_sm(cx)),
            line_height: DefiniteLength::from(ui_text_sm(cx) * 1.4),
            color: rgb(t.text_primary).into(),
            ..Default::default()
        };

        let display_text = if line_text.is_empty() {
            " ".to_string()
        } else {
            line_text
        };

        let highlights = crate::selection::merge_highlights(
            &display_text,
            &highlights,
            sel_byte_range,
            selection_style,
        );

        let styled = StyledText::new(display_text).with_default_highlights(&base_style, highlights);

        hit_tests.borrow_mut().push(BlockHitTest {
            char_start: line_global_start,
            char_end: line_global_end,
            layout: styled.layout().clone(),
        });

        code_lines_div.push(
            div()
                .w_full()
                .min_w(px(0.0))
                .whitespace_normal()
                .child(styled)
                .into_any_element(),
        );

        line_offset_in_block += line_char_len + 1; // +1 for '\n'
    }

    div()
        .w_full()
        .min_w(px(0.0))
        .my(px(4.0))
        .rounded(px(6.0))
        .bg(p.surface_raised)
        .border_1()
        .border_color(p.border_subtle)
        .overflow_hidden()
        // Header bar
        .child(
            h_flex()
                .h(px(28.0))
                .px(px(10.0))
                .bg(p.surface_header)
                .border_b_1()
                .border_color(p.border_subtle)
                .justify_between()
                .items_center()
                .child(
                    div()
                        .font_family(mono_font_family(cx))
                        .text_size(px(11.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(t.text_muted))
                        .child(display_lang),
                )
                .child(
                    h_flex()
                        .id(ElementId::from(format!("copy-code-{}", block_idx)))
                        .cursor_pointer()
                        .items_center()
                        .gap(px(4.0))
                        .px(px(6.0))
                        .py(px(2.0))
                        .rounded(px(4.0))
                        .bg(p.surface_card)
                        .border_1()
                        .border_color(p.border_subtle)
                        .hover(|s| s.bg(p.surface_hover))
                        .on_mouse_down(MouseButton::Left, copy_action)
                        .child(copy_icon.size(px(11.0)).text_color(rgb(t.text_muted)))
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(rgb(t.text_secondary))
                                .child(copy_label),
                        ),
                ),
        )
        // Code content body
        .child(
            div()
                .id(ElementId::from(format!("code-body-{}", block_idx)))
                .w_full()
                .min_w(px(0.0))
                .p(px(10.0))
                .font_family(mono_font_family(cx))
                .text_size(ui_text_sm(cx))
                .children(code_lines_div),
        )
        .into_any_element()
}


#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn test_markdown_element_creation_and_node_extraction() {
        let md = r#"# Heading 1
Here is **bold** and *italic* text and `code` chip.
- Item A
- Item B

1. First
2. Second

```bash
sudo du -xhd 1 / | sort -hr
```

> A quote block
"#;
        let el = MarkdownElement::new("test-md", md);
        assert_eq!(el.document.nodes().len(), 6);

        match &el.document.nodes()[0] {
            Node::Heading { level, .. } => assert_eq!(*level, 1),
            _ => panic!("Expected Heading"),
        }
        match &el.document.nodes()[1] {
            Node::Paragraph { children } => {
                assert!(children.iter().any(|c| matches!(c, Inline::Bold(_))));
                assert!(children.iter().any(|c| matches!(c, Inline::Italic(_))));
                assert!(children.iter().any(|c| matches!(c, Inline::Code(_))));
            }
            _ => panic!("Expected Paragraph"),
        }
        match &el.document.nodes()[2] {
            Node::List { ordered, items } => {
                assert!(!*ordered);
                assert_eq!(items.len(), 2);
            }
            _ => panic!("Expected unordered List"),
        }
        match &el.document.nodes()[3] {
            Node::List { ordered, items } => {
                assert!(*ordered);
                assert_eq!(items.len(), 2);
            }
            _ => panic!("Expected ordered List"),
        }
        match &el.document.nodes()[4] {
            Node::CodeBlock { language, code } => {
                assert_eq!(language.as_deref(), Some("bash"));
                assert_eq!(code.trim(), "sudo du -xhd 1 / | sort -hr");
            }
            _ => panic!("Expected CodeBlock"),
        }
        match &el.document.nodes()[5] {
            Node::Blockquote { .. } => {}
            _ => panic!("Expected Blockquote"),
        }
    }

    #[test]
    fn test_slice_plain_text() {
        let doc = MarkdownDocument::parse("# Heading\n\nParagraph text.");
        let sliced = doc.slice_plain_text(0, 7);
        assert_eq!(sliced, "Heading");
    }

    #[test]
    fn test_ini_code_block_parsing() {
        let md = r#"针对 Qt5 (~/.config/qt5ct/qt5ct.conf，如果使用 qt5ct 桥接)：
```ini
[Appearance]
style=kvantum
icon_theme=breeze-dark
standard_dialogs=default
```
"#;
        let el = MarkdownElement::new("test-ini", md);
        assert_eq!(el.document.nodes().len(), 2);
        match &el.document.nodes()[1] {
            Node::CodeBlock { language, code } => {
                assert_eq!(language.as_deref(), Some("ini"));
                assert!(code.contains("[Appearance]"));
                assert!(code.contains("style=kvantum"));
                assert!(code.contains("standard_dialogs=default"));
            }
            _ => panic!("Expected CodeBlock"),
        }
    }

    #[test]
    fn test_unclosed_ini_code_block_parsing() {
        let md = "针对 Qt5 (~/.config/qt5ct/qt5ct.conf，如果使用 qt5ct 桥接)：\n```ini\n[Appearance]\nstyle=kvantum\nicon_theme=breeze-dark";
        let el = MarkdownElement::new("test-unclosed-ini", md);
        assert_eq!(el.document.nodes().len(), 2);
        match &el.document.nodes()[1] {
            Node::CodeBlock { language, code } => {
                assert_eq!(language.as_deref(), Some("ini"));
                assert!(code.contains("[Appearance]"));
                assert!(code.contains("style=kvantum"));
            }
            _ => panic!("Expected CodeBlock for unclosed fence"),
        }
    }
}


