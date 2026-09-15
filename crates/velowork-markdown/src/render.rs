//! Rendering logic for markdown nodes and inline elements.

use std::ops::Range;

use velowork_core::theme::ThemeColors;
use velowork_ui::code_block::code_block_container;
use velowork_ui::tokens::{mono_font_family, ui_text_md, ui_text_sm, ui_text_xl};
use gpui::*;
use gpui::prelude::FluentBuilder;
use velowork_ui::{h_flex, v_flex};

use super::types::{
    char_len, slice_by_chars, FmValue, Frontmatter, Inline, Node,
};
use super::{MarkdownDocument, RenderedNode};

impl MarkdownDocument {
    /// Number of top-level blocks in the document. Each maps to one list item.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Render a single top-level node by index, ready for the caller to wrap
    /// with mouse handlers. Code blocks/tables are returned with their
    /// individual selectable lines/rows. Returns None if `idx` is out of range.
    pub fn render_node(
        &self,
        idx: usize,
        t: &ThemeColors,
        cx: &App,
        selection: Option<(usize, usize)>,
    ) -> Option<RenderedNode> {
        let node = self.nodes.get(idx)?;
        let offset = self.node_offsets.get(idx).copied().unwrap_or(0);
        let node_len = Self::node_text_length(node);
        let node_selection = selection.and_then(|(start, end)| {
            if end <= offset || start >= offset + node_len {
                None
            } else {
                Some((
                    start.saturating_sub(offset),
                    (end - offset).min(node_len),
                ))
            }
        });

        let rendered = match node {
                Node::CodeBlock { language, code } => {
                    // Return code blocks with individual lines for per-line selection
                    let selection_bg = rgba(0x3390ff40);
                    let mut lines = Vec::new();
                    let mut line_offset = offset;

                    for line in code.lines() {
                        let line_len = char_len(line);
                        let line_end = line_offset + line_len + 1; // +1 for newline

                        let line_sel = node_selection.and_then(|(s, e)| {
                            let rel_offset = line_offset - offset;
                            let rel_end = rel_offset + line_len + 1;
                            if e <= rel_offset || s >= rel_end {
                                None
                            } else {
                                Some((
                                    s.saturating_sub(rel_offset),
                                    (e - rel_offset).min(line_len),
                                ))
                            }
                        });

                        let line_div = if let Some((sel_start, sel_end)) = line_sel {
                            let (before, selected, after) = slice_by_chars(line, sel_start, sel_end);
                            div()
                                .h(px(18.0))
                                .flex()
                                .child(div().child(before))
                                .child(div().bg(selection_bg).child(selected))
                                .child(div().child(after))
                        } else {
                            div()
                                .h(px(18.0))
                                .child(if line.is_empty() { " ".to_string() } else { line.to_string() })
                        };

                        lines.push((line_div, line_offset, line_end));
                        line_offset = line_end;
                    }

                    RenderedNode::CodeBlock {
                        language: language.clone(),
                        lines,
                    }
                }
                Node::Table { headers, rows, col_widths } => {
                    // Return tables with individual rows for per-row selection.
                    // Column widths are precomputed at parse time.
                    let mut row_offset = offset;
                    let mut rendered_rows = Vec::new();
                    let mut rendered_header = None;

                    // Header row
                    if !headers.is_empty() {
                        let header_len: usize = headers.iter().map(|h| Self::inlines_text_length(h)).sum::<usize>()
                            + headers.len().saturating_sub(1) + 1; // tabs + newline
                        let header_end = row_offset + header_len;

                        let header_sel = node_selection.and_then(|(s, e)| {
                            let rel_start = row_offset - offset;
                            let rel_end = rel_start + header_len;
                            if e <= rel_start || s >= rel_end {
                                None
                            } else {
                                Some((s.saturating_sub(rel_start), (e - rel_start).min(header_len)))
                            }
                        });

                        let mut header_row = h_flex();
                        let mut cell_offset = 0usize;
                        for (i, header) in headers.iter().enumerate() {
                            let cell_len = Self::inlines_text_length(header) + if i > 0 { 1 } else { 0 };
                            let cell_sel = header_sel.and_then(|(s, e)| {
                                let cell_start = cell_offset + if i > 0 { 1 } else { 0 };
                                let cell_end = cell_offset + cell_len;
                                if e <= cell_start || s >= cell_end {
                                    None
                                } else {
                                    Some((s.saturating_sub(cell_start), (e - cell_start).min(Self::inlines_text_length(header))))
                                }
                            });

                            let width = col_widths.get(i).copied().unwrap_or(10);
                            let min_w = ((width * 8) + 24).max(80) as f32;
                            header_row = header_row.child(
                                div()
                                    .min_w(px(min_w))
                                    .px(px(12.0))
                                    .py(px(8.0))
                                    .child(Self::render_inlines_with_selection(
                                        header,
                                        &{
                                            let mut b =
                                                Self::inline_base(t.text_primary, ui_text_md(cx), px(18.0), false);
                                            b.font_weight = FontWeight::SEMIBOLD;
                                            b
                                        },
                                        t,
                                        cx,
                                        cell_sel,
                                    ))
                            );
                            cell_offset += cell_len;
                        }

                        let header_div = header_row.bg(rgb(t.bg_header)).border_b_1().border_color(rgb(t.border));
                        rendered_header = Some((header_div, row_offset, header_end));
                        row_offset = header_end;
                    }

                    // Data rows
                    for (row_idx, row) in rows.iter().enumerate() {
                        let row_len: usize = row.iter().map(|cell| Self::inlines_text_length(cell)).sum::<usize>()
                            + row.len().saturating_sub(1) + 1; // tabs + newline
                        let row_end = row_offset + row_len;

                        let row_sel = node_selection.and_then(|(s, e)| {
                            let rel_start = row_offset - offset;
                            let rel_end = rel_start + row_len;
                            if e <= rel_start || s >= rel_end {
                                None
                            } else {
                                Some((s.saturating_sub(rel_start), (e - rel_start).min(row_len)))
                            }
                        });

                        let mut row_div = h_flex();
                        if row_idx % 2 == 1 {
                            row_div = row_div.bg(rgb(t.bg_secondary));
                        }
                        if row_idx < rows.len() - 1 {
                            row_div = row_div.border_b_1().border_color(rgb(t.border));
                        }

                        let mut cell_offset = 0usize;
                        for (i, cell) in row.iter().enumerate() {
                            let cell_len = Self::inlines_text_length(cell) + if i > 0 { 1 } else { 0 };
                            let cell_sel = row_sel.and_then(|(s, e)| {
                                let cell_start = cell_offset + if i > 0 { 1 } else { 0 };
                                let cell_end = cell_offset + cell_len;
                                if e <= cell_start || s >= cell_end {
                                    None
                                } else {
                                    Some((s.saturating_sub(cell_start), (e - cell_start).min(Self::inlines_text_length(cell))))
                                }
                            });

                            let width = col_widths.get(i).copied().unwrap_or(10);
                            let min_w = ((width * 8) + 24).max(80) as f32;
                            row_div = row_div.child(
                                div()
                                    .min_w(px(min_w))
                                    .px(px(12.0))
                                    .py(px(6.0))
                                    .child(Self::render_inlines_with_selection(
                                        cell,
                                        &Self::inline_base(t.text_secondary, ui_text_md(cx), px(18.0), false),
                                        t,
                                        cx,
                                        cell_sel,
                                    ))
                            );
                            cell_offset += cell_len;
                        }

                        rendered_rows.push((row_div, row_offset, row_end));
                        row_offset = row_end;
                    }

                    RenderedNode::Table {
                        header: rendered_header,
                        rows: rendered_rows,
                    }
                }
                _ => {
                    // Other nodes are simple blocks
                    let node_div = Self::render_node_with_selection(node, t, cx, node_selection);
                    RenderedNode::Simple {
                        div: node_div,
                        start_offset: offset,
                        end_offset: offset + node_len,
                    }
                }
            };

        Some(rendered)
    }

    /// Render the entire document as a vertical flow of blocks, intended for
    /// embedding inside a chat bubble.
    ///
    /// All styling is driven by [`ThemeColors`] (no hardcoded colors), so the
    /// output tracks the active theme. Code blocks are wrapped in a
    /// horizontally-scrollable container so long lines never clip, while prose
    /// blocks wrap responsively to the bubble width.
    pub fn render_flow(&self, t: &ThemeColors, cx: &App) -> AnyElement {
        let mut blocks: Vec<AnyElement> = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let block = match node {
                Node::CodeBlock { language, code } => {
                    Self::render_code_block_flow(language, code, t, cx)
                }
                Node::Table { headers, rows, col_widths } => Self::render_table_with_selection(
                    headers,
                    rows,
                    col_widths,
                    t,
                    cx,
                    None,
                )
                .into_any_element(),
                _ => Self::render_node_with_selection(node, t, cx, None).into_any_element(),
            };
            blocks.push(block);
        }
        v_flex()
            .gap(px(8.0))
            .children(blocks)
            .into_any_element()
    }

    /// Render a fenced code block with a language label header and a body that
    /// scrolls horizontally when a line is longer than the available width.
    fn render_code_block_flow(
        language: &Option<String>,
        code: &str,
        t: &ThemeColors,
        cx: &App,
    ) -> AnyElement {
        let lang_label = language.clone().unwrap_or_default();
        v_flex()
            .rounded(px(6.0))
            .bg(rgb(t.bg_primary))
            .border_1()
            .border_color(rgb(t.border))
            .overflow_hidden()
            .font_family(mono_font_family(cx))
            .when(!lang_label.is_empty(), |d| {
                d.child(
                    div()
                        .px(px(12.0))
                        .py(px(6.0))
                        .bg(rgb(t.bg_header))
                        .border_b_1()
                        .border_color(rgb(t.border))
                        .text_size(ui_text_sm(cx))
                        .text_color(rgb(t.text_muted))
                        .child(lang_label),
                )
            })
            .child(
                div()
                    .p(px(12.0))
                    .text_size(ui_text_md(cx))
                    .text_color(rgb(t.text_secondary))
                    .line_height(px(18.0))
                    .children(code.lines().map(|line| {
                        div().child(if line.is_empty() {
                            " ".to_string()
                        } else {
                            line.to_string()
                        })
                    })),
            )
            .into_any_element()
    }

    /// Calculate the text length of a node (for selection offset tracking, in characters).
    pub(crate) fn node_text_length(node: &Node) -> usize {
        match node {
            Node::Heading { level: _, children } |
            Node::Paragraph { children } |
            Node::Blockquote { children } => {
                Self::inlines_text_length(children) + 1 // +1 for newline
            }
            Node::CodeBlock { code, .. } => {
                // Sum of character lengths of each line + 1 newline per line
                code.lines().map(|line| char_len(line) + 1).sum::<usize>().max(1)
            }
            Node::List { items, .. } => {
                items.iter().map(|item| Self::inlines_text_length(item) + 1).sum()
            }
            Node::Table { headers, rows, .. } => {
                let header_len: usize = headers.iter().map(|h| Self::inlines_text_length(h)).sum::<usize>()
                    + headers.len().saturating_sub(1) // tabs
                    + 1; // newline
                let rows_len: usize = rows.iter().map(|row| {
                    row.iter().map(|cell| Self::inlines_text_length(cell)).sum::<usize>()
                        + row.len().saturating_sub(1) // tabs
                        + 1 // newline
                }).sum();
                header_len + rows_len
            }
            Node::HorizontalRule => 1, // newline
            Node::Frontmatter { text_len, .. } => *text_len,
        }
    }

    /// Calculate the text length of inline elements (in characters, not bytes).
    pub(crate) fn inlines_text_length(inlines: &[Inline]) -> usize {
        inlines.iter().map(|inline| {
            match inline {
                Inline::Text(t) => char_len(t),
                Inline::Code(c) => char_len(c),
                Inline::Bold(children) | Inline::Italic(children) | Inline::Strikethrough(children) => {
                    Self::inlines_text_length(children)
                }
                Inline::Link { children, .. } => {
                    Self::inlines_text_length(children)
                }
            }
        }).sum()
    }

    /// Render a node with selection highlighting.
    fn render_node_with_selection(node: &Node, t: &ThemeColors, cx: &App, selection: Option<(usize, usize)>) -> Div {
        match node {
            Node::Heading { level, children } => {
                let (size, weight) = match level {
                    1 => (px(28.0), FontWeight::BOLD),
                    2 => (px(24.0), FontWeight::BOLD),
                    3 => (px(20.0), FontWeight::SEMIBOLD),
                    4 => (px(18.0), FontWeight::SEMIBOLD),
                    5 => (px(16.0), FontWeight::MEDIUM),
                    _ => (px(14.0), FontWeight::MEDIUM),
                };

                // Headings keep the heading size/weight/color; selection is
                // applied as a background range over the inline text.
                let mut base = Self::inline_base(t.text_primary, size, px(22.0), false);
                base.font_weight = weight;
                let content =
                    Self::render_inlines_with_selection(children, &base, t, cx, selection);

                div()
                    .pb(px(4.0))
                    .when(*level <= 2, |d| {
                        d.border_b_1()
                            .border_color(rgb(t.border))
                            .mb(px(4.0))
                    })
                    .child(content)
            }
            Node::Paragraph { children } => {
                let base = Self::inline_base(t.text_secondary, ui_text_xl(cx), px(22.0), false);
                Self::render_inlines_with_selection(children, &base, t, cx, selection)
            }
            Node::CodeBlock { language, code } => {
                let selection_bg = rgba(0x3390ff40);

                // Render code lines with selection
                let mut code_lines: Vec<Div> = Vec::new();
                let mut offset = 0usize;

                for line in code.lines() {
                    let line_len = char_len(line);
                    let line_end = offset + line_len + 1; // +1 for newline

                    let line_sel = selection.and_then(|(s, e)| {
                        if e <= offset || s >= line_end {
                            None
                        } else {
                            Some((
                                s.saturating_sub(offset),
                                (e - offset).min(line_len),
                            ))
                        }
                    });

                    let line_div = if let Some((sel_start, sel_end)) = line_sel {
                        let (before, selected, after) = slice_by_chars(line, sel_start, sel_end);
                        div()
                            .h(px(18.0))
                            .flex()
                            .child(div().child(before))
                            .child(div().bg(selection_bg).child(selected))
                            .child(div().child(after))
                    } else {
                        div()
                            .h(px(18.0))
                            .child(if line.is_empty() { " ".to_string() } else { line.to_string() })
                    };

                    code_lines.push(line_div);
                    offset = line_end;
                }

                code_block_container(language.as_deref(), t, cx)
                    .child(
                        div()
                            .p(px(12.0))
                            .font_family(mono_font_family(cx))
                            .text_size(ui_text_md(cx))
                            .text_color(rgb(t.text_secondary))
                            .flex()
                            .flex_col()
                            .children(code_lines)
                    )
            }
            Node::List { ordered, items } => {
                let mut list = v_flex().gap(px(4.0)).pl(px(16.0));
                let mut offset = 0usize;

                for (i, item_inlines) in items.iter().enumerate() {
                    let item_len = Self::inlines_text_length(item_inlines) + 1;
                    let item_sel = selection.and_then(|(s, e)| {
                        if e <= offset || s >= offset + item_len {
                            None
                        } else {
                            Some((
                                s.saturating_sub(offset),
                                (e - offset).min(item_len - 1), // -1 to exclude newline
                            ))
                        }
                    });

                    let marker = if *ordered {
                        format!("{}.", i + 1)
                    } else {
                        "\u{2022}".to_string()
                    };
                    list = list.child(
                        div()
                            .flex()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(ui_text_xl(cx))
                                    .text_color(rgb(t.text_muted))
                                    .w(px(16.0))
                                    .flex_shrink_0()
                                    .child(marker)
                            )
                            .child(
                                Self::render_inlines_with_selection(
                                    item_inlines,
                                    &Self::inline_base(t.text_secondary, ui_text_xl(cx), px(22.0), false),
                                    t,
                                    cx,
                                    item_sel,
                                )
                                .flex_1(),
                            )
                    );
                    offset += item_len;
                }
                list
            }
            Node::Table { headers, rows, col_widths } => {
                Self::render_table_with_selection(headers, rows, col_widths, t, cx, selection)
            }
            Node::Blockquote { children } => {
                let base = Self::inline_base(t.text_muted, ui_text_xl(cx), px(22.0), true);
                div()
                    .pl(px(12.0))
                    .border_l_2()
                    .border_color(rgb(t.text_muted))
                    .child(Self::render_inlines_with_selection(children, &base, t, cx, selection))
            }
            Node::HorizontalRule => {
                div()
                    .w_full()
                    .h(px(1.0))
                    .bg(rgb(t.border))
                    .my(px(8.0))
            }
            // Frontmatter renders as a self-contained metadata card. Partial
            // (inline) selection highlighting is intentionally omitted; block
            // selection and copy still work through the flat-text offsets.
            Node::Frontmatter { block, .. } => Self::render_frontmatter(block, t, cx),
        }
    }

    /// Render a frontmatter block as a bordered metadata card.
    pub(crate) fn render_frontmatter(fm: &Frontmatter, t: &ThemeColors, cx: &App) -> Div {
        let card = v_flex()
            .gap(px(4.0))
            .w_full()
            .p(px(12.0))
            .mb(px(8.0))
            .rounded(px(6.0))
            .bg(rgb(t.bg_secondary))
            .border_1()
            .border_color(rgb(t.border))
            .text_size(ui_text_md(cx));

        match fm {
            Frontmatter::Raw(raw) => card.font_family(mono_font_family(cx)).children(
                raw.lines().map(|line| {
                    div()
                        .text_color(rgb(t.text_secondary))
                        .child(if line.is_empty() { " ".to_string() } else { line.to_string() })
                }),
            ),
            Frontmatter::Parsed(entries) => {
                card.children(entries.iter().map(|(key, value)| {
                    Self::render_fm_entry(key, value, t, cx)
                }))
            }
        }
    }

    /// Render a single `key: value` frontmatter entry. Scalars sit inline next
    /// to the key; lists and nested maps stack below it, indented.
    fn render_fm_entry(key: &str, value: &FmValue, t: &ThemeColors, cx: &App) -> Div {
        let key_label = || {
            div()
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(t.text_muted))
                .child(key.to_string())
        };

        match value {
            FmValue::Scalar(s) => h_flex()
                .gap(px(8.0))
                .items_baseline()
                .child(key_label().min_w(px(120.0)).flex_shrink_0())
                .child(
                    div()
                        .flex_1()
                        .text_color(rgb(t.text_primary))
                        .child(s.clone()),
                ),
            FmValue::Empty => h_flex()
                .gap(px(8.0))
                .items_baseline()
                .child(key_label().min_w(px(120.0)).flex_shrink_0())
                .child(
                    div()
                        .italic()
                        .text_color(rgb(t.text_muted))
                        .child("\u{2014}"),
                ),
            FmValue::List(items) => v_flex()
                .gap(px(2.0))
                .child(key_label())
                .child(Self::render_fm_list(items, t, cx)),
            FmValue::Map(sub) => v_flex()
                .gap(px(2.0))
                .child(key_label())
                .child(
                    v_flex()
                        .gap(px(4.0))
                        .pl(px(16.0))
                        .children(sub.iter().map(|(k, v)| Self::render_fm_entry(k, v, t, cx))),
                ),
        }
    }

    /// Render a frontmatter sequence as a bulleted, indented list.
    fn render_fm_list(items: &[FmValue], t: &ThemeColors, cx: &App) -> Div {
        let mut list = v_flex().gap(px(2.0)).pl(px(16.0));
        for item in items {
            list = list.child(match item {
                FmValue::Scalar(s) => h_flex()
                    .gap(px(8.0))
                    .items_baseline()
                    .child(div().text_color(rgb(t.text_muted)).child("\u{2022}"))
                    .child(div().text_color(rgb(t.text_primary)).child(s.clone())),
                FmValue::Empty => h_flex()
                    .gap(px(8.0))
                    .child(div().text_color(rgb(t.text_muted)).child("\u{2022}")),
                FmValue::List(inner) => v_flex()
                    .child(div().text_color(rgb(t.text_muted)).child("\u{2022}"))
                    .child(Self::render_fm_list(inner, t, cx)),
                FmValue::Map(sub) => v_flex()
                    .gap(px(4.0))
                    .child(div().text_color(rgb(t.text_muted)).child("\u{2022}"))
                    .child(
                        v_flex()
                            .gap(px(4.0))
                            .pl(px(16.0))
                            .children(sub.iter().map(|(k, v)| Self::render_fm_entry(k, v, t, cx))),
                    ),
            });
        }
        list
    }

    /// Render inline elements with selection highlighting.
    /// Render inline elements as a single wrapping text element with
    /// per-run highlighting (bold / italic / inline code / links / selection).
    ///
    /// Using one `StyledText` (instead of a `flex()` row of text `div`s)
    /// lets GPUI's text layout wrap the paragraph at word boundaries inside
    /// the bubble's constrained width. The previous `flex()` approach made
    /// each text run a shrinkable flex item, which collapsed long runs to a
    /// single character per line.
    pub(crate) fn render_inlines_with_selection(
        inlines: &[Inline],
        base: &TextStyle,
        t: &ThemeColors,
        cx: &App,
        selection: Option<(usize, usize)>,
    ) -> Div {
        let styled = Self::build_inline_styled(inlines, base, t, cx, selection, &[]);
        div().whitespace_normal().child(styled)
    }

    /// Build a `StyledText` for the given inline nodes, applying per-range
    /// highlights for bold / italic / inline code / links and an optional
    /// selection background. Wrapping is handled natively by GPUI's text
    /// layout once the element is placed in a container with a definite width.
    pub(crate) fn build_inline_styled(
        inlines: &[Inline],
        base: &TextStyle,
        t: &ThemeColors,
        _cx: &App,
        selection: Option<(usize, usize)>,
        search_highlights: &[(Range<usize>, HighlightStyle)],
    ) -> StyledText {
        let text = Self::render_inlines_as_text(inlines);
        let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();

        // Record a highlight for every styled leaf (bold / italic / code / link / strikethrough).
        let mut char_offset = 0usize;
        Self::collect_inline_highlights(
            inlines,
            t,
            &text,
            &mut char_offset,
            FontWeight::default(),
            FontStyle::default(),
            false,
            false,
            &mut highlights,
        );

        // Append search highlights
        highlights.extend_from_slice(search_highlights);

        let sel_byte_range = selection.and_then(|(start, end)| {
            let sel_start = Self::char_to_byte(&text, start);
            let sel_end = Self::char_to_byte(&text, end);
            if sel_end > sel_start {
                Some(sel_start..sel_end)
            } else {
                None
            }
        });

        let selection_style = HighlightStyle {
            background_color: Some(rgba(0x3390ff40).into()),
            ..Default::default()
        };

        let highlights = crate::selection::merge_highlights(
            &text,
            &highlights,
            sel_byte_range,
            selection_style,
        );

        StyledText::new(text).with_default_highlights(base, highlights)
    }

    /// Recursively collect highlight ranges for styled inline leaves.
    #[allow(clippy::too_many_arguments)]
    fn collect_inline_highlights(
        inlines: &[Inline],
        t: &ThemeColors,
        text: &str,
        char_offset: &mut usize,
        weight: FontWeight,
        style: FontStyle,
        link: bool,
        strikethrough: bool,
        out: &mut Vec<(Range<usize>, HighlightStyle)>,
    ) {
        for inline in inlines {
            match inline {
                Inline::Text(s) => {
                    let byte_start = Self::char_to_byte(text, *char_offset);
                    let byte_end = Self::char_to_byte(text, *char_offset + s.chars().count());
                    if weight != FontWeight::default()
                        || style != FontStyle::default()
                        || link
                        || strikethrough
                    {
                        out.push((byte_start..byte_end, Self::inline_highlight(t, weight, style, link, strikethrough, None)));
                    }
                    *char_offset += s.chars().count();
                }
                Inline::Code(c) => {
                    let byte_start = Self::char_to_byte(text, *char_offset);
                    let byte_end = Self::char_to_byte(text, *char_offset + c.chars().count());
                    out.push((
                        byte_start..byte_end,
                        Self::inline_highlight(t, weight, style, link, strikethrough, Some(t.bg_primary)),
                    ));
                    *char_offset += c.chars().count();
                }
                Inline::Bold(children) => Self::collect_inline_highlights(
                    children,
                    t,
                    text,
                    char_offset,
                    FontWeight::BOLD,
                    style,
                    link,
                    strikethrough,
                    out,
                ),
                Inline::Italic(children) => Self::collect_inline_highlights(
                    children,
                    t,
                    text,
                    char_offset,
                    weight,
                    FontStyle::Italic,
                    link,
                    strikethrough,
                    out,
                ),
                Inline::Strikethrough(children) => Self::collect_inline_highlights(
                    children,
                    t,
                    text,
                    char_offset,
                    weight,
                    style,
                    link,
                    true,
                    out,
                ),
                Inline::Link { children, .. } => {
                    Self::collect_inline_highlights(
                        children,
                        t,
                        text,
                        char_offset,
                        weight,
                        style,
                        true,
                        strikethrough,
                        out,
                    );
                }
            }
        }
    }

    /// Build the base `TextStyle` shared by every run of an inline block.
    fn inline_base(color: u32, size: Pixels, lh: Pixels, italic: bool) -> TextStyle {
        TextStyle {
            color: rgb(color).into(),
            font_size: AbsoluteLength::from(size),
            line_height: DefiniteLength::from(lh),
            font_style: if italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            },
            ..Default::default()
        }
    }

    /// Convert a character offset within `text` to its byte offset.
    pub(crate) fn char_to_byte(text: &str, char_idx: usize) -> usize {
        text.char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(text.len())
    }

    /// Build the `HighlightStyle` for a single styled inline leaf.
    fn inline_highlight(
        t: &ThemeColors,
        weight: FontWeight,
        style: FontStyle,
        link: bool,
        strikethrough: bool,
        background: Option<u32>,
    ) -> HighlightStyle {
        HighlightStyle {
            color: if link {
                Some(rgb(t.accent).into())
            } else {
                None
            },
            font_weight: if weight == FontWeight::BOLD {
                Some(FontWeight::BOLD)
            } else {
                None
            },
            font_style: if style == FontStyle::Italic {
                Some(FontStyle::Italic)
            } else {
                None
            },
            background_color: background.map(|c| rgb(c).into()),
            underline: if link {
                Some(UnderlineStyle::default())
            } else {
                None
            },
            strikethrough: if strikethrough {
                Some(StrikethroughStyle::default())
            } else {
                None
            },
            ..Default::default()
        }
    }

    /// Render a table with selection highlighting.
    pub(crate) fn render_table_with_selection(
        headers: &[Vec<Inline>],
        rows: &[Vec<Vec<Inline>>],
        col_widths: &[usize],
        t: &ThemeColors,
        cx: &App,
        selection: Option<(usize, usize)>,
    ) -> Div {
        // Column widths are precomputed at parse time.
        let mut table = v_flex()
            .rounded(px(4.0))
            .border_1()
            .border_color(rgb(t.border))
            .overflow_hidden();

        let mut offset = 0usize;

        // Header row
        if !headers.is_empty() {
            let mut header_row = div()
                .flex()
                .bg(rgb(t.bg_header))
                .border_b_1()
                .border_color(rgb(t.border));

            for (i, header) in headers.iter().enumerate() {
                let cell_len = Self::inlines_text_length(header) + if i > 0 { 1 } else { 0 }; // +1 for tab
                let cell_sel = selection.and_then(|(s, e)| {
                    let cell_start = offset + if i > 0 { 1 } else { 0 }; // skip tab
                    let cell_end = offset + cell_len;
                    if e <= cell_start || s >= cell_end {
                        None
                    } else {
                        Some((
                            s.saturating_sub(cell_start),
                            (e - cell_start).min(Self::inlines_text_length(header)),
                        ))
                    }
                });

                let width = col_widths.get(i).copied().unwrap_or(10);
                let min_w = ((width * 8) + 24).max(80) as f32;
                let mut header_base = Self::inline_base(t.text_primary, ui_text_md(cx), px(18.0), false);
                header_base.font_weight = FontWeight::SEMIBOLD;
                header_row = header_row.child(
                    div()
                        .min_w(px(min_w))
                        .px(px(12.0))
                        .py(px(8.0))
                        .child(Self::render_inlines_with_selection(
                            header,
                            &header_base,
                            t,
                            cx,
                            cell_sel,
                        ))
                );
                offset += cell_len;
            }
            offset += 1; // newline
            table = table.child(header_row);
        }

        // Data rows
        for (row_idx, row) in rows.iter().enumerate() {
            let mut row_div = div()
                .flex()
                .when(row_idx % 2 == 1, |d| d.bg(rgb(t.bg_secondary)));

            if row_idx < rows.len() - 1 {
                row_div = row_div.border_b_1().border_color(rgb(t.border));
            }

            for (i, cell) in row.iter().enumerate() {
                let cell_len = Self::inlines_text_length(cell) + if i > 0 { 1 } else { 0 };
                let cell_sel = selection.and_then(|(s, e)| {
                    let cell_start = offset + if i > 0 { 1 } else { 0 };
                    let cell_end = offset + cell_len;
                    if e <= cell_start || s >= cell_end {
                        None
                    } else {
                        Some((
                            s.saturating_sub(cell_start),
                            (e - cell_start).min(Self::inlines_text_length(cell)),
                        ))
                    }
                });

                let width = col_widths.get(i).copied().unwrap_or(10);
                let min_w = ((width * 8) + 24).max(80) as f32;
                let cell_base = Self::inline_base(t.text_secondary, ui_text_md(cx), px(18.0), false);
                row_div = row_div.child(
                    div()
                        .min_w(px(min_w))
                        .px(px(12.0))
                        .py(px(6.0))
                        .child(Self::render_inlines_with_selection(
                            cell,
                            &cell_base,
                            t,
                            cx,
                            cell_sel,
                        ))
                );
                offset += cell_len;
            }
            offset += 1; // newline
            table = table.child(row_div);
        }

        table
    }

    /// Render inlines as plain text (for measuring, headings, etc.).
    pub(crate) fn render_inlines_as_text(inlines: &[Inline]) -> String {
        let mut result = String::new();
        for inline in inlines {
            Self::inline_to_text(inline, &mut result);
        }
        result
    }

    fn inline_to_text(inline: &Inline, out: &mut String) {
        match inline {
            Inline::Text(text) => out.push_str(text),
            Inline::Code(code) => out.push_str(code),
            Inline::Bold(children) | Inline::Italic(children) | Inline::Strikethrough(children) => {
                for child in children {
                    Self::inline_to_text(child, out);
                }
            }
            Inline::Link { children, .. } => {
                for child in children {
                    Self::inline_to_text(child, out);
                }
            }
        }
    }

}
