//! Selection exporter translating document selections to Markdown/plain text copy strings.

use super::document::{Document, DocumentBlock};
use super::position::DocumentPosition;

/// Formats the selected document range as a styled Markdown string.
pub fn export_selection_to_markdown(
    doc: &Document,
    start: DocumentPosition,
    end: DocumentPosition,
) -> String {
    let mut out = String::new();

    for i in start.block_index..=end.block_index {
        if i >= doc.blocks.len() {
            break;
        }
        let block = &doc.blocks[i];
        let plain = block.plain_text();
        let len = plain.chars().count();

        let start_char = if i == start.block_index { start.char_offset } else { 0 };
        let end_char = if i == end.block_index { end.char_offset } else { len };

        if start_char >= end_char {
            continue;
        }

        let is_full = start_char == 0 && end_char == len;
        let substring: String = plain.chars().skip(start_char).take(end_char - start_char).collect();

        if i > start.block_index {
            out.push('\n');
            if is_full {
                out.push('\n'); // double spacing for markdown blocks
            }
        }

        if is_full {
            match block {
                DocumentBlock::Paragraph { .. } => {
                    out.push_str(&substring);
                }
                DocumentBlock::Heading { level, .. } => {
                    for _ in 0..*level {
                        out.push('#');
                    }
                    out.push(' ');
                    out.push_str(&substring);
                }
                DocumentBlock::CodeBlock { language, code, .. } => {
                    let lang = language.as_deref().unwrap_or("");
                    out.push_str("```");
                    out.push_str(lang);
                    out.push('\n');
                    out.push_str(code);
                    if !code.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str("```");
                }
                DocumentBlock::Blockquote { .. } => {
                    out.push_str("> ");
                    out.push_str(&substring);
                }
                DocumentBlock::ListItem { ordered, index, depth, .. } => {
                    for _ in 0..*depth {
                        out.push_str("  ");
                    }
                    if let Some(idx) = index {
                        out.push_str(&format!("{}. ", idx));
                    } else if *ordered {
                        out.push_str("1. ");
                    } else {
                        out.push_str("- ");
                    }
                    out.push_str(&substring);
                }
                DocumentBlock::HorizontalRule => {
                    out.push_str("---");
                }
                DocumentBlock::Frontmatter { .. } => {
                    out.push_str("---\n");
                    out.push_str(&substring);
                    if !substring.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str("---");
                }
                DocumentBlock::Table { headers, rows, .. } => {
                    out.push_str("| ");
                    for h in headers {
                        out.push_str(h);
                        out.push_str(" | ");
                    }
                    out.push('\n');
                    out.push_str("| ");
                    for _ in headers {
                        out.push_str("--- | ");
                    }
                    out.push('\n');
                    for row in rows {
                        out.push_str("| ");
                        for cell in row {
                            out.push_str(cell);
                            out.push_str(" | ");
                        }
                        out.push('\n');
                    }
                }
            }
        } else {
            out.push_str(&substring);
        }
    }

    out
}
