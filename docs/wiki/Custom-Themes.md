# 自定义主题与外观定制实操手册 (Custom Themes & Appearance)

Velowork 将极致的工程师实用主义与现代桌面美学（Aesthetics）融为一体。本手册详细介绍如何切换预设主题、编写自定义 JSON 调色板、调节窗口毛玻璃与背景透明度、定制标题栏对齐位置以及配置终端沉浸式壁纸。

---

## 1. 内置主题切换与快捷选择器 (Built-in Themes)

按下快捷键 `Cmd+K Cmd+T` (macOS) / `Ctrl+K Ctrl+T` (Linux/Windows)，即可唤起全应用浮动主题选择器：

- **Dark（经典深色）**：现代专业暗色调，具备极致的视觉反差与抗视觉疲劳特性，适合长时间运维排障；
- **Light（经典浅色）**：高明度纯净白底，适合白天明亮办公光照环境；
- **Pastel Dark（柔和低饱和深色）**：调低对比度与纯色饱和度，呈现温润优雅的质感；
- **High Contrast（无障碍高对比）**：针对特殊弱光或强光户外环境设计，文字与边界轮廓锐利分明。

---

## 2. 编写自定义 JSON 主题 (Custom JSON Themes)

Velowork 支持加载外部自定义 JSON 主题文件。所有自定义主题放置在当前 Profile 的 themes 目录下：

- **macOS**: `~/Library/Application Support/velowork/profiles/default/themes/`
- **Linux**: `~/.config/velowork/profiles/default/themes/`
- **Windows**: `%APPDATA%\velowork\profiles\default\themes\`

### 2.1 主题文件范例 (`my-custom-theme.json`)
```json
{
  "name": "Cyberpunk Neon",
  "author": "Velowork User",
  "appearance": "Dark",
  "terminal": {
    "foreground": "#E0E6ED",
    "background": "#141729",
    "cursor": "#FF007F",
    "selection_background": "#2E3B5B",
    "selection_foreground": "#FFFFFF",
    "ansi": {
      "black": "#1B1E34",
      "red": "#FF5370",
      "green": "#C3E88D",
      "yellow": "#FFCB6B",
      "blue": "#82AAFF",
      "magenta": "#C792EA",
      "cyan": "#89DDFF",
      "white": "#EEFFFF",
      "bright_black": "#546E7A",
      "bright_red": "#FF5370",
      "bright_green": "#C3E88D",
      "bright_yellow": "#FFCB6B",
      "bright_blue": "#82AAFF",
      "bright_magenta": "#C792EA",
      "bright_cyan": "#89DDFF",
      "bright_white": "#FFFFFF"
    }
  },
  "ui": {
    "surface_background": "#0F111E",
    "surface_card": "#181B2C",
    "surface_hover": "#22263D",
    "border_subtle": "#282D46",
    "accent": "#00E5FF",
    "text_primary": "#F1F5F9",
    "text_secondary": "#94A3B8",
    "text_muted": "#64748B"
  }
}
```

### 2.2 加载与生效
保存文件后，在 **设置 -> 外观 -> 主题方案** 中选择 `Custom`，并在自定义主题列表中选取您的主题名称，界面即刻实现热重载刷新。

---

## 3. 窗口背景透明度与防晕染机制 (Translucency & Opacity)

### 3.1 调节背景不透明度 (`bg_opacity`)
- 在 **设置 -> 外观** 中拖动 **窗口背景不透明度 (Background Opacity)** 滑块（范围 `0.1` 至 `1.0`）；
- 结合操作系统毛玻璃磨砂特效（macOS Vibrant Blur、Windows 11 Mica / Acrylic、Linux Wayland 模糊合成器），呈现通透且具有景深质感的现代桌面风范。

### 3.2 复合 Alpha 防晕染回退机制 (Alpha Compounding Fallback)
- 在窗口透明度低于 100%（半透明模式）下，常规线性阴影或多层叠加极易因为二次 Alpha 混合产生断层切边或矩形黑块；
- Velowork 在渲染内核中设计了智能回退机制：当检测到背景透明度开启时，自动微调遮罩与浮层阴影的物理计算模型，确保半透明下视觉同样清澈通透，零脏边无瑕疵。

---

## 4. 标题栏对齐与风格定制 (Titlebar Customization)

Velowork 针对跨平台用户习惯，提供了极具弹性的标题栏定制系统：

### 4.1 标题文字对齐位置 (Title Position - 新增特性)
在 **设置 -> 外观 -> 标题对齐方式** 中选择：
- **居中对齐 (Center - 默认)**：传统 macOS / 现代 Windows 居中视效，与窗口控制按钮保持自适应间距防碰撞；
- **靠左对齐 (Left)**：经典 Linux CSD / 紧凑排版习惯，释放右侧大面积可拖拽窗口区域；
- **靠右对齐 (Right)**：适合右侧快捷工具扩展场景。

### 4.2 主窗口动态会话标题 (Dynamic Session Title - 新增特性)
- 勾选 **主窗口标题显示当前会话名称**；
- 当您在终端中切换标签页或连接不同主机时，主窗口自定义标题与操作系统任务栏原生窗口标题会毫秒级同步显示为 `{当前会话名称} - Velowork`；
- 在多显示器、系统任务栏（Alt-Tab）与桌面多任务视图中，无需切入即可精准辨识每个窗口对应的生产服务器。

### 4.3 预设标题栏样式 (Presets)
- **Auto**：根据操作系统原生环境自动自适应；
- **macOS**：经典红黄绿交通灯圆钮，精准对齐原生左侧内边距；
- **Windows 11**：现代无边框平滑矩形最小化/最大化/关闭按钮；
- **Linux CSD / KDE Breeze**：深度适配主流 Linux 桌面环境窗口装饰规范。

---

## 5. 终端沉浸壁纸与平滑消融特效 (Wallpaper & Scroll Fade)

### 5.1 自定义终端壁纸 (Terminal Wallpaper)
- 在 **设置 -> 终端** 中点击选取本地背景壁纸图片（支持 PNG、JPG、WebP）；
- 系统会自动施加自适应的高斯模糊遮罩（Gaussian Blur Scrim）与半透明顶栏保护层，无论壁纸细节多么繁复，均能确保终端文字与光标具备绝佳的高反差可读性。

### 5.2 滚动边缘平滑淡出 (Scroll Fade Edges)
- 在浏览长篇终端输出、查看会话树或回放历史命令时，列表顶部与底部内置了连续缓动平滑消融效果；
- 告别传统硬切断线的视觉突兀感，内容优雅隐入背景，呈现细腻的现代交互质感。
