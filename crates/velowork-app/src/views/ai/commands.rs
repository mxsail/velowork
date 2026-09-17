//! Command and script extraction utilities from AI markdown responses.

/// 从 AI Markdown 输出中提取 bash/sh/shell/zsh 代码块或单行命令。
pub fn extract_commands(text: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut in_code_block = false;
    let mut is_shell_block = false;
    let mut current_block = String::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if in_code_block {
                in_code_block = false;
                if is_shell_block && !current_block.trim().is_empty() {
                    commands.push(current_block.trim().to_string());
                }
                current_block.clear();
                is_shell_block = false;
            } else {
                in_code_block = true;
                let lang = trimmed.trim_start_matches('`').to_lowercase();
                is_shell_block = lang.is_empty()
                    || lang.contains("bash")
                    || lang.contains("sh")
                    || lang.contains("shell")
                    || lang.contains("zsh");
            }
        } else if in_code_block && is_shell_block {
            if !current_block.is_empty() {
                current_block.push('\n');
            }
            current_block.push_str(line);
        }
    }

    commands
}
