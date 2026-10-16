# 快速入门指南 (Getting Started Guide)

本指南面向首次使用 Velowork 的开发者与运维人员，涵盖从软件安装、首次主密码初始化、工作台核心区域导航，到发起第一个终端连接与自由分屏的完整实操流程。

---

## 1. 下载与安装 (Installation)

Velowork 提供全平台原生二进制包，请前往 [GitHub Releases](https://github.com/mxsail/velowork/releases) 下载最新发行版（当前最新稳定版本为 `v0.1.0-beta.15`）。

### 1.1 Linux 平台
- **Debian / Ubuntu 系 (`.deb`)**：
  ```bash
  sudo dpkg -i velowork_*_amd64.deb
  sudo apt-get install -f # 修复缺失的系统依赖
  ```
- **通用二进制便携包 (`.tar.gz`)**：
  ```bash
  tar -xzf velowork-linux-x86_64.tar.gz
  cd velowork
  ./velowork
  ```
- **源码编译构建**（详见 [主 README 编译指南](https://github.com/mxsail/velowork/blob/main/README_zh.md#%EF%B8%8F-编译与本地构建)）。

### 1.2 Windows 平台
- 下载 `velowork-windows-x64.zip` 解压至任意目录，运行 `velowork.exe`；
- 支持 Windows 11 / Windows 10 (Build 19041+)；
- 原生支持 **Windows Terminal WSL** 与 **PowerShell / CMD** 自动识别。

### 1.3 macOS 平台
- 下载 `.dmg` 文件，将 `Velowork.app` 拖入 `/Applications` 应用程序文件夹；
- 首次在 macOS 打开时，若系统提示“无法打开未签名的应用”，可在 **系统设置 -> 隐私与安全性** 中点击“仍要打开”。

---

## 2. 首次启动与安全初始化 (First Launch & Security)

首次启动 Velowork 时，应用会自动初始化本地 SQLite 数据库与用户配置结构：

1. **设置主密码 (Master Password)**：
   - 弹窗提示创建主密码。该密码用于派生用于保护 SSH 私钥密码、远程服务器登录口令与 API 凭据的 AES-256-GCM 根密钥。
   - 系统将自动检测并集成操作系统的安全密钥环（macOS Keychain、Linux Secret Service / Keyutils、Windows Credential Manager），免去频繁手动输入的繁琐。
2. **应用锁定与解锁**：
   - 随时按快捷键 `Cmd+Alt+L` (macOS) / `Ctrl+Alt+L` (Linux/Windows) 可主动锁定 Velowork。
   - 锁屏状态下，所有正在运行的终端界面与敏感凭据均会被加密遮蔽，独立脱离窗口会自动安全收回，必须输入主密码方可唤醒。

---

## 3. 工作台导航与常用快捷键 (Workbench Navigation)

Velowork 采用高效的单手/键盘直达导航理念。您无需反复寻找鼠标即可瞬间掌控所有区域：

### 3.1 四大区域一键聚焦 (Focus Panels)
- `Cmd+1` (macOS) / `Alt+1` (Linux/Windows)：**聚焦左侧 Dock 栏**（自动展开会话树，支持直接上下键选会话）；
- `Cmd+2` (macOS) / `Alt+2` (Linux/Windows)：**聚焦中央终端工作区**（光标立即回到当前活动终端，无需点击）；
- `Cmd+3` (macOS) / `Alt+3` (Linux/Windows)：**聚焦底栏 Dock**（激活 SFTP 文件列表或快捷执行控制台）；
- `Cmd+4` (macOS) / `Alt+4` (Linux/Windows)：**聚焦右侧 Dock**（直达 AI 助手输入框或服务监控列表）。

### 3.2 面板折叠与显示 (Toggle Docks)
- `Cmd+B` / `Ctrl+B`：展开 / 收起左侧会话面板；
- `Cmd+Shift+R` / `Ctrl+Shift+R`：展开 / 收起右侧扩展面板；
- `Cmd+Shift+Y` / `Ctrl+Shift+Y`：展开 / 收起底栏命令控制台；
- `Cmd+Shift+F` / `Ctrl+Shift+F`：直接唤起/聚焦底栏 SFTP 远程文件浏览器。

---

## 4. 发起您的第一个连接 (Connecting to Sessions)

### 4.1 新建本地 Shell 终端
在中央工作区点击标签栏右侧的 `+` 按钮，或直接按下快捷键：
- macOS: `Cmd+T`
- Linux / Windows: `Ctrl+Shift+T`

Velowork 会自动检测当前系统已安装的 Shell（如 `zsh`、`bash`、`fish`、`PowerShell` 或 `cmd.exe`），并在默认标签中秒级启动一个全新的本地终端会话。

### 4.2 新建 SSH 远程会话
1. 按下快捷键 `Cmd+N` / `Ctrl+N`（或点击左侧会话栏顶部的“新建会话”图标）；
2. 在弹出对话框中填入：
   - **会话名称**：自定义标签（如 `生产环境-Web-01`）；
   - **主机地址**：服务器 IP 或域名（如 `192.168.1.100`）；
   - **端口**：默认为 `22`；
   - **用户名**：如 `root` 或 `ubuntu`；
   - **认证方式**：可选择密码登录或公私钥证书（详见 [会话与远程连接实操手册](SSH-Configuration)）；
3. 点击右下角 **连接** 或按 `Enter` 保存并立即建立会话。

---

## 5. 标签页与自由分屏管理 (Tabs & Split Panes)

### 5.1 多标签页特性
- **标签拖拽排序**：鼠标按住标签页可任意左右拖动改变排列顺序；
- **三种宽度自适应模式**：进入 **设置 -> 外观** 可切换标签宽度呈现：
  - **紧凑模式 (Compact)**：固定紧凑尺寸，适合同时开启十几个会话的高密度多任务；
  - **等宽模式 (Equal)**：所有标签平分顶部空间；
  - **标题自适应 (TitleLength)**：根据会话名称长短自动拉伸宽度。
- **悬停快照卡片预览 (Tab Hover Preview)**：
  - 将鼠标悬停在非活动标签页上，无需切换即可弹出一张半透明卡片，实时呈现该终端当前的屏幕输出快照、运行命令与网络协议。

### 5.2 自由分屏 (Split Panes)
在复杂的排障与对比场景中，您可以在同一个标签页内进行任意层级的自由分屏：
- **垂直分屏（左右并排）**：`Cmd+D` (macOS) / `Ctrl+Shift+D` (Linux/Windows)；
- **水平分屏（上下并排）**：`Cmd+Shift+D` (macOS) / `Ctrl+D` (Linux/Windows)；
- **调整分屏尺寸**：直接按住两窗格之间的分割线并左右/上下平滑拖动；
- **均分所有分屏**：按下 `Cmd+Alt+E` / `Ctrl+Alt+E`，系统将自动重置并等比均分当前所有分屏窗格；
- **单窗格全屏沉浸**：按下 `Shift+Escape` 可让当前聚焦的终端独占整个工作区，再次按下恢复分屏网格。

### 5.3 独立脱离窗口 (Detachable Windows)
点击终端右上角的“脱离”图标，即可将任意正在运行的终端面板弹出为一个独立的系统级浮动窗口，支持跨显示器拖拽；点击浮动窗口顶部的“吸附”按钮，即可随时无缝收回主工作区。

---

## 6. 工作区自动持久化 (Auto Persistence)

Velowork 内置防抖自动保存引擎（Auto-Save）：
- 当您关闭软件或遭遇异常关机时，当前的窗口几何尺寸、所有分屏结构、打开的会话路径与回滚日志均会被安全持久化；
- 下次启动时，软件将自动恢复至上次退出时的完整工作区状态，工作流零打断。
