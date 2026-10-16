# Session Dialog 架构（Session Dialog Architecture）

> 优先级：★★★★★
> 目标：为「新建/编辑会话」配置对话框（`session_dialog/` 模块）建立**架构基准**，明确
> Registry / Event / Descriptor / Model / Renderer 之间的**职责边界与依赖方向**，
> 从源头上避免随着 SSH 功能、插件、AI 能力增加，再次出现 `SessionPanel`（现约 7500 行）膨胀。
>
> 本文是实现与后续新增功能的**契约**：任何新增分组、字段、解析器、模板、Advisor
> 都必须遵循此处定义的扩展点，不得绕过 Registry 直接向 `SessionPanel` 追加内联状态。

---

## 0. 问题诊断（重构前）

当前 `crates/velowork-app/src/views/panels/session_panel.rs`：

| 症状 | 现状证据 | 后果 |
| --- | --- | --- |
| 状态内联 | `struct SessionPanel` 内联 60+ 个 `Entity<SimpleInputState>` 与散状态（`session_*_input` / `session_auth_type` / `session_gex_*` …，行 258–337） | 每加一个字段就改结构体 + `new` + `load` + `reset` + `build` + `submit` 五处 |
| 渲染硬编码 | `SshDialogTab` 枚举（行 129）驱动 `render_tab_*` 竖向 9 标签切页 | 新增分组要改 enum + match + 渲染 + 搜索 + 摘要多处 |
| 领域态与瞬态混杂 | 持久化字段（host/port…）与 UI 态（tab/展开/校验）同处一实体 | 保存逻辑要手动挑字段，易漏易错 |
| 校验弱且字符串化 | `submit_dialog`（行 2375）仅提交时查 name/host/username 非空，写 `validation_error: Option<String>` | 无逐字段、无 Severity、无内联提示 |

**结论**：把「会话配置对话框」从 `SessionPanel` 抽出为独立的 `session_dialog/` 模块，
以**声明式 Registry + 领域/瞬态分离 + 事件驱动缓存**组织，`SessionPanel` 仅保留外壳。

---

## 1. 分层与依赖方向（总览）

依赖**单向向下**，上层可依赖下层，下层严禁反向依赖上层（避免环）：

```
┌─────────────────────────────────────────────────────────────┐
│  L5  Shell 外壳     SessionPanel (render_dialog_overlay)       │  仅持有 Option<SessionDialogModel>，委托渲染
├─────────────────────────────────────────────────────────────┤
│  L4  Renderer 渲染  mod.rs / sections/*.rs                     │  读 Model + Registry → 产出 AnyElement
├─────────────────────────────────────────────────────────────┤
│  L3  Registry 注册  SectionRegistry / AdvisorRegistry         │  元数据 + 派发表；启动装配一次
│                     ConnectionParserPipeline / TemplateProvider│
├─────────────────────────────────────────────────────────────┤
│  L2  State 状态     SessionDialogModel                         │  config/baseline(领域) + ui(瞬态) + inputs + undo_stack
├─────────────────────────────────────────────────────────────┤
│  L1  Domain 领域    SshSession (velowork-state)               │  纯数据，可序列化，唯一持久化载体
└─────────────────────────────────────────────────────────────┘

  横切能力（被 L2/L4 调用，自身不持有 UI 状态）：
    parser.rs  validation.rs  summary.rs  changeset.rs  section.rs(类型)  advisor.rs(类型)
```

**核心约束（不可违反）：**

1. **L1 领域态**（`SshSession`）不感知任何 UI 概念（不含展开态、校验态、active tab）。
2. **L4 Renderer 无副作用**：渲染函数只读 `Model` 与 `Registry` 缓存，**绝不**在渲染期做校验/派生/写状态。所有派生在**事件回调**里完成（见 §5）。
3. **扩展只加不改**：新增分组/解析器/模板/Advisor = 向对应 Registry `register` 一个条目，不改已有 `match`。

---

## 2. Descriptor —— 声明式元数据

`section.rs`。描述符是**纯数据**，不含行为（无 `render` 指针、无 `Box<dyn Advisor>`）。

```rust
pub struct SectionDescriptor {
    pub id: SshSection,                    // 稳定标识（枚举）
    pub title_key: &'static str,           // i18n key，禁止硬编码文案
    pub icon: &'static str,                // "icons/*.svg"
    pub default_expand: bool,              // 基础/连接/认证/终端 = true
    pub search_keywords: &'static [&'static str], // 搜索命中词（含中英）
}
```

| 职责 | 说明 |
| --- | --- |
| 承载 | 分组的静态元数据（身份、标题、图标、默认展开、搜索词） |
| 不承载 | 渲染逻辑、Advisor 实例、校验规则、摘要计算（这些属于 Renderer/Registry/横切能力） |
| 依赖 | 仅依赖 `SshSection` 枚举与 i18n key（字符串），零运行时依赖 |

> **为什么 Descriptor 不含 render/advisor**：函数指针与 `Box<dyn>` 内嵌进 const 会使描述符无法作为纯数据被插件/序列化复用，也把「元数据」与「行为」耦死。行为交给 Registry 按 `id` 派发。

---

## 3. Registry —— 运行时装配与派发

所有 Registry 在 `mod.rs` 的对话框初始化时**装配一次**（非每帧），随 `SessionDialogModel` 生命周期存在（或作为进程级静态）。它们是 Descriptor（数据）与行为（函数/trait 对象）之间的**绑定层**。

### 3.1 SectionRegistry

```rust
pub type SectionRenderFn =
    fn(&mut SessionDialogModel, &mut SessionPanel, &mut Window, &mut App) -> gpui::AnyElement;

pub struct SectionRegistry {
    descriptors: Vec<SectionDescriptor>,
    renderers:   HashMap<SshSection, SectionRenderFn>,
    advisor_ids: HashMap<SshSection, &'static str>,
}
```

| 方法 | 职责 |
| --- | --- |
| `register(desc, render_fn, advisor_id)` | 装配一个分组：元数据 + 渲染函数 + 关联 Advisor id |
| `iter()` | 按声明顺序遍历（驱动左导航与右侧卡片顺序） |
| `render(id, model, panel, w, cx)` | 按 `id` 派发到对应 `SectionRenderFn` |
| `match_search(keyword)` | 用 `search_keywords` 过滤命中的分组 |
| `advisor_id(id)` | 查该分组关联的 Advisor id，交给 `AdvisorRegistry` |

**唯一的分组事实来源**：导航渲染、卡片渲染、搜索过滤、摘要索引全部走 `SectionRegistry`，杜绝多处 `match SshSection`。

### 3.2 AdvisorRegistry

```rust
pub trait SectionAdvisor {
    fn id(&self) -> &'static str;
    fn label(&self, cx: &App) -> String;                 // i18n
    fn analyze(&self, model: &SessionDialogModel) -> Vec<Suggestion>;
    fn apply(&self, model: &mut SessionDialogModel, cx: &mut App);
}
pub struct AdvisorRegistry { advisors: Vec<Box<dyn SectionAdvisor>>, /* id→index */ }
```

| 职责 | 说明 |
| --- | --- |
| 注册/查找 | `register(Box<dyn SectionAdvisor>)`；`get_for_section(id)` 经 `SectionRegistry::advisor_id` 反查 |
| 策略替换 | `DefaultAdvisor`（本地规则，如 Ubuntu→Curve25519/Ed25519/ChaCha20/Compression Off）先落地；未来 `OpenAIAdvisor` / `CompanyAdvisor` / `CloudAdvisor` 仅 `register` 即生效，**UI 不改** |
| 命名统一 | 后台 trait 与 UI 按钮均用「Advisor / Recommendations」命名 |

### 3.3 ConnectionParserPipeline

```rust
pub trait ConnectionParser { fn parse(&self, raw: &str) -> Option<ParsedTarget>; }
pub struct ConnectionParserPipeline { parsers: Vec<Box<dyn ConnectionParser>> }
```

| 职责 | 说明 |
| --- | --- |
| 链式解析 | 依序尝试 `OpenSshParser` → `UriParser`(ssh://, scp://, sftp://) → `ScpParser` → `SftpParser` → `FallbackParser`，命中即返回 `ParsedTarget { username?, host, port? }` |
| 扩展 | 未来 `mosh://` = 新增一个 `ConnectionParser` push 进 pipeline，不改已有解析器 |
| 纯函数 | 无状态、无副作用；由 Host 输入的事件回调调用，回填 username/port |

### 3.4 TemplateProvider

```rust
pub trait TemplateProvider {
    fn templates(&self) -> Vec<TemplateMeta>;
    fn apply(&self, id: &str, model: &mut SessionDialogModel, cx: &mut App);
}
```

`BuiltinTemplateProvider`（Linux 默认 / 跳板机 / 数据库服务器 / 开发环境）先落地；`apply` 前先 `push_undo`。未来 Company/Plugin/AI Provider 即插即用。

> **Registry 共性**：都是「注册即扩展」的开放结构，构成本架构对抗膨胀的核心手段。

---

## 4. Model —— UIState / DomainState 分离

`model.rs`。是对话框的**唯一可变状态容器**，明确切分为「领域态」「瞬态」「输入」「撤销栈」。

```rust
pub struct SessionDialogModel {
    pub config:     SshSession,             // ① 领域态：唯一持久化载体
    pub baseline:   SshSession,             // ② ChangeSet 基准（load 时快照）
    pub inputs:     SessionDialogInputs,    // ③ 聚合输入，避免 Model 膨胀
    pub ui:         SessionDialogUiState,   // ④ 瞬态：保存时丢弃
    pub undo_stack: Vec<SshSession>,        // ⑤ 撤销：config 快照
}

pub struct SessionDialogUiState {
    pub expanded_sections: HashSet<SshSection>,
    pub active_section:    SshSection,
    pub nav_search:        String,
    pub validation:        Vec<Option<ValidationResult>>, // 按 FieldId discriminant 索引
    pub summaries:         [Summary; SECTION_COUNT],       // 事件驱动缓存（见 §5）
    pub change_set:        ChangeSet,                       // 事件驱动缓存
    pub hover:             Option<SshSection>,
}
```

| 成员 | 层 | 生命周期 | 说明 |
| --- | --- | --- | --- |
| `config` | 领域 | 持久化 | 保存时 `build_ssh_session` 只读它 |
| `baseline` | 领域 | 对话框内 | 与 `config` 对比派生 ChangeSet；「已修改」判断基准 |
| `inputs` | 聚合 | 对话框内 | 所有 `Entity<SimpleInputState>` 收进 `SessionDialogInputs`，Model 不直接堆输入字段 |
| `ui` | 瞬态 | 对话框内 | 展开/active/搜索/校验/摘要/变更/hover，**保存时全部丢弃** |
| `undo_stack` | 瞬态 | 对话框内 | Template/Advisor/Import apply 前 push |

**Model 提供的核心方法（唯一状态入口）：**

| 方法 | 职责 |
| --- | --- |
| `new(cx)` / `load(session, cx)` | 构造/载入：建 inputs、设 `config`+`baseline`、默认展开 4 组、`recompute()` 初始化缓存 |
| `to_session_config()` | 从 `config` 产出持久化对象（只碰领域态） |
| `is_valid()` | `ui.validation` 中无 `Error` 即 true（Warning/Info 不阻断） |
| `mark_dirty()` | 置脏，触发 `recompute_change_set` |
| `push_undo()` | `undo_stack.push(config.clone())` |
| `recompute()` | 事件驱动统一入口：`recompute_summaries` + `recompute_change_set`（见 §5） |

> **SessionDialogInputs（`inputs.rs`）**：聚合全部 `Entity<SimpleInputState>` + `new(cx)` 创建 + 统一订阅 `InputChangedEvent`。它把「输入实体」这一噪声从 Model 顶层剥离，是防膨胀的关键收纳层。

---

## 5. Event —— 事件驱动的派生与缓存

**核心原则：派生只在事件里发生，渲染只读缓存。** 契合 GPUI 状态驱动模型，杜绝每帧重复计算。

### 5.1 事件源

| 事件 | 触发者 | 处理链 |
| --- | --- | --- |
| `InputChangedEvent` | `SessionDialogInputs` 各输入 | `validate_field(FieldId)` → （Host 时）`ConnectionParserPipeline::parse` 回填 → `sync config` → `mark_dirty` → `recompute()` → `cx.notify()` |
| 开关/下拉变更 | checkbox/dropdown 回调 | `sync config` → `mark_dirty` → `recompute()` → `cx.notify()` |
| 模板应用 | `TemplateProvider::apply` | `push_undo` → 写 `config` + 刷 inputs → `recompute()` → `cx.notify()` |
| Advisor 应用 | `SectionAdvisor::apply` | `push_undo` → 写 `config` + 刷 inputs → `recompute()` → `cx.notify()` |
| Undo | `undo_last_apply` | 弹栈恢复 `config` + 刷 inputs → `recompute()` → `cx.notify()` |
| 导航点击/搜索 | 左导航 | 设 `active_section` / 展开 / `scroll_to` / 更新 `nav_search`（不动领域态） |
| 提交 | footer Save | `validate_all` → 有 Error 则展开首个错误组+横幅+禁用；否则 `build_ssh_session` |

### 5.2 派生函数（横切，无 UI 状态）

| 模块 | 函数 | 输入 → 输出 | 写入 |
| --- | --- | --- | --- |
| `validation.rs` | `validate_field(id, model)` / `validate_all(model)` | 字段值 → `ValidationResult` | `ui.validation[FieldId]` |
| `summary.rs` | `recompute_summaries(model)` | `config`+`validation`+`change_set` → `[Summary; N]` | `ui.summaries` |
| `changeset.rs` | `recompute_change_set(model)` | `config` vs `baseline` → `ChangeSet` | `ui.change_set` |
| `parser.rs` | `pipeline.parse(raw)` | 原始文本 → `ParsedTarget` | 回填到 `config`/inputs |

### 5.3 校验类型（FieldId + Severity）

```rust
pub enum FieldId { Name, Host, Port, Username, KeepaliveInterval, KeepaliveMax,
                   IdleTimeout, ChannelBuffer, GexMin, GexPreferred, GexMax, RekeyTime, /* ... */ }
pub enum ValidationSeverity { Info, Warning, Error }     // 仅 Error 阻断保存
pub struct ValidationResult { pub severity: ValidationSeverity, pub message: String }
```

- 校验状态用 **`Vec<Option<ValidationResult>>` 按 `FieldId` discriminant 索引**，取代字符串键 `HashMap`（类型安全、零字符串）。
- 分级示例：Host 空 = `Error`；Compression ON / SHA1 HostKey = `Warning`；Port 65535 = `Info`。
- Advisor 可读取 `ui.validation` 生成建议（校验结果是建议的输入之一）。

### 5.4 Summary / ChangeSet 对象（非 String）

```rust
pub enum SectionStatus { Ok, Error, Modified, Active, None }
pub struct Summary { pub title: String, pub subtitle: Option<String>,
                     pub status: SectionStatus, pub icon: Option<&'static str>,
                     pub badges: Vec<Badge> }               // Encrypted/Verified/SavedPassword…
pub struct ChangeSet { pub changed_fields: HashSet<FieldId>,
                       pub changed_sections: HashSet<SshSection>, pub change_count: usize }
```

---

## 6. Renderer —— 只读渲染

`mod.rs` + `sections/*.rs`。**无副作用**：只读 `Model` 与 `Registry` 缓存产出 `AnyElement`。

| 单元 | 职责 |
| --- | --- |
| `mod.rs :: render_session_dialog` | 顶层布局：左导航（状态+搜索+Summary+变更计数） + 右滚动可折叠卡片 + footer；遍历 `SectionRegistry::iter()` |
| `sections/{basic,connection,authentication,terminal,network,security,advanced,notes}.rs` | 各 `SectionRenderFn`：渲染该组字段、读 `ui.summaries[id]` 作 Header 摘要、读 `ui.validation` 画内联错误、挂 Advisor 按钮 |

**Renderer 禁止**：调用 `validate_*` / `recompute_*` / 写 `config` / 写 `ui`。它们只在 §5 的事件回调里发生。渲染读到的一切派生结果都来自缓存。

**响应式**：宽屏（dialog_width ≥ 720）左导航 ~200px + 右侧双列；窄屏左导航收顶部横条 + 字段 `w_full`。

---

## 7. 依赖矩阵（谁能依赖谁）

| ↓依赖 \ 被依赖→ | Domain | Model | validation/parser/summary/changeset | section/advisor(类型) | Registry | Renderer | Shell |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **Domain** (`SshSession`) | — | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| **横切** (validate/parse/summary/changeset) | ✓ | ✓(读) | — | ✓ | ✗ | ✗ | ✗ |
| **类型** (section/advisor) | ✓ | ✓(签名) | ✓ | — | ✗ | ✗ | ✗ |
| **Registry** | ✓ | ✓ | ✓ | ✓ | — | ✗ | ✗ |
| **Renderer** | ✓ | ✓(读) | ✓(读缓存) | ✓ | ✓ | — | ✓(SessionPanel 引用) |
| **Shell** (`SessionPanel`) | ✓ | ✓ | — | — | ✓ | ✓ | — |

✓=允许依赖，✗=禁止（反向或跨层）。**领域态零依赖**是本架构的地基。

---

## 8. 扩展指南（新增功能时照此做，勿改旧代码）

| 需求 | 只需 | 禁止 |
| --- | --- | --- |
| 新增分组（如 Mosh/端口转发） | 加 `SectionDescriptor` + 一个 `SectionRenderFn`，`SectionRegistry::register` | 改已有 `match SshSection` / 往 `SessionPanel` 加字段 |
| 新增字段 | 在 `SshSession`(serde default) + `SessionDialogInputs` + `FieldId` + 对应 section 渲染 + `validate_field` 分支 | 往 `SessionPanel` 内联输入实体 |
| 新增解析格式 | 实现 `ConnectionParser`，push 进 pipeline | 改已有 parser |
| 新增模板来源 | 实现 `TemplateProvider`，注册 | 改 `BuiltinTemplateProvider` |
| 接入 AI/企业建议 | 实现 `SectionAdvisor`，`AdvisorRegistry::register` | 改 UI/按钮 |
| 新增摘要徽章 | 扩 `BadgeKind` + 在 `recompute_summaries` 产出 | 在渲染期临时拼字符串 |

---

## 9. 与 russh 0.62.2 的领域对齐（数据契约）

`SshSession`（`crates/velowork-state/src/ssh_sessions.rs`）是领域态，须与 russh `client::Config` 对齐：

- **保留** `gex_min/gex_preferred/gex_max` → `GexParams::for_client_config(min,pref,max)`（client 端要求 min ≥ 2048）。
- **新增** `rekey_time: u32`（默认 3600）→ `limits.rekey_time_limit`。
- **新增** 终端布尔 `shell_integration/bracketed_paste/osc52_clipboard/true_color` 与 `algorithms_automatic`，均 `#[serde(default)]`。
- 接线点：`pty_manager.rs` 两处 `connect`（约 1846 / 2099）在 `session_config` 块内补 `gex` 与 `limits`。

新增字段一律 `serde(default)`，旧配置自动回退；`SshSessionConfig`/存储结构不变。

---

## 10. 目录与职责映射

```
session_dialog/
  mod.rs         # 装配 Registry(注册 8 组) + render_session_dialog 顶层布局；对外出口
  model.rs       # SessionDialogModel / SessionDialogUiState + new/load/to_config/is_valid/mark_dirty/push_undo/recompute
  inputs.rs      # SessionDialogInputs 聚合全部 SimpleInputState + 订阅 InputChangedEvent
  section.rs     # SshSection / SectionDescriptor(纯元数据) / SectionRegistry / SectionStatus / Summary / Badge
  advisor.rs     # SectionAdvisor trait / DefaultAdvisor / AdvisorRegistry / Suggestion
  validation.rs  # FieldId / ValidationSeverity / ValidationResult / validate_field / validate_all
  parser.rs      # ConnectionParser trait / 各 Parser / ConnectionParserPipeline / ParsedTarget
  summary.rs     # recompute_summaries(事件驱动缓存)
  changeset.rs   # recompute_change_set(config vs baseline)
  template.rs    # TemplateProvider / BuiltinTemplateProvider / TemplateMeta
  undo.rs        # 快照 undo_last_apply
  sections/      # basic/connection/authentication/terminal/network/security/advanced/notes 各 SectionRenderFn(~300 行)
```

`session_panel.rs`：仅保留 `Option<SessionDialogModel>` 与 `render_dialog_overlay` 委托渲染（+ Dirty/ChangeSet 标题与关闭确认），移除 `SshDialogTab` / `render_tab_*` 与已迁移字段。

---

## 11. V2（不在本轮实现，仅登记）

- **Connection Preview Card**：标题下 SSH/user/port/key/auth 摘要卡。
- **Connection Health**：DNS/Latency/Reachable/Key Algo 实时 + Test Connection + 未来 AI 分析。

两者均作为新分组/新组件挂入既有 Registry，不影响本文定义的核心结构。
