//! DiagnoseSkill（P0，Community）：分析终端错误并给出修复建议。
//!
//! 内部自动：读取聚焦终端屏幕 → 结合记忆中的最近错误 → 调用 LLM 诊断根因并给修复。

use serde_json::json;

use crate::agent::AgentEvent;
use crate::prompt::{render, ContextBundle, PromptScene};
use crate::provider::stream_complete;
use crate::skill::{Skill, SkillCategory, SkillCtx, SkillResult, SkillTier};

pub struct DiagnoseSkill;

impl Skill for DiagnoseSkill {
    fn name(&self) -> &str {
        "diagnose"
    }
    fn i18n_key(&self) -> &str {
        "skill.diagnose"
    }
    fn description(&self) -> &str {
        "Diagnose a terminal error or failure and propose a concrete fix. \
         Provide the error text or describe the problem; the skill reads the focused \
         terminal screen, recalls recent errors from memory, and asks the model for the \
         root cause plus a step-by-step fix."
    }
    fn tier(&self) -> SkillTier {
        SkillTier::Community
    }
    fn category(&self) -> SkillCategory {
        SkillCategory::Diagnose
    }

    fn run(&self, input: &str, ctx: &mut SkillCtx) -> SkillResult {
        let screen = ctx
            .registry
            .call("read_terminal_screen", json!({}), ctx.tool_ctx, ctx.cx)
            .unwrap_or_default();

        let mut bundle = ContextBundle::default();
        let mem_ctx = ctx.memory.to_context_string();
        if !mem_ctx.trim().is_empty() {
            bundle.extra.push(format!("# Memory\n{}", mem_ctx));
        }
        if !screen.trim().is_empty() {
            bundle.terminal_screen = Some(screen);
        }
        let system = render(PromptScene::ErrorDiagnosis, &bundle);

        let user = if input.trim().is_empty() {
            "The terminal shows an error. Diagnose the root cause and suggest a concrete fix.".to_string()
        } else {
            format!(
                "Problem reported by the user:\n{}\n\nDiagnose the root cause and suggest a concrete, step-by-step fix.",
                input
            )
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
            Ok(t) => {
                if !input.trim().is_empty() {
                    ctx.memory.record_error(input);
                }
                SkillResult {
                    text: t,
                    used_tools: vec!["read_terminal_screen".to_string()],
                }
            }
            Err(e) => SkillResult {
                text: format!("Error: {}", e),
                used_tools: vec![],
            },
        }
    }
}
