# 全局状态管理架构（Global State Architecture）

> 优先级：★★★★★
> 目标：解决因组件深度嵌套导致的状态流转混乱，以及过度依赖
> `cx.notify()` / `cx.emit()` / `cx.update()` 造成的"更新源头不可追踪"。

---

## 1. 问题诊断

### 1.1 现状（重构前）

`Workspace` 是一个**上帝对象（god object）**，把互相独立的状态域全部塞进一个 GPUI Entity：

| 状态域      | 当前载体                                          | 问题                               |
| ----------- | ------------------------------------------------- | ---------------------------------- |
| SSH 会话树  | `Workspace.ssh_sessions: SshSessionConfig`        | 纯数据，与项目布局无关却同处一实体 |
| 项目 / 布局 | `Workspace.data: WorkspaceData`                   | 与 SSH 共用一个 `cx.notify()` 广播 |
| 焦点        | `FocusManager`（每窗口一个，已从 Workspace 移出） | ✅ 已独立                          |
| 设置        | `Entity<SettingsState>`（Global）                 | ✅ 已近似 Store                    |
| 主题        | `Entity<AppTheme>`（Global）                      | ✅ 已近似 Store                    |
| 连接态      | `Workspace.active_ssh_sessions: HashSet`          | 与 SSH 树混在一起                  |

### 1.2 "连锁更新"与"不可追踪"的根因

以 **SessionPanel 修改一个 SSH 会话** 为例，旧链路是：

```
SessionPanel 直接改 Workspace.ssh_sessions.tree（甚至局部闭包 rename_in_tree + 直接 save_ssh_sessions）
  └─ Workspace 的 6 个 ssh_* mutator 全部以裸 cx.notify() 结尾（广播整个 Workspace）
       │
       ├─ (A) SessionPanel 自身：render 里 read(workspace) 自动 observe → 重渲染
       ├─ (B) 任何 render 里 read(workspace).ssh_sessions 的组件（terminal tab 标签）→ 重渲染
       └─ (C) 所有 cx.observe(&workspace) 的组件（status_bar / service_panel / sftp_panel / ...）
              → 被无差别唤醒（即使它们根本不读 ssh）
```

三个核心病灶：

1. **状态域未隔离**：SSH 会话、项目布局、连接态共享一个实体与一个 `notify` 通道。
2. **更新源头不可追踪**：没有 `SessionEvent` 之类的细粒度事件。一个组件只知道"Workspace 变了"，不知道"哪个 SSH 会话的哪个字段变了"——只能 `read()` 全量重算。
3. **UI 直接持有/修改业务数据**：`SessionPanel` 里有 `rename_in_tree(&mut ws.ssh_sessions.tree, ...)` 这种**绕开所有 mutator、直接改字段 + 直接落盘**的写法，是"不可追踪"的活样本。

---

## 2. 架构总览

```
┌─────────────────────────────────────────────────────────────┐
│                      AppState  (Global)                    │  ← 唯一的聚合根，不含任何数据
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐ │
│  │ Session   │ │Connection│ │ Settings  │ │  Theme    │ │
│  │ Store     │ │ Store    │ │ Store     │ │  Store    │ │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘ │
│  ┌──────────┐ ┌──────────┐                          │
│  │ Window    │ │  Focus    │                          │
│  │ Store     │ │  Store    │                          │
│  └──────────┘ └──────────┘                          │
└─────────────────────────────────────────────────────────────┘
        │ 每个 Store 都是独立 Entity<XxxStore>
        │ 只有该 Store 能改自己的数据
        ▼
┌─────────────────────────────────────────────────────────────┐
│  UI 层（SessionPanel / MainView / Editor / Terminal …）  │
│  • 只读：store.read(cx).query()                        │
│  • 只触发：store.update(cx, |s, cx| s.mutate(...))    │
│  • 响应：cx.subscribe(&store, |_, e, cx| match e {...})│
│  • 绝不持有业务数据、绝不自己改业务数据                │
└─────────────────────────────────────────────────────────────┘
```

数据流是**单向且可追踪**的：

```
用户交互（点击/输入）
   │
   ▼
UI 调用 store.mutate(...)        ← 唯一写入口，全工程只有这里能改该域
   │
   ▼
Store 内部：改字段 → 落盘 → cx.emit(XxxEvent::具体变更) → cx.notify()
   │
   ▼
关心该变更的 UI 通过 cx.subscribe(&store, ...) 收到精确事件
   │
   ▼
只重渲染受影响的局部
```

---

## 3. 严格的状态域划分

六大独立状态模块，每个都是 `impl Global` 的 `Entity`，是**各自域唯一的所有者与唯一写入者**：

| Store               | 拥有数据                                          | 职责边界                                                  |
| ------------------- | ------------------------------------------------- | --------------------------------------------------------- |
| **SessionStore**    | `SshSessionConfig`（SSH 会话树）                  | 会话/文件夹的增删改、折叠态、持久化到 `ssh_sessions.json` |
| **ConnectionStore** | `active_ssh_sessions: HashSet` + 连接信息表       | 哪些 SSH 会话当前有活跃连接；连接/断开事件                |
| **SettingsStore**   | `AppSettings`（复用现有 `Entity<SettingsState>`） | 全局设置，带 clamp + 防抖落盘                             |
| **ThemeStore**      | `AppTheme`（复用现有 `Entity<AppTheme>`）         | 主题模式、配色、自定义主题                                |
| **WindowStore**     | `HashMap<WindowId, WindowEntry>`                  | 每个窗口的 bounds / 激活 / 焦点状态                       |
| **FocusStore**      | `HashMap<WindowId, Entity<FocusManager>>`         | 应用级焦点层权威；持有每窗口 `FocusManager` 并向其委托    |

> **为何复用 SettingsState / AppTheme 而非新建包装**：二者本身已是良好的 Store
> （带 typed setter、`save_and_notify`、可被 observe）。强行包装只会增加无谓的
> 间接层。它们直接作为 `AppState.settings` / `AppState.theme`，仍是"单一写入者"。

每个 Store 都定义自己的 **typed event**：

```rust
pub enum SessionEvent {
    NodeAdded,
    SessionUpdated(String),     // 携带 session id，消费者可精确判断
    NodeRemoved,
    FolderToggled(String),
    AllFoldersCollapsed(bool),
    NodeRenamed(String),
    TreeReplaced,
}
impl EventEmitter<SessionEvent> for SessionStore {}
```

相比旧的"整个 Workspace 广播一个 `notify`"，现在 `terminal tab` 只需：

```rust
cx.subscribe(&session_store, |_, event, window, cx| match event {
    SessionEvent::SessionUpdated(id) if id == self.session_id => self.rerender_tab(window, cx),
    _ => {}
})
```

**只有真正相关、且知道"哪个会话变了"的消费者才重渲染**——连锁更新消失。

---

## 4. 职责分离（硬约束）

| 规则                    | 说明                                                                                    | 违规示例（已修复）                                                 |
| ----------------------- | --------------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| UI 不得持有业务数据     | 组件 struct 里不出现 `SshSessionConfig` / `AppSettings` 等业务字段                      | —                                                                  |
| UI 不得直接改业务数据   | 禁止 `ws.ssh_sessions.tree.push(...)`、`rename_in_tree(&mut ws.ssh_sessions.tree, ...)` | `SessionPanel` 旧 `rename_in_tree` 闭包 + 直接 `save_ssh_sessions` |
| UI 只触发，不决定持久化 | 落盘逻辑只在 Store 内                                                                   | 旧 `toggle_folder` 里直接 `save_ssh_sessions(&ws.ssh_sessions)`    |
| 所有写都过 Store        | 任何业务变更只能调 `store.mutate(...)`                                                  | 旧 6 个 `Workspace.ssh_*` mutator 各自 `cx.notify()`               |

> 一致性约束由"Store 是唯一 `pub` 可写入口"强制：业务字段在 Store 内是私有的，
> UI 只能走 `query()` / `mutate()`。编译期即阻止 UI 直接改数据。

---

## 5. 数据流向（可追溯）

每一次业务状态变更都满足：

1. **唯一写入口**：只有 `XxxStore::mutate_*` 方法能改该域数据（字段私有）。
2. **显式落盘**：持久化只在 Store 内发生（如 `SessionStore::persist_and_emit`）。
3. **typed 事件**：`cx.emit(XxxEvent::具体变更)` 携带足够上下文（id 等）。
4. **精确订阅**：消费者用 `cx.subscribe` 而非 `cx.observe` 全量刷新。

由此，任意一条状态的"从哪来、改了什么、谁关心"都可从 `XxxEvent` 的发射点与 `subscribe` 点静态追溯——解决了"更新源头不可追踪"。

---

## 6. 参考实现：SessionPanel 修改 → 不再连锁

重构后链路：

```
SessionPanel 对话框"保存"
  └─ SessionStore::update_session(session, cx)        ← 唯一写入口
        ├─ 改树（update_session_in_tree）
        ├─ save_ssh_sessions(&config)                 ← 落盘只在 Store 内
        ├─ cx.emit(SessionEvent::SessionUpdated(id))    ← typed 事件
        └─ cx.notify()

消费者（各自只关心自己的事件）：
  • SessionPanel：subscribe(SessionEvent) → 局部刷新树
  • terminal tab：subscribe(SessionEvent::SessionUpdated(id))，id 匹配才刷新标题
  • status_bar / service_panel / sftp_panel：根本不订阅 SessionStore → 不再被唤醒
```

与旧链路对比：`Workspace` 不再持有 `ssh_sessions`，因此"改 SSH"**根本不会**触发 `Workspace` 的 `notify`——所有 `observe(&workspace)` 的组件自然静默。

---

## 7. 迁移路径（落地顺序）

1. ✅ **SessionStore（完整迁移）**：`Workspace.ssh_sessions` 字段删除，数据所有权与全部 mutator 迁入 `SessionStore`；`session_panel` / `tabs` 改从 `GlobalSessionStore` 读写；旧的 `rename_in_tree` 局部闭包 + 直接落盘被替换为 `SessionStore::rename_node`。
2. ✅ **ConnectionStore（完整迁移）**：`Workspace.active_ssh_sessions` 迁入；`mark_ssh_session_connected/disconnected`、`is_ssh_session_connected` 改为委托 `ConnectionStore`。
3. ✅ **WindowStore / FocusStore**：结构已建立并**已完整接线**（不再是死代码）。
   - `FocusStore`：每个 `WindowView` 在 `WindowView::new` 创建时把自身的 `Entity<FocusManager>` 注册进 `GlobalFocusStore`，成为应用级每窗口焦点管理器注册表（`FocusStore::manager(window_id)` 可查）。
   - `WindowStore`：`WindowView::new` 创建时注册窗口（带实时 `bounds`）；既有的 `observe_window_bounds` 钩子在每个 move/resize 时把最新 bounds 推入 `WindowStore::set_bounds`，成为窗口几何/激活态的单一可追踪写点。
   - **OS 焦点变化**：`WindowView::new` 内通过 `cx.observe_window_activation(window, …)` 订阅，回调里用 `window.is_window_active()` 判断激活态，激活时写入 `WindowStore::set_focused(id, true)` + `FocusStore::set_window_focused(id)`，失活时写入 `WindowStore::set_focused(id, false)`。这是窗口焦点态的**单一可追踪写点**。
   - **窗口关闭**：`WindowView::new` 内通过 `window.on_window_should_close(cx, …)` 订阅，在 OS 关闭请求（Cmd+W / 关闭按钮 / 程序退出）时调用 `WindowStore::close(id)` 并放行（`true`）。因为回调直接持有 velowork `WindowId`（Copy），无需 gpui `WindowId` → velowork `WindowId` 的额外映射，写点与 register/set_bounds 同处一处。
   - 接线统一用 `cx.try_global::<GlobalXxxStore>()` + 先 `.clone()` Entity 再 `.update` 的安全惯用法，避免在全局未设置（部分 test harness）时 panic，也规避 `try_global` 借用与 `.update(cx)` 的借用冲突。
4. ⬜ **SettingsStore / ThemeStore**：已通过复用现有 `SettingsState` / `AppTheme` 接入 `AppState`；后续若有跨域派生状态（如"主题变化 → 重算 token 字号"）统一经 Store 事件触发，不再靠 `cx.observe_global` 盲刷。

### 逐步迁移原则

- 每迁移一个域，先让旧 `Workspace` 对应方法**委托**到新 Store（保持其他调用点不破），再择机删除旧方法。
- 禁止"半吊子"双写：`Workspace` 不再保留业务字段的 mirror，数据只存在于对应 Store。
- 每个 Store 提供一个 `GlobalXxx` 包装，使低层 crate（如 `velowork-views-terminal`）无需依赖 `velowork-app` 即可读写。

---

## 8. 校验

- `cargo check -p velowork-workspace -p velowork-app -p velowork-views-terminal`
- `cargo test -p velowork-workspace`（`Workspace` 既有测试仍通过，证明移除 ssh 字段未破坏契约）
- 实跑：在 SessionPanel 改一个会话名 → 只有对应 terminal tab 标题刷新，侧边栏/状态栏/编辑器**不再**无谓重渲染。
