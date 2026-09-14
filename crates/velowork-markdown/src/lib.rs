#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]
#![recursion_limit = "512"]

//! Markdown renderer and Selection Framework for GPUI.

pub mod position;
pub mod selection;
pub mod document;
pub mod clipboard;
pub mod widgets;
pub mod types;
pub mod element;

mod parser;
mod render;

pub use element::{MarkdownElement, MarkdownSelectionEvent, SelectionEventCallback};
pub use selection::{find_line_boundaries, find_word_boundaries};
pub use types::{FmValue, Frontmatter, Inline, Node};

use gpui::*;
use velowork_core::selection::SelectionState;

/// Type alias for markdown selection (1D character offset).
pub type MarkdownSelection = SelectionState<usize>;


/// A rendered node that can be either a simple block or a code block with selectable lines.
pub enum RenderedNode {
    /// A simple block (heading, paragraph, list, etc.) - single selectable unit
    Simple {
        div: Div,
        start_offset: usize,
        end_offset: usize,
    },
    /// A code block with individually selectable lines
    CodeBlock {
        language: Option<String>,
        /// Each line as (div, start_offset, end_offset)
        lines: Vec<(Div, usize, usize)>,
    },
    /// A table with individually selectable rows
    Table {
        /// Header row (div, start_offset, end_offset)
        header: Option<(Div, usize, usize)>,
        /// Data rows as (div, start_offset, end_offset)
        rows: Vec<(Div, usize, usize)>,
    },
}

/// Parsed markdown document ready for rendering.
pub struct MarkdownDocument {
    pub(crate) nodes: Vec<Node>,
    /// Cumulative start offset (in characters) of each node, parallel to `nodes`.
    /// Precomputed at parse time so rendering does not re-walk node text lengths.
    pub(crate) node_offsets: Vec<usize>,
    /// Flat text representation of all visible content
    pub plain_text: String,
}

impl MarkdownDocument {
    /// Returns the parsed AST nodes.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Slice the document's plain text by character offset range [start, end).
    pub fn slice_plain_text(&self, start: usize, end: usize) -> String {
        let s = start.min(end);
        let e = start.max(end);
        self.plain_text.chars().skip(s).take(e - s).collect()
    }
}

