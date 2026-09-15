# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0-beta.5] - 2026-09-16

### Added / 新增
- **终端内联 AI 交互与快捷键触发**：
  - 支持在终端窗口内直接唤起内嵌式 AI（Terminal Inline AI），实现就地上下文提问与交互。  
  *(Support in-situ Terminal Inline AI interaction with dedicated shortcut triggers).*
- **AI 助手面板与会话管理全面增强**：
  - 新增历史会话查看与管理浮层，支持一键清空会话与便捷切换。
  - 会话搜索能力升级：支持关键词多重高亮、实时平滑滚动定位及全文检索。
  - 新增 Token 环状进度指示条组件，悬浮展示详尽的上下文用量与配额 Tooltip。  
  *(Comprehensive AI assistant panel enhancements: session management overlay, real-time keyword highlight & smooth search navigation, token usage progress ring with detail tooltips).*
- **Markdown 划选与原生复制**：
  - AI 回复支持纯文本与格式化 Markdown 的划选高亮及原生 `Ctrl+C` 剪贴板复制。  
  *(Support text selection and native Ctrl+C clipboard copy within rendered AI Markdown responses).*

### Improved / 优化
- **AI 性能与流畅度优化**：
  - 引入 `gpui::list` 虚拟化渲染与 SQLite 分页加载机制，极大优化长会话滚动帧率与内存开销。
  - 优化右侧 Dock 实例保活与展开流体微动效，消除展开瞬间的卡顿与残影。  
  *(Adopt gpui::list virtualization and SQLite pagination for long chat history; optimize right dock instance keep-alive and fluid expansion motion).*

## [0.1.0-beta.4] - 2026-09-16

### Fixed / 修复
- **标签展开动画终点空白闪烁与行号闪动根治**：
  - 彻底移除了动效卡片在 300ms 时的提前退出脱靶判定，使其与恢复状态机原子同步，消灭交接瞬间因异步定时器时差造成的空白帧闪烁。
  - 将 Tab 组内子容器与独立还原逻辑解耦，防止内外双重隐身定时器时间差；并统一动效卡片底色与终端调色板算法，平滑收敛终点圆角与外阴影。
  - 抽离公共行号渲染器 `render_line_numbers_gutter` 供终端与动效卡片共享，确保行号自起始展开动画第一帧即刻对齐。
  - 过滤全新创建 Tab 的展开动画误触发，解决终端打开时无行号闪烁问题。  
  *(Eliminate visual flicker and blank-frame gap when expanding restored tabs; decouple inner container restore epochs; share line numbers gutter across preview cards and real terminal).*
- **点击终端界面导致底部最小化标签闪烁问题修复**：
  - 优化全局焦点与重绘响应链路，防止点击终端时因多余状态变更引起底部最小化胶囊不必要的重刷与闪烁。  
  *(Prevent minimized tab capsules in bottom dock from flickering when clicking inside terminal panes).*

### Improved / 优化
- **标签页与底部胶囊悬浮预览弹窗定位与动效优化**：
  - 重构标签页与最小化胶囊的预览弹窗坐标算法，支持自适应边界避让、平滑入场微动效与源点感知。  
  *(Improve terminal tab and bottom dock thumbnail preview popup positioning, boundary detection, and smooth entrance transitions).*

## [0.1.0-beta.3] - 2026-09-12

### Fixed / 修复
- **自动更新检查器 GitHub API 速率限制规避与探针优化**：改用 GitHub Releases 网页重定向探针（`HEAD` 请求），彻底摆脱未鉴权 REST API 60次/小时公网 IP 限流导致的误报错；同时完善多平台构建产物与校验和自动探测机制，并将后台自动检测错误降级为 warning。  
  *(Bypass GitHub API rate limit by probing Releases web redirect headers instead of consuming REST API quota, add asset probe fallback, and demote background auto-check errors to warning).*
- **串口/Telnet/本地终端标签名称显示与连接失败无反馈修复**：修复新建串口、Telnet、本地终端双击打开后标签栏无法正确展示配置中会话名称的缺陷；修复打开不存在串口或连接失败时缺少用户反馈的问题，增加端口必填校验与友好浮动通知。  
  *(Fix session tab names for Serial, Telnet, and Local terminal tabs; add required port validation and floating notification on serial connection failures).*
- **终端字体字重设置生效与选项标准化**：统一终端设置中字重下拉选项与后端存储的大小写映射，补齐 Light 档位，彻底修复字重默认显示为空及选择后渲染未生效问题。  
  *(Fix terminal font weight settings by aligning casing between dropdown options and model serialization, adding Light weight option, and ensuring live render updates).*
- **设置子弹窗 ESC 键穿透关闭问题修复**：修复在设置面板的子弹窗（如添加连接/编辑会话）中按下 ESC 键会连带关闭外层主设置窗口的穿透问题。  
  *(Fix ESC key event propagation in settings sub-modals accidentally closing the parent settings window).*
- **终端标签页展开动效与焦点回弹优化**：修复标签页展开与恢复时的排版错乱与尺寸跳变，解决动画完成后终端输入焦点丢失的问题。  
  *(Fix tab expand/restore layout glitches and restore keyboard focus smoothly after animation completes).*
- **Prompt 瞬态清屏闪烁根治**：优化清屏与光标重绘逻辑，彻底消除快速交互时的瞬态闪烁。  
  *(Eliminate transient screen clearing flicker on prompt redraws).*

### Added / 新增
- **状态栏右侧工具条快捷开关与布局收窄**：将状态栏右下角按钮改造为控制右侧工具条显隐的开关，支持状态持久化记忆与高亮反馈，并将右侧图标栏宽度收窄至 30px 以最大化终端可视区域。  
  *(Add toggle button in status bar for right sidebar visibility with state persistence and active indicator, and optimize right bar width to 30px).*
- **全局物理流体微动效覆盖与弹窗源点感知**：弹窗与浮层全量接入物理弹簧与流体过渡，支持源点感知形变；优化 Windows 平台弹窗打开延迟与字体渲染清晰度。  
  *(Full coverage of fluid motion animation and source-aware modal transitions; optimize modal open latency and text clarity on Windows).*

## [0.1.0-beta.2] - 2026-09-10

### Fixed / 修复
- **Windows 11 自定义标题栏与窗口控制按钮渲染修复**：修复了在客户端装饰模式（CSD）下 Windows 11 平台不显示自定义标题栏及最小化/最大化/关闭按钮的问题，纠正了窗口装饰属性判断与 `TitlebarOptions` 透明样式传递。  
  *(Fix custom title bar and window caption buttons not rendering on Windows 11 under client-side decoration mode).*
- **任务栏右键“关闭窗口”与退出假死修复**：区分 OS 级关闭信号与标题栏关闭事件；在弹出退出确认对话框前先唤醒并激活窗口（解决最小化至后台时弹窗无法响应的假死问题）；并将默认关闭行为调整为“直接退出 (`Exit`)”。  
  *(Fix taskbar right-click "Close window" and exit unresponsiveness by distinguishing OS close from titlebar close, unminimizing/activating window before confirmation dialog, and defaulting close behavior to Exit).*
- **CI 流水线 Clippy 栈溢出修复与耗时优化**：将全局编译器栈深度 `RUST_MIN_STACK` 从 32MB 扩容至 64MB；并将 Clippy 门禁限定于全工作区生产代码（避免深层 `#[gpui::test]` 宏展开导致 AST 递归访问器崩溃），静态检查耗时由 3+ 分钟崩溃骤降至 18 秒完成。  
  *(Fix Clippy stack overflow in CI by increasing `RUST_MIN_STACK` to 64MB and scoping Clippy checks to workspace production targets, reducing lint time to 18 seconds).*

### Added / 新增
- **精简版 GitHub Flow 开发规范与工程资产**：制定了单主干演进、禁止直推 main、Squash and Merge 线性历史的轻量规范，新增标准 PR 模板 (`.github/PULL_REQUEST_TEMPLATE.md`) 与规范的变更日志跟踪。  
  *(Adopt lean GitHub Flow workflow specifications, along with standardized GitHub Pull Request template and changelog tracking).*

## [0.1.0-beta.1] - 2026-09-10

### Added
- Initial public beta release of Velowork.
- Cross-platform GPU-accelerated terminal and workspace application built with GPUI.
- Support for local shells and remote SSH session management with credential encryption.
- Multi-pane terminal splitting (horizontal/vertical) and tab management.
- Theme system and configurable keyboard shortcuts.
- Automated CI matrix release packaging for Linux (`.AppImage`, `.deb`), macOS (`.dmg`, `.zip` for ARM64 & Intel), and Windows (`.msi`, `.zip`).
