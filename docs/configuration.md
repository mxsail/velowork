# Velowork Configuration & Storage Guide

Velowork stores configuration, database records, and runtime states in your operating system's standard application data directory (resolved via the `dirs` crate):

| Platform | Path |
| :--- | :--- |
| **macOS** | `~/Library/Application Support/velowork/` |
| **Linux** | `~/.config/velowork/` |
| **Windows** | `%APPDATA%\velowork\` |

---

## 1. Storage Architecture (Profiles)

Velowork is designed around **Profiles** — isolated, self-contained business units. You can select the active profile via the CLI flag `--profile <id>`, the `VELOWORK_PROFILE` environment variable, or the profile index (`profiles.json`).

Each Profile resides under `<config-dir>/profiles/<id>/` with a clean layered architecture:

```text
<config-dir>/
├── profiles.json                   # Profile index (id, display_name, last_used)
├── profiles/<id>/                  # Self-contained, syncable Profile business unit
│   ├── manifest.json               # Profile manifest (DB, config, and schema versions)
│   ├── config/                     # Human-editable JSON configurations
│   │   ├── settings.json           # Unified settings (appearance, terminal, AI, sync, proxy)
│   │   └── keybindings.json        # Custom keybinding overrides
│   ├── data/                       # Core business data storage
│   │   └── velowork.db             # Single embedded SQLite database
│   ├── themes/                     # Custom theme JSON definitions
│   └── sessions/                   # SSH keys (*.pem), imported configs & certificates
├── logs/<id>/velowork.log          # Per-profile application runtime logs
├── runtime/<id>/                   # IPC sockets, PID files, and temporary buffers
├── cache/                          # Update packages & font caches
└── locks/<id>.lock                 # Process instance lock
```

### 1.1 SQLite Business Database (`data/velowork.db`)

All persistent structured business data is unified in the SQLite database file `data/velowork.db`. Key tables include:

| Table | Content & Responsibility |
| :--- | :--- |
| `workspace` | Workspace state, project layout trees, terminal splits, active tabs, and column geometry. |
| `session_tree_node` | Hierarchical SSH session tree (folders, hosts, ports, usernames, auth method references). |
| `tunnel_tree_node` | SSH port-forwarding tunnel definitions (Local, Remote, Dynamic SOCKS5). |
| `service_tree_node` | Custom service monitoring tree (Systemd, Docker containers, Command scripts). |
| `snippets` | Quick command scripts and snippet library. |
| `credentials` | Credential metadata and account identifiers. |
| `security` | Profile lock status, password hash, and timeout policies. |

### 1.2 Credentials & Security Model

Credential **secrets** (passwords, private key passphrases, API keys) are **never stored in plaintext** within `velowork.db` or config files. They are managed through the `CredentialProvider` pipeline:

1. **System Keyring (`KeyringProvider`)**: Uses OS-level credential storage (macOS Keychain, Linux Secret Service / Keyutils, Windows Credential Manager).
2. **Encrypted Vault (`EncryptedFileProvider`)**: Fallback encrypted storage using AES-256-GCM under `<config-dir>/.vault_key`.
3. **Memory (`MemoryProvider`)**: Ephemeral in-memory store for automated testing.

---

## 2. Settings Configuration (`config/settings.json`)

The unified configuration file for all user preferences. Created automatically with defaults on first launch. Accessible via **Settings Panel** (`Cmd+,` / `Ctrl+,`) or direct file editing (`Cmd+Alt+,` / `Ctrl+Alt+,`).

```json
{
  "version": 1,
  "color_schema": "System",
  "dark_color_theme": "Dark",
  "light_color_theme": "Light",
  "font_family": "JetBrains Mono",
  "font_size": 14.0,
  "ui_font_size": 13.0,
  "ui_font_family": "system-ui",
  "file_font_size": 12.0,
  "line_height": 1.3,
  "cursor_style": "Bar",
  "cursor_blink": true,
  "scrollback_lines": 10000,
  "default_shell": "Default",
  "show_shell_selector": false,
  "session_backend": "Auto",
  "file_opener": "",
  "show_focused_border": false,
  "show_line_numbers": false,
  "terminal_ctrl_c_copies_selection": false,
  "sidebar": {
    "is_open": false,
    "auto_hide": false,
    "width": 250
  },
  "proxy_mode": "none",
  "proxy_host": "",
  "proxy_port": 7890,
  "start_on_boot": false,
  "close_behavior": "confirm",
  "auto_check_updates": true,
  "bg_opacity": 1.0,
  "tab_width_mode": "compact",
  "diff_view_mode": "Unified",
  "diff_ignore_whitespace": false,
  "idle_timeout_secs": 0,
  "min_column_width": 400,
  "enabled_extensions": [],
  "notifications": {
    "enabled": true,
    "sound": true
  },
  "ai_enabled": true,
  "ai_default_model_id": "deepseek-chat",
  "ai_temperature": 0.7,
  "ai_max_tokens": 4096,
  "ai_max_context_tokens": 32000,
  "ai_max_history_messages": 20,
  "ai_auto_compress": true,
  "ai_models": [
    {
      "id": "deepseek-chat",
      "provider": "deepseek",
      "name": "DeepSeek V3",
      "base_url": "https://api.deepseek.com/v1",
      "model_name": "deepseek-chat"
    }
  ],
  "title_position": "Center",
  "show_active_session_in_title": true,
  "status_bar_mode": "Always",
  "sync": {
    "enabled": false,
    "provider": "s3",
    "webdav": {
      "server_url": "https://dav.example.com/velowork",
      "username": "user"
    },
    "s3": {
      "endpoint": "https://s3.amazonaws.com",
      "bucket": "my-velowork-backup",
      "region": "us-east-1",
      "access_key_id": "YOUR_KEY_ID",
      "prefix": "velowork/",
      "path_style": false
    },
    "auto_sync": true,
    "sync_interval_secs": 1800,
    "data_scope": {
      "sessions": true,
      "tunnels": true,
      "services": true,
      "quick_commands": true,
      "ai_chat": true,
      "command_history": true,
      "settings": true,
      "themes": true,
      "credentials": true
    }
  }
}
```

### Settings Reference

#### Appearance
| Key | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `color_schema` | string | `"System"` | Appearance mode: `"Dark"`, `"Light"`, or `"System"`. |
| `dark_color_theme` | string | `"Dark"` | Palette for dark mode: `"Dark"`, `"Light"`, `"PastelDark"`, `"HighContrast"`, `"Custom"`. |
| `light_color_theme` | string | `"Light"` | Palette for light mode. Same options as `dark_color_theme`. |
| `custom_theme_id` | string | `""` | File stem of the active custom theme file. |
| `font_family` | string | `"JetBrains Mono"` | Monospace terminal font family. |
| `font_size` | float | `14.0` | Monospace font size (8.0 - 48.0). |
| `ui_font_size` | float | `13.0` | UI controls and dialogs font size (8.0 - 24.0). |
| `ui_font_family` | string | `"system-ui"` | Font family for UI chrome and dialogs. |
| `line_height` | float | `1.3` | Terminal line height multiplier (1.0 - 3.0). |
| `bg_opacity` | float | `1.0` | Window background opacity (0.1 - 1.0). |
| `tab_width_mode` | string | `"compact"` | Tab header width mode: `"compact"`, `"equal"`, `"flexible"`. |
| `show_focused_border` | bool | `false` | Highlight active terminal pane with a focus border. |
| `title_position` | string | `"Center"` | Titlebar title text alignment: `"Center"`, `"Left"`, `"Right"`. |
| `show_active_session_in_title` | bool | `true` | Format window title dynamically as `{session_name} - Velowork`. |
| `status_bar_mode` | string | `"Always"` | Status bar visibility mode: `"Always"`, `"AutoHide"` (32px), or `"Never"`. |

#### Terminal
| Key | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `cursor_style` | string | `"Bar"` | Cursor shape: `"Block"`, `"Bar"`, or `"Underline"`. |
| `cursor_blink` | bool | `true` | Enable cursor blinking animation. |
| `scrollback_lines` | int | `10000` | Terminal scrollback buffer limit (100 - 100,000). |
| `default_shell` | string | `"Default"` | Initial shell program for local terminals (`"Default"`, `"Bash"`, `"Zsh"`, `"Fish"`, `"PowerShell"`, `"Cmd"`). |
| `show_shell_selector` | bool | `false` | Show shell selector dropdown in terminal header. |
| `show_line_numbers` | bool | `false` | Show line numbers gutter along the left edge. |
| `terminal_ctrl_c_copies_selection` | bool | `false` | When text is selected, Ctrl+C copies instead of sending SIGINT. |
| `idle_timeout_secs` | int | `0` | Seconds of inactivity before marking a session idle (`0` = disabled). |

#### AI Assistant
| Key | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `ai_enabled` | bool | `true` | Enable or disable the AI assistant subsystem. |
| `ai_default_model_id` | string | `""` | ID of the default AI model to use for conversations. |
| `ai_temperature` | float | `0.7` | Sampling temperature for AI model responses (0.0 - 2.0). |
| `ai_max_tokens` | int | `4096` | Maximum generation tokens per completion. |
| `ai_max_context_tokens` | int | `32000` | Maximum token limit for conversation context window. |
| `ai_max_history_messages`| int | `20` | Maximum conversation messages to retain in context. |
| `ai_auto_compress` | bool | `true` | Auto-compress conversation history when approaching limit. |
| `ai_models` | array | `[...]` | Configured AI model profiles (OpenAI, Claude, DeepSeek, Ollama). |

> **Note**: AI provider API keys are stored securely in the system keyring and omitted from plain JSON export.

#### Cloud Sync (WebDAV & S3)
| Key | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `sync.enabled` | bool | `false` | Enable or disable profile cloud synchronization. |
| `sync.provider` | string | `"s3"` | Sync protocol provider: `"s3"` or `"webdav"`. |
| `sync.s3.endpoint` | string | `""` | S3 server endpoint URL (e.g. `https://s3.amazonaws.com` or `http://192.168.1.100:9000`). |
| `sync.s3.bucket` | string | `""` | Remote bucket name for sync bundles. |
| `sync.s3.region` | string | `"us-east-1"`| S3 storage region. |
| `sync.s3.access_key_id` | string | `""` | S3 Access Key ID. |
| `sync.s3.prefix` | string | `"velowork/"` | Storage folder prefix. |
| `sync.s3.path_style` | bool | `false` | Force Path-Style access (auto-enabled when endpoint is an IP address). |
| `sync.webdav.server_url` | string | `""` | WebDAV server endpoint URL. |
| `sync.webdav.username` | string | `""` | WebDAV login username. |
| `sync.auto_sync` | bool | `false` | Automatically synchronize profile on app launch and periodic intervals. |
| `sync.sync_interval_secs`| int | `1800` | Periodic sync interval in seconds (default: 30 minutes). |
| `sync.data_scope.*` | bool | `true` | Granular toggles for `sessions`, `tunnels`, `services`, `quick_commands`, `ai_chat`, `command_history`, `settings`, `themes`, `credentials`. |

##### Multi-Device Smart Merge & Snapshot Backup
Velowork synchronizes user profiles across devices using an intelligent three-way merge and snapshot protection mechanism:
- **Causal Merging**: When synchronizing with remote WebDAV or S3 storage, Velowork analyzes entity revisions and modification timestamps to automatically merge non-conflicting modifications made on different devices (such as new SSH sessions added on a laptop and terminal shortcuts modified on a desktop).
- **Automated Local Snapshots**: Prior to every sync execution, a full local snapshot backup is automatically created and archived. Velowork retains the **last 10 snapshots** by default, ensuring point-in-time recovery and preventing unexpected data loss.
- **End-to-End Encryption**: Sync bundles are encrypted with AES-256-GCM using keys derived from the user's Master Password before leaving the local machine.
- **Conflict Handling**: In the event of concurrent modifications on the same entity, the selected conflict resolution strategy resolves differences deterministically while preserving auditability.

#### Network & Proxy
| Key | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `proxy_mode` | string | `"none"` | Proxy mode: `"none"`, `"system"`, `"http"`. |
| `proxy_host` | string | `""` | HTTP proxy server hostname or IP address. |
| `proxy_port` | int | `7890` | HTTP proxy server port number. |

#### General & System
| Key | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `start_on_boot` | bool | `false` | Auto-launch Velowork on operating system startup. |
| `close_behavior` | string | `"confirm"` | Window close action: `"confirm"`, `"minimize"`, or `"quit"`. |
| `auto_check_updates` | bool | `true` | Automatically check for application updates on startup and in the background. |
| `file_opener` | string | `""` | External editor command for opening paths (`"code"`, `"cursor"`, `"zed"`, etc.). |
| `enabled_extensions` | array | `[]` | Enabled extension IDs. |

---

## 3. Custom Keybindings (`config/keybindings.json`)

User keybindings override built-in defaults. The configuration maps action names to binding arrays:

```json
{
  "version": 1,
  "bindings": {
    "SplitVertical": [
      { "keystroke": "cmd-d", "context": "TerminalPane" },
      { "keystroke": "ctrl-shift-d", "context": "TerminalPane" }
    ],
    "ToggleLeftDock": [
      { "keystroke": "cmd-b", "enabled": true }
    ]
  }
}
```

### Syntax & Rules
- **Modifiers**: `cmd`, `ctrl`, `alt`, `shift`. Joined by `-` (e.g. `ctrl-shift-p`).
- **Chords**: Two sequential strokes separated by a space (e.g. `ctrl-k ctrl-s`).
- **Context Scoping**: Optional `context` constraint (`"TerminalPane"`, `"Editor"`, `"SearchBar"`).

> 📖 For the complete list of built-in actions and default shortcut assignments, see [Velowork 快捷键完整映射表](shortcuts.md)。

---

## 4. Custom Themes (`themes/*.json`)

Place custom theme definitions in `themes/` within your profile directory:

| Platform | Custom Themes Directory Path |
| :--- | :--- |
| **macOS** | `~/Library/Application Support/velowork/profiles/<profile_id>/themes/*.json` |
| **Linux** | `~/.config/velowork/profiles/<profile_id>/themes/*.json` |
| **Windows** | `%APPDATA%\velowork\profiles\<profile_id>\themes\*.json` |

Velowork 会自动扫描并加载 `themes/` 目录下的所有 `*.json` 自定义主题文件，并在主题选择器（快捷键 `Ctrl+K Ctrl+T`）中即时呈现供选择与预览。

### 4.1 主题文件结构示例

自定义主题专心定义 21 项纯粹的核心 UI 颜色（十六进制 `#rrggbb` 格式）：

```json
{
  "name": "Catppuccin Mocha",
  "description": "Soothing pastel theme for high-spirited computing",
  "is_dark": true,
  "colors": {
    "bg_primary": "#1e1e2e",
    "bg_secondary": "#181825",
    "bg_header": "#11111b",
    "bg_panel": "#181825",
    "bg_selection": "#313244",
    "bg_hover": "#313244",
    "accent": "#89b4fa",
    "border": "#313244",
    "border_active": "#89b4fa",
    "text_primary": "#cdd6f4",
    "text_secondary": "#a6adc8",
    "text_muted": "#6c7086",
    "success": "#a6e3a1",
    "warning": "#f9e2af",
    "error": "#f38ba8",
    "folder_default": "#f9e2af",
    "folder_red": "#f38ba8",
    "folder_orange": "#fab387",
    "folder_yellow": "#f9e2af",
    "folder_lime": "#a6e3a1",
    "folder_green": "#a6e3a1",
    "folder_teal": "#94e2d5",
    "folder_cyan": "#89dceb",
    "folder_blue": "#89b4fa",
    "folder_indigo": "#b4befe",
    "folder_purple": "#cba6f7",
    "folder_pink": "#f5c2e7"
  }
}
```

### 4.2 色彩配置字段参考（21 项 UI Token）

| 分类 | 字段名 | 说明 |
| :--- | :--- | :--- |
| **基础信息** | `name` | 主题显示名称 |
| | `description` | 主题描述 |
| | `is_dark` | 是否为暗色主题（用于匹配系统的暗色/亮色外观切换） |
| **界面背景与强调** | `bg_primary` | 应用工作区主背景色 |
| | `bg_secondary` | 侧边栏/卡片/辅助背景色 |
| | `bg_header` | 标题栏与标签栏条状背景色 |
| | `bg_panel` | 弹出面板与对话框外壳背景色 |
| | `bg_selection` | 列表项与文本选区选中背景色 |
| | `bg_hover` | 控件鼠标悬浮背景色 |
| | `accent` | 系统强调/品牌色彩（主操作按钮背景与焦点光标基色） |
| **边框与活动状态** | `border` | 标准分割线与边框颜色 |
| | `border_active` | 活动面板/活动项/聚焦窗格高亮边框色 |
| **排版与提示** | `text_primary` | 主文字颜色 |
| | `text_secondary` | 次要文字颜色 |
| | `text_muted` | 占位符、空闲指示与微弱提示文字颜色 |
| **通用语义状态** | `success` | 成功/正常运行/已连接指示色 |
| | `warning` | 警告/高负载/终端响铃告警指示色 |
| | `error` | 错误/失败/断开连接指示色 |
| **文件夹 12 色** | `folder_default`, `folder_red` ~ `folder_pink` | 项目管理/会话树中各文件夹的分类颜色标签 |

### 4.3 语义派生与终端配色方案

- **微观控件纯语义化派生**：
  - **按钮交互**：主按钮（`ButtonVariant::Primary`）消费 `surface_accent`（由 `accent` 生成），悬停态与高对比文字颜色由 `SemanticPalette` 自动动态派生。
  - **气泡提示 (Tooltip)**：背景统一消费 `bg_panel`，文字自适应 `text_primary`，并带细微 `border` 描边。
  - **搜索高亮**：当前聚焦匹配项使用高亮 `accent`，其他匹配项使用半透明弱化态 `with_alpha(accent, 0.4)` 自动派生。
  - **终端边框状态**：聚焦使用 `border_active`，响铃告警使用 `warning`，空闲指示使用 `text_muted`。
- **终端配色方案 (`TerminalPalette`)**：终端内部的 ANSI 渲染与颜色查询完全独立，可在设置中配置 `settings.color_scheme`（支持 Dark, OneDark, Dracula, Nord, SolarizedDark, Monokai, GruvboxDark, GitHubDark, CatppuccinMocha 等 9 套专业终端预设）。
- **背景透明度 (`bg_opacity`)**：在设置中通过 `settings.bg_opacity`（10% ~ 100%）独立调节，由 UI 语义层 `SemanticPalette` 自动动态混合，不侵入静态色彩定义。
