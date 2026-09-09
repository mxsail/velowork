use gpui::prelude::*;
use gpui::*;
use velowork_state::SshSession;
use velowork_ui::icon::AppIcon;
use velowork_ui::input::InputState;
use velowork_ui::theme::{surface_bg_t, theme};
use velowork_ui::tokens::{
    ui_space_lg, ui_space_md, ui_space_xs, ui_text_md, RADIUS_STD,
};

// ── Session tree drag-and-drop types ────────────────────────────────────

/// Drag payload for a session node in the session tree.
#[derive(Clone)]
pub struct SessionTreeSessionDrag {
    pub session_id: String,
    pub session_name: String,
}

/// Drag preview for a session node.
pub struct SessionTreeSessionDragView {
    pub name: String,
    pub cursor_offset_x: Pixels,
    pub cursor_offset_y: Pixels,
}

impl Render for SessionTreeSessionDragView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        div().child(
            div()
                .absolute()
                .top(self.cursor_offset_y + ui_space_xs(cx))
                .left(self.cursor_offset_x)
                .px(ui_space_md(cx))
                .py(ui_space_xs(cx))
                .bg(surface_bg_t(t.bg_panel, &t))
                .border_1()
                .border_color(rgb(t.border))
                .rounded(RADIUS_STD)
                .shadow_lg()
                .text_size(ui_text_md(cx))
                .text_color(rgb(t.text_primary))
                .flex()
                .items_center()
                .gap(ui_space_xs(cx))
                .child(
                    AppIcon::Terminal
                        .size(ui_space_lg(cx))
                        .text_color(rgb(t.text_muted)),
                )
                .child(self.name.clone()),
        )
    }
}

/// Drag payload for a folder node in the session tree.
#[derive(Clone)]
pub struct SessionTreeFolderDrag {
    pub folder_id: String,
    pub folder_name: String,
}

/// Drag preview for a folder node.
pub struct SessionTreeFolderDragView {
    pub name: String,
    pub cursor_offset_x: Pixels,
    pub cursor_offset_y: Pixels,
}

impl Render for SessionTreeFolderDragView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        div().child(
            div()
                .absolute()
                .top(self.cursor_offset_y + ui_space_xs(cx))
                .left(self.cursor_offset_x)
                .px(ui_space_md(cx))
                .py(ui_space_xs(cx))
                .bg(surface_bg_t(t.bg_panel, &t))
                .border_1()
                .border_color(rgb(t.border))
                .rounded(RADIUS_STD)
                .shadow_lg()
                .text_size(ui_text_md(cx))
                .text_color(rgb(t.text_primary))
                .flex()
                .items_center()
                .gap(ui_space_xs(cx))
                .child(AppIcon::Folder.size(ui_space_lg(cx)).text_color(rgb(t.text_muted)))
                .child(self.name.clone()),
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SshDialogTab {
    General,
    Authentication,
    Connection,
    Terminal,
    Advanced,
    Kex,
    Cipher,
    Mac,
    Hostkey,
}

impl SshDialogTab {
    pub fn all() -> &'static [SshDialogTab] {
        &[
            SshDialogTab::General,
            SshDialogTab::Authentication,
            SshDialogTab::Connection,
            SshDialogTab::Terminal,
            SshDialogTab::Advanced,
            SshDialogTab::Kex,
            SshDialogTab::Cipher,
            SshDialogTab::Mac,
            SshDialogTab::Hostkey,
        ]
    }

    pub fn label_key(&self) -> &'static str {
        match self {
            SshDialogTab::General => "ssh.tab.general",
            SshDialogTab::Authentication => "ssh.tab.authentication",
            SshDialogTab::Connection => "ssh.tab.connection",
            SshDialogTab::Terminal => "ssh.tab.terminal",
            SshDialogTab::Advanced => "ssh.tab.advanced",
            SshDialogTab::Kex => "ssh.tab.russh_kex",
            SshDialogTab::Cipher => "ssh.tab.russh_cipher",
            SshDialogTab::Mac => "ssh.tab.russh_mac",
            SshDialogTab::Hostkey => "ssh.tab.russh_hostkey",
        }
    }

    pub fn icon(&self) -> AppIcon {
        match self {
            SshDialogTab::General => AppIcon::Settings,
            SshDialogTab::Authentication => AppIcon::Link,
            SshDialogTab::Connection => AppIcon::Link,
            SshDialogTab::Terminal => AppIcon::Terminal,
            SshDialogTab::Advanced => AppIcon::Settings,
            SshDialogTab::Kex => AppIcon::Transfer,
            SshDialogTab::Cipher => AppIcon::EyeOff,
            SshDialogTab::Mac => AppIcon::Check,
            SshDialogTab::Hostkey => AppIcon::Folder,
        }
    }

    pub fn is_sub_tab(&self) -> bool {
        matches!(
            self,
            SshDialogTab::Kex | SshDialogTab::Cipher | SshDialogTab::Mac | SshDialogTab::Hostkey
        )
    }
}

#[derive(Clone)]
pub enum TestConnectionStatus {
    Idle,
    Testing,
    Success { latency_ms: u64 },
    Failed { error: String },
}

#[derive(Clone, Debug)]
pub enum SessionPanelDialog {
    AddFolder { parent_id: Option<String> },
    AddSession {
        parent_id: Option<String>,
        protocol: Option<velowork_state::SessionProtocol>,
    },
    EditFolder { id: String, name: String },
    EditSession { session: SshSession },
}

pub enum SessionPanelEvent {
    SpawnTerminals { project_id: String },
    DialogChanged,
}

#[derive(Clone, Debug)]
pub struct SessionTreeContextMenu {
    pub position: Point<Pixels>,
    pub node_id: String,
    pub label: String,
    pub is_folder: bool,
    /// True when the menu was opened on empty tree space (not on a node).
    pub is_blank: bool,
}

/// Tracks which text area is being resized and the starting drag position.
#[derive(Clone)]
pub(crate) enum ResizeTarget {
    StartupCommand,
    Notes,
}

pub(crate) struct ResizeDragState {
    pub target: ResizeTarget,
    pub start_height: Pixels,
    pub anchor_y: Option<Pixels>,
}

#[derive(Clone)]
pub(crate) enum InlineFolderState {
    Creating {
        parent_id: Option<String>,
        input: Option<Entity<InputState>>,
    },
    Renaming {
        id: String,
        original_name: String,
        input: Option<Entity<InputState>>,
    },
}

/// Inline rename state for a session node.
#[derive(Clone)]
pub(crate) struct InlineSessionRename {
    pub id: String,
    pub original_name: String,
    pub input: Option<Entity<InputState>>,
}

/// 获取指定协议对应的图标
pub fn session_protocol_icon(protocol: velowork_state::SessionProtocol) -> AppIcon {
    match protocol {
        velowork_state::SessionProtocol::Ssh => AppIcon::Server,
        velowork_state::SessionProtocol::Serial => AppIcon::Serial,
        velowork_state::SessionProtocol::Telnet => AppIcon::Telnet,
        velowork_state::SessionProtocol::Local => AppIcon::Terminal,
    }
}

/// 获取指定会话对应的图标
pub fn session_icon(session: &SshSession) -> AppIcon {
    session_protocol_icon(session.protocol)
}

/// 获取会话在树列表中右侧弱化显示的简要配置信息
pub fn session_subtext(session: &SshSession) -> String {
    match session.protocol {
        velowork_state::SessionProtocol::Ssh => {
            let host = session.host.trim();
            if host.is_empty() {
                return String::new();
            }
            let user = session.username.trim();
            let has_user = !user.is_empty();
            let is_custom_port = session.port != 22 && session.port != 0;

            match (has_user, is_custom_port) {
                (true, true) => format!("{}@{}:{}", user, host, session.port),
                (true, false) => format!("{}@{}", user, host),
                (false, true) => format!("{}:{}", host, session.port),
                (false, false) => host.to_string(),
            }
        }
        velowork_state::SessionProtocol::Serial => {
            let port = session.serial_port.as_deref().unwrap_or("").trim();
            if port.is_empty() {
                return "Serial".to_string();
            }
            format!("{} {}", port, session.serial_baud_rate)
        }
        velowork_state::SessionProtocol::Telnet => {
            let host = session.telnet_host.as_deref().unwrap_or("").trim();
            if host.is_empty() {
                return "Telnet".to_string();
            }
            if session.telnet_port != 23 && session.telnet_port != 0 {
                format!("{}:{}", host, session.telnet_port)
            } else {
                host.to_string()
            }
        }
        velowork_state::SessionProtocol::Local => {
            if let Some(shell_str) = session.local_shell.as_deref() {
                let trimmed = shell_str.trim();
                if !trimmed.is_empty() {
                    if let Ok(st) = serde_json::from_str::<velowork_core::shell::ShellType>(trimmed) {
                        return st.local_shell_name();
                    }
                    let first_token = trimmed.split_whitespace().next().unwrap_or(trimmed);
                    let base_name = std::path::Path::new(first_token)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(first_token);
                    return base_name.to_string();
                }
            }
            "Local".to_string()
        }
    }
}

/// 获取会话在树列表中鼠标悬停展示的结构化多行 Tooltip
pub fn session_tooltip(session: &SshSession, cx: &App) -> String {
    use velowork_i18n::i18n;

    let mut lines = Vec::new();

    // 1. 协议与名称
    let protocol_label = match session.protocol {
        velowork_state::SessionProtocol::Ssh => "SSH".to_string(),
        velowork_state::SessionProtocol::Serial => i18n!(cx, "ssh.protocol.serial"),
        velowork_state::SessionProtocol::Telnet => "Telnet".to_string(),
        velowork_state::SessionProtocol::Local => i18n!(cx, "ssh.protocol.local"),
    };
    lines.push(format!("[{}] {}", protocol_label, session.name));

    // 2. 核心连接参数
    match session.protocol {
        velowork_state::SessionProtocol::Ssh => {
            let user = session.username.trim();
            let host = session.host.trim();
            let target = if host.is_empty() {
                "-".to_string()
            } else if user.is_empty() {
                format!("{}:{}", host, session.port)
            } else {
                format!("{}@{}:{}", user, host, session.port)
            };
            lines.push(format!("{}: {}", i18n!(cx, "session.tooltip.target"), target));

            // 认证方式
            let auth_desc = match &session.auth_type {
                velowork_state::SshAuthType::Password { .. } => {
                    i18n!(cx, "session.tooltip.password_auth")
                }
                velowork_state::SshAuthType::PrivateKey { key_path, .. } => {
                    if key_path.trim().is_empty() {
                        i18n!(cx, "session.tooltip.key_auth")
                    } else {
                        format!("{} ({})", i18n!(cx, "session.tooltip.key_auth"), key_path.trim())
                    }
                }
                velowork_state::SshAuthType::SshAgent { socket_path } => {
                    if let Some(sock) = socket_path {
                        format!("{} ({})", i18n!(cx, "session.tooltip.agent_auth"), sock.trim())
                    } else {
                        i18n!(cx, "session.tooltip.agent_auth")
                    }
                }
                velowork_state::SshAuthType::KeyboardInteractive => {
                    i18n!(cx, "session.tooltip.kb_auth")
                }
            };
            lines.push(format!("{}: {}", i18n!(cx, "session.tooltip.auth"), auth_desc));
        }
        velowork_state::SessionProtocol::Serial => {
            let port = session.serial_port.as_deref().unwrap_or("").trim();
            lines.push(format!(
                "{}: {}",
                i18n!(cx, "session.tooltip.serial_port"),
                if port.is_empty() { "-" } else { port }
            ));
            lines.push(format!(
                "{}: {} baud, {} data bits, {} stop bits, {} parity",
                i18n!(cx, "session.tooltip.serial_params"),
                session.serial_baud_rate,
                session.serial_data_bits,
                session.serial_stop_bits,
                session.serial_parity
            ));
            lines.push(format!(
                "{}: {}",
                i18n!(cx, "session.tooltip.flow_control"),
                session.serial_flow_control
            ));
        }
        velowork_state::SessionProtocol::Telnet => {
            let host = session.telnet_host.as_deref().unwrap_or("").trim();
            let target = if host.is_empty() {
                "-".to_string()
            } else {
                format!("{}:{}", host, session.telnet_port)
            };
            lines.push(format!("{}: {}", i18n!(cx, "session.tooltip.target"), target));
            let charset = session.terminal.charset.as_deref().unwrap_or("UTF-8");
            if !charset.is_empty() && !charset.eq_ignore_ascii_case("UTF-8") {
                lines.push(format!(
                    "{}: {}",
                    i18n!(cx, "ssh.terminal.charset"),
                    charset
                ));
            }
        }
        velowork_state::SessionProtocol::Local => {
            let shell_display = if let Some(shell_str) = session.local_shell.as_deref() {
                let trimmed = shell_str.trim();
                if trimmed.is_empty() {
                    i18n!(cx, "session.tooltip.system_default")
                } else if let Ok(st) = serde_json::from_str::<velowork_core::shell::ShellType>(trimmed) {
                    st.display_name()
                } else {
                    let first_token = trimmed.split_whitespace().next().unwrap_or(trimmed);
                    let base_name = std::path::Path::new(first_token)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(first_token);
                    base_name.to_string()
                }
            } else {
                i18n!(cx, "session.tooltip.system_default")
            };
            lines.push(format!(
                "{}: {}",
                i18n!(cx, "session.tooltip.shell"),
                shell_display
            ));
        }
    }

    // 3. 启动命令（若有）
    if let Some(cmd) = session.startup_command.as_deref() {
        let trimmed = cmd.trim();
        if !trimmed.is_empty() {
            lines.push(format!("{}: {}", i18n!(cx, "session.tooltip.startup_cmd"), trimmed));
        }
    }

    // 4. 标签（若有）
    if let Some(tags) = session.tags.as_deref() {
        let trimmed = tags.trim();
        if !trimmed.is_empty() {
            lines.push(format!("{}: {}", i18n!(cx, "session.tooltip.tags"), trimmed));
        }
    }

    // 5. 备注说明（若有）
    if let Some(notes) = session.notes.as_deref() {
        let trimmed = notes.trim();
        if !trimmed.is_empty() {
            lines.push(format!("{}: {}", i18n!(cx, "session.tooltip.notes"), trimmed));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{session_subtext, session_tooltip};
    use gpui::TestAppContext;
    use velowork_i18n::{init_locale, Locale};
    use velowork_state::{SessionProtocol, SshAuthType, SshSession};

    #[gpui::test]
    fn test_session_subtext(_cx: &mut TestAppContext) {
        // SSH default port
        let mut ssh = SshSession::default();
        ssh.protocol = SessionProtocol::Ssh;
        ssh.username = "root".to_string();
        ssh.host = "192.168.1.100".to_string();
        ssh.port = 22;
        assert_eq!(session_subtext(&ssh), "root@192.168.1.100");

        // SSH custom port
        ssh.port = 2222;
        assert_eq!(session_subtext(&ssh), "root@192.168.1.100:2222");

        // SSH empty user
        ssh.username = "".to_string();
        ssh.port = 22;
        assert_eq!(session_subtext(&ssh), "192.168.1.100");

        // Serial
        let mut serial = SshSession::default();
        serial.protocol = SessionProtocol::Serial;
        serial.serial_port = Some("COM3".to_string());
        serial.serial_baud_rate = 115200;
        assert_eq!(session_subtext(&serial), "COM3 115200");

        // Telnet
        let mut telnet = SshSession::default();
        telnet.protocol = SessionProtocol::Telnet;
        telnet.telnet_host = Some("switch.local".to_string());
        telnet.telnet_port = 23;
        assert_eq!(session_subtext(&telnet), "switch.local");
        telnet.telnet_port = 2323;
        assert_eq!(session_subtext(&telnet), "switch.local:2323");

        // Local without args
        let mut local = SshSession::default();
        local.protocol = SessionProtocol::Local;
        local.local_shell = Some("/bin/zsh".to_string());
        assert_eq!(session_subtext(&local), "zsh");

        // Local with args (should ONLY show shell type, no args)
        local.local_shell = Some("/usr/bin/bash --login -i".to_string());
        assert_eq!(session_subtext(&local), "bash");

        // Local JSON ShellType with args
        let custom_shell = serde_json::json!({
            "type": "Custom",
            "path": "/bin/zsh",
            "args": ["-l", "-i"]
        }).to_string();
        local.local_shell = Some(custom_shell);
        assert_eq!(session_subtext(&local), "zsh");
    }

    #[gpui::test]
    fn test_session_tooltip(cx: &mut TestAppContext) {
        cx.update(|cx| {
            init_locale(Locale::Zh, cx);

            let mut ssh = SshSession::default();
            ssh.name = "My Server".to_string();
            ssh.protocol = SessionProtocol::Ssh;
            ssh.username = "admin".to_string();
            ssh.host = "example.com".to_string();
            ssh.port = 22;
            ssh.auth_type = SshAuthType::Password { password: Some("secret".to_string()) };
            ssh.tags = Some("prod, web".to_string());
            ssh.notes = Some("Production Web Server".to_string());

            let tip = session_tooltip(&ssh, cx);
            assert!(tip.contains("[SSH] My Server"));
            assert!(tip.contains("admin@example.com:22"));
            assert!(tip.contains("密码认证"));
            assert!(tip.contains("标签: prod, web"));
            assert!(tip.contains("备注说明: Production Web Server"));

            let mut serial = SshSession::default();
            serial.name = "Router Console".to_string();
            serial.protocol = SessionProtocol::Serial;
            serial.serial_port = Some("COM1".to_string());
            serial.serial_baud_rate = 9600;

            let tip_serial = session_tooltip(&serial, cx);
            assert!(tip_serial.contains("Router Console"));
            assert!(tip_serial.contains("COM1"));
            assert!(tip_serial.contains("9600 baud"));
        });
    }
}
