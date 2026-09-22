//! Smart Action recommendation engine for terminal selection.
//!
//! Analyzes selected terminal text and terminal exit status to infer user intent
//! and recommend the highest-value one-click action chip in the Capsule Toolbar.

/// The classified category of terminal text selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmartActionKind {
    /// Compiler error, runtime exception, panic, or non-zero exit failure.
    FixError,
    /// Shell command or pipeline (e.g. git, docker, cargo, curl).
    ExplainCommand,
    /// File path with line number (e.g. `src/main.rs:42:15`) or URL / endpoint.
    InspectTarget,
    /// JSON or structured data block.
    FormatData,
    /// General terminal text.
    General,
}

/// A structured recommendation containing UI metadata and pre-filled AI prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmartActionRecommendation {
    pub kind: SmartActionKind,
    /// Internationalization key for the chip label.
    pub label_key: &'static str,
    /// Standard icon name for UI representation.
    pub icon_name: &'static str,
    /// Pre-filled prompt to immediately send to LLM when chip is clicked.
    pub prompt_text: String,
}

/// Classify terminal selection intent in <= 0.1ms with zero blocking.
pub fn classify_selection_intent(
    selection: &str,
    last_exit_code: Option<i32>,
) -> SmartActionRecommendation {
    let trimmed = selection.trim();
    if trimmed.chars().count() < 2 {
        return SmartActionRecommendation {
            kind: SmartActionKind::General,
            label_key: "terminal.ai_toolbar_smart_explain",
            icon_name: "ai_assistant",
            prompt_text: format!("解释以下终端文本：\n```\n{}\n```", trimmed),
        };
    }

    // Truncate to first 2000 chars for classification efficiency
    let head: String = trimmed.chars().take(2000).collect();
    let lower = head.to_lowercase();

    // 1. Check for Error / Exception / Panic / Failure
    let has_error_keyword = lower.contains("error:")
        || lower.contains("error[e")
        || lower.contains("panic:")
        || lower.contains("panicked at")
        || lower.contains("exception:")
        || lower.contains("fatal:")
        || lower.contains("failed to")
        || lower.contains("traceback (most recent call last):")
        || lower.contains("command not found")
        || lower.contains("permission denied")
        || lower.contains("syntaxerror")
        || lower.contains("typeerror")
        || lower.contains("nullpointerexception")
        || lower.contains("undefined is not a function")
        || lower.contains("cannot find module")
        || lower.contains("segmentation fault")
        || lower.contains("segfault")
        || lower.contains("failed with exit code");

    let is_failed_exit = last_exit_code.map(|c| c != 0).unwrap_or(false);

    if has_error_keyword || (is_failed_exit && lower.contains("error")) {
        return SmartActionRecommendation {
            kind: SmartActionKind::FixError,
            label_key: "terminal.ai_toolbar_smart_fix",
            icon_name: "sparkle",
            prompt_text: format!("诊断并给出此报错的根本原因及可执行的修复命令：\n```\n{}\n```", trimmed),
        };
    }

    // 2. Check for CLI Command
    let first_line = trimmed.lines().next().unwrap_or("").trim();
    let cleaned_cmd = first_line.strip_prefix("$ ").or_else(|| first_line.strip_prefix("# ")).unwrap_or(first_line).trim();

    let is_cli_command = [
        "git ", "docker ", "kubectl ", "cargo ", "npm ", "pnpm ", "yarn ",
        "bun ", "pip ", "python ", "node ", "curl ", "wget ", "ssh ",
        "scp ", "rsync ", "systemctl ", "journalctl ", "tar ", "grep ",
        "awk ", "sed ", "find ", "apt ", "yum ", "brew ", "pacman ",
        "make ", "cmake ", "go ", "rustc ", "mvn ", "gradle ", "ansible ",
        "terraform ", "helm ",
    ]
    .iter()
    .any(|prefix| cleaned_cmd.starts_with(prefix));

    if is_cli_command {
        return SmartActionRecommendation {
            kind: SmartActionKind::ExplainCommand,
            label_key: "terminal.ai_toolbar_smart_explain_cmd",
            icon_name: "terminal",
            prompt_text: format!("详细解释以下命令的各参数作用与执行行为：\n```\n{}\n```", trimmed),
        };
    }

    // 3. Check for File location with line numbers (e.g. `src/main.rs:42:15` or `path/to/file.py:10`)
    let has_file_line_ref = trimmed.contains(".rs:")
        || trimmed.contains(".ts:")
        || trimmed.contains(".js:")
        || trimmed.contains(".py:")
        || trimmed.contains(".go:")
        || trimmed.contains(".c:")
        || trimmed.contains(".cpp:")
        || trimmed.contains(".h:")
        || trimmed.contains(".java:");

    if has_file_line_ref || trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return SmartActionRecommendation {
            kind: SmartActionKind::InspectTarget,
            label_key: "terminal.ai_toolbar_smart_inspect",
            icon_name: "external_link",
            prompt_text: format!("分析并查看以下目标上下文：\n{}", trimmed),
        };
    }

    // 4. Check for JSON / Structured Data
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() || trimmed.contains("\": \"") {
            return SmartActionRecommendation {
                kind: SmartActionKind::FormatData,
                label_key: "terminal.ai_toolbar_smart_format",
                icon_name: "file_text",
                prompt_text: format!("格式化并解析以下结构化数据，分析其关键字段结构：\n```\n{}\n```", trimmed),
            };
        }
    }

    // 5. Default General Explain
    SmartActionRecommendation {
        kind: SmartActionKind::General,
        label_key: "terminal.ai_toolbar_smart_explain",
        icon_name: "ai_assistant",
        prompt_text: format!("解释以下终端选中文本的内容与含义：\n```\n{}\n```", trimmed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_rust_error() {
        let text = "error[E0425]: cannot find value `foo` in this scope\n  --> src/main.rs:12:5";
        let rec = classify_selection_intent(text, Some(101));
        assert_eq!(rec.kind, SmartActionKind::FixError);
        assert_eq!(rec.label_key, "terminal.ai_toolbar_smart_fix");
    }

    #[test]
    fn test_classify_python_traceback() {
        let text = "Traceback (most recent call last):\n  File \"app.py\", line 10, in <module>\nZeroDivisionError: division by zero";
        let rec = classify_selection_intent(text, None);
        assert_eq!(rec.kind, SmartActionKind::FixError);
    }

    #[test]
    fn test_classify_cli_command() {
        let text = "docker run -d -p 8080:80 --name web nginx:alpine";
        let rec = classify_selection_intent(text, None);
        assert_eq!(rec.kind, SmartActionKind::ExplainCommand);
        assert_eq!(rec.label_key, "terminal.ai_toolbar_smart_explain_cmd");

        let text_with_prompt = "$ git log --oneline -n 5";
        let rec2 = classify_selection_intent(text_with_prompt, None);
        assert_eq!(rec2.kind, SmartActionKind::ExplainCommand);
    }

    #[test]
    fn test_classify_file_target() {
        let text = "src/layout/terminal_pane/mod.rs:150:20";
        let rec = classify_selection_intent(text, None);
        assert_eq!(rec.kind, SmartActionKind::InspectTarget);
        assert_eq!(rec.label_key, "terminal.ai_toolbar_smart_inspect");
    }

    #[test]
    fn test_classify_json_data() {
        let text = r#"{"status": "ok", "code": 200, "message": "success"}"#;
        let rec = classify_selection_intent(text, None);
        assert_eq!(rec.kind, SmartActionKind::FormatData);
        assert_eq!(rec.label_key, "terminal.ai_toolbar_smart_format");
    }

    #[test]
    fn test_classify_general_text() {
        let text = "Welcome to Ubuntu 24.04 LTS (GNU/Linux 6.8.0 x86_64)";
        let rec = classify_selection_intent(text, Some(0));
        assert_eq!(rec.kind, SmartActionKind::General);
        assert_eq!(rec.label_key, "terminal.ai_toolbar_smart_explain");
    }
}
