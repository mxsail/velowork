//! 提示词模板：按场景组装 system prompt。
//!
//! 纯数据 + 渲染，不含 IO，不依赖 gpui / workspace，可独立编译与单元测试。
//! system prompt 是面向 LLM 的技术指令，使用英文（与模型训练语料一致）；
//! 若未来需要多语言，可由调用方在外部包装。

/// 能力场景。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptScene {
    CommandGen,
    ShellScript,
    ErrorDiagnosis,
    LogExplain,
    ConfigGen,
    Troubleshoot,
    General,
}

/// 注入 system prompt 的结构化上下文。
#[derive(Clone, Debug, Default)]
pub struct ContextBundle {
    /// 聚焦终端屏幕内容。
    pub terminal_screen: Option<String>,
    /// 会话列表文本。
    pub sessions: Option<String>,
    /// 单个会话配置文本。
    pub session_config: Option<String>,
    /// 相关文件：(路径/名称, 内容)。
    pub files: Vec<(String, String)>,
    /// 其他任意上下文片段（已格式化的文本）。
    pub extra: Vec<String>,
    /// 实时采集的「当前上下文」段落（会话 / Tab / cwd / 选中文本）。
    pub live_context: Option<String>,
}

/// 渲染某场景的 system prompt。
pub fn render(scene: PromptScene, bundle: &ContextBundle) -> String {
    let mut p = String::new();
    p.push_str(base_instruction());
    p.push('\n');
    p.push_str(scene_instruction(scene));
    p.push_str("\n\n# Context\n");
    if let Some(s) = &bundle.terminal_screen {
        p.push_str(&format!("## Focused terminal screen\n```\n{}\n```\n", s));
    }
    if let Some(s) = &bundle.sessions {
        p.push_str(&format!("## Sessions\n{}\n", s));
    }
    if let Some(s) = &bundle.session_config {
        p.push_str(&format!("## Session config\n{}\n", s));
    }
    for (name, content) in &bundle.files {
        p.push_str(&format!("## File: {}\n```\n{}\n```\n", name, content));
    }
    if let Some(live) = &bundle.live_context {
        p.push_str(live);
        p.push('\n');
    }
    for e in &bundle.extra {
        p.push_str(e);
        p.push('\n');
    }
    p
}

/// 通用角色与行为约束。
fn base_instruction() -> &'static str {
    "You are Velowork AI, an assistant embedded in a cross-platform terminal multiplexer. \
     You can read the focused terminal screen, run commands on it, list sessions and inspect \
     their configuration. When a task is best solved by a tool, emit a tool call instead of \
     guessing. Prefer read-only actions; destructive commands are gated by the user's permission."
}

/// 各场景的目标说明。
fn scene_instruction(scene: PromptScene) -> &'static str {
    match scene {
        PromptScene::CommandGen => {
            "Goal: generate a precise shell command for the user's request. Return the command, \
             and only add explanation when necessary."
        }
        PromptScene::ShellScript => {
            "Goal: write a robust shell script (bash) for the user's automation request. Prefer \
             safe, idempotent scripts and explain any assumptions."
        }
        PromptScene::ErrorDiagnosis => {
            "Goal: diagnose an error from the provided terminal/log context. Explain the root \
             cause and suggest a concrete fix."
        }
        PromptScene::LogExplain => {
            "Goal: explain the provided log excerpt in plain language, highlighting warnings and \
             errors and their likely meaning."
        }
        PromptScene::ConfigGen => {
            "Goal: generate a configuration snippet for the requested kind (ssh, nginx, systemd, \
             docker-compose). Keep it minimal and correct."
        }
        PromptScene::Troubleshoot => {
            "Goal: walk through troubleshooting steps for the reported issue, using available \
             tools and context. Proceed step by step."
        }
        PromptScene::General => {
            "Goal: help the user with their terminal / session / configuration question, using \
             available tools when helpful."
        }
    }
}

/// 渲染终端 Inline AI 气泡浮窗的场景化 system prompt。
///
/// 遵循 Prompt Caching 最佳实践排序：
/// 1. 静态基础角色与格式指令
/// 2. 场景化目标与代码块强约束
/// 3. 运行环境元数据 (OS, Shell, CWD, Remote)
/// 4. 终端选区与关联上下文快照
pub fn render_inline_prompt(
    scene: PromptScene,
    snapshot: &crate::context::TerminalContextSnapshot,
) -> String {
    let mut p = String::new();
    p.push_str(inline_base_instruction());
    p.push('\n');
    p.push_str(inline_scene_instruction(scene));
    p.push_str("\n\n# Runtime Environment\n");
    p.push_str(&format!("- OS: {}\n", snapshot.os));
    p.push_str(&format!("- Shell: {}\n", snapshot.shell));
    if !snapshot.cwd.is_empty() {
        p.push_str(&format!("- Working Directory: {}\n", snapshot.cwd));
    }
    if snapshot.is_remote {
        let host = snapshot.remote_host.as_deref().unwrap_or("remote-host");
        p.push_str(&format!("- Session: {} (SSH Remote: {})\n", snapshot.session_name, host));
    } else if !snapshot.session_name.is_empty() {
        p.push_str(&format!("- Session: {} (Local)\n", snapshot.session_name));
    }
    if let Some(ref draft) = snapshot.active_input_draft {
        if !draft.trim().is_empty() {
            p.push_str(&format!("- Active Prompt / Unexecuted Command Draft: `{}`\n", draft));
        }
    }
    if let Some(ref last_cmd) = snapshot.last_command {
        if !last_cmd.trim().is_empty() {
            p.push_str(&format!("- Last Executed Command: `{}`\n", last_cmd));
        }
    }

    if let Some(ref sel) = snapshot.selected_text {
        if !sel.trim().is_empty() {
            p.push_str(&format!("\n# Selected Terminal Text\n```\n{}\n```\n", sel));
        }
    }

    if let Some(ref buf) = snapshot.surrounding_buffer {
        if !buf.trim().is_empty() {
            p.push_str(&format!("\n# Recent Terminal Buffer Context\n```\n{}\n```\n", buf));
        }
    }

    p
}

fn inline_base_instruction() -> &'static str {
    "You are Velowork Terminal Inline AI, a fast, lightweight terminal coding assistant. \
     You provide precise, concise help tailored to the user's active shell and operating system. \
     Keep responses brief and avoid conversational fluff. When suggesting terminal commands, \
     always format them in fenced code blocks (e.g. ```bash ... ```) so they can be run directly."
}

fn inline_scene_instruction(scene: PromptScene) -> &'static str {
    match scene {
        PromptScene::CommandGen => {
            "Goal: Generate the most accurate shell command for the user's intent. Return the executable command directly."
        }
        PromptScene::ErrorDiagnosis => {
            "Goal: Diagnose the selected error or failure. Explain root cause in 1-2 sentences, then provide the exact fix command."
        }
        PromptScene::LogExplain => {
            "Goal: Explain the selected log output concisely, highlighting the critical warning/error message."
        }
        _ => {
            "Goal: Answer the user's terminal question directly and concisely, providing accurate commands."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_includes_scene_and_context() {
        let bundle = ContextBundle {
            terminal_screen: Some("user@host:~$ ls".to_string()),
            sessions: Some("Sessions:\n- web [remote]".to_string()),
            ..Default::default()
        };
        let p = render(PromptScene::Troubleshoot, &bundle);
        assert!(p.contains("troubleshoot"));
        assert!(p.contains("user@host:~$ ls"));
        assert!(p.contains("web [remote]"));
    }

    #[test]
    fn render_empty_bundle_has_no_context_noise() {
        let p = render(PromptScene::General, &ContextBundle::default());
        assert!(p.contains("Velowork AI"));
        assert!(!p.contains("## Focused terminal screen"));
    }
}
