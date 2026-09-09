//! `SessionDialogInputs`：聚合全部 `Entity<InputState>`。
//!
//! 把「输入实体」这一噪声从 [`super::model::SessionDialogModel`] 顶层剥离，是
//! 防止 Model 膨胀的关键收纳层。输入的 `InputEvent` 订阅在 `SessionPanel`
//! 集成阶段统一挂载（需要 `Context<SessionPanel>`），此处仅负责创建。

use gpui::{AppContext, Context, Entity, Window};
use velowork_i18n::i18n;
use velowork_ui::input::InputState;

/// 对话框内的全部文本输入实体。
#[derive(Clone)]
pub struct SessionDialogInputs {
    // Basic
    pub name: Entity<InputState>,
    pub startup_command: Entity<InputState>,
    // Serial
    pub serial_port: Entity<InputState>,
    // Telnet
    pub telnet_host: Entity<InputState>,
    pub telnet_port: Entity<InputState>,
    // Connection
    pub host: Entity<InputState>,
    pub port: Entity<InputState>,
    pub connection_timeout: Entity<InputState>,
    pub keepalive_interval: Entity<InputState>,
    pub keepalive_max: Entity<InputState>,
    pub idle_disconnect: Entity<InputState>,
    pub proxy_host: Entity<InputState>,
    pub proxy_port: Entity<InputState>,
    pub proxy_username: Entity<InputState>,
    pub proxy_password: Entity<InputState>,
    // Authentication
    pub username: Entity<InputState>,
    pub password: Entity<InputState>,
    pub key_path: Entity<InputState>,
    pub passphrase: Entity<InputState>,
    pub agent_socket_path: Entity<InputState>,
    pub totp_secret: Entity<InputState>,
    // Local
    pub local_cwd: Entity<InputState>,
    // Terminal
    pub font_size: Entity<InputState>,
    pub scrollback: Entity<InputState>,
    pub word_separators: Entity<InputState>,
    pub bell_cooldown_ms: Entity<InputState>,
    // Advanced
    pub rekey_time: Entity<InputState>,
    pub gex_min: Entity<InputState>,
    pub gex_preferred: Entity<InputState>,
    pub gex_max: Entity<InputState>,
    pub max_packets: Entity<InputState>,
    pub recv_window: Entity<InputState>,
    // X11 Forwarding
    pub x11_display: Entity<InputState>,
    // Notes
    pub tags: Entity<InputState>,
    pub notes: Entity<InputState>,
    // 导航搜索
    pub nav_search: Entity<InputState>,
}

impl SessionDialogInputs {
    /// 创建全部输入实体。`T` 为宿主实体类型（通常是 `SessionPanel`）。
    pub fn new<T: 'static>(_window: &mut Window, cx: &mut Context<T>) -> Self {
        let name = cx.new(|cx| InputState::new(cx).placeholder(i18n!(cx, "ssh.general.name_placeholder")));
        let startup_command = cx.new(|cx| {
            InputState::new(cx)
                .placeholder(i18n!(cx, "ssh.general.startup_command_placeholder"))
        });
        let serial_port = cx.new(|cx| {
            InputState::new(cx)
                .placeholder(i18n!(cx, "ssh.serial.port_placeholder"))
        });
        let telnet_host = cx.new(|cx| {
            InputState::new(cx)
                .placeholder(i18n!(cx, "ssh.telnet.host_placeholder"))
        });
        let telnet_port = cx.new(|cx| {
            InputState::new(cx)
                .default_value("23")
                .placeholder("23")
                .digits_only(true)
                .max_length(5)
                .max_number(65535)
        });
        let host =
            cx.new(|cx| InputState::new(cx).placeholder(i18n!(cx, "ssh.connection.host_placeholder")));
        let port = cx.new(|cx| {
            InputState::new(cx)
                .default_value("22")
                .placeholder("22")
                .digits_only(true)
                .max_length(5)
                .max_number(65535)
        });
        let connection_timeout = cx.new(|cx| {
            InputState::new(cx)
                .default_value("30")
                .placeholder("30")
                .digits_only(true)
                .max_length(5)
        });
        let keepalive_interval = cx.new(|cx| {
            InputState::new(cx)
                .default_value("30")
                .placeholder("30")
                .digits_only(true)
                .max_length(5)
        });
        let keepalive_max = cx.new(|cx| {
            InputState::new(cx)
                .default_value("3")
                .placeholder("3")
                .digits_only(true)
                .max_length(5)
        });
        let idle_disconnect = cx.new(|cx| {
            InputState::new(cx)
                .default_value("0")
                .placeholder("0")
                .digits_only(true)
                .max_length(5)
        });
        let proxy_host = cx.new(|cx| {
            InputState::new(cx).placeholder(i18n!(cx, "ssh.connection.host_placeholder"))
        });
        let proxy_port = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("1080")
                .digits_only(true)
                .max_length(5)
                .max_number(65535)
        });
        let proxy_username = cx.new(|cx| {
            InputState::new(cx).placeholder(i18n!(cx, "ssh.auth.username_placeholder"))
        });
        let proxy_password = cx.new(|cx| {
            InputState::new(cx)
                .password()
                .placeholder(i18n!(cx, "ssh.auth.password_placeholder"))
        });
        let username = cx.new(|cx| {
            InputState::new(cx).placeholder(i18n!(cx, "ssh.auth.username_placeholder"))
        });
        let password = cx.new(|cx| {
            InputState::new(cx)
                .password()
                .placeholder(i18n!(cx, "ssh.auth.password_placeholder"))
        });
        let key_path = cx.new(|cx| {
            InputState::new(cx).placeholder(i18n!(cx, "ssh.auth.private_key_placeholder"))
        });
        let passphrase = cx.new(|cx| {
            InputState::new(cx)
                .password()
                .placeholder(i18n!(cx, "ssh.auth.passphrase_placeholder"))
        });
        let agent_socket_path = cx.new(|cx| {
            InputState::new(cx).placeholder(i18n!(cx, "session_dialog.agent_socket_placeholder"))
        });
        let totp_secret = cx.new(|cx| {
            InputState::new(cx)
                .password()
                .placeholder(i18n!(cx, "ssh.auth.totp_placeholder"))
        });
        let local_cwd = cx.new(|cx| {
            InputState::new(cx).placeholder(i18n!(cx, "session_dialog.local_cwd_placeholder"))
        });
        let font_size = cx.new(|cx| {
            InputState::new(cx)
                .placeholder(i18n!(cx, "session_dialog.font_size_placeholder"))
                .max_length(5)
        });
        let scrollback = cx.new(|cx| {
            InputState::new(cx)
                .placeholder(i18n!(cx, "session_dialog.scrollback_placeholder"))
                .digits_only(true)
                .max_length(7)
        });
        let word_separators = cx.new(|cx| {
            InputState::new(cx)
                .placeholder(i18n!(cx, "ssh.terminal.word_separators_placeholder"))
        });
        let bell_cooldown_ms = cx.new(|cx| {
            InputState::new(cx)
                .placeholder(i18n!(cx, "ssh.terminal.bell_cooldown_placeholder"))
                .digits_only(true)
                .max_length(5)
        });
        let rekey_time = cx.new(|cx| {
            InputState::new(cx)
                .default_value("3600")
                .placeholder(i18n!(cx, "ssh.advanced.rekey_time_placeholder"))
                .digits_only(true)
                .max_length(6)
        });
        let gex_min = cx.new(|cx| {
            InputState::new(cx)
                .default_value("2048")
                .placeholder(i18n!(cx, "ssh.advanced.gex_min_placeholder"))
                .digits_only(true)
                .max_length(5)
        });
        let gex_preferred = cx.new(|cx| {
            InputState::new(cx)
                .default_value("4096")
                .placeholder(i18n!(cx, "ssh.advanced.gex_preferred_placeholder"))
                .digits_only(true)
                .max_length(5)
        });
        let gex_max = cx.new(|cx| {
            InputState::new(cx)
                .default_value("8192")
                .placeholder(i18n!(cx, "ssh.advanced.gex_max_placeholder"))
                .digits_only(true)
                .max_length(5)
        });
        let max_packets = cx.new(|cx| {
            InputState::new(cx)
                .default_value("32768")
                .placeholder(i18n!(cx, "ssh.advanced.max_packets_placeholder"))
                .digits_only(true)
                .max_length(10)
        });
        let recv_window = cx.new(|cx| {
            InputState::new(cx)
                .default_value("2097152")
                .placeholder(i18n!(cx, "ssh.advanced.recv_window_placeholder"))
                .digits_only(true)
                .max_length(10)
        });
        let x11_display = cx.new(|cx| {
            InputState::new(cx).placeholder(i18n!(cx, "ssh.advanced.x11_display_placeholder"))
        });
        let tags = cx.new(|cx| InputState::new(cx).placeholder(i18n!(cx, "ssh.notes.tags_placeholder")));
        let notes = cx.new(|cx| {
            InputState::new(cx).placeholder(i18n!(cx, "ssh.notes.notes_placeholder"))
        });
        let nav_search = cx.new(|cx| {
            InputState::new(cx).placeholder(i18n!(cx, "session_dialog.search_placeholder"))
        });

        Self {
            name,
            startup_command,
            serial_port,
            telnet_host,
            telnet_port,
            host,
            port,
            connection_timeout,
            keepalive_interval,
            keepalive_max,
            idle_disconnect,
            proxy_host,
            proxy_port,
            proxy_username,
            proxy_password,
            username,
            password,
            key_path,
            passphrase,
            agent_socket_path,
            totp_secret,
            local_cwd,
            font_size,
            scrollback,
            word_separators,
            bell_cooldown_ms,
            rekey_time,
            gex_min,
            gex_preferred,
            gex_max,
            max_packets,
            recv_window,
            x11_display,
            tags,
            notes,
            nav_search,
        }
    }

    /// 所有输入实体（用于统一订阅 `InputEvent`）。
    pub fn all(&self) -> Vec<Entity<InputState>> {
        vec![
            self.name.clone(),
            self.startup_command.clone(),
            self.serial_port.clone(),
            self.telnet_host.clone(),
            self.telnet_port.clone(),
            self.host.clone(),
            self.port.clone(),
            self.connection_timeout.clone(),
            self.keepalive_interval.clone(),
            self.keepalive_max.clone(),
            self.idle_disconnect.clone(),
            self.proxy_host.clone(),
            self.proxy_port.clone(),
            self.proxy_username.clone(),
            self.proxy_password.clone(),
            self.username.clone(),
            self.password.clone(),
            self.key_path.clone(),
            self.passphrase.clone(),
            self.agent_socket_path.clone(),
            self.totp_secret.clone(),
            self.local_cwd.clone(),
            self.font_size.clone(),
            self.scrollback.clone(),
            self.word_separators.clone(),
            self.bell_cooldown_ms.clone(),
            self.rekey_time.clone(),
            self.gex_min.clone(),
            self.gex_preferred.clone(),
            self.gex_max.clone(),
            self.max_packets.clone(),
            self.recv_window.clone(),
            self.x11_display.clone(),
            self.tags.clone(),
            self.notes.clone(),
            self.nav_search.clone(),
        ]
    }
}
