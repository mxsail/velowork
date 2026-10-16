# Velowork 快捷键映射与分配规则

本文档记录 Velowork 当前系统快捷键的统一分配规则与完整键位映射表。快捷键设计兼顾 macOS（`Cmd`）与 Linux/Windows（`Ctrl` / `Alt`）使用习惯，并采用基于 GPUI 上下文分发的双维冲突判定机制。

---

## 1. 快捷键模型与分发规则

### 1.1 双维模型

1. **维度一：ActionScope（语义维度，仅用于元数据与 UI 分组）**
   - 分类为 `Global`（全局）、`Panel`（面板内）、`Terminal`（终端内）、`Fullscreen`（全屏）、`Search`（搜索）、`Navigation`（导航）、`Project`（项目）、`Session`（会话）、`Window`（窗口）。
   - 仅用于在快捷键帮助面板（`ShowKeybindings`）与设置中心中进行分类检索展示，不参与运行时按键路由。

2. **维度二：KeyContext（运行时分发维度，由 GPUI 调度）**
   - 每个快捷键绑定可指定一个生效上下文 `context`：
     - `None`：全局有效（工作台级快捷键，如 Dock 聚焦、面板开关、命令面板、设置）。
     - `"TerminalPane"`：终端聚焦时生效（复制、粘贴、分屏、全屏、标签操作、字体缩放）。
   - **Context Override（上下文覆盖）**：更具体的 Context 优先匹配。例如终端聚焦时 `Ctrl+Shift+C` 命中 `TerminalPane` 的复制操作，属于合法覆盖而非键位冲突。

### 1.2 Esc 消费层级

`Esc` 键按照严格的视觉层级向下消费，被拦截后不再向下传递：

1. **Overlay**（模态弹窗、命令面板、设置中心）→ 关闭自身。
2. **Popup**（上下文菜单、下拉列表、重命名输入框）→ 取消并关闭。
3. **Panel Editing**（面板内行内编辑态）→ 取消编辑并交还焦点。
4. **Workbench**（常规工作台态）→ 清除焦点或取消高亮。

---

## 2. 完整快捷键映射表

### 2.1 全局与窗口管理（Context = `None`）

| macOS         | Linux / Windows | Action 名称          | 说明                              | 分类   |
| :------------ | :-------------- | :------------------- | :-------------------------------- | :----- |
| `Cmd+Shift+P` | `Ctrl+Shift+P`  | `ShowCommandPalette` | 打开命令面板（Command Palette）   | Global |
| `Cmd+,`       | `Ctrl+,`        | `ShowSettings`       | 打开设置中心                      | Global |
| `Cmd+Alt+,`   | `Ctrl+Alt+,`    | `OpenSettingsFile`   | 直接打开 `settings.json` 配置文件 | Global |
| `Cmd+K Cmd+S` | `Ctrl+K Ctrl+S` | `ShowKeybindings`    | 打开快捷键列表帮助与配置          | Global |
| `Cmd+K Cmd+T` | `Ctrl+K Ctrl+T` | `ShowThemeSelector`  | 打开主题选择器                    | Global |
| `F1`          | `F1`            | `ShowHelp`           | 打开帮助中心                      | Global |
| `Cmd+Shift+N` | `Ctrl+Shift+N`  | `NewWindow`          | 新建独立工作台窗口                | Window |
| `Cmd+Alt+L`   | `Ctrl+Alt+L`    | `LockApp`            | 锁定应用程序（需主密码解锁）      | Global |
| `Cmd+Q`       | `Ctrl+Q`        | `Quit`               | 安全退出 Velowork                 | Global |

### 2.2 四大分区聚焦与 Dock 导航（Context = `None`）

| macOS         | Linux / Windows | Action 名称              | 对应操作 / 焦点行为                                    | 分类       |
| :------------ | :-------------- | :----------------------- | :----------------------------------------------------- | :--------- |
| `Cmd+1`       | `Alt+1`         | `FocusLeftDock`          | 聚焦左侧栏（自动展开并高亮首项会话，支持方向键操作）   | Navigation |
| `Cmd+2`       | `Alt+2`         | `FocusCenterDock`        | 聚焦中间终端工作区（激活活动终端输入）                 | Navigation |
| `Cmd+3`       | `Alt+3`         | `FocusBottomDock`        | 聚焦底栏（自适应激活 SFTP 文件列表或快捷命令输入框）   | Navigation |
| `Cmd+4`       | `Alt+4`         | `FocusRightDock`         | 聚焦右侧面板（自动激活 AI 助手输入框或各扩展面板列表） | Navigation |
| `Cmd+B`       | `Ctrl+B`        | `ToggleLeftDock`         | 显示 / 隐藏左侧 Dock 栏                                | View       |
| `Cmd+Shift+R` | `Ctrl+Shift+R`  | `ToggleRightDock`        | 显示 / 隐藏右侧 Dock 栏                                | View       |
| `Cmd+Shift+B` | `Ctrl+Shift+B`  | `ToggleLeftDockAutoHide` | 切换左侧栏悬浮自动隐藏模式                             | View       |
| `F6`          | `F6`            | `CyclePanelNext`         | 顺时针轮换切换聚焦区域                                 | Navigation |
| `Shift+F6`    | `Shift+F6`      | `CyclePanelPrev`         | 逆时针轮换切换聚焦区域                                 | Navigation |

### 2.3 会话、项目与扩展面板（Context = `None`）

| macOS         | Linux / Windows | Action 名称               | 对应面板 / 视图                     | 分类    |
| :------------ | :-------------- | :------------------------ | :---------------------------------- | :------ |
| `Cmd+N`       | `Ctrl+N`        | `NewSession`              | 打开新建 SSH 会话对话框             | Session |
| `Cmd+E`       | `Ctrl+E`        | `ShowProjectSwitcher`     | 快速项目切换器                      | Project |
| `Cmd+Shift+I` | `Ctrl+Shift+I`  | `ShowImportSessionDialog` | 导入会话（SSH Config / Xshell 等）  | Project |
| `Cmd+Shift+Y` | `Ctrl+Shift+Y`  | `ToggleCommandsPanel`     | 底栏命令面板（脚本/命令快速执行器） | View    |
| `Cmd+Shift+F` | `Ctrl+Shift+F`  | `ToggleSftpPanel`         | 底栏 SFTP 远程文件管理器            | View    |
| `Cmd+J`       | `Ctrl+J`        | `ToggleStatusBar`         | 显示 / 隐藏底部状态栏               | View    |
| `Cmd+Shift+A` | `Ctrl+Shift+A`  | `ShowAiAssistant`         | 右侧 AI 助手面板                    | View    |
| `Cmd+Shift+K` | `Ctrl+Shift+K`  | `ShowQuickCommandsPanel`  | 右侧快捷指令面板                    | View    |
| `Cmd+Shift+H` | `Ctrl+Shift+H`  | `ShowHistoryPanel`        | 右侧历史执行命令面板                | View    |
| `Cmd+Shift+U` | `Ctrl+Shift+U`  | `ShowTunnelsPanel`        | 右侧 SSH 端口转发隧道面板           | View    |
| `Cmd+Shift+S` | `Ctrl+Shift+S`  | `ShowServicesPanel`       | 右侧后台守护服务监控面板            | View    |

### 2.4 终端窗格与网格操作（Context = `"TerminalPane"`）

| macOS                        | Linux / Windows               | Action 名称               | 说明                                        | 分类       |
| :--------------------------- | :---------------------------- | :------------------------ | :------------------------------------------ | :--------- |
| `Cmd+I`                      | `Ctrl+I`                      | `TerminalInlineAi`        | 呼出终端内联 AI 提示框（自然语言转命令/解释） | Terminal   |
| `Cmd+T`                      | `Ctrl+Shift+T`                | `AddTab`                  | 新建终端标签页                              | Terminal   |
| `Cmd+W`                      | `Ctrl+Shift+W`                | `CloseTerminal`           | 关闭当前聚焦的终端                          | Terminal   |
| `Cmd+D`                      | `Ctrl+Shift+D`                | `SplitVertical`           | 垂直分屏（左右并排）                        | Terminal   |
| `Cmd+Shift+D`                | `Ctrl+D`                      | `SplitHorizontal`         | 水平分屏（上下并排）                        | Terminal   |
| `Cmd+Alt+S`                  | `Ctrl+Alt+S`                  | `DuplicateSession`        | 在新标签页中克隆当前会话（重新发起连接）    | Terminal   |
| `Cmd+Alt+C`                  | `Ctrl+Alt+C`                  | `DuplicateChannel`        | 在新标签页中复制通道（复用现有 SSH 连接）   | Terminal   |
| `Cmd+Alt+R`                  | `Ctrl+Alt+R`                  | `ReconnectTerminal`       | 重新连接断开的终端会话                      | Terminal   |
| `Cmd+M`                      | `Ctrl+Shift+M`                | `MinimizeTerminal`        | 最小化/折叠当前终端窗格（收起到下方任务栏） | Terminal   |
| `Cmd+C`                      | `Ctrl+Shift+C`                | `Copy`                    | 复制选中内容（避免与终端 SIGINT 冲突）      | Terminal   |
| `Cmd+V`                      | `Ctrl+Shift+V`                | `Paste`                   | 粘贴剪贴板文本                              | Terminal   |
| `Cmd+F`                      | `Ctrl+F`                      | `Search`                  | 终端内字符串 / 正则搜索                     | Search     |
| `Cmd+=`                      | `Ctrl+=`                      | `ZoomIn`                  | 放大终端字体                                | Terminal   |
| `Cmd+-`                      | `Ctrl+-`                      | `ZoomOut`                 | 缩小终端字体                                | Terminal   |
| `Cmd+0`                      | `Ctrl+0`                      | `ResetZoom`               | 重置终端字体缩放到默认                      | Terminal   |
| `Shift+PageUp`               | `Shift+PageUp`                | `ScrollUp`                | 向上滚动回滚缓冲区                          | Terminal   |
| `Shift+PageDown`             | `Shift+PageDown`              | `ScrollDown`              | 向下滚动回滚缓冲区                          | Terminal   |
| `Shift+Escape`               | `Shift+Escape`                | `ToggleFullscreen`        | 切换当前终端窗格全屏最大化                  | Fullscreen |
| `Cmd+Shift+]`                | `Ctrl+Tab`                    | `FocusNextTerminal`       | 聚焦下一个终端窗格                          | Navigation |
| `Cmd+Shift+[`                | `Ctrl+Shift+Tab`              | `FocusPrevTerminal`       | 聚焦上一个终端窗格                          | Navigation |
| `Cmd+Alt+Left/Right/Up/Down` | `Ctrl+Alt+Left/Right/Up/Down` | `FocusLeft/Right/Up/Down` | 按空间方向切换终端窗格焦点                  | Navigation |
| `Cmd+Up`                     | `Ctrl+Up`                     | `JumpToPreviousPrompt`    | 向上跳转至上一个 Prompt 提示符行            | Terminal   |
| `Cmd+Down`                   | `Ctrl+Down`                   | `JumpToNextPrompt`        | 向下跳转至下一个 Prompt 提示符行            | Terminal   |
| `Cmd+\``                     | `Ctrl+\``                     | `TogglePaneSwitcher`      | 快速分屏数字编号选择器（按数字直达）        | Navigation |
| `Cmd+Alt+E`                  | `Ctrl+Alt+E`                  | `EqualizeLayout`          | 均分当前所有分屏尺寸                        | Terminal   |

### 2.5 面板内操作与树列表键盘导航

| 按键      | Action / 操作      | 生效范围                                          | 说明                                                     |
| :-------- | :----------------- | :------------------------------------------------ | :------------------------------------------------------- |
| `F2`      | `RenameActiveNode` | 会话树 / 隧道树 / 服务树 / 快捷指令树 / SFTP 列表 | 重命名当前选中的节点                                     |
| `↑` / `↓` | 列表方向移动       | 会话树 / 快捷指令 / 历史命令 / 隧道 / 服务 / SFTP | 上下切换选中项并自动滚动可视区                           |
| `→`       | 展开目录           | 会话树 / 快捷指令 / 隧道 / 服务                   | 展开当前选中的文件夹节点                                 |
| `←`       | 收起目录           | 会话树 / 快捷指令 / 隧道 / 服务                   | 收起当前选中的文件夹节点                                 |
| `Enter`   | 激活 / 执行        | 会话树 / 快捷指令 / 历史命令 / 隧道 / 服务        | 连接会话 / 执行命令 / 投递历史 / 启动停止隧道 / 编辑服务 |
| `Delete`  | 删除选中项         | 会话树 / 快捷指令 / 历史记录 / 隧道 / 服务        | 弹出删除确认框或直接删除历史记录                         |
| `Space`   | 切换展开 / 单选    | 快捷指令 / 隧道 / 服务                            | 切换选中项展开状态或选中标记                             |

---

## 3. 未保存修改二次确认保护 (`soft_close`)

在表单输入与弹窗编辑态下，Velowork 全面接入了二次确认拦截防误触体系：
- 当用户在「新建/编辑会话」、「快捷指令编辑」、「设置修改」等对话框中输入了尚未保存的数据；
- 若触发 `Esc` 键、关闭按钮或点击模态遮罩外部，系统**不会直接关闭弹窗**，而是自动弹出“存在未保存修改”确认对话框；
- 确认放弃后方关闭并安全将焦点回还至触发前的原属控件，杜绝误触与焦点丢失。

