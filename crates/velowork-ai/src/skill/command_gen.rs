//! CommandGenerateSkill（P0，Community）：自然语言生成 Shell 命令。
//!
//! 内部自动：结合记忆中的 shell 偏好与最近命令，调用 LLM 生成精确命令。

use serde_json::json;

use crate::agent::AgentEvent;
use crate::prompt::{render, ContextBundle, PromptScene};
use crate::provider::stream_complete;
use crate::skill::{Skill, SkillCategory, SkillCtx, SkillResult, SkillTier};

pub struct CommandGenerateSkill;

impl Skill for CommandGenerateSkill {
    fn name(&self) -> &str {
        "command_generate"
    }
    fn i18n_key(&self) -> &str {
        "skill.command_generate"
    }
    fn description(&self) -> &str {
        "Generate a precise shell command from a natural-language request. \
         Returns the command (in a code block) with minimal explanation. \
         Considers the user's shell preference and recent commands from memory."
    }
    fn tier(&self) -> SkillTier {
        SkillTier::Community
    }
    fn category(&self) -> SkillCategory {
        SkillCategory::Generate
    }

    fn run(&self, input: &str, ctx: &mut SkillCtx) -> SkillResult {
        let mut bundle = ContextBundle::default();
        let mem_ctx = ctx.memory.to_context_string();
        if !mem_ctx.trim().is_empty() {
            bundle.extra.push(format!("# Memory\n{}", mem_ctx));
        }
        let system = render(PromptScene::CommandGen, &bundle);

        let user = if input.trim().is_empty() {
            "Generate a useful shell command for the current session.".to_string()
        } else {
            input.to_string()
        };

        let messages = vec![
            json!({ "role": "system", "content": system }),
            json!({ "role": "user", "content": user }),
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
                used_tools: vec![],
            },
            Err(e) => SkillResult {
                text: format!("Error: {}", e),
                used_tools: vec![],
            },
        }
    }
}
