//! Dialog Dirty / ChangeSet：由 `config` 与 `baseline` 对比派生。
//! 事件驱动缓存进 `ui.change_set`，render 只读缓存。

use std::collections::HashSet;

use velowork_state::SshSession;

use super::section::SshSection;
use super::validation::FieldId;

#[derive(Clone, Debug, Default)]
pub struct ChangeSet {
    pub changed_fields: HashSet<FieldId>,
    pub changed_sections: HashSet<SshSection>,
    pub change_count: usize,
}

impl ChangeSet {
    pub fn is_dirty(&self) -> bool {
        // 直接用整体 PartialEq 判定；change_count 只统计可映射到 FieldId 的字段。
        self.change_count > 0 || !self.changed_sections.is_empty()
    }

    pub fn section_changed(&self, section: SshSection) -> bool {
        self.changed_sections.contains(&section)
    }
}

/// 逐字段对比，派生 ChangeSet。整体 dirty 以 `config != baseline` 为准，
/// 但 change_count / 分组标记基于可识别字段的差异集合。
pub fn recompute_change_set(config: &SshSession, baseline: &SshSession) -> ChangeSet {
    let mut cs = ChangeSet::default();

    let mut mark = |field: FieldId| {
        cs.changed_fields.insert(field);
        cs.changed_sections.insert(field.section());
    };

    if config.name != baseline.name {
        mark(FieldId::Name);
    }
    if config.host != baseline.host {
        mark(FieldId::Host);
    }
    if config.port != baseline.port {
        mark(FieldId::Port);
    }
    if config.username != baseline.username {
        mark(FieldId::Username);
    }
    if config.auth_type != baseline.auth_type || config.totp_secret != baseline.totp_secret {
        mark(FieldId::Password);
    }
    if config.connection_timeout != baseline.connection_timeout {
        mark(FieldId::ConnectionTimeout);
    }
    if config.keep_alive_interval != baseline.keep_alive_interval {
        mark(FieldId::KeepaliveInterval);
    }
    if config.keep_alive_max != baseline.keep_alive_max {
        mark(FieldId::KeepaliveMax);
    }
    if config.idle_disconnect_timeout != baseline.idle_disconnect_timeout {
        mark(FieldId::IdleTimeout);
    }
    if config.channel_buffer_size != baseline.channel_buffer_size {
        mark(FieldId::ChannelBuffer);
    }
    if config.proxy_host != baseline.proxy_host {
        mark(FieldId::ProxyHost);
    }
    if config.proxy_port != baseline.proxy_port {
        mark(FieldId::ProxyPort);
    }
    if config.terminal.scrollback_lines != baseline.terminal.scrollback_lines {
        mark(FieldId::Scrollback);
    }
    if config.max_packets != baseline.max_packets {
        mark(FieldId::MaxPackets);
    }
    if config.recv_window != baseline.recv_window {
        mark(FieldId::RecvWindow);
    }
    if config.gex_min != baseline.gex_min {
        mark(FieldId::GexMin);
    }
    if config.gex_preferred != baseline.gex_preferred {
        mark(FieldId::GexPreferred);
    }
    if config.gex_max != baseline.gex_max {
        mark(FieldId::GexMax);
    }
    if config.rekey_time != baseline.rekey_time {
        mark(FieldId::RekeyTime);
    }

    if config.terminal.word_separators != baseline.terminal.word_separators {
        mark(FieldId::WordSeparators);
    }
    if config.serial_port != baseline.serial_port {
        mark(FieldId::SerialPort);
    }
    if config.telnet_host != baseline.telnet_host {
        mark(FieldId::TelnetHost);
    }
    if config.telnet_port != baseline.telnet_port {
        mark(FieldId::TelnetPort);
    }

    // 其它不映射到 FieldId 的字段：仅标记分组
    if config.icon_color != baseline.icon_color
        || config.parent_folder_id != baseline.parent_folder_id
        || config.startup_command != baseline.startup_command
        || config.local_shell != baseline.local_shell
        || config.local_cwd != baseline.local_cwd
        || config.local_env != baseline.local_env
        || config.serial_baud_rate != baseline.serial_baud_rate
        || config.serial_data_bits != baseline.serial_data_bits
        || config.serial_stop_bits != baseline.serial_stop_bits
        || config.serial_parity != baseline.serial_parity
        || config.serial_flow_control != baseline.serial_flow_control
        || config.serial_dtr != baseline.serial_dtr
        || config.serial_rts != baseline.serial_rts
        || config.serial_display_mode != baseline.serial_display_mode
        || config.serial_line_ending != baseline.serial_line_ending
        || config.serial_local_echo != baseline.serial_local_echo
        || config.serial_timestamps != baseline.serial_timestamps
        || config.serial_auto_reconnect != baseline.serial_auto_reconnect
        || config.telnet_encoding != baseline.telnet_encoding
    {
        cs.changed_sections.insert(SshSection::Basic);
    }
    if config.keepalive_strategy != baseline.keepalive_strategy
        || config.tcp_nodelay != baseline.tcp_nodelay
        || config.enable_x11_forwarding != baseline.enable_x11_forwarding
        || config.x11_display != baseline.x11_display
        || config.proxy_type != baseline.proxy_type
        || config.proxy_username != baseline.proxy_username
        || config.proxy_password != baseline.proxy_password
    {
        cs.changed_sections.insert(SshSection::Connection);
    }
    if config.terminal != baseline.terminal {
        cs.changed_sections.insert(SshSection::Terminal);
    }
    if config.compression != baseline.compression
        || config.strict_host_key != baseline.strict_host_key
        || config.algorithms_automatic != baseline.algorithms_automatic
        || config.kex_algorithms != baseline.kex_algorithms
        || config.cipher_algorithms != baseline.cipher_algorithms
        || config.mac_algorithms != baseline.mac_algorithms
        || config.hostkey_algorithms != baseline.hostkey_algorithms
    {
        cs.changed_sections.insert(SshSection::Security);
    }
    if config.tags != baseline.tags
        || config.notes != baseline.notes
    {
        cs.changed_sections.insert(SshSection::Notes);
    }

    // 根据当前协议过滤仅相关的字段与分组
    cs.changed_fields.retain(|f| f.is_applicable(config.protocol));
    let visible = super::section::visible_sections(config.protocol);
    cs.changed_sections.retain(|s| visible.contains(s));

    cs.change_count = cs.changed_fields.len();
    cs
}
