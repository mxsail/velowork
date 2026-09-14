//! Selection tracking type in a Document.

use super::position::DocumentPosition;

/// Document-level text selection state.
#[derive(Clone, Default, Debug)]
pub struct DocumentSelection {
    /// Starting boundary of drag
    pub start: Option<DocumentPosition>,
    /// Ending boundary of drag
    pub end: Option<DocumentPosition>,
    /// Active drag flag
    pub is_selecting: bool,
}

impl DocumentSelection {
    /// Return the selection bounds normalized (start <= end).
    pub fn normalized(&self) -> Option<(DocumentPosition, DocumentPosition)> {
        match (self.start, self.end) {
            (Some(s), Some(e)) => {
                if s <= e {
                    Some((s, e))
                } else {
                    Some((e, s))
                }
            }
            _ => None,
        }
    }

    /// Check if the selection is empty (start == end or unset).
    pub fn is_empty(&self) -> bool {
        match (self.start, self.end) {
            (Some(s), Some(e)) => s == e,
            _ => true,
        }
    }

    /// Clear selection.
    pub fn clear(&mut self) {
        self.start = None;
        self.end = None;
        self.is_selecting = false;
    }
}

/// Find word boundaries (start_char, end_char) around a character offset in text.
pub fn find_word_boundaries(text: &str, char_offset: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let idx = char_offset.min(chars.len().saturating_sub(1));
    let is_word_char = |c: char| c.is_alphanumeric() || c == '_';

    let target_is_word = is_word_char(chars[idx]);
    let mut start = idx;
    while start > 0 && is_word_char(chars[start - 1]) == target_is_word && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    let mut end = idx;
    while end < chars.len() && is_word_char(chars[end]) == target_is_word && !chars[end].is_whitespace() {
        end += 1;
    }
    (start, end)
}

/// Find line boundaries (start_char, end_char) around a character offset in text.
pub fn find_line_boundaries(text: &str, char_offset: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let idx = char_offset.min(chars.len().saturating_sub(1));
    let mut start = idx;
    while start > 0 && chars[start - 1] != '\n' {
        start -= 1;
    }
    let mut end = idx;
    while end < chars.len() && chars[end] != '\n' {
        end += 1;
    }
    (start, end)
}

/// Merge potentially overlapping highlight ranges and an optional selection range into
/// a strictly sorted, non-overlapping, coalesced list of highlights for GPUI's `StyledText`.
pub fn merge_highlights(
    text: &str,
    highlights: &[(std::ops::Range<usize>, gpui::HighlightStyle)],
    selection: Option<std::ops::Range<usize>>,
    selection_style: gpui::HighlightStyle,
) -> Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> {
    if text.is_empty() {
        return Vec::new();
    }

    let text_len = text.len();

    // Helper to clamp and snap to valid UTF-8 char boundary
    let snap_char_boundary = |mut b: usize| -> usize {
        b = b.min(text_len);
        while b > 0 && !text.is_char_boundary(b) {
            b -= 1;
        }
        b
    };

    let mut points: Vec<usize> = Vec::new();
    points.push(0);
    points.push(text_len);

    let mut valid_layers: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> = Vec::new();

    for (range, style) in highlights {
        let s = snap_char_boundary(range.start);
        let e = snap_char_boundary(range.end);
        if e > s {
            points.push(s);
            points.push(e);
            valid_layers.push((s..e, *style));
        }
    }

    let valid_selection = selection.and_then(|sel| {
        let s = snap_char_boundary(sel.start);
        let e = snap_char_boundary(sel.end);
        if e > s {
            points.push(s);
            points.push(e);
            Some(s..e)
        } else {
            None
        }
    });

    points.sort_unstable();
    points.dedup();

    let mut result: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> = Vec::new();

    for i in 0..points.len().saturating_sub(1) {
        let p_start = points[i];
        let p_end = points[i + 1];
        if p_start >= p_end {
            continue;
        }

        let mut merged = gpui::HighlightStyle::default();
        let mut active = false;

        for (layer_range, layer_style) in &valid_layers {
            if layer_range.start <= p_start && layer_range.end >= p_end {
                merged = merge_style(merged, *layer_style);
                active = true;
            }
        }

        if let Some(ref sel_range) = valid_selection {
            if sel_range.start <= p_start && sel_range.end >= p_end {
                merged = merge_style(merged, selection_style);
                active = true;
            }
        }

        if active && merged != gpui::HighlightStyle::default() {
            // Coalesce with previous if contiguous and identical in style
            if let Some(prev) = result.last_mut()
                && prev.0.end == p_start && prev.1 == merged {
                prev.0.end = p_end;
                continue;
            }
            result.push((p_start..p_end, merged));
        }
    }

    result
}

fn merge_style(base: gpui::HighlightStyle, over: gpui::HighlightStyle) -> gpui::HighlightStyle {
    gpui::HighlightStyle {
        color: over.color.or(base.color),
        font_weight: over.font_weight.or(base.font_weight),
        font_style: over.font_style.or(base.font_style),
        background_color: over.background_color.or(base.background_color),
        underline: over.underline.or(base.underline),
        strikethrough: over.strikethrough.or(base.strikethrough),
        fade_out: over.fade_out.or(base.fade_out),
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, DocumentBlock};
    use crate::clipboard::export_selection_to_markdown;

    #[test]
    fn test_position_ordering() {
        let pos1 = DocumentPosition::new(0, 5);
        let pos2 = DocumentPosition::new(0, 10);
        let pos3 = DocumentPosition::new(1, 0);

        assert!(pos1 < pos2);
        assert!(pos2 < pos3);
        assert!(pos1 < pos3);
    }

    #[test]
    fn test_selection_normalization() {
        let pos_start = DocumentPosition::new(1, 0);
        let pos_end = DocumentPosition::new(0, 5);

        let mut sel = DocumentSelection::default();
        sel.start = Some(pos_start);
        sel.end = Some(pos_end);

        let (norm_start, norm_end) = sel.normalized().unwrap();
        assert_eq!(norm_start, pos_end);
        assert_eq!(norm_end, pos_start);
    }

    #[test]
    fn test_export_selection_to_markdown() {
        let doc = Document {
            blocks: vec![
                DocumentBlock::Heading { level: 2, text: "Introduction".to_string() },
                DocumentBlock::Paragraph { text: "Hello world!".to_string() },
                DocumentBlock::CodeBlock { language: Some("rust".to_string()), code: "fn main() {}".to_string(), depth: 0 },
            ],
        };

        // Case 1: Partial selection within a paragraph block
        let sel_start = DocumentPosition::new(1, 0);
        let sel_end = DocumentPosition::new(1, 5);
        let result = export_selection_to_markdown(&doc, sel_start, sel_end);
        assert_eq!(result, "Hello");

        // Case 2: Full selection across headings and paragraphs
        let sel_start = DocumentPosition::new(0, 0);
        let sel_end = DocumentPosition::new(1, 12);
        let result = export_selection_to_markdown(&doc, sel_start, sel_end);
        assert_eq!(result, "## Introduction\n\nHello world!");
    }

    #[test]
    fn test_word_boundaries() {
        let text = "hello world_foo bar123";
        let (s, e) = find_word_boundaries(text, 1);
        assert_eq!(&text[s..e], "hello");

        let (s, e) = find_word_boundaries(text, 8);
        assert_eq!(&text[s..e], "world_foo");

        let (s, e) = find_word_boundaries(text, 18);
        assert_eq!(&text[s..e], "bar123");
    }

    #[test]
    fn test_line_boundaries() {
        let text = "first line\nsecond line\nthird line";
        let (s, e) = find_line_boundaries(text, 14);
        assert_eq!(&text[s..e], "second line");
    }

    #[test]
    fn test_merge_highlights_guarantees_non_overlapping() {
        let text = "Hello world! Bold and Italic text here.";
        let bold_style = gpui::HighlightStyle {
            font_weight: Some(gpui::FontWeight::BOLD),
            ..Default::default()
        };
        let italic_style = gpui::HighlightStyle {
            font_style: Some(gpui::FontStyle::Italic),
            ..Default::default()
        };
        let sel_style = gpui::HighlightStyle {
            background_color: Some(gpui::rgba(0x3390ff40).into()),
            ..Default::default()
        };

        // Two overlapping base highlights: 0..12 and 6..18
        let raw = vec![(0..12, bold_style), (6..18, italic_style)];
        // Selection spanning 4..15
        let merged = merge_highlights(text, &raw, Some(4..15), sel_style);

        // Verify:
        // 1. All ranges strictly increasing and non-overlapping
        let mut last_end = 0;
        for (range, _) in &merged {
            assert!(range.start >= last_end);
            assert!(range.end > range.start);
            assert!(range.end <= text.len());
            assert!(text.is_char_boundary(range.start));
            assert!(text.is_char_boundary(range.end));
            last_end = range.end;
        }
    }

    #[test]
    fn test_merge_highlights_cjk_user_scenario() {
        let text = " 的空间，让系统回到安全状态（占用率控制在 85% 以下）。";
        let sel_style = gpui::HighlightStyle {
            background_color: Some(gpui::rgba(0x3390ff40).into()),
            ..Default::default()
        };
        let code_style = gpui::HighlightStyle {
            color: Some(gpui::rgb(0xff0000).into()),
            ..Default::default()
        };

        let raw = vec![(4..10, code_style)];
        let merged = merge_highlights(text, &raw, Some(0..text.len()), sel_style);

        let mut last_end = 0;
        for (range, _) in &merged {
            assert!(range.start >= last_end);
            assert!(range.end > range.start);
            assert!(range.end <= text.len());
            assert!(text.is_char_boundary(range.start));
            assert!(text.is_char_boundary(range.end));
            last_end = range.end;
        }

        // Must succeed without panic in GPUI text run builder
        let base = gpui::TextStyle::default();
        let _styled = gpui::StyledText::new(text).with_default_highlights(&base, merged);
    }
}

