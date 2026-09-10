<p align="center">
  <img src=".github/assets/velowork-icon.png" alt="Velowork" width="128" height="128">
</p>

<h1 align="center">Velowork</h1>

<p align="center">
  <em>高性能、原生的跨平台终端复用器与远程会话管理工作站。</em>
</p>

<p align="center">
  <strong>基于 Rust 与 <a href="https://github.com/zed-industries/zed/tree/main/crates/gpui">GPUI</a>（Zed 编辑器 GPU 加速 UI 框架）构建。</strong><br/>
  <a href="https://github.com/mxsail/velowork"><strong>GitHub 仓库</strong></a> ·
  <a href="https://github.com/mxsail/velowork/wiki"><strong>文档中心</strong></a> ·
  <a href="https://github.com/mxsail/velowork/releases"><strong>版本发布</strong></a>
</p>

<p align="center">
  SSH、本地终端、SFTP 文件传输、快捷指令、端口隧道、AI 工具集成与工作区持久化的一站式原生桌面客户端。
</p>

<p align="center">
  <a href="https://github.com/mxsail/velowork/releases"><img alt="Version" src="https://img.shields.io/github/v/release/mxsail/velowork?style=flat-square&logo=github&color=5B6BD6&labelColor=334155"></a>
  &nbsp;
  <a href="https://github.com/mxsail/velowork/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/mxsail/velowork/total?style=flat-square&logo=github&color=5B6BD6&label=Downloads&labelColor=334155"></a>
  &nbsp;
  <a href="#"><img alt="Rust Version" src="https://img.shields.io/badge/Rust-1.95.0-orange?style=flat-square&logo=rust&labelColor=334155"></a>
  &nbsp;
  <a href="#"><img alt="Platform" src="https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-5B6BD6?style=flat-square&logo=linux&labelColor=334155"></a>
  &nbsp;
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/License-AGPL_3.0-5B6BD6?style=flat-square&logo=opensourceinitiative&labelColor=334155"></a>
</p>

<p align="center">
  <a href="./README.md">English</a> · <a href="./README_zh.md">简体中文</a>
</p>

<p align="center">
  <img src=".github/assets/screenshot.png" alt="Velowork 预览截图" width="100%">
</p>

---

## 🌟 项目简介

**Velowork** 是一款基于 [Okena](https://github.com/contember/okena) 开发的全新的原生远程终端管理软件与工作站。它将远程服务器集群管理、本地多 Shell 终端、工业级多协议连接、后台进程与容器服务编排、SSH 隧道转发及 SFTP 文件双向同步融为一体，并以精心雕琢的原生 UI 审美与细腻交互细节重塑日常运维与开发操作体验。

- ⚡ **120 FPS 丝滑 GPU 渲染** — 纯 Rust 基于 GPUI 打造，亚毫秒级输入响应，瞬时冷启动，并针对多线程小内存分配进行优化（`jemalloc` / `mimalloc`）。
- 🌐 **一体化远程与本地工作区** — 并排管理 SSH 主机、跳板机、端口隧道、SFTP 传输与本地终端，支持标签页、自由分屏与独立浮动窗口。
- 🔄 **强韧会话持久化** — 借助底层原生会话引擎（`dtach`、`tmux`、`screen` 及 Windows WSL），即使遭遇网络波动或应用重启，终端会话依旧保持在线。
- 🤖 **AI 助手（初步接入 / 迭代中）** — 当前已初步接入多模型 Provider（Claude、OpenAI、DeepSeek、Ollama），提供基础的自然语言转 Shell 命令行与侧边栏对话。该模块目前处于早期集成阶段，更多深入的终端协同功能仍在持续迭代完善中。
- 📊 **实时系统与服务监控** — 实时硬件状态栏（CPU、内存、磁盘 I/O、网络带宽）；图形化**服务面板**统一监管 Systemd 单元、Docker 容器与自定义命令服务，支持健康探活与启停控制。
- 🔐 **本地优先安全与凭据保险箱** — 支持主密码保护（`velowork-security`），深度整合系统密钥环（Keyring）与 AES-256-GCM 本地加密存储及同步包。

---

## 🚀 核心功能特性

### 🪟 布局与工作区管理
- **自由分屏面板** — 支持水平与垂直自由分屏，拖拽分割线即可即时平滑调整尺寸。
- **标签页容器与停靠栏** — 灵活组织终端标签页，支持鼠标拖拽排序；支持折叠左/右/底三向 Dock 面板。
- **多种标签页宽度模式** — 支持等宽（Equal）、紧凑（Compact）与标题自适应（TitleLength）三种模式，适配不同标签数量与多任务场景。
- **标签页悬停卡片预览** — 鼠标悬停未激活标签页时，自动弹出带实时终端快照、连接协议与会话元信息的半透明卡片预览。
- **可脱离浮动窗口** — 将任意终端面板弹出为独立浮动窗口，并可随时吸附回主工作区。
- **多项目并排管理** — 并行展示多个独立项目列，支持按需调整各列宽度与层级。
- **工作区状态自动持久化** — 防抖自动保存并恢复完整的窗口布局、终端历史输出、打开的标签页与偏好设置。

### 💻 终端使用体验
- **完整终端仿真** — 基于 `alacritty_terminal`，支持完整的 24-bit TrueColor 真彩色与 ANSI 转义序列、图片及 Sixel 渲染。
- **沉浸式终端背景壁纸** — 支持自定义设置终端背景图片，配备自适应高斯模糊磨砂遮罩与半透明顶栏保护层，兼顾个性化壁纸与文字图标的清晰对比度。
- **ZMODEM 高速文件互传** — 原生集成 ZMODEM 协议，终端内执行 `rz` / `sz` 即可直接唤起图形化文件选择器完成双向传输。
- **行内文本搜索** — 快速检索终端输出，支持正则表达式、大小写匹配与命中高亮计数。
- **智能链接识别与编辑器联动** — 自动识别 URL、IP 地址与文件路径；支持 `file:line:col` 语法，一键在 VS Code、Cursor、Zed、Sublime、Neovim 等编辑器中打开。
- **安全括号粘贴与图片粘贴** — 自动处理多行粘贴防代码注入，支持直接向 TUI 应用粘贴剪贴板图片。
- **逐终端独立 Shell** — 每个终端窗口可自由指定不同的 Shell（`bash`、`zsh`、`fish`、`cmd.exe`、`PowerShell`、`WSL`）。
- **灵活回滚与响铃配置** — 支持最高 100,000 行回滚缓冲与视觉/听觉响铃提示。

### 🌐 多协议远程连接与会话管理
- **资产与会话树** — 目录分组树、标签分类、自定义图标配色与一键直连。
- **工业串口 (Serial Port) 支持** — 支持本地物理串口与 USB 转串口设备连接，提供串口列表自选与标准波特率调节。
- **Telnet 协议支持** — 原生支持 Telnet 远程登录与交换机/路由器网络设备调试管理。
- **X11 远程图形转发** — 支持 SSH X11 Forwarding，远程 Linux GUI 应用程序窗口可无缝投影到本地桌面显示。
- **跳板机 (ProxyJump)** — 支持多跳堡垒机链路与 SSH 密钥密码缓存。
- **SSH 隧道转发** — 图形化配置和管理本地（`-L`）、远程（`-R`）与动态 SOCKS5（`-D`）端口转发。
- **SFTP 文件浏览器** — 双栏文件管理器，支持拖拽上传/下载、远程文件实时浏览与远程行内编辑。
- **快捷指令库** — 沉淀高频命令，支持参数占位符 `{{变量}}` 弹出式交互输入与模态填充。

### 🔄 会话持久化
- **零中断重连** — 通过 `dtach`、`tmux` 或 `screen`（Unix）在断网或重启后无缝恢复会话状态。
- **WSL 会话支持** — 无缝兼容 Windows 平台的 WSL 终端持久化与环境隔离。
- **配置与场景快照** — 命名工作区配置的快速保存、加载、导出与导入。

### 🤖 AI 智能助手（初步接入 / 迭代中）
- **AI 交互侧边栏** — 融合当前活动终端与项目上下文的 AI 对话窗口。
- **自然语言转命令** — 将日常语言需求一键转为精准 Shell 命令行并直接执行。
- **多模型支持** — 支持对接 Claude、OpenAI、DeepSeek、Ollama 或任意兼容 OpenAI 接口的模型。
> 💡 **说明**：AI 功能目前处于早期接入与探索阶段，更多深度的终端智能联动特性仍在持续开发完善中。

### 📊 系统与服务监控
- **实时硬件状态栏** — 毫秒级展示 CPU、内存、磁盘 I/O、网络流量与系统时钟。
- **自定义服务面板** — 在服务面板中直观管理 Systemd 单元、Docker 容器与自定义命令服务，实时检测存活状态并一键启停/重启。

### 🎨 外观定制与全面国际化
- **自研原生设计系统** — 统一的 Design Tokens、语义调色板、同心圆角设计与微动效，兼顾优雅观感与高密度工作区效率。
- **多平台风格动态标题栏** — 支持精美应用内置标题栏与原生系统标题栏随时切换，并提供 macOS 交通灯、Windows 11、Linux CSD、KDE Breeze 多套平台级控件风格预设。
- **精致主题系统** — 内置深色、浅色、柔和深色、高对比度主题，支持导入自定义 JSON 主题。
- **完整双语国际化 (i18n)** — 全面支持英文与简体中文，纯图标按钮配备悬停中文 Tooltip 提示。

---

## 🏗️ 系统架构

Velowork 采用清晰的分层设计，由 17 个专业化 Cargo workspace crate 构成：

```
velowork (二进制入口)
  ├── velowork-app (桌面业务层: 窗口调度、菜单栏、浮层协调、快捷键路由)
  │     ├── velowork-views-terminal (视图层: 终端面板、SFTP 浏览器、对话框、侧边栏)
  │     ├── velowork-ui (UI 组件层: 设计令牌、按钮、输入框、对话框、标签页)
  │     │     ├── velowork-theme (主题引擎与语义颜色调色板)
  │     │     └── velowork-core (核心共享类型、进程管理、Git 集成)
  │     ├── velowork-layout (Dock 与面板分屏布局原语)
  │     ├── velowork-state (响应式状态存储、服务监控定义与事件通道)
  │     ├── velowork-workspace (Workspace GPUI 实体、持久化与项目状态)
  │     ├── velowork-terminal (PTY 调度循环、Shell 探测、会话持久化引擎)
  │     ├── velowork-security (主密码加密、系统 Keyring 凭据保险箱)
  │     ├── velowork-ai (AI 助手、LLM Provider 适配与自然语言命令生成)
  │     ├── velowork-monitor (硬件资源与系统指标采集: CPU/内存/磁盘/网络)
  │     ├── velowork-markdown (Markdown 解析与渲染器、文档阅读器)
  │     ├── velowork-i18n (内置多语言翻译、词条加载器与 i18n! 宏)
  │     ├── velowork-extensions (扩展与插件运行时系统)
  │     └── velowork-updater (带加密校验的后台自动更新服务)
```

| Crate | 职责划分 |
| :--- | :--- |
| `velowork-app` | 桌面应用业务层：主窗口生命周期、菜单栏、浮层管理器、快捷键调度 |
| `velowork-app-core` | 应用核心领域模型、接口契约与跨模块状态桥接 |
| `velowork-workspace` | LayoutNode 布局树、多窗格停靠、工作区持久化与设置状态 |
| `velowork-views-terminal` | 终端面板视图、SFTP 浏览器、隧道与远程连接对话框、侧边栏视图 |
| `velowork-terminal` | PTY 调度循环、Shell 探测、会话持久化引擎（`dtach`/`tmux`/`screen`） |
| `velowork-layout` | Dock 停靠栏、Panel 面板与分屏结构原语 |
| `velowork-state` | 状态存储、响应式状态模型、服务监控定义与消息通道 |
| `velowork-ui` | 设计令牌、图标库、现代化自研 UI 通用组件库（`Button`、`SimpleInput`、`Tab` 等） |
| `velowork-theme` | 主题颜色定义、动态主题解析与语义颜色映射 |
| `velowork-security` | 主密码加密、OS Keyring 接入与凭据保险箱 |
| `velowork-ai` | AI 助手集成、LLM Provider 适配与自然语言命令生成 |
| `velowork-monitor` | 硬件与系统资源监控（CPU、内存、磁盘 I/O、网络流量） |
| `velowork-markdown` | Markdown 渲染引擎、文档查看器与语法高亮 |
| `velowork-i18n` | 内置多语言字典、国际化加载器与 `i18n!` 宏 |
| `velowork-core` | 共享数据类型、线协议、Git Diff 解析与 Worktree 操作 |
| `velowork-extensions` | 扩展系统与插件体系规范 |
| `velowork-updater` | 带数字签名与哈希校验的后台自动更新服务 |

---

## ⌨️ 常用快捷键

> 所有快捷键均支持上下文感知（Context-Aware），并可通过 `keybindings.json` 自由修改。

| 操作 | macOS | Linux / Windows | 生效范围 / 上下文 |
| :--- | :--- | :--- | :--- |
| **全局命令面板** | `Cmd+Shift+P` | `Ctrl+Shift+P` | 全局 |
| **聚焦左侧栏 (会话树)** | `Cmd+1` | `Alt+1` | 全局 |
| **聚焦中间工作区 (终端)** | `Cmd+2` | `Alt+2` | 全局 |
| **聚焦底栏 (SFTP / 命令)** | `Cmd+3` | `Alt+3` | 全局 |
| **聚焦右侧面板 (AI / 工具)** | `Cmd+4` | `Alt+4` | 全局 |
| **切换左侧 Dock 栏** | `Cmd+B` | `Ctrl+B` | 全局 |
| **切换右侧 Dock 栏** | `Cmd+Shift+R` | `Ctrl+Shift+R` | 全局 |
| **打开 AI 助手面板** | `Cmd+Shift+A` | `Ctrl+Shift+A` | 全局 |
| **打开 SSH 隧道面板** | `Cmd+Shift+U` | `Ctrl+Shift+U` | 全局 |
| **打开服务面板** | `Cmd+Shift+S` | `Ctrl+Shift+S` | 全局 |
| **打开快捷指令面板** | `Cmd+Shift+K` | `Ctrl+Shift+K` | 全局 |
| **打开 SFTP 文件浏览器** | `Cmd+Shift+F` | `Ctrl+Shift+F` | 全局 |
| **打开底部命令面板** | `Cmd+Shift+Y` | `Ctrl+Shift+Y` | 全局 |
| **打开历史命令记录面板** | `Cmd+Shift+H` | `Ctrl+Shift+H` | 全局 |
| **均化所有分屏尺寸** | `Cmd+Alt+E` | `Ctrl+Alt+E` | 全局 |
| **新建应用窗口** | `Cmd+Shift+N` | `Ctrl+Shift+N` | 全局 |
| **偏好设置** | `Cmd+,` | `Ctrl+,` | 全局 |
| **快捷键查看与配置** | `Cmd+K Cmd+S` | `Ctrl+K Ctrl+S` | 全局 |
| **主题选择器** | `Cmd+K Cmd+T` | `Ctrl+K Ctrl+T` | 全局 |
| **新建终端标签** | `Cmd+T` | `Ctrl+Shift+T` | 终端面板 |
| **垂直分屏** | `Cmd+D` | `Ctrl+Shift+D` | 终端面板 |
| **水平分屏** | `Cmd+Shift+D` | `Ctrl+D` | 终端面板 |
| **关闭当前终端** | `Cmd+W` | `Ctrl+Shift+W` | 终端面板 |
| **终端内搜索** | `Cmd+F` | `Ctrl+F` | 终端面板 |
| **复制 / 粘贴** | `Cmd+C` / `Cmd+V` | `Ctrl+Shift+C` / `Ctrl+Shift+V` | 终端面板 |
| **放大 / 缩小字号** | `Cmd+=` / `Cmd+-` | `Ctrl+=` / `Ctrl+-` | 终端面板 |

---

## 🛠️ 构建与安装

### 环境准备

- **Rust 工具链**: **1.95.0**（通过 `rust-toolchain.toml` 锁定，Edition 2024）。
- **C/C++ 工具链与依赖**: `cmake`、`pkg-config` 及基础 C 编译环境。

#### Linux 系统依赖安装
Debian / Ubuntu 系列：
```bash
sudo apt update
sudo apt install -y build-essential pkg-config libx11-dev libxkbcommon-x11-dev \
    libfontconfig1-dev libwayland-dev libssl-dev cmake
```

Fedora / RHEL 系列：
```bash
sudo dnf install -y gcc gcc-c++ pkgconfig libX11-devel libxkbcommon-x11-devel \
    fontconfig-devel wayland-devel openssl-devel cmake
```

### 源码编译

```bash
# 克隆仓库
git clone https://github.com/mxsail/velowork.git
cd velowork

# 编译优化后的 Release 版本
cargo build --release

# 运行 Velowork
./target/release/velowork
```

### Windows 构建

在 Windows 环境下，请在 **x64 Native Tools Command Prompt for VS 2022** 命令行中执行：

```powershell
cargo build --release --target x86_64-pc-windows-msvc
# 生成二进制产物：target\x86_64-pc-windows-msvc\release\velowork.exe
```

### 性能分析构建

如需使用 `dhat` 生成堆内存分配分析文件：

```bash
cargo run --profile profiling --features dhat-heap
```

---

## ⚙️ 配置文件与多 Profile 架构

Velowork 采用 Profile 隔离的配置存储方案。各平台配置路径如下：

- **macOS**: `~/Library/Application Support/velowork/`
- **Linux**: `~/.config/velowork/`
- **Windows**: `%APPDATA%\velowork\`

### 目录结构组织

```text
<config-dir>/velowork/
├── profiles/
│   └── default/
│       ├── manifest.json
│       ├── config/
│       │   ├── settings.json       # 统一设置（外观、字体、AI 助手、云同步、网络代理等）
│       │   └── keybindings.json    # 用户自定义快捷键映射
│       ├── data/
│       │   └── velowork.db         # SQLite 单文件数据库（主机资产、会话、服务、历史记录）
│       └── themes/                 # 自定义 JSON 主题文件
├── logs/                           # 按 Profile 划分的运行日志
└── runtime/                        # 套接字、IPC 与运行锁文件
```

---

## 📖 文档与支持

- [快速入门指南](https://github.com/mxsail/velowork/wiki/Getting-Started)
- [SSH 与远程连接配置](https://github.com/mxsail/velowork/wiki/SSH-Configuration)
- [快捷指令与占位符变量](https://github.com/mxsail/velowork/wiki/Quick-Commands)
- [自定义主题与字体配置](https://github.com/mxsail/velowork/wiki/Custom-Themes)
- [快捷键完整规则手册](docs/shortcuts.md)

---

## 🤝 参与贡献

欢迎提交 Issue 与 Pull Request！代码贡献与功能开发请严格遵守项目的 [核心架构与贡献宪法](CONSTITUTION.md)。

```bash
# 运行全部 Workspace 单元测试
cargo test

# 快速类型检查
cargo check
```

---

## 💡 开发故事与创作者说明

### 为什么选择“重复造轮子”？
在日常的系统运维与终端交互中，市面上现有的终端与远程连接工具始终未能完全满足个人的使用预期：
- **审美与桌面系统的割裂感**：作为一名日常主要工作在 **KDE Plasma (Breeze 主题)** 与 **Windows 11** 环境下的用户，现有多数终端界面的设计风格与现代原生桌面环境格格不入，缺乏与系统浑然一体的精致感。
- **伪轻量与高资源占用**：部分工具虽然宣称基于 Rust 构建，但底层实际上是基于 Tauri / Webview 等 Web 技术栈打包。看似初始占用低，但在高并发日志流、多标签分屏与长时间后台挂载下，内存膨胀与渲染功耗依旧居高不下。
- **工作流细节的不顺手**：以日常高频使用的“快捷指令 / 固化命令”为例，很多常用命令只需输入固定前缀，中间参数需要根据当时的执行上下文动态填入。而市面上大多数终端软件要么只能死板地全量替换整行，要么缺乏优雅直观的交互式变量参数占位符（如 `{{variable}}`）支持，频繁修改命令体验割裂。

经过深入的技术调研，**Rust + GPUI** 的技术组合脱颖而出：真正的原生二进制带来极限的低资源开销与确定性内存占用，GPU 硬件加速则保证了 120 FPS 的亚毫秒级渲染响应，这正是理想终端工作站所需要的基石。

### 零经验者的 Vibe Coding：人机协同实践
然而，作为一个**没有任何 Rust 开发经验**的爱好者，在陡峭的学习曲线前开发这样一款复杂的桌面软件近乎天方夜谭。于是，我选择了拥抱当下前沿的 **Vibe Coding** 模式：
- **人类创作者的角色**：我完全遵循自己多年沉淀的真实使用习惯与痛点，专注于产品愿景、交互逻辑、功能定义与视觉细节把控，为软件注入灵魂。
- **AI 担当系统架构与全栈编写**：全仓库的所有 Rust 核心逻辑、GPUI 视图组件、跨平台兼容层及 CI/CD 自动化，**100% 由 AI 编写**。
  - 在立项初期，曾借助 **mimocode** 与 **codebuddy** 等 AI 开发工具进行技术验证与早期原型试水；
  - 随着工程深度推进，目前已全面迁移并深度依赖 **Google Antigravity（搭载 Gemini 模型）** 进行全生命周期的代码实现、疑难排查与版本发布。

### 坦诚面对缺陷，欢迎共同完善
正因为代码完全由 AI 编写且处于高频演进的早期阶段，软件中难免存在未被覆盖的边缘情况与 Bug，这在复杂的系统级桌面软件开发中完全正常。

- **遇到 Bug 很正常**：如果您在使用过程中遇到任何异常或崩溃，欢迎随时在 GitHub 提交 [Issue](https://github.com/mxsail/velowork/issues)。我会第一时间将问题上下文交给 Antigravity 进行定位、修复与回归验证。
- **期待您的灵感与创意**：如果您对终端交互、SSH 会话管理或效率工具有任何有趣、实用的新想法，也十分欢迎提供建议或参与讨论，让我们一起通过人机协同打造更顺手的现代工作站！

---

## 📄 开源协议、致谢与免责声明

### 📜 开源协议
本项目基于 [AGPLv3 开源许可证](LICENSE) 发布。版权所有 © 2026 Velowork Team。

### 💖 项目致谢
Velowork 的诞生离不开开源生态与前沿 AI 技术的助力，特此向以下项目与工具致以由衷的感谢：

- **[Google Antigravity & Gemini](https://deepmind.google/technologies/gemini/)**：感谢谷歌 Antigravity 工具与 Gemini 模型强大的智能编程与系统工程落地能力，承担了当前全部的 Rust 代码实现与重构演进。
- **[Okena](https://github.com/contember/okena)**：Velowork 是基于 Okena 开发的全新的远程终端管理软件。感谢 Okena 团队在 GPUI 终端多路复用、工作区布局与底层架构上的开拓性贡献。
- **[GPUI (Zed)](https://github.com/zed-industries/zed)**：感谢 Zed 团队开源的高性能 GPU 加速 UI 框架，为应用提供了亚毫秒级丝滑响应的图形渲染基石。
- **[Alacritty](https://github.com/alacritty/alacritty)**：感谢 Alacritty 项目提供的快速、标准的终端仿真核心后端。
- **早期 AI 探索工具**：感谢 **mimocode**、**codebuddy** 在项目最初期原型探索阶段提供的辅助支持。
- **Rust 开源社区**：感谢 `tokio`、`smol`、`portable-pty`、`serde`、`keyring` 等高质量基础生态库的开发者们。

### ⚠️ 免责声明
- **按现状提供**：本项目是出于个人兴趣与技术探索而开发的开源软件，基于 AGPLv3 许可证按“现状（AS IS）”免费提供，不包含任何明示或暗示的商业担保。
- **AI 辅助代码说明**：全仓库代码均由 AI 协同生成。虽然我们在发布前均会进行基础单测与运行自检，但仍建议您在关键生产环境使用前先行在测试环境中体验与验证。
- **日常使用提示**：在使用 SSH 远程连接、文件传输与终端命令时，请妥善保管好个人服务器凭据与私钥，并在执行重要操作前做好必要的数据备份。使用者自行对个人会话与操作行为负责。
