//! 逐字段实时校验：`FieldId` 枚举 + 定长数组索引（取代字符串键 HashMap），
//! 三级严重度（Info / Warning / Error），仅 Error 阻断保存。

use super::section::SshSection;
use velowork_state::SessionProtocol;

/// 可校验字段的类型安全标识。数组下标 = discriminant。
///
/// 新增字段时在此追加变体，并同步更新 [`FieldId::COUNT`]（自动通过 `All` 派生）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldId {
    Name,
    Host,
    Port,
    Username,
    Password,
    KeyPath,
    Passphrase,
    SerialPort,
    TelnetHost,
    TelnetPort,
    ConnectionTimeout,
    KeepaliveInterval,
    KeepaliveMax,
    IdleTimeout,
    ChannelBuffer,
    ProxyHost,
    ProxyPort,
    Scrollback,
    MaxPackets,
    RecvWindow,
    GexMin,
    GexPreferred,
    GexMax,
    RekeyTime,
    StartupCommand,
    ProxyUsername,
    ProxyPassword,
    Tags,
    Notes,
    WordSeparators,
}

impl FieldId {
    /// 所有字段，顺序即 discriminant 索引。
    pub const ALL: &'static [FieldId] = &[
        FieldId::Name,
        FieldId::Host,
        FieldId::Port,
        FieldId::Username,
        FieldId::Password,
        FieldId::KeyPath,
        FieldId::Passphrase,
        FieldId::SerialPort,
        FieldId::TelnetHost,
        FieldId::TelnetPort,
        FieldId::ConnectionTimeout,
        FieldId::KeepaliveInterval,
        FieldId::KeepaliveMax,
        FieldId::IdleTimeout,
        FieldId::ChannelBuffer,
        FieldId::ProxyHost,
        FieldId::ProxyPort,
        FieldId::Scrollback,
        FieldId::MaxPackets,
        FieldId::RecvWindow,
        FieldId::GexMin,
        FieldId::GexPreferred,
        FieldId::GexMax,
        FieldId::RekeyTime,
        FieldId::StartupCommand,
        FieldId::ProxyUsername,
        FieldId::ProxyPassword,
        FieldId::Tags,
        FieldId::Notes,
        FieldId::WordSeparators,
    ];

    /// 字段总数，用于分配定长校验数组。
    pub const COUNT: usize = FieldId::ALL.len();

    /// 数组索引。
    pub const fn index(self) -> usize {
        self as usize
    }

    /// 根据表单标识字符串解析 FieldId。
    pub fn from_key(key: &str) -> Option<FieldId> {
        Some(match key {
            "name" => FieldId::Name,
            "host" => FieldId::Host,
            "port" => FieldId::Port,
            "username" => FieldId::Username,
            "password" => FieldId::Password,
            "key_path" => FieldId::KeyPath,
            "passphrase" => FieldId::Passphrase,
            "serial_port" => FieldId::SerialPort,
            "telnet_host" => FieldId::TelnetHost,
            "telnet_port" => FieldId::TelnetPort,
            "timeout" | "connection_timeout" => FieldId::ConnectionTimeout,
            "keepalive" | "keepalive_interval" => FieldId::KeepaliveInterval,
            "keepalive_max" => FieldId::KeepaliveMax,
            "idle" | "idle_timeout" | "idle_disconnect" => FieldId::IdleTimeout,
            "channel_buffer" => FieldId::ChannelBuffer,
            "proxy_host" => FieldId::ProxyHost,
            "proxy_port" => FieldId::ProxyPort,
            "scrollback" => FieldId::Scrollback,
            "max_packets" => FieldId::MaxPackets,
            "recv_window" => FieldId::RecvWindow,
            "gex_min" => FieldId::GexMin,
            "gex_preferred" => FieldId::GexPreferred,
            "gex_max" => FieldId::GexMax,
            "rekey" | "rekey_time" => FieldId::RekeyTime,
            "startup" | "startup_command" => FieldId::StartupCommand,
            "proxy_user" | "proxy_username" => FieldId::ProxyUsername,
            "proxy_pass" | "proxy_password" => FieldId::ProxyPassword,
            "tags" => FieldId::Tags,
            "notes" => FieldId::Notes,
            "word_separators" => FieldId::WordSeparators,
            _ => return None,
        })
    }

    /// 字段所属分组（用于把校验状态上卷到左侧导航 Summary）。
    pub fn section(self) -> SshSection {
        match self {
            FieldId::Name
            | FieldId::StartupCommand
            | FieldId::SerialPort
            | FieldId::TelnetHost
            | FieldId::TelnetPort => SshSection::Basic,
            FieldId::Host
            | FieldId::Port
            | FieldId::ConnectionTimeout
            | FieldId::ProxyHost
            | FieldId::ProxyPort
            | FieldId::ProxyUsername
            | FieldId::ProxyPassword => SshSection::Connection,
            FieldId::Username | FieldId::Password | FieldId::KeyPath | FieldId::Passphrase => {
                SshSection::Authentication
            }
            FieldId::Scrollback | FieldId::WordSeparators => SshSection::Terminal,
            FieldId::KeepaliveInterval
            | FieldId::KeepaliveMax
            | FieldId::IdleTimeout
            | FieldId::ChannelBuffer
            | FieldId::MaxPackets
            | FieldId::RecvWindow => SshSection::Network,
            FieldId::GexMin | FieldId::GexPreferred | FieldId::GexMax | FieldId::RekeyTime => {
                SshSection::Advanced
            }
            FieldId::Tags | FieldId::Notes => SshSection::Notes,
        }
    }

    /// 字段是否适用于指定的会话协议（白名单机制）。
    pub fn is_applicable(self, protocol: SessionProtocol) -> bool {
        match protocol {
            SessionProtocol::Ssh => !matches!(
                self,
                FieldId::SerialPort | FieldId::TelnetHost | FieldId::TelnetPort
            ),
            SessionProtocol::Serial => matches!(
                self,
                FieldId::Name
                    | FieldId::SerialPort
                    | FieldId::Scrollback
                    | FieldId::WordSeparators
                    | FieldId::StartupCommand
                    | FieldId::Tags
                    | FieldId::Notes
            ),
            SessionProtocol::Telnet => matches!(
                self,
                FieldId::Name
                    | FieldId::TelnetHost
                    | FieldId::TelnetPort
                    | FieldId::Scrollback
                    | FieldId::WordSeparators
                    | FieldId::StartupCommand
                    | FieldId::Tags
                    | FieldId::Notes
            ),
            SessionProtocol::Local => matches!(
                self,
                FieldId::Name
                    | FieldId::Scrollback
                    | FieldId::WordSeparators
                    | FieldId::StartupCommand
                    | FieldId::Tags
                    | FieldId::Notes
            ),
        }
    }
}

/// 校验严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationSeverity {
    Info,
    Warning,
    Error,
}

/// 单字段校验结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationResult {
    pub severity: ValidationSeverity,
    pub message: String,
}

impl ValidationResult {
    pub fn error(msg: impl Into<String>) -> Self {
        Self { severity: ValidationSeverity::Error, message: msg.into() }
    }
    pub fn warning(msg: impl Into<String>) -> Self {
        Self { severity: ValidationSeverity::Warning, message: msg.into() }
    }
    pub fn info(msg: impl Into<String>) -> Self {
        Self { severity: ValidationSeverity::Info, message: msg.into() }
    }
}

/// 定长校验状态：按 `FieldId` discriminant 索引，无字符串键。
#[derive(Clone, Debug)]
pub struct ValidationState {
    slots: Vec<Option<ValidationResult>>,
}

impl Default for ValidationState {
    fn default() -> Self {
        Self { slots: vec![None; FieldId::COUNT] }
    }
}

impl ValidationState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: FieldId) -> Option<&ValidationResult> {
        self.slots.get(id.index()).and_then(|s| s.as_ref())
    }

    pub fn set(&mut self, id: FieldId, result: Option<ValidationResult>) {
        if let Some(slot) = self.slots.get_mut(id.index()) {
            *slot = result;
        }
    }

    pub fn clear(&mut self) {
        for s in &mut self.slots {
            *s = None;
        }
    }

    /// 是否存在任意 Error（用于阻断保存）。
    pub fn has_error(&self) -> bool {
        self.slots.iter().flatten().any(|r| r.severity == ValidationSeverity::Error)
    }

    /// 某分组内最高严重度（None = 无问题）。
    pub fn section_severity(&self, section: SshSection) -> Option<ValidationSeverity> {
        let mut worst: Option<ValidationSeverity> = None;
        for (idx, slot) in self.slots.iter().enumerate() {
            if let Some(r) = slot {
                if FieldId::ALL[idx].section() == section {
                    worst = Some(match worst {
                        Some(w) => max_severity(w, r.severity),
                        None => r.severity,
                    });
                }
            }
        }
        worst
    }

    /// 首个存在 Error 的字段（用于提交时定位）。
    pub fn first_error_field(&self) -> Option<FieldId> {
        self.slots.iter().enumerate().find_map(|(idx, slot)| {
            slot.as_ref().and_then(|r| {
                (r.severity == ValidationSeverity::Error).then_some(FieldId::ALL[idx])
            })
        })
    }
}

fn max_severity(a: ValidationSeverity, b: ValidationSeverity) -> ValidationSeverity {
    use ValidationSeverity::*;
    match (a, b) {
        (Error, _) | (_, Error) => Error,
        (Warning, _) | (_, Warning) => Warning,
        _ => Info,
    }
}

use gpui::App;
use velowork_i18n::i18n;

/// 纯规则：解析后的值 → 校验结果。使用 `i18n!` 提供国际化消息。
pub fn validate_value(id: FieldId, raw: &str, cx: &App) -> Option<ValidationResult> {
    let v = raw.trim();
    match id {
        FieldId::Name => {
            if v.is_empty() {
                Some(ValidationResult::error(i18n!(cx, "ssh.validation.name_required")))
            } else {
                None
            }
        }
        FieldId::SerialPort => {
            if v.is_empty() {
                Some(ValidationResult::error(i18n!(cx, "ssh.validation.serial_port_required")))
            } else {
                None
            }
        }
        FieldId::TelnetHost => {
            if v.is_empty() {
                Some(ValidationResult::error(i18n!(cx, "ssh.validation.telnet_host_required")))
            } else {
                None
            }
        }
        FieldId::TelnetPort => {
            if v.is_empty() {
                return None;
            }
            match v.parse::<u32>() {
                Ok(0) | Ok(65536..=u32::MAX) => Some(ValidationResult::error(i18n!(cx, "ssh.validation.port_range"))),
                Ok(_) => None,
                Err(_) => Some(ValidationResult::error(i18n!(cx, "ssh.validation.port_number"))),
            }
        }
        FieldId::Host => {
            if v.is_empty() {
                Some(ValidationResult::error(i18n!(cx, "ssh.validation.host_required")))
            } else {
                None
            }
        }
        FieldId::Username => {
            if v.is_empty() {
                Some(ValidationResult::error(i18n!(cx, "ssh.validation.username_required")))
            } else {
                None
            }
        }
        FieldId::Port | FieldId::ProxyPort => {
            if v.is_empty() {
                return None;
            }
            match v.parse::<u32>() {
                Ok(0) => Some(ValidationResult::error(i18n!(cx, "ssh.validation.port_range"))),
                Ok(p) if p > 65535 => Some(ValidationResult::error(i18n!(cx, "ssh.validation.port_range"))),
                Ok(65535) => Some(ValidationResult::info("Port 65535 is unusual")),
                Ok(_) => None,
                Err(_) => Some(ValidationResult::error(i18n!(cx, "ssh.validation.port_number"))),
            }
        }
        FieldId::GexMin => match v.parse::<u32>() {
            Ok(n) if n < 2048 => Some(ValidationResult::error(i18n!(cx, "ssh.validation.gex_min_range"))),
            Ok(_) => None,
            Err(_) if v.is_empty() => None,
            Err(_) => Some(ValidationResult::error(i18n!(cx, "ssh.validation.number_required"))),
        },
        FieldId::ConnectionTimeout
        | FieldId::KeepaliveInterval
        | FieldId::KeepaliveMax
        | FieldId::IdleTimeout
        | FieldId::ChannelBuffer
        | FieldId::Scrollback
        | FieldId::MaxPackets
        | FieldId::RecvWindow
        | FieldId::GexPreferred
        | FieldId::GexMax
        | FieldId::RekeyTime => {
            if v.is_empty() {
                None
            } else if v.parse::<u32>().is_err() {
                Some(ValidationResult::error(i18n!(cx, "ssh.validation.number_required")))
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use velowork_state::SessionProtocol;

    #[test]
    fn test_field_applicability_whitelist() {
        // SSH
        assert!(FieldId::Name.is_applicable(SessionProtocol::Ssh));
        assert!(FieldId::Host.is_applicable(SessionProtocol::Ssh));
        assert!(FieldId::Port.is_applicable(SessionProtocol::Ssh));
        assert!(FieldId::Username.is_applicable(SessionProtocol::Ssh));
        assert!(!FieldId::SerialPort.is_applicable(SessionProtocol::Ssh));
        assert!(!FieldId::TelnetHost.is_applicable(SessionProtocol::Ssh));
        assert!(!FieldId::TelnetPort.is_applicable(SessionProtocol::Ssh));

        // Serial
        assert!(FieldId::Name.is_applicable(SessionProtocol::Serial));
        assert!(FieldId::SerialPort.is_applicable(SessionProtocol::Serial));
        assert!(!FieldId::Host.is_applicable(SessionProtocol::Serial));
        assert!(!FieldId::Port.is_applicable(SessionProtocol::Serial));
        assert!(!FieldId::Username.is_applicable(SessionProtocol::Serial));
        assert!(!FieldId::TelnetHost.is_applicable(SessionProtocol::Serial));

        // Telnet
        assert!(FieldId::Name.is_applicable(SessionProtocol::Telnet));
        assert!(FieldId::TelnetHost.is_applicable(SessionProtocol::Telnet));
        assert!(FieldId::TelnetPort.is_applicable(SessionProtocol::Telnet));
        assert!(!FieldId::Host.is_applicable(SessionProtocol::Telnet));
        assert!(!FieldId::Port.is_applicable(SessionProtocol::Telnet));
        assert!(!FieldId::Username.is_applicable(SessionProtocol::Telnet));
        assert!(!FieldId::SerialPort.is_applicable(SessionProtocol::Telnet));

        // Local
        assert!(FieldId::Name.is_applicable(SessionProtocol::Local));
        assert!(!FieldId::Host.is_applicable(SessionProtocol::Local));
        assert!(!FieldId::Port.is_applicable(SessionProtocol::Local));
        assert!(!FieldId::Username.is_applicable(SessionProtocol::Local));
        assert!(!FieldId::SerialPort.is_applicable(SessionProtocol::Local));
        assert!(!FieldId::TelnetHost.is_applicable(SessionProtocol::Local));
    }
}
