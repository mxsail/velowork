use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use velowork_core::types::{BellStyle, CursorShape};

/// 单一扁平的会话终端覆盖选项（所有协议共用）
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SessionTerminalOptions {
    // 视觉外观 (Live)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_scheme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_shape: Option<CursorShape>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_blink: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scrollback_lines: Option<u32>,

    // 交互行为 (Live)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_separators: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bell_style: Option<BellStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bell_cooldown_ms: Option<u32>,

    // 协议与仿真 (Reconnect)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub term_type: Option<String>,

    // 增强特性三态 (Reconnect: None=跟随全局, Some(true)=强制开, Some(false)=强制关)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell_integration: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bracketed_paste: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub osc52_clipboard: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub true_color: Option<bool>,
}

impl PartialEq for SessionTerminalOptions {
    fn eq(&self, other: &Self) -> bool {
        self.font_family == other.font_family
            && self.font_size.map(|v| v.to_bits()) == other.font_size.map(|v| v.to_bits())
            && self.color_scheme == other.color_scheme
            && self.cursor_shape == other.cursor_shape
            && self.cursor_blink == other.cursor_blink
            && self.scrollback_lines == other.scrollback_lines
            && self.word_separators == other.word_separators
            && self.bell_style == other.bell_style
            && self.bell_cooldown_ms == other.bell_cooldown_ms
            && self.charset == other.charset
            && self.term_type == other.term_type
            && self.shell_integration == other.shell_integration
            && self.bracketed_paste == other.bracketed_paste
            && self.osc52_clipboard == other.osc52_clipboard
            && self.true_color == other.true_color
    }
}

impl Eq for SessionTerminalOptions {}

/// 会话协议类型
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SessionProtocol {
    #[default]
    #[serde(rename = "ssh")]
    Ssh,
    #[serde(rename = "serial")]
    Serial,
    #[serde(rename = "telnet")]
    Telnet,
    #[serde(rename = "local")]
    Local,
}

impl SessionProtocol {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Ssh => "SSH",
            Self::Serial => "Serial",
            Self::Telnet => "Telnet",
            Self::Local => "Local",
        }
    }

    pub fn icon_name(&self) -> &'static str {
        match self {
            Self::Ssh => "ssh",
            Self::Serial => "serial",
            Self::Telnet => "telnet",
            Self::Local => "terminal",
        }
    }
}

/// SSH session configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SshSession {
    // 会话协议类型
    #[serde(default)]
    pub protocol: SessionProtocol,

    // 基础字段
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: SshAuthType,

    // 串口 (Serial) 专属配置
    #[serde(default)]
    pub serial_port: Option<String>,
    #[serde(default = "default_serial_baud_rate")]
    pub serial_baud_rate: u32,
    #[serde(default = "default_serial_data_bits")]
    pub serial_data_bits: u8,
    #[serde(default = "default_serial_stop_bits")]
    pub serial_stop_bits: u8,
    #[serde(default = "default_serial_parity")]
    pub serial_parity: String,
    #[serde(default = "default_serial_flow_control")]
    pub serial_flow_control: String,
    #[serde(default = "default_true")]
    pub serial_dtr: bool,
    #[serde(default = "default_true")]
    pub serial_rts: bool,
    #[serde(default = "default_serial_display_mode")]
    pub serial_display_mode: String,
    #[serde(default = "default_serial_line_ending")]
    pub serial_line_ending: String,
    #[serde(default)]
    pub serial_local_echo: bool,
    #[serde(default)]
    pub serial_timestamps: bool,
    #[serde(default)]
    pub serial_auto_reconnect: bool,

    // Telnet 专属配置
    #[serde(default)]
    pub telnet_host: Option<String>,
    #[serde(default = "default_telnet_port")]
    pub telnet_port: u16,
    #[serde(default, skip_serializing)]
    pub telnet_encoding: Option<String>,

    // 本地终端 (Local) 专属配置
    #[serde(default)]
    pub local_shell: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_cwd: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub local_env: HashMap<String, String>,

    // 常规设置
    #[serde(default)]
    pub icon_color: IconColor,

    // 目录/文件夹
    #[serde(default)]
    pub parent_folder_id: Option<String>,

    // 启动命令
    #[serde(default)]
    pub startup_command: Option<String>,

    // 认证设置
    #[serde(default)]
    pub save_credentials: bool,
    #[serde(default)]
    pub totp_secret: Option<String>,

    // 连接设置
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout: u32,
    #[serde(default = "default_keep_alive")]
    pub keep_alive_interval: u32,
    #[serde(default = "default_keep_alive_max")]
    pub keep_alive_max: u32,
    // 空闲断开超时（秒）：连接空闲超过该值才主动断开；0 = 关闭（永不因空闲断开）
    #[serde(default = "default_idle_disconnect_timeout")]
    pub idle_disconnect_timeout: u32,
    // 连接保活策略（场景化预设，仅用于 UI 记忆上次选择的策略；底层行为仍由下面三参数决定）
    #[serde(default = "default_keepalive_strategy")]
    pub keepalive_strategy: KeepAliveStrategy,
    #[serde(default = "default_true")]
    pub tcp_nodelay: bool,
    #[serde(default = "default_channel_buffer_size")]
    pub channel_buffer_size: u32,
    #[serde(default)]
    pub proxy_type: ProxyType,
    #[serde(default)]
    pub proxy_host: Option<String>,
    #[serde(default)]
    pub proxy_port: Option<u16>,
    #[serde(default)]
    pub proxy_username: Option<String>,
    #[serde(default)]
    pub proxy_password: Option<String>,
    /// ProxyJump: 引用另一条 SSH 会话作为跳板机。
    #[serde(default)]
    pub jump_session_id: Option<String>,
    #[serde(default = "default_true")]
    pub enable_sftp: bool,
    #[serde(default)]
    pub enable_x11_forwarding: bool,
    #[serde(default)]
    pub x11_display: Option<String>,
    #[serde(default)]
    pub enable_agent_forwarding: bool,
    #[serde(default)]
    pub enable_monitor: bool,
    #[serde(default = "default_true")]
    pub monitor_cpu: bool,
    #[serde(default = "default_true")]
    pub monitor_mem: bool,
    #[serde(default)]
    pub monitor_disk: bool,

    // 终端设置（稀疏覆盖，None = 使用系统默认值）
    #[serde(default)]
    pub terminal: SessionTerminalOptions,

    // 高级设置
    #[serde(default)]
    pub compression: CompressionType,
    #[serde(default)]
    pub strict_host_key: StrictHostKey,
    #[serde(default = "default_max_packets")]
    pub max_packets: u32,
    #[serde(default = "default_recv_window")]
    pub recv_window: u32,
    #[serde(default = "default_gex_min")]
    pub gex_min: u32,
    #[serde(default = "default_gex_preferred")]
    pub gex_preferred: u32,
    #[serde(default = "default_gex_max")]
    pub gex_max: u32,
    // 重新密钥时间限制（秒），对齐 russh Limits::rekey_time_limit
    #[serde(default = "default_rekey_time")]
    pub rekey_time: u32,

    // 算法列表
    // 是否自动推荐算法（true = Automatic，隐藏手动多选）
    #[serde(default = "default_true")]
    pub algorithms_automatic: bool,
    #[serde(default)]
    pub kex_algorithms: Vec<String>,
    #[serde(default)]
    pub cipher_algorithms: Vec<String>,
    #[serde(default)]
    pub mac_algorithms: Vec<String>,
    #[serde(default)]
    pub hostkey_algorithms: Vec<String>,

    // 备注
    #[serde(default)]
    pub tags: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

impl SshSession {
    /// 显式归一化方法：在反序列化或从 DB 读取后调用
    pub fn normalize(&mut self) {
        if self.terminal.charset.is_none() && let Some(ref enc) = self.telnet_encoding {
            let trimmed = enc.trim();
            if !trimmed.is_empty() {
                self.terminal.charset = Some(trimmed.to_string());
            }
        }
        self.telnet_encoding = None;
    }
}

impl Default for SshSession {
    fn default() -> Self {
        Self {
            protocol: SessionProtocol::Ssh,
            id: String::new(),
            name: String::new(),
            host: String::new(),
            port: 22,
            username: String::new(),
            auth_type: SshAuthType::Password { password: None },
            serial_port: None,
            serial_baud_rate: default_serial_baud_rate(),
            serial_data_bits: default_serial_data_bits(),
            serial_stop_bits: default_serial_stop_bits(),
            serial_parity: default_serial_parity(),
            serial_flow_control: default_serial_flow_control(),
            serial_dtr: default_true(),
            serial_rts: default_true(),
            serial_display_mode: default_serial_display_mode(),
            serial_line_ending: default_serial_line_ending(),
            serial_local_echo: false,
            serial_timestamps: false,
            serial_auto_reconnect: false,
            telnet_host: None,
            telnet_port: default_telnet_port(),
            telnet_encoding: None,
            local_shell: None,
            local_cwd: None,
            local_env: HashMap::new(),
            icon_color: IconColor::default(),
            parent_folder_id: None,
            startup_command: None,
            save_credentials: false,
            totp_secret: None,
            connection_timeout: default_connection_timeout(),
            keep_alive_interval: default_keep_alive(),
            keep_alive_max: default_keep_alive_max(),
            idle_disconnect_timeout: default_idle_disconnect_timeout(),
            keepalive_strategy: default_keepalive_strategy(),
            tcp_nodelay: default_true(),
            channel_buffer_size: default_channel_buffer_size(),
            proxy_type: ProxyType::default(),
            proxy_host: None,
            proxy_port: None,
            proxy_username: None,
            proxy_password: None,
            jump_session_id: None,
            enable_sftp: default_true(),
            enable_x11_forwarding: false,
            x11_display: None,
            enable_agent_forwarding: false,
            enable_monitor: false,
            monitor_cpu: default_true(),
            monitor_mem: default_true(),
            monitor_disk: false,
            terminal: SessionTerminalOptions::default(),
            compression: CompressionType::default(),
            strict_host_key: StrictHostKey::default(),
            max_packets: default_max_packets(),
            recv_window: default_recv_window(),
            gex_min: default_gex_min(),
            gex_preferred: default_gex_preferred(),
            gex_max: default_gex_max(),
            rekey_time: default_rekey_time(),
            algorithms_automatic: default_true(),
            kex_algorithms: Vec::new(),
            cipher_algorithms: Vec::new(),
            mac_algorithms: Vec::new(),
            hostkey_algorithms: Vec::new(),
            tags: None,
            notes: None,
        }
    }
}

impl PartialEq for SshSession {
    fn eq(&self, other: &Self) -> bool {
        self.protocol == other.protocol
            && self.id == other.id
            && self.name == other.name
            && self.host == other.host
            && self.port == other.port
            && self.username == other.username
            && self.auth_type == other.auth_type
            && self.serial_port == other.serial_port
            && self.serial_baud_rate == other.serial_baud_rate
            && self.serial_data_bits == other.serial_data_bits
            && self.serial_stop_bits == other.serial_stop_bits
            && self.serial_parity == other.serial_parity
            && self.serial_flow_control == other.serial_flow_control
            && self.serial_dtr == other.serial_dtr
            && self.serial_rts == other.serial_rts
            && self.telnet_host == other.telnet_host
            && self.telnet_port == other.telnet_port
            && self.telnet_encoding == other.telnet_encoding
            && self.local_shell == other.local_shell
            && self.local_cwd == other.local_cwd
            && self.local_env == other.local_env
            && self.icon_color == other.icon_color
            && self.parent_folder_id == other.parent_folder_id
            && self.startup_command == other.startup_command
            && self.save_credentials == other.save_credentials
            && self.totp_secret == other.totp_secret
            && self.connection_timeout == other.connection_timeout
            && self.keep_alive_interval == other.keep_alive_interval
            && self.keep_alive_max == other.keep_alive_max
            && self.idle_disconnect_timeout == other.idle_disconnect_timeout
            && self.keepalive_strategy == other.keepalive_strategy
            && self.tcp_nodelay == other.tcp_nodelay
            && self.channel_buffer_size == other.channel_buffer_size
            && self.proxy_type == other.proxy_type
            && self.proxy_host == other.proxy_host
            && self.proxy_port == other.proxy_port
            && self.proxy_username == other.proxy_username
            && self.proxy_password == other.proxy_password
            && self.jump_session_id == other.jump_session_id
            && self.enable_sftp == other.enable_sftp
            && self.enable_x11_forwarding == other.enable_x11_forwarding
            && self.x11_display == other.x11_display
            && self.enable_monitor == other.enable_monitor
            && self.monitor_cpu == other.monitor_cpu
            && self.monitor_mem == other.monitor_mem
            && self.monitor_disk == other.monitor_disk
            && self.terminal == other.terminal
            && self.compression == other.compression
            && self.strict_host_key == other.strict_host_key
            && self.max_packets == other.max_packets
            && self.recv_window == other.recv_window
            && self.gex_min == other.gex_min
            && self.gex_preferred == other.gex_preferred
            && self.gex_max == other.gex_max
            && self.rekey_time == other.rekey_time
            && self.algorithms_automatic == other.algorithms_automatic
            && self.kex_algorithms == other.kex_algorithms
            && self.cipher_algorithms == other.cipher_algorithms
            && self.mac_algorithms == other.mac_algorithms
            && self.hostkey_algorithms == other.hostkey_algorithms
            && self.tags == other.tags
            && self.notes == other.notes
    }
}

/// 连接保活策略（UI 场景化预设）。底层行为仍由 keep_alive_interval /
/// keep_alive_max / idle_disconnect_timeout 三个参数决定，预设只是便捷映射。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum KeepAliveStrategy {
    /// 持续在线（默认）：频繁探测，防止 NAT / 防火墙超时而断开
    #[default]
    Always,
    /// 按需节省：长间隔探测 + 空闲超时，空闲时更快释放资源
    Save,
    /// 极简模式：禁用所有保活机制
    Minimal,
    /// 自定义：由用户手动设置三个底层参数
    Custom,
}

/// SSH 认证方式
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum SshAuthType {
    /// 密码认证
    Password {
        #[serde(default)]
        password: Option<String>,
    },
    /// 公钥认证
    PrivateKey {
        key_path: String,
        #[serde(default)]
        passphrase: Option<String>,
    },
    /// 键盘交互认证
    KeyboardInteractive,
    /// SSH Agent 认证
    SshAgent {
        #[serde(default)]
        socket_path: Option<String>,
    },
}

/// 图标颜色
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IconColor {
    #[default]
    Default,
    Blue,
    Green,
    Orange,
    Red,
}

impl IconColor {
    pub fn display_name(self) -> &'static str {
        match self {
            IconColor::Default => "Default",
            IconColor::Blue => "Blue",
            IconColor::Green => "Green",
            IconColor::Orange => "Orange",
            IconColor::Red => "Red",
        }
    }

    pub fn color_hex(self) -> Option<u32> {
        match self {
            IconColor::Default => None,
            IconColor::Blue => Some(0x5B6BD6),
            IconColor::Green => Some(0x2DB86A),
            IconColor::Orange => Some(0xD4A937),
            IconColor::Red => Some(0xD94F4F),
        }
    }

    pub fn all_variants() -> &'static [IconColor] {
        &[
            IconColor::Default,
            IconColor::Blue,
            IconColor::Green,
            IconColor::Orange,
            IconColor::Red,
        ]
    }
}

/// 代理类型
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProxyType {
    #[default]
    None,
    Socks5,
    Http,
    Jump,
}

impl ProxyType {
    pub fn display_name(self) -> &'static str {
        match self {
            ProxyType::None => "None",
            ProxyType::Socks5 => "SOCKS5",
            ProxyType::Http => "HTTP CONNECT",
            ProxyType::Jump => "Jump Host",
        }
    }

    pub fn all_variants() -> &'static [ProxyType] {
        &[
            ProxyType::None,
            ProxyType::Socks5,
            ProxyType::Http,
            ProxyType::Jump,
        ]
    }
}

/// 压缩类型
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CompressionType {
    #[default]
    Zlib,
    None,
    ZlibOpenSsh,
}

impl CompressionType {
    pub fn display_name(self) -> &'static str {
        match self {
            CompressionType::Zlib => "zlib (Enabled)",
            CompressionType::None => "none (Disabled)",
            CompressionType::ZlibOpenSsh => "zlib@openssh.com",
        }
    }

    pub fn all_variants() -> &'static [CompressionType] {
        &[
            CompressionType::Zlib,
            CompressionType::None,
            CompressionType::ZlibOpenSsh,
        ]
    }
}

/// 严格主机密钥验证
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StrictHostKey {
    #[default]
    AcceptNew,
    Yes,
    No,
}

impl StrictHostKey {
    pub fn display_name(self) -> &'static str {
        match self {
            StrictHostKey::Yes => "yes (Verify)",
            StrictHostKey::No => "no (Accept Any)",
            StrictHostKey::AcceptNew => "accept-new",
        }
    }

    pub fn all_variants() -> &'static [StrictHostKey] {
        &[
            StrictHostKey::Yes,
            StrictHostKey::No,
            StrictHostKey::AcceptNew,
        ]
    }
}

// 默认值函数
fn default_connection_timeout() -> u32 {
    30
}
fn default_keep_alive() -> u32 {
    60
}
fn default_keep_alive_max() -> u32 {
    3
}
fn default_idle_disconnect_timeout() -> u32 {
    0
}
fn default_keepalive_strategy() -> KeepAliveStrategy {
    KeepAliveStrategy::Always
}
fn default_max_packets() -> u32 {
    32768
}
fn default_recv_window() -> u32 {
    2097152
} // 2MB
fn default_channel_buffer_size() -> u32 {
    100
}
fn default_true() -> bool {
    true
}
fn default_gex_min() -> u32 {
    2048
}
fn default_gex_preferred() -> u32 {
    4096
}
fn default_gex_max() -> u32 {
    8192
}
fn default_rekey_time() -> u32 {
    3600
}
fn default_serial_baud_rate() -> u32 {
    115200
}
fn default_serial_data_bits() -> u8 {
    8
}
fn default_serial_stop_bits() -> u8 {
    1
}
fn default_serial_parity() -> String {
    "none".to_string()
}
fn default_serial_flow_control() -> String {
    "none".to_string()
}
fn default_serial_display_mode() -> String {
    "text".to_string()
}
fn default_serial_line_ending() -> String {
    "crlf".to_string()
}
fn default_telnet_port() -> u16 {
    23
}

/// 会话树节点
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SessionTreeNode {
    Folder {
        id: String,
        name: String,
        children: Vec<SessionTreeNode>,
        #[serde(default)]
        is_collapsed: bool,
    },
    Session {
        session: SshSession,
    },
}

impl PartialEq for SessionTreeNode {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                SessionTreeNode::Folder {
                    id: id1,
                    name: name1,
                    children: children1,
                    is_collapsed: collapsed1,
                },
                SessionTreeNode::Folder {
                    id: id2,
                    name: name2,
                    children: children2,
                    is_collapsed: collapsed2,
                },
            ) => id1 == id2 && name1 == name2 && children1 == children2 && collapsed1 == collapsed2,
            (
                SessionTreeNode::Session { session: session1 },
                SessionTreeNode::Session { session: session2 },
            ) => session1 == session2,
            _ => false,
        }
    }
}

impl SessionTreeNode {
    pub fn id(&self) -> &str {
        match self {
            SessionTreeNode::Folder { id, .. } => id,
            SessionTreeNode::Session { session } => &session.id,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            SessionTreeNode::Folder { name, .. } => name,
            SessionTreeNode::Session { session } => &session.name,
        }
    }
}

/// SSH 会话配置
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct SshSessionConfig {
    #[serde(default)]
    pub tree: Vec<SessionTreeNode>,
    #[serde(default)]
    pub by_project: std::collections::HashMap<String, Vec<SessionTreeNode>>,
}

impl SshSessionConfig {
    pub fn tree_for_project(&self, project_id: Option<&str>) -> &[SessionTreeNode] {
        let pid = project_id.unwrap_or("default");
        if let Some(nodes) = self.by_project.get(pid) {
            nodes.as_slice()
        } else if (pid == "default" || pid.is_empty()) && !self.tree.is_empty() {
            &self.tree
        } else {
            &[]
        }
    }

    /// Collect all sessions across every project (deduplicated by session id).
    pub fn all_sessions(&self) -> Vec<(&str, &str)> {
        fn recurse<'a>(
            nodes: &'a [SessionTreeNode],
            seen: &mut std::collections::HashSet<&'a str>,
            out: &mut Vec<(&'a str, &'a str)>,
        ) {
            for node in nodes {
                match node {
                    SessionTreeNode::Session { session } => {
                        if seen.insert(&session.id) {
                            out.push((session.id.as_str(), session.name.as_str()));
                        }
                    }
                    SessionTreeNode::Folder { children, .. } => {
                        recurse(children, seen, out);
                    }
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        recurse(&self.tree, &mut seen, &mut result);
        for nodes in self.by_project.values() {
            recurse(nodes, &mut seen, &mut result);
        }
        result
    }

    /// Collect all SSH sessions (protocol == Ssh) across every project (deduplicated by session id).
    pub fn all_ssh_sessions(&self) -> Vec<(&str, &str)> {
        fn recurse<'a>(
            nodes: &'a [SessionTreeNode],
            seen: &mut std::collections::HashSet<&'a str>,
            out: &mut Vec<(&'a str, &'a str)>,
        ) {
            for node in nodes {
                match node {
                    SessionTreeNode::Session { session } => {
                        if session.protocol == SessionProtocol::Ssh && seen.insert(&session.id) {
                            out.push((session.id.as_str(), session.name.as_str()));
                        }
                    }
                    SessionTreeNode::Folder { children, .. } => {
                        recurse(children, seen, out);
                    }
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        recurse(&self.tree, &mut seen, &mut result);
        for nodes in self.by_project.values() {
            recurse(nodes, &mut seen, &mut result);
        }
        result
    }


    pub fn tree_for_project_mut(&mut self, project_id: Option<&str>) -> &mut Vec<SessionTreeNode> {
        let pid = match project_id {
            Some(id) if !id.trim().is_empty() => id.trim().to_string(),
            _ => "default".to_string(),
        };
        if pid == "default" && !self.tree.is_empty() && !self.by_project.contains_key(&pid) {
            self.by_project
                .insert(pid.clone(), std::mem::take(&mut self.tree));
        }
        self.by_project.entry(pid).or_default()
    }

    pub fn find_session(&self, id: &str) -> Option<&SshSession> {
        fn find_in_nodes<'a>(nodes: &'a [SessionTreeNode], id: &str) -> Option<&'a SshSession> {
            for node in nodes {
                match node {
                    SessionTreeNode::Session { session } => {
                        if session.id == id {
                            return Some(session);
                        }
                    }
                    SessionTreeNode::Folder { children, .. } => {
                        if let Some(session) = find_in_nodes(children, id) {
                            return Some(session);
                        }
                    }
                }
            }
            None
        }
        if let Some(session) = find_in_nodes(&self.tree, id) {
            return Some(session);
        }
        for nodes in self.by_project.values() {
            if let Some(session) = find_in_nodes(nodes, id) {
                return Some(session);
            }
        }
        None
    }

    pub fn load_from_disk() -> Option<Self> {
        let root = if let Some(p) = velowork_core::profiles::try_current() {
            p.root.clone()
        } else {
            velowork_core::profiles::config_root()
        };
        let path = root.join("ssh_sessions.json");
        if path.exists()
            && let Ok(content) = std::fs::read_to_string(&path)
            && let Ok(config) = serde_json::from_str::<SshSessionConfig>(&content)
        {
            return Some(config);
        }
        None
    }
}

/// SSH 测试连接结果
#[derive(Clone, Debug)]
pub struct SshTestResult {
    pub success: bool,
    pub latency_ms: u64,
    pub server_version: Option<String>,
    pub error: Option<String>,
}
