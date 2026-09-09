//! LogExplainSkill（P1，Community）：解释 systemd / nginx / docker 等日志。
//!
//! 内部自动：若输入是文件路径则读取内容（`read_file`），否则把输入当作日志片段，
//! 调用 LLM 用通俗语言解释警告 / 错误及其可能含义。

use serde_json::json;

use crate::agent::AgentEvent;
use crate::prompt::{render, ContextBundle, PromptScene};
use crate::provider::stream_complete;
use crate::skill::{Skill, SkillCategory, SkillCtx, SkillResult, SkillTier};

pub struct LogExplainSkill;

impl Skill for LogExplainSkill {
    fn name(&self) -> &str {
        "log_explain"
    }
    fn i18n_key(&self) -> &str {
        "skill.log_explain"
    }
    fn description(&self) -> &str {
        "Explain a log excerpt (systemd / nginx / docker / generic) in plain language, \
         highlighting warnings and errors and their likely meaning. Accepts either a log \
         file path or raw log text."
    }
    fn tier(&self) -> SkillTier {
        SkillTier::Community
    }
    fn category(&self) -> SkillCategory {
        SkillCategory::Explain
    }

    fn run(&self, input: &str, ctx: &mut SkillCtx) -> SkillResult {
        let log_content = if input.trim().is_empty() {
            None
        } else if std::path::Path::new(input.trim()).exists() {
            ctx.registry
                .call("read_file", json!({ "path": input.trim() }), ctx.tool_ctx, ctx.cx)
                .ok()
        } else {
            Some(input.to_string())
        };

        let log_content = match log_content {
            Some(c) if !c.trim().is_empty() => c,
            _ => {
                // 退而求其次：读取聚焦终端屏幕作为日志来源。
                ctx.registry
                    .call("read_terminal_screen", json!({}), ctx.tool_ctx, ctx.cx)
                    .unwrap_or_default()
            }
        };

        if log_content.trim().is_empty() {
            return SkillResult {
                text: "No log content provided and the focused terminal is empty. Please paste a log excerpt or a log file path.".to_string(),
                used_tools: vec![],
            };
        }

        let mut bundle = ContextBundle::default();
        bundle.extra.push(format!("# Log to explain\n```\n{}\n```", log_content));
        let system = render(PromptScene::LogExplain, &bundle);

        let messages = vec![
            json!({ "role": "system", "content": system }),
            json!({ "role": "user", "content": "Explain the log above." }),
        ];

        let mut text = String::new();
        let result = stream_complete(
            &ctx.llm.base_url,
            &ctx.llm.api_key,
            &ctx.llm.model_id,
            &messages,
            |delta| {
                text.push_str(delta);
                if let Some(sink) = ctx.events {
                    let _ = sink.send(AgentEvent::Thought(delta.to_string()));
                }
            },
        );

        match result {
            Ok(t) => SkillResult {
                text: t,
                used_tools: vec!["read_file".to_string()],
            },
            Err(e) => SkillResult {
                text: format!("Error: {}", e),
                used_tools: vec![],
            },
        }
    }
}
