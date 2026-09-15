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
    CodeBlock { language: Option<String>, code: String, depth: usize },
    /// Blockquote text
    Blockquote { text: String },
    /// List item with ordered flag, item index (for ordered), text value, and nesting depth
    ListItem { ordered: bool, index: Option<u64>, text: String, depth: usize },
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
        options.insert(Options::ENABLE_TASKLISTS);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_HEADING_ATTRIBUTES);
        let parser = Parser::new_ext(markdown, options);

        let mut current_text = String::new();
        let mut code_lang = None;
        let mut list_stack: Vec<(bool, u64)> = Vec::new();
        let mut list_item_emitted: Vec<bool> = Vec::new();
        let mut list_depth: usize = 0;
        let mut in_table = false;
        let mut table_headers: Vec<String> = Vec::new();
        let mut table_rows: Vec<Vec<String>> = Vec::new();
        let mut current_row: Vec<String> = Vec::new();
        let mut current_cell = String::new();

        let flush_list_item = |
            blocks: &mut Vec<DocumentBlock>,
            current_text: &mut String,
            list_depth: usize,
            list_stack: &mut Vec<(bool, u64)>,
            list_item_emitted: &mut Vec<bool>,
        | {
            if list_depth > 0 {
                if let Some(emitted) = list_item_emitted.last_mut() {
                    if !*emitted && !current_text.trim().is_empty() {
                        *emitted = true;
                        let text = current_text.trim().to_string();
                        current_text.clear();
                        let (ordered, index) = if let Some((is_ord, next_idx)) = list_stack.last_mut() {
                            let is_o = *is_ord;
                            let idx = if is_o {
                                let cur = *next_idx;
                                *next_idx += 1;
                                Some(cur)
                            } else {
                                None
                            };
                            (is_o, idx)
                        } else {
                            (false, None)
                        };
                        blocks.push(DocumentBlock::ListItem {
                            ordered,
                            index,
                            text,
                            depth: list_depth.saturating_sub(1),
                        });
                        return true;
                    }
                }
            }
            false
        };

        for event in parser {
            match event {
                Event::Start(Tag::Paragraph) => {
                    if list_depth == 0 {
                        current_text.clear();
                    }
                }
                Event::End(TagEnd::Paragraph) => {
                    let flushed = flush_list_item(&mut blocks, &mut current_text, list_depth, &mut list_stack, &mut list_item_emitted);
                    if !flushed {
                        let text = current_text.trim().to_string();
                        current_text.clear();
                        if !text.is_empty() {
                            blocks.push(DocumentBlock::Paragraph { text });
                        }
                    }
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
                    let text = current_text.trim().to_string();
                    current_text.clear();
                    blocks.push(DocumentBlock::Heading { level: lvl, text });
                }
                Event::Start(Tag::CodeBlock(kind)) => {
                    flush_list_item(&mut blocks, &mut current_text, list_depth, &mut list_stack, &mut list_item_emitted);
                    code_lang = match kind {
                        CodeBlockKind::Fenced(lang) => {
                            let clean = lang.split_whitespace().next().unwrap_or(&lang);
                            Some(clean.to_string())
                        }
                        CodeBlockKind::Indented => None,
                    };
                    current_text.clear();
                }
                Event::End(TagEnd::CodeBlock) => {
                    let code = current_text.trim_end_matches(['\r', '\n']).to_string();
                    current_text.clear();
                    blocks.push(DocumentBlock::CodeBlock {
                        language: code_lang.take(),
                        code,
                        depth: list_depth,
                    });
                }
                Event::Start(Tag::BlockQuote(_)) => {
                    flush_list_item(&mut blocks, &mut current_text, list_depth, &mut list_stack, &mut list_item_emitted);
                    current_text.clear();
                }
                Event::End(TagEnd::BlockQuote(_)) => {
                    let text = current_text.trim().to_string();
                    current_text.clear();
                    blocks.push(DocumentBlock::Blockquote { text });
                }
                Event::Start(Tag::List(ordered)) => {
                    flush_list_item(&mut blocks, &mut current_text, list_depth, &mut list_stack, &mut list_item_emitted);
                    let start_idx = ordered.unwrap_or(1);
                    list_stack.push((ordered.is_some(), start_idx));
                    list_depth += 1;
                }
                Event::End(TagEnd::List(_)) => {
                    list_stack.pop();
                    list_depth = list_depth.saturating_sub(1);
                }
                Event::Start(Tag::Item) => {
                    flush_list_item(&mut blocks, &mut current_text, list_depth, &mut list_stack, &mut list_item_emitted);
                    current_text.clear();
                    list_item_emitted.push(false);
                }
                Event::End(TagEnd::Item) => {
                    flush_list_item(&mut blocks, &mut current_text, list_depth, &mut list_stack, &mut list_item_emitted);
                    list_item_emitted.pop();
                    current_text.clear();
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
                        current_cell.push('`');
                        current_cell.push_str(&t);
                        current_cell.push('`');
                    } else {
                        current_text.push('`');
                        current_text.push_str(&t);
                        current_text.push('`');
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ordered_list_sequential_indices() {
        let md = "1. First item\n2. Second item\n3. Third item\n";
        let doc = Document::parse(md);
        assert_eq!(doc.blocks.len(), 3);

        if let DocumentBlock::ListItem { ordered, index, text, .. } = &doc.blocks[0] {
            assert!(*ordered);
            assert_eq!(*index, Some(1));
            assert_eq!(text, "First item");
        } else {
            panic!("Expected ListItem, got {:?}", doc.blocks[0]);
        }

        if let DocumentBlock::ListItem { ordered, index, text, .. } = &doc.blocks[1] {
            assert!(*ordered);
            assert_eq!(*index, Some(2));
            assert_eq!(text, "Second item");
        } else {
            panic!("Expected ListItem, got {:?}", doc.blocks[1]);
        }

        if let DocumentBlock::ListItem { ordered, index, text, .. } = &doc.blocks[2] {
            assert!(*ordered);
            assert_eq!(*index, Some(3));
            assert_eq!(text, "Third item");
        } else {
            panic!("Expected ListItem, got {:?}", doc.blocks[2]);
        }
    }

    #[test]
    fn test_code_block_parsing() {
        let md = "```bash\necho 'hello world'\n```\n";
        let doc = Document::parse(md);
        assert_eq!(doc.blocks.len(), 1);

        if let DocumentBlock::CodeBlock { language, code, .. } = &doc.blocks[0] {
            assert_eq!(language.as_deref(), Some("bash"));
            assert_eq!(code, "echo 'hello world'");
            assert!(!code.ends_with('\n'));
            assert_eq!(code.split('\n').count(), 1);
        } else {
            panic!("Expected CodeBlock, got {:?}", doc.blocks[0]);
        }
    }

    #[test]
    fn test_code_block_no_trailing_blank_line() {
        let md = "```bash\nline1\nline2\n```\n";
        let doc = Document::parse(md);
        if let DocumentBlock::CodeBlock { code, .. } = &doc.blocks[0] {
            assert_eq!(code, "line1\nline2");
            assert!(!code.ends_with('\n'));
            assert_eq!(code.split('\n').count(), 2);
        } else {
            panic!("Expected CodeBlock, got {:?}", doc.blocks[0]);
        }
    }

    #[test]
    fn test_list_with_code_block() {
        let md = r#"
1. **清理系统日志**
   如果使用的是 Systemd：
   ```bash
   # 只保留最近 7 天的日志
   sudo journalctl --vacuum-time=7d
   ```

2. **清理包管理器缓存**
   * **Ubuntu/Debian 系**：
     ```bash
     sudo apt-get clean
     ```
"#;
        for e in Parser::new_ext(md, Options::empty()) {
            println!("EVENT: {:?}", e);
        }
        let doc = Document::parse(md);
        for (i, b) in doc.blocks.iter().enumerate() {
            println!("{}: {:?}", i, b);
        }

        // Verify: Item 1 should be a ListItem, NOT plain Paragraph, and the bash script should be a CodeBlock, NOT duplicated!
        assert_eq!(doc.blocks.len(), 5);
        if let DocumentBlock::ListItem { ordered, index, text, .. } = &doc.blocks[0] {
            assert!(*ordered);
            assert_eq!(*index, Some(1));
            assert!(text.contains("清理系统日志"));
        } else {
            panic!("Expected ListItem for block 0, got {:?}", doc.blocks[0]);
        }
        if let DocumentBlock::CodeBlock { language, code, .. } = &doc.blocks[1] {
            assert_eq!(language.as_deref(), Some("bash"));
            assert!(code.contains("journalctl"));
        } else {
            panic!("Expected CodeBlock for block 1, got {:?}", doc.blocks[1]);
        }
        if let DocumentBlock::ListItem { ordered, index, text, .. } = &doc.blocks[2] {
            assert!(*ordered);
            assert_eq!(*index, Some(2));
            assert!(text.contains("清理包管理器缓存"));
        } else {
            panic!("Expected ListItem for block 2, got {:?}", doc.blocks[2]);
        }
        if let DocumentBlock::ListItem { ordered, index, text, depth } = &doc.blocks[3] {
            assert!(!*ordered);
            assert_eq!(*index, None);
            assert_eq!(*depth, 1);
            assert!(text.contains("Ubuntu/Debian"));
        } else {
            panic!("Expected nested ListItem for block 3, got {:?}", doc.blocks[3]);
        }
        if let DocumentBlock::CodeBlock { language, code, .. } = &doc.blocks[4] {
            assert_eq!(language.as_deref(), Some("bash"));
            assert!(code.contains("apt-get clean"));
        } else {
            panic!("Expected CodeBlock for block 4, got {:?}", doc.blocks[4]);
        }
    }

    #[test]
    fn test_danger_markdown_parsing() {
        let md = r#"
### 为什么这很危险？
当根目录（`/`）被占满（达到 100%）时，系统可能会出现以下严重问题：
1. **系统卡死或无法登录**：图形界面（GUI）需要写入临时文件，空间不足会导致无法登录桌面。
2. **服务崩溃**：数据库、Web 服务器等需要写日志或缓存的服务会停止运行。
3. **无法更新或安装软件**：任何写操作都可能因为“空间不足（No space left on device）”而失败。

---

### 紧急排查与清理建议

你可以按照以下步骤释放空间：

#### 步骤 1：查找是哪些文件/目录占用了空间
打开终端，运行以下命令来查看根目录下哪个文件夹最大（需要 `sudo` 权限）：
```bash
sudo du -xhd 1 / | sort -hr
```
*这会列出 `/` 下第一级子目录的大小并排序。常见的大户通常是 `/var`（日志/Docker）、`/home`（用户个人文件）或 `/usr`。*
"#;
        let doc = Document::parse(md);
        assert!(!doc.blocks.is_empty());

        // Heading 3: 为什么这很危险？
        if let DocumentBlock::Heading { level, text } = &doc.blocks[0] {
            assert_eq!(*level, 3);
            assert_eq!(text, "为什么这很危险？");
        } else {
            panic!("Expected Heading for block 0, got {:?}", doc.blocks[0]);
        }

        // Paragraph: 当根目录（`/`）被占满...
        if let DocumentBlock::Paragraph { text } = &doc.blocks[1] {
            assert!(text.contains("`/`"));
        } else {
            panic!("Expected Paragraph for block 1, got {:?}", doc.blocks[1]);
        }

        // Items 1, 2, 3: sequential indices
        for i in 1..=3 {
            if let DocumentBlock::ListItem { ordered, index, text, .. } = &doc.blocks[i + 1] {
                assert!(*ordered);
                assert_eq!(*index, Some(i as u64));
                assert!(!text.is_empty());
            } else {
                panic!("Expected ListItem for block {}, got {:?}", i + 1, doc.blocks[i + 1]);
            }
        }
    }
}
