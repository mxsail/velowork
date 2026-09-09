//! Position tracking type in a Document.

/// Coordinate pointing to a specific character inside a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocumentPosition {
    /// Index of the block within the document tree
    pub block_index: usize,
    /// Flat character offset inside the block's text representation
    pub char_offset: usize,
}

impl DocumentPosition {
    /// Create a new DocumentPosition.
    pub fn new(block_index: usize, char_offset: usize) -> Self {
        Self {
            block_index,
            char_offset,
        }
    }
}

impl PartialOrd for DocumentPosition {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DocumentPosition {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.block_index.cmp(&other.block_index) {
            std::cmp::Ordering::Equal => self.char_offset.cmp(&other.char_offset),
            ord => ord,
        }
    }
}
