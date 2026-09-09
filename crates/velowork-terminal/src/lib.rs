#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

pub mod backend;
pub mod input;
/// macOS process introspection via libproc (replaces `pgrep`/`lsof`/`ps`).
#[cfg(target_os = "macos")]
pub mod macos_proc;
pub mod process;
pub mod pty_manager;
pub mod resolved_config;
pub mod serial_session;
pub mod session_backend;
pub mod shell_config;
pub mod ssh_agent;
pub mod ssh_monitor;
pub mod service_monitor;
pub mod ssh_test;
pub mod telnet_session;
pub mod terminal;
pub mod totp;
pub mod transport_pool;
pub mod tunnel_engine;
pub mod x11;
pub mod zmodem;

pub use pty_manager::{build_russh_client_config, GlobalProxySettings, PtyEvent, PtyManager};
pub use resolved_config::{resolve_effective_terminal_config, ResolvedTerminalConfig, TerminalDefaults};
pub use serial_session::{list_available_serial_ports, SerialConfig, SerialPortDescription};
pub use telnet_session::TelnetConfig;
pub use ssh_monitor::{SshMonitorSource, SshSessionHandle};
pub use service_monitor::{GlobalServiceMonitorEngine, ServiceMonitorEngine, ServiceProbeResult};
pub use transport_pool::{ChannelType, ConnectionManager, ConnectionPolicy, SharedTransport};
pub use tunnel_engine::{TunnelEngine, TunnelHandle};

use gpui::*;

/// Process-wide singleton handle to the SSH tunnel engine.
///
/// Shared by the tunnel panel (start/stop/toggle) and the overlay manager
/// (recursively stop engines when a folder is deleted), so a single engine
/// instance owns every running tunnel's `TunnelHandle`.
pub struct GlobalTunnelEngine(pub Arc<TunnelEngine>);
impl Global for GlobalTunnelEngine {}

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

/// Shared terminals registry for PTY event routing.
/// Maps terminal ID → Terminal instance.
pub type TerminalsRegistry = Arc<Mutex<HashMap<String, Arc<terminal::Terminal>>>>;
