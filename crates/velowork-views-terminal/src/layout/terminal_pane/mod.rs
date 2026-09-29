//! Terminal pane view - composition of child entity views.

mod actions;
pub mod blocks_popup;
pub mod commands_panel;
mod content;
pub mod file_row;
pub mod history_cache;
pub mod history_popup;
mod navigation;
mod render;
mod scrollbar;
mod search_bar;
pub mod sftp_panel;
pub mod url_detector;
mod zoom;

use std::path::PathBuf;
use velowork_i18n::i18n;

use search_bar::{SearchBar, SearchBarEvent};
use velowork_ui::design::semantic::SemanticPalette;
use velowork_ui::icon::AppIcon;
use velowork_ui::icon_button::icon_button_sized;
use velowork_ui::simple_input::{InputChangedEvent, SimpleInputState};
use velowork_ui::theme::surface_bg_t;
use velowork_ui::tokens::{
    ICON_STD, RADIUS_LG, RADIUS_MD, RADIUS_STD, SPACE_LG, SPACE_MD, SPACE_SM, SPACE_XS, ui_text_md,
    ui_text_ms,
};
use velowork_ui::tooltip::Tooltip;

pub use content::{TerminalContent, TerminalContentEvent, render_line_numbers_gutter};

use crate::ActionDispatch;
use crate::terminal_view_settings;
use gpui::prelude::*;
use gpui::*;
use std::sync::Arc;
use std::time::Duration;
use velowork_terminal::TerminalsRegistry;
use velowork_terminal::backend::TerminalBackend;
use velowork_terminal::shell_config::ShellType;
use velowork_terminal::terminal::{Terminal, TerminalSize};
use velowork_workspace::focus::FocusManager;
use velowork_workspace::request_broker::RequestBroker;
use velowork_workspace::state::{WindowId, Workspace};

/// A terminal pane view composed of child entity views.
pub struct TerminalPane<D: ActionDispatch> {
    // Identity
    workspace: Entity<Workspace>,
    pub(super) focus_manager: Entity<FocusManager>,
    request_broker: Entity<RequestBroker>,
    pub(super) window_id: WindowId,
    project_id: String,
    project_path: String,
    layout_path: Vec<usize>,

    // Terminal state
    terminal: Option<Arc<Terminal>>,
    terminal_id: Option<String>,
    backend: Arc<dyn TerminalBackend>,
    terminals: TerminalsRegistry,

    // Child views
    content: Entity<TerminalContent>,
    search_bar: Entity<SearchBar>,
    pub(super) quick_connect_input: Entity<SimpleInputState>,

    // Focus
    focus_handle: FocusHandle,

    // State
    minimized: bool,
    detached: bool,
    cursor_visible: bool,
    shell_type: ShellType,
    was_focused: bool,

    // Action dispatcher (local or remote)
    pub(super) action_dispatcher: Option<D>,

    // Log recording toolbar timer & drag positioning
    log_timer_running: bool,
    log_toolbar_pos: Option<Point<Pixels>>,
    log_toolbar_mouse_offset: Option<Point<Pixels>>,
    log_toolbar_bounds: Option<Bounds<Pixels>>,
    pane_bounds: Option<Bounds<Pixels>>,
    log_toolbar_start_time: Option<std::time::Instant>,
    log_toolbar_exiting: bool,
    log_toolbar_exit_start_time: Option<std::time::Instant>,

    // History Autocompletion & Multi-line command accumulation
    input_line_buffer: String,
    command_accumulator: Vec<String>,
    history_popup_items: Vec<velowork_workspace::repositories::HistoryEntry>,
    history_popup_selected: Option<usize>,
    history_popup_open: bool,

    // Welcome screen search selection
    pub(super) welcome_selected_index: Option<usize>,

    // Reconnection state
    is_reconnecting: bool,

    // ZMODEM in-pane transfer state
    pub(super) zmodem_cancel_flag: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub(super) is_zmodem_prompt_open: bool,
    pub(super) last_zmodem_close_time: Option<std::time::Instant>,
    pub(super) zmodem_aborted_notified: Arc<std::sync::atomic::AtomicBool>,
}

impl<D: ActionDispatch + Send + Sync> TerminalPane<D> {
    // GPUI view constructor: each param is a distinct injected dependency.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        workspace: Entity<Workspace>,
        focus_manager: Entity<FocusManager>,
        request_broker: Entity<RequestBroker>,
        window_id: WindowId,
        project_id: String,
        project_path: String,
        layout_path: Vec<usize>,
        terminal_id: Option<String>,
        minimized: bool,
        detached: bool,
        backend: Arc<dyn TerminalBackend>,
        terminals: TerminalsRegistry,
        action_dispatcher: Option<D>,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();

        let shell_type = workspace
            .read(cx)
            .get_terminal_shell(&project_id, &layout_path)
            .unwrap_or(ShellType::Default);

        let content = cx.new(|cx| {
            TerminalContent::new(
                focus_handle.clone(),
                Some(window_id),
                project_id.clone(),
                layout_path.clone(),
                workspace.clone(),
                cx,
            )
        });

        let search_bar = cx.new(|cx| SearchBar::new(workspace.clone(), focus_manager.clone(), cx));
        let quick_connect_input = cx.new(|cx| {
            SimpleInputState::new(cx).placeholder(velowork_i18n::i18n!(
                cx,
                "welcome.quick_connect_placeholder"
            ))
        });
        cx.subscribe(
            &quick_connect_input,
            |this: &mut Self, _, _: &InputChangedEvent, cx| {
                this.welcome_selected_index = None;
                cx.notify();
            },
        )
        .detach();
        let fm_for_quick = focus_manager.clone();
        cx.subscribe(
            &quick_connect_input,
            move |_this: &mut Self, _, _: &velowork_ui::simple_input::InputFocusedEvent, cx| {
                fm_for_quick.update(cx, |fm, _| {
                    fm.request_focus(velowork_workspace::focus::FocusLayer::Terminal);
                });
            },
        )
        .detach();

        cx.subscribe(&search_bar, Self::handle_search_bar_event)
            .detach();
        cx.subscribe(&content, Self::handle_content_event)
            .detach();
        let mut pane = Self {
            workspace,
            focus_manager,
            request_broker,
            window_id,
            project_id,
            project_path,
            layout_path,
            terminal: None,
            terminal_id,
            backend,
            terminals,
            content,
            search_bar,
            quick_connect_input,
            focus_handle,
            minimized,
            detached,
            cursor_visible: true,
            shell_type,
            was_focused: false,
            action_dispatcher,
            log_timer_running: false,
            log_toolbar_pos: None,
            log_toolbar_mouse_offset: None,
            log_toolbar_bounds: None,
            pane_bounds: None,
            log_toolbar_start_time: None,
            log_toolbar_exiting: false,
            log_toolbar_exit_start_time: None,
            input_line_buffer: String::new(),
            command_accumulator: Vec::new(),
            history_popup_items: Vec::new(),
            history_popup_selected: None,
            history_popup_open: false,
            welcome_selected_index: None,
            is_reconnecting: false,
            zmodem_cancel_flag: None,
            is_zmodem_prompt_open: false,
            last_zmodem_close_time: None,
            zmodem_aborted_notified: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };

        let conn_store = cx
            .try_global::<velowork_workspace::stores::GlobalConnectionStore>()
            .map(|c| c.0.clone());
        if let Some(conn_store) = conn_store {
            cx.subscribe(
                &conn_store,
                |this: &mut Self, _, event: &velowork_workspace::stores::ConnectionEvent, cx| {
                    if let velowork_workspace::stores::ConnectionEvent::Connected(_) = event {
                        this.is_reconnecting = false;
                    }
                    cx.notify();
                },
            )
            .detach();
        }

        let session_store = cx
            .try_global::<velowork_workspace::stores::GlobalSessionStore>()
            .map(|s| s.0.clone());
        if let Some(session_store) = session_store {
            cx.observe(&session_store, |_, _, cx| cx.notify()).detach();
        }

        if pane.shell_type == ShellType::Welcome {
            if pane.terminal_id.is_none() {
                let welcome_tid = format!("welcome:{}", uuid::Uuid::new_v4());
                pane.terminal_id = Some(welcome_tid.clone());
                pane.workspace.update(cx, |ws, cx| {
                    ws.set_terminal_id(&pane.project_id, &pane.layout_path, welcome_tid, cx);
                });
            }
        } else if let Some(ref id) = pane.terminal_id {
            pane.create_terminal_for_existing_pty(id.clone(), cx);
        } else {
            pane.create_new_terminal(cx);
        }

        if pane
            .terminal_id
            .as_deref()
            .is_some_and(|id| id.starts_with("remote:"))
        {
            pane.start_remote_dirty_check_loop(cx);
        }
        pane.start_cursor_blink_loop(cx);
        pane.start_idle_check_loop(cx);
        pane.start_connection_watch(cx);
        cx.observe(&pane.content, |this, _, cx| this.process_zmodem_events(cx)).detach();
        // Observe the shared terminal background cache so this pane re-renders
        // (for the fade-in) the moment the single global decode completes.
        if let Some(cache) = crate::terminal_background_cache(cx) {
            cx.observe(&cache, |_, _, cx| cx.notify()).detach();
        }

        pane
    }

    fn handle_search_bar_event(
        &mut self,
        _: Entity<SearchBar>,
        event: &SearchBarEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            SearchBarEvent::Closed => {
                self.content.update(cx, |content, _| {
                    content.set_search_highlights(Arc::new(Vec::new()), None);
                });
                cx.notify();
            }
            SearchBarEvent::MatchesChanged(matches, idx) => {
                self.content.update(cx, |content, _| {
                    content.set_search_highlights(matches.clone(), *idx);
                });
                cx.notify();
            }
        }
    }

    fn handle_content_event(
        &mut self,
        _: Entity<TerminalContent>,
        event: &TerminalContentEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            TerminalContentEvent::RequestContextMenu {
                position,
                has_selection,
                link_url,
            } => {
                if let Some(ref terminal_id) = self.terminal_id {
                    self.request_broker.update(cx, |broker, cx| {
                        broker.push_overlay_request(
                            velowork_workspace::requests::OverlayRequest::Project(velowork_workspace::requests::ProjectOverlay {
                                project_id: self.project_id.clone(),
                                kind: velowork_workspace::requests::ProjectOverlayKind::TerminalContextMenu {
                                    terminal_id: terminal_id.clone(),
                                    layout_path: self.layout_path.clone(),
                                    position: *position,
                                    has_selection: *has_selection,
                                    link_url: link_url.clone(),
                                },
                            }),
                            cx,
                        );
                    });
                }
            }
            TerminalContentEvent::ShowAiFloatingToolbar {
                position,
                selection_text,
            } => {
                if let Some(ref terminal_id) = self.terminal_id {
                    self.request_broker.update(cx, |broker, cx| {
                        broker.push_overlay_request(
                            velowork_workspace::requests::OverlayRequest::Project(
                                velowork_workspace::requests::ProjectOverlay {
                                    project_id: self.project_id.clone(),
                                    kind: velowork_workspace::requests::ProjectOverlayKind::ShowAiFloatingToolbar {
                                        terminal_id: terminal_id.clone(),
                                        position: *position,
                                        selection_text: selection_text.clone(),
                                    },
                                },
                            ),
                            cx,
                        );
                    });
                }
            }
            TerminalContentEvent::DismissAiFloatingToolbar => {
                self.request_broker.update(cx, |broker, cx| {
                    broker.push_overlay_request(
                        velowork_workspace::requests::OverlayRequest::Project(
                            velowork_workspace::requests::ProjectOverlay {
                                project_id: self.project_id.clone(),
                                kind: velowork_workspace::requests::ProjectOverlayKind::DismissAiFloatingToolbar,
                            },
                        ),
                        cx,
                    );
                });
            }
        }
    }

    fn start_remote_dirty_check_loop(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this: WeakEntity<TerminalPane<D>>, cx| {
            let mut current_interval = Duration::from_millis(8);
            loop {
                smol::Timer::after(current_interval).await;
                let result = this.update(cx, |pane, cx| {
                    if let Some(terminal) = pane.terminal.as_ref() {
                        if terminal.take_dirty() {
                            // Parse freshly arrived bytes and update UI
                            terminal.process_pending_output();
                            pane.process_zmodem_events(cx);
                            pane.content.update(cx, |_, cx| cx.notify());
                            current_interval = Duration::from_millis(8);
                        } else {
                            // Back off when idle up to 100ms to reduce CPU wakeups
                            current_interval =
                                (current_interval * 2).min(Duration::from_millis(100));
                        }
                    }
                });
                if result.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn start_connection_watch(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this: WeakEntity<TerminalPane<D>>, cx| {
            let interval = Duration::from_millis(500);
            let mut last_lost = false;
            loop {
                smol::Timer::after(interval).await;
                let result = this.update(cx, |pane, cx| {
                    let current_lost = pane.is_connection_lost(cx);
                    if current_lost != last_lost {
                        last_lost = current_lost;
                        cx.notify();
                    }
                });
                if result.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Whether this terminal connection or process has been lost.
    pub fn is_connection_lost(&self, cx: &App) -> bool {
        if self.shell_type == ShellType::Welcome {
            return false;
        }
        let Some(ref tid) = self.terminal_id else {
            return false;
        };
        if let Some(s) = self.backend.get_ssh_session(tid) {
            if let Some(sid) = self.backend.get_ssh_session_id(tid) {
                let store_connected = cx
                    .global::<velowork_workspace::stores::GlobalConnectionStore>()
                    .0
                    .read(cx)
                    .is_connected(&sid);
                !store_connected || s.is_closed()
            } else {
                s.is_closed()
            }
        } else if let Some(sid) = self.backend.get_ssh_session_id(tid) {
            let store_connected = cx
                .global::<velowork_workspace::stores::GlobalConnectionStore>()
                .0
                .read(cx)
                .is_connected(&sid);
            !store_connected
        } else {
            let lock = self.terminals.lock();
            if let Some(t) = lock.get(tid) {
                t.shell_pid().is_none()
            } else {
                true
            }
        }
    }
}

impl<D: ActionDispatch + Send + Sync> TerminalPane<D> {

    /// Reconnect the terminal process / session.
    pub fn handle_reconnect(&mut self, cx: &mut Context<Self>) {
        if self.is_reconnecting {
            return;
        }
        let Some(ref old_terminal_id) = self.terminal_id.clone() else {
            return;
        };
        self.is_reconnecting = true;
        cx.notify();

        let ws = self.workspace.read(cx);
        let settings = terminal_view_settings(cx);
        let shell = self.shell_type.clone().resolve_default(
            ws.project(&self.project_id)
                .and_then(|p| p.default_shell.as_ref()),
            &settings.default_shell,
        );
        let project_path = ws
            .project(&self.project_id)
            .map(|p| p.path.clone())
            .unwrap_or_else(|| self.project_path.clone());

        // Resolve session ID for SSH/Serial/Telnet sessions
        let session_id = match &shell {
            ShellType::Custom { path, args } if path == "ssh" => {
                velowork_terminal::pty_manager::parse_ssh_args(args)
                    .and_then(|(_, _, _, _, sid, _)| sid)
            }
            ShellType::Custom { path, args } if path == "serial" => {
                velowork_terminal::pty_manager::parse_serial_args(args).and_then(|(_, _, sid)| sid)
            }
            ShellType::Custom { path, args } if path == "telnet" => {
                velowork_terminal::pty_manager::parse_telnet_args(args).and_then(|(_, _, sid)| sid)
            }
            _ => None,
        }
        .or_else(|| self.backend.get_ssh_session_id(old_terminal_id));

        if let Some(ref sid) = session_id {
            if let Some(conn_store) = cx
                .try_global::<velowork_workspace::stores::GlobalConnectionStore>()
                .map(|c| c.0.clone())
            {
                conn_store.update(cx, |store, cx| {
                    store.mark_connected(sid, cx);
                });
            }
        }

        // Kill any lingering backend process and remove cached terminal before reconnecting
        self.backend.kill(old_terminal_id);
        self.terminals.lock().remove(old_terminal_id);

        match self.backend.create_terminal(&project_path, Some(&shell)) {
            Ok(new_terminal_id) => {
                self.terminal_id = Some(new_terminal_id.clone());
                self.workspace.update(cx, |ws, cx| {
                    ws.set_terminal_id(
                        &self.project_id,
                        &self.layout_path,
                        new_terminal_id.clone(),
                        cx,
                    );
                });

                let resolved = self.resolve_terminal_config(Some(&new_terminal_id), cx);
                let size = TerminalSize::default();
                let terminal = Arc::new(Terminal::new_with_options(
                    new_terminal_id.clone(),
                    size,
                    self.backend.transport(),
                    project_path,
                    resolved.scrollback_lines as usize,
                    Some(resolved.word_separators.clone()),
                ));
                if let Some(pid) = self.backend.get_foreground_shell_pid(&new_terminal_id) {
                    terminal.set_shell_pid(pid);
                }
                self.terminals
                    .lock()
                    .insert(new_terminal_id.clone(), terminal.clone());
                self.terminal = Some(terminal.clone());
                self.update_child_terminals(terminal, cx);

                let pid = self.project_id.clone();
                let path = self.layout_path.clone();
                self.focus_manager.update(cx, |fm, _| {
                    fm.focus_terminal(pid.clone(), path.clone());
                });
                self.workspace.update(cx, |ws, cx| {
                    ws.touch_project(&pid);
                    ws.bump_activity(&pid, cx);
                    cx.notify();
                });

                cx.spawn(async move |this: WeakEntity<Self>, cx| {
                    smol::Timer::after(Duration::from_millis(200)).await;
                    let _ = this.update(cx, |this, cx| {
                        this.is_reconnecting = false;
                        cx.notify();
                    });
                })
                .detach();
            }
            Err(e) => {
                self.is_reconnecting = false;
                if let Some(ref sid) = session_id {
                    if let Some(conn_store) = cx
                        .try_global::<velowork_workspace::stores::GlobalConnectionStore>()
                        .map(|c| c.0.clone())
                    {
                        conn_store.update(cx, |store, cx| {
                            store.mark_disconnected(sid, cx);
                        });
                    }
                }
                velowork_workspace::toast::ToastManager::error(
                    format!("Failed to reconnect terminal: {}", e),
                    cx,
                );
            }
        }
        cx.notify();
    }

    /// Whether this project's layout contains a `Split` node — i.e. the
    /// terminal area is showing more than one pane at once ("分屏"). Single-pane
    /// projects (even with several terminals stacked behind inactive tabs)
    /// return `false`, so the "no split" focus treatment applies.
    fn is_project_split(&self, cx: &App) -> bool {
        self.workspace
            .read(cx)
            .project(&self.project_id)
            .and_then(|p| p.layout.as_ref())
            .is_some_and(|layout| layout.has_split())
    }

    /// Whether this pane is the currently focused terminal in the unified
    /// `FocusManager` — the "active" region of a split.
    #[allow(dead_code)]
    fn is_active_pane(&self, cx: &App) -> bool {
        self.focus_manager
            .read(cx)
            .is_focused(&self.project_id, &self.layout_path)
    }

    pub(crate) fn resolve_terminal_config(
        &self,
        terminal_id_opt: Option<&str>,
        cx: &App,
    ) -> velowork_terminal::ResolvedTerminalConfig {
        let default_session_options = velowork_state::SessionTerminalOptions::default();
        let session_id = terminal_id_opt
            .and_then(|tid| self.backend.get_ssh_session_id(tid))
            .or_else(|| match &self.shell_type {
                velowork_core::shell::ShellType::Custom { path, args } if path == "ssh" => {
                    velowork_terminal::pty_manager::parse_ssh_args(args)
                        .and_then(|(_, _, _, _, sid, _)| sid)
                }
                velowork_core::shell::ShellType::Custom { path, args } if path == "serial" => {
                    velowork_terminal::pty_manager::parse_serial_args(args)
                        .and_then(|(_, _, sid)| sid)
                }
                velowork_core::shell::ShellType::Custom { path, args } if path == "telnet" => {
                    velowork_terminal::pty_manager::parse_telnet_args(args)
                        .and_then(|(_, _, sid)| sid)
                }
                velowork_core::shell::ShellType::Custom { path, args } if path == "local" => {
                    velowork_terminal::pty_manager::parse_local_args(args)
                }
                _ => None,
            });

        let session_options = session_id.as_deref().and_then(|sid| {
            cx.try_global::<velowork_workspace::stores::GlobalSessionStore>()
                .and_then(|s| s.0.read(cx).find_session(sid).map(|s| s.terminal.clone()))
        });
        let session_options_ref = session_options.as_ref().unwrap_or(&default_session_options);
        let defaults = crate::terminal_view_settings(cx).terminal_defaults();
        velowork_terminal::resolve_effective_terminal_config(session_options_ref, &defaults)
    }

    fn start_cursor_blink_loop(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this: WeakEntity<TerminalPane<D>>, cx| {
            let interval = Duration::from_millis(500);
            loop {
                smol::Timer::after(interval).await;

                let result = this.update(cx, |pane, cx| {
                    let resolved = pane.resolve_terminal_config(pane.terminal_id.as_deref(), cx);
                    // App-set DECSCUSR blinking wins over the user setting.
                    let blink_enabled = pane
                        .terminal
                        .as_ref()
                        .and_then(|t| t.app_cursor_blinking())
                        .unwrap_or(resolved.cursor_blink);

                    if blink_enabled {
                        if !pane.was_focused {
                            if !pane.cursor_visible {
                                pane.cursor_visible = true;
                                pane.content.update(cx, |content, _| {
                                    content.set_cursor_visible(true);
                                });
                            }
                            return;
                        }
                        pane.cursor_visible = !pane.cursor_visible;
                        pane.content.update(cx, |content, cx| {
                            content.set_cursor_visible(pane.cursor_visible);
                            cx.notify();
                        });
                    } else if !pane.cursor_visible {
                        pane.cursor_visible = true;
                        pane.content.update(cx, |content, cx| {
                            content.set_cursor_visible(true);
                            cx.notify();
                        });
                    }
                });

                if result.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn start_idle_check_loop(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this: WeakEntity<TerminalPane<D>>, cx| {
            let interval = Duration::from_secs(2);
            let mut was_waiting = false;
            loop {
                smol::Timer::after(interval).await;

                let check_info = this.update(cx, |pane, cx| {
                    let idle_timeout = crate::terminal_view_settings(cx).idle_timeout_secs;
                    if idle_timeout == 0 {
                        return None;
                    }
                    pane.terminal.as_ref().map(|t| {
                        let idle_threshold = Duration::from_secs(idle_timeout as u64);
                        let is_idle = t.last_output_time().elapsed() >= idle_threshold;
                        let pid = t.shell_pid();
                        let had_input = t.had_user_input();
                        let has_unseen = t.has_unseen_output();
                        (t.clone(), is_idle, pid, had_input, has_unseen)
                    })
                });

                let check_info = match check_info {
                    Ok(Some(info)) => info,
                    Ok(None) => {
                        if was_waiting {
                            was_waiting = false;
                            let _ = this.update(cx, |pane, cx| {
                                if let Some(ref t) = pane.terminal {
                                    t.set_waiting_for_input(false);
                                }
                                cx.notify();
                            });
                        }
                        continue;
                    }
                    Err(_) => break,
                };

                let (terminal, is_idle, pid, had_input, has_unseen) = check_info;

                if !had_input || !has_unseen {
                    if was_waiting {
                        was_waiting = false;
                        terminal.set_waiting_for_input(false);
                        let _ = this.update(cx, |_pane, cx| {
                            cx.notify();
                        });
                    }
                    continue;
                }

                let has_children = if let Some(pid) = pid {
                    smol::unblock(move || velowork_terminal::terminal::has_child_processes(pid))
                        .await
                } else {
                    false
                };

                let is_waiting = is_idle && !has_children;

                terminal.set_waiting_for_input(is_waiting);
                if is_waiting != was_waiting {
                    was_waiting = is_waiting;
                    let _ = this.update(cx, |_pane, cx| {
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    /// Start 1-second timer loop for log recording toolbar elapsed time display.
    fn ensure_log_timer(&mut self, cx: &mut Context<Self>) {
        if self.log_timer_running {
            return;
        }
        self.log_timer_running = true;
        cx.spawn(async move |this: WeakEntity<TerminalPane<D>>, cx| {
            loop {
                smol::Timer::after(Duration::from_secs(1)).await;
                let should_continue = this.update(cx, |pane, cx| {
                    let is_recording = pane.terminal.as_ref().is_some_and(|t| t.is_log_recording());
                    if is_recording {
                        cx.notify();
                        true
                    } else {
                        pane.log_timer_running = false;
                        false
                    }
                });
                match should_continue {
                    Ok(true) => {}
                    _ => break,
                }
            }
        })
        .detach();
    }

    /// Render the floating log recording toolbar at the top center of the terminal.
    fn render_log_recording_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use velowork_i18n::i18n;
        use velowork_ui::theme::with_alpha;
        let t = velowork_ui::theme::theme(cx);
        let p = SemanticPalette::from_theme(&t);

        // Check if morph entry animation is currently running from modal to capsule
        if let Some(start) = self.log_toolbar_start_time {
            let elapsed = start.elapsed();
            if elapsed < Duration::from_millis(280) {
                // Morph card is still travelling across screen; hide the pane toolbar
                return div().id("log-toolbar-hidden").into_any_element();
            }
        }

        let (anim_opacity, anim_offset_y) = if self.log_toolbar_exiting {
            if let Some(exit_start) = self.log_toolbar_exit_start_time {
                let t = (exit_start.elapsed().as_secs_f32() / 0.180).clamp(0.0, 1.0);
                let ease_t = velowork_ui::motion::ease_in_cubic(t);
                ((1.0 - ease_t).max(0.0), px(-16.0 * ease_t))
            } else {
                (0.0, px(-16.0))
            }
        } else {
            (1.0, px(0.0))
        };

        let terminal = self.terminal.as_ref().unwrap();
        let is_paused = terminal.is_log_recording_paused();
        let elapsed = terminal.get_log_recording_elapsed_secs();
        let hours = elapsed / 3600;
        let minutes = (elapsed % 3600) / 60;
        let seconds = elapsed % 60;
        let time_str = format!("{:02}:{:02}:{:02}", hours, minutes, seconds);

        let status_label = if is_paused {
            i18n!(cx, "toolbar.log.paused")
        } else {
            i18n!(cx, "toolbar.log.recording")
        };

        let pause_tooltip = i18n!(cx, "toolbar.log.pause");
        let resume_tooltip = i18n!(cx, "toolbar.log.resume");
        let stop_tooltip = i18n!(cx, "toolbar.log.stop");
        let cancel_tooltip = i18n!(cx, "toolbar.log.cancel");

        let pulse_active = !is_paused && (elapsed % 2 == 0);
        let dot_indicator = div()
            .w(px(14.0))
            .h(px(14.0))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .when(!is_paused, |d| {
                d.bg(with_alpha(t.error, if pulse_active { 0.28 } else { 0.10 }))
            })
            .child(
                div()
                    .w(px(8.0))
                    .h(px(8.0))
                    .rounded_full()
                    .bg(if is_paused {
                        p.text_muted
                    } else {
                        p.status_error
                    })
                    .when(pulse_active, |d| d.opacity(0.85)),
            );

        let this_entity = cx.entity().downgrade();
        let bounds_tracker = canvas(
            move |bounds, _window, cx| {
                if let Some(entity) = this_entity.upgrade() {
                    entity.update(cx, |this, _| {
                        this.log_toolbar_bounds = Some(bounds);
                    });
                }
            },
            |_, _, _, _| {},
        );

        let toolbar_content = div()
            .flex()
            .items_center()
            .gap(SPACE_MD)
            .px(SPACE_LG)
            .py(SPACE_SM)
            .bg(p.surface_overlay)
            .border_1()
            .border_color(if is_paused { p.border_subtle } else { with_alpha(t.error, 0.35) })
            .rounded(RADIUS_LG)
            .shadow_xl()
            .opacity(anim_opacity)
            .relative()
            .top(anim_offset_y)
            .child(bounds_tracker.absolute().inset_0())
            // Draggable drag handle / info section
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(SPACE_MD)
                    .cursor_move()
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                        cx.stop_propagation();
                        if let Some(tb) = this.log_toolbar_bounds {
                            let offset_in_toolbar = point(
                                event.position.x - tb.origin.x,
                                event.position.y - tb.origin.y,
                            );
                            this.log_toolbar_mouse_offset = Some(offset_in_toolbar);
                        } else {
                            this.log_toolbar_mouse_offset = Some(point(px(50.0), px(16.0)));
                        }
                        cx.notify();
                    }))
                    // Dynamic breathing / halo dot
                    .child(dot_indicator)
                    // Status label
                    .child(
                        div()
                            .text_size(ui_text_md(cx))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.text_primary)
                            .child(status_label)
                    )
                    // Elapsed time
                    .child(
                        div()
                            .text_size(ui_text_ms(cx))
                            .font_family("JetBrains Mono")
                            .text_color(p.text_muted)
                            .px(SPACE_SM)
                            .py(px(2.0))
                            .bg(surface_bg_t(t.bg_hover, &t))
                            .rounded(RADIUS_MD)
                            .child(time_str)
                    )
            )
            // Separator
            .child(
                div()
                    .w(px(1.0))
                    .h(px(16.0))
                    .bg(p.border_subtle)
            )
            // Pause/Resume button
            .child(
                if is_paused {
                    icon_button_sized("log-toolbar-resume", AppIcon::Play, 24.0, 14.0, &t)
                        .tooltip(move |_, cx| { let __tip = resume_tooltip.clone(); cx.new(|_| Tooltip::new(__tip)).into() })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _, _window, cx| {
                            if let Some(ref terminal) = this.terminal {
                                terminal.resume_log_recording();
                            }
                            cx.notify();
                        }))
                } else {
                    icon_button_sized("log-toolbar-pause", AppIcon::Pause, 24.0, 14.0, &t)
                        .tooltip(move |_, cx| { let __tip = pause_tooltip.clone(); cx.new(|_| Tooltip::new(__tip)).into() })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _, _window, cx| {
                            if let Some(ref terminal) = this.terminal {
                                terminal.pause_log_recording();
                            }
                            cx.notify();
                        }))
                }
            )
            // Stop button (red accent)
            .child(
                div()
                    .id("log-toolbar-stop")
                    .w(px(24.0))
                    .h(px(24.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(RADIUS_STD)
                    .cursor_pointer()
                    .hover(move |s| s.bg(with_alpha(t.error, 0.15)))
                    .tooltip(move |_, cx| { let __tip = stop_tooltip.clone(); cx.new(|_| Tooltip::new(__tip)).into() })
                    .child(AppIcon::Stop.size(ICON_STD).text_color(p.status_error))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, _window, cx| {
                        if let Some(ref terminal_id) = this.terminal_id {
                            this.request_broker.update(cx, |broker, cx| {
                                broker.push_overlay_request(
                                    velowork_workspace::requests::OverlayRequest::Project(
                                        velowork_workspace::requests::ProjectOverlay {
                                            project_id: this.project_id.clone(),
                                            kind: velowork_workspace::requests::ProjectOverlayKind::TerminalLogStop {
                                                terminal_id: terminal_id.clone(),
                                            },
                                        }
                                    ),
                                    cx,
                                );
                            });
                        }
                    }))
            )
            // Cancel button
            .child(
                icon_button_sized("log-toolbar-cancel", AppIcon::Close, 24.0, 14.0, &t)
                    .tooltip(move |_, cx| { let __tip = cancel_tooltip.clone(); cx.new(|_| Tooltip::new(__tip)).into() })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, _window, cx| {
                        if this.log_toolbar_exiting {
                            return;
                        }
                        this.log_toolbar_exiting = true;
                        this.log_toolbar_exit_start_time = Some(std::time::Instant::now());
                        cx.notify();

                        let this_weak = cx.entity().downgrade();
                        cx.spawn(async move |_, cx| {
                            for _ in 0..18 {
                                smol::Timer::after(Duration::from_millis(10)).await;
                                if this_weak.update(cx, |_, cx| cx.notify()).is_err() {
                                    return;
                                }
                            }
                            let _ = this_weak.update(cx, |this, cx| {
                                if let Some(ref terminal) = this.terminal {
                                    let path = terminal.stop_log_recording();
                                    // Delete the file on cancel
                                    if let Some(p) = path {
                                        let _ = std::fs::remove_file(&p);
                                    }
                                }
                                this.log_toolbar_exiting = false;
                                this.log_toolbar_exit_start_time = None;
                                this.log_toolbar_start_time = None;
                                cx.notify();
                            });
                        })
                        .detach();
                    }))
            );

        if let Some(pos) = self.log_toolbar_pos {
            div()
                .absolute()
                .top(pos.y)
                .left(pos.x)
                .child(toolbar_content)
                .into_any_element()
        } else {
            div()
                .absolute()
                .top(SPACE_XS)
                .left(px(0.0))
                .right(px(0.0))
                .flex()
                .justify_center()
                .child(toolbar_content)
                .into_any_element()
        }
    }

    fn create_terminal_for_existing_pty(&mut self, terminal_id: String, cx: &mut Context<Self>) {
        let existing = self.terminals.lock().get(&terminal_id).cloned();
        if let Some(terminal) = existing {
            if let Some(pid) = self.backend.get_shell_pid(&terminal_id) {
                terminal.set_shell_pid(pid);
            }
            self.terminal = Some(terminal.clone());
            self.update_child_terminals(terminal, cx);
            return;
        }

        // On app startup, do NOT reconnect/reopen previously-open terminal
        // sessions (the sessions that were active before the app closed).
        // Start a fresh shell instead so the previous session is not reopened.
        if !terminal_view_settings(cx).restore_terminals_on_startup {
            self.create_new_terminal(cx);
            return;
        }

        let settings = terminal_view_settings(cx);
        let ws = self.workspace.read(cx);
        let shell = self.shell_type.clone().resolve_default(
            ws.project(&self.project_id)
                .and_then(|p| p.default_shell.as_ref()),
            &settings.default_shell,
        );

        match self
            .backend
            .reconnect_terminal(&terminal_id, &self.project_path, Some(&shell))
        {
            Ok(_) => {}
            Err(e) => {
                log::error!("[views:terminal] Failed to reconnect terminal | terminal_id={} | error: {:#}", terminal_id, e);
                velowork_workspace::toast::ToastManager::error(
                    format!("Failed to reconnect terminal: {}", e),
                    cx,
                );
            }
        }

        let resolved = self.resolve_terminal_config(Some(&terminal_id), cx);
        let size = TerminalSize::default();
        let terminal = Arc::new(Terminal::new_with_options(
            terminal_id.clone(),
            size,
            self.backend.transport(),
            self.project_path.clone(),
            resolved.scrollback_lines as usize,
            Some(resolved.word_separators.clone()),
        ));
        if let Some(pid) = self.backend.get_foreground_shell_pid(&terminal_id) {
            terminal.set_shell_pid(pid);
        }
        self.terminals.lock().insert(terminal_id, terminal.clone());
        self.terminal = Some(terminal.clone());
        self.update_child_terminals(terminal, cx);

        // Auto-focus the reconnected terminal on startup, just like connecting
        // to a session does: point the FocusManager at this pane so its next
        // render claims physical focus.
        let pid = self.project_id.clone();
        let path = self.layout_path.clone();
        self.focus_manager.update(cx, |fm, _| {
            fm.focus_terminal(pid.clone(), path.clone());
        });
        self.workspace.update(cx, |ws, cx| {
            ws.touch_project(&pid);
            ws.bump_activity(&pid, cx);
            cx.notify();
        });
    }

    fn create_new_terminal(&mut self, cx: &mut Context<Self>) {
        if self.backend.is_remote() {
            return;
        }

        let settings = terminal_view_settings(cx);
        let ws = self.workspace.read(cx);
        let shell = self.shell_type.clone().resolve_default(
            ws.project(&self.project_id)
                .and_then(|p| p.default_shell.as_ref()),
            &settings.default_shell,
        );

        // Read fresh path from workspace state
        let project_path = ws
            .project(&self.project_id)
            .map(|p| p.path.clone())
            .unwrap_or_else(|| self.project_path.clone());

        match self.backend.create_terminal(&project_path, Some(&shell)) {
            Ok(terminal_id) => {
                self.terminal_id = Some(terminal_id.clone());
                self.workspace.update(cx, |ws, cx| {
                    ws.set_terminal_id(
                        &self.project_id,
                        &self.layout_path,
                        terminal_id.clone(),
                        cx,
                    );
                });

                let resolved = self.resolve_terminal_config(Some(&terminal_id), cx);
                let size = TerminalSize::default();
                let terminal = Arc::new(Terminal::new_with_options(
                    terminal_id.clone(),
                    size,
                    self.backend.transport(),
                    project_path,
                    resolved.scrollback_lines as usize,
                    Some(resolved.word_separators.clone()),
                ));
                if let Some(pid) = self.backend.get_shell_pid(&terminal_id) {
                    terminal.set_shell_pid(pid);
                }
                self.terminals
                    .lock()
                    .insert(terminal_id.clone(), terminal.clone());
                self.terminal = Some(terminal.clone());

                self.update_child_terminals(terminal, cx);

                // Auto-focus the freshly created terminal: point the FocusManager at this pane.
                let pid = self.project_id.clone();
                let path = self.layout_path.clone();
                self.focus_manager.update(cx, |fm, _| {
                    fm.focus_terminal(pid.clone(), path.clone());
                });
                self.workspace.update(cx, |ws, cx| {
                    ws.touch_project(&pid);
                    ws.bump_activity(&pid, cx);
                    cx.notify();
                });
                cx.notify();
            }
            Err(e) => {
                log::error!("[views:terminal] Failed to create terminal | error: {:#}", e);
                crate::toast_error(format!("Failed to create terminal: {}", e), cx);
            }
        }
    }

    fn update_child_terminals(&mut self, terminal: Arc<Terminal>, cx: &mut Context<Self>) {
        let is_remote = self.shell_type.is_remote()
            || self.backend.is_remote()
            || self
                .workspace
                .read(cx)
                .project(&self.project_id)
                .map_or(false, |p| p.is_remote);
        terminal.set_remote(is_remote);

        crate::register_content_pane(terminal.terminal_id.clone(), self.content.downgrade());

        self.content.update(cx, |content, cx| {
            content.set_terminal(Some(terminal.clone()), cx);
        });
        self.search_bar.update(cx, |search_bar, _| {
            search_bar.set_terminal(Some(terminal));
        });
    }

    pub fn terminal_id(&self) -> Option<String> {
        self.terminal_id.clone()
    }

    pub fn terminal_arc(&self) -> Option<Arc<Terminal>> {
        self.terminal.clone()
    }

    pub fn set_detached(&mut self, detached: bool, cx: &mut Context<Self>) {
        if self.detached != detached {
            self.detached = detached;
            if detached {
                self.deregister_resize_viewer(cx);
            }
            cx.notify();
        }
    }

    /// Explicitly clear any ZMODEM cooldown and suppression (e.g. when user submits a new command).
    pub fn clear_zmodem_cooldown(&mut self) {
        if self.last_zmodem_close_time.take().is_some() {
            log::info!("[ZMODEM-PANE] clear_zmodem_cooldown triggered: reset last_zmodem_close_time");
        }
        if let Some(ref term) = self.terminal {
            term.clear_zmodem_suppression();
        }
    }

    pub fn is_zmodem_active(&self) -> bool {
        self.zmodem_cancel_flag.is_some()
            || self.is_zmodem_prompt_open
            || self.terminal.as_ref().is_some_and(|t| t.is_zmodem_active())
    }

    pub fn process_zmodem_events(&mut self, cx: &mut Context<Self>) {
        let Some(ref terminal) = self.terminal else {
            return;
        };
        // If a transfer is active or a file prompt is open, ignore events
        if self.zmodem_cancel_flag.is_some() || self.is_zmodem_prompt_open {
            terminal.take_zmodem_events();
            return;
        }
        // Cooldown guard: discard trailing packets for 1.5 seconds after closing/cancelling
        if let Some(last_close) = self.last_zmodem_close_time
            && last_close.elapsed() < std::time::Duration::from_millis(1500)
        {
            terminal.take_zmodem_events();
            return;
        }
        if terminal.is_zmodem_suppressed() {
            terminal.take_zmodem_events();
            return;
        }
        let events = terminal.take_zmodem_events();
        for ev in events {
            match ev {
                velowork_terminal::terminal::ZmodemEvent::UploadRequested => {
                    self.handle_zmodem_upload_requested(cx);
                }
                velowork_terminal::terminal::ZmodemEvent::DownloadRequested => {
                    self.handle_zmodem_download_requested(cx);
                }
            }
        }
    }

    pub fn handle_zmodem_upload_requested(&mut self, cx: &mut Context<Self>) {
        log::info!("[ZMODEM] Upload requested by remote");
        if self.is_zmodem_prompt_open {
            return;
        }
        let Some(terminal) = self.terminal.clone() else {
            return;
        };
        let pending = terminal.take_pending_upload_files();
        if !pending.is_empty() {
            self.start_zmodem_upload(pending, cx);
        } else {
            self.is_zmodem_prompt_open = true;
            let prompt_fut = cx.prompt_for_paths(gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple: true,
                prompt: Some(i18n!(cx, "terminal.zmodem.select_files").into()),
            });
            cx.spawn(async move |this: WeakEntity<Self>, cx| {
                let selected_opt = match prompt_fut.await {
                    Ok(Ok(Some(selected))) if !selected.is_empty() => Some(selected),
                    _ => None,
                };
                let _ = this.update(cx, |pane, cx| {
                    pane.is_zmodem_prompt_open = false;
                    let Some(selected) = selected_opt else {
                        log::info!("[ZMODEM] File chooser cancelled by user");
                        pane.zmodem_aborted_notified.store(true, std::sync::atomic::Ordering::SeqCst);
                        if let Some(ref term) = pane.terminal {
                            term.write_to_screen(b"\r\nTransfer cancelled.\r\n");
                        }
                        pane.cancel_zmodem(cx);
                        return;
                    };
                    pane.start_zmodem_upload(selected, cx);
                });
            })
            .detach();
        }
    }

    pub fn start_zmodem_upload(&mut self, files: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(terminal) = self.terminal.clone() else {
            return;
        };
        if files.is_empty() {
            terminal.cancel_zmodem();
            return;
        }

        let first_name = files
            .first()
            .and_then(|f| f.file_name())
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        terminal.write_to_screen(format!("\r\nSending: {}\r\n", first_name).as_bytes());

        let _total_files = files.len();
        let total_bytes: u64 = files
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum();
        let cancel_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.zmodem_cancel_flag = Some(cancel_flag.clone());
        self.zmodem_aborted_notified.store(false, std::sync::atomic::Ordering::SeqCst);

        let overwrite = cx
            .try_global::<velowork_app_core::settings::GlobalSettings>()
            .map(|g| g.0.read(cx).settings.zmodem_upload_overwrite)
            .unwrap_or(false);

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        terminal.register_zmodem_raw_sender(tx);

        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        let event_tx_upload = event_tx.clone();
        let cancel_flag_prog = cancel_flag.clone();
        let upload_fut = velowork_terminal::zmodem::session::send_zmodem_upload_with_progress(
            terminal.clone(),
            files,
            overwrite,
            rx,
            cancel_flag,
            move |event| {
                let _ = event_tx_upload.send(event);
            },
        );

        let term_for_prog = terminal.clone();
        let mut last_render = std::time::Instant::now();
        let mut current_fname = first_name;
        let transfer_start = std::time::Instant::now();

        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            while let Some(event) = event_rx.recv().await {
                if cancel_flag_prog.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                match event {
                    velowork_terminal::zmodem::ZmodemUploadEvent::Progress {
                        transferred,
                        total,
                        speed,
                        filename,
                        ..
                    } => {
                        if filename != current_fname {
                            term_for_prog.write_to_screen(format!("\r\nSending: {}\r\n", filename).as_bytes());
                            current_fname = filename;
                        }
                        let percent = (transferred * 100).checked_div(total).unwrap_or(0);
                        let elapsed = transfer_start.elapsed().as_secs();
                        if last_render.elapsed() >= std::time::Duration::from_millis(50) {
                            let line = format_zmodem_progress_bar(percent, elapsed, speed, total, false);
                            term_for_prog.write_to_screen(line.as_bytes());
                            last_render = std::time::Instant::now();
                        }
                    }
                    velowork_terminal::zmodem::ZmodemUploadEvent::FileSkipped { filename, reason } => {
                        let text = this.update(cx, |_, cx| {
                            match reason {
                                velowork_terminal::zmodem::ZmodemSkipReason::Protected => {
                                    i18n!(cx, "terminal.zmodem.skipped_protected").replace("{}", &filename)
                                }
                                velowork_terminal::zmodem::ZmodemSkipReason::FileExists => {
                                    i18n!(cx, "terminal.zmodem.skipped_exists").replace("{}", &filename)
                                }
                            }
                        }).unwrap_or_else(|_| {
                            match reason {
                                velowork_terminal::zmodem::ZmodemSkipReason::Protected => {
                                    format!("Rejected: {} (protected / permission denied)", filename)
                                }
                                velowork_terminal::zmodem::ZmodemSkipReason::FileExists => {
                                    format!("Skipped: {} (already exists)", filename)
                                }
                            }
                        });
                        term_for_prog.write_to_screen(format!("\r\x1b[2K[ZMODEM] {}\r\n", text).as_bytes());
                    }
                }
            }
        })
        .detach();

        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        let term_for_unreg = terminal.clone();
        velowork_terminal::get_tokio_runtime().spawn(async move {
            let res = upload_fut.await;
            drop(event_tx);
            term_for_unreg.unregister_zmodem_raw_sender();
            let is_err = res.is_err();
            let _ = result_tx.send(res);
            if is_err {
                tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                term_for_unreg.send_bytes(b"\r");
            }
        });

        let term_for_res = terminal.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let res = match result_rx.await {
                Ok(r) => r,
                Err(_) => Err("Upload task terminated".to_string()),
            };

            let _ = this.update(cx, |pane, cx| {
                pane.zmodem_cancel_flag = None;
                pane.last_zmodem_close_time = Some(std::time::Instant::now());
                match res {
                    Ok(summary) => {
                        if summary.transferred_files > 0 {
                            let final_line = format_zmodem_progress_bar(
                                100,
                                transfer_start.elapsed().as_secs(),
                                0.0,
                                summary.transferred_bytes,
                                true,
                            );
                            term_for_res.write_to_screen(final_line.as_bytes());
                        } else if !summary.skipped_files.is_empty() {
                            let msg = format!(
                                "\r\x1b[2K[ZMODEM] {}\r\n\r\n",
                                i18n!(cx, "terminal.zmodem.all_skipped_or_protected")
                            );
                            term_for_res.write_to_screen(msg.as_bytes());
                        } else {
                            let final_line = format_zmodem_progress_bar(
                                100,
                                transfer_start.elapsed().as_secs(),
                                0.0,
                                total_bytes,
                                true,
                            );
                            term_for_res.write_to_screen(final_line.as_bytes());
                        }
                    }
                    Err(err) => {
                        if err.contains("cancelled") || err.contains("aborted") {
                            if pane
                                .zmodem_aborted_notified
                                .compare_exchange(
                                    false,
                                    true,
                                    std::sync::atomic::Ordering::SeqCst,
                                    std::sync::atomic::Ordering::SeqCst,
                                )
                                .is_ok()
                            {
                                term_for_res.write_to_screen(b"\r\x1b[2KTransfer aborted.\r\n\x1b[0m\x0f");
                            }
                        } else {
                            pane.zmodem_aborted_notified.store(true, std::sync::atomic::Ordering::SeqCst);
                            term_for_res.write_to_screen(
                                format!("\r\x1b[2KError: {}\r\nTransfer aborted.\r\n", err).as_bytes(),
                            );
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn handle_zmodem_download_requested(&mut self, cx: &mut Context<Self>) {
        log::info!("[ZMODEM] Download requested by remote");
        if self.terminal.is_none() {
            return;
        }

        let app_settings = cx
            .try_global::<velowork_app_core::settings::GlobalSettings>()
            .map(|g| g.0.read(cx).settings.clone());
        let auto_download = app_settings
            .as_ref()
            .map(|s| s.zmodem_auto_download)
            .unwrap_or(true);
        let default_dir = app_settings
            .as_ref()
            .and_then(|s| s.zmodem_download_directory.as_ref())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::download_dir().unwrap_or_else(std::env::temp_dir)
            });

        if auto_download {
            self.start_zmodem_download(default_dir, cx);
        } else {
            if self.is_zmodem_prompt_open {
                return;
            }
            self.is_zmodem_prompt_open = true;
            let prompt_fut = cx.prompt_for_paths(gpui::PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: Some(i18n!(cx, "terminal.zmodem.select_download_dir").into()),
            });
            cx.spawn(async move |this: WeakEntity<Self>, cx| {
                let chosen_dir_opt = match prompt_fut.await {
                    Ok(Ok(Some(mut dirs))) if !dirs.is_empty() => Some(dirs.remove(0)),
                    _ => None,
                };
                let _ = this.update(cx, |pane, cx| {
                    pane.is_zmodem_prompt_open = false;
                    let Some(chosen_dir) = chosen_dir_opt else {
                        log::info!("[ZMODEM] Download dir chooser cancelled by user");
                        pane.zmodem_aborted_notified.store(true, std::sync::atomic::Ordering::SeqCst);
                        if let Some(ref term) = pane.terminal {
                            term.write_to_screen(b"\r\nTransfer cancelled.\r\n");
                        }
                        pane.cancel_zmodem(cx);
                        return;
                    };
                    pane.start_zmodem_download(chosen_dir, cx);
                });
            })
            .detach();
        }
    }

    pub fn start_zmodem_download(&mut self, target_dir: PathBuf, cx: &mut Context<Self>) {
        let Some(terminal) = self.terminal.clone() else {
            return;
        };

        let cancel_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.zmodem_cancel_flag = Some(cancel_flag.clone());
        self.zmodem_aborted_notified.store(false, std::sync::atomic::Ordering::SeqCst);

        terminal.write_to_screen(b"\r\nReceiving file...\r\n");

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        terminal.register_zmodem_raw_sender(tx);

        let (prog_tx, mut prog_rx) = tokio::sync::mpsc::unbounded_channel();
        let prog_tx_dl = prog_tx.clone();
        let cancel_flag_prog = cancel_flag.clone();
        let dl_fut = velowork_terminal::zmodem::session::receive_zmodem_download_with_progress(
            terminal.clone(),
            target_dir,
            rx,
            cancel_flag,
            move |transferred, total, speed, filename, file_idx, total_files| {
                let _ = prog_tx_dl.send((
                    transferred,
                    total,
                    speed,
                    filename.to_string(),
                    file_idx,
                    total_files,
                ));
            },
        );

        let term_for_prog = terminal.clone();
        let mut last_render = std::time::Instant::now();
        let mut current_fname = String::new();
        let transfer_start = std::time::Instant::now();

        cx.spawn(async move |_this: WeakEntity<Self>, _cx| {
            while let Some((transferred, total, speed, filename, _file_idx, _total_files)) =
                prog_rx.recv().await
            {
                if cancel_flag_prog.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                if !filename.is_empty() && filename != current_fname {
                    term_for_prog.write_to_screen(format!("\r\x1b[2KReceiving: {}\r\n", filename).as_bytes());
                    current_fname = filename;
                }
                let percent = (transferred * 100).checked_div(total).unwrap_or(0);
                let elapsed = transfer_start.elapsed().as_secs();
                if last_render.elapsed() >= std::time::Duration::from_millis(50) {
                    let line = format_zmodem_progress_bar(percent, elapsed, speed, total, false);
                    term_for_prog.write_to_screen(line.as_bytes());
                    last_render = std::time::Instant::now();
                }
            }
        })
        .detach();

        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        let term_for_unreg = terminal.clone();
        velowork_terminal::get_tokio_runtime().spawn(async move {
            let res = dl_fut.await;
            drop(prog_tx);
            term_for_unreg.unregister_zmodem_raw_sender();
            let is_err = res.is_err();
            let _ = result_tx.send(res);
            if is_err {
                tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                term_for_unreg.send_bytes(b"\r");
            }
        });

        let term_for_res = terminal.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let res = match result_rx.await {
                Ok(r) => r,
                Err(_) => Err("Download task terminated".to_string()),
            };

            let _ = this.update(cx, |pane, cx| {
                pane.zmodem_cancel_flag = None;
                pane.last_zmodem_close_time = Some(std::time::Instant::now());
                match res {
                    Ok(saved_path) => {
                        let final_line = format!(
                            "\r\x1b[2K 100% [{}] {:02}:{:02}:{:02}\r\nSaved to: {}\r\n\r\n",
                            "=".repeat(20),
                            transfer_start.elapsed().as_secs() / 3600,
                            (transfer_start.elapsed().as_secs() % 3600) / 60,
                            transfer_start.elapsed().as_secs() % 60,
                            saved_path.display()
                        );
                        term_for_res.write_to_screen(final_line.as_bytes());
                    }
                    Err(err) => {
                        if err.contains("cancelled") || err.contains("aborted") || err.contains("terminated") {
                            if pane
                                .zmodem_aborted_notified
                                .compare_exchange(
                                    false,
                                    true,
                                    std::sync::atomic::Ordering::SeqCst,
                                    std::sync::atomic::Ordering::SeqCst,
                                )
                                .is_ok()
                            {
                                term_for_res.write_to_screen(b"\r\x1b[2KTransfer aborted.\r\n\x1b[0m\x0f");
                            }
                        } else if pane
                            .zmodem_aborted_notified
                            .compare_exchange(
                                false,
                                true,
                                std::sync::atomic::Ordering::SeqCst,
                                std::sync::atomic::Ordering::SeqCst,
                            )
                            .is_ok()
                        {
                            term_for_res.write_to_screen(
                                format!("\r\x1b[2KError: {}\r\nTransfer aborted.\r\n\x1b[0m\x0f", err).as_bytes(),
                            );
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn cancel_zmodem(&mut self, cx: &mut Context<Self>) {
        log::info!("[ZMODEM-PANE] cancel_zmodem called: cancelling transfer and resetting state");
        let had_task = self.zmodem_cancel_flag.is_some();
        let was_active = had_task || self.is_zmodem_prompt_open;
        if let Some(ref flag) = self.zmodem_cancel_flag.take() {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.is_zmodem_prompt_open = false;
        self.input_line_buffer.clear();
        self.last_zmodem_close_time = Some(std::time::Instant::now());
        if let Some(ref terminal) = self.terminal {
            if was_active
                && self
                    .zmodem_aborted_notified
                    .compare_exchange(
                        false,
                        true,
                        std::sync::atomic::Ordering::SeqCst,
                        std::sync::atomic::Ordering::SeqCst,
                    )
                    .is_ok()
            {
                terminal.write_to_screen(b"\r\x1b[2KTransfer aborted.\r\n\x1b[0m\x0f");
            }
            // CRITICAL: If a background transfer task is active, flag.store(true) will trigger
            // abort_and_drain_stream in that task, which drains in-flight binary bytes via raw_rx
            // before unregistering zmodem_raw_sender. Calling terminal.cancel_zmodem() now would
            // prematurely destroy zmodem_raw_sender and spill in-flight binary packets to the screen!
            if !had_task {
                terminal.cancel_zmodem();
            }
        }
        cx.notify();
    }


    pub fn set_minimized(&mut self, minimized: bool, cx: &mut Context<Self>) {
        if self.minimized != minimized {
            self.minimized = minimized;
            if minimized {
                self.deregister_resize_viewer(cx);
            }
            cx.notify();
        }
    }

    pub(super) fn deregister_resize_viewer(&mut self, cx: &mut Context<Self>) {
        self.content.update(cx, |content, _| {
            content.deregister_resize_viewer();
        });
    }

    fn id_suffix(&self) -> String {
        self.terminal_id.clone().unwrap_or_else(|| {
            format!(
                "{}-{}",
                self.project_id,
                self.layout_path
                    .iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join("-")
            )
        })
    }
}

impl<D: ActionDispatch> Drop for TerminalPane<D> {
    fn drop(&mut self) {
        if let Some(ref terminal) = self.terminal
            && terminal.is_zmodem_active()
        {
            terminal.cancel_zmodem();
        }
        // Remove this pane from the spatial navigation map so stale entries
        // don't linger after a terminal is closed.
        crate::layout::navigation::deregister_pane_bounds(
            self.window_id,
            &self.project_id,
            &self.layout_path,
        );
    }
}

impl<D: ActionDispatch + Send + Sync> gpui::Focusable for TerminalPane<D> {
    fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}

fn format_zmodem_progress_bar(
    percent: u64,
    elapsed_secs: u64,
    speed_bps: f64,
    total_bytes: u64,
    is_complete: bool,
) -> String {
    let hrs = elapsed_secs / 3600;
    let mins = (elapsed_secs % 3600) / 60;
    let secs = elapsed_secs % 60;
    let time_str = format!("{:02}:{:02}:{:02}", hrs, mins, secs);

    if is_complete {
        let bar = "=".repeat(20);
        format!("\r\x1b[2K 100% [{}] {} {} bytes\r\n\r\n", bar, time_str, total_bytes)
    } else {
        let bar = if percent >= 100 {
            "=".repeat(20)
        } else {
            let filled = ((percent * 20) / 100).min(19) as usize;
            if filled == 0 {
                format!(">{}", " ".repeat(19))
            } else {
                format!("{}>{}", "=".repeat(filled), " ".repeat(19 - filled))
            }
        };

        let speed_str = if speed_bps >= 1024.0 * 1024.0 {
            format!("{:.2} MB/s", speed_bps / (1024.0 * 1024.0))
        } else if speed_bps >= 1024.0 {
            format!("{:.2} KB/s", speed_bps / 1024.0)
        } else {
            format!("{:.0} B/s", speed_bps)
        };

        format!("\r\x1b[2K{:3}% [{}] {}  {}", percent, bar, time_str, speed_str)
    }
}

