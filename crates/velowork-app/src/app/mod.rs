pub mod detached_overlays;
mod detached_terminals;
mod extras;
mod notifications;

pub use detached_overlays::{close_all_detached_windows, open_detached_overlay, DetachedOverlayOptions, DetachedWindowsRegistry};


use crate::views::panels::toast::ToastManager;
use crate::terminal::pty_manager::{PtyEvent, PtyManager};
use crate::views::window::{TerminalsRegistry, WindowView};
use crate::workspace::persistence;
use crate::workspace::state::{GlobalWorkspace, WindowId, Workspace, WorkspaceData};
use velowork_workspace::security;
use velowork_workspace::stores::GlobalConnectionStore;
use async_channel::Receiver;
use gpui::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// When terminals are removed from the layout/registry — whether by an explicit
/// soft (grace-period) close or because their PTY exited on its own — any SSH
/// session that no longer has *any* open terminal must be marked disconnected so
/// the session-tree icon reverts to its default (neutral) color.
///
/// `closed` is the list of `(terminal_id, ssh_session_id)` pairs for the
/// terminals being removed. The `ssh_session_id` MUST be captured *before* the
/// pty handle is reaped: the EXIT path calls `PtyManager::cleanup_exited`, which
/// removes the `PtyHandle` (and therefore its `ssh_session_id`) from the
/// registry. Reading the id afterwards via `get_ssh_session_id` would return
/// `None` and silently skip the disconnect — exactly the bug where a failed SSH
/// connection's tab, once closed, left its session-tree icon stuck in the
/// "connected" color.
///
/// Terminals named in `closed` are excluded from the "still open?" check since
/// their removal is already in flight.
fn disconnect_ssh_sessions_for_closed_terminals(
    pty_manager: &PtyManager,
    terminals: &TerminalsRegistry,
    closed: &[(String, Option<String>)],
    cx: &mut App,
) {
    let mut closed_sids: Vec<String> = Vec::new();
    for (_, sid) in closed {
        if let Some(sid) = sid {
            if !closed_sids.iter().any(|s| s == sid) {
                closed_sids.push(sid.clone());
            }
        }
    }
    if closed_sids.is_empty() {
        return;
    }
    let remaining: Vec<String> = terminals.lock().keys().cloned().collect();
    let closed_ids: Vec<&String> = closed.iter().map(|(tid, _)| tid).collect();
    for sid in &closed_sids {
        let still_open = remaining
            .iter()
            .filter(|t| !closed_ids.contains(t))
            .any(|tid| pty_manager.get_ssh_session_id(tid).as_deref() == Some(sid.as_str()));
        if !still_open {
            let store = cx.global::<GlobalConnectionStore>().0.clone();
            store.update(cx, |c, cx| c.mark_disconnected(sid, cx));
        }
    }
}

fn contains_screen_erase(data: &[u8]) -> bool {
    data.windows(3).any(|w| w == b"\x1b[J")
        || data.windows(4).any(|w| {
            w == b"\x1b[0J"
                || w == b"\x1b[1J"
                || w == b"\x1b[2J"
                || w == b"\x1b[3J"
        })
}

/// Main application state and view
pub struct Velowork {
    /// The single, always-present main window. Closing it quits the app
    /// (per the multi-window PRD's main-is-special invariant).
    main_window: Entity<WindowView>,
    /// OS window handle of the main window. Captured from `window.window_handle()`
    /// in `Velowork::new`'s `cx.open_window` build closure (see main.rs). Used by
    /// the remote-bridge command loop to resolve actions to the focused
    /// window's per-window `FocusManager` per PRD cri 13.
    pub(super) main_window_handle: AnyWindowHandle,
    /// Ephemeral extras spawned at runtime, keyed by `WindowId::Extra(uuid)`.
    /// Populated by the workspace observer in `handle_extra_windows_changed`
    /// when `WorkspaceData.extra_windows` gains a new entry; the matching
    /// `Entity<WindowView>` is created and inserted as part of the
    /// `cx.open_window` build closure (see `extras.rs`).
    extra_windows: HashMap<WindowId, Entity<WindowView>>,
    /// OS window handles for extras, keyed by `WindowId::Extra(uuid)`. Populated
    /// alongside `extra_windows` in `extras.rs::open_extra_window`. Same
    /// purpose as `main_window_handle` — focused-window resolution at the
    /// remote-bridge boundary (PRD cri 13).
    pub(super) extra_window_handles: HashMap<WindowId, AnyWindowHandle>,
    pub(crate) workspace: Entity<Workspace>,
    pub(crate) pty_manager: Arc<PtyManager>,
    pub(crate) terminals: TerminalsRegistry,
    /// Track which detached windows we've already opened
    pub(crate) opened_detached_windows: HashSet<String>,
    /// Flag indicating workspace needs to be saved (for debouncing)
    /// Note: Field is read by spawned tasks, not directly
    #[allow(dead_code)]
    save_pending: Arc<AtomicBool>,
    /// Sender handed to desktop-notification threads. When a user clicks an
    /// XDG notification, the thread sends a `NotificationJump` here and the
    /// click loop focuses the originating pane. See `app/notifications.rs`.
    notification_jump_tx: async_channel::Sender<notifications::NotificationJump>,
    /// Whether the app is currently locked (enhanced mode startup or idle timeout).
    locked: bool,
    /// Lock screen entity (created on demand when enhanced mode is enabled).
    lock_screen: Option<Entity<crate::views::overlays::lock_screen::LockScreen>>,
    /// Timestamp of last user activity (for idle lock timeout).
    last_activity: std::time::Instant,
}

impl Velowork {
    pub fn new(
        workspace_data: WorkspaceData,
        pty_manager: Arc<PtyManager>,
        pty_events: Receiver<PtyEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Create workspace entity
        let workspace = cx.new(|_cx| Workspace::new(workspace_data));
        cx.set_global(GlobalWorkspace(workspace.clone()));

        // Transfer store backing the status-bar transfer manager.
        let transfer_store = cx.new(|_cx| velowork_views_terminal::transfer_store::TransferStore::new());
        cx.set_global(velowork_views_terminal::transfer_store::GlobalTransferStore(
            transfer_store.clone(),
        ));

        // Shared flag for debounced save
        let save_pending = Arc::new(AtomicBool::new(false));
        // Track last saved data_version to skip saves for UI-only changes
        let last_saved_version = Arc::new(AtomicU64::new(0));

        // Set up debounced auto-save on workspace changes
        let save_pending_for_observer = save_pending.clone();
        let last_saved_version_for_observer = last_saved_version.clone();
        let workspace_for_save = workspace.clone();
        cx.observe(&workspace, move |_this, _workspace, cx| {
            // Check if persistent data actually changed
            let current_version = _workspace.read(cx).data_version();
            if current_version == last_saved_version_for_observer.load(Ordering::Relaxed) {
                return; // UI-only change, skip save
            }

            save_pending_for_observer.store(true, Ordering::Relaxed);

            let save_pending = save_pending_for_observer.clone();
            let last_saved = last_saved_version_for_observer.clone();
            let workspace = workspace_for_save.clone();
            cx.spawn(async move |_, cx| {
                smol::Timer::after(std::time::Duration::from_millis(300)).await;

                if save_pending.swap(false, Ordering::Relaxed) {
                    let (data, version) = cx.update(|cx| {
                        let _slow = velowork_core::timing::SlowGuard::new("workspace_save_clone");
                        let ws = workspace.read(cx);
                        (ws.data().clone(), ws.data_version())
                    });
                    // Run blocking fs IO off the GPUI main thread — on Windows
                    // an AV scan or OneDrive sync of workspace.json can stall
                    // for seconds and would otherwise freeze the UI.
                    let save_result = smol::unblock(move || persistence::save_workspace(&data)).await;
                    match save_result {
                        Ok(()) => {
                            last_saved.store(version, Ordering::Relaxed);
                        }
                        Err(e) => {
                            log::error!("[app] Failed to save workspace | error: {:#}", e);
                            cx.update(|cx| {
                                ToastManager::error(format!("Failed to save workspace: {}", e), cx);
                            });
                            // Don't update last_saved — next mutation will retry the save
                        }
                    }
                }
            }).detach();
        })
        .detach();

        // Shared terminals registry — one per Velowork instance, threaded into
        // every WindowView (main + extras). Each TerminalPane looks up the
        // existing Arc<Terminal> for its terminal_id from this registry; if
        // each window had its own registry, an extra rendering a project
        // already shown in main would create a NEW Terminal model and PTY
        // bytes (which feed the original Arc<Terminal>) would never reach
        // the extra's content pane.
        let terminals: TerminalsRegistry = Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new()));

        // Create the main window's per-window view, sharing the registry.
        let pty_manager_clone = pty_manager.clone();
        let terminals_for_main = terminals.clone();
        let main_window = cx.new(|cx| {
            WindowView::new(WindowId::Main, workspace.clone(), pty_manager_clone, terminals_for_main, window, cx)
        });

        // Listen for cross-window requests (e.g. "jump into a project's terminal"
        // from the Switch Project overlay). Velowork is the only place that holds
        // every window's view + OS handle, so it executes these.
        cx.subscribe(&main_window, Self::handle_window_view_event).detach();

        // Observe window bounds changes to force re-render
        cx.observe_window_bounds(window, |_this, _window, cx| {
            cx.notify();
        })
        .detach();

        // Channel for clicked desktop notifications → "jump to that pane".
        let (notification_jump_tx, notification_jump_rx) = async_channel::unbounded();

        let main_window_handle = window.window_handle();

        // Check if enhanced security mode is enabled AND a master password is
        // actually set — if so, start locked. Without a master password the lock
        // screen can never be unlocked, so we must not show it.
        let needs_lock = crate::settings::settings_entity(cx)
            .read(cx)
            .settings
            .security
            .security_mode == "enhanced"
            && security::is_master_password_set();

        let lock_screen = if needs_lock {
            Some(cx.new(|cx| crate::views::overlays::lock_screen::LockScreen::new(cx)))
        } else {
            None
        };

        if needs_lock {
            main_window.update(cx, |w, cx| w.set_locked(true, cx));
        }

        // Observe main_window so Velowork re-renders whenever child overlays/modals change
        cx.observe(&main_window, |_this, _, cx| {
            cx.notify();
        })
        .detach();

        let mut manager = Self {
            main_window,
            main_window_handle,
            extra_windows: HashMap::new(),
            extra_window_handles: HashMap::new(),
            workspace: workspace.clone(),
            pty_manager,
            terminals,
            opened_detached_windows: HashSet::new(),
            save_pending,
            notification_jump_tx,
            locked: needs_lock,
            lock_screen,
            last_activity: std::time::Instant::now(),
        };

        // Subscribe to lock screen unlock event
        if let Some(ref ls) = manager.lock_screen {
            cx.subscribe(ls, |this, _, event, cx| {
                let crate::views::overlays::lock_screen::LockScreenEvent::Unlocked = event;
                this.unlock_app(cx);
            }).detach();
        }

        // Start PTY event loop (centralized for all windows)
        manager.start_pty_event_loop(pty_events, cx);

        // Route clicked desktop notifications back to their originating pane.
        manager.start_notification_click_loop(notification_jump_rx, cx);

        // Start idle timeout monitor (checks every 10 seconds)
        cx.spawn(async move |this: WeakEntity<Velowork>, cx| {
            loop {
                smol::Timer::after(std::time::Duration::from_secs(10)).await;
                let _ = cx.update(|cx| {
                    if let Some(this) = this.upgrade() {
                        // Resolve the OS window handle so the idle lock can run
                        // in a real `Window` context and dismiss any open
                        // floating surfaces (e.g. an expanded dropdown) behind
                        // the lock screen.
                        let handle = this.read(cx).main_window_handle;
                        let _ = cx.update_window(handle, |_, window, cx| {
                            this.update(cx, |this, cx| {
                                this.check_idle_timeout(window, cx);
                            });
                        });
                    }
                });
            }
        })
        .detach();

        // Kill orphaned terminals when projects are deleted
        cx.observe(&workspace, move |this, workspace, cx| {
            let kills = workspace.update(cx, |ws, _| ws.drain_pending_terminal_kills());
            if !kills.is_empty() {
                // Mark SSH sessions disconnected *before* killing — kill() drops
                // the pty handle that carries the session id. Any session with no
                // other open terminal reverts its session-tree icon to default.
                let closed: Vec<(String, Option<String>)> = kills
                    .iter()
                    .map(|tid| (tid.clone(), this.pty_manager.get_ssh_session_id(tid)))
                    .collect();
                disconnect_ssh_sessions_for_closed_terminals(
                    this.pty_manager.as_ref(),
                    &this.terminals,
                    &closed,
                    cx,
                );
                let mut reg = this.terminals.lock();
                for tid in &kills {
                    this.pty_manager.kill(tid);
                    reg.remove(tid);
                }
                drop(reg);
                velowork_core::memory::trim_process_memory();
            }
        })
        .detach();

        // Flush soft-closed terminals on quit. Their grace timer can't fire once
        // the app is gone, so tear the PTYs down here — otherwise a terminal
        // closed seconds before quitting would leak its persistent (dtach/tmux)
        // session. on_app_quit fires for every exit path.
        cx.on_app_quit(move |this: &mut Self, cx| {
            let ids = this
                .workspace
                .update(cx, |ws, _| ws.drain_pending_closes());
            if !ids.is_empty() {
                let closed: Vec<(String, Option<String>)> = ids
                    .iter()
                    .map(|tid| (tid.clone(), this.pty_manager.get_ssh_session_id(tid)))
                    .collect();
                disconnect_ssh_sessions_for_closed_terminals(
                    this.pty_manager.as_ref(),
                    &this.terminals,
                    &closed,
                    cx,
                );
                let mut reg = this.terminals.lock();
                for tid in &ids {
                    this.pty_manager.kill(tid);
                    reg.remove(tid);
                }
            }
            async {}
        })
        .detach();

        // Set up observer for detached terminals
        cx.observe(&workspace, move |this, workspace, cx| {
            this.handle_detached_terminals_changed(workspace, cx);
        })
        .detach();

        // Open an OS window per fresh `WorkspaceData.extra_windows` entry —
        // slice 05 keystone. The data-layer `Workspace::spawn_extra_window`
        // mutation push fires this observer; the diff against
        // `Velowork.extra_windows` is the spawn signal.
        cx.observe(&workspace, |this, _workspace, cx| {
            this.handle_extra_windows_changed(cx);
        })
        .detach();

        // Scrub stale focus across every window's FocusManager on each
        // workspace change. Deleting a project from one window can leave
        // another window's focus pointing at a now-gone project; without
        // this, the orphaned window renders a ghost zoom of the deleted
        // project (or worse, panics on missing data).
        cx.observe(&workspace, |this, workspace, cx| {
            let valid_ids: HashSet<String> = workspace
                .read(cx)
                .projects()
                .iter()
                .map(|p| p.id.clone())
                .collect();
            let mut fms: Vec<Entity<crate::workspace::focus::FocusManager>> = Vec::with_capacity(1 + this.extra_windows.len());
            fms.push(this.main_window.read(cx).focus_manager());
            for view in this.extra_windows.values() {
                fms.push(view.read(cx).focus_manager());
            }
            for fm in fms {
                fm.update(cx, |fm, cx| {
                    if fm.clear_stale_focus(|id| valid_ids.contains(id)) {
                        cx.notify();
                    }
                });
            }
        })
        .detach();

        // Slice 07 cri 1: kick the extras observer once so persisted
        // `WorkspaceData.extra_windows` entries reopen at launch. The observer
        // above only fires when `workspace` notifies, but `Workspace::new` does
        // not notify on construction — without an explicit kick, persisted
        // extras would stay invisible until the user mutates the workspace.
        // Deferred via `cx.spawn` because `open_extra_window` captures
        // `cx.entity()` and calls `velowork.update` inside `cx.open_window`'s
        // build closure; running synchronously inside `Velowork::new` would touch
        // a half-constructed entity. By the time the spawned task body runs,
        // the entity is fully wrapped and `update` is safe.
        cx.spawn(async move |this: WeakEntity<Velowork>, cx| {
            let _ = this.update(cx, |this, cx| {
                this.handle_extra_windows_changed(cx);
            });
        })
        .detach();

        // Note: updater is now handled by velowork-updater.
        // GlobalUpdateInfo is set in main.rs via velowork_updater::init().

        manager
    }

    /// Centralized PTY event loop - notifies all windows (main and detached)
    fn start_pty_event_loop(
        &mut self,
        pty_events: Receiver<PtyEvent>,
        cx: &mut Context<Self>,
    ) {
        let terminals = self.terminals.clone();
        let pty_manager = self.pty_manager.clone();

        // Per-turn work budget. A single high-bandwidth terminal (cat hugefile,
        // `yes`, a runaway build log) can keep this loop draining the channel
        // forever, starving input/render/resize for ALL terminals (they all
        // funnel through this one loop on the GPUI thread). Once we've parsed
        // this many bytes in one drain pass we stop, yield to the executor so
        // input/render get scheduled, then loop back — the remaining events
        // stay in the bounded channel and are picked up next turn (nothing is
        // dropped). 256 KiB is a few render frames' worth of throughput while
        // staying small enough to keep the UI responsive under sustained load.
        const MAX_BYTES_PER_TURN: usize = 256 * 1024;

        cx.spawn(async move |this: WeakEntity<Velowork>, cx| {
            loop {
                let event = match pty_events.recv().await {
                    Ok(event) => event,
                    Err(_) => break,
                };

                let _slow = velowork_core::timing::SlowGuard::new("Velowork::pty_event_batch");

                // Collect exit events and track which terminals received data
                let mut exit_events: Vec<(String, Option<u32>)> = Vec::new();
                // Capture each exiting terminal's SSH session id *before*
                // `cleanup_exited` drops the PtyHandle (which carries it).
                let mut exited_sids: Vec<(String, Option<String>)> = Vec::new();
                let mut dirty_terminal_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

                // Bytes parsed so far in this drain pass (across batched events).
                let mut bytes_this_turn: usize = 0;

                let mut erase_coalesce_deadline = if match &event {
                    PtyEvent::Data { data, .. } => contains_screen_erase(data),
                    _ => false,
                } {
                    Some(std::time::Instant::now() + std::time::Duration::from_millis(16))
                } else {
                    None
                };

                // Process first event (broadcasting handled by PtyOutputSink in reader threads)
                match &event {
                    PtyEvent::Data { terminal_id, data } => {
                        let term = terminals.lock().get(terminal_id).cloned();
                        if let Some(term) = term {
                            bytes_this_turn += data.len();
                            term.process_output(data);
                        }
                        dirty_terminal_ids.insert(terminal_id.clone());
                    }
                    PtyEvent::Exit { terminal_id, exit_code } => {
                        exited_sids.push((
                            terminal_id.clone(),
                            pty_manager.get_ssh_session_id(terminal_id),
                        ));
                        pty_manager.cleanup_exited(terminal_id);
                        exit_events.push((terminal_id.clone(), *exit_code));
                        dirty_terminal_ids.insert(terminal_id.clone());
                    }
                }

                // Drain any additional pending events (batch processing), but
                // stop once we exceed the per-turn byte budget so we yield back
                // to the executor instead of monopolizing the GPUI thread.
                while bytes_this_turn < MAX_BYTES_PER_TURN {
                    let event = match pty_events.try_recv() {
                        Ok(event) => event,
                        Err(_) => {
                            if let Some(deadline) = erase_coalesce_deadline {
                                let now = std::time::Instant::now();
                                if now < deadline {
                                    let remaining = deadline - now;
                                    let wait_chunk = remaining.min(std::time::Duration::from_millis(4));
                                    smol::Timer::after(wait_chunk).await;
                                    match pty_events.try_recv() {
                                        Ok(ev) => ev,
                                        Err(_) => {
                                            let now2 = std::time::Instant::now();
                                            if now2 < deadline {
                                                let remaining2 = deadline - now2;
                                                smol::Timer::after(remaining2.min(std::time::Duration::from_millis(4))).await;
                                                match pty_events.try_recv() {
                                                    Ok(ev) => ev,
                                                    Err(_) => break,
                                                }
                                            } else {
                                                break;
                                            }
                                        }
                                    }
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    };
                    match &event {
                        PtyEvent::Data { terminal_id, data } => {
                            let term = terminals.lock().get(terminal_id).cloned();
                            if let Some(term) = term {
                                bytes_this_turn += data.len();
                                term.process_output(data);
                            }
                            if contains_screen_erase(data) {
                                erase_coalesce_deadline = Some(std::time::Instant::now() + std::time::Duration::from_millis(16));
                            }
                            dirty_terminal_ids.insert(terminal_id.clone());
                        }
                        PtyEvent::Exit { terminal_id, exit_code } => {
                            exited_sids.push((
                                terminal_id.clone(),
                                pty_manager.get_ssh_session_id(terminal_id),
                            ));
                            pty_manager.cleanup_exited(terminal_id);
                            exit_events.push((terminal_id.clone(), *exit_code));
                            dirty_terminal_ids.insert(terminal_id.clone());
                        }
                    }
                }

                // Notify main window after processing the batch
                let _ = this.update(cx, |this, cx| {
                    if !exit_events.is_empty() {
                        // Kill session backends and remove UI Terminals for exited terminals.
                        // This is critical for dtach: the PTY exit only means the client disconnected,
                        // but the dtach daemon keeps running. kill() ensures kill_session() is called
                        // to SIGTERM the daemon and remove the socket file.
                        {
                            // Mark their SSH sessions disconnected before killing the pty handles
                            // so the session-tree icon reverts if this was the last terminal for that session.
                            if !exited_sids.is_empty() {
                                disconnect_ssh_sessions_for_closed_terminals(
                                    this.pty_manager.as_ref(),
                                    &this.terminals,
                                    &exited_sids,
                                    cx,
                                );
                            }
                            let mut reg = this.terminals.lock();
                            for (terminal_id, _) in &exit_events {
                                this.pty_manager.kill(terminal_id);
                                reg.remove(terminal_id);
                            }
                        }

                        // If any exited terminal was mid soft-close, its undo toast
                        // is now useless (the PTY is gone) and the pending record
                        // would otherwise linger until the grace timer fired a
                        // redundant kill — drop both now.
                        let stale_toasts: Vec<String> = this.workspace.update(cx, |ws, _| {
                            exit_events
                                .iter()
                                .filter_map(|(tid, _)| ws.cancel_pending_close(tid))
                                .collect()
                        });
                        for toast_id in &stale_toasts {
                            crate::workspace::toast::ToastManager::dismiss(toast_id, cx);
                        }

                        // If an exited terminal had just been *restored* by a
                        // soft-close undo that raced this exit, its PTY is dead
                        // now — the registry-based `alive` check let undo bring
                        // back a doomed pane. Tear it back out so it doesn't
                        // linger (and respawn a fresh shell on next render).
                        this.workspace.update(cx, |ws, cx| {
                            for (tid, _) in &exit_events {
                                ws.reap_restored_close(tid, cx);
                            }
                        });
                    }
                    // Notify dirty terminal content panes directly (batched in one update).
                    // All notifications happen in the same GPUI update → single layout pass.
                    // Each terminal_id may be rendered by multiple panes (one per window
                    // whose visible set includes its host project), so iterate the vec
                    // and prune dead weaks lazily.
                    if !dirty_terminal_ids.is_empty() {

                        let mut registry = crate::views::window::content_pane_registry().lock();
                        let mut any_local_pane = false;
                        for tid in &dirty_terminal_ids {
                            let now_empty = if let Some(weaks) = registry.get_mut(tid) {
                                if crate::views::window::notify_pane_weaks(weaks, cx) {
                                    any_local_pane = true;
                                }
                                weaks.is_empty()
                            } else {
                                false
                            };
                            if now_empty {
                                registry.remove(tid);
                            }
                        }
                        drop(registry);
                        // Remote-only terminals have no local content pane. Without
                        // cx.notify(), GPUI's draw cycle won't run and the event loop
                        // effectively stalls. Notify main_window to keep GPUI responsive
                        // for bridge commands, state queries, and other remote work.
                        if !any_local_pane {
                            this.main_window.update(cx, |_, cx| cx.notify());
                        }
                    }

                    // Drain OSC 9 / OSC 777 notifications for terminals that
                    // produced output this batch and raise OS notifications
                    // for background panes. Runs here (not in a pane's render)
                    // so background tabs and detached windows are covered too.
                    if !dirty_terminal_ids.is_empty() {
                        this.process_terminal_notifications(&dirty_terminal_ids, cx);
                        // Stamp project activity for any command that finished
                        // this batch (OSC 133 ;D), independent of bell/OSC alerts.
                        this.process_command_finished_activity(&dirty_terminal_ids, cx);
                    }

                    if !exit_events.is_empty() {
                        // A terminal exited — every window rendering its
                        // project column needs to re-render so the layout
                        // reflects the removal. Fan out to all live windows.
                        this.main_window.update(cx, |_, cx| cx.notify());
                        for view in this.extra_windows.values() {
                            view.update(cx, |_, cx| cx.notify());
                        }
                    }
                });

                // Cooperatively yield to the executor between drain passes so
                // input, rendering, resize, and other terminals' parsing get
                // scheduled even under a sustained high-bandwidth stream. The
                // next recv().await picks up any events left in the channel, so
                // the loop always makes progress and nothing is dropped.
                smol::future::yield_now().await;
            }
        })
        .detach();
    }

}

impl Velowork {
    /// Lock the app (show lock screen overlay).
    pub fn lock_app(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Without a master password the lock screen can never be unlocked, so
        // refuse to lock.
        if !security::is_master_password_set() {
            return;
        }
        if let Ok(mut svc) = security::current_security_service() {
            svc.lock();
        }

        // Safely close and re-attach all detached standalone OS windows (terminals, dock panels, settings, logs).
        // This invokes each window's `on_close` handler to re-attach detached terminals and dock
        // panels back into the main window layout before removing the OS windows.
        crate::app::detached_overlays::close_all_detached_windows(cx);
        self.opened_detached_windows.clear();

        // Dismiss every interactive floating surface (modals, dropdown panels,
        // popovers, context menus, ...) BEFORE showing the lock screen. Any
        // open surface that renders as a topmost `anchored` layer (e.g. a
        // "forward type" select dropdown) would otherwise stay visible behind
        // the lock screen and leak information or leave a visual remnant. The
        // underlying editor state (e.g. the lock-screen-settings edit dialog)
        // lives in the main window subtree, not in a floating surface, so it is
        // preserved and restored intact after unlocking.
        self.main_window.update(cx, |w, cx| {
            w.set_locked(true, cx);
            w.close_all_overlays(window, cx);
        });
        for extra in self.extra_windows.values() {
            extra.update(cx, |w, cx| {
                w.set_locked(true, cx);
            });
        }
        if self.lock_screen.is_none() {
            let ls = cx.new(|cx| crate::views::overlays::lock_screen::LockScreen::new(cx));
            // Subscribe to the new lock screen's unlock event
            cx.subscribe(&ls, |this, _, event, cx| {
                let crate::views::overlays::lock_screen::LockScreenEvent::Unlocked = event;
                this.unlock_app(cx);
            }).detach();
            self.lock_screen = Some(ls);
        }
        self.locked = true;
        cx.notify();
    }

    /// Unlock the app (hide lock screen, resume normal operation).
    pub fn unlock_app(&mut self, cx: &mut Context<Self>) {
        self.locked = false;
        self.lock_screen = None;
        self.last_activity = std::time::Instant::now();
        self.main_window
            .update(cx, |w, cx| w.set_locked(false, cx));
        for extra in self.extra_windows.values() {
            extra.update(cx, |w, cx| {
                w.set_locked(false, cx);
            });
        }
        cx.notify();
    }

    /// Reset the idle timer (called on user activity).
    pub fn reset_idle_timer(&mut self) {
        self.last_activity = std::time::Instant::now();
    }

    /// Check if the idle timeout has been exceeded and lock if so.
    pub fn check_idle_timeout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.locked {
            return; // Already locked
        }
        // No master password set — locking would be irreversible, so never
        // trigger the lock screen on idle timeout.
        if !security::is_master_password_set() {
            return;
        }
        let timeout_secs = crate::settings::settings_entity(cx)
            .read(cx)
            .settings
            .security
            .password_timeout_secs;
        if timeout_secs == 0 {
            return; // Timeout disabled
        }
        if self.last_activity.elapsed().as_secs() >= timeout_secs as u64 {
            self.lock_app(window, cx);
        }
    }
}

impl Render for Velowork {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme::theme(cx);
        let settings = &crate::settings::settings_entity(cx).read(cx).settings;
        let is_custom_titlebar = if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
            settings.titlebar_style == velowork_workspace::settings::TitlebarStyle::Custom
        } else {
            matches!(window.window_decorations(), gpui::Decorations::Client { .. })
        };
        let window_corner_radius = settings.window_corner_radius;
        let is_maximized = window.is_maximized();
        let is_fullscreen = window.is_fullscreen();
        let has_rounded_corners = is_custom_titlebar && !is_maximized && !is_fullscreen && window_corner_radius > 0.0;
        let radius = px(window_corner_radius);

        let titlebar_offset = if is_custom_titlebar && (!cfg!(target_os = "macos") || !window.is_fullscreen()) {
            px(velowork_ui::decorations::get_titlebar_height(cx))
        } else {
            px(0.0)
        };

        let base = if self.locked {
            // Show lock screen overlay below custom titlebar and occlude mouse events
            if let Some(ref lock_screen) = self.lock_screen {
                let ls = lock_screen.clone();
                let mut overlay_div = div()
                    .absolute()
                    .top(titlebar_offset)
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .bg(rgb(t.bg_primary))
                    .occlude();

                if has_rounded_corners {
                    overlay_div = overlay_div.rounded_bl(radius).rounded_br(radius);
                }

                // If a system ConfirmDialog is active while locked (e.g. user requested quit with active sessions),
                // render ONLY that confirm dialog on top of the lock screen so the user can confirm exit.
                // Sensitive business modals remain shielded beneath the lock screen.
                let confirm_modal = self.main_window.read(cx).overlay_manager.read(cx).render_confirm_dialog_modal();

                let mut root_div = div()
                    .size_full()
                    .child(self.main_window.clone())
                    .child(overlay_div.child(ls));

                if let Some(modal) = confirm_modal {
                    let mut modal_div = div()
                        .absolute()
                        .top(titlebar_offset)
                        .bottom_0()
                        .left_0()
                        .right_0();
                    if has_rounded_corners {
                        modal_div = modal_div.rounded_bl(radius).rounded_br(radius).overflow_hidden();
                    }
                    root_div = root_div.child(modal_div.child(modal));
                }

                root_div
            } else {
                div().size_full().child(self.main_window.clone())
            }
        } else {
            div().size_full().child(self.main_window.clone())
        };

        // Track user activity for idle lock timeout and register lock action.
        // Mouse/key events on the root element indicate user interaction.
        let entity_for_mouse = cx.entity();
        let entity_for_key = cx.entity();
        base.on_mouse_move(move |_, _window, cx| {
            entity_for_mouse.update(cx, |this, _| {
                if !this.locked {
                    this.last_activity = std::time::Instant::now();
                }
            });
        })
        .on_key_down(move |_, _window, cx| {
            entity_for_key.update(cx, |this, _| {
                if !this.locked {
                    this.last_activity = std::time::Instant::now();
                }
            });
        })
        .on_action(cx.listener(|this, _: &crate::keybindings::LockApp, window, cx| {
            this.lock_app(window, cx);
        }))
        .on_action(cx.listener(|this, _: &crate::keybindings::Quit, window, cx| {
            this.main_window.update(cx, |w, cx| {
                w.request_quit(window, cx);
            });
        }))
        .on_action(cx.listener(|this, _: &crate::keybindings::CloseWindow, window, cx| {
            this.main_window.update(cx, |w, cx| {
                w.request_close_window(window, cx);
            });
        }))
        .on_action(cx.listener(|this, _: &crate::keybindings::AddTab, window, cx| {
            if !this.locked {
                this.main_window.update(cx, |w, cx| {
                    w.handle_global_add_tab(window, cx);
                });
            }
        }))
        .on_action(cx.listener(|this, _: &crate::keybindings::NewSession, _window, cx| {
            if !this.locked {
                this.main_window.update(cx, |w, cx| {
                    w.open_add_session_dialog(cx);
                });
            }
        }))
        .on_action(cx.listener(|this, _: &crate::keybindings::ShowCommandPalette, _window, cx| {
            if !this.locked {
                this.main_window.update(cx, |w, cx| {
                    w.overlay_manager.update(cx, |om, cx| om.toggle_command_palette(cx));
                });
            }
        }))
        .on_action(cx.listener(|this, _: &crate::keybindings::ShowSettings, _window, cx| {
            if !this.locked {
                this.main_window.update(cx, |w, cx| {
                    w.overlay_manager.update(cx, |om, cx| om.toggle_settings_panel(cx));
                });
            }
        }))
        .on_action(cx.listener(|this, _: &crate::keybindings::ShowKeybindings, _window, cx| {
            if !this.locked {
                this.main_window.update(cx, |w, cx| {
                    w.overlay_manager.update(cx, |om, cx| om.toggle_keybindings_help(cx));
                });
            }
        }))
    }
}
