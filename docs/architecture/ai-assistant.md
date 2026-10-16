# Velowork AI 助手功能分析报告

> 分析范围:`crates/velowork-ai/`(纯逻辑层)+ `crates/velowork-app/src/views/panels/ai_assistant_panel`(UI 调用入口)
> 核心分层:`Provider`(模型连接) → `Agent`(编排) → `Skill`(工作流) → `Tool`(能力单元) → `System`(SSH/Terminal/File 运行时)

---

## 一、发送消息后的处理流程

UI 入口位于 `ai_assistant_panel/mod.rs` 的 `submit_message`,整体走"无 Agent 直接本地回复"路径(非 agent 模式时)。

### 1. 入队与流式消费

- `submit_message` 把用户消息写入 `messages` / `streaming_message`,并生成 `streaming_id`,随后调用 `local_reply(...)`。
- UI 通过 `cx.spawn` 异步接收 `AgentEvent` 流(`TextDelta` / `ThinkingDelta` / `ToolCall` / `ToolResult` / `Done` / `Error`),实时更新 `streaming_message.buffer` 与 `buffer_thinking`,并通过 `cx.notify()` 触发渲染。
- 特点:**UI 永不等待网络,所有状态变化经事件回主线程**(`runtime/mod.rs` 的架构原则)。

### 2. `local_reply` 编排(`lib.rs`)

对每条用户消息:

- 构建 `context_bundle`:收集聚焦终端屏幕、会话清单、配置文件、记忆(`build_context_bundle`)。
- 调用 `provider.stream_reply(...)` 拿到 `StreamEvent` 流。
- 循环消费事件:
  - `TextDelta` → 累积到 `buffer`。
  - `ThinkingDelta` → 累积到 `buffer_thinking`。
  - `ToolCall` / `ToolResult` → 原样回传 UI(UI 用 `render_tool_call` 展示工具卡)。
  - `Done` → 退出,记录本轮到记忆。
  - `Error` → 回传错误文案到 `replies`。

### 3. 多轮上下文压缩(关键)

- `lib.rs` 内 `compress_history` 调用 `context::compression::compress_chat_history`,按设置(`AiCompressionStrategy`)与 `max_context_tokens` / `max_history_messages` 阈值裁剪历史,再交给模型。
- 三种策略:
  - `SlidingWindow`:保留最近 N 条消息。
  - `TruncateOldest`:按 token 逐条删除最旧消息。
  - `Summarize`:对旧轮生成启发式摘要段落,保留近期消息。

### 4. 记忆回写

- 每轮结束后 `memory.record_command` / `record_directory` / `record_error` / `record_preference`,持久化到 `~/.config/velowork/ai_memory.json`(首次访问惰性建文件)。

---

## 二、如何调用工具

### 触发方式

LLM 通过 provider 的 tool-calling 协议返回 `ToolCall` 事件(含 `name` + `args` JSON)。`local_reply` 将 `ToolCall` / `ToolResult` 透传给 UI 展示,实际执行在 `agent::run_agent_turn`(agent 模式)或 skill 内部编排。

### 执行链路

1. `ToolRegistry::call(name, args, ctx, cx)` → `find` → `Tool::execute`(`tool/mod.rs`)。
2. 工具实现 `Tool` trait(`name` / `description` / `schema` / `execute`)。
3. 命令类工具 `run_terminal_command` 在 `execute` 内先过权限门禁 `ctx.permission.allows(cmd)`,拒绝则记 `record_error` 并返回 `PermissionDenied`。
4. 通过 `send_command_to_focused_terminal` 把字节写入聚焦终端 PTY(`terminal.send_bytes`)。

### 权限门禁(`runtime/mod.rs`)

- 三档:`Deny` / `ReadOnly` / `AllowAll`(UI 下拉可切换)。
- `is_readonly_command` 用 `READONLY_COMMANDS` 前缀白名单 + `DANGEROUS_TOKENS` 黑名单(含 `rm` / `sudo` / `mkfs` / `dd` / `curl` / `wget` 等)做启发式判定。
- 局限:基于**子串包含 + 前缀匹配**,对管道拼接、变量展开、反引号、别名绕过较脆弱。

### 内置工具(8 个)(`builtin_tools`)

| 工具 | 能力 | 权限 |
|---|---|---|
| `run_terminal_command` | 向聚焦终端发命令 | 受门禁 |
| `read_terminal_screen` | 读聚焦终端当前屏幕 | 无 |
| `list_sessions` | 列出所有会话及连接态 | 无 |
| `get_session_config` | 查单个会话配置 | 无 |
| `read_file` | 读文件 | 无 |
| `write_file` | 写文件 | 受门禁(实际由 AllowAll 才放行) |
| `search_logs` | 搜日志 | 无 |
| `generate_config` | 生成配置 | 无 |

### MCP 工具

`tool/mcp.rs` 支持拉取远程 MCP server 的 tool 列表并包装为 `Box<dyn Tool>`,通过 `McpClient` 经 `RequestBroker` 调用——即模型可调用外部 MCP 工具。

---

## 三、AI 能获取哪些信息

通过 `context::build_context_bundle` + 工具,信息来源:

1. **聚焦终端实时屏幕**(`read_focused_terminal`,取 `FocusManager` → `LayoutNode::Terminal` → 读滚动缓冲/可见区)。
2. **会话拓扑**:`list_sessions` / `get_session_config`(SSH/本地会话、连接状态、配置)。
3. **文件系统**:`read_file` / `write_file`(工作区可访问路径)。
4. **日志**:`search_logs`(跨会话/项目日志检索)。
5. **命令执行结果**:`run_terminal_command` 把命令发往 PTY,但**结果需再次 `read_terminal_screen` 读取**——工具不直接捕获 stdout。
6. **跨调用记忆**(`AiMemory`):主机(host)、目录(cwd)、最近命令、最近错误、用户偏好。
7. **MCP 外部数据**:取决于挂载的 MCP server。

### 关键缺口

- 工具调用结果是"发到终端 + 另行读取屏幕",非结构化 stdout 捕获,存在竞态(命令未执行完就读屏)。
- 无 `cwd` / 环境变量 / 进程列表的直接工具(只能靠 `run_terminal_command` 间接获取)。
- 上下文 bundle 默认不主动注入(需 LLM 主动调工具),首轮无"当前主机状态"背景。

---

## 四、有哪些技能(Skill)

`skill` 模块把多个 `Tool` 编排为可复用工作流,经 `SkillTool` 包装成 `Tool` 暴露给 LLM/MCP。内置 5 个(`builtin_skills`):

| 技能 | 分类 | 分层 | 说明 |
|---|---|---|---|
| `diagnose` 诊断 | Diagnose | Community | 读屏幕+日志,定位报错并给修复步骤 |
| `resource` 资源巡检 | Inspect | Community | 用 `free` / `ps` / `df` 等只读命令评估负载 |
| `command_gen` 命令生成 | Generate | Community | 据需求生成 shell 命令 |
| `script_deploy` 脚本部署 | Deploy | Pro | 生成运维脚本并写文件/发送 |
| `log_explain` 日志解释 | Explain | Community | 解释日志片段 |

### 架构亮点

- `Skill` trait 抽象清晰(`i18n_key` / `description` / `tier` / `category` / `run`)。
- `SkillTool` 只引用底层 `ToolRegistry`,**不嵌套其它 Skill**,避免递归环。
- `SkillRegistry` 预留 `register`,支持插件/企业版扩展。
- `SkillTier`(Community / Pro / Enterprise)为商业化分层打基础。

---

## 五、后期优化方向

### A. 流程与可靠性

1. **结构化命令执行结果**:`run_terminal_command` 改为直接捕获 PTY stdout(带超时/退出码),而非"发命令 + 另读屏幕",消除竞态与屏幕解析误差。
2. **工具调用闭环验证**:命令执行后自动 `read_terminal_screen` 一次并回灌模型,减少多轮空转。
3. **流式中断与重试**:当前 `StreamEvent` 无显式 cancel 透传;可复用 `runtime` 已有的 `CancelToken` / `RetryConfig` / `TimeoutConfig`(`task.rs`)做断点续传。

### B. 工具与权限

4. **权限引擎升级**:`is_readonly_command` 子串匹配易被绕过。建议改为轻量 AST/分词解析 + 只读命令白名单 + 危险操作二次确认(而非纯 deny)。
5. **增量工具集**:补充 `get_cwd` / `list_processes` / `read_env` / `search_files`(grep/find)等高频运维原语,降低对自然语言拼命令的依赖。
6. **MCP 工具缓存与错误隔离**:远程工具失败不应拖垮整轮对话。

### C. 上下文与记忆

7. **首轮主动背景注入**:`build_context_bundle` 默认带"当前主机/会话/cwd"摘要,减少模型反复调工具。
8. **记忆质量提升**:当前 `AiMemory` 仅记录原始命令/错误文本;可加语义聚类(自动归纳"该主机常犯 X 错")与 TTL 清理。
9. **压缩策略增强**:`Summarize` 当前是启发式截断(100 字符片段),可改为"调用模型生成真正摘要"或保留工具调用轨迹(比文本更省 token)。

### D. 技能与扩展

10. **技能可流式进度**:`SkillCtx.events` 已预留 `mpsc::Sender<AgentEvent>`(当前 `SkillTool` 传 `None`),接上后诊断/部署可实时展示"正在检查磁盘…"。
11. **技能市场雏形**:`SkillRegistry::register` + `SkillTier` 已就绪,下一步做"技能描述自动注入 system prompt + UI 商店式列举"。
12. **Agent 模式强化**:`agent::run_agent_turn` 是真正的"模型自决多步"循环,目前 UI 主路径走 `local_reply`(单轮)。可让复杂意图自动升级到 agent 模式。

### E. 成本与体验

13. **Token 估算校准**:`estimate_tokens` 是启发式(CJK≈1.5/ASCII≈4),与真实 tokenizer 有偏差,长对话压缩阈值易误判;可接模型方 tokenizer。
14. **工具结果长度上限**:`read_terminal_screen` / `search_logs` 返回可能过大,建议截断 + "展开看更多"机制,避免上下文爆炸。

---

## 六、近期已落地核心演进 (v0.1.0-beta.4 ~ beta.15)

1. **终端内联 AI (`TerminalInlineAi` / `Cmd+I` / `Ctrl+I`)**：
   - 彻底脱离侧栏依赖，以 Command Palette 单行输入条呼出，输入自然语言后展开黄金分割比结果卡片；
   - 包含意图自适应提示词（Intent-Adaptive Prompts），针对 NL-to-CLI、代码解释与错误排查提供专属精准推理，支持 `Tab`/`Enter` 一键上屏到 PTY。
2. **多模态 Vision 图像理解管道**：
   - 支持在 AI 输入框直接通过 `Ctrl+V` 粘贴系统截图与剪贴板图像；
   - 图像在后台异步执行格式校验、缩放与 Base64 编码，嵌入 `VisionMessage` 协议，并具备严格的内存配额熔断保护。
   - 附件列表提供等比自适应弹窗大图预览与系统原生图片工具调用。
3. **列表虚拟化与 SQLite 分页**：
   - 侧边栏消息渲染接入 `gpui::list` 视口虚拟化技术，结合底层 SQLite 分页拉取，彻底解决上千条长对话导致的内存膨胀与滚动掉帧。
4. **提示词安全防御与契约硬化**：
   - 落地五层系统提示词契约（System Prompt Contract），建立严格的证据优先级与防提示词注入（Prompt Injection）屏障，杜绝有害命令生成。

