# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
