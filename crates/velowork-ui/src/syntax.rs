//! Shared syntax highlighting utilities for velowork-ui.
//!
//! Provides types and functions for syntax highlighting that can be used
//! across inputs (`SimpleInput`), terminal commands, and viewers.

use gpui::Rgba;
use std::ops::Range;
use std::sync::{Arc, OnceLock};
use syntect::easy::HighlightLines;
use syntect::highlighting::Theme;
use syntect::parsing::SyntaxSet;

/// Global cached syntax set with extended syntaxes.
static SYNTAX_SET: OnceLock<Arc<SyntaxSet>> = OnceLock::new();

/// Global cached dark syntax highlighting theme.
static SYNTAX_THEME_DARK: OnceLock<Theme> = OnceLock::new();
/// Global cached light syntax highlighting theme.
static SYNTAX_THEME_LIGHT: OnceLock<Theme> = OnceLock::new();

/// Load a SyntaxSet with extended syntaxes.
/// The `SyntaxSet` is large (megabytes), so it is stored once behind an `Arc`
/// and shared. Each call only bumps the refcount instead of deep-cloning.
pub fn load_syntax_set() -> Arc<SyntaxSet> {
    SYNTAX_SET
        .get_or_init(|| Arc::new(two_face::syntax::extra_newlines()))
        .clone()
}

/// Load the syntax highlighting theme (cached).
/// Returns Dracula for dark themes, GitHub for light themes.
pub fn load_syntax_theme(is_dark: bool) -> &'static Theme {
    if is_dark {
        SYNTAX_THEME_DARK.get_or_init(|| {
            let theme_set = two_face::theme::extra();
            theme_set
                .get(two_face::theme::EmbeddedThemeName::Dracula)
                .clone()
        })
    } else {
        SYNTAX_THEME_LIGHT.get_or_init(|| {
            let theme_set = two_face::theme::extra();
            theme_set
                .get(two_face::theme::EmbeddedThemeName::Github)
                .clone()
        })
    }
}

/// A pre-processed span with color and byte range ready for display.
#[derive(Clone, Debug, PartialEq)]
pub struct HighlightedSpan {
    pub color: Rgba,
    pub text: String,
    pub range: Range<usize>,
}

/// Highlight a single-line or multi-line string with the given syntax (e.g. "bash", "sh", "json", "python", "rust").
pub fn highlight_text(text: &str, syntax_name: &str, is_dark: bool) -> Vec<HighlightedSpan> {
    if text.is_empty() {
        return Vec::new();
    }

    let ps = load_syntax_set();
    let theme = load_syntax_theme(is_dark);

    let syntax = ps
        .find_syntax_by_name(syntax_name)
        .or_else(|| ps.find_syntax_by_extension(syntax_name))
        .or_else(|| ps.find_syntax_by_token(syntax_name))
        .unwrap_or_else(|| ps.find_syntax_plain_text());

    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut spans = Vec::new();
    let mut current_offset = 0;

    for line in text.split_inclusive('\n') {
        let ranges = highlighter
            .highlight_line(line, &ps)
            .unwrap_or_default();

        for (style, part) in ranges {
            let len = part.len();
            let start = current_offset;
            let end = start + len;
            current_offset = end;

            let color = Rgba {
                r: style.foreground.r as f32 / 255.0,
                g: style.foreground.g as f32 / 255.0,
                b: style.foreground.b as f32 / 255.0,
                a: style.foreground.a as f32 / 255.0,
            };

            spans.push(HighlightedSpan {
                color,
                text: part.to_string(),
                range: start..end,
            });
        }
    }

    spans
}

/// Map file extension to syntax name for better coverage.
pub fn map_extension_to_syntax(ext: &str) -> Option<&'static str> {
    match ext.to_lowercase().as_str() {
        // Shell
        "sh" | "bash" | "zsh" | "fish" | "ksh" => Some("bash"),
        // TypeScript/JavaScript variants
        "ts" | "mts" | "cts" => Some("ts"),
        "tsx" => Some("tsx"),
        "jsx" => Some("tsx"),
        "mjs" | "cjs" | "js" => Some("js"),
        // Web
        "vue" | "svelte" | "astro" | "html" | "htm" => Some("html"),
        "css" | "scss" | "sass" | "less" => Some("css"),
        "json" | "jsonc" | "json5" => Some("json"),
        "yaml" | "yml" => Some("yaml"),
        "toml" => Some("toml"),
        "xml" | "svg" => Some("xml"),
        "md" | "markdown" => Some("markdown"),
        // Systems / Backend
        "rs" => Some("rust"),
        "go" => Some("go"),
        "py" | "pyw" | "pyi" => Some("python"),
        "c" | "h" => Some("c"),
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" => Some("cpp"),
        "java" => Some("java"),
        "kt" | "kts" => Some("kotlin"),
        "rb" => Some("ruby"),
        "php" => Some("php"),
        "sql" => Some("sql"),
        // Schema / IaC / RPC
        "graphql" | "gql" | "graphqls" => Some("graphql"),
        "prisma" => Some("graphql"),
        "tf" | "tfvars" | "hcl" => Some("tf"),
        "proto" => Some("protobuf"),
        "dockerfile" | "containerfile" => Some("dockerfile"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_bash_command() {
        let spans = highlight_text("echo 'hello world' | grep hello", "bash", true);
        assert!(!spans.is_empty());
        let full_text: String = spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(full_text, "echo 'hello world' | grep hello");
    }

    #[test]
    fn test_map_extension() {
        assert_eq!(map_extension_to_syntax("sh"), Some("bash"));
        assert_eq!(map_extension_to_syntax("rs"), Some("rust"));
        assert_eq!(map_extension_to_syntax("unknown_ext"), None);
    }
}
