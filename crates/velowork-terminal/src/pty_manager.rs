use crate::session_backend::{ResolvedBackend, SessionBackend};
#[cfg(not(windows))]
use crate::session_backend::get_extended_path;
use crate::shell_config::{ShellCommandExt, ShellType};
use anyhow::Result;
use async_channel::{Receiver, Sender};
use parking_lot::{Mutex, RwLock};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::JoinHandle;


/// Trait for broadcasting PTY output to external consumers (e.g. remote WebSocket clients).
/// Implementations must be thread-safe as this is called from PTY reader threads.
pub trait PtyOutputSink: Send + Sync {
    fn publish(&self, terminal_id: String, data: Vec<u8>);
    /// `server_owns` is true when the origin's local user currently holds resize
    /// authority. Clients use it to stop re-asserting their own window size and
    /// defer to the origin instead of fighting it back over the next round-trip.
    fn publish_resize(&self, _terminal_id: String, _cols: u16, _rows: u16, _server_owns: bool) {}
}

/// Events from PTY processes
#[derive(Debug)]
pub enum PtyEvent {
    /// Data received from PTY
    Data { terminal_id: String, data: Vec<u8> },
    /// PTY process exited
    Exit {
        terminal_id: String,
        exit_code: Option<u32>,
    },
}

/// Shared shutdown coordination between reader/writer threads
struct PtyShutdownState {
    broken: AtomicBool,
    terminal_id: String,
}

impl PtyShutdownState {
    fn new(terminal_id: String) -> Self {
        Self {
            broken: AtomicBool::new(false),
            terminal_id,
        }
    }

    fn is_broken(&self) -> bool {
        self.broken.load(Ordering::Relaxed)
    }

    fn mark_broken(&self) {
        self.broken.store(true, Ordering::Relaxed);
    }
}

/// Extract a human-readable message from a panic payload
fn format_panic(payload: &dyn std::any::Any) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Number of shared teardown worker threads. Bounds how many PTY teardowns
/// (thread joins + `lsof`/`tmux kill-session`/SIGTERM subprocess calls) can run
/// concurrently. On bulk shutdown we enqueue N jobs but only this many run at once,
/// instead of spawning one detached OS thread per `kill()`/`cleanup_exited()` call.
const TEARDOWN_WORKERS: usize = 2;

/// What a teardown worker should do with a job. Modeling the two paths explicitly
/// keeps the deliberate double-fire behavior (see `kill` / `cleanup_exited`) legible.
enum TeardownKind {
    /// The process already EOF'd (reader saw it); we only reap the reader/writer
    /// threads via `shutdown_handle`. No session kill — there's nothing left to
    /// SIGTERM from our side. Enqueued by `cleanup_exited`.
    ReapOnly,
    /// Kill the underlying session backend (tmux/screen/dtach), and on Windows the
    /// WSL session. Enqueued by `kill`. The job's `handle` may be `None`: that is the
    /// "client already exited (cleanup_exited ran first), SIGTERM the lingering
    /// session/daemon" path — in that case the worker does ONLY the session kill.
    KillSession {
        session_backend: ResolvedBackend,
        session_name: String,
        /// WSL distro for the session (Windows only).
        #[cfg(windows)]
        wsl_distro: Option<String>,
        /// Resolved WSL backend; when present the kill happens inside WSL (Windows only).
        #[cfg(windows)]
        wsl_backend: Option<ResolvedBackend>,
    },
}

/// A unit of teardown work handed to the shared worker pool. Everything is owned so
/// the job is `Send` and the workers never touch `PtyManager` state.
struct TeardownJob {
    /// The PTY handle to reap, if we own it. `None` for the `KillSession` path when
    /// `cleanup_exited` already took the handle (double-fire) — then only the session
    /// kill runs.
    handle: Option<PtyHandle>,
    kind: TeardownKind,
}

#[derive(Clone)]
pub struct SshClient {
    pub strict_host_key: velowork_state::StrictHostKey,
    pub host: String,
    pub port: u16,
    pub x11_forwarder: Option<Arc<crate::x11::X11Forwarder>>,
    pub agent_forwarding: bool,
    pub agent_socket_path: Option<String>,
}

impl russh::client::Handler for SshClient {
    type Error = russh::Error;
    fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let host = self.host.clone();
        let port = self.port;
        let mode = self.strict_host_key;
        let key = server_public_key.clone();

        async move {
            match mode {
                velowork_state::StrictHostKey::No => Ok(true),
                velowork_state::StrictHostKey::AcceptNew => {
                    match russh::keys::check_known_hosts(&host, port, &key) {
                        Ok(true) => Ok(true),
                        Ok(false) => {
                            log::info!("[pty:ssh] Accepting new host key | host={}:{}", host, port);
                            Ok(true)
                        }
                        Err(e) => {
                            log::debug!("[pty:ssh] check_known_hosts | host={}:{} | error: {:#}", host, port, e);
                            Ok(true)
                        }
                    }
                }
                velowork_state::StrictHostKey::Yes => {
                    match russh::keys::check_known_hosts(&host, port, &key) {
                        Ok(valid) => Ok(valid),
                        Err(e) => {
                            log::warn!("[pty:ssh] Host key verification failed | host={}:{} | error: {:#}", host, port, e);
                            Ok(false)
                        }
                    }
                }
            }
        }
    }

    fn server_channel_open_x11(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        originator_address: &str,
        originator_port: u32,
        _channel_open_handle: russh::ChannelOpenHandleInner<russh::client::Msg>,
        session: &mut russh::client::Session,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        let forwarder = self.x11_forwarder.clone();
        let orig_addr = originator_address.to_string();
        let orig_port = originator_port;
        let _ = session;

        async move {
            log::debug!("[pty:x11] Incoming X11 forwarding channel | addr={}:{}", orig_addr, orig_port);
            if let Some(fwd) = forwarder {
                tokio::spawn(async move {
                    if let Err(e) = crate::x11::handle_x11_channel(channel, fwd).await {
                        log::warn!("[pty:x11] X11 forwarding connection failed | error: {:#}", e);
                    }
                });
            } else {
                log::warn!("[pty:x11] X11 channel rejected | reason=not enabled");
            }
            Ok(())
        }
    }

    fn server_channel_open_agent_forward(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        reply: russh::ChannelOpenHandleInner<russh::client::Msg>,
        _session: &mut russh::client::Session,
    ) -> impl std::future::Future<Output = Result<(), Self::Error>> + Send {
        let forwarding_enabled = self.agent_forwarding;
        let socket_path = self.agent_socket_path.clone();

        async move {
            if forwarding_enabled {
                reply.accept().await;
                tokio::spawn(async move {
                    if let Err(e) = crate::ssh_agent::handle_agent_forwarding_channel(channel, socket_path).await {
                        log::warn!("[pty:ssh_agent] SSH agent forwarding channel error | error: {:#}", e);
                    }
                });
            } else {
                log::warn!("[pty:ssh_agent] Agent forward channel rejected | reason=not enabled");
            }
            Ok(())
        }
    }
}

/// Handle to a single PTY process
struct PtyHandle {
    /// `Option` so teardown (`shutdown_handle`) and the `Drop` backstop can both
    /// `take()` it idempotently to close the PTY and unblock the reader thread.
    master: Option<Box<dyn MasterPty + Send>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    /// Channel to send input to the writer thread.
    /// `Option` so teardown and the `Drop` backstop can both `take()` it
    /// idempotently to close the channel and unblock the writer thread.
    input_tx: Option<mpsc::Sender<Vec<u8>>>,
    reader_handle: Option<JoinHandle<()>>,
    writer_handle: Option<JoinHandle<()>>,
    shutdown: Arc<PtyShutdownState>,
    /// WSL distro name if this terminal runs inside WSL (Windows only)
    #[cfg(windows)]
    wsl_distro: Option<String>,
    /// Resolved session backend for WSL terminals (Windows only)
    #[cfg(windows)]
    wsl_backend: Option<ResolvedBackend>,
    ssh_session: Option<Arc<russh::client::Handle<SshClient>>>,
    /// The business session id (from `SshSession.id`) this terminal is
    /// connected to, if it is an SSH terminal. Lets UI code map a focused
    /// terminal back to its `enable_sftp` setting without reaching into the
    /// russh handle.
    ssh_session_id: Option<String>,
    ssh_channel_id: Option<russh::ChannelId>,
    ssh_resize_tx: Option<tokio::sync::mpsc::Sender<(u16, u16)>>,
    tokio_input_tx: Option<tokio::sync::mpsc::Sender<Vec<u8>>>,
    tokio_resize_tx: Option<tokio::sync::mpsc::Sender<(u16, u16)>>,
    serial_config: Option<crate::serial_session::SerialConfig>,
    telnet_config: Option<crate::telnet_session::TelnetConfig>,
    exit_signal: Option<Arc<AtomicBool>>,
    last_size: Option<(u16, u16)>,
}

impl Drop for PtyHandle {
    /// Non-blocking teardown backstop.
    ///
    /// The normal teardown path is [`PtyManager::shutdown_handle`], which kills
    /// the child, drops the channel/master, and joins the reader/writer threads.
    /// This `Drop` impl only exists for the off-happy-path case where a handle is
    /// dropped without `shutdown_handle` having run (e.g. a future code path that
    /// removes it from the map directly). In that case we still want the threads
    /// to observe EOF and exit instead of leaking silently.
    ///
    /// It is idempotent (safe to run after `shutdown_handle` already took the
    /// fields) and must NOT block: it signals shutdown and drops `input_tx` /
    /// `master` so the channel and PTY close, but it does NOT join the threads
    /// and does NOT call `child.kill()` (the PID may have been reaped/recycled).
    fn drop(&mut self) {
        // Signal the reader/writer threads to stop (idempotent: just sets a bool).
        self.shutdown.mark_broken();
        if let Some(ref sig) = self.exit_signal {
            sig.store(true, Ordering::Relaxed);
        }
        // Closing the input channel makes the writer thread's `recv` return Err;
        // dropping the master unblocks a reader still stuck in `read`. Both are
        // no-ops if `shutdown_handle` already took them.
        drop(self.input_tx.take());
        drop(self.master.take());
        drop(self.tokio_input_tx.take());
        drop(self.tokio_resize_tx.take());
        drop(self.ssh_resize_tx.take());
        if let Some(session) = self.ssh_session.take() {
            get_tokio_runtime().spawn(async move {
                let _ = session.disconnect(
                    russh::Disconnect::ByApplication,
                    "Session closed",
                    "en",
                ).await;
            });
        }
        // Intentionally do NOT join reader_handle / writer_handle here — a Drop
        // must not block. The threads exit on their own once the channel/master
        // close (or the process exits).
    }
}

/// Manages all PTY processes
pub struct PtyManager {
    terminals: Arc<Mutex<HashMap<String, PtyHandle>>>,
    event_tx: Sender<PtyEvent>,
    /// Session backend for persistence (tmux/screen/none)
    session_backend: ResolvedBackend,
    /// Raw user preference (needed for WSL per-terminal resolution)
    #[cfg(windows)]
    session_backend_preference: SessionBackend,
    /// Optional sink for streaming PTY output to external consumers (e.g. remote clients).
    /// Publishing happens directly from reader threads to avoid UI event loop latency.
    output_sink: Arc<Mutex<Option<Arc<dyn PtyOutputSink>>>>,
    /// Extra environment overrides applied to every spawned PTY. `Some(val)` sets
    /// the variable; `None` removes it from the inherited environment so a stale
    /// value (e.g. a `CLAUDE_CONFIG_DIR` exported in the user's shell that launched
    /// Velowork) cannot leak into the terminal.
    extra_env: Mutex<Vec<(String, Option<String>)>>,
    /// Global default terminal type (TERM) synchronized from AppSettings.
    default_term_type: RwLock<String>,
    /// Sender for the shared teardown worker pool. `kill`/`cleanup_exited` enqueue
    /// jobs here instead of spawning a detached thread per call. Wrapped in `Option`
    /// only so `Drop` can `take()` it and close the channel, signaling workers to
    /// drain remaining jobs and exit.
    teardown_tx: Option<Sender<TeardownJob>>,
}

/// 全局网络代理配置（由应用层设置面板热更新）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobalProxySettings {
    /// 代理模式："none"、"system"、"http"
    pub mode: String,
    /// HTTP 代理主机名或 IP 地址
    pub host: String,
    /// HTTP 代理端口（默认 8080）
    pub port: u16,
}

impl GlobalProxySettings {
    pub fn new() -> Self {
        Self {
            mode: "none".to_string(),
            host: String::new(),
            port: 8080,
        }
    }
}

impl Default for GlobalProxySettings {
    fn default() -> Self {
        Self::new()
    }
}

static GLOBAL_PROXY: RwLock<GlobalProxySettings> = RwLock::new(GlobalProxySettings {
    mode: String::new(),
    host: String::new(),
    port: 8080,
});

impl PtyManager {
    /// Create a new PTY manager with the specified session backend
    pub fn new(backend: SessionBackend) -> (Self, Receiver<PtyEvent>) {
        let (tx, rx) = async_channel::bounded(4096);
        let session_backend = backend.resolve();

        if session_backend.supports_persistence() {
            log::debug!("[pty] Session persistence enabled | backend={:?}", session_backend);
        }

        // Clean up stale dtach sockets from previous crashes
        #[cfg(unix)]
        if matches!(session_backend, ResolvedBackend::Dtach)
            && let Err(e) = std::thread::Builder::new()
                .name("dtach-socket-gc".into())
                .spawn(|| {
                    crate::session_backend::cleanup_stale_dtach_sockets();
                })
            {
                log::warn!("failed to spawn dtach cleanup thread: {e}");
            }

        // Shared teardown worker pool. `async-channel` is MPMC, so all workers share
        // one `Receiver` and pull jobs via `recv_blocking`. Unbounded so enqueuing
        // never blocks the GPUI thread; concurrency is bounded by the worker count.
        let (teardown_tx, teardown_rx) = async_channel::unbounded::<TeardownJob>();
        for i in 0..TEARDOWN_WORKERS {
            let rx = teardown_rx.clone();
            if let Err(e) = std::thread::Builder::new()
                .name(format!("pty-teardown-{i}"))
                .stack_size(256 * 1024)
                .spawn(move || {
                    // Exits when the channel is closed AND drained (Drop closes the
                    // sender, then `recv_blocking` returns Err once buffered jobs run).
                    while let Ok(job) = rx.recv_blocking() {
                        Self::run_teardown_job(job);
                    }
                })
            {
                log::error!("failed to spawn teardown worker {i}: {e}");
            }
        }
        // Drop our copy of the receiver so the only receivers are the workers.
        drop(teardown_rx);

        (
            Self {
                terminals: Arc::new(Mutex::new(HashMap::new())),
                event_tx: tx,
                session_backend,
                #[cfg(windows)]
                session_backend_preference: backend,
                output_sink: Arc::new(Mutex::new(None)),
                extra_env: Mutex::new(Vec::new()),
                default_term_type: RwLock::new(velowork_core::DEFAULT_TERM_TYPE.to_string()),
                teardown_tx: Some(teardown_tx),
            },
            rx,
        )
    }

    /// Execute one teardown job on a worker thread. This is exactly what the old
    /// per-call detached closures did: reap the handle's reader/writer threads (if a
    /// handle is present), then run the session kill (only for `KillSession` jobs).
    fn run_teardown_job(job: TeardownJob) {
        if let Some(handle) = job.handle {
            Self::shutdown_handle(handle);
        }
        match job.kind {
            // Process already EOF'd; nothing to SIGTERM from our side.
            TeardownKind::ReapOnly => {}
            TeardownKind::KillSession {
                session_backend,
                session_name,
                #[cfg(windows)]
                wsl_distro,
                #[cfg(windows)]
                wsl_backend,
            } => {
                // On Windows, if this was a WSL terminal with a session backend,
                // kill the session inside WSL instead of on the host.
                #[cfg(windows)]
                {
                    if let Some(backend) = wsl_backend {
                        crate::session_backend::kill_wsl_session(
                            backend,
                            wsl_distro.as_deref(),
                            &session_name,
                        );
                        return;
                    }
                }
                session_backend.kill_session(&session_name);
            }
        }
    }

    /// Set the output sink for streaming PTY output to external consumers.
    /// Must be called after construction, before spawning terminals.
    pub fn set_output_sink(&self, sink: Arc<dyn PtyOutputSink>) {
        *self.output_sink.lock() = Some(sink);
    }

    /// Set the extra environment overrides applied to every spawned PTY.
    /// `Some(val)` sets the variable; `None` removes it from the inherited
    /// environment. Replaces any previously configured overrides.
    pub fn set_extra_env(&self, env: Vec<(String, Option<String>)>) {
        *self.extra_env.lock() = env;
    }

    /// Set the global default terminal type (e.g. from AppSettings).
    pub fn set_default_term_type(&self, term_type: String) {
        *self.default_term_type.write() = term_type;
    }

    /// Get the current global default terminal type.
    pub fn default_term_type(&self) -> String {
        self.default_term_type.read().clone()
    }

    /// Set the global network proxy settings (e.g. from AppSettings).
    pub fn set_global_proxy(&self, settings: GlobalProxySettings) {
        *GLOBAL_PROXY.write() = settings;
    }

    /// Get the current global network proxy settings.
    pub fn global_proxy(&self) -> GlobalProxySettings {
        GLOBAL_PROXY.read().clone()
    }

    /// Create a new terminal with a PTY process (uses system default shell)
    #[allow(dead_code)] // Kept for API compatibility, prefer create_terminal_with_shell
    pub fn create_terminal(&self, cwd: &str) -> Result<String> {
        self.create_terminal_with_shell(cwd, None)
    }

    /// Create a new serial port terminal session
    pub fn create_serial_terminal(&self, config: crate::serial_session::SerialConfig) -> Result<String> {
        let terminal_id = uuid::Uuid::new_v4().to_string();
        let (input_tx, input_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(1024);
        let event_tx = self.event_tx.clone();
        let tid = terminal_id.clone();
        let exit_signal = Arc::new(AtomicBool::new(false));
        let exit_signal_c = exit_signal.clone();
        let config_clone = config.clone();

        get_tokio_runtime().spawn(async move {
            if let Err(e) = crate::serial_session::run_serial_session(tid.clone(), config_clone, event_tx, input_rx, exit_signal_c).await {
                log::warn!("Serial session '{}' ended with error: {e}", tid);
            }
        });

        let handle = PtyHandle {
            master: None,
            child: None,
            input_tx: None,
            tokio_input_tx: Some(input_tx),
            tokio_resize_tx: None,
            reader_handle: None,
            writer_handle: None,
            shutdown: Arc::new(PtyShutdownState::new(terminal_id.clone())),
            #[cfg(windows)]
            wsl_distro: None,
            #[cfg(windows)]
            wsl_backend: None,
            ssh_session: None,
            ssh_session_id: None,
            ssh_channel_id: None,
            ssh_resize_tx: None,
            serial_config: Some(config),
            telnet_config: None,
            exit_signal: Some(exit_signal),
            last_size: None,
        };

        self.terminals.lock().insert(terminal_id.clone(), handle);
        Ok(terminal_id)
    }

    /// Create a new Telnet terminal session
    pub fn create_telnet_terminal(&self, config: crate::telnet_session::TelnetConfig) -> Result<String> {
        let terminal_id = uuid::Uuid::new_v4().to_string();
        let (input_tx, input_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(1024);
        let (resize_tx, resize_rx) = tokio::sync::mpsc::channel::<(u16, u16)>(16);
        let event_tx = self.event_tx.clone();
        let tid = terminal_id.clone();
        let exit_signal = Arc::new(AtomicBool::new(false));
        let exit_signal_c = exit_signal.clone();
        let config_clone = config.clone();

        get_tokio_runtime().spawn(async move {
            if let Err(e) = crate::telnet_session::run_telnet_session(tid.clone(), config_clone, event_tx, input_rx, resize_rx, exit_signal_c).await {
                log::warn!("Telnet session '{}' ended with error: {e}", tid);
            }
        });

        let handle = PtyHandle {
            master: None,
            child: None,
            input_tx: None,
            tokio_input_tx: Some(input_tx),
            tokio_resize_tx: Some(resize_tx),
            reader_handle: None,
            writer_handle: None,
            shutdown: Arc::new(PtyShutdownState::new(terminal_id.clone())),
            #[cfg(windows)]
            wsl_distro: None,
            #[cfg(windows)]
            wsl_backend: None,
            ssh_session: None,
            ssh_session_id: None,
            ssh_channel_id: None,
            ssh_resize_tx: None,
            serial_config: None,
            telnet_config: Some(config),
            exit_signal: Some(exit_signal),
            last_size: None,
        };

        self.terminals.lock().insert(terminal_id.clone(), handle);
        Ok(terminal_id)
    }

    /// Get serial configuration for a terminal if it is a serial terminal
    pub fn get_serial_config(&self, terminal_id: &str) -> Option<crate::serial_session::SerialConfig> {
        self.terminals.lock().get(terminal_id).and_then(|h| h.serial_config.clone())
    }

    /// Get telnet configuration for a terminal if it is a telnet terminal
    pub fn get_telnet_config(&self, terminal_id: &str) -> Option<crate::telnet_session::TelnetConfig> {
        self.terminals.lock().get(terminal_id).and_then(|h| h.telnet_config.clone())
    }

    /// Create a new terminal with a specific shell type
    pub fn create_terminal_with_shell(&self, cwd: &str, shell: Option<&ShellType>) -> Result<String> {
        let terminal_id = uuid::Uuid::new_v4().to_string();
        self.create_terminal_with_id(&terminal_id, cwd, shell)?;
        Ok(terminal_id)
    }

    /// Create or reconnect to a terminal (uses system default shell)
    /// If terminal_id is provided and session backend supports persistence,
    /// it will try to reconnect to an existing session.
    #[allow(dead_code)] // Kept for API compatibility, prefer create_or_reconnect_terminal_with_shell
    pub fn create_or_reconnect_terminal(
        &self,
        terminal_id: Option<&str>,
        cwd: &str,
    ) -> Result<String> {
        self.create_or_reconnect_terminal_with_shell(terminal_id, cwd, None)
    }

    /// Create or reconnect to a terminal with a specific shell type
    pub fn create_or_reconnect_terminal_with_shell(
        &self,
        terminal_id: Option<&str>,
        cwd: &str,
        shell: Option<&ShellType>,
    ) -> Result<String> {
        match terminal_id {
            Some(id) => {
                // Check if we already have this terminal running
                if self.terminals.lock().contains_key(id) {
                    return Ok(id.to_string());
                }
                // Try to reconnect or create with this ID
                self.create_terminal_with_id(id, cwd, shell)?;
                Ok(id.to_string())
            }
            None => self.create_terminal_with_shell(cwd, shell),
        }
    }

    /// Internal: create a terminal with a specific ID
    fn create_terminal_with_id(
        &self,
        terminal_id: &str,
        cwd: &str,
        shell: Option<&ShellType>,
    ) -> Result<()> {
        log::debug!(
            "[pty:spawn] terminal_id={} cwd={}",
            terminal_id,
            cwd
        );
        if let Some(ShellType::Custom { path, args }) = shell
            && path == "serial"
            && let Some((port_arg, baud_arg, session_id)) = parse_serial_args(args)
        {
            let mut session_config = None;
            if let Some(ref sid) = session_id {
                if let Some(db) = velowork_core::storage::database() {
                    let conn = db.conn();
                    if let Ok(payload) = conn.query_row(
                        "SELECT payload FROM session_tree_node WHERE id = ?1 AND kind = 'session'",
                        &[sid.as_str()],
                        |row| row.get::<_, String>(0),
                    ) {
                        if let Ok(session) = serde_json::from_str::<velowork_state::SshSession>(&payload) {
                            session_config = Some(session);
                        }
                    }
                }
            }

            let mut serial_config = crate::serial_session::SerialConfig::default();
            if let Some(mut sc) = session_config {
                sc.normalize();
                let effective_cs = sc
                    .terminal
                    .charset
                    .unwrap_or_else(|| velowork_core::charset::DEFAULT_CHARSET.to_string());
                serial_config.port = sc.serial_port.unwrap_or(port_arg);
                serial_config.baud_rate = sc.serial_baud_rate;
                serial_config.data_bits = sc.serial_data_bits;
                serial_config.stop_bits = sc.serial_stop_bits;
                serial_config.parity = sc.serial_parity;
                serial_config.flow_control = sc.serial_flow_control;
                serial_config.dtr = sc.serial_dtr;
                serial_config.rts = sc.serial_rts;
                serial_config.charset = Some(effective_cs);
                serial_config.display_mode = sc.serial_display_mode;
                serial_config.line_ending = sc.serial_line_ending;
                serial_config.local_echo = sc.serial_local_echo;
                serial_config.timestamps = sc.serial_timestamps;
                serial_config.auto_reconnect = sc.serial_auto_reconnect;
            } else {
                serial_config.port = port_arg;
                serial_config.baud_rate = baud_arg;
            }

            let (input_tx, input_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(1024);
            let event_tx = self.event_tx.clone();
            let tid = terminal_id.to_string();
            let exit_signal = Arc::new(AtomicBool::new(false));
            let exit_signal_c = exit_signal.clone();
            let config_clone = serial_config.clone();

            get_tokio_runtime().spawn(async move {
                if let Err(e) = crate::serial_session::run_serial_session(tid.clone(), config_clone, event_tx, input_rx, exit_signal_c).await {
                    log::warn!("Serial session '{}' ended with error: {e}", tid);
                }
            });

            let handle = PtyHandle {
                master: None,
                child: None,
                input_tx: None,
                tokio_input_tx: Some(input_tx),
                tokio_resize_tx: None,
                reader_handle: None,
                writer_handle: None,
                shutdown: Arc::new(PtyShutdownState::new(terminal_id.to_string())),
                #[cfg(windows)]
                wsl_distro: None,
                #[cfg(windows)]
                wsl_backend: None,
                ssh_session: None,
                ssh_session_id: session_id,
                ssh_channel_id: None,
                ssh_resize_tx: None,
                serial_config: Some(serial_config),
                telnet_config: None,
                exit_signal: Some(exit_signal),
                last_size: None,
            };

            self.terminals.lock().insert(terminal_id.to_string(), handle);
            return Ok(());
        }

        if let Some(ShellType::Custom { path, args }) = shell
            && path == "telnet"
            && let Some((host_arg, port_arg, session_id)) = parse_telnet_args(args)
        {
            let mut session_config = None;
            if let Some(ref sid) = session_id {
                if let Some(db) = velowork_core::storage::database() {
                    let conn = db.conn();
                    if let Ok(payload) = conn.query_row(
                        "SELECT payload FROM session_tree_node WHERE id = ?1 AND kind = 'session'",
                        &[sid.as_str()],
                        |row| row.get::<_, String>(0),
                    ) {
                        if let Ok(session) = serde_json::from_str::<velowork_state::SshSession>(&payload) {
                            session_config = Some(session);
                        }
                    }
                }
            }

            let mut telnet_config = crate::telnet_session::TelnetConfig::default();
            if let Some(mut sc) = session_config.clone() {
                sc.normalize();
                let effective_cs = sc
                    .terminal
                    .charset
                    .unwrap_or_else(|| velowork_core::charset::DEFAULT_CHARSET.to_string());
                let effective_tt = sc
                    .terminal
                    .term_type
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| self.default_term_type());
                telnet_config.host = sc.telnet_host.unwrap_or(host_arg);
                telnet_config.port = if sc.telnet_port > 0 { sc.telnet_port } else { port_arg };
                telnet_config.encoding = effective_cs;
                telnet_config.term_type = effective_tt;
                telnet_config.startup_command = sc.startup_command;
            } else {
                telnet_config.host = host_arg;
                telnet_config.port = port_arg;
                telnet_config.term_type = self.default_term_type();
            }

            let (input_tx, input_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(1024);
            let (resize_tx, resize_rx) = tokio::sync::mpsc::channel::<(u16, u16)>(16);
            let event_tx = self.event_tx.clone();
            let tid = terminal_id.to_string();
            let exit_signal = Arc::new(AtomicBool::new(false));
            let exit_signal_c = exit_signal.clone();
            let config_clone = telnet_config.clone();
            let startup_cmd_opt = telnet_config.startup_command.clone();
            let input_tx_clone = input_tx.clone();

            get_tokio_runtime().spawn(async move {
                let stream_res = connect_tcp_or_proxy_stream(&config_clone.host, config_clone.port, session_config.as_ref()).await;
                match stream_res {
                    Ok(stream) => {
                        if let Some(cmd_str) = startup_cmd_opt {
                            let lines: Vec<String> = cmd_str.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                            if !lines.is_empty() {
                                let tx = input_tx_clone;
                                tokio::spawn(async move {
                                    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                                    for line in lines {
                                        if tx.send(format!("{}\r\n", line).into_bytes()).await.is_err() {
                                            break;
                                        }
                                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                                    }
                                });
                            }
                        }
                        if let Err(e) = crate::telnet_session::run_telnet_session_with_stream(tid.clone(), config_clone, stream, event_tx, input_rx, resize_rx, exit_signal_c).await {
                            log::warn!("Telnet session '{}' ended with error: {e}", tid);
                        }
                    }
                    Err(e) => {
                        log::warn!("Telnet session '{}' connection failed: {e}", tid);
                        let _ = event_tx.send(PtyEvent::Data {
                            terminal_id: tid.clone(),
                            data: format!("\x1b[31m[Failed to connect to Telnet {}:{}: {}]\x1b[0m\r\n", config_clone.host, config_clone.port, e).into_bytes(),
                        }).await;
                        let _ = event_tx.send(PtyEvent::Exit { terminal_id: tid, exit_code: Some(1) }).await;
                    }
                }
            });

            let handle = PtyHandle {
                master: None,
                child: None,
                input_tx: None,
                tokio_input_tx: Some(input_tx),
                tokio_resize_tx: Some(resize_tx),
                reader_handle: None,
                writer_handle: None,
                shutdown: Arc::new(PtyShutdownState::new(terminal_id.to_string())),
                #[cfg(windows)]
                wsl_distro: None,
                #[cfg(windows)]
                wsl_backend: None,
                ssh_session: None,
                ssh_session_id: session_id,
                ssh_channel_id: None,
                ssh_resize_tx: None,
                serial_config: None,
                telnet_config: Some(telnet_config),
                exit_signal: Some(exit_signal),
                last_size: None,
            };

            self.terminals.lock().insert(terminal_id.to_string(), handle);
            return Ok(());
        }

        if let Some(ShellType::Custom { path, args }) = shell
            && path == "ssh"
            && let Some((host, port, username, key_path, session_id, reuse_from)) = parse_ssh_args(args)
        {
            let mut session_config = None;
            if let Some(ref sid) = session_id {
                // 会话的唯一真相源是 velowork.db（SessionStore 把会话树持久化到
                // `session_tree_node` 表）。之前这里只读旧的 ssh_sessions.json，
                // 而新建/已保存的会话并不写入该 JSON，导致查不到会话、密码缺失，
                // 连接时回退到交互式密码提示。这里优先按 id 直接从数据库取出该
                // 会话的完整 payload（含认证密码）。
                if let Some(db) = velowork_core::storage::database() {
                    let conn = db.conn();
                    if let Ok(payload) = conn.query_row(
                        "SELECT payload FROM session_tree_node WHERE id = ?1 AND kind = 'session'",
                        &[sid.as_str()],
                        |row| row.get::<_, String>(0),
                    ) {
                        if let Ok(session) = serde_json::from_str::<velowork_state::SshSession>(&payload) {
                            session_config = Some(session);
                        }
                    }
                }
                // 回退：数据库不可用时读取旧的 ssh_sessions.json
                if session_config.is_none() {
                    if let Some(config) = velowork_state::SshSessionConfig::load_from_disk() {
                        if let Some(session) = config.find_session(sid) {
                            session_config = Some(session.clone());
                        }
                    }
                }
            }

            // Resolve jump session for ProxyJump
            let jump_session = session_config.as_ref()
                .and_then(|sc| {
                    if sc.proxy_type == velowork_state::ProxyType::Jump {
                        sc.jump_session_id.as_deref()
                    } else {
                        None
                    }
                })
                .and_then(|jump_id| {
                    if let Some(db) = velowork_core::storage::database() {
                        let conn = db.conn();
                        if let Ok(payload) = conn.query_row(
                            "SELECT payload FROM session_tree_node WHERE id = ?1 AND kind = 'session'",
                            &[jump_id],
                            |row| row.get::<_, String>(0),
                        ) {
                            if let Ok(session) = serde_json::from_str::<velowork_state::SshSession>(&payload) {
                                return Some(session);
                            }
                        }
                    }
                    None
                });

            let reuse_ssh_session = if let Some(ref source_tid) = reuse_from {
                self.terminals.lock().get(source_tid).and_then(|h| {
                    if let Some(ref s) = h.ssh_session {
                        if !s.is_closed() {
                            return Some(s.clone());
                        }
                    }
                    None
                })
            } else {
                None
            };

            let (input_tx, input_rx) = std::sync::mpsc::channel::<Vec<u8>>();
            let shutdown = Arc::new(PtyShutdownState::new(terminal_id.to_string()));
            let event_tx = self.event_tx.clone();
            let terminal_id_str = terminal_id.to_string();
            
            let handle = PtyHandle {
                master: None,
                child: None,
                input_tx: Some(input_tx.clone()),
                tokio_input_tx: None,
                tokio_resize_tx: None,
                reader_handle: None,
                writer_handle: None,
                shutdown: Arc::clone(&shutdown),
                #[cfg(windows)]
                wsl_distro: None,
                #[cfg(windows)]
                wsl_backend: None,
                ssh_session: None,
                ssh_session_id: None,
                ssh_channel_id: None,
                ssh_resize_tx: None,
                serial_config: None,
                telnet_config: None,
                exit_signal: None,
                last_size: None,
            };
            self.terminals.lock().insert(terminal_id_str.clone(), handle);

            // Associate the terminal with its SSH session id as early as possible,
            // *before* the connection attempt. `run_ssh_connection` only sets
            // `ssh_session_id` after a successful channel open, so if the connection
            // fails the handle would keep `ssh_session_id == None`. Tying the id to
            // the terminal at creation time lets the disconnect logic (in
            // velowork-app) revert the session-tree icon even for failed connections
            // when the tab is later closed or the terminal exits.
            if let Some(ref sid) = session_id {
                if let Some(h) = self.terminals.lock().get_mut(&terminal_id_str) {
                    h.ssh_session_id = Some(sid.clone());
                }
            }

            let rt = get_tokio_runtime();
            let terminals = self.terminals.clone();
            let default_term_type = self.default_term_type();
            rt.spawn(async move {
                if let Err(e) = run_ssh_connection(
                    terminal_id_str.clone(),
                    host.clone(),
                    port,
                    username.clone(),
                    key_path,
                    session_config,
                    jump_session,
                    reuse_ssh_session,
                    input_rx,
                    event_tx.clone(),
                    shutdown,
                    terminals.clone(),
                    default_term_type,
                ).await {
                    log::error!("[pty:ssh] SSH connection error | host={}:{} user={} terminal_id={} | error: {:#}", host, port, username, terminal_id_str, e);
                    let err_str = e.to_string();
                    if !err_str.contains("Permission denied") && !err_str.contains("Authentication cancelled") {
                        let _ = event_tx.send(PtyEvent::Data {
                            terminal_id: terminal_id_str.clone(),
                            data: format!("\r\nConnection error: {}\r\n", e).into_bytes(),
                        }).await;
                    }
                    terminals.lock().remove(&terminal_id_str);
                    let _ = event_tx.send(PtyEvent::Exit {
                        terminal_id: terminal_id_str,
                        exit_code: Some(1),
                    }).await;
                }
            });
            return Ok(());
        }

        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut local_session_cfg: Option<velowork_state::SshSession> = None;
        let mut effective_shell = shell.cloned();

        if let Some(ShellType::Custom { path, args }) = shell
            && path == "local"
        {
            if let Some(sid) = parse_local_args(args) {
                if let Some(db) = velowork_core::storage::database() {
                    let conn = db.conn();
                    if let Ok(payload) = conn.query_row(
                        "SELECT payload FROM session_tree_node WHERE id = ?1 AND kind = 'session'",
                        &[sid.as_str()],
                        |row| row.get::<_, String>(0),
                    ) {
                        if let Ok(s) = serde_json::from_str::<velowork_state::SshSession>(&payload) {
                            local_session_cfg = Some(s);
                        }
                    }
                }
            }

            let shell_from_args = parse_local_shell_arg(args);
            effective_shell = local_session_cfg
                .as_ref()
                .and_then(|s| s.local_shell.as_deref())
                .or(shell_from_args.as_deref())
                .and_then(|s| serde_json::from_str::<ShellType>(s).ok())
                .or(Some(ShellType::Default));
        }

        let effective_term_type = local_session_cfg
            .as_ref()
            .and_then(|s| s.terminal.term_type.clone())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| self.default_term_type());

        let mut effective_cwd: Option<String> = if cwd.trim().is_empty() {
            None
        } else {
            Some(cwd.to_string())
        };
        if let Some(ref sc) = local_session_cfg
            && let Some(ref lcwd) = sc.local_cwd
        {
            let trimmed = lcwd.trim();
            if !trimmed.is_empty() {
                let expanded = if let Some(stripped) = trimmed.strip_prefix("~/") {
                    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
                        format!("{}/{}", home.trim_end_matches(['/', '\\']), stripped)
                    } else {
                        trimmed.to_string()
                    }
                } else if trimmed == "~" {
                    std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_else(|_| trimmed.to_string())
                } else {
                    trimmed.to_string()
                };
                let p = std::path::Path::new(&expanded);
                if p.is_dir() {
                    effective_cwd = Some(expanded);
                } else {
                    log::warn!("[pty:local] Configured local_cwd '{}' does not exist or is not a directory, falling back to default", trimmed);
                }
            }
        }
        let final_cwd = effective_cwd.as_deref().unwrap_or(cwd);

        // Build command based on session backend and shell config
        #[cfg(unix)]
        let mut cmd = self.build_terminal_command(terminal_id, final_cwd, effective_shell.as_ref(), &effective_term_type);
        #[cfg(windows)]
        let (mut cmd, wsl_distro, wsl_backend) = self.build_terminal_command(terminal_id, final_cwd, effective_shell.as_ref(), &effective_term_type);

        if let Some(ref sc) = local_session_cfg {
            for (key, val) in &sc.local_env {
                cmd.env(key, val);
            }
        }

        // Apply caller-configured env overrides to the PTY unconditionally.
        // These are profile-scoped values (e.g. CLAUDE_CONFIG_DIR) that must
        // override whatever the user's shell rc or the parent process has set.
        // `None` removes the variable so a stale inherited value cannot leak in.
        for (key, val) in &*self.extra_env.lock() {
            match val {
                Some(val) => cmd.env(key, val),
                None => cmd.env_remove(key),
            }
        }

        // Spawn the process
        let child = pair.slave.spawn_command(cmd)?;

        // Get reader and writer
        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let shutdown = Arc::new(PtyShutdownState::new(terminal_id.to_string()));
        let child_pid = child.process_id();

        // Spawn reader thread with panic guard
        let tx = self.event_tx.clone();
        let id = terminal_id.to_string();
        let reader_shutdown = Arc::clone(&shutdown);
        let output_sink = self.output_sink.lock().clone();
        let reader_handle = std::thread::Builder::new()
            .name(format!("pty-reader-{}", &terminal_id[..8.min(terminal_id.len())]))
            .spawn(move || {
                let tx_panic = tx.clone();
                let shutdown_panic = Arc::clone(&reader_shutdown);
                let id_panic = id.clone();
                if let Err(panic) = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    Self::read_loop(id, reader, tx, reader_shutdown, child_pid, output_sink);
                })) {
                    log::error!("[pty] PTY reader thread panicked | terminal_id={} | error: {}", id_panic, format_panic(&*panic));
                    shutdown_panic.mark_broken();
                    let _ = tx_panic.send_blocking(PtyEvent::Exit {
                        terminal_id: id_panic,
                        exit_code: None,
                    });
                }
            })?;

        // Create input channel and spawn writer thread with panic guard
        let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>();
        let writer_shutdown = Arc::clone(&shutdown);
        let writer_event_tx = self.event_tx.clone();
        let writer_id = terminal_id.to_string();
        let writer_handle = std::thread::Builder::new()
            .name(format!("pty-writer-{}", &terminal_id[..8.min(terminal_id.len())]))
            .spawn(move || {
                let tx_panic = writer_event_tx.clone();
                let shutdown_panic = Arc::clone(&writer_shutdown);
                let id_panic = writer_id.clone();
                if let Err(panic) = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    Self::write_loop(writer, input_rx, writer_shutdown, writer_event_tx, writer_id);
                })) {
                    log::error!("[pty] PTY writer thread panicked | terminal_id={} | error: {}", id_panic, format_panic(&*panic));
                    shutdown_panic.mark_broken();
                    let _ = tx_panic.send_blocking(PtyEvent::Exit {
                        terminal_id: id_panic,
                        exit_code: None,
                    });
                }
            })?;

        // Execute local startup command if configured
        if let Some(ref sc) = local_session_cfg {
            if let Some(ref cmd_str) = sc.startup_command {
                let lines: Vec<String> = cmd_str
                    .lines()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !lines.is_empty() {
                    let startup_tx = input_tx.clone();
                    let _ = std::thread::Builder::new()
                        .name(format!("local-startup-{}", &terminal_id[..8.min(terminal_id.len())]))
                        .spawn(move || {
                            std::thread::sleep(std::time::Duration::from_millis(250));
                            for line in lines {
                                #[cfg(windows)]
                                let bytes = format!("{}\r\n", line).into_bytes();
                                #[cfg(not(windows))]
                                let bytes = format!("{}\n", line).into_bytes();
                                if startup_tx.send(bytes).is_err() {
                                    break;
                                }
                                std::thread::sleep(std::time::Duration::from_millis(100));
                            }
                        });
                }
            }
        }

        let local_session_id = match shell {
            Some(ShellType::Custom { path, args }) if path == "local" => parse_local_args(args),
            _ => None,
        };

        // Store the handle
        self.terminals.lock().insert(
            terminal_id.to_string(),
            PtyHandle {
                master: Some(pair.master),
                child: Some(child),
                input_tx: Some(input_tx),
                tokio_input_tx: None,
                tokio_resize_tx: None,
                reader_handle: Some(reader_handle),
                writer_handle: Some(writer_handle),
                shutdown,
                #[cfg(windows)]
                wsl_distro,
                #[cfg(windows)]
                wsl_backend,
                ssh_session: None,
                ssh_session_id: local_session_id,
                ssh_channel_id: None,
                ssh_resize_tx: None,
                serial_config: None,
                telnet_config: None,
                exit_signal: None,
                last_size: None,
            },
        );

        Ok(())
    }

    /// Build the command to run in the terminal.
    /// On Unix, returns just the CommandBuilder.
    /// On Windows, also returns WSL distro/backend info for session persistence.
    #[cfg(unix)]
    fn build_terminal_command(
        &self,
        terminal_id: &str,
        cwd: &str,
        shell: Option<&ShellType>,
        term_type: &str,
    ) -> CommandBuilder {
        // Extract custom command from ShellType::Custom{path:<shell>, args:["-c"/"-ic", cmd]}
        // so it can be passed to the session backend
        let custom_command = match shell {
            Some(ShellType::Custom { args, .. }) if args.len() == 2 && (args[0] == "-c" || args[0] == "-ic") => {
                Some(args[1].as_str())
            }
            _ => None,
        };

        let extra_env = self.extra_env.lock().clone();
        let mut cmd = if let Some((program, args)) = self
            .session_backend
            .build_command(&self.session_backend.session_name(terminal_id), cwd, custom_command, &extra_env)
        {
            let mut cmd = CommandBuilder::new(program);
            for arg in args {
                cmd.arg(arg);
            }
            // For screen, we need to set cwd separately as it doesn't have -c flag
            if matches!(self.session_backend, ResolvedBackend::Screen) {
                cmd.cwd(cwd);
            }
            cmd
        } else {
            // No session backend - use shell config or default
            match shell {
                Some(shell_type) => shell_type.build_command(cwd),
                None => {
                    let mut cmd = CommandBuilder::new_default_prog();
                    cmd.cwd(cwd);
                    cmd
                }
            }
        };

        Self::set_terminal_env(&mut cmd, terminal_id, term_type);
        cmd
    }

    /// Build the command to run in the terminal (Windows version).
    /// Returns (cmd, wsl_distro, wsl_backend) for WSL session tracking.
    #[cfg(windows)]
    fn build_terminal_command(
        &self,
        terminal_id: &str,
        cwd: &str,
        shell: Option<&ShellType>,
        term_type: &str,
    ) -> (CommandBuilder, Option<String>, Option<ResolvedBackend>) {
        use crate::session_backend::resolve_for_wsl;
        use crate::shell_config::windows_path_to_wsl;

        // Extract custom command from ShellType::Custom{path:<shell>, args:["-c"/"-ic", cmd]}
        let custom_command = match shell {
            Some(ShellType::Custom { args, .. }) if args.len() == 2 && (args[0] == "-c" || args[0] == "-ic") => {
                Some(args[1].as_str())
            }
            _ => None,
        };

        // Wrap a non-WSL shell through the host session backend (psmux) when one
        // is available. WSL terminals get their own per-distro backend below
        // because the daemon must live inside WSL, not on the host.
        let wrap_with_host_backend = |fallback: CommandBuilder| -> CommandBuilder {
            if !self.session_backend.supports_persistence() {
                return fallback;
            }
            let session_name = self.session_backend.session_name(terminal_id);
            let extra_env = self.extra_env.lock().clone();
            match self.session_backend.build_command(&session_name, cwd, custom_command, &extra_env) {
                Some((program, args)) => {
                    let mut cmd = CommandBuilder::new(program);
                    for arg in args {
                        cmd.arg(arg);
                    }
                    cmd
                }
                None => fallback,
            }
        };

        let (mut cmd, wsl_distro, wsl_backend) = match shell {
            Some(ShellType::Wsl { distro }) => {
                let wsl_backend = resolve_for_wsl(distro.as_deref(), self.session_backend_preference);
                let session_name = wsl_backend.session_name(terminal_id);
                let wsl_cwd = windows_path_to_wsl(cwd);

                if let Some((program, args)) = wsl_backend.build_wsl_session_command(
                    distro.as_deref(),
                    &session_name,
                    &wsl_cwd,
                    custom_command,
                ) {
                    let mut cmd = CommandBuilder::new(program);
                    for arg in args {
                        cmd.arg(arg);
                    }
                    (cmd, distro.clone(), Some(wsl_backend))
                } else {
                    (
                        ShellType::Wsl { distro: distro.clone() }.build_command(cwd),
                        distro.clone(),
                        None,
                    )
                }
            }
            Some(shell_type) => (wrap_with_host_backend(shell_type.build_command(cwd)), None, None),
            None => {
                let mut default_cmd = CommandBuilder::new_default_prog();
                default_cmd.cwd(cwd);
                (wrap_with_host_backend(default_cmd), None, None)
            }
        };

        Self::set_terminal_env(&mut cmd, terminal_id, term_type);
        (cmd, wsl_distro, wsl_backend)
    }

    /// Set common terminal environment variables on a command.
    fn set_terminal_env(cmd: &mut CommandBuilder, terminal_id: &str, term_type: &str) {
        // Allow processes inside the terminal to identify which Velowork terminal they run in
        cmd.env("VELOWORK_TERMINAL_ID", terminal_id);

        // Set TERM environment variable - required for proper terminal operation
        // especially when running as a macOS app bundle which doesn't inherit shell environment
        cmd.env("TERM", term_type);
        // COLORTERM enables 24-bit truecolor support in many applications.
        // Suppress COLORTERM for dumb/ansi/vt terminals to prevent escape sequences pollution.
        if term_type != "dumb" && term_type != "ansi" && !term_type.starts_with("vt") {
            cmd.env("COLORTERM", "truecolor");
        }

        // Ensure UTF-8 locale for child processes. macOS app bundles launched from
        // Finder/Spotlight don't inherit shell environment, so LANG defaults to
        // C/POSIX (ASCII-only). This breaks non-ASCII text in shells and CLI tools.
        #[cfg(not(windows))]
        if std::env::var("LANG").is_err() {
            cmd.env("LANG", "en_US.UTF-8");
        }

        // Extend PATH for child processes. Desktop entries and app bundles start
        // with a minimal PATH missing user tools (~/.cargo/bin, ~/.bun/bin, etc.)
        #[cfg(not(windows))]
        cmd.env("PATH", get_extended_path());
    }

    /// Read loop for PTY output
    fn read_loop(
        terminal_id: String,
        mut reader: Box<dyn Read + Send>,
        tx: Sender<PtyEvent>,
        shutdown: Arc<PtyShutdownState>,
        child_pid: Option<u32>,
        output_sink: Option<Arc<dyn PtyOutputSink>>,
    ) {
        // Use larger buffer like alacritty (they use 1MB, we use 64KB)
        let mut buf = [0u8; 65536];
        loop {
            if shutdown.is_broken() {
                log::debug!("PTY reader {} stopping: shutdown signaled", terminal_id);
                break;
            }
            match reader.read(&mut buf) {
                Ok(0) => {
                    // EOF - process exited, try to get exit code
                    let exit_code = child_pid.and_then(wait_for_exit_code);
                    let _ = tx.send_blocking(PtyEvent::Exit {
                        terminal_id,
                        exit_code,
                    });
                    break;
                }
                Ok(n) => {
                    if shutdown.is_broken() {
                        break;
                    }
                    let data = buf[..n].to_vec();
                    log::trace!("PTY {} received {} bytes", terminal_id, n);
                    // Broadcast to external consumers immediately (bypasses UI event loop)
                    if let Some(ref sink) = output_sink {
                        sink.publish(terminal_id.clone(), data.clone());
                    }
                    // send_blocking will block when channel is full (backpressure)
                    if tx.send_blocking(PtyEvent::Data {
                        terminal_id: terminal_id.clone(),
                        data,
                    }).is_err() {
                        // Receiver dropped - app is shutting down
                        break;
                    }
                }
                Err(e) => {
                    if !shutdown.is_broken() {
                        log::error!("[pty] PTY read error | terminal_id={} | error: {:#}", terminal_id, e);
                    }
                    let exit_code = child_pid.and_then(wait_for_exit_code);
                    let _ = tx.send_blocking(PtyEvent::Exit {
                        terminal_id,
                        exit_code,
                    });
                    break;
                }
            }
        }
    }

    /// Write loop for PTY input - batches writes for better performance
    fn write_loop(
        mut writer: Box<dyn Write + Send>,
        rx: mpsc::Receiver<Vec<u8>>,
        shutdown: Arc<PtyShutdownState>,
        event_tx: Sender<PtyEvent>,
        terminal_id: String,
    ) {
        // Loop exits when the channel is closed (`recv` returns Err).
        while let Ok(first) = rx.recv() {
            // Collect any additional pending messages (non-blocking)
            let mut batch = first;
            while let Ok(data) = rx.try_recv() {
                batch.extend(data);
            }

            // Write the batched data
            if let Err(e) = writer.write_all(&batch) {
                log::error!("[pty] Failed to write to PTY | terminal_id={} | error: {:#}", terminal_id, e);
                shutdown.mark_broken();
                let _ = event_tx.send_blocking(PtyEvent::Exit {
                    terminal_id,
                    exit_code: None,
                });
                break;
            }
        }
    }

    /// Send input to a terminal
    /// Input is sent through a channel to a dedicated writer thread,
    /// which batches writes for better performance.
    pub fn send_input(&self, terminal_id: &str, data: &[u8]) {
        if let Some(handle) = self.terminals.lock().get(terminal_id) {
            if let Some(input_tx) = handle.input_tx.as_ref() {
                let _ = input_tx.send(data.to_vec());
            } else if let Some(tokio_input_tx) = handle.tokio_input_tx.as_ref() {
                let _ = tokio_input_tx.try_send(data.to_vec());
            }
        } else {
            log::warn!("[pty:input] send_input terminal not found | terminal_id={}", terminal_id);
        }
    }

    /// Resize a terminal
    pub fn resize(&self, terminal_id: &str, cols: u16, rows: u16) {
        let prev_size = self.terminals.lock().get(terminal_id).and_then(|h| h.last_size);
        log::debug!(
            "[pty:resize] terminal_id={} cols={} rows={} prev={:?}",
            terminal_id,
            cols,
            rows,
            prev_size
        );
        if let Some(handle) = self.terminals.lock().get_mut(terminal_id) {
            if handle.last_size == Some((cols, rows)) {
                log::debug!(
                    "[pty:resize_noop] terminal_id={} already has size {}x{}, skipping SIGWINCH",
                    terminal_id, cols, rows
                );
                return;
            }
            handle.last_size = Some((cols, rows));
            if let Some(master) = handle.master.as_ref() {
                log::debug!(
                    "[pty:sigwinch] terminal_id={} sending SIGWINCH to kernel: cols={} rows={}",
                    terminal_id, cols, rows
                );
                if let Err(e) = master.resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                }) {
                    log::error!("[pty] Failed to resize PTY | terminal_id={} | error: {:#}", terminal_id, e);
                }
            } else if let Some(ssh_resize_tx) = handle.ssh_resize_tx.clone() {
                log::debug!(
                    "[pty:ssh_resize] terminal_id={} sending ssh resize: cols={} rows={}",
                    terminal_id, cols, rows
                );
                let _ = ssh_resize_tx.try_send((cols, rows));
            } else if let Some(tokio_resize_tx) = handle.tokio_resize_tx.clone() {
                let _ = tokio_resize_tx.try_send((cols, rows));
            }
        }
        // Notify remote clients about the resize so they can update their grids.
        // Carry the current resize authority so a client knows whether this
        // resize comes from the origin's local user reclaiming control — in
        // which case the client must stop re-asserting its own size.
        if let Some(sink) = self.output_sink.lock().as_ref() {
            let server_owns = crate::terminal::is_resize_authority_local();
            sink.publish_resize(terminal_id.to_string(), cols, rows, server_owns);
        }
    }

    /// Kill a terminal
    /// Also kills the underlying tmux/screen session if applicable
    pub fn kill(&self, terminal_id: &str) {
        // Remove handle from map immediately (fast, non-blocking).
        // `handle` may be `None` if `cleanup_exited` already took it on PTY EOF
        // (the double-fire). In that case the enqueued job does ONLY the session
        // kill below — SIGTERMing the lingering session/daemon after the client EOF'd.
        let handle = self.terminals.lock().remove(terminal_id);
        let session_backend = self.session_backend;
        let session_name = session_backend.session_name(terminal_id);

        // Read WSL info before moving the handle
        #[cfg(windows)]
        let wsl_distro = handle.as_ref().and_then(|h| h.wsl_distro.clone());
        #[cfg(windows)]
        let wsl_backend = handle.as_ref().and_then(|h| h.wsl_backend);

        let job = TeardownJob {
            handle,
            kind: TeardownKind::KillSession {
                session_backend,
                session_name,
                #[cfg(windows)]
                wsl_distro,
                #[cfg(windows)]
                wsl_backend,
            },
        };
        self.enqueue_teardown(job);
    }

    /// Hand a teardown job to the shared worker pool. If the channel is closed
    /// (only happens once `Drop` has taken the sender during quit), run the teardown
    /// inline so a handle is never silently leaked — better to block briefly on the
    /// calling thread than to drop reader/writer threads on the floor.
    fn enqueue_teardown(&self, job: TeardownJob) {
        match self.teardown_tx.as_ref() {
            Some(tx) => {
                if let Err(e) = tx.send_blocking(job) {
                    log::warn!("[pty:teardown] Teardown channel closed; running teardown inline");
                    Self::run_teardown_job(e.into_inner());
                }
            }
            // Sender already taken by Drop — fall back to inline teardown.
            None => Self::run_teardown_job(job),
        }
    }

    /// Perform coordinated shutdown of a single PTY handle
    fn shutdown_handle(mut handle: PtyHandle) {
        let id = &handle.shutdown.terminal_id;

        // 1. Signal shutdown to threads
        handle.shutdown.mark_broken();
        if let Some(ref sig) = handle.exit_signal {
            sig.store(true, Ordering::Relaxed);
        }

        // 2. Kill child process - closes PTY slave, reader gets EOF
        if let Some(ref mut child) = handle.child {
            if let Err(e) = child.kill() {
                log::warn!("[pty] Failed to kill PTY process | terminal_id={} | error: {:#}", id, e);
            }
        }

        // 3. Drop input_tx and async channels
        drop(handle.input_tx.take());
        drop(handle.tokio_input_tx.take());
        drop(handle.tokio_resize_tx.take());
        drop(handle.ssh_resize_tx.take());

        // 4. Disconnect SSH session if present
        if let Some(session) = handle.ssh_session.take() {
            get_tokio_runtime().spawn(async move {
                let _ = session.disconnect(
                    russh::Disconnect::ByApplication,
                    "Session closed by user",
                    "en",
                ).await;
            });
        }

        // 5. Drop master - safety net to unblock reader if still stuck
        drop(handle.master.take());

        // 6. Join writer thread (should exit quickly after input_tx drop)
        if let Some(h) = handle.writer_handle.take()
            && let Err(e) = h.join() {
                log::warn!("[pty] PTY writer thread panicked on join | terminal_id={} | error: {}", id, format_panic(&*e));
            }

        // 7. Join reader thread (should exit after child kill + master drop)
        if let Some(h) = handle.reader_handle.take()
            && let Err(e) = h.join() {
                log::warn!("[pty] PTY reader thread panicked on join | terminal_id={} | error: {}", id, format_panic(&*e));
            }

        // 8. Reap the child to prevent a zombie. The reader normally reaps via
        //    `wait_for_exit_code` on EOF, but that is a bounded `WNOHANG` poll that
        //    gives up if the SIGKILL'd child is briefly unreapable (e.g. stuck in
        //    D-state on slow IO). Now that the reader has joined, a blocking wait
        //    guarantees the PID is reaped. If the reader already reaped it via raw
        //    `waitpid`, this just returns ECHILD, which is harmless.
        if let Some(ref mut child) = handle.child {
            if let Err(e) = child.wait() {
                log::debug!("[pty] PTY child already reaped or wait failed | terminal_id={} | error: {:#}", id, e);
            }
        }

        // 9. Trigger memory trim / arena purge back to OS
        velowork_core::memory::trim_process_memory();
    }

    /// Detach from all terminals without killing sessions
    /// Sessions will persist and can be reconnected on next app start
    pub fn detach_all(&self) {
        // Drain all handles while holding the lock, then release lock before joining
        let handles: Vec<PtyHandle> = self.terminals.lock().drain().map(|(_, h)| h).collect();
        for handle in handles {
            Self::shutdown_handle(handle);
        }
    }

    /// Get the shell process PID for a terminal
    pub fn get_shell_pid(&self, terminal_id: &str) -> Option<u32> {
        self.terminals.lock().get(terminal_id)
            .and_then(|h| h.child.as_ref().and_then(|c| c.process_id()))
    }

    /// Get the active SSH session handle if this is a programmatic SSH connection
    pub fn get_ssh_session(&self, terminal_id: &str) -> Option<Arc<russh::client::Handle<SshClient>>> {
        self.terminals.lock().get(terminal_id)
            .and_then(|h| h.ssh_session.clone())
    }

    /// Get the business session id this terminal is connected to, if it is an
    /// SSH terminal. Returns `None` for local terminals or before a connection
    /// has been established.
    pub fn get_ssh_session_id(&self, terminal_id: &str) -> Option<String> {
        self.terminals.lock().get(terminal_id)
            .and_then(|h| h.ssh_session_id.clone())
    }

    /// Total count of active SSH terminals currently open.
    pub fn active_ssh_session_count(&self) -> usize {
        self.terminals
            .lock()
            .values()
            .filter(|h| h.ssh_session_id.is_some() || h.ssh_session.is_some())
            .count()
    }

    /// Whether there is at least one active SSH terminal currently open.
    pub fn has_active_ssh_sessions(&self) -> bool {
        self.terminals
            .lock()
            .values()
            .any(|h| h.ssh_session_id.is_some() || h.ssh_session.is_some())
    }

    /// Get the real foreground shell pid for this terminal, resolving through
    /// session-backend proxies (dtach / tmux). For plain PTYs this is the same
    /// as `get_shell_pid`. For dtach, walks from the daemon to its direct child
    /// (the actual shell). For tmux, the pane pid returned by `list-panes` IS
    /// the shell pid. Callers get a pid they can pgrep / `/proc`-inspect for
    /// running children.
    pub fn get_foreground_shell_pid(&self, terminal_id: &str) -> Option<u32> {
        #[cfg(unix)]
        {
            match self.session_backend {
                ResolvedBackend::Dtach => {
                    if let Some(daemon) = self.get_dtach_service_pids(terminal_id).into_iter().next() {
                        return first_proc_child(daemon).or(Some(daemon));
                    }
                }
                ResolvedBackend::Tmux => {
                    if let Some(pane) = self.get_tmux_service_pids(terminal_id).into_iter().next() {
                        return Some(pane);
                    }
                }
                _ => {}
            }
        }
        self.get_shell_pid(terminal_id)
    }

    /// Get root PIDs for port detection.
    /// With session backends (dtach/tmux), the PTY child is the attach process,
    /// not the actual service. This method finds the real service root PID.
    pub fn get_service_pids(&self, terminal_id: &str) -> Vec<u32> {
        #[cfg(unix)]
        {
            match self.session_backend {
                ResolvedBackend::Dtach => {
                    return self.get_dtach_service_pids(terminal_id);
                }
                ResolvedBackend::Tmux => {
                    return self.get_tmux_service_pids(terminal_id);
                }
                _ => {}
            }
        }
        self.get_shell_pid(terminal_id).into_iter().collect()
    }

    /// Find the dtach daemon PID holding the session socket, excluding the
    /// attach PID. Uses the /proc-based socket scan (no `lsof` subprocess on
    /// Linux — a per-poll `lsof -t` was ~1s each).
    #[cfg(unix)]
    fn get_dtach_service_pids(&self, terminal_id: &str) -> Vec<u32> {
        let session_name = self.session_backend.session_name(terminal_id);
        let socket_path = match self.session_backend.socket_path(&session_name) {
            Some(p) if p.exists() => p,
            _ => return self.get_shell_pid(terminal_id).into_iter().collect(),
        };

        let holders = find_pids_for_unix_sockets(std::slice::from_ref(&socket_path));
        let attach_pid = self.get_shell_pid(terminal_id);
        let pids: Vec<u32> = holders
            .get(&socket_path)
            .into_iter()
            .flatten()
            .copied()
            .filter(|pid| Some(*pid) != attach_pid)
            .collect();

        if pids.is_empty() {
            self.get_shell_pid(terminal_id).into_iter().collect()
        } else {
            pids
        }
    }

    /// Find the shell PID inside a tmux session pane.
    #[cfg(unix)]
    fn get_tmux_service_pids(&self, terminal_id: &str) -> Vec<u32> {
        let session_name = self.session_backend.session_name(terminal_id);
        let output = match crate::process::safe_output(
            crate::process::command("tmux")
                .args(["list-panes", "-t", &session_name, "-F", "#{pane_pid}"]),
        ) {
            Ok(o) if o.status.success() => o,
            _ => return self.get_shell_pid(terminal_id).into_iter().collect(),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let pids: Vec<u32> = stdout
            .lines()
            .filter_map(|line| line.trim().parse::<u32>().ok())
            .collect();

        if pids.is_empty() {
            self.get_shell_pid(terminal_id).into_iter().collect()
        } else {
            pids
        }
    }

    /// Batch version of `get_service_pids` for multiple terminals at once.
    /// On Linux with dtach, reads `/proc` once instead of spawning `lsof` per terminal.
    pub fn get_batch_service_pids(&self, terminal_ids: &[&str]) -> HashMap<String, Vec<u32>> {
        #[cfg(unix)]
        {
            if self.session_backend == ResolvedBackend::Dtach {
                return self.get_batch_dtach_service_pids(terminal_ids);
            }
        }
        // Fallback: call per-terminal method
        terminal_ids
            .iter()
            .map(|tid| (tid.to_string(), self.get_service_pids(tid)))
            .collect()
    }

    /// Batch dtach PID lookup. On Linux, reads `/proc/net/unix` + `/proc/*/fd/`
    /// once for all sockets. On other Unix, falls back to lsof per terminal.
    #[cfg(unix)]
    fn get_batch_dtach_service_pids(&self, terminal_ids: &[&str]) -> HashMap<String, Vec<u32>> {
        // Collect socket paths for all terminals
        let mut socket_to_terminal: HashMap<std::path::PathBuf, &str> = HashMap::new();
        let mut attach_pids: HashMap<&str, Option<u32>> = HashMap::new();

        for &tid in terminal_ids {
            let session_name = self.session_backend.session_name(tid);
            if let Some(p) = self.session_backend.socket_path(&session_name)
                && p.exists() {
                    socket_to_terminal.insert(p, tid);
                    attach_pids.insert(tid, self.get_shell_pid(tid));
                }
        }

        // Resolve PIDs for all sockets at once
        let socket_pids = find_pids_for_unix_sockets(
            &socket_to_terminal.keys().cloned().collect::<Vec<_>>(),
        );

        // Build result map
        let mut result: HashMap<String, Vec<u32>> = HashMap::new();
        for &tid in attach_pids.keys() {
            let session_name = self.session_backend.session_name(tid);
            let socket_path = match self.session_backend.socket_path(&session_name) {
                Some(p) => p,
                None => {
                    result.insert(
                        tid.to_string(),
                        self.get_shell_pid(tid).into_iter().collect(),
                    );
                    continue;
                }
            };

            let attach_pid = attach_pids.get(tid).copied().flatten();
            let pids: Vec<u32> = socket_pids
                .get(&socket_path)
                .map(|pids| {
                    pids.iter()
                        .copied()
                        .filter(|pid| Some(*pid) != attach_pid)
                        .collect()
                })
                .unwrap_or_default();

            if pids.is_empty() {
                result.insert(
                    tid.to_string(),
                    self.get_shell_pid(tid).into_iter().collect(),
                );
            } else {
                result.insert(tid.to_string(), pids);
            }
        }

        // Terminals without a valid socket path
        for &tid in terminal_ids {
            result
                .entry(tid.to_string())
                .or_insert_with(|| self.get_shell_pid(tid).into_iter().collect());
        }

        result
    }

    /// Check if the session backend handles mouse events (tmux with mouse on)
    pub fn uses_mouse_backend(&self) -> bool {
        matches!(self.session_backend, ResolvedBackend::Tmux)
    }

    /// Capture the terminal buffer to a file (only works with tmux backend)
    /// Returns the path to the captured file, or None if not using tmux
    pub fn capture_buffer(&self, terminal_id: &str) -> Option<std::path::PathBuf> {
        // Check for WSL tmux first (Windows only)
        #[cfg(windows)]
        {
            let terminals = self.terminals.lock();
            if let Some(handle) = terminals.get(terminal_id) {
                if matches!(handle.wsl_backend, Some(ResolvedBackend::Tmux)) {
                    let session_name = ResolvedBackend::Tmux.session_name(terminal_id);
                    let output_path = std::env::temp_dir().join(format!(
                        "terminal-{}.txt",
                        &terminal_id[..8.min(terminal_id.len())]
                    ));
                    let distro = handle.wsl_distro.clone();
                    drop(terminals); // Release lock before subprocess call

                    let mut cmd = crate::process::command("wsl.exe");
                    if let Some(d) = &distro {
                        cmd.args(["-d", d]);
                    }
                    cmd.args([
                        "--", "tmux", "capture-pane", "-t", &session_name, "-p", "-S", "-",
                    ]);
                    return match crate::process::safe_output(&mut cmd) {
                        Ok(output) if output.status.success() => {
                            match std::fs::write(&output_path, &output.stdout) {
                                Ok(_) => {
                                    log::debug!("[pty:tmux] Captured WSL terminal buffer | path={:?}", output_path);
                                    Some(output_path)
                                }
                                Err(e) => {
                                    log::error!("[pty:tmux] Failed to write capture file | path={:?} | error: {:#}", output_path, e);
                                    None
                                }
                            }
                        }
                        Ok(output) => {
                            log::error!(
                                "[pty:tmux] WSL tmux capture-pane failed | error: {}",
                                String::from_utf8_lossy(&output.stderr)
                            );
                            None
                        }
                        Err(e) => {
                            log::error!("[pty:tmux] Failed to run WSL tmux capture-pane | error: {:#}", e);
                            None
                        }
                    };
                }
            }
        }

        if !matches!(self.session_backend, ResolvedBackend::Tmux) {
            log::warn!("[pty:tmux] Buffer capture only supported with tmux backend");
            return None;
        }

        let session_name = self.session_backend.session_name(terminal_id);
        let output_path = std::env::temp_dir().join(format!("terminal-{}.txt", &terminal_id[..8.min(terminal_id.len())]));

        // Use tmux capture-pane to get the entire scrollback buffer
        let result = crate::process::safe_output(
            crate::process::command("tmux").args([
                "capture-pane",
                "-t", &session_name,
                "-p",      // output to stdout
                "-S", "-", // start from beginning of scrollback
            ]),
        );

        match result {
            Ok(output) if output.status.success() => {
                match std::fs::write(&output_path, &output.stdout) {
                    Ok(_) => {
                        log::debug!("[pty:tmux] Captured terminal buffer | path={:?}", output_path);
                        Some(output_path)
                    }
                    Err(e) => {
                        log::error!("[pty:tmux] Failed to write capture file | path={:?} | error: {:#}", output_path, e);
                        None
                    }
                }
            }
            Ok(output) => {
                log::error!("[pty:tmux] tmux capture-pane failed | error: {}", String::from_utf8_lossy(&output.stderr));
                None
            }
            Err(e) => {
                log::error!("[pty:tmux] Failed to run tmux capture-pane | error: {:#}", e);
                None
            }
        }
    }

    /// Check if buffer capture is supported (tmux backend)
    pub fn supports_buffer_capture(&self) -> bool {
        matches!(self.session_backend, ResolvedBackend::Tmux)
    }

    /// Clean up a PtyHandle after the process exited naturally (reader got EOF).
    /// Removes the handle from the internal map and joins threads in the background.
    pub fn cleanup_exited(&self, terminal_id: &str) {
        let handle = self.terminals.lock().remove(terminal_id);
        if let Some(handle) = handle {
            // Process already EOF'd — only reap the reader/writer threads. The later
            // `kill()` in the exit-events loop does the session kill (and finds the
            // handle already gone here).
            self.enqueue_teardown(TeardownJob {
                handle: Some(handle),
                kind: TeardownKind::ReapOnly,
            });
        }
    }
}

impl crate::terminal::TerminalTransport for PtyManager {
    fn send_input(&self, terminal_id: &str, data: &[u8]) {
        self.send_input(terminal_id, data)
    }

    fn resize(&self, terminal_id: &str, cols: u16, rows: u16) {
        self.resize(terminal_id, cols, rows)
    }

    fn uses_mouse_backend(&self) -> bool {
        self.uses_mouse_backend()
    }
}

impl Drop for PtyManager {
    fn drop(&mut self) {
        // On drop, just detach - don't kill sessions
        // This allows sessions to persist across app restarts
        self.detach_all();

        // Close the teardown channel so the worker threads drain any buffered jobs
        // and then exit on `Closed`. We intentionally do NOT join the workers here:
        // teardown can block on `lsof`/`tmux kill-session`/`waitpid`, and joining
        // would risk stalling app quit on a hung subprocess. This matches the prior
        // behavior of detached-per-call threads (also never joined). As a result,
        // teardown of already-enqueued jobs is best-effort at quit — the process may
        // exit before slow jobs finish, which is acceptable for graceful detach.
        drop(self.teardown_tx.take());
    }
}

/// Try to retrieve the exit code for a process that has exited.
/// Uses `waitpid` on Unix to get the actual exit status.
fn wait_for_exit_code(pid: u32) -> Option<u32> {
    #[cfg(unix)]
    {
        // The process should have exited by now (reader got EOF).
        // Try a few times with small delays in case it hasn't fully terminated yet.
        for _ in 0..10 {
            let mut status: libc::c_int = 0;
            let result = unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) };
            if result > 0 {
                if libc::WIFEXITED(status) {
                    return Some(libc::WEXITSTATUS(status) as u32);
                }
                // Killed by signal — no exit code
                return None;
            }
            if result < 0 {
                // ECHILD — already reaped by someone else
                return None;
            }
            // result == 0: not exited yet, wait briefly
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        None
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        None
    }
}

/// Find which PIDs have the given Unix sockets open.
///
/// On Linux, reads `/proc/net/unix` to map socket paths to inode numbers,
/// then scans `/proc/*/fd/` to find PIDs holding those inodes.
/// On macOS, uses `libproc` to scan each process's socket fds — no subprocess.
/// On other Unix systems, falls back to a single `lsof` invocation.
#[cfg(unix)]
pub(crate) fn find_pids_for_unix_sockets(
    socket_paths: &[std::path::PathBuf],
) -> HashMap<std::path::PathBuf, Vec<u32>> {
    if socket_paths.is_empty() {
        return HashMap::new();
    }

    #[cfg(target_os = "linux")]
    {
        find_pids_for_unix_sockets_linux(socket_paths)
    }

    #[cfg(target_os = "macos")]
    {
        crate::macos_proc::pids_holding_unix_sockets(socket_paths)
    }

    #[cfg(all(unix, not(target_os = "linux"), not(target_os = "macos")))]
    {
        find_pids_for_unix_sockets_lsof(socket_paths)
    }
}

/// Linux implementation: read `/proc/net/unix` and `/proc/*/fd/` — no subprocess spawning.
#[cfg(target_os = "linux")]
fn find_pids_for_unix_sockets_linux(
    socket_paths: &[std::path::PathBuf],
) -> HashMap<std::path::PathBuf, Vec<u32>> {
    // Step 1: Read /proc/net/unix to find inodes for our socket paths.
    // Format: "Num RefCount Protocol Flags Type St Inode Path"
    let proc_net = match std::fs::read_to_string("/proc/net/unix") {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };

    // Build a set of canonical socket paths for fast lookup
    let canonical_paths: HashMap<std::path::PathBuf, &std::path::PathBuf> = socket_paths
        .iter()
        .filter_map(|p| std::fs::canonicalize(p).ok().map(|c| (c, p)))
        .collect();

    // Map inode -> original socket path
    let mut inode_to_path: HashMap<u64, &std::path::PathBuf> = HashMap::new();
    for line in proc_net.lines().skip(1) {
        // Fields are space-separated; path is the last field (may be absent)
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 8 {
            continue;
        }
        let inode: u64 = match fields[6].parse() {
            Ok(i) => i,
            Err(_) => continue,
        };
        let path_str = fields[7];
        let path = std::path::Path::new(path_str);

        // Check against canonical paths
        if let Some(&orig) = canonical_paths.get(path) {
            inode_to_path.insert(inode, orig);
        } else if let Ok(canon) = std::fs::canonicalize(path)
            && let Some(&orig) = canonical_paths.get(&canon) {
                inode_to_path.insert(inode, orig);
            }
    }

    if inode_to_path.is_empty() {
        return HashMap::new();
    }

    // Step 2: Scan /proc/*/fd/ to find PIDs that hold these inodes.
    let mut result: HashMap<std::path::PathBuf, Vec<u32>> = HashMap::new();

    let proc_dir = match std::fs::read_dir("/proc") {
        Ok(d) => d,
        Err(_) => return HashMap::new(),
    };

    for entry in proc_dir.flatten() {
        let pid: u32 = match entry.file_name().to_str().and_then(|s| s.parse().ok()) {
            Some(p) => p,
            None => continue,
        };

        let fd_dir = entry.path().join("fd");
        let fd_entries = match std::fs::read_dir(&fd_dir) {
            Ok(d) => d,
            Err(_) => continue, // permission denied or process gone
        };

        for fd_entry in fd_entries.flatten() {
            // readlink on /proc/<pid>/fd/<n> gives "socket:[<inode>]"
            let link = match std::fs::read_link(fd_entry.path()) {
                Ok(l) => l,
                Err(_) => continue,
            };
            let link_str = match link.to_str() {
                Some(s) => s,
                None => continue,
            };
            // Parse "socket:[12345]"
            if let Some(inode_str) = link_str
                .strip_prefix("socket:[")
                .and_then(|s| s.strip_suffix(']'))
                && let Ok(inode) = inode_str.parse::<u64>()
                    && let Some(&socket_path) = inode_to_path.get(&inode) {
                        result
                            .entry(socket_path.clone())
                            .or_default()
                            .push(pid);
                    }
                    // Early exit if we found all inodes
                    // (not worth the bookkeeping for a small set)
        }
    }

    result
}

/// Fallback for non-Linux, non-macOS Unix (e.g. BSD): single `lsof` call for
/// all sockets. macOS uses `crate::macos_proc` instead.
#[cfg(all(unix, not(target_os = "linux"), not(target_os = "macos")))]
fn find_pids_for_unix_sockets_lsof(
    socket_paths: &[std::path::PathBuf],
) -> HashMap<std::path::PathBuf, Vec<u32>> {
    // lsof can take multiple file arguments at once
    let mut cmd = crate::process::command("lsof");
    cmd.arg("-t");
    for path in socket_paths {
        cmd.arg(path);
    }

    let output = match crate::process::safe_output(&mut cmd) {
        Ok(o) if o.status.success() => o,
        _ => return HashMap::new(),
    };

    // lsof -t with multiple files just lists PIDs (no file association).
    // We need per-file results, so use full output instead.
    drop(output);

    let mut cmd = crate::process::command("lsof");
    cmd.arg("-F").arg("pn"); // machine-readable: p=PID, n=name fields
    for path in socket_paths {
        cmd.arg(path);
    }

    let output = match crate::process::safe_output(&mut cmd) {
        Ok(o) if o.status.success() => o,
        _ => return HashMap::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut result: HashMap<std::path::PathBuf, Vec<u32>> = HashMap::new();
    let mut current_pid: Option<u32> = None;

    // lsof -F output: lines starting with 'p' = PID, 'n' = name (path)
    for line in stdout.lines() {
        if let Some(pid_str) = line.strip_prefix('p') {
            current_pid = pid_str.parse().ok();
        } else if let Some(name) = line.strip_prefix('n')
            && let Some(pid) = current_pid
        {
            let path = std::path::PathBuf::from(name);
            if socket_paths.contains(&path) {
                result.entry(path).or_default().push(pid);
            }
        }
    }

    result
}

/// Return the first direct child pid of `pid` via `/proc/<pid>/task/<pid>/children`.
/// Used to walk from a dtach daemon down to the actual shell process.
#[cfg(target_os = "linux")]
fn first_proc_child(pid: u32) -> Option<u32> {
    let path = format!("/proc/{}/task/{}/children", pid, pid);
    let contents = std::fs::read_to_string(path).ok()?;
    contents.split_whitespace().next().and_then(|s| s.parse().ok())
}

#[cfg(target_os = "macos")]
fn first_proc_child(pid: u32) -> Option<u32> {
    crate::macos_proc::first_child_pid(pid)
}

#[cfg(all(unix, not(target_os = "linux"), not(target_os = "macos")))]
fn first_proc_child(pid: u32) -> Option<u32> {
    let output = crate::process::safe_output(
        crate::process::command("pgrep").args(["-P", &pid.to_string()]),
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .and_then(|s| s.trim().parse().ok())
}

#[cfg(not(unix))]
fn first_proc_child(_pid: u32) -> Option<u32> {
    None
}

pub fn parse_serial_args(args: &[String]) -> Option<(String, u32, Option<String>)> {
    let mut port = String::new();
    let mut baud_rate = 115200;
    let mut session_id = None;

    let mut i = 0;
    while i < args.len() {
        if (args[i] == "--port" || args[i] == "-p") && i + 1 < args.len() {
            port = args[i + 1].clone();
            i += 2;
        } else if (args[i] == "--baud" || args[i] == "-b") && i + 1 < args.len() {
            baud_rate = args[i + 1].parse().unwrap_or(115200);
            i += 2;
        } else if (args[i] == "--id" || args[i] == "--session-id") && i + 1 < args.len() {
            session_id = Some(args[i + 1].clone());
            i += 2;
        } else if !args[i].starts_with('-') && port.is_empty() {
            port = args[i].clone();
            i += 1;
        } else {
            i += 1;
        }
    }
    if !port.is_empty() || session_id.is_some() {
        Some((port, baud_rate, session_id))
    } else {
        None
    }
}

pub fn parse_telnet_args(args: &[String]) -> Option<(String, u16, Option<String>)> {
    let mut host = String::new();
    let mut port = 23;
    let mut session_id = None;

    let mut i = 0;
    while i < args.len() {
        if (args[i] == "--port" || args[i] == "-p") && i + 1 < args.len() {
            port = args[i + 1].parse().unwrap_or(23);
            i += 2;
        } else if (args[i] == "--id" || args[i] == "--session-id") && i + 1 < args.len() {
            session_id = Some(args[i + 1].clone());
            i += 2;
        } else if !args[i].starts_with('-') && host.is_empty() {
            host = args[i].clone();
            i += 1;
        } else {
            i += 1;
        }
    }
    if !host.is_empty() || session_id.is_some() {
        Some((host, port, session_id))
    } else {
        None
    }
}

pub fn parse_ssh_args(args: &[String]) -> Option<(String, u16, String, Option<String>, Option<String>, Option<String>)> {
    let mut host = String::new();
    let mut port = 22;
    let mut username = String::new();
    let mut key_path = None;
    let mut session_id = None;
    let mut reuse_from = None;

    let mut i = 0;
    while i < args.len() {
        if args[i] == "-p" && i + 1 < args.len() {
            port = args[i + 1].parse().unwrap_or(22);
            i += 2;
        } else if args[i] == "-i" && i + 1 < args.len() {
            key_path = Some(args[i + 1].clone());
            i += 2;
        } else if (args[i] == "--id" || args[i] == "--session-id") && i + 1 < args.len() {
            session_id = Some(args[i + 1].clone());
            i += 2;
        } else if (args[i] == "--reuse-from" || args[i] == "--reuse-terminal") && i + 1 < args.len() {
            reuse_from = Some(args[i + 1].clone());
            i += 2;
        } else if !args[i].starts_with('-') {
            let parts: Vec<&str> = args[i].split('@').collect();
            if parts.len() == 2 {
                username = parts[0].to_string();
                host = parts[1].to_string();
            } else {
                host = args[i].clone();
            }
            i += 1;
        } else {
            i += 1;
        }
    }

    if !host.is_empty() {
        if username.is_empty() {
            if let Ok(user) = std::env::var("USER") {
                username = user;
            }
        }
        return Some((host, port, username, key_path, session_id, reuse_from));
    }
    None
}

pub fn parse_local_args(args: &[String]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        if (args[i] == "--id" || args[i] == "--session-id") && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
        i += 1;
    }
    None
}

pub fn parse_local_shell_arg(args: &[String]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--shell" && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
        i += 1;
    }
    None
}

static TOKIO_RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

pub fn get_tokio_runtime() -> &'static tokio::runtime::Runtime {
    TOKIO_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(8)
            .thread_name("velowork-tokio")
            .thread_stack_size(512 * 1024)
            .enable_all()
            .build()
            .expect("Failed to build Tokio runtime for SSH connections")
    })
}

/// Handle keyboard interactive authentication prompts
async fn handle_keyboard_interactive(
    response: russh::client::KeyboardInteractiveAuthResponse,
    session: &mut russh::client::Handle<SshClient>,
    _username: &str,
    terminal_id: &str,
    input_rx: &mut tokio::sync::mpsc::Receiver<Vec<u8>>,
    event_tx: &async_channel::Sender<PtyEvent>,
    shutdown: &Arc<PtyShutdownState>,
    configured_password: Option<&str>,
    configured_totp_secret: Option<&str>,
) -> Result<bool, anyhow::Error> {
    use russh::client::KeyboardInteractiveAuthResponse;

    let mut current_response = response;

    loop {
        match current_response {
            KeyboardInteractiveAuthResponse::Success => {
                return Ok(true);
            }
            KeyboardInteractiveAuthResponse::Failure { .. } => {
                return Ok(false);
            }
            KeyboardInteractiveAuthResponse::InfoRequest {
                name: _,
                instructions,
                prompts,
            } => {
                // Show instructions if any
                if !instructions.is_empty() {
                    let _ = event_tx.send(PtyEvent::Data {
                        terminal_id: terminal_id.to_string(),
                        data: format!("{}\r\n", instructions).into_bytes(),
                    }).await;
                }

                // Collect responses for each prompt
                let mut responses = Vec::new();
                for prompt in &prompts {
                    // 1. Check if prompt matches TOTP / 2FA verification code
                    if let Some(secret) = configured_totp_secret
                        && crate::totp::is_otp_prompt(&prompt.prompt)
                    {
                        match crate::totp::generate_current_totp(secret) {
                            Ok(code) => {
                                let _ = event_tx.send(PtyEvent::Data {
                                    terminal_id: terminal_id.to_string(),
                                    data: format!("{}\x1b[32m[Auto-filled 2FA OTP: ******]\x1b[0m\r\n", prompt.prompt).into_bytes(),
                                }).await;
                                responses.push(code);
                                continue;
                            }
                            Err(e) => {
                                log::warn!("Failed to generate TOTP code: {}", e);
                            }
                        }
                    }

                    // 2. Check if prompt matches saved password
                    if let Some(pwd) = configured_password
                        && crate::totp::is_password_prompt(&prompt.prompt)
                    {
                        let _ = event_tx.send(PtyEvent::Data {
                            terminal_id: terminal_id.to_string(),
                            data: format!("{}\x1b[32m[Auto-filled Password]\x1b[0m\r\n", prompt.prompt).into_bytes(),
                        }).await;
                        responses.push(pwd.to_string());
                        continue;
                    }

                    // 3. Fallback to interactive terminal prompt
                    let _ = event_tx.send(PtyEvent::Data {
                        terminal_id: terminal_id.to_string(),
                        data: prompt.prompt.as_bytes().to_vec(),
                    }).await;

                    // Read user input from terminal
                    let mut buffer = Vec::new();
                    loop {
                        if shutdown.is_broken() {
                            return Ok(false);
                        }

                        if let Some(bytes) = input_rx.recv().await {
                            if bytes == [3] || bytes == [27] {
                                let _ = event_tx.send(PtyEvent::Data {
                                    terminal_id: terminal_id.to_string(),
                                    data: b"^C\r\nAuthentication cancelled.\r\n".to_vec(),
                                }).await;
                                return Err(anyhow::anyhow!("Authentication cancelled by user."));
                            }

                            if bytes.starts_with(b"\x1b") {
                                continue;
                            }

                            let mut entered = false;
                            for b in bytes {
                                if b == 3 {
                                    let _ = event_tx.send(PtyEvent::Data {
                                        terminal_id: terminal_id.to_string(),
                                        data: b"^C\r\nAuthentication cancelled.\r\n".to_vec(),
                                    }).await;
                                    return Err(anyhow::anyhow!("Authentication cancelled by user."));
                                }
                                if b == b'\r' || b == b'\n' {
                                    entered = true;
                                    break;
                                } else if b == 127 || b == 8 {
                                    if !buffer.is_empty() {
                                        buffer.pop();
                                        if prompt.echo {
                                            let _ = event_tx.send(PtyEvent::Data {
                                                terminal_id: terminal_id.to_string(),
                                                data: b"\x08 \x08".to_vec(),
                                            }).await;
                                        }
                                    }
                                } else {
                                    buffer.push(b);
                                    if prompt.echo {
                                        let _ = event_tx.send(PtyEvent::Data {
                                            terminal_id: terminal_id.to_string(),
                                            data: vec![b],
                                        }).await;
                                    }
                                }
                            }

                            if entered {
                                let response = String::from_utf8_lossy(&buffer).into_owned();
                                responses.push(response);
                                let _ = event_tx.send(PtyEvent::Data {
                                    terminal_id: terminal_id.to_string(),
                                    data: b"\r\n".to_vec(),
                                }).await;
                                break;
                            }
                        } else {
                            return Err(anyhow::anyhow!("Input channel closed"));
                        }
                    }
                }

                // Send responses back to SSH server
                match session.authenticate_keyboard_interactive_respond(responses).await {
                    Ok(next_response) => {
                        current_response = next_response;
                    }
                    Err(e) => {
                        let _ = event_tx.send(PtyEvent::Data {
                            terminal_id: terminal_id.to_string(),
                            data: format!("Keyboard interactive auth error: {}\r\n", e).into_bytes(),
                        }).await;
                        return Ok(false);
                    }
                }
            }
        }
    }
}

async fn perform_interactive_password_step(
    conn: &mut russh::client::Handle<SshClient>,
    username: &str,
    password: &str,
    known_mode: &mut Option<bool>,
) -> Result<bool, russh::Error> {
    match *known_mode {
        Some(true) => match conn.authenticate_password(username, password).await {
            Ok(res) => Ok(res.success()),
            Err(e) => Err(e),
        },
        Some(false) => {
            match conn.authenticate_keyboard_interactive_start(username, None).await {
                Ok(mut response) => loop {
                    match response {
                        russh::client::KeyboardInteractiveAuthResponse::Success => return Ok(true),
                        russh::client::KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
                        russh::client::KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                            let mut responses = Vec::new();
                            for _ in &prompts {
                                responses.push(password.to_string());
                            }
                            match conn.authenticate_keyboard_interactive_respond(responses).await {
                                Ok(next) => response = next,
                                Err(e) => return Err(e),
                            }
                        }
                    }
                },
                Err(e) => Err(e),
            }
        }
        None => {
            match conn.authenticate_password(username, password).await {
                Ok(res) if res.success() => {
                    *known_mode = Some(true);
                    Ok(true)
                }
                Ok(_) => {
                    *known_mode = Some(true);
                    Ok(false)
                }
                Err(_) => {
                    match conn.authenticate_keyboard_interactive_start(username, None).await {
                        Ok(mut response) => {
                            *known_mode = Some(false);
                            loop {
                                match response {
                                    russh::client::KeyboardInteractiveAuthResponse::Success => return Ok(true),
                                    russh::client::KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
                                    russh::client::KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                                        let mut responses = Vec::new();
                                        for _ in &prompts {
                                            responses.push(password.to_string());
                                        }
                                        match conn.authenticate_keyboard_interactive_respond(responses).await {
                                            Ok(next) => response = next,
                                            Err(e) => return Err(e),
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => Err(e),
                    }
                }
            }
        }
    }
}

/// 使用私钥文件执行 SSH 公钥认证（含 RSA 签名 hash 算法协商）。
///
/// 「测试连接」(`ssh_test.rs`) 与「实际连接」(`run_ssh_connection`) 共用此函数，
/// 确保两者采用完全一致的密钥加载与认证行为：
/// - 使用 russh 自带的 `load_secret_key`，兼容 OpenSSH / PKCS#1(RSA) /
///   PKCS#8 / PuTTY PPK 等多种私钥格式；
/// - 对 RSA 密钥通过 server-sig-algs 扩展协商 rsa-sha2-512/256，避免现代
///   OpenSSH(8.8+) 因禁用 ssh-rsa(SHA-1) 而拒绝公钥认证、回退到密码。
///
/// 返回 `Ok(())` 表示认证成功，返回 `Err(String)` 表示密钥加载或认证失败。
pub(crate) async fn authenticate_with_private_key<S>(
    session: &mut russh::client::Handle<S>,
    username: &str,
    key_path: &str,
    passphrase: Option<&str>,
) -> Result<(), String>
where
    S: russh::client::Handler,
{
    // 加载私钥：使用 russh 自带的 load_secret_key（内部走 decode_secret_key），
    // 兼容 OpenSSH / PKCS#1(RSA) / PKCS#8 / PuTTY PPK 等多种格式，
    // 避免 read_openssh_file 仅支持严格 OpenSSH PEM 而把带 CRLF、注释
    // 或某些加密变体的合法私钥误报为 "unexpected PEM type label"。
    let key = match russh::keys::load_secret_key(key_path, passphrase) {
        Ok(k) => k,
        Err(e) => return Err(format!("Invalid key: {}", e)),
    };
    let key = Arc::new(key);
    // 对 RSA 密钥协商签名 hash 算法：hash_alg 为 None 时等价于 ssh-rsa(SHA-1)，
    // 现代 OpenSSH(8.8+) 默认禁用它，会导致公钥认证失败而回退到密码。
    // 通过 server-sig-algs 扩展选出服务器支持的 rsa-sha2-512/256；
    // 非 RSA 密钥（ed25519/ecdsa 等）保持 None 即可。
    let hash_alg = if matches!(key.algorithm(), russh::keys::Algorithm::Rsa { .. }) {
        match session.best_supported_rsa_hash().await {
            Ok(Some(Some(h))) => Some(h),
            Ok(Some(None)) => None,
            // 服务器未通告 server-sig-algs 时，优先尝试 rsa-sha2-512。
            _ => Some(russh::keys::HashAlg::Sha512),
        }
    } else {
        None
    };
    let key_with_hash = russh::keys::PrivateKeyWithHashAlg::new(key, hash_alg);
    match session.authenticate_publickey(username, key_with_hash).await {
        Ok(auth) if auth.success() => Ok(()),
        Ok(_) => Err("Public key authentication failed".to_string()),
        Err(e) => Err(format!("Public key auth error: {}", e)),
    }
}

/// Combined async stream trait for SSH transport (TCP, SOCKS5, HTTP proxy, or Jump channel).
pub trait AsyncReadWrite: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static> AsyncReadWrite for T {}

/// 解析代理字符串（支持 socks5://、http://、https:// 或无协议头格式，支持可选的用户名密码）。
pub fn parse_proxy_url(raw: &str) -> Option<(velowork_state::ProxyType, String, u16, Option<String>, Option<String>)> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }

    let (proxy_type, rest) = if let Some(stripped) = s.strip_prefix("socks5://") {
        (velowork_state::ProxyType::Socks5, stripped)
    } else if let Some(stripped) = s.strip_prefix("http://") {
        (velowork_state::ProxyType::Http, stripped)
    } else if let Some(stripped) = s.strip_prefix("https://") {
        (velowork_state::ProxyType::Http, stripped)
    } else {
        (velowork_state::ProxyType::Http, s)
    };

    let (auth, host_port) = if let Some((u_p, hp)) = rest.split_once('@') {
        (Some(u_p), hp)
    } else {
        (None, rest)
    };

    let (user, pass) = if let Some(u_p) = auth {
        if let Some((u, p)) = u_p.split_once(':') {
            (Some(u.to_string()), Some(p.to_string()))
        } else {
            (Some(u_p.to_string()), None)
        }
    } else {
        (None, None)
    };

    let (host, port) = if let Some((h, p)) = host_port.rsplit_once(':') {
        let p_num = p.trim_end_matches('/').parse::<u16>().ok()?;
        (h.to_string(), p_num)
    } else {
        (host_port.trim_end_matches('/').to_string(), if proxy_type == velowork_state::ProxyType::Socks5 { 1080 } else { 8080 })
    };

    if host.is_empty() {
        return None;
    }

    Some((proxy_type, host, port, user, pass))
}

/// 获取系统环境变量中配置的代理（优先级：ALL_PROXY > HTTPS_PROXY > HTTP_PROXY）。
pub fn detect_system_proxy() -> Option<(velowork_state::ProxyType, String, u16, Option<String>, Option<String>)> {
    let env_vars = ["ALL_PROXY", "all_proxy", "HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"];
    for var in env_vars {
        if let Ok(val) = std::env::var(var) {
            if let Some(parsed) = parse_proxy_url(&val) {
                return Some(parsed);
            }
        }
    }
    None
}

pub async fn connect_socks5_proxy_stream(
    ph: &str,
    pp: u16,
    user: Option<&str>,
    pass: Option<&str>,
    host: &str,
    port: u16,
) -> Result<Box<dyn AsyncReadWrite>, anyhow::Error> {
    let socks_conn = if let (Some(u), Some(p)) = (user, pass) {
        tokio_socks::tcp::Socks5Stream::connect_with_password((ph, pp), (host, port), u, p).await?
    } else {
        tokio_socks::tcp::Socks5Stream::connect((ph, pp), (host, port)).await?
    };
    Ok(Box::new(socks_conn.into_inner()))
}

pub async fn connect_http_proxy_stream(
    ph: &str,
    pp: u16,
    user: Option<&str>,
    pass: Option<&str>,
    host: &str,
    port: u16,
) -> Result<Box<dyn AsyncReadWrite>, anyhow::Error> {
    let mut stream = tokio::net::TcpStream::connect((ph, pp)).await?;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let req_hdr = if let (Some(u), Some(p)) = (user, pass) {
        use base64::Engine;
        let creds = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", u, p));
        format!(
            "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\nProxy-Authorization: Basic {}\r\n\r\n",
            host, port, host, port, creds
        )
    } else {
        format!(
            "CONNECT {}:{} HTTP/1.1\r\nHost: {}:{}\r\n\r\n",
            host, port, host, port
        )
    };

    stream.write_all(req_hdr.as_bytes()).await?;

    let mut resp_buf = [0u8; 512];
    let n = stream.read(&mut resp_buf).await?;
    let resp = String::from_utf8_lossy(&resp_buf[..n]);
    if !resp.contains(" 200 ") && !resp.starts_with("HTTP/1.1 200") && !resp.starts_with("HTTP/1.0 200") {
        return Err(anyhow::anyhow!("HTTP proxy CONNECT failed: {}", resp.lines().next().unwrap_or("")));
    }
    Ok(Box::new(stream))
}

/// Get the TCP/proxy stream for a session. For `ProxyType::Jump`, returns `None`
/// (the caller should handle the jump connection separately via `connect_jump_stream`).
pub async fn connect_tcp_or_proxy_stream(
    host: &str,
    port: u16,
    session_config: Option<&velowork_state::SshSession>,
) -> Result<Box<dyn AsyncReadWrite>, anyhow::Error> {
    use velowork_state::ProxyType;
    let proxy_type = session_config.map(|s| s.proxy_type).unwrap_or(ProxyType::None);

    match proxy_type {
        ProxyType::Socks5 => {
            let ph = session_config
                .and_then(|s| s.proxy_host.as_deref())
                .filter(|h| !h.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("SOCKS5 proxy host not configured"))?;
            let pp = session_config
                .and_then(|s| s.proxy_port)
                .unwrap_or(1080);
            let user = session_config.and_then(|s| s.proxy_username.as_deref());
            let pass = session_config.and_then(|s| s.proxy_password.as_deref());
            connect_socks5_proxy_stream(ph, pp, user, pass, host, port).await
        }
        ProxyType::Http => {
            let ph = session_config
                .and_then(|s| s.proxy_host.as_deref())
                .filter(|h| !h.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("HTTP proxy host not configured"))?;
            let pp = session_config
                .and_then(|s| s.proxy_port)
                .unwrap_or(8080);
            let user = session_config.and_then(|s| s.proxy_username.as_deref());
            let pass = session_config.and_then(|s| s.proxy_password.as_deref());
            connect_http_proxy_stream(ph, pp, user, pass, host, port).await
        }
        // Jump is handled by the caller via connect_jump_stream
        _ => {
            let global = GLOBAL_PROXY.read().clone();
            if global.mode.eq_ignore_ascii_case("http") && !global.host.trim().is_empty() {
                log::debug!("[pty:network] Cascading connection to global HTTP proxy {}:{}", global.host, global.port);
                connect_http_proxy_stream(&global.host, global.port, None, None, host, port).await
            } else if global.mode.eq_ignore_ascii_case("system") {
                if let Some((pt, ph, pp, user, pass)) = detect_system_proxy() {
                    log::debug!("[pty:network] Cascading connection to system proxy {:?} {}:{}", pt, ph, pp);
                    match pt {
                        ProxyType::Socks5 => connect_socks5_proxy_stream(&ph, pp, user.as_deref(), pass.as_deref(), host, port).await,
                        _ => connect_http_proxy_stream(&ph, pp, user.as_deref(), pass.as_deref(), host, port).await,
                    }
                } else {
                    let stream = tokio::net::TcpStream::connect((host, port)).await?;
                    Ok(Box::new(stream))
                }
            } else {
                let stream = tokio::net::TcpStream::connect((host, port)).await?;
                Ok(Box::new(stream))
            }
        }
    }
}

/// 根据 SshSession 构造完整的 russh 客户端配置（统合加密算法白名单、压缩、GEX、Limits 与性能参数）。
pub fn build_russh_client_config(session: &velowork_state::SshSession) -> russh::client::Config {
    let recv_window = if session.recv_window > 0 { session.recv_window } else { 2097152 };
    let max_packets = if session.max_packets > 0 { session.max_packets } else { 32768 };
    let keepalive_secs = session.keep_alive_interval;
    let keepalive_max = if session.keep_alive_max > 0 { session.keep_alive_max as usize } else { 3 };
    let tcp_nodelay = session.tcp_nodelay;
    let channel_buf = if session.channel_buffer_size > 0 { session.channel_buffer_size as usize } else { 100 };

    let inactivity_timeout_opt = if session.idle_disconnect_timeout > 0 {
        Some(std::time::Duration::from_secs(session.idle_disconnect_timeout as u64))
    } else {
        None
    };
    let keepalive_interval_opt = if keepalive_secs > 0 {
        Some(std::time::Duration::from_secs(keepalive_secs as u64))
    } else {
        None
    };

    let mut client_config = russh::client::Config {
        inactivity_timeout: inactivity_timeout_opt,
        window_size: recv_window,
        maximum_packet_size: max_packets,
        keepalive_interval: keepalive_interval_opt,
        keepalive_max,
        nodelay: tcp_nodelay,
        channel_buffer_size: channel_buf,
        ..Default::default()
    };

    if !session.algorithms_automatic {
        if !session.kex_algorithms.is_empty() {
            let mut list = Vec::new();
            for algo in &session.kex_algorithms {
                if let Ok(name) = russh::kex::Name::try_from(algo.as_str()) {
                    list.push(name);
                }
            }
            for marker in &["ext-info-c", "kex-strict-c-v00@openssh.com"] {
                if let Ok(name) = russh::kex::Name::try_from(*marker) {
                    if !list.contains(&name) {
                        list.push(name);
                    }
                }
            }
            client_config.preferred.kex = std::borrow::Cow::Owned(list);
        }
        if !session.cipher_algorithms.is_empty() {
            let mut list = Vec::new();
            for algo in &session.cipher_algorithms {
                if let Ok(name) = russh::cipher::Name::try_from(algo.as_str()) {
                    list.push(name);
                }
            }
            client_config.preferred.cipher = std::borrow::Cow::Owned(list);
        }
        if !session.mac_algorithms.is_empty() {
            let mut list = Vec::new();
            for algo in &session.mac_algorithms {
                if let Ok(name) = russh::mac::Name::try_from(algo.as_str()) {
                    list.push(name);
                }
            }
            client_config.preferred.mac = std::borrow::Cow::Owned(list);
        }
        if !session.hostkey_algorithms.is_empty() {
            let mut list = Vec::new();
            for algo in &session.hostkey_algorithms {
                if let Ok(name) = algo.parse::<russh::keys::Algorithm>() {
                    list.push(name);
                }
            }
            client_config.preferred.key = std::borrow::Cow::Owned(list);
        }

        use velowork_state::CompressionType;
        let mut comp_list = match session.compression {
            CompressionType::None => vec![russh::compression::NONE],
            CompressionType::Zlib => vec![russh::compression::ZLIB],
            CompressionType::ZlibOpenSsh => vec![russh::compression::ZLIB_LEGACY],
        };
        if !comp_list.contains(&russh::compression::NONE) {
            comp_list.push(russh::compression::NONE);
        }
        client_config.preferred.compression = std::borrow::Cow::Owned(comp_list);

        // DH 动态群交换参数（GexParams 要求 min ≥ 2048）
        client_config.gex = russh::client::GexParams::for_client_config(
            session.gex_min as usize,
            session.gex_preferred as usize,
            session.gex_max as usize,
        )
        .unwrap_or_default();

        // 重新密钥时间限制（rekey_time 秒）
        client_config.limits = russh::Limits {
            rekey_time_limit: std::time::Duration::from_secs(session.rekey_time as u64),
            ..Default::default()
        };
    }

    client_config
}

/// Connect to a target host through a bastion (jump) host via SSH.
/// Uses `Box::pin` internally to break the type-level async recursion between
/// this function and `connect_ssh_handle`.
pub fn connect_jump_stream<'a>(
    host: &'a str,
    port: u16,
    jump: &'a velowork_state::SshSession,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Box<dyn AsyncReadWrite>, anyhow::Error>> + Send + 'a>> {
    Box::pin(async move {
        log::info!("[pty:ssh:proxy_jump] Connecting to bastion {}:{}...", jump.host, jump.port);
        let bastion_handle = connect_ssh_handle(jump, None).await?;

        log::info!("[pty:ssh:proxy_jump] Opening direct-tcpip channel | target={}:{} via bastion={}:{}", host, port, jump.host, jump.port);
        let channel = bastion_handle
            .channel_open_direct_tcpip(host, port as u32, "127.0.0.1", 0)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to open direct-tcpip channel: {:?}", e))?;

        Ok(Box::new(channel.into_stream()) as Box<dyn AsyncReadWrite>)
    })
}

pub async fn connect_ssh_handle(
    session: &velowork_state::SshSession,
    jump_session: Option<&velowork_state::SshSession>,
) -> Result<russh::client::Handle<SshClient>, anyhow::Error> {
    let timeout_secs = if session.connection_timeout > 0 { session.connection_timeout } else { 30 };
    let client_config = build_russh_client_config(session);

    let tcp_stream: Box<dyn AsyncReadWrite> = if session.proxy_type == velowork_state::ProxyType::Jump {
        let jump = jump_session
            .ok_or_else(|| anyhow::anyhow!("Jump host session not configured for '{}'", session.name))?;
        connect_jump_stream(&session.host, session.port, jump).await?
    } else {
        connect_tcp_or_proxy_stream(&session.host, session.port, Some(session)).await?
    };
    let config = std::sync::Arc::new(client_config);
    let x11_forwarder = if session.enable_x11_forwarding {
        Some(std::sync::Arc::new(crate::x11::X11Forwarder::new(session.x11_display.as_deref())))
    } else {
        None
    };
    let sh = SshClient {
        strict_host_key: session.strict_host_key,
        host: session.host.clone(),
        port: session.port,
        x11_forwarder,
        agent_forwarding: session.enable_agent_forwarding,
        agent_socket_path: match &session.auth_type {
            velowork_state::SshAuthType::SshAgent { socket_path } => socket_path.clone(),
            _ => None,
        },
    };

    let mut conn = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs as u64),
        russh::client::connect_stream(config, tcp_stream, sh),
    )
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "Connection to {}:{} timed out after {}s",
            session.host,
            session.port,
            timeout_secs
        )
    })??;

    let mut authenticated = false;
    match &session.auth_type {
        velowork_state::SshAuthType::PrivateKey { key_path, passphrase } => {
            if !key_path.is_empty() {
                if authenticate_with_private_key(&mut conn, &session.username, key_path.as_str(), passphrase.as_deref())
                    .await
                    .is_ok()
                {
                    authenticated = true;
                }
            }
        }
        velowork_state::SshAuthType::Password { password } => {
            if let Some(pwd) = password {
                if let Ok(auth_res) = conn.authenticate_password(&session.username, pwd).await {
                    if auth_res.success() {
                        authenticated = true;
                    }
                }
                if !authenticated {
                    if let Ok(mut response) = conn
                        .authenticate_keyboard_interactive_start(&session.username, None)
                        .await
                    {
                        loop {
                            match response {
                                russh::client::KeyboardInteractiveAuthResponse::Success => {
                                    authenticated = true;
                                    break;
                                }
                                russh::client::KeyboardInteractiveAuthResponse::Failure { .. } => {
                                    break;
                                }
                                russh::client::KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                                    let mut answers = Vec::new();
                                    for _ in &prompts {
                                        answers.push(pwd.to_string());
                                    }
                                    if let Ok(next) = conn.authenticate_keyboard_interactive_respond(answers).await {
                                        response = next;
                                    } else {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        velowork_state::SshAuthType::SshAgent { socket_path } => {
            if let Ok(true) = crate::ssh_agent::authenticate_with_agent(
                &mut conn,
                &session.username,
                socket_path.as_deref(),
            )
            .await
            {
                authenticated = true;
            }
        }
        _ => {}
    }

    if !authenticated {
        // Fallback default ssh key locations
        if let Ok(home) = std::env::var("HOME") {
            let home_path = std::path::Path::new(&home);
            for default_key in &["id_ed25519", "id_rsa", "id_ecdsa", "id_dsa"] {
                let path = home_path.join(".ssh").join(default_key);
                if path.exists() {
                    if let Some(path_str) = path.to_str() {
                        if authenticate_with_private_key(&mut conn, &session.username, path_str, None)
                            .await
                            .is_ok()
                        {
                            authenticated = true;
                            break;
                        }
                    }
                }
            }
        }
    }

    if !authenticated
        && (matches!(session.auth_type, velowork_state::SshAuthType::KeyboardInteractive)
            || session.totp_secret.is_some())
    {
        if let Ok(mut response) = conn
            .authenticate_keyboard_interactive_start(&session.username, None)
            .await
        {
            let pwd = match &session.auth_type {
                velowork_state::SshAuthType::Password { password: Some(p) } => Some(p.as_str()),
                _ => None,
            };
            let totp = session.totp_secret.as_deref();

            loop {
                match response {
                    russh::client::KeyboardInteractiveAuthResponse::Success => {
                        authenticated = true;
                        break;
                    }
                    russh::client::KeyboardInteractiveAuthResponse::Failure { .. } => {
                        break;
                    }
                    russh::client::KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                        let mut answers = Vec::new();
                        for prompt in &prompts {
                            if let Some(t) = totp && crate::totp::is_otp_prompt(&prompt.prompt) {
                                if let Ok(code) = crate::totp::generate_current_totp(t) {
                                    answers.push(code);
                                    continue;
                                }
                            }
                            if let Some(p) = pwd && crate::totp::is_password_prompt(&prompt.prompt) {
                                answers.push(p.to_string());
                                continue;
                            }
                            answers.push(String::new());
                        }
                        if let Ok(next) = conn.authenticate_keyboard_interactive_respond(answers).await {
                            response = next;
                        } else {
                            break;
                        }
                    }
                }
            }
        }
    }

    if !authenticated {
        return Err(anyhow::anyhow!("SSH authentication failed for session '{}'", session.name));
    }

    Ok(conn)
}

#[allow(clippy::too_many_arguments)]
async fn run_ssh_connection(
    terminal_id: String,
    host: String,
    port: u16,
    username: String,
    key_path: Option<String>,
    session_config: Option<velowork_state::SshSession>,
    jump_session: Option<velowork_state::SshSession>,
    existing_session: Option<Arc<russh::client::Handle<SshClient>>>,
    input_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    event_tx: async_channel::Sender<PtyEvent>,
    shutdown: Arc<PtyShutdownState>,
    terminals: Arc<parking_lot::Mutex<std::collections::HashMap<String, PtyHandle>>>,
    default_term_type: String,
) -> Result<(), anyhow::Error> {
    let username = if username.trim().is_empty() {
        std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "root".to_string())
    } else {
        username
    };

    // Convert std mpsc to tokio mpsc for async compatibility
    let (tokio_input_tx, mut tokio_input_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(100);
    let _ = std::thread::Builder::new()
        .name("ssh-input-bridge".into())
        .stack_size(128 * 1024)
        .spawn(move || {
            while let Ok(bytes) = input_rx.recv() {
                if tokio_input_tx.blocking_send(bytes).is_err() {
                    break;
                }
            }
        });

    let session = if let Some(existing) = existing_session {
        if !existing.is_closed() {
            let _ = event_tx.send(PtyEvent::Data {
                terminal_id: terminal_id.clone(),
                data: format!("Reusing SSH connection to {}:{}...\r\n", host, port).into_bytes(),
            }).await;
            existing
        } else {
            let _ = event_tx.send(PtyEvent::Data {
                terminal_id: terminal_id.clone(),
                data: format!("Connecting to {}:{}...\r\n", host, port).into_bytes(),
            }).await;
            let client_config = if let Some(ref sc) = session_config {
                build_russh_client_config(sc)
            } else {
                russh::client::Config::default()
            };

            let strict_host_key = session_config.as_ref()
                .map(|s| s.strict_host_key)
                .unwrap_or_default();

            let tcp_stream: Box<dyn AsyncReadWrite> = if session_config.as_ref().map(|s| s.proxy_type) == Some(velowork_state::ProxyType::Jump) {
                let jump = jump_session.as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Jump host session not configured"))?;
                connect_jump_stream(&host, port, jump).await?
            } else {
                connect_tcp_or_proxy_stream(&host, port, session_config.as_ref()).await?
            };
            let config = std::sync::Arc::new(client_config);
            let x11_forwarder = if session_config.as_ref().map(|s| s.enable_x11_forwarding).unwrap_or(false) {
                Some(std::sync::Arc::new(crate::x11::X11Forwarder::new(session_config.as_ref().and_then(|s| s.x11_display.as_deref()))))
            } else {
                None
            };
            let sh = SshClient {
                strict_host_key,
                host: host.to_string(),
                port,
                x11_forwarder,
                agent_forwarding: session_config.as_ref().map(|s| s.enable_agent_forwarding).unwrap_or(false),
                agent_socket_path: session_config.as_ref().and_then(|s| match &s.auth_type {
                    velowork_state::SshAuthType::SshAgent { socket_path } => socket_path.clone(),
                    _ => None,
                }),
            };
            let timeout_secs = session_config.as_ref().map(|s| s.connection_timeout).filter(|&t| t > 0).unwrap_or(30);
            let mut conn = tokio::time::timeout(
                std::time::Duration::from_secs(timeout_secs as u64),
                russh::client::connect_stream(config, tcp_stream, sh),
            )
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "Connection to {}:{} timed out after {}s",
                    host,
                    port,
                    timeout_secs
                )
            })??;

            let mut authenticated = false;

            if let Some(ref sc) = session_config
                && let velowork_state::SshAuthType::SshAgent { ref socket_path } = sc.auth_type
            {
                match crate::ssh_agent::authenticate_with_agent(&mut conn, &username, socket_path.as_deref()).await {
                    Ok(true) => {
                        authenticated = true;
                    }
                    Ok(false) => {
                        let _ = event_tx.send(PtyEvent::Data {
                            terminal_id: terminal_id.clone(),
                            data: b"SSH Agent authentication failed (no keys accepted by server or agent is empty).\r\n".to_vec(),
                        }).await;
                    }
                    Err(err) => {
                        let msg = format!("SSH Agent error: {}\r\n", err);
                        let _ = event_tx.send(PtyEvent::Data {
                            terminal_id: terminal_id.clone(),
                            data: msg.into_bytes(),
                        }).await;
                    }
                }
            }

            let mut keys_to_try = Vec::new();
            if let Some(ref kp) = key_path {
                if !kp.is_empty() {
                    keys_to_try.push(kp.clone());
                }
            } else {
                if let Ok(home) = std::env::var("HOME") {
                    let home_path = std::path::Path::new(&home);
                    for default_key in &["id_ed25519", "id_rsa", "id_ecdsa", "id_dsa"] {
                        let path = home_path.join(".ssh").join(default_key);
                        if path.exists() {
                            if let Some(path_str) = path.to_str() {
                                keys_to_try.push(path_str.to_string());
                            }
                        }
                    }
                }
            }

            let passphrase = if let Some(ref sc) = session_config
                && let velowork_state::SshAuthType::PrivateKey { passphrase: Some(ref pp), .. } = sc.auth_type
            {
                Some(pp.as_str())
            } else {
                None
            };

            for kp in keys_to_try {
                if authenticate_with_private_key(&mut conn, &username, &kp, passphrase)
                    .await
                    .is_ok()
                {
                    authenticated = true;
                    break;
                }
            }

            if !authenticated {
                if let Some(ref sc) = session_config
                    && let velowork_state::SshAuthType::Password { password: Some(ref pwd) } = sc.auth_type
                {
                    if let Ok(auth_res) = conn.authenticate_password(&username, pwd).await {
                        if auth_res.success() {
                            authenticated = true;
                        }
                    }
                }
            }

            let configured_pwd = if let Some(ref sc) = session_config {
                match &sc.auth_type {
                    velowork_state::SshAuthType::Password { password: Some(p) } => Some(p.as_str()),
                    _ => None,
                }
            } else {
                None
            };
            let configured_totp = session_config.as_ref().and_then(|sc| sc.totp_secret.as_deref());

            if !authenticated && (session_config.as_ref().map(|sc| matches!(sc.auth_type, velowork_state::SshAuthType::KeyboardInteractive) || sc.totp_secret.is_some()).unwrap_or(false)) {
                let _ = event_tx.send(PtyEvent::Data {
                    terminal_id: terminal_id.clone(),
                    data: b"Keyboard interactive authentication...\r\n".to_vec(),
                }).await;

                match conn.authenticate_keyboard_interactive_start(&username, None).await {
                    Ok(response) => {
                        authenticated = handle_keyboard_interactive(
                            response,
                            &mut conn,
                            &username,
                            &terminal_id,
                            &mut tokio_input_rx,
                            &event_tx,
                            &shutdown,
                            configured_pwd,
                            configured_totp,
                        ).await?;
                    }
                    Err(e) => {
                        log::debug!("Keyboard interactive auth start: {}", e);
                    }
                }
            }

            if !authenticated {
                let prompt_text = format!("{}@{}'s password: ", username, host);
                let _ = event_tx.send(PtyEvent::Data {
                    terminal_id: terminal_id.clone(),
                    data: prompt_text.into_bytes(),
                }).await;

                const MAX_AUTH_ATTEMPTS: usize = 3;
                let mut attempts = 0;
                let mut auth_mode = None;
                let mut password_buffer = Vec::new();
                loop {
                    if shutdown.is_broken() {
                        return Ok(());
                    }

                    if let Some(bytes) = tokio_input_rx.recv().await {
                        if bytes == [3] || bytes == [27] {
                            let _ = event_tx.send(PtyEvent::Data {
                                terminal_id: terminal_id.clone(),
                                data: b"^C\r\nAuthentication cancelled.\r\n".to_vec(),
                            }).await;
                            return Err(anyhow::anyhow!("Authentication cancelled by user."));
                        }

                        if bytes.starts_with(b"\x1b") {
                            continue;
                        }

                        let mut password_entered = false;
                        for b in bytes {
                            if b == 3 {
                                let _ = event_tx.send(PtyEvent::Data {
                                    terminal_id: terminal_id.clone(),
                                    data: b"^C\r\nAuthentication cancelled.\r\n".to_vec(),
                                }).await;
                                return Err(anyhow::anyhow!("Authentication cancelled by user."));
                            }
                            if b == b'\r' || b == b'\n' {
                                password_entered = true;
                                break;
                            } else if b == 127 || b == 8 {
                                password_buffer.pop();
                            } else {
                                password_buffer.push(b);
                            }
                        }

                        if password_entered {
                            attempts += 1;
                            let password = String::from_utf8_lossy(&password_buffer).into_owned();
                            password_buffer.clear();
                            
                            let _ = event_tx.send(PtyEvent::Data {
                                terminal_id: terminal_id.clone(),
                                data: b"\r\n".to_vec(),
                            }).await;

                            match perform_interactive_password_step(&mut conn, &username, &password, &mut auth_mode).await {
                                Ok(true) => {
                                    authenticated = true;
                                    break;
                                }
                                Ok(false) => {
                                    if attempts >= MAX_AUTH_ATTEMPTS {
                                        let final_msg = "Permission denied (publickey,password).\r\n";
                                        let _ = event_tx.send(PtyEvent::Data {
                                            terminal_id: terminal_id.clone(),
                                            data: final_msg.as_bytes().to_vec(),
                                        }).await;
                                        return Err(anyhow::anyhow!("Permission denied (publickey,password)."));
                                    }
                                    let denied_msg = format!("Permission denied, please try again.\r\n{}@{}'s password: ", username, host);
                                    let _ = event_tx.send(PtyEvent::Data {
                                        terminal_id: terminal_id.clone(),
                                        data: denied_msg.into_bytes(),
                                    }).await;
                                }
                                Err(e) => {
                                    let err_msg = format!("Connection closed by remote host: {}\r\n", e);
                                    let _ = event_tx.send(PtyEvent::Data {
                                        terminal_id: terminal_id.clone(),
                                        data: err_msg.into_bytes(),
                                    }).await;
                                    return Err(anyhow::anyhow!("SSH authentication aborted: {}", e));
                                }
                            }
                        }
                    } else {
                        return Err(anyhow::anyhow!("Input channel closed"));
                    }
                }
            }

            if !authenticated {
                return Err(anyhow::anyhow!("Authentication failed"));
            }

            Arc::new(conn)
        }
    } else {
        let _ = event_tx.send(PtyEvent::Data {
            terminal_id: terminal_id.clone(),
            data: format!("Connecting to {}:{}...\r\n", host, port).into_bytes(),
        }).await;
        let timeout_secs = session_config.as_ref().map(|s| s.connection_timeout).filter(|&t| t > 0).unwrap_or(30);
        let recv_window = session_config.as_ref().map(|s| s.recv_window).filter(|&w| w > 0).unwrap_or(2097152);
        let max_packets = session_config.as_ref().map(|s| s.max_packets).filter(|&p| p > 0).unwrap_or(32768);
        let keepalive_secs = session_config.as_ref().map(|s| s.keep_alive_interval).unwrap_or(60);
        let keepalive_max = session_config.as_ref().map(|s| s.keep_alive_max as usize).filter(|&m| m > 0).unwrap_or(3);
        let tcp_nodelay = session_config.as_ref().map(|s| s.tcp_nodelay).unwrap_or(true);
        let channel_buf = session_config.as_ref().map(|s| s.channel_buffer_size as usize).filter(|&b| b > 0).unwrap_or(100);

        // 空闲断开超时仅在有保活时启用：取 keepalive 周期 × (max+2) 作为
        // 安全下界，远大于保活发包间隔，避免 keepalive 还没发出就被误断。
        // 关闭保活（间隔为 0）时不设空闲超时（None），永不因空闲断开——
        // connection_timeout 只用于握手阶段，不再作为此处兜底。
        // 空闲断开超时直接来自独立配置项 idle_disconnect_timeout，
        // 不再由 keep_alive_interval / keep_alive_max 推算。0 = 关闭。
        let idle_disconnect_secs = session_config
            .as_ref()
            .map(|s| s.idle_disconnect_timeout)
            .unwrap_or(0);
        let inactivity_timeout_opt = if idle_disconnect_secs > 0 {
            Some(std::time::Duration::from_secs(idle_disconnect_secs as u64))
        } else {
            None
        };
        let keepalive_interval_opt = if keepalive_secs > 0 {
            Some(std::time::Duration::from_secs(keepalive_secs as u64))
        } else {
            None
        };

        let mut client_config = russh::client::Config {
            inactivity_timeout: inactivity_timeout_opt,
            window_size: recv_window,
            maximum_packet_size: max_packets,
            keepalive_interval: keepalive_interval_opt,
            keepalive_max,
            nodelay: tcp_nodelay,
            channel_buffer_size: channel_buf,
            ..Default::default()
        };

        if let Some(ref sc) = session_config {
            if !sc.algorithms_automatic {
                if !sc.kex_algorithms.is_empty() {
                    let mut list = Vec::new();
                    for algo in &sc.kex_algorithms {
                        if let Ok(name) = russh::kex::Name::try_from(algo.as_str()) {
                            list.push(name);
                        }
                    }
                    for marker in &["ext-info-c", "kex-strict-c-v00@openssh.com"] {
                        if let Ok(name) = russh::kex::Name::try_from(*marker) {
                            if !list.contains(&name) {
                                list.push(name);
                            }
                        }
                    }
                    client_config.preferred.kex = std::borrow::Cow::Owned(list);
                }
                if !sc.cipher_algorithms.is_empty() {
                    let mut list = Vec::new();
                    for algo in &sc.cipher_algorithms {
                        if let Ok(name) = russh::cipher::Name::try_from(algo.as_str()) {
                            list.push(name);
                        }
                    }
                    client_config.preferred.cipher = std::borrow::Cow::Owned(list);
                }
                if !sc.mac_algorithms.is_empty() {
                    let mut list = Vec::new();
                    for algo in &sc.mac_algorithms {
                        if let Ok(name) = russh::mac::Name::try_from(algo.as_str()) {
                            list.push(name);
                        }
                    }
                    client_config.preferred.mac = std::borrow::Cow::Owned(list);
                }
                if !sc.hostkey_algorithms.is_empty() {
                    let mut list = Vec::new();
                    for algo in &sc.hostkey_algorithms {
                        if let Ok(name) = algo.parse::<russh::keys::Algorithm>() {
                            list.push(name);
                        }
                    }
                    client_config.preferred.key = std::borrow::Cow::Owned(list);
                }

                use velowork_state::CompressionType;
                let mut comp_list = match sc.compression {
                    CompressionType::None => vec![russh::compression::NONE],
                    CompressionType::Zlib => vec![russh::compression::ZLIB],
                    CompressionType::ZlibOpenSsh => vec![russh::compression::ZLIB_LEGACY],
                };
                if !comp_list.contains(&russh::compression::NONE) {
                    comp_list.push(russh::compression::NONE);
                }
                client_config.preferred.compression = std::borrow::Cow::Owned(comp_list);

                // DH 动态群交换参数（GexParams 要求 min ≥ 2048）
                client_config.gex = russh::client::GexParams::for_client_config(
                    sc.gex_min as usize,
                    sc.gex_preferred as usize,
                    sc.gex_max as usize,
                )
                .unwrap_or_default();

                // 重新密钥时间限制（rekey_time 秒）
                client_config.limits = russh::Limits {
                    rekey_time_limit: std::time::Duration::from_secs(sc.rekey_time as u64),
                    ..Default::default()
                };
            }
        }

        let strict_host_key = session_config.as_ref()
            .map(|s| s.strict_host_key)
            .unwrap_or_default();

        let tcp_stream: Box<dyn AsyncReadWrite> = if session_config.as_ref().map(|s| s.proxy_type) == Some(velowork_state::ProxyType::Jump) {
            let jump = jump_session.as_ref()
                .ok_or_else(|| anyhow::anyhow!("Jump host session not configured"))?;
            connect_jump_stream(&host, port, jump).await?
        } else {
            connect_tcp_or_proxy_stream(&host, port, session_config.as_ref()).await?
        };
        let config = std::sync::Arc::new(client_config);
        let x11_forwarder = if session_config.as_ref().map(|s| s.enable_x11_forwarding).unwrap_or(false) {
            Some(std::sync::Arc::new(crate::x11::X11Forwarder::new(session_config.as_ref().and_then(|s| s.x11_display.as_deref()))))
        } else {
            None
        };
        let sh = SshClient {
            strict_host_key,
            host: host.to_string(),
            port,
            x11_forwarder: x11_forwarder.clone(),
            agent_forwarding: session_config.as_ref().map(|s| s.enable_agent_forwarding).unwrap_or(false),
            agent_socket_path: session_config.as_ref().and_then(|s| match &s.auth_type {
                velowork_state::SshAuthType::SshAgent { socket_path } => socket_path.clone(),
                _ => None,
            }),
        };
        // 用连接超时包裹整个握手流程，否则 connection_timeout 不会真正生效
        // （russh 的 connect 自身不限制握手耗时）。超时返回明确的错误信息。
        let mut conn = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs as u64),
            russh::client::connect_stream(config, tcp_stream, sh),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Connection to {}:{} timed out after {}s",
                host,
                port,
                timeout_secs
            )
        })??;

        let mut authenticated = false;

        if let Some(ref sc) = session_config
            && let velowork_state::SshAuthType::SshAgent { ref socket_path } = sc.auth_type
        {
            match crate::ssh_agent::authenticate_with_agent(&mut conn, &username, socket_path.as_deref()).await {
                Ok(true) => {
                    authenticated = true;
                }
                Ok(false) => {
                    let _ = event_tx.send(PtyEvent::Data {
                        terminal_id: terminal_id.clone(),
                        data: b"SSH Agent authentication failed (no keys accepted by server or agent is empty).\r\n".to_vec(),
                    }).await;
                }
                Err(err) => {
                    let msg = format!("SSH Agent error: {}\r\n", err);
                    let _ = event_tx.send(PtyEvent::Data {
                        terminal_id: terminal_id.clone(),
                        data: msg.into_bytes(),
                    }).await;
                }
            }
        }

        let mut keys_to_try = Vec::new();
        if let Some(ref kp) = key_path {
            if !kp.is_empty() {
                keys_to_try.push(kp.clone());
            }
        } else {
            if let Ok(home) = std::env::var("HOME") {
                let home_path = std::path::Path::new(&home);
                for default_key in &["id_ed25519", "id_rsa", "id_ecdsa", "id_dsa"] {
                    let path = home_path.join(".ssh").join(default_key);
                    if path.exists() {
                        if let Some(path_str) = path.to_str() {
                            keys_to_try.push(path_str.to_string());
                        }
                    }
                }
            }
        }

        let passphrase = if let Some(ref sc) = session_config
            && let velowork_state::SshAuthType::PrivateKey { passphrase: Some(ref pp), .. } = sc.auth_type
        {
            Some(pp.as_str())
        } else {
            None
        };

        for kp in keys_to_try {
            if authenticate_with_private_key(&mut conn, &username, &kp, passphrase)
                .await
                .is_ok()
            {
                authenticated = true;
                break;
            }
        }

        if !authenticated {
            if let Some(ref sc) = session_config
                && let velowork_state::SshAuthType::Password { password: Some(ref pwd) } = sc.auth_type
            {
                if let Ok(auth_res) = conn.authenticate_password(&username, pwd).await {
                    if auth_res.success() {
                        authenticated = true;
                    }
                }
            }
        }

        let configured_pwd = if let Some(ref sc) = session_config {
            match &sc.auth_type {
                velowork_state::SshAuthType::Password { password: Some(p) } => Some(p.as_str()),
                _ => None,
            }
        } else {
            None
        };
        let configured_totp = session_config.as_ref().and_then(|sc| sc.totp_secret.as_deref());

        if !authenticated && (session_config.as_ref().map(|sc| matches!(sc.auth_type, velowork_state::SshAuthType::KeyboardInteractive) || sc.totp_secret.is_some()).unwrap_or(false)) {
            let _ = event_tx.send(PtyEvent::Data {
                terminal_id: terminal_id.clone(),
                data: b"Keyboard interactive authentication...\r\n".to_vec(),
            }).await;

            match conn.authenticate_keyboard_interactive_start(&username, None).await {
                Ok(response) => {
                    authenticated = handle_keyboard_interactive(
                        response,
                        &mut conn,
                        &username,
                        &terminal_id,
                        &mut tokio_input_rx,
                        &event_tx,
                        &shutdown,
                        configured_pwd,
                        configured_totp,
                    ).await?;
                }
                Err(e) => {
                    log::debug!("Keyboard interactive auth start: {}", e);
                }
            }
        }

        if !authenticated {
            let prompt_text = format!("{}@{}'s password: ", username, host);
            let _ = event_tx.send(PtyEvent::Data {
                terminal_id: terminal_id.clone(),
                data: prompt_text.into_bytes(),
            }).await;

            const MAX_AUTH_ATTEMPTS: usize = 3;
            let mut attempts = 0;
            let mut auth_mode = None;
            let mut password_buffer = Vec::new();
            loop {
                if shutdown.is_broken() {
                    return Ok(());
                }

                if let Some(bytes) = tokio_input_rx.recv().await {
                    if bytes == [3] || bytes == [27] {
                        let _ = event_tx.send(PtyEvent::Data {
                            terminal_id: terminal_id.clone(),
                            data: b"^C\r\nAuthentication cancelled.\r\n".to_vec(),
                        }).await;
                        return Err(anyhow::anyhow!("Authentication cancelled by user."));
                    }

                    if bytes.starts_with(b"\x1b") {
                        continue;
                    }

                    let mut password_entered = false;
                    for b in bytes {
                        if b == 3 {
                            let _ = event_tx.send(PtyEvent::Data {
                                terminal_id: terminal_id.clone(),
                                data: b"^C\r\nAuthentication cancelled.\r\n".to_vec(),
                            }).await;
                            return Err(anyhow::anyhow!("Authentication cancelled by user."));
                        }
                        if b == b'\r' || b == b'\n' {
                            password_entered = true;
                            break;
                        } else if b == 127 || b == 8 {
                            password_buffer.pop();
                        } else {
                            password_buffer.push(b);
                        }
                    }

                    if password_entered {
                        attempts += 1;
                        let password = String::from_utf8_lossy(&password_buffer).into_owned();
                        password_buffer.clear();
                        
                        let _ = event_tx.send(PtyEvent::Data {
                            terminal_id: terminal_id.clone(),
                            data: b"\r\n".to_vec(),
                        }).await;

                        match perform_interactive_password_step(&mut conn, &username, &password, &mut auth_mode).await {
                            Ok(true) => {
                                authenticated = true;
                                break;
                            }
                            Ok(false) => {
                                if attempts >= MAX_AUTH_ATTEMPTS {
                                    let final_msg = "Permission denied (publickey,password).\r\n";
                                    let _ = event_tx.send(PtyEvent::Data {
                                        terminal_id: terminal_id.clone(),
                                        data: final_msg.as_bytes().to_vec(),
                                    }).await;
                                    return Err(anyhow::anyhow!("Permission denied (publickey,password)."));
                                }
                                let denied_msg = format!("Permission denied, please try again.\r\n{}@{}'s password: ", username, host);
                                let _ = event_tx.send(PtyEvent::Data {
                                    terminal_id: terminal_id.clone(),
                                    data: denied_msg.into_bytes(),
                                }).await;
                            }
                            Err(e) => {
                                let err_msg = format!("Connection closed by remote host: {}\r\n", e);
                                let _ = event_tx.send(PtyEvent::Data {
                                    terminal_id: terminal_id.clone(),
                                    data: err_msg.into_bytes(),
                                }).await;
                                return Err(anyhow::anyhow!("SSH authentication aborted: {}", e));
                            }
                        }
                    }
                } else {
                    return Err(anyhow::anyhow!("Input channel closed"));
                }
            }
        }

        if !authenticated {
            return Err(anyhow::anyhow!("Authentication failed"));
        }

        Arc::new(conn)
    };

    let _ = event_tx.send(PtyEvent::Data {
        terminal_id: terminal_id.clone(),
        data: b"Connection established. Spawning shell...\r\n".to_vec(),
    }).await;

    // Open terminal PTY session channel
    let mut channel = session.channel_open_session().await?;
    let term_type = session_config
        .as_ref()
        .and_then(|s| s.terminal.term_type.clone())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(default_term_type);

    let (initial_cols, initial_rows) = {
        let lock = terminals.lock();
        lock.get(&terminal_id)
            .and_then(|h| h.last_size)
            .unwrap_or((80, 24))
    };
    log::debug!(
        "[pty:size] ssh request_pty | terminal_id={} cols={} rows={}",
        terminal_id,
        initial_cols,
        initial_rows
    );

    channel.request_pty(
        true,
        &term_type,
        initial_cols as u32,
        initial_rows as u32,
        0,
        0,
        &[],
    ).await?;

    // Request X11 forwarding if enabled
    if let Some(ref sc) = session_config
        && sc.enable_x11_forwarding
    {
        let fwd = crate::x11::X11Forwarder::new(sc.x11_display.as_deref());
        match channel
            .request_x11(
                true,
                false,
                &fwd.auth_protocol,
                &fwd.fake_cookie_hex,
                fwd.display_info.screen_number,
            )
            .await
        {
            Ok(_) => {
                log::info!("[pty:x11] X11 forwarding requested successfully | display={}", fwd.display_info.display_str);
            }
            Err(e) => {
                log::warn!("[pty:x11] X11 forwarding request rejected by remote server | error: {:#}", e);
                let _ = event_tx
                    .send(PtyEvent::Data {
                        terminal_id: terminal_id.clone(),
                        data: format!("\x1b[33m[Warning: Remote server rejected X11 forwarding request: {}]\x1b[0m\r\n", e).into_bytes(),
                    })
                    .await;
            }
        }
    }

    // Request SSH Agent forwarding if enabled
    if let Some(ref sc) = session_config
        && sc.enable_agent_forwarding
    {
        match channel.agent_forward(false).await {
            Ok(_) => {
                log::info!("[pty:ssh_agent] SSH Agent forwarding requested successfully");
            }
            Err(e) => {
                log::warn!("[pty:ssh_agent] SSH Agent forwarding request rejected by remote server | error: {:#}", e);
                let _ = event_tx
                    .send(PtyEvent::Data {
                        terminal_id: terminal_id.clone(),
                        data: format!("\x1b[33m[Warning: Remote server rejected SSH Agent forwarding request: {}]\x1b[0m\r\n", e).into_bytes(),
                    })
                    .await;
            }
        }
    }

    // Re-sync the window size *before* the remote shell exists.
    //
    // The size handed to `request_pty` above was read some milliseconds (or
    // hundreds of them, while authenticating) earlier; meanwhile the UI may
    // have measured the pane's real geometry. If we let that correction arrive
    // after the shell started, the shell repaints an already drawn prompt — the
    // visible "flash". Applying it here means the shell is born at the correct
    // size and only ever paints once.
    let mut applied_remote_size = (initial_cols, initial_rows);
    if let Some((cols, rows)) = {
        let lock = terminals.lock();
        lock.get(&terminal_id).and_then(|h| h.last_size)
    } {
        if (cols, rows) != (initial_cols, initial_rows) {
            log::debug!(
                "[pty:size] ssh sync size before request_shell | terminal_id={} from={}x{} to={}x{}",
                terminal_id,
                initial_cols,
                initial_rows,
                cols,
                rows
            );
            match channel.window_change(cols as u32, rows as u32, 0, 0).await {
                Ok(_) => applied_remote_size = (cols, rows),
                Err(e) => log::warn!(
                    "[pty:size] ssh pre-shell window_change failed | terminal_id={} | error: {:#}",
                    terminal_id,
                    e
                ),
            }
        }
    }

    channel.request_shell(true).await?;

    // Execute startup command lines if configured
    if let Some(ref sc) = session_config
        && let Some(ref cmd) = sc.startup_command
    {
        for line in cmd.lines() {
            let line = line.trim();
            if !line.is_empty() {
                let _ = channel.data(format!("{}\r\n", line).as_bytes()).await;
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            }
        }
    }

    let (resize_tx, mut resize_rx) = tokio::sync::mpsc::channel::<(u16, u16)>(10);

    let handle = session.clone();
    let channel_id = channel.id();
    // Update the terminals map with the active handle and session
    let pending_size = if let Some(t_handle) = terminals.lock().get_mut(&terminal_id) {
        t_handle.ssh_session = Some(handle.clone());
        if t_handle.ssh_session_id.is_none() {
            t_handle.ssh_session_id = session_config.as_ref().map(|s| s.id.clone());
        }
        t_handle.ssh_channel_id = Some(channel_id);
        t_handle.ssh_resize_tx = Some(resize_tx.clone());
        t_handle.last_size
    } else {
        None
    };

    // Only fire if a newer size landed while we were starting the shell — a
    // correction that arrives *after* the prompt is already on screen costs a
    // full repaint, so anything already applied above is not repeated.
    if let Some((cols, rows)) = pending_size {
        if (cols, rows) != applied_remote_size {
            log::debug!(
                "[pty:size] ssh resize after shell start | terminal_id={} from={}x{} to={}x{}",
                terminal_id,
                applied_remote_size.0,
                applied_remote_size.1,
                cols,
                rows
            );
            let _ = resize_tx.try_send((cols, rows));
        }
    }

    let event_tx_c = event_tx.clone();
    let terminal_id_c = terminal_id.clone();
    let shutdown_c = shutdown.clone();
    let charset_c = session_config
        .as_ref()
        .and_then(|s| s.terminal.charset.clone())
        .unwrap_or_else(|| "UTF-8".to_string());

    tokio::spawn(async move {
        let mut shutdown_interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
        loop {
            if shutdown_c.is_broken() {
                let _ = channel.close().await;
                break;
            }
            tokio::select! {
                _ = shutdown_interval.tick() => {
                    if shutdown_c.is_broken() {
                        let _ = channel.close().await;
                        break;
                    }
                }
                msg = channel.wait() => {
                    match msg {
                        Some(russh::ChannelMsg::Data { data }) => {
                            let processed_data = transcode_to_utf8(&data, &charset_c);
                            let _ = event_tx_c.send(PtyEvent::Data {
                                terminal_id: terminal_id_c.clone(),
                                data: processed_data,
                            }).await;
                        }
                        Some(russh::ChannelMsg::ExtendedData { data, .. }) => {
                            let processed_data = transcode_to_utf8(&data, &charset_c);
                            let _ = event_tx_c.send(PtyEvent::Data {
                                terminal_id: terminal_id_c.clone(),
                                data: processed_data,
                            }).await;
                        }
                        Some(russh::ChannelMsg::WindowAdjusted { .. }) => {}
                        Some(russh::ChannelMsg::Success) => {}
                        Some(russh::ChannelMsg::Failure) => {}
                        Some(russh::ChannelMsg::Eof) | Some(russh::ChannelMsg::Close) | None => {
                            let _ = event_tx_c.send(PtyEvent::Exit {
                                terminal_id: terminal_id_c.clone(),
                                exit_code: Some(0),
                            }).await;
                            break;
                        }
                        _ => {}
                    }
                }
                bytes_opt = tokio_input_rx.recv() => {
                    if let Some(bytes) = bytes_opt {
                        let target_bytes = transcode_from_utf8(&bytes, &charset_c);
                        if let Err(e) = channel.data(&target_bytes[..]).await {
                            log::warn!("[pty:ssh] channel.data failed: {:?} | terminal_id={}", e, terminal_id_c);
                            break;
                        }
                    } else {
                        let _ = channel.close().await;
                        break;
                    }
                }
                resize_opt = resize_rx.recv() => {
                    if let Some((cols, rows)) = resize_opt {
                        let _ = channel.window_change(cols as u32, rows as u32, 0, 0).await;
                    }
                }
            }
        }
        let _ = channel.close().await;
    });

    Ok(())
}

const IBM850_TABLE: [char; 128] = [
    '\u{00C7}', '\u{00FC}', '\u{00E9}', '\u{00E2}', '\u{00E4}', '\u{00E0}', '\u{00E5}', '\u{00E7}',
    '\u{00EA}', '\u{00EB}', '\u{00E8}', '\u{00EF}', '\u{00EE}', '\u{00EC}', '\u{00C4}', '\u{00C5}',
    '\u{00C9}', '\u{00E6}', '\u{00C6}', '\u{00F4}', '\u{00F6}', '\u{00F2}', '\u{00FB}', '\u{00F9}',
    '\u{00FF}', '\u{00D6}', '\u{00DC}', '\u{00F8}', '\u{00A3}', '\u{00D8}', '\u{00D7}', '\u{0192}',
    '\u{00E1}', '\u{00ED}', '\u{00F3}', '\u{00FA}', '\u{00F1}', '\u{00D1}', '\u{00AA}', '\u{00BA}',
    '\u{00BF}', '\u{00AE}', '\u{00AC}', '\u{00BD}', '\u{00BC}', '\u{00A1}', '\u{00AB}', '\u{00BB}',
    '\u{2591}', '\u{2592}', '\u{2593}', '\u{2502}', '\u{2524}', '\u{00C1}', '\u{00C2}', '\u{00C0}',
    '\u{00A9}', '\u{2563}', '\u{2551}', '\u{2557}', '\u{255D}', '\u{00A2}', '\u{00A5}', '\u{2510}',
    '\u{2514}', '\u{2534}', '\u{252C}', '\u{251C}', '\u{2500}', '\u{253C}', '\u{00E3}', '\u{00C3}',
    '\u{255A}', '\u{2554}', '\u{2569}', '\u{2566}', '\u{2560}', '\u{2550}', '\u{256C}', '\u{00A4}',
    '\u{00F0}', '\u{00D0}', '\u{00CA}', '\u{00CB}', '\u{00C8}', '\u{0131}', '\u{00CD}', '\u{00CE}',
    '\u{00CF}', '\u{2518}', '\u{250C}', '\u{2588}', '\u{2584}', '\u{00A6}', '\u{00CC}', '\u{2580}',
    '\u{00D3}', '\u{00DF}', '\u{00D4}', '\u{00D2}', '\u{00F5}', '\u{00D5}', '\u{00B5}', '\u{00FE}',
    '\u{00DE}', '\u{00DA}', '\u{00DB}', '\u{00D9}', '\u{00FD}', '\u{00DD}', '\u{00AF}', '\u{00B4}',
    '\u{00AD}', '\u{00B1}', '\u{2017}', '\u{00BE}', '\u{00B6}', '\u{00A7}', '\u{00F7}', '\u{00B8}',
    '\u{00B0}', '\u{00A8}', '\u{00B7}', '\u{00B9}', '\u{00B3}', '\u{00B2}', '\u{25A0}', '\u{00A0}',
];

const IBM860_TABLE: [char; 128] = [
    '\u{00C7}', '\u{00FC}', '\u{00E9}', '\u{00E2}', '\u{00E3}', '\u{00E0}', '\u{00C1}', '\u{00E7}',
    '\u{00EA}', '\u{00CA}', '\u{00E8}', '\u{00CD}', '\u{00D4}', '\u{00EC}', '\u{00C3}', '\u{00C2}',
    '\u{00C9}', '\u{00C0}', '\u{00C8}', '\u{00F4}', '\u{00F5}', '\u{00F2}', '\u{00DA}', '\u{00F9}',
    '\u{00CC}', '\u{00D5}', '\u{00DC}', '\u{00A2}', '\u{00A3}', '\u{00D9}', '\u{20A7}', '\u{00D3}',
    '\u{00E1}', '\u{00ED}', '\u{00F3}', '\u{00FA}', '\u{00F1}', '\u{00D1}', '\u{00AA}', '\u{00BA}',
    '\u{00BF}', '\u{00D2}', '\u{00AC}', '\u{00BD}', '\u{00BC}', '\u{00A1}', '\u{00AB}', '\u{00BB}',
    '\u{2591}', '\u{2592}', '\u{2593}', '\u{2502}', '\u{2524}', '\u{2561}', '\u{2562}', '\u{2556}',
    '\u{2555}', '\u{2563}', '\u{2551}', '\u{2557}', '\u{255D}', '\u{255C}', '\u{255B}', '\u{2510}',
    '\u{2514}', '\u{2534}', '\u{252C}', '\u{251C}', '\u{2500}', '\u{253C}', '\u{255E}', '\u{255F}',
    '\u{255A}', '\u{2554}', '\u{2569}', '\u{2566}', '\u{2560}', '\u{2550}', '\u{256C}', '\u{2567}',
    '\u{2568}', '\u{2564}', '\u{2565}', '\u{2559}', '\u{2558}', '\u{2552}', '\u{2553}', '\u{256B}',
    '\u{256A}', '\u{2518}', '\u{250C}', '\u{2588}', '\u{2584}', '\u{258C}', '\u{2590}', '\u{2580}',
    '\u{03B1}', '\u{00DF}', '\u{0393}', '\u{03C0}', '\u{03A3}', '\u{03C3}', '\u{00B5}', '\u{03C4}',
    '\u{03A6}', '\u{0398}', '\u{03A9}', '\u{03B4}', '\u{221E}', '\u{03C6}', '\u{03B5}', '\u{2229}',
    '\u{2261}', '\u{00B1}', '\u{2265}', '\u{2264}', '\u{2320}', '\u{2321}', '\u{00F7}', '\u{2248}',
    '\u{00B0}', '\u{2219}', '\u{00B7}', '\u{221A}', '\u{207F}', '\u{00B2}', '\u{25A0}', '\u{00A0}',
];

const HP_ROMAN8_TABLE: [char; 128] = [
    '\u{0080}', '\u{0081}', '\u{0082}', '\u{0083}', '\u{0084}', '\u{0085}', '\u{0086}', '\u{0087}',
    '\u{0088}', '\u{0089}', '\u{008A}', '\u{008B}', '\u{008C}', '\u{008D}', '\u{008E}', '\u{008F}',
    '\u{0090}', '\u{0091}', '\u{0092}', '\u{0093}', '\u{0094}', '\u{0095}', '\u{0096}', '\u{0097}',
    '\u{0098}', '\u{0099}', '\u{009A}', '\u{009B}', '\u{009C}', '\u{009D}', '\u{009E}', '\u{009F}',
    '\u{00A0}', '\u{00C0}', '\u{00C2}', '\u{00C8}', '\u{00CA}', '\u{00CB}', '\u{00CE}', '\u{00CF}',
    '\u{00B4}', '\u{02CB}', '\u{02C6}', '\u{00A8}', '\u{02DC}', '\u{00D9}', '\u{00DB}', '\u{20A4}',
    '\u{00AF}', '\u{00DD}', '\u{00FD}', '\u{00B0}', '\u{00C7}', '\u{00E7}', '\u{00D1}', '\u{00F1}',
    '\u{00A1}', '\u{00BF}', '\u{00A4}', '\u{00A3}', '\u{00A5}', '\u{00A7}', '\u{0192}', '\u{00A2}',
    '\u{00E2}', '\u{00EA}', '\u{00F4}', '\u{00FB}', '\u{00E1}', '\u{00E9}', '\u{00F3}', '\u{00FA}',
    '\u{00E0}', '\u{00E8}', '\u{00F2}', '\u{00F9}', '\u{00E4}', '\u{00EB}', '\u{00F6}', '\u{00FC}',
    '\u{00C5}', '\u{00EE}', '\u{00D8}', '\u{00C6}', '\u{00E5}', '\u{00ED}', '\u{00F8}', '\u{00E6}',
    '\u{00C4}', '\u{00EC}', '\u{00D6}', '\u{00DC}', '\u{00C9}', '\u{00EF}', '\u{00DF}', '\u{00D4}',
    '\u{00C1}', '\u{00C3}', '\u{00E3}', '\u{00D0}', '\u{00F0}', '\u{00CD}', '\u{00CC}', '\u{00D3}',
    '\u{00D2}', '\u{00D5}', '\u{00F5}', '\u{0160}', '\u{0161}', '\u{00DA}', '\u{0178}', '\u{00FF}',
    '\u{00DE}', '\u{00FE}', '\u{00B7}', '\u{00B5}', '\u{00B6}', '\u{00BE}', '\u{2014}', '\u{00BC}',
    '\u{00BD}', '\u{00AA}', '\u{00BA}', '\u{00AB}', '\u{25A0}', '\u{00BB}', '\u{00B1}', '\u{FFFD}',
];

const WINSAMI2_TABLE: [char; 128] = [
    '\u{20AC}', '\u{0081}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{008D}', '\u{017D}', '\u{008F}',
    '\u{0090}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{009D}', '\u{017E}', '\u{0178}',
    '\u{00A0}', '\u{00A1}', '\u{00A2}', '\u{00A3}', '\u{00A4}', '\u{00A5}', '\u{00A6}', '\u{00A7}',
    '\u{00A8}', '\u{00A9}', '\u{014B}', '\u{00AB}', '\u{00AC}', '\u{00AD}', '\u{00AE}', '\u{00AF}',
    '\u{00B0}', '\u{00B1}', '\u{00B2}', '\u{00B3}', '\u{00B4}', '\u{00B5}', '\u{00B6}', '\u{00B7}',
    '\u{00B8}', '\u{00B9}', '\u{014A}', '\u{00BB}', '\u{00BC}', '\u{00BD}', '\u{00BE}', '\u{00BF}',
    '\u{00C0}', '\u{00C1}', '\u{00C2}', '\u{00C3}', '\u{00C4}', '\u{00C5}', '\u{00C6}', '\u{00C7}',
    '\u{00C8}', '\u{00C9}', '\u{00CA}', '\u{00CB}', '\u{00CC}', '\u{00CD}', '\u{00CE}', '\u{00CF}',
    '\u{0110}', '\u{00D1}', '\u{00D2}', '\u{00D3}', '\u{00D4}', '\u{00D5}', '\u{00D6}', '\u{00D7}',
    '\u{00D8}', '\u{00D9}', '\u{00DA}', '\u{00DB}', '\u{00DC}', '\u{00DD}', '\u{00DE}', '\u{00DF}',
    '\u{00E0}', '\u{00E1}', '\u{00E2}', '\u{00E3}', '\u{00E4}', '\u{00E5}', '\u{00E6}', '\u{00E7}',
    '\u{00E8}', '\u{00E9}', '\u{00EA}', '\u{00EB}', '\u{00EC}', '\u{00ED}', '\u{00EE}', '\u{00EF}',
    '\u{0111}', '\u{00F1}', '\u{00F2}', '\u{00F3}', '\u{00F4}', '\u{00F5}', '\u{00F6}', '\u{00F7}',
    '\u{00F8}', '\u{00F9}', '\u{00FA}', '\u{00FB}', '\u{00FC}', '\u{00FD}', '\u{00FE}', '\u{00FF}',
];

const TSCII_TABLE: [char; 128] = [
    '\u{0BA9}', '\u{0BB1}', '\u{0BB2}', '\u{0BB3}', '\u{0BB4}', '\u{0BB5}', '\u{0BB8}', '\u{0BB9}',
    '\u{0B9C}', '\u{0BB7}', '\u{0B95}', '\u{0B99}', '\u{0B9A}', '\u{0B9E}', '\u{0B9F}', '\u{0BA3}',
    '\u{0BA4}', '\u{0BA8}', '\u{0BAA}', '\u{0BAE}', '\u{0BAF}', '\u{0BB0}', '\u{0096}', '\u{0097}',
    '\u{0098}', '\u{0099}', '\u{009A}', '\u{009B}', '\u{009C}', '\u{009D}', '\u{009E}', '\u{009F}',
    '\u{00A0}', '\u{0B85}', '\u{0B86}', '\u{0B87}', '\u{0B88}', '\u{0B89}', '\u{0B8A}', '\u{0B8E}',
    '\u{0B8F}', '\u{0B90}', '\u{0B92}', '\u{0B93}', '\u{0B94}', '\u{0B83}', '\u{0BBE}', '\u{0BBF}',
    '\u{0BC0}', '\u{0BC1}', '\u{0BC2}', '\u{0BC6}', '\u{0BC7}', '\u{0BC8}', '\u{0BCA}', '\u{0BCB}',
    '\u{0BCC}', '\u{0BCD}', '\u{00BA}', '\u{00BB}', '\u{00BC}', '\u{00BD}', '\u{00BE}', '\u{00BF}',
    '\u{00C0}', '\u{00C1}', '\u{00C2}', '\u{00C3}', '\u{00C4}', '\u{00C5}', '\u{00C6}', '\u{00C7}',
    '\u{00C8}', '\u{00C9}', '\u{00CA}', '\u{00CB}', '\u{00CC}', '\u{00CD}', '\u{00CE}', '\u{00CF}',
    '\u{00D0}', '\u{00D1}', '\u{00D2}', '\u{00D3}', '\u{00D4}', '\u{00D5}', '\u{00D6}', '\u{00D7}',
    '\u{00D8}', '\u{00D9}', '\u{00DA}', '\u{00DB}', '\u{00DC}', '\u{00DD}', '\u{00DE}', '\u{00DF}',
    '\u{00E0}', '\u{00E1}', '\u{00E2}', '\u{00E3}', '\u{00E4}', '\u{00E5}', '\u{00E6}', '\u{00E7}',
    '\u{00E8}', '\u{00E9}', '\u{00EA}', '\u{00EB}', '\u{00EC}', '\u{00ED}', '\u{00EE}', '\u{00EF}',
    '\u{00F0}', '\u{00F1}', '\u{00F2}', '\u{00F3}', '\u{00F4}', '\u{00F5}', '\u{00F6}', '\u{00F7}',
    '\u{00F8}', '\u{00F9}', '\u{00FA}', '\u{00FB}', '\u{00FC}', '\u{00FD}', '\u{00FE}', '\u{00FF}',
];

fn decode_single_byte(bytes: &[u8], table: &[char; 128]) -> Vec<u8> {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if b < 0x80 {
            out.push(b as char);
        } else {
            out.push(table[(b - 0x80) as usize]);
        }
    }
    out.into_bytes()
}

fn encode_single_byte(text: &str, table: &[char; 128]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for c in text.chars() {
        if (c as u32) < 0x80 {
            out.push(c as u8);
        } else if let Some(idx) = table.iter().position(|&tc| tc == c) {
            out.push((idx as u8) + 0x80);
        } else {
            out.push(b'?');
        }
    }
    out
}

fn decode_utf32_le(bytes: &[u8]) -> Vec<u8> {
    let mut out = String::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let code = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
    }
    out.into_bytes()
}

fn encode_utf32_le(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() * 4);
    for c in text.chars() {
        out.extend_from_slice(&(c as u32).to_le_bytes());
    }
    out
}

fn decode_utf32_be(bytes: &[u8]) -> Vec<u8> {
    let mut out = String::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let code = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
    }
    out.into_bytes()
}

fn encode_utf32_be(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() * 4);
    for c in text.chars() {
        out.extend_from_slice(&(c as u32).to_be_bytes());
    }
    out
}

/// Helper to convert raw bytes from a non-UTF8 charset into UTF-8 bytes for alacritty_terminal.
pub fn transcode_to_utf8(bytes: &[u8], charset: &str) -> Vec<u8> {
    let trimmed = charset.trim();
    if trimmed.eq_ignore_ascii_case("UTF-8") || trimmed.is_empty() {
        return bytes.to_vec();
    }
    let canonical = velowork_core::charset::canonicalize_charset(trimmed).unwrap_or(trimmed);
    match canonical {
        "UTF-32LE" => return decode_utf32_le(bytes),
        "UTF-32BE" => return decode_utf32_be(bytes),
        "IBM850" => return decode_single_byte(bytes, &IBM850_TABLE),
        "IBM860" => return decode_single_byte(bytes, &IBM860_TABLE),
        "hp-roman8" => return decode_single_byte(bytes, &HP_ROMAN8_TABLE),
        "WINSAMI2" => return decode_single_byte(bytes, &WINSAMI2_TABLE),
        "TSCII" => return decode_single_byte(bytes, &TSCII_TABLE),
        _ => {}
    }
    if let Some(enc) = encoding_rs::Encoding::for_label(canonical.as_bytes()) {
        if enc == encoding_rs::UTF_8 {
            return bytes.to_vec();
        }
        let (cow, _, _) = enc.decode(bytes);
        cow.as_bytes().to_vec()
    } else {
        bytes.to_vec()
    }
}

/// Helper to convert UTF-8 input bytes into target charset bytes for PTY input.
pub fn transcode_from_utf8(bytes: &[u8], charset: &str) -> Vec<u8> {
    let trimmed = charset.trim();
    if trimmed.eq_ignore_ascii_case("UTF-8") || trimmed.is_empty() {
        return bytes.to_vec();
    }
    let canonical = velowork_core::charset::canonicalize_charset(trimmed).unwrap_or(trimmed);
    let Ok(text) = std::str::from_utf8(bytes) else {
        return bytes.to_vec();
    };
    match canonical {
        "UTF-32LE" => return encode_utf32_le(text),
        "UTF-32BE" => return encode_utf32_be(text),
        "IBM850" => return encode_single_byte(text, &IBM850_TABLE),
        "IBM860" => return encode_single_byte(text, &IBM860_TABLE),
        "hp-roman8" => return encode_single_byte(text, &HP_ROMAN8_TABLE),
        "WINSAMI2" => return encode_single_byte(text, &WINSAMI2_TABLE),
        "TSCII" => return encode_single_byte(text, &TSCII_TABLE),
        _ => {}
    }
    if let Some(enc) = encoding_rs::Encoding::for_label(canonical.as_bytes()) {
        if enc == encoding_rs::UTF_8 {
            return bytes.to_vec();
        }
        let (cow, _, _) = enc.encode(text);
        cow.to_vec()
    } else {
        bytes.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transcode_utf8_passthrough() {
        let input = "Hello, 世界!".as_bytes();
        assert_eq!(transcode_to_utf8(input, "UTF-8"), input);
        assert_eq!(transcode_from_utf8(input, "UTF-8"), input);
    }

    #[test]
    fn test_transcode_gbk_roundtrip() {
        let (gbk_bytes, _, _) = encoding_rs::GBK.encode("你好，世界！");
        let decoded = transcode_to_utf8(&gbk_bytes, "GBK");
        assert_eq!(std::str::from_utf8(&decoded).unwrap(), "你好，世界！");

        let encoded = transcode_from_utf8(&decoded, "GBK");
        assert_eq!(encoded, gbk_bytes.to_vec());
    }

    #[test]
    fn test_transcode_big5_and_shiftjis() {
        let (big5_bytes, _, _) = encoding_rs::BIG5.encode("繁體中文測試");
        let decoded_big5 = transcode_to_utf8(&big5_bytes, "Big5");
        assert_eq!(std::str::from_utf8(&decoded_big5).unwrap(), "繁體中文測試");

        let (sjis_bytes, _, _) = encoding_rs::SHIFT_JIS.encode("日本語テスト");
        let decoded_sjis = transcode_to_utf8(&sjis_bytes, "Shift_JIS");
        assert_eq!(std::str::from_utf8(&decoded_sjis).unwrap(), "日本語テスト");
    }

    #[test]
    fn test_transcode_utf32_and_custom_codecs() {
        // UTF-32LE
        let sample = "Velowork 终端";
        let le_bytes = encode_utf32_le(sample);
        let decoded_le = transcode_to_utf8(&le_bytes, "UTF-32LE");
        assert_eq!(std::str::from_utf8(&decoded_le).unwrap(), sample);
        assert_eq!(transcode_from_utf8(sample.as_bytes(), "UTF-32LE"), le_bytes);

        // UTF-32BE
        let be_bytes = encode_utf32_be(sample);
        let decoded_be = transcode_to_utf8(&be_bytes, "UTF-32BE");
        assert_eq!(std::str::from_utf8(&decoded_be).unwrap(), sample);
        assert_eq!(transcode_from_utf8(sample.as_bytes(), "UTF-32BE"), be_bytes);

        // IBM850
        let ibm850_sample = "Çüéâäàåç";
        let ibm850_bytes = encode_single_byte(ibm850_sample, &IBM850_TABLE);
        let decoded_850 = transcode_to_utf8(&ibm850_bytes, "IBM850");
        assert_eq!(std::str::from_utf8(&decoded_850).unwrap(), ibm850_sample);

        // hp-roman8
        let hp_sample = "ÀÂÈÊËÎÏ";
        let hp_bytes = encode_single_byte(hp_sample, &HP_ROMAN8_TABLE);
        let decoded_hp = transcode_to_utf8(&hp_bytes, "hp-roman8");
        assert_eq!(std::str::from_utf8(&decoded_hp).unwrap(), hp_sample);

        // ISO-8859-15
        let (iso15_bytes, _, _) = encoding_rs::ISO_8859_15.encode("€ Euro sign test");
        let decoded_iso15 = transcode_to_utf8(&iso15_bytes, "ISO-8859-15");
        assert_eq!(std::str::from_utf8(&decoded_iso15).unwrap(), "€ Euro sign test");
    }

    #[test]
    fn test_parse_protocol_args() {
        // Local args
        let local_args = vec!["--id".to_string(), "local-session-123".to_string(), "--shell".to_string(), "bash".to_string()];
        assert_eq!(parse_local_args(&local_args), Some("local-session-123".to_string()));
        assert_eq!(parse_local_shell_arg(&local_args), Some("bash".to_string()));

        let local_session_id_flag = vec!["--session-id".to_string(), "local-sid-456".to_string()];
        assert_eq!(parse_local_args(&local_session_id_flag), Some("local-sid-456".to_string()));

        let empty_args: Vec<String> = vec![];
        assert_eq!(parse_local_args(&empty_args), None);

        // Serial args
        let serial_args = vec!["--port".to_string(), "/dev/ttyUSB0".to_string(), "--baud".to_string(), "115200".to_string(), "--id".to_string(), "serial-1".to_string()];
        let (port, baud, sid) = parse_serial_args(&serial_args).unwrap();
        assert_eq!(port, "/dev/ttyUSB0");
        assert_eq!(baud, 115200);
        assert_eq!(sid, Some("serial-1".to_string()));

        // Telnet args
        let telnet_args = vec!["192.168.1.1".to_string(), "-p".to_string(), "2323".to_string(), "--id".to_string(), "telnet-1".to_string()];
        let (host, port, sid) = parse_telnet_args(&telnet_args).unwrap();
        assert_eq!(host, "192.168.1.1");
        assert_eq!(port, 2323);
        assert_eq!(sid, Some("telnet-1".to_string()));

        // SSH args
        let ssh_args = vec!["-p".to_string(), "2222".to_string(), "--id".to_string(), "ssh-1".to_string(), "root@example.com".to_string()];
        let (host, port, user, _, sid, _) = parse_ssh_args(&ssh_args).unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 2222);
        assert_eq!(user, "root");
        assert_eq!(sid, Some("ssh-1".to_string()));
    }

    #[test]
    fn test_default_term_type_and_env_injection() {
        let (mgr, _) = PtyManager::new(SessionBackend::None);
        assert_eq!(mgr.default_term_type(), velowork_core::DEFAULT_TERM_TYPE);

        mgr.set_default_term_type("linux".to_string());
        assert_eq!(mgr.default_term_type(), "linux");

        let mut cmd = CommandBuilder::new_default_prog();
        PtyManager::set_terminal_env(&mut cmd, "test-term-1", "dumb");
        // Verify dumb suppresses COLORTERM
        // CommandBuilder doesn't expose public env map inspect on all platforms directly,
        // but we verify execution doesn't panic and logic runs safely.
        PtyManager::set_terminal_env(&mut cmd, "test-term-2", "xterm-256color");
        PtyManager::set_terminal_env(&mut cmd, "test-term-3", "vt100");
    }

    #[test]
    fn test_parse_proxy_url() {
        let p1 = parse_proxy_url("http://127.0.0.1:7890").unwrap();
        assert_eq!(p1.0, velowork_state::ProxyType::Http);
        assert_eq!(p1.1, "127.0.0.1");
        assert_eq!(p1.2, 7890);
        assert_eq!(p1.3, None);
        assert_eq!(p1.4, None);

        let p2 = parse_proxy_url("socks5://admin:secret123@proxy.example.com:1080").unwrap();
        assert_eq!(p2.0, velowork_state::ProxyType::Socks5);
        assert_eq!(p2.1, "proxy.example.com");
        assert_eq!(p2.2, 1080);
        assert_eq!(p2.3, Some("admin".to_string()));
        assert_eq!(p2.4, Some("secret123".to_string()));

        let p3 = parse_proxy_url("192.168.1.100:8888").unwrap();
        assert_eq!(p3.0, velowork_state::ProxyType::Http);
        assert_eq!(p3.1, "192.168.1.100");
        assert_eq!(p3.2, 8888);

        assert_eq!(parse_proxy_url(""), None);
    }

    #[test]
    fn test_global_proxy_settings() {
        let (mgr, _) = PtyManager::new(SessionBackend::None);
        let s = GlobalProxySettings {
            mode: "http".to_string(),
            host: "127.0.0.1".to_string(),
            port: 7890,
        };
        mgr.set_global_proxy(s.clone());
        assert_eq!(mgr.global_proxy(), s);
    }

    #[test]
    fn test_build_russh_client_config() {
        let mut session = velowork_state::SshSession {
            host: "test-host".to_string(),
            port: 22,
            username: "testuser".to_string(),
            ..Default::default()
        };
        session.algorithms_automatic = false;
        session.kex_algorithms = vec!["curve25519-sha256".to_string()];
        session.compression = velowork_state::CompressionType::Zlib;
        session.rekey_time = 1800;

        let cfg = build_russh_client_config(&session);
        assert!(cfg.preferred.kex.iter().any(|k| k.as_ref() == "curve25519-sha256"));
        assert!(cfg.preferred.compression.contains(&russh::compression::ZLIB));
        assert_eq!(cfg.limits.rekey_time_limit, std::time::Duration::from_secs(1800));
    }
}
