//! ResourceInspectSkill（P0，Pro）：采集 CPU / 内存 / 磁盘 / 网络并生成巡检报告。
//!
//! 内部自动：把一组只读巡检命令发到聚焦终端（在远程主机上真实执行），
//! 再读取当前终端屏幕汇总成报告。命令受当前权限门禁控制。

use serde_json::json;

use crate::skill::{Skill, SkillCategory, SkillCtx, SkillResult, SkillTier};

/// 巡检命令（只读，安全）。
const INSPECT_COMMANDS: &[&str] = &[
    "uname -a",
    "uptime",
    "free -m",
    "df -h",
    "vmstat 1 2",
    "ss -tunlp 2>/dev/null | head -n 20",
];

pub struct ResourceInspectSkill;

impl Skill for ResourceInspectSkill {
    fn name(&self) -> &str {
        "resource_inspect"
    }
    fn i18n_key(&self) -> &str {
        "skill.resource_inspect"
    }
    fn description(&self) -> &str {
        "Collect a resource inspection report from the focused terminal's host: \
         OS/kernel, uptime/load, memory, disk usage, and listening sockets. \
         Runs a set of read-only commands and summarizes the output."
    }
    fn tier(&self) -> SkillTier {
        SkillTier::Pro
    }
    fn category(&self) -> SkillCategory {
        SkillCategory::Inspect
    }

    fn run(&self, _input: &str, ctx: &mut SkillCtx) -> SkillResult {
        let mut used = Vec::new();
        let mut blocked = Vec::new();

        for cmd in INSPECT_COMMANDS {
            let res = ctx.registry.call(
                "run_terminal_command",
                json!({ "command": cmd }),
                ctx.tool_ctx,
                ctx.cx,
            );
            match res {
                Ok(_) => used.push((*cmd).to_string()),
                Err(_) => blocked.push((*cmd).to_string()),
            }
        }

        // 命令已在远程执行；读取当前屏幕以汇总（输出可能仍在刷新）。
        let screen = ctx
            .registry
            .call("read_terminal_screen", json!({}), ctx.tool_ctx, ctx.cx)
            .unwrap_or_default();

        let mut report = String::from("## Resource Inspection\n\n");
        report.push_str(&format!(
            "The following read-only commands were sent to the focused terminal:\n```\n{}\n```\n\n",
            INSPECT_COMMANDS.join("\n")
        ));
        if !screen.trim().is_empty() {
            report.push_str(&format!("### Current terminal output\n```\n{}\n```\n", screen));
        }
        if !blocked.is_empty() {
            report.push_str(&format!(
                "\n> Note: the following commands were blocked by the current permission level:\n> `{}`\n",
                blocked.join("`, `")
            ));
        }
        report.push_str(
            "\nReview the output above for CPU load, memory pressure, disk usage, and listening services.",
        );

        SkillResult {
            text: report,
            used_tools: used,
        }
    }
}
