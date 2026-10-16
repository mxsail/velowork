# Velowork UI 全局设计体系与工程规约

> **适用对象**：Velowork 研发团队及各类 AI 辅助编码应用（Antigravity, OpenCode, Claude Code, Cursor, Windsurf 等）。  
> **核心宗旨**：显著增加研发团队的确定性，消除不必要的设计纠结与样式割裂，保持系统一致性，让开发者专注于最具创意与业务价值的核心逻辑。

---

## 目录

1. [架构分层与设计哲学](#1-架构分层与设计哲学)
2. [色彩与透明度模型 (Colors & Translucency)](#2-色彩与透明度模型-colors--translucency)
3. [字体与排版系统 (Typography & Scale)](#3-字体与排版系统-typography--scale)
4. [图标体系与无障碍 (Iconography & Accessibility)](#4-图标体系与无障碍-iconography--accessibility)
5. [布局、间距与圆角 (Layout, Spacing & Radius)](#5-布局间距与圆角-layout-spacing--radius)
6. [阴影与层级体系 (Shadows & Elevation)](#6-阴影与层级体系-shadows--elevation)
7. [控件规格与通用组件库 (`crates/velowork-ui`)](#7-控件规格与通用组件库-cratesvelowork-ui)
8. [交互状态、焦点管理与事件防护](#8-交互状态焦点管理与事件防护)
9. [国际化规约 (`velowork-i18n`)](#9-国际化规约-velowork-i18n)
10. [反模式与防错代码对比 (Do's and Don'ts)](#10-反模式与防错代码对比-dos-and-donts)

---

## 1. 架构分层与设计哲学

Velowork 采用严格的单向依赖架构。UI 与视图层必须严格遵守以下层级关系，禁止反向依赖：

```
velowork-app (业务/视图聚合层)
   │
   ▼
velowork-views-* (专业视图层，如 terminal, sidebar, remote)
   │
   ▼
velowork-ui (通用组件层: Button, Select, RadioGroup, SimpleInput, etc.)
   │
   ▼
velowork-theme / velowork-core (核心层: ThemeColors, SemanticPalette, Design Tokens)
```

### 核心设计原则

1. **单一真实源 (Single Source of Truth)**：所有样式、间距、字号、圆角、颜色必须从 Design Tokens 与 `SemanticPalette` 导出，**严禁硬编码**。
2. **通透磨砂一致性 (Frosted Translucency)**：全平台窗口、弹窗、浮层与控件在任意透明度（`bg_opacity`）下，必须保持纯净同调的色调层次与高辨识度。
3. **确定性与无障碍 (Determinism & Accessibility)**：纯图标必带中文 Tooltip、所有面板支持拖拽缩放、所有文案支持国际化。

---

## 2. 色彩与透明度模型 (Colors & Translucency)

### 2.1 四套内置主题 (`ThemeColors`)

Velowork 内置 4 套开箱即用主题，定义于 `crates/velowork-core/src/theme/colors.rs`：

- **`DARK_THEME`**：暗色主题（深黑曜石碳灰低反光底色 `0x202326`、表面层 `0x2a2d31` 与经典蓝 `0x0078d4`）。
- **`LIGHT_THEME`**：明亮主题（现代白瓷工作区底色 `0xffffff`、表面层 `0xf6f8fa`、面板层 `0xf4f5f7`、天水蓝选区 `0xcde6ff` 与深海蓝 `0x0969da`）。
- **`PASTEL_DARK_THEME`**：柔和暗调主题（带微蓝紫调，护眼低对比）。
- **`HIGH_CONTRAST_THEME`**：高对比主题（纯黑白极高辨识度）。

> **注意**：新增主题字段时，必须同时在 4 套主题结构体中完整声明。

### 2.2 语义调色盘 (`SemanticPalette`)

在 UI 渲染中，一律使用语义化调色盘字段，禁止直接使用裸 Hex 颜色：

| 语义字段                 | 描述 / 计算规则                           | 典型应用场景                                            |
| :----------------------- | :---------------------------------------- | :------------------------------------------------------ |
| `p.surface_base`         | `with_alpha(t.bg_primary, opacity)`       | 主窗口背景、独立设置窗口基底、弹窗基底                  |
| `p.surface_raised`       | `with_alpha(t.bg_secondary, opacity)`     | 悬浮卡片、次级容器、步进器/数值展示框                   |
| `p.surface_card`         | `with_alpha(t.text_primary, 0.035)`       | 磨砂卡片层、控件静止底色、弹窗头部/底部区域             |
| `p.surface_selection`    | `with_alpha(t.bg_selection, opacity)`     | 列表选中行、分段按钮选中态、Tab 激活态                  |
| `p.surface_accent`       | `with_alpha(t.accent, (0.85 + 0.15*a)*a)` | 主操作按钮（保存）、协议切换选中 Tab、Switch 开关开启态 |
| `p.surface_accent_hover` | 亮度自适应动态 Hover                      | 强调色按钮/Tab 的悬停状态                               |
| `p.surface_overlay`      | `with_alpha(t.bg_primary, opacity)`       | 下拉菜单浮层、右键菜单浮层、Tooltip 容器                |
| `p.surface_hover`        | `with_alpha(t.bg_hover, opacity)`         | 列表行悬停、普通按钮悬停、分段项悬停                    |
| `p.border_subtle`        | `with_alpha(t.border, 0.5)`               | 通用细边框、卡片边框、分段控件分隔线                    |
| `p.border_active`        | `to_hsla(t.border_active)`                | 输入框聚焦边框、Tab 底部指示条、单选圆钮高亮边框        |
| `p.text_primary`         | `to_hsla(t.text_primary)`                 | 主标题、正文、激活态标签                                |
| `p.text_secondary`       | `to_hsla(t.text_secondary)`               | 次级标签、辅助文本、未选中 Tab                          |
| `p.text_muted`           | `to_hsla(t.text_muted)`                   | 禁用文本、快捷键提示说明、占位符 (Placeholder)          |
| `p.text_on_accent`       | 高对比黑/白文本                           | 位于 `surface_accent` 之上的文字与图标                  |

### 2.3 透明度三大硬性法则

1. **强制从 Context 派生**：在任何 `render` 函数或闭包中，**必须使用 `let p = SemanticPalette::from_context(cx);`**。严禁使用静态实色的 `from_theme(&t)`。
2. **底色单一来源原则**：主窗口或外层弹窗容器已定义透明底色时，内层内容容器严禁重复设置 `.bg(surface_bg(...))`，避免两层透明通道相乘导致色调变厚、发灰、失真。
3. **遮罩暗化联动**：模态暗化遮罩必须与透明度联动：`with_alpha(0x000000, 0.35 * bg_opacity(cx))`。

---

## 3. 字体与排版系统 (Typography & Scale)

Velowork 支持用户自定义 `ui_font_size`（界面字号微调）、`ui_scale`（全局缩放）以及终端等宽字体。排版必须统一使用缩放安全的 Token 函数。

### 3.1 字号阶梯速查表

```
+-----------------------------------------------------------------------------------+
| ui_text_2xl(cx) (22px) -> Hero 突出标题、主数据大字                                 |
| ui_text_xl(cx)  (18px) -> 弹窗大标题、一级模态头部                                   |
| ui_text_lg(cx)  (15px) -> 分组标题、面板 Section 标题                              |
| ui_text_md(cx)  (13px) -> [标准基准] 正文、按钮文字、菜单项、输入框文本              |
| ui_text_sm(cx)  (11px) -> 辅助说明、参数描述、次级 Hint                             |
| ui_text_xs(cx)  (10px) -> 角标 (Badge)、标签 (Tag)、状态小点文字                     |
+-----------------------------------------------------------------------------------+
```

### 3.2 字体族与代码规范

- **UI 界面文字**：默认使用系统高质量无衬线字体族，直接调用 `.child("文字")`。
- **数值与代码**：使用 `.font_family("monospace")` 或跟随终端配置字体。
- **自定义字号**：如需任意基准像素（如 14px），必须调用 `ui_text(14.0, cx)` 参与缩放流水线，**禁止直接写 `px(14.0)` 作为字号**。

---

## 4. 图标体系与无障碍 (Iconography & Accessibility)

### 4.1 图标尺寸阶梯 (`crates/velowork-ui/src/tokens/icon.rs`)

- **`ICON_SM` (10px)** / `ui_icon_sm(cx)`：行内微型指示符、下拉小箭头。
- **`ICON_MICRO` (12px)** / `ui_icon_micro(cx)`：文件树节点折叠/展开箭头。
- **`ICON_STD` (14px)** / `ui_icon_std(cx)`：**[标准推荐]** 普通按钮图标、行内操作图标。
- **`ICON_MD` (16px)** / `ui_icon_md(cx)`：设置项大图标、右键菜单项图标。
- **`ICON_LG` (20px)** / `ui_icon_lg(cx)`：主活动栏 (Activity Bar)、侧边导航大图标。

### 4.2 图标无障碍强制规约

- **所有纯图标按钮必须配备中文 Tooltip**：
  ```rust
  // 正确范例：
  IconButton::new("refresh-btn", AppIcon::Refresh)
      .tooltip(i18n!(cx, "common.refresh"))
  ```
- **图标配色语义化**：
  - 默认图标：`.text_color(p.text_secondary)`
  - 悬停图标：`.text_color(p.text_primary)`
  - 强调色背景上的图标：`.text_color(p.text_on_accent)`

---

## 5. 布局、间距与圆角 (Layout, Spacing & Radius)

### 5.1 间距 Token (`crates/velowork-ui/src/tokens/spacing.rs`)

| 常量        | 像素 | 缩放函数          | 典型应用场景                                        |
| :---------- | :--- | :---------------- | :-------------------------------------------------- |
| `SPACE_2XS` | 2px  | -                 | 微间距、分段按钮内部间隙 (`gap(SPACE_2XS)`)         |
| `SPACE_XS`  | 4px  | `ui_space_xs(cx)` | 紧凑间距、图标与文字微间隙                          |
| `SPACE_SM`  | 6px  | `ui_space_sm(cx)` | 列表项内边距、控件紧凑 padding                      |
| `SPACE_MD`  | 8px  | `ui_space_md(cx)` | **[标准间距]** 控件间距、表单项 gap、菜单项 padding |
| `SPACE_LG`  | 12px | `ui_space_lg(cx)` | 分组间距、面板内容 padding                          |
| `SPACE_XL`  | 16px | `ui_space_xl(cx)` | 模态弹窗内边距、对话框 padding                      |

### 5.2 圆角 Token (`crates/velowork-ui/src/tokens/radius.rs`)

- `RADIUS_XS` (2px)：紧凑微型角标 (`Badge`)。
- `RADIUS_MD` (3px)：标准角标、小尺寸分段项。
- `RADIUS_STD` (4px)：**[通用标准]** 按钮、输入框、普通卡片、Tab 选项卡。
- `RADIUS_LG` (8px)：下拉菜单容器、上下文菜单外壳、分组卡片。
- `RADIUS_XL` (10px)：模态弹窗外壳 (`ModalContainer`)。

### 5.3 面板与布局规约

- **多栏面板必须支持调整尺寸**：所有停靠面板（Sidebar, Dock, Pane）必须通过拖拽边框调整大小（`Resizable Panes`）。
- **水平与垂直流容器**：优先使用 `h_flex()` / `v_flex()` 并搭配 `.gap(SPACE_*)`。

---

## 6. 阴影与层级体系 (Shadows & Elevation)

### 6.1 阴影阶梯

- **`shadow_sm()`**：微浮起（滑块手柄、微型悬浮按钮）。
- **`shadow_md()`**：卡片悬浮、局部小面板。
- **`shadow_lg()`**：Tooltip 气泡、悬浮提示框。
- **`shadow_xl()`**：模态弹窗、右键菜单、下拉浮层。

### 6.2 焦点环光效 (Focus Ring)

输入框与交互控件聚焦时，必须使用**内嵌式焦点环**，不增加元素物理外边距，防止导致布局抖动：

```rust
// 统一调用:
let ring = focus_ring_shadows(&t);
div().track_focus(&focus_handle)
     .focus(move |s| s.border_color(p.border_active).shadow(ring))
```

---

## 7. 控件规格与通用组件库 (`crates/velowork-ui`)

### 7.1 控件三级尺寸体系 (`ControlSize`)

所有通用控件共享 3 级标准高度：

- **`ControlSize::Compact`**：高度 24px（用于紧凑工具栏、内嵌表格行）。
- **`ControlSize::Default`**：高度 28px（**[全局标准]** 用于表单、设置项、常规对话框）。
- **`ControlSize::Large`**：高度 36px（用于 Hero 区域、重点引导步骤）。

---

### 7.2 核心控件标准使用范式

#### A. 按钮 (`Button` / `button_*`)

```rust
use velowork_ui::button::{button, button_primary, button_danger};

// 1. 次级/普通按钮
button("cancel-btn", i18n!(cx, "dialog.cancel"), &t)
    .on_click(|_, _, cx| { ... });

// 2. 主行动点按钮（保存/确认，半透明强调色）
button_primary("save-btn", i18n!(cx, "dialog.save"), &t)
    .on_click(|_, _, cx| { ... });

// 3. 危险操作按钮（删除/重置）
button_danger("delete-btn", i18n!(cx, "dialog.delete"), &t)
    .on_click(|_, _, cx| { ... });
```

#### B. 单选与分段控件 (`RadioGroup`)

```rust
use velowork_ui::radio::{RadioGroup, RadioMode, RadioOption};

// 分段按钮模式 (Segmented Buttons)
RadioGroup::new("proxy-mode-radio")
    .mode(RadioMode::Button)
    .selected(Some(current_mode))
    .options(vec![
        RadioOption::new(Mode::Direct, i18n!(cx, "proxy.direct")),
        RadioOption::new(Mode::System, i18n!(cx, "proxy.system")),
    ])
    .on_change(move |val, window, cx| { ... });
```

#### C. 下拉选择器 (`Select` / `Dropdown`)

```rust
use velowork_ui::select::{Select, SelectState, SelectOption};

// 创建 SelectState
let select_state = cx.new(|cx| {
    SelectState::new(cx)
        .options(options)
        .selected(Some(selected_id))
});

// 渲染
Select::new(&select_state)
```

#### D. 开关 (`Switch`) 与滑块 (`Slider`)

```rust
use velowork_ui::switch::Switch;
use velowork_ui::slider::{Slider, SliderState};

// 切换开关
Switch::new("auto-update-toggle")
    .checked(is_enabled)
    .on_click(cx.listener(|this, checked: &bool, window, cx| { ... }));

// 滑块（支持透明度与百分比）
Slider::new(&slider_state)
```

#### E. 文本输入框 (`SimpleInput` / `SimpleInputState` / `Input`)

```rust
use velowork_ui::input::{Input, SimpleInputState};

// 创建状态
let input = cx.new(|cx| SimpleInputState::new(cx).placeholder("..."));

// 读取值
let val = input.read(cx).value(cx).to_string();

// 渲染
Input::new(&input)
```

---

## 8. 交互状态、焦点管理与事件防护

### 8.1 事件穿透防护（极其重要）

所有浮层、弹窗、下拉菜单和独立面板，必须在最外层容器声明以下防护，防止鼠标点击与滚轮事件穿透到底层终端或代码编辑器：

```rust
div()
    .occlude() // 阻挡下层视图命中测试
    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
```

### 8.2 键盘导航与焦点闭环

- 弹窗打开时，自动聚焦首个输入框或主操作按钮：`input.update(cx, |s, cx| s.focus(window, cx));`。
- 支持 `Escape` 键快速关闭弹窗/浮层。
- 分段控件 `RadioGroup` 支持 `ArrowLeft` / `ArrowRight` / `ArrowUp` / `ArrowDown` 方向键循环切换。

---

## 9. 国际化规约 (`velowork-i18n`)

- **严禁硬编码任何中英文字符串**。
- 翻译文件位于 `crates/velowork-i18n/locales/{en,zh}.json`，采用点号命名空间（例如 `settings.proxy_mode.label`）。
- 调用范式：
  ```rust
  use velowork_i18n::i18n;
  let label = i18n!(cx, "dialog.confirm"); // 返回 String
  ```
- 若主题变量 `t` 与 i18n 的 `t` 产生命名冲突，使用别名：
  ```rust
  use velowork_i18n::{t as i18n_t, i18n};
  ```

---

## 10. 反模式与防错代码对比 (Do's and Don'ts)

### 反模式 1：使用静态实色 Palette（导致透明度失效）

❌ **错误示范**：

```rust
let p = SemanticPalette::from_theme(&t); // 永远为 1.0 实色，透明度调节无效！
```

✅ **正确示范**：

```rust
let p = SemanticPalette::from_context(cx); // 自动读取当前全局 bg_opacity
```

---

### 反模式 2：双层透明背景叠加（导致变厚、发暗）

❌ **错误示范**：

```rust
// 外层 Overlay
div().bg(surface_bg(t.bg_primary, cx)).child(
    // 内层面板又写了一遍透明背景，两层 alpha 相乘！
    div().bg(surface_bg(t.bg_panel, cx)).child(...)
)
```

✅ **正确示范**：

```rust
// 外层定义单一透明底色，内层使用磨砂卡片层或纯透明容器
div().bg(surface_bg(t.bg_primary, cx)).child(
    div().bg(p.surface_card).child(...)
)
```

---

### 反模式 3：硬编码像素与色彩

❌ **错误示范**：

```rust
div()
    .p(px(16.0))
    .rounded(px(4.0))
    .bg(rgb(0x5B6BD6))
    .text_size(px(13.0))
```

✅ **正确示范**：

```rust
div()
    .p(SPACE_XL)
    .rounded(RADIUS_STD)
    .bg(p.surface_accent)
    .text_size(ui_text_md(cx))
```

---

### 反模式 4：纯图标按钮缺少 Tooltip

❌ **错误示范**：

```rust
div().child(AppIcon::Close.size(ICON_STD)) // 用户无法得知按钮含义
```

✅ **正确示范**：

```rust
IconButton::new("close-btn", AppIcon::Close)
    .tooltip(i18n!(cx, "common.close"))
```

---

### 反模式 5：浮层缺少防穿透拦截

❌ **错误示范**：

```rust
div().id("my-popover").child(...) // 点击会穿透并触发底层终端输入！
```

✅ **正确示范**：

```rust
div()
    .id("my-popover")
    .occlude()
    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    .child(...)
```

---

### 反模式 6：在 render 中重复初始化 Entity

❌ **错误示范**：

```rust
// 每次 re-render 都 new 一个 SliderState，导致拖拽手柄状态丢失并引发抖动
let slider = cx.new(|cx| SliderState::new(cx));
```

✅ **正确示范**：

```rust
// 状态保存在 View / Entity 字段中，render 中仅复用实体
Slider::new(&self.slider_state)
```
