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
