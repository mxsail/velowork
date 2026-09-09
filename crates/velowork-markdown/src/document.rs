//! Document tree model and parser.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// A block node in the Document representation.
#[derive(Clone, Debug)]
pub enum DocumentBlock {
    /// Paragraph text block
    Paragraph { text: String },
    /// Heading text block with H1-H6 level
    Heading { level: u8, text: String },
    /// Fenced or indented code block
    CodeBlock { language: Option<String>, code: String },
    /// Blockquote text
    Blockquote { text: String },
    /// List item with ordered flag, text value, and nesting depth
    ListItem { ordered: bool, text: String, depth: usize },
    /// Horizontal rule separator (---)
    HorizontalRule,
    /// YAML frontmatter metadata block
    Frontmatter { text: String },
    /// Grid-aligned table
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        plain_text: String,
    },
}

impl DocumentBlock {
    /// Returns the flat plain text representation of the block.
    pub fn plain_text(&self) -> &str {
        match self {
            DocumentBlock::Paragraph { text } => text,
            DocumentBlock::Heading { text, .. } => text,
            DocumentBlock::CodeBlock { code, .. } => code,
            DocumentBlock::Blockquote { text } => text,
            DocumentBlock::ListItem { text, .. } => text,
            DocumentBlock::HorizontalRule => "---",
            DocumentBlock::Frontmatter { text } => text,
            DocumentBlock::Table { plain_text, .. } => plain_text,
        }
    }
}

/// A parsed Document containing sequential blocks.
#[derive(Clone, Debug)]
pub struct Document {
    /// List of document blocks
    pub blocks: Vec<DocumentBlock>,
}

impl Document {
    /// Parses a raw Markdown string into a Document.
    pub fn parse(content: &str) -> Self {
        let mut blocks = Vec::new();

        // Peel off YAML frontmatter first if present
        let markdown = match split_frontmatter(content) {
            Some((inner, rest)) => {
                blocks.push(DocumentBlock::Frontmatter { text: inner.to_string() });
                rest
            }
            None => content,
        };

        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        let parser = Parser::new_ext(markdown, options);

        let mut current_text = String::new();
        let mut code_lang = None;
        let mut list_ordered = false;
        let mut list_depth: usize = 0;
        let mut in_table = false;
        let mut table_headers: Vec<String> = Vec::new();
        let mut table_rows: Vec<Vec<String>> = Vec::new();
        let mut current_row: Vec<String> = Vec::new();
        let mut current_cell = String::new();

        for event in parser {
            match event {
                Event::Start(Tag::Paragraph) => {
                    current_text.clear();
                }
                Event::End(TagEnd::Paragraph) => {
                    blocks.push(DocumentBlock::Paragraph { text: current_text.trim().to_string() });
                }
                Event::Start(Tag::Heading { level: _, .. }) => {
                    current_text.clear();
                }
                Event::End(TagEnd::Heading(level)) => {
                    let lvl = match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    };
                    blocks.push(DocumentBlock::Heading { level: lvl, text: current_text.trim().to_string() });
                }
                Event::Start(Tag::CodeBlock(kind)) => {
                    code_lang = match kind {
                        CodeBlockKind::Fenced(lang) => Some(lang.to_string()),
                        CodeBlockKind::Indented => None,
                    };
                    current_text.clear();
                }
                Event::End(TagEnd::CodeBlock) => {
                    blocks.push(DocumentBlock::CodeBlock {
                        language: code_lang.take(),
                        code: current_text.clone(),
                    });
                }
                Event::Start(Tag::BlockQuote(_)) => {
                    current_text.clear();
                }
                Event::End(TagEnd::BlockQuote(_)) => {
                    blocks.push(DocumentBlock::Blockquote { text: current_text.trim().to_string() });
                }
                Event::Start(Tag::List(ordered)) => {
                    list_ordered = ordered.is_some();
                    list_depth += 1;
                }
                Event::End(TagEnd::List(_)) => {
                    list_depth = list_depth.saturating_sub(1);
                }
                Event::Start(Tag::Item) => {
                    current_text.clear();
                }
                Event::End(TagEnd::Item) => {
                    blocks.push(DocumentBlock::ListItem {
                        ordered: list_ordered,
                        text: current_text.trim().to_string(),
                        depth: list_depth.saturating_sub(1),
                    });
                }
                Event::Start(Tag::Table(_)) => {
                    in_table = true;
                    table_headers.clear();
                    table_rows.clear();
                }
                Event::End(TagEnd::Table) => {
                    in_table = false;
                    let mut plain_text = String::new();
                    // Generate formatted plain text representation
                    for (i, h) in table_headers.iter().enumerate() {
                        if i > 0 { plain_text.push_str(" | "); }
                        plain_text.push_str(h);
                    }
                    plain_text.push('\n');
                    for row in &table_rows {
                        for (i, cell) in row.iter().enumerate() {
                            if i > 0 { plain_text.push_str(" | "); }
                            plain_text.push_str(cell);
                        }
                        plain_text.push('\n');
                    }
                    blocks.push(DocumentBlock::Table {
                        headers: table_headers.clone(),
                        rows: table_rows.clone(),
                        plain_text,
                    });
                }
                Event::Start(Tag::TableCell) => {
                    current_cell.clear();
                }
                Event::End(TagEnd::TableCell) => {
                    if table_headers.len() > current_row.len() && !table_rows.is_empty() {
                        current_row.push(current_cell.trim().to_string());
                    } else if table_rows.is_empty() {
                        table_headers.push(current_cell.trim().to_string());
                    } else {
                        current_row.push(current_cell.trim().to_string());
                    }
                }
                Event::Start(Tag::TableRow) => {
                    current_row.clear();
                }
                Event::End(TagEnd::TableRow) => {
                    if !table_headers.is_empty() && !current_row.is_empty() {
                        table_rows.push(current_row.clone());
                    }
                }
                Event::Text(t) => {
                    if in_table {
                        current_cell.push_str(&t);
                    } else {
                        current_text.push_str(&t);
                    }
                }
                Event::Code(t) => {
                    if in_table {
                        current_cell.push_str(&t);
                    } else {
                        current_text.push_str(&t);
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    if in_table {
                        current_cell.push('\n');
                    } else {
                        current_text.push('\n');
                    }
                }
                Event::Rule => {
                    blocks.push(DocumentBlock::HorizontalRule);
                }
                _ => {}
            }
        }

        Self { blocks }
    }
}

fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    if content.starts_with("---\n") || content.starts_with("---\r\n") {
        let delimiter = if content.starts_with("---\n") { "\n---\n" } else { "\r\n---\r\n" };
        if let Some(idx) = content[4..].find(delimiter) {
            let inner_start = 4;
            let inner_end = 4 + idx;
            let rest_start = inner_end + delimiter.len();
            return Some((&content[inner_start..inner_end], &content[rest_start..]));
        }
    }
    None
}
