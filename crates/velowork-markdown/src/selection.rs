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
                DocumentBlock::CodeBlock { language: Some("rust".to_string()), code: "fn main() {}".to_string() },
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
}
