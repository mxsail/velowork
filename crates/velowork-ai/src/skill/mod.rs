//! 技能系统（Skill）：可复用的高层工作流编排层。
//!
//! 分层（自上而下）：
//! - **Agent**：理解用户意图、选择合适的 Skill、决定执行顺序（见 `agent` 模块）。
//! - **Skill**（本模块）：由多个 [`Tool`](crate::tool::Tool) 编排而成的可复用工作流，
//!   如诊断、巡检、部署、日志解释。产品级能力沉淀于此。
//! - **Tool**：最小能力单元（执行命令、读终端、读写文件、搜日志、生成配置）。
//! - **System**：SSH / SFTP / Terminal / 文件系统等底层运行时。
//!
//! 设计要点：
//! - `Skill` trait 从第一天就抽象，未来内置 / 第三方插件 / 企业版 Skill 统一实现它。
//! - `SkillTool` 把任意 `Skill` 包装为 [`Tool`]，使其可被 LLM / MCP 直接调用
//!   （产品级工作流对外暴露为工具）。Skill 内部只编排「底层工具」，不嵌套其它 Skill，
//!   避免递归与引用环。
//! - `SkillRegistry` 支持启动时注册内置技能，并预留 `register` 供插件 API 扩展。

pub mod command_gen;
pub mod diagnose;
pub mod log_explain;
pub mod memory;
pub mod resource;
pub mod script_deploy;

use std::sync::{Arc, Mutex};
use std::sync::mpsc;

use gpui::App;
use serde_json::json;

use crate::agent::AgentEvent;
use crate::provider::LlmConfig;
use crate::tool::{Tool, ToolCtx, ToolRegistry};

/// 技能商业化分层（决定内置 / Pro / Enterprise 归属，用于 UI 标注与收费点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillTier {
    Community,
    Pro,
    Enterprise,
}

impl SkillTier {
    pub fn label_key(self) -> &'static str {
        match self {
            SkillTier::Community => "skill.tier.community",
            SkillTier::Pro => "skill.tier.pro",
            SkillTier::Enterprise => "skill.tier.enterprise",
        }
    }
}

/// 技能能力分类（用于 UI 分组）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillCategory {
    Diagnose,
    Inspect,
    Generate,
    Deploy,
    Explain,
    Other,
}

/// 技能运行结果。
pub struct SkillResult {
    /// 最终回复文本（已渲染，可直接展示）。
    pub text: String,
    /// 本次运行实际用到的底层工具名（用于审计 / 展示）。
    pub used_tools: Vec<String>,
}

/// 技能运行上下文：持有底层工具表、运行时实体、记忆、模型配置与事件回传通道。
pub struct SkillCtx<'a> {
    /// 底层工具注册表（仅 Tool，不含其它 Skill）。
    pub registry: &'a ToolRegistry,
    /// 运行时实体与权限（命令执行类工具过门禁）。
    pub tool_ctx: &'a ToolCtx,
    /// 跨调用记忆（可读取主机 / 目录 / 最近命令 / 错误 / 偏好，并可写入）。
    pub memory: &'a mut memory::AiMemory,
    /// 模型连接配置（技能内部回调 LLM 用）。
    pub llm: &'a LlmConfig,
    /// gpui App 上下文（工具执行需要）。
    pub cx: &'a App,
    /// 可选事件回传（用于流式展示思考 / 进度）。
    pub events: Option<&'a mpsc::Sender<AgentEvent>>,
}

/// 技能抽象：可复用的高层工作流。
///
/// 需要 `Send + Sync`：技能会被包装进 [`SkillTool`]（`impl Tool`，需 `Send + Sync`），
/// 并随工具注册表移动到后台异步任务执行。
pub trait Skill: Send + Sync {
    /// 唯一标识（小写蛇形），同时作为对外暴露 Tool 的名称。
    fn name(&self) -> &str;
    /// UI 展示用的 i18n key（中文标签走 `i18n!`）。
    fn i18n_key(&self) -> &str;
    /// 面向 LLM 的技术描述（注入 system prompt / tool 定义，使用英文）。
    fn description(&self) -> &str;
    /// 商业化分层。
    fn tier(&self) -> SkillTier;
    /// 能力分类。
    fn category(&self) -> SkillCategory;
    /// 运行技能：编排 Tool / 调用 LLM，返回结构化结果。
    fn run(&self, input: &str, ctx: &mut SkillCtx) -> SkillResult;
}

/// 技能元数据（拥有的快照，便于 UI 列举而不借用 trait 对象）。
pub struct SkillMeta {
    pub name: String,
    pub i18n_key: String,
    pub description: String,
    pub tier: SkillTier,
    pub category: SkillCategory,
}

/// 技能注册表：聚合全部可用技能，按名称查找与列举。
pub struct SkillRegistry {
    skills: Vec<Box<dyn Skill>>,
}

impl SkillRegistry {
    pub fn new(skills: Vec<Box<dyn Skill>>) -> Self {
        Self { skills }
    }

    /// 按名称查找技能。
    pub fn find(&self, name: &str) -> Option<&dyn Skill> {
        self.skills
            .iter()
            .find(|s| s.name() == name)
            .map(|b| b.as_ref())
    }

    /// 列出全部已注册技能。
    pub fn list(&self) -> &[Box<dyn Skill>] {
        &self.skills
    }

    /// 导出元数据快照（供 UI 展示）。
    pub fn metas(&self) -> Vec<SkillMeta> {
        self.skills
            .iter()
            .map(|s| SkillMeta {
                name: s.name().to_string(),
                i18n_key: s.i18n_key().to_string(),
                description: s.description().to_string(),
                tier: s.tier(),
                category: s.category(),
            })
            .collect()
    }

    /// 按分层过滤。
    pub fn by_tier(&self, tier: SkillTier) -> Vec<&dyn Skill> {
        self.skills
            .iter()
            .filter(|s| s.tier() == tier)
            .map(|b| b.as_ref())
            .collect()
    }

    /// 插件 / 第三方可注册额外技能（预留 plugin-api 接入点）。
    pub fn register(&mut self, skill: Box<dyn Skill>) {
        self.skills.push(skill);
    }
}

/// 构造全部内置技能。
pub fn builtin_skills() -> Vec<Box<dyn Skill>> {
    vec![
        Box::new(diagnose::DiagnoseSkill),
        Box::new(resource::ResourceInspectSkill),
        Box::new(command_gen::CommandGenerateSkill),
        Box::new(script_deploy::ScriptDeploySkill),
        Box::new(log_explain::LogExplainSkill),
    ]
}

/// 将技能包装为 [`Tool`]，使其可被 LLM / MCP 直接调用。
///
/// 内部只引用「底层工具注册表」（不含其它 Skill），避免引用环与递归。
pub struct SkillTool {
    skill: Box<dyn Skill>,
    llm: LlmConfig,
    memory: Arc<Mutex<memory::AiMemory>>,
    registry: Arc<ToolRegistry>,
}

impl SkillTool {
    pub fn new(
        skill: Box<dyn Skill>,
        llm: LlmConfig,
        memory: Arc<Mutex<memory::AiMemory>>,
        registry: Arc<ToolRegistry>,
    ) -> Self {
        Self {
            skill,
            llm,
            memory,
            registry,
        }
    }
}

impl Tool for SkillTool {
    fn name(&self) -> &str {
        self.skill.name()
    }
    fn description(&self) -> &str {
        self.skill.description()
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "input": {
                    "type": "string",
                    "description": "The user request or context for this skill (e.g. the error text, or what to deploy)."
                }
            },
            "required": ["input"]
        })
    }
    fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolCtx,
        cx: &App,
    ) -> Result<String, crate::tool::ToolError> {
        let input = args
            .get("input")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mut mem = self.memory.lock().unwrap();
        let mut skill_ctx = SkillCtx {
            registry: &self.registry,
            tool_ctx: ctx,
            memory: &mut mem,
            llm: &self.llm,
            cx,
            events: None,
        };
        Ok(self.skill.run(&input, &mut skill_ctx).text)
    }
}
