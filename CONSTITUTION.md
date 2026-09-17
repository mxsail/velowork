# CONSTITUTION.md — Velowork 核心宪法

> **版本**：v1.0.0  
> **地位**：本文档定义 Velowork 项目的**最高原则与核心红线**，具备最高优先权。所有 AI Coding 助手及开发者的代码变更必须严格服从本文档，任何与本文档冲突的实现均视为违规。

---

## 5 大不可逾越核心红线 (The 5 Inviolable Red Lines)

### 红线 1：零 Panic 防御 (Zero-Panic Policy)
- **绝对禁止**在生产 GUI 渲染、事件响应、PTY/网络数据解析流中使用裸 `unwrap()` 或 `expect()`。
- 任何可能失败的操作（解析错误、缺失状态、空指针、网络中断）必须使用 `if let`、`let-else`、模式匹配或 `?` 进行显式处理与优雅降级。
- 客户端绝不能因为单个终端会话或非关键组件的异常而导致整个应用窗口闪退崩溃。

### 红线 2：UI 线程严禁阻塞 (Zero UI Blocking)
- GPUI 主渲染线程（UI Thread）必须保持 60/120fps 平滑帧率。
- **绝对禁止**在 UI 线程或 `render()` 函数中执行阻塞式磁盘 I/O、同步网络请求、慢速系统调用或高开销密集计算。
- 所有耗时操作必须调度至后台执行器（`cx.background_executor()`）或 Tokio 运行时后台任务，并通过 Channel 或 GPUI 异步通知机制更新状态。

### 红线 3：浮动层必防穿透 (Occlusion & Stop Propagation)
- 所有弹出层、模态框、下拉菜单、Tooltip、右键菜单最外层容器必须显式添加 `.occlude()`。
- 浮动层容器必须拦截鼠标按下事件：`on_mouse_down(|_, _, cx| cx.stop_propagation())`，严禁点击事件穿透到下层的终端或工作区。

### 红线 4：样式、尺寸与文本必走规范 (Token & i18n Enforcement)
- **绝对禁止**在代码中直接使用裸十六进制颜色（如 `#1e1e1e`）或硬编码像素尺寸（如 `px(16.0)`）。
- 颜色必须使用 `SemanticPalette::from_context(cx)` 获取语义色彩；间距/圆角/图标必须使用 `velowork_ui::tokens::*`（`SPACE_*`, `RADIUS_*`, `ICON_*`, `ui_text_*`）。
- **绝对禁止**在界面中硬编码可见英文字符串；所有文本必须通过 `i18n!(cx, "...")` 宏加载。
- 纯图标按钮在鼠标悬停时**必须**具备中文 Tooltip。

### 红线 5：严格单向分层依赖 (Strict Layering)
- 代码库严格遵循单向依赖流向：
  $$\text{velowork-app} \longrightarrow \text{velowork-views-*} \longrightarrow \text{velowork-ui} \longrightarrow \text{velowork-theme} \longrightarrow \text{velowork-core}$$
- 下层 crate 严禁反向依赖上层 crate；核心层 `velowork-core` 严禁依赖任何其他内部 crate。
- 所有可通用的 UI 控件必须下沉并在 `crates/velowork-ui` 中创建并复用，禁止在业务 views 中重复造轮子。

### 红线 6：渲染期零副作用与焦点事件驱动 (Event-Driven Focus Only)
- **绝对禁止**在 `render()` 或 `paint()` 等 GUI 渲染周期方法中引入主动转移焦点的副作用（如 `cx.focus()` / `window.focus()` / 轮询标志位抢焦）。
- 焦点变更必须是**纯事件驱动（Event-Driven）**的，只能由用户交互事件回调（`on_action`、`on_mouse_down`、键盘按键、面板折叠委托等）显式触发。
- 派生聚焦状态（如面板是否处于聚焦态）必须直接基于 GPUI 焦点树实时派生（`focus_handle.contains_focused(window, cx)`），严禁自建状态双写。

### 红线 7：编码前必核验分支与主干闭环 (Verify Branch & Immediate Trunk Integration)
- AI Agent 与开发者在编写任何业务代码或执行 `git commit` 前，必须首先核验当前所在分支（`git branch --show-current`）。
- **严禁在 `backup-*`、`detached HEAD` 或未经授权的保护分支上进行任何业务编码与提交**。
- 所有新功能、优化与修复必须严格从最新的 `main` 主干分支检出 `feat/*`、`fix/*`、`perf/*` 等临时特性分支后方可修改代码。
- 特性分支自测通过后遵循短生命周期即时合入本地 `main`，发版与推送前**强制执行 `git branch --no-merged main` 审计防漏**。
- **严禁在未经用户明确显式要求前擅自执行 `git push`**。

---

## 核心架构原则

1. **安全第一 (Safety First)**：充分发挥 Rust 所有权与类型系统优势，严格限制 `unsafe`，凭据（密码、私钥）必须通过 OS Keyring 安全存储。
2. **显式优于隐式 (Explicit over Implicit)**：状态变更通过 GPUI Entity 显式通知（`cx.notify()`），避免隐晦的副作用与竞态条件。
3. **可维护性与测试驱动 (Testability & Verification)**：核心业务状态与协议流转必须具备单元测试，提交前必须通过 `cargo check`、`cargo clippy` 与 `cargo test`。
