//! ScriptDeploySkill（P1，Pro）：生成脚本、上传、赋权、执行、验证的完整运维工作流。
//!
//! 内部自动：调用 LLM 生成 bash 脚本 → 写入临时文件（`write_file`）→
//! 发送执行命令到聚焦终端（`run_terminal_command`，受权限门禁）→ 读取屏幕验证。

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::agent::AgentEvent;
use crate::prompt::{render, ContextBundle, PromptScene};
use crate::provider::stream_complete;
use crate::skill::{Skill, SkillCategory, SkillCtx, SkillResult, SkillTier};

pub struct ScriptDeploySkill;

impl Skill for ScriptDeploySkill {
    fn name(&self) -> &str {
        "script_deploy"
    }
    fn i18n_key(&self) -> &str {
        "skill.script_deploy"
    }
    fn description(&self) -> &str {
        "Generate a deployment script from a request, write it to a temp file, \
         make it executable, run it on the focused terminal, and verify the output. \
         Execution is gated by the current permission level."
    }
    fn tier(&self) -> SkillTier {
        SkillTier::Pro
    }
    fn category(&self) -> SkillCategory {
        SkillCategory::Deploy
    }

    fn run(&self, input: &str, ctx: &mut SkillCtx) -> SkillResult {
        let mut used = Vec::new();

        let user = if input.trim().is_empty() {
            "Generate a robust bash deployment script.".to_string()
        } else {
            format!(
                "Generate a robust, idempotent bash deployment script for the following request:\n{}",
                input
            )
        };
        let system = render(PromptScene::ShellScript, &ContextBundle::default());

        let messages = vec![
            json!({ "role": "system", "content": system }),
            json!({ "role": "user", "content": user }),
        ];

        let mut script_text = String::new();
        let generated = stream_complete(
            &ctx.llm.base_url,
            &ctx.llm.api_key,
            &ctx.llm.model_id,
            &messages,
            |delta| {
                script_text.push_str(delta);
                if let Some(sink) = ctx.events {
                    let _ = sink.send(AgentEvent::Thought(delta.to_string()));
                }
            },
        );

        let script_text = match generated {
            Ok(t) => t,
            Err(e) => {
                return SkillResult {
                    text: format!("Failed to generate script: {}", e),
                    used_tools: used,
                }
            }
        };

        // 去掉可能的 markdown 代码围栏，保留纯脚本。
        let script = strip_code_fence(&script_text);

        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("velowork_deploy_{}.sh", ts));
        let path_str = path.to_string_lossy().to_string();

        let write_res = ctx.registry.call(
            "write_file",
            json!({ "path": path_str, "content": script }),
            ctx.tool_ctx,
            ctx.cx,
        );
        match &write_res {
            Ok(_) => used.push("write_file".to_string()),
            Err(e) => {
                return SkillResult {
                    text: format!("Failed to write script to {}: {}", path_str, e),
                    used_tools: used,
                }
            }
        }

        // 赋权并执行。
        let exec_cmd = format!("chmod +x {} && bash {}", path_str, path_str);
        let exec_res = ctx.registry.call(
            "run_terminal_command",
            json!({ "command": exec_cmd }),
            ctx.tool_ctx,
            ctx.cx,
        );
        match &exec_res {
            Ok(_) => used.push("run_terminal_command".to_string()),
            Err(e) => {
                return SkillResult {
                    text: format!(
                        "Script written to {} but execution was blocked: {}\n\nScript:\n```bash\n{}\n```",
                        path_str, e, script
                    ),
                    used_tools: used,
                }
            }
        }

        let screen = ctx
            .registry
            .call("read_terminal_screen", json!({}), ctx.tool_ctx, ctx.cx)
            .unwrap_or_default();
        used.push("read_terminal_screen".to_string());

        let mut out = format!(
            "## Script Deploy\n\nScript written to `{}` and executed on the focused terminal.\n\n### Script\n```bash\n{}\n```\n",
            path_str, script
        );
        if !screen.trim().is_empty() {
            out.push_str(&format!("\n### Verification (terminal output)\n```\n{}\n```\n", screen));
        }

        SkillResult {
            text: out,
            used_tools: used,
        }
    }
}

/// 去掉 ```bash ... ``` 之类的代码围栏，返回纯脚本内容。
fn strip_code_fence(s: &str) -> String {
    let trimmed = s.trim();
    if let Some(stripped) = trimmed.strip_prefix("```") {
        // 去掉首行的语言标识（如 bash）
        let rest = stripped.strip_prefix("bash").unwrap_or(stripped);
        if let Some(end) = rest.rfind("```") {
            return rest[..end].trim().to_string();
        }
        return rest.trim().to_string();
    }
    trimmed.to_string()
}
