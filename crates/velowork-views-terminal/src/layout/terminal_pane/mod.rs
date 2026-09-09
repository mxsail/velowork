//! Terminal pane view - composition of child entity views.

mod actions;
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

pub use content::{TerminalContent, TerminalContentEvent};
use parking_lot::Mutex;

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
            input_line_buffer: String::new(),
            command_accumulator: Vec::new(),
            history_popup_items: Vec::new(),
            history_popup_selected: None,
            history_popup_open: false,
            welcome_selected_index: None,
            is_reconnecting: false,
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
            .relative()
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
                        if let Some(ref terminal) = this.terminal {
                            let path = terminal.stop_log_recording();
                            // Delete the file on cancel
                            if let Some(p) = path {
                                let _ = std::fs::remove_file(&p);
                            }
                        }
                        cx.notify();
                    }))
            );

        if let Some(pos) = self.log_toolbar_pos {
            div()
                .absolute()
                .top(pos.y)
                .left(pos.x)
                .child(toolbar_content)
        } else {
            div()
                .absolute()
                .top(SPACE_XS)
                .left(px(0.0))
                .right(px(0.0))
                .flex()
                .justify_center()
                .child(toolbar_content)
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

    pub fn set_detached(&mut self, detached: bool, cx: &mut Context<Self>) {
        if self.detached != detached {
            self.detached = detached;
            if detached {
                self.deregister_resize_viewer(cx);
            }
            cx.notify();
        }
    }

    pub fn process_zmodem_events(&self, cx: &mut Context<Self>) {
        if let Some(ref terminal) = self.terminal {
            let events = terminal.take_zmodem_events();
            let store = cx
                .global::<crate::transfer_store::GlobalTransferStore>()
                .0
                .clone();
            for ev in events {
                match ev {
                    velowork_terminal::terminal::ZmodemEvent::UploadRequested => {
                        let pending = terminal.take_pending_upload_files();
                        let term = terminal.clone();
                        let store = store.clone();
                        if !pending.is_empty() {
                            cx.spawn(async move |_this, cx| {
                            for p in pending {
                                let filename = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                                let file_size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                                let id = crate::transfer_store::next_transfer_id();
                                let _ = store.update(cx, |s, cx| {
                                    s.add(crate::transfer_store::TransferTask {
                                        id: id.clone(),
                                        name: filename.clone(),
                                        direction: crate::transfer_store::TransferDirection::Upload,
                                        local_path: p.to_string_lossy().to_string(),
                                        remote_path: format!("zmodem:{}", filename),
                                        total_bytes: file_size,
                                        transferred_bytes: 0,
                                        status: crate::transfer_store::TransferStatus::Active,
                                        speed_bps: 0.0,
                                        error: None,
                                    });
                                    cx.notify();
                                });

                                let progress = Arc::new(Mutex::new(crate::transfer_store::TransferProgress::default()));
                                let store_tick = store.clone();
                                let id_tick = id.clone();
                                let prog_tick = progress.clone();
                                cx.spawn(async move |cx| {
                                    loop {
                                        smol::Timer::after(std::time::Duration::from_millis(200)).await;
                                        let p = prog_tick.lock().clone();
                                        let _ = store_tick.update(cx, |s, cx| {
                                            if let Some(t) = s.tasks.iter_mut().find(|t| t.id == id_tick) {
                                                t.transferred_bytes = p.transferred;
                                                t.total_bytes = if p.total > 0 { p.total } else { file_size };
                                                t.speed_bps = p.speed_bps;
                                                if p.done {
                                                    t.status = p.status;
                                                    t.error = p.error.clone();
                                                }
                                            }
                                            cx.notify();
                                        });
                                        if p.done {
                                            break;
                                        }
                                    }
                                }).detach();

                                let prog_cb = progress.clone();
                                let res = velowork_terminal::zmodem::session::send_zmodem_upload_with_progress(
                                    term.clone(),
                                    p,
                                    move |transferred, total, speed| {
                                        let mut p = prog_cb.lock();
                                        p.transferred = transferred;
                                        p.total = total;
                                        p.speed_bps = speed;
                                    },
                                ).await;

                                let mut p = progress.lock();
                                p.done = true;
                                p.transferred = file_size;
                                p.total = file_size;
                                p.status = if res.is_ok() {
                                    crate::transfer_store::TransferStatus::Complete
                                } else {
                                    crate::transfer_store::TransferStatus::Error
                                };
                                p.error = res.err();
                            }
                        }).detach();
                        } else {
                            let prompt_fut = cx.prompt_for_paths(gpui::PathPromptOptions {
                                files: true,
                                directories: false,
                                multiple: true,
                                prompt: Some("Select Files for Upload (rz)".into()),
                            });
                            cx.spawn(async move |_this, cx| {
                            if let Ok(Ok(Some(selected))) = prompt_fut.await {
                                for p in selected {
                                    let filename = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                                    let file_size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                                    let id = crate::transfer_store::next_transfer_id();
                                    let _ = store.update(cx, |s, cx| {
                                        s.add(crate::transfer_store::TransferTask {
                                            id: id.clone(),
                                            name: filename.clone(),
                                            direction: crate::transfer_store::TransferDirection::Upload,
                                            local_path: p.to_string_lossy().to_string(),
                                            remote_path: format!("zmodem:{}", filename),
                                            total_bytes: file_size,
                                            transferred_bytes: 0,
                                            status: crate::transfer_store::TransferStatus::Active,
                                            speed_bps: 0.0,
                                            error: None,
                                        });
                                        cx.notify();
                                    });

                                    let progress = Arc::new(Mutex::new(crate::transfer_store::TransferProgress::default()));
                                    let store_tick = store.clone();
                                    let id_tick = id.clone();
                                    let prog_tick = progress.clone();
                                    cx.spawn(async move |cx| {
                                        loop {
                                            smol::Timer::after(std::time::Duration::from_millis(200)).await;
                                            let p = prog_tick.lock().clone();
                                            let _ = store_tick.update(cx, |s, cx| {
                                                if let Some(t) = s.tasks.iter_mut().find(|t| t.id == id_tick) {
                                                    t.transferred_bytes = p.transferred;
                                                    t.total_bytes = if p.total > 0 { p.total } else { file_size };
                                                    t.speed_bps = p.speed_bps;
                                                    if p.done {
                                                        t.status = p.status;
                                                        t.error = p.error.clone();
                                                    }
                                                }
                                                cx.notify();
                                            });
                                            if p.done {
                                                break;
                                            }
                                        }
                                    }).detach();

                                    let prog_cb = progress.clone();
                                    let res = velowork_terminal::zmodem::session::send_zmodem_upload_with_progress(
                                        term.clone(),
                                        p,
                                        move |transferred, total, speed| {
                                            let mut p = prog_cb.lock();
                                            p.transferred = transferred;
                                            p.total = total;
                                            p.speed_bps = speed;
                                        },
                                    ).await;

                                    let mut p = progress.lock();
                                    p.done = true;
                                    p.transferred = file_size;
                                    p.total = file_size;
                                    p.status = if res.is_ok() {
                                        crate::transfer_store::TransferStatus::Complete
                                    } else {
                                        crate::transfer_store::TransferStatus::Error
                                    };
                                    p.error = res.err();
                                }
                            } else {
                                term.send_bytes(b"\x18\x18\x18\x18\x18\x08\x08\x08\x08\x08");
                            }
                        }).detach();
                        }
                    }
                    velowork_terminal::terminal::ZmodemEvent::DownloadRequested => {
                        let term = terminal.clone();
                        let store = store.clone();
                        let (tx, rx) = tokio::sync::mpsc::channel::<Vec<u8>>(200);
                        term.set_zmodem_raw_sender(Some(tx));

                        let download_dir = std::env::var("HOME")
                            .or_else(|_| std::env::var("USERPROFILE"))
                            .map(|h| std::path::PathBuf::from(h).join("Downloads"))
                            .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"));

                        let _ = std::fs::create_dir_all(&download_dir);

                        let id = crate::transfer_store::next_transfer_id();
                        let _ = store.update(cx, |s, cx| {
                            s.add(crate::transfer_store::TransferTask {
                                id: id.clone(),
                                name: "Download (sz)".to_string(),
                                direction: crate::transfer_store::TransferDirection::Download,
                                local_path: download_dir.to_string_lossy().to_string(),
                                remote_path: "zmodem:remote".to_string(),
                                total_bytes: 0,
                                transferred_bytes: 0,
                                status: crate::transfer_store::TransferStatus::Active,
                                speed_bps: 0.0,
                                error: None,
                            });
                            cx.notify();
                        });

                        let progress = Arc::new(Mutex::new(
                            crate::transfer_store::TransferProgress::default(),
                        ));
                        let store_tick = store.clone();
                        let id_tick = id.clone();
                        let prog_tick = progress.clone();
                        cx.spawn(async move |_this, cx| {
                            loop {
                                smol::Timer::after(std::time::Duration::from_millis(200)).await;
                                let p = prog_tick.lock().clone();
                                let _ = store_tick.update(cx, |s, cx| {
                                    if let Some(t) = s.tasks.iter_mut().find(|t| t.id == id_tick) {
                                        t.transferred_bytes = p.transferred;
                                        if p.total > 0 {
                                            t.total_bytes = p.total;
                                        }
                                        t.speed_bps = p.speed_bps;
                                        if p.done {
                                            t.status = p.status;
                                            t.error = p.error.clone();
                                        }
                                    }
                                    cx.notify();
                                });
                                if p.done {
                                    break;
                                }
                            }
                        })
                        .detach();

                        let prog_cb = progress.clone();
                        let term_cleanup = term.clone();
                        let store_final = store.clone();
                        let id_final = id.clone();
                        cx.spawn(async move |_this, cx| {
                        let res = velowork_terminal::zmodem::session::receive_zmodem_download_with_progress(
                            term.clone(),
                            download_dir,
                            rx,
                            move |transferred, total, speed, _path| {
                                let mut p = prog_cb.lock();
                                p.transferred = transferred;
                                p.total = total;
                                p.speed_bps = speed;
                            },
                        ).await;

                        term_cleanup.set_zmodem_raw_sender(None);

                        let mut p = progress.lock();
                        p.done = true;
                        p.status = if res.is_ok() {
                            crate::transfer_store::TransferStatus::Complete
                        } else {
                            crate::transfer_store::TransferStatus::Error
                        };
                        p.error = res.as_ref().err().cloned();

                        if let Ok(ref saved_path) = res {
                            let final_name = saved_path.file_name().unwrap_or_default().to_string_lossy().to_string();
                            let final_local = saved_path.to_string_lossy().to_string();
                            let _ = store_final.update(cx, |s, cx| {
                                if let Some(t) = s.tasks.iter_mut().find(|t| t.id == id_final) {
                                    t.name = final_name;
                                    t.local_path = final_local;
                                    t.status = crate::transfer_store::TransferStatus::Complete;
                                }
                                cx.notify();
                            });
                        }
                    }).detach();
                    }
                }
            }
        }
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
