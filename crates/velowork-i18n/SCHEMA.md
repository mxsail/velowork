# Velowork 国际化 (i18n) 信息架构规范

> **地位**：本规范是 Velowork 国际化资源组织与 Key 命名约定的唯一标准参考，旨在防止长周期迭代下的结构劣化与碎片化。

---

## 核心设计哲学与原则

### 1. 顶层命名空间 = 核心产品领域 / 稳定 UI 外壳模块
顶层 Namespace 严禁因局部组件或对话框的临时需求而碎片化（例如严禁出现 `add_project`、`session_dialog`、`folder_context_menu` 等瞬态命名）。所有相关功能必须收敛至对应的业务领域或 UI 容器根节点下。

### 2. 界面文字绝不用作 Key（Semantic Action IDs）
动作与命令必须使用语义化 ID（如 `commands.new_session.label`），严禁使用英文原文作为 Key（如 `"Quit": "退出"`）。

### 3. 三层正交结构模型
键层级必须按照：
```text
<product_domain>.<submodule_or_entity>.<element/field/action>
```
例如：
- `project.editor.title`
- `project.switcher.search_placeholder`
- `session.dialog.host`
- `settings.terminal.cursor_blink`
- `ai.ghost_text.tab_accept`

### 4. 双语强对称性保障
`crates/velowork-i18n/locales/zh.json` 与 `en.json` 的 Key 树必须保持严格的 100% 结构一致，由编译期与单元测试门禁（`test_locales_keys_match`）强制校验。

---

## 顶层命名空间注册表 (Top-level Namespaces)

当前系统收敛为以下 38 个稳定顶层命名空间：

| 命名空间 | 类别 | 职责与范围 | 典型子键 / 结构示例 |
| :--- | :--- | :--- | :--- |
| `app` | 应用元数据 | 应用名称、版本标识等全局静态信息 | `app.name` |
| `common` | 通用词汇 | 纯高频、跨模块通用的动作与状态动词（如确认、取消、保存） | `common.action.cancel`, `common.requires_restart` |
| `profile` | 配置管理 | 多 Profile 配置文件隔离与切换 | `profile.manager.title`, `profile.manager.delete_confirm` |
| `workspace` | 工作区容器 | 多窗口管理、工作树、目录折叠与状态 | `workspace.explorer`, `workspace.folder.*`, `workspace.window.*` |
| `session` | 会话领域 | SSH/Serial/Telnet/Local 会话创建、导入、编辑与列表 | `session.dialog.*`, `session.import.*`, `session.category.*` |
| `project` | 项目领域 | 项目工程组织、切换器、配置编辑器与导入导出 | `project.switcher.*`, `project.editor.*`, `project.export.*`, `project.import.*` |
| `terminal` | 终端运行态 | 运行态终端标签、上下文菜单、分屏操作与状态 | `terminal.split_horizontal`, `terminal.context_menu.*` |
| `sftp` | 文件传输与管理 | SFTP 浏览、权限编辑、属性查看与操作菜单 | `sftp.panel.*`, `sftp.dialog.*`, `sftp.toolbar.*` |
| `tunnel` | SSH 隧道 | 端口转发规则、编辑与生命周期监听 | `tunnel.type_local`, `tunnel.delete_confirm`, `tunnel.tooltip.*` |
| `service` | 服务监控 | 远程主机守护进程与容器状态监控 | `service.add`, `service.status.*`, `service.start_command` |
| `quick_commands` | 快捷指令 | 快捷指令与变量模板库 | `quick_commands.title`, `quick_commands.variable.*` |
| `command_history` | 历史命令 | 终端历史命令搜索与持久化治理 | `command_history.search_placeholder`, `command_history.clear_all` |
| `ai` | AI 与智能辅助 | 智能助手面板、Agent 工具调用、流式思考与 GhostText | `ai.title`, `ai.thinking`, `ai.ghost_text.*` |
| `skill` | 智能技能系统 | 终端错误诊断、命令生成与部署工作流技能 | `skill.diagnose.*`, `skill.resource_inspect.*` |
| `dock` | 停靠面板系统 | 面板停靠状态、Dock 标题、全屏与拆分操作 | `dock.panel.*`, `dock.tab.*`, `dock.action.*` |
| `search` | 全局搜索 | 快速跳转、文件查找与内容全局匹配检索 | `search.command_palette.*`, `search.file_search.*`, `search.content_search.*` |
| `settings` | 全局偏好设置 | 分类偏好设置面板（通用、外观、终端、安全、AI 等） | `settings.terminal.*`, `settings.nav.*`, `settings.data_storage.*` |
| `dialog` | 弹窗通用外壳 | 提示弹窗标题、系统级操作对话框共通文案 | `dialog.confirm`, `dialog.title` |
| `overlay` | 浮窗与蒙层 | 浮动层容器、状态感知与蒙层状态 | `overlay.close`, `overlay.detach` |
| `menu` | 菜单栏系统 | 顶部主菜单项（文件、编辑、视图、窗口、帮助等） | `menu.file`, `menu.new_window`, `menu.settings` |
| `titlebar` | 窗口标题栏 | 标题栏控件、控制按钮 Tooltip 与状态指示 | `titlebar.minimize`, `titlebar.toggle_left_dock` |
| `status_bar` | 底部状态栏 | 系统性能监视（CPU/内存）、网络编码与快捷开关 | `status_bar.cpu`, `status_bar.encoding` |
| `context_menu` | 右键上下文菜单 | 项目树与通用上下文菜单动作集 | `context_menu.pin`, `context_menu.folder.*` |
| `toolbar` | 工具栏 | 辅助快捷操作工具栏动作 | `toolbar.quick_actions` |
| `toast` | 浮动通知提示 | 快捷操作反馈提示文案 | `toast.copied`, `toast.saved` |
| `ssh` | SSH 底层协议 | 证书、身份认证、Known Hosts 与指纹确认 | `ssh.auth.*`, `ssh.fingerprint.*` |
| `theme` | 配色主题 | 应用 UI 主题与色彩配置元信息 | `theme.selector_title`, `theme.desc_dark` |
| `terminal_color_schemes` | 终端配色方案 | 终端仿真颜色方案管理器 | `terminal_color_schemes.manage_button`, `terminal_color_schemes.preset.*` |
| `lock_screen` | 应用锁屏 | 隐私保护与锁屏状态验证 | `lock_screen.title`, `lock_screen.unlock` |
| `update` | 软件版本更新 | 自动检查更新、下载与更新日志 | `update.checking`, `update.found_new` |
| `about` | 关于界面 | 软件元信息、版权与版本展示 | `about.title`, `about.version` |
| `help` | 帮助与引导 | 使用指南与快捷键概览 | `help.quick_start`, `help.docs_url` |
| `welcome` | 欢迎指引界面 | 初始无项目状态指引与快速上手面板 | `welcome.quick_start`, `welcome.recent_sessions` |
| `log` | 系统日志 | 应用运行时诊断与日志输出 | `log.export`, `log.filter` |
| `transfers` | 传输管理器 | 集中管理后台文件上传下载任务进度与状态 | `transfers.label`, `transfers.pause_all`, `transfers.clear_done` |
| `keybindings` | 快捷键配置 | 自定义快捷键绑定与分类 | `keybindings.record`, `keybindings.reset` |
| `commands` | 命令总注册表 | 全局 71 项 Action 的标准化语义定义（标签与描述） | `commands.<action_id>.label`, `commands.category.<id>` |

---

## 新增 i18n Key 的最佳实践

### 1. 新增功能时的命名检查清单
1. **是否存在对应的顶层领域？** 优先归入已有 38 个顶级领域之一。
2. **严禁新增瞬态根键**：例如若需要添加“SSH 证书导出”，应放入 `ssh.cert.export` 或 `session.export`，严禁在顶层创建 `ssh_cert_export`。
3. **参数占位符**：动态插值必须使用标准命名占位符（如 `{name}`、`{count}`、`{error}`），严禁使用 `{}` 无名占位符。
4. **两端同步更新**：在 `zh.json` 中新增键时，必须同步在 `en.json` 中添加对应英文翻译。

### 2. 代码自检
提交前必须通过以下两项验证门禁：
```bash
cargo test -p velowork-i18n --lib
cargo check
```
其中 `test_codebase_i18n_keys_validity` 会全量扫描源码中的所有 `i18n!` 调用，一旦存在缺失或拼写错误的 Key 将直接中断编译测试。
