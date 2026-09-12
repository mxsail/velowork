//! Window-level bottom dock (SFTP + Commands).
//!
//! The bottom dock used to live on each `ProjectColumn`, which meant every
//! visible project owned its own `DockPanel`/`sftp`/`commands` entity even
//! though only the focused project's dock was ever rendered, and its content
//! (SFTP/Commands) always follows the *window's* focused terminal — never a
//! specific column. That per-project ownership was therefore an artefact, not
//! a real requirement.
//!
//! This module hoists the whole bottom-dock ecosystem to `WindowView` as a
//! single instance shared across the entire center region. The visual layout
//! is unchanged (still rendered inside `center-column` below the projects
//! grid); only the ownership moved up one level.

use gpui::*;
use std::sync::Arc;

use velowork_i18n::i18n;
use velowork_ui::dock::{
    AnyPanel, DockPanel, DockPanelDetachEvent, DockPanelDetachWholeEvent, DockPanelEvent,
    DockPanelHideEvent, PanelCollapseState, PanelMode, PanelProvider,
};
use velowork_ui::icon::AppIcon;
use velowork_ui::overlay_registry::OverlayRegistry;

use velowork_workspace::dock_controller::{
    ANIMATION_DURATION_MS, AnimationTarget, DockController, FRAME_TIME_MS,
};

use velowork_workspace::stores::{ConnectionEvent, GlobalConnectionStore, GlobalSessionStore};

use crate::app::detached_overlays::{DetachedOverlayOptions, open_detached_overlay};
use crate::terminal::backend::TerminalBackend;
use crate::views::layout::terminal_pane::commands_panel::CommandsPanel;
use crate::views::layout::terminal_pane::sftp_panel::BottomPanel;
use crate::views::overlays::detached_overlay::{DetachedHost, DetachedHostCloseEvent};
use crate::workspace::focus::FocusManager;
use crate::workspace::state::{LayoutNode, Workspace};

use super::{TerminalsRegistry, WindowView};

/// Build the window-level bottom dock together with its SFTP and Commands
/// panels, wiring every subscription (dock events, detach, connection changes)
/// onto the owning `WindowView`. Returns the three entities so `WindowView::new`
/// can store them as fields.
///
/// The SFTP and Commands panels bind to the *focused* terminal (resolved via
/// the shared `FocusManager` + `TerminalsRegistry`), so a single shared set is
/// correct — there is no per-project state to preserve.
pub(super) fn build_bottom_dock(
    workspace: Entity<Workspace>,
    focus_manager: Entity<FocusManager>,
    terminals: TerminalsRegistry,
    backend: Arc<dyn TerminalBackend>,
    overlay_registry: Entity<OverlayRegistry>,
    cx: &mut Context<WindowView>,
) -> (
    Entity<DockPanel>,
    Entity<BottomPanel>,
    Entity<CommandsPanel>,
) {
    // Self-contained commands panel. Terminal-independent: it broadcasts bash to
    // sessions resolved from the shared `TerminalsRegistry` + focused terminal.
    let commands_panel = {
        let ts = terminals.clone();
        let ws = workspace.clone();
        let fm = focus_manager.clone();
        let overlay_reg = overlay_registry.clone();
        cx.new(move |cx| CommandsPanel::new(ts, ws, fm, Some(overlay_reg.clone()), cx))
    };
    cx.observe(&commands_panel, |_, _, cx| cx.notify()).detach();

    // Self-contained SFTP panel that dynamically binds to the active terminal's
    // live SSH session.
    let sftp_panel = {
        let ws = workspace.clone();
        let fm = focus_manager.clone();
        let be = backend.clone();
        let cp = commands_panel.clone();
        let ts = terminals.clone();
        cx.new(move |cx| BottomPanel::new(ws, fm, be, cp, ts, cx))
    };

    // Bottom DockPanel hosting SFTP and Commands tabs. Uses the same DockPanel
    // container as the right sidebar for consistent structure.
    let bottom_dock = {
        let sftp = sftp_panel.clone();
        let cmd = commands_panel.clone();
        cx.new(move |cx| {
            let mut dp = DockPanel::new("bottom_dock_panel", 240.0, cx);
            dp.set_header_mode(velowork_ui::dock::DockHeaderMode::Tabs);
            dp.set_resize_edge(velowork_ui::dock::ResizeEdge::Top, cx);
            dp.set_panel_providers(vec![
                PanelProvider {
                    id: "sftp".to_string(),
                    title: i18n!(cx, "sftp.panel.title"),
                    icon: AppIcon::Folder,
                },
                PanelProvider {
                    id: "commands".to_string(),
                    title: i18n!(cx, "sftp.commands.title"),
                    icon: AppIcon::CommandAction,
                },
            ]);
            let sftp2 = sftp.clone();
            let cmd2 = cmd.clone();
            dp.set_on_request_panel(move |id, _window, _cx| match id {
                "sftp" => AnyPanel::new(sftp2.clone()),
                "commands" => AnyPanel::new(cmd2.clone()),
                _ => panic!("unknown panel id: {}", id),
            });
            // Pre-add Commands tab so toggle methods can select it without
            // window access. SFTP tab is added dynamically when an SSH
            // connection with enable_sftp becomes active. Start fully hidden —
            // the dock only appears when the user toggles SFTP or Commands.
            dp.add_tab(AnyPanel::new(cmd.clone()), cx);
            dp.collapse_state = PanelCollapseState::Hidden;
            dp
        })
    };
    cx.observe(&bottom_dock, |_, _, cx| cx.notify()).detach();

    // Hand the overlay registry to the bottom dock + SFTP panel so their menus
    // / dialogs register for centralized click-outside dismissal.
    bottom_dock.update(cx, |dp, _cx| {
        dp.set_overlay_registry(overlay_registry.clone())
    });
    sftp_panel.update(cx, |sp, _cx| {
        sp.set_overlay_registry(overlay_registry.clone())
    });

    // Re-render the window when the bottom dock state changes & keep height synced.
    cx.subscribe(&bottom_dock, |this, dock, _event: &DockPanelEvent, cx| {
        let height = dock.read(cx).size;
        this.bottom_dock_ctrl.set_width(height);
        // Keep `sftp_tab_added` in sync: if the SFTP tab was removed (e.g. the
        // user closed its tab via the dock's X button), reset the flag so the
        // status-bar "Files" button can re-add it. Without this, the button's
        // `ensure_sftp_tab` early-returns and nothing reappears.
        if this.sftp_tab_added
            && !dock
                .read(cx)
                .tabs
                .iter()
                .any(|t| t.metadata(cx).id.0 == "sftp")
        {
            this.sftp_tab_added = false;
        }
        cx.notify();
    })
    .detach();

    // Hide panel event from context menu / button: trigger smooth close animation.
    cx.subscribe(&bottom_dock, |this, _, _event: &DockPanelHideEvent, cx| {
        this.animate_bottom_dock_to(AnimationTarget::Close, cx);
    })
    .detach();

    // DockPanelDetachEvent — open a single tab in its own OS window.
    cx.subscribe(&bottom_dock, {
        let bottom_dock_for_detach = bottom_dock.clone();
        move |this, _, event: &DockPanelDetachEvent, cx| {
            let panel_id = event.id.clone();
            let main_dock = bottom_dock_for_detach.clone();
            spawn_detached_bottom_tab_window(
                panel_id.clone(),
                main_dock,
                this.workspace.clone(),
                this.focus_manager.clone(),
                this.terminals.clone(),
                this.backend.clone(),
                this.overlay_registry.clone(),
                cx,
            );
            bottom_dock_for_detach.update(cx, |d, cx| {
                d.remove_tab_by_id(&panel_id, cx);
                if d.tabs.is_empty() {
                    // The detached tab was the only one: close the dock instead
                    // of leaving an empty (blank) container behind.
                    cx.emit(DockPanelEvent);
                } else {
                    // Keep the remaining tabs visible at their normal state.
                    d.collapse_state = PanelCollapseState::Normal;
                    cx.emit(DockPanelEvent);
                }
                cx.notify();
            });
            // Animate: collapse the dock when it's now empty, otherwise reveal
            // the remaining tabs. This avoids the blank gap left by simply
            // hiding the panel.
            if bottom_dock_for_detach.read(cx).tabs.is_empty() {
                this.animate_bottom_dock_to(AnimationTarget::Close, cx);
            } else {
                this.animate_bottom_dock_to(AnimationTarget::Open, cx);
            }
            cx.notify();
        }
    })
    .detach();

    // DockPanelDetachWholeEvent — open the currently visible tabs in a detached
    // window, mirroring exactly what was shown in the original dock.
    cx.subscribe(&bottom_dock, {
        let bottom_dock_w = bottom_dock.clone();
        move |this, _, event: &DockPanelDetachWholeEvent, cx| {
            let tab_ids: Vec<String> = event
                .tabs
                .iter()
                .map(|t| t.metadata(cx).id.0.clone())
                .collect();
            let active_tab_index = event.active_tab_index;
            let main_dock = bottom_dock_w.clone();
            spawn_detached_bottom_dock_window(
                tab_ids.clone(),
                active_tab_index,
                main_dock,
                this.workspace.clone(),
                this.focus_manager.clone(),
                this.terminals.clone(),
                this.backend.clone(),
                this.overlay_registry.clone(),
                cx,
            );
            bottom_dock_w.update(cx, |d, cx| {
                // Remove every detached tab so the original dock is left empty
                // rather than an unusable blank container.
                for id in tab_ids.iter() {
                    d.remove_tab_by_id(id, cx);
                }
                cx.emit(DockPanelEvent);
                cx.notify();
            });
            // Close the now-empty dock with the smooth collapse animation.
            this.animate_bottom_dock_to(AnimationTarget::Close, cx);
            cx.notify();
        }
    })
    .detach();

    // Auto add/remove the SFTP tab when SSH sessions with enable_sftp connect
    // or disconnect.
    {
        let connection_store = cx.global::<GlobalConnectionStore>().0.clone();
        cx.subscribe(
            &connection_store,
            |this, store, event: &ConnectionEvent, cx| {
                match event {
                    ConnectionEvent::Connected(_) | ConnectionEvent::Disconnected(_) => {
                        this.schedule_sync_sftp_tab(cx);
                    }
                }
                let _ = store;
            },
        )
        .detach();
    }

    (bottom_dock, sftp_panel, commands_panel)
}

impl WindowView {
    /// Whether the window-level bottom dock is in maximized or fullscreen mode.
    /// Used by render to hide the left sidebar and right dock when the bottom
    /// dock expands to fill the screen.
    pub(super) fn is_bottom_dock_fullscreen(&self, cx: &App) -> bool {
        let mode = self.bottom_dock.read(cx).mode;
        mode == PanelMode::Maximized || mode == PanelMode::Fullscreen
    }

    /// Show (expand) the bottom dock. Used when a detached panel is re-attached
    /// back into the main bottom dock: adding the tab alone leaves the dock's
    /// controller collapsed, so we must also open + animate it.
    pub(super) fn show_bottom_dock(&mut self, cx: &mut Context<Self>) {
        self.bottom_dock.update(cx, |d, cx| {
            d.collapse_state = PanelCollapseState::Normal;
            cx.emit(DockPanelEvent);
            cx.notify();
        });
        self.bottom_dock_ctrl.set_open(true);
        let dock_size = self.bottom_dock.read(cx).size;
        self.bottom_dock_ctrl.set_width(dock_size);
        self.animate_bottom_dock_to(AnimationTarget::Open, cx);
    }

    /// Hide (collapse) the bottom dock.
    pub(super) fn hide_bottom_dock(&mut self, cx: &mut Context<Self>) {
        self.focus_manager.update(cx, |fm, _| {
            fm.request_focus(velowork_workspace::focus::FocusLayer::Terminal);
        });
        self.animate_bottom_dock_to(AnimationTarget::Close, cx);
    }

    /// Animate bottom dock to target if needed
    pub(super) fn animate_bottom_dock_to(
        &mut self,
        target: AnimationTarget,
        cx: &mut Context<Self>,
    ) {
        match target {
            AnimationTarget::Open => {
                self.bottom_dock_ctrl.set_open(true);
            }
            AnimationTarget::Close => {
                self.bottom_dock_ctrl.set_open(false);
            }
            AnimationTarget::None => {}
        }
        if let Some(target_value) = target.value() {
            self.animate_bottom_dock(target_value, cx);
        }
    }

    /// Animate bottom dock height to target value (0.0 = collapsed, 1.0 = expanded)
    pub(super) fn animate_bottom_dock(&mut self, target: f32, cx: &mut Context<Self>) {
        let current = self.bottom_dock_ctrl.animation();

        // Skip animation if already at target
        if (current - target).abs() < 0.01 {
            self.bottom_dock_ctrl.set_animation(target);
            self.bottom_dock_anim_task = None;
            if target == 0.0 {
                self.bottom_dock.update(cx, |dock, cx| {
                    dock.collapse_state = PanelCollapseState::Hidden;
                    cx.emit(DockPanelEvent);
                });
            }
            cx.notify();
            return;
        }

        let duration = std::time::Duration::from_millis(ANIMATION_DURATION_MS);
        let step_duration = std::time::Duration::from_millis(FRAME_TIME_MS);

        let task = cx.spawn(async move |this: WeakEntity<WindowView>, cx| {
            let start = std::time::Instant::now();
            loop {
                smol::Timer::after(step_duration).await;
                let elapsed = start.elapsed();
                let ratio = (elapsed.as_secs_f32() / duration.as_secs_f32()).min(1.0);
                let progress = DockController::ease_progress_ratio(current, target, ratio);

                let result = this.update(cx, |this, cx| {
                    this.bottom_dock_ctrl.set_animation(progress);
                    cx.notify();
                });
                if result.is_err() || ratio >= 1.0 {
                    break;
                }
            }

            // Ensure we reach target exactly
            let _ = this.update(cx, |this, cx| {
                this.bottom_dock_ctrl.set_animation(target);
                if target == 0.0 {
                    this.bottom_dock.update(cx, |dock, cx| {
                        dock.collapse_state = PanelCollapseState::Hidden;
                        cx.notify();
                    });
                }
                cx.notify();
            });
        });
        self.bottom_dock_anim_task = Some(task);
    }

    /// Toggle the bottom "Commands" panel.
    pub(crate) fn toggle_commands(&mut self, cx: &mut Context<Self>) {
        let is_currently_open =
            self.bottom_dock_ctrl.is_open() && self.bottom_dock_ctrl.animation() > 0.01;
        let commands_active = self
            .bottom_dock
            .read(cx)
            .active_tab()
            .map_or(false, |t| t.metadata(cx).id.0 == "commands");

        let target = if commands_active && is_currently_open {
            if self.bottom_dock_ctrl.is_open() {
                self.bottom_dock_ctrl.toggle();
            }
            AnimationTarget::Close
        } else {
            let commands_panel = self.commands_panel.clone();
            self.bottom_dock.update(cx, |dock, cx| {
                // Re-add the commands tab if it was previously closed via its
                // tab X button, otherwise the dock opens empty and nothing shows.
                if dock
                    .tabs
                    .iter()
                    .position(|t| t.metadata(cx).id.0 == "commands")
                    .is_none()
                {
                    dock.add_tab(AnyPanel::new(commands_panel), cx);
                }
                if let Some(idx) = dock
                    .tabs
                    .iter()
                    .position(|t| t.metadata(cx).id.0 == "commands")
                {
                    dock.select_tab(idx, cx);
                }
                dock.collapse_state = PanelCollapseState::Normal;
                cx.notify();
            });
            if !self.bottom_dock_ctrl.is_open() {
                self.bottom_dock_ctrl.toggle();
            }
            AnimationTarget::Open
        };

        let dock_size = self.bottom_dock.read(cx).size;
        self.bottom_dock_ctrl.set_width(dock_size);
        self.animate_bottom_dock_to(target, cx);
    }

    /// Open and expand the bottom Commands panel with prefilled command text.
    pub(crate) fn open_commands_with(&mut self, cmd: &str, cx: &mut Context<Self>) {
        let commands_panel = self.commands_panel.clone();
        commands_panel.update(cx, |cp, cx| {
            cp.open_with_command(cmd, cx);
        });
        self.bottom_dock.update(cx, |dock, cx| {
            if dock
                .tabs
                .iter()
                .position(|t| t.metadata(cx).id.0 == "commands")
                .is_none()
            {
                dock.add_tab(velowork_ui::dock::AnyPanel::new(commands_panel), cx);
            }
            if let Some(idx) = dock
                .tabs
                .iter()
                .position(|t| t.metadata(cx).id.0 == "commands")
            {
                dock.select_tab(idx, cx);
            }
            dock.collapse_state = velowork_ui::dock::PanelCollapseState::Normal;
            cx.notify();
        });
        if !self.bottom_dock_ctrl.is_open() {
            self.bottom_dock_ctrl.toggle();
        }
        let dock_size = self.bottom_dock.read(cx).size;
        self.bottom_dock_ctrl.set_width(dock_size);
        self.animate_bottom_dock_to(AnimationTarget::Open, cx);
    }

    /// Toggle the bottom "SFTP" panel.
    pub(crate) fn toggle_sftp(&mut self, cx: &mut Context<Self>) {
        self.ensure_sftp_tab(cx);
        let is_currently_open =
            self.bottom_dock_ctrl.is_open() && self.bottom_dock_ctrl.animation() > 0.01;
        let sftp_active = self
            .bottom_dock
            .read(cx)
            .active_tab()
            .map_or(false, |t| t.metadata(cx).id.0 == "sftp");

        let target = if sftp_active && is_currently_open {
            if self.bottom_dock_ctrl.is_open() {
                self.bottom_dock_ctrl.toggle();
            }
            AnimationTarget::Close
        } else {
            self.bottom_dock.update(cx, |dock, cx| {
                if let Some(idx) = dock.tabs.iter().position(|t| t.metadata(cx).id.0 == "sftp") {
                    dock.select_tab(idx, cx);
                }
                dock.collapse_state = PanelCollapseState::Normal;
                cx.notify();
            });
            if !self.bottom_dock_ctrl.is_open() {
                self.bottom_dock_ctrl.toggle();
            }
            AnimationTarget::Open
        };

        let dock_size = self.bottom_dock.read(cx).size;
        self.bottom_dock_ctrl.set_width(dock_size);
        self.animate_bottom_dock_to(target, cx);
    }

    /// Ensure the SFTP tab is present in the bottom dock (add it if not).
    fn ensure_sftp_tab(&mut self, cx: &mut Context<Self>) {
        if self.sftp_tab_added {
            return;
        }
        let sftp = self.sftp_panel.clone();
        self.bottom_dock.update(cx, |dock, cx| {
            dock.add_tab(AnyPanel::new(sftp), cx);
        });
        self.sftp_tab_added = true;
    }

    /// Schedule a debounced (150ms) SFTP tab visibility sync. Cancels any
    /// previously scheduled timer if the user rapidly switches focus.
    pub(super) fn schedule_sync_sftp_tab(&mut self, cx: &mut Context<Self>) {
        self.sftp_sync_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(150))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.sync_sftp_tab(cx);
            });
        }));
    }

    /// Sync the SFTP tab visibility based on the focused connection's settings.
    /// The SFTP tab is shown ONLY when the focused terminal is an SSH session
    /// with `enable_sftp: true`.
    fn sync_sftp_tab(&mut self, cx: &mut Context<Self>) {
        let focused_id = self.dock_focused_terminal_id(cx);
        let show = if let Some(ref tid) = focused_id {
            if let Some(sid) = self.backend.get_ssh_session_id(tid) {
                let session_store = cx.global::<GlobalSessionStore>().0.read(cx);
                session_store
                    .find_session(&sid)
                    .map_or(false, |s| s.enable_sftp)
            } else {
                false
            }
        } else {
            self.sftp_tab_added
        };

        if show {
            self.ensure_sftp_tab(cx);
        } else if self.sftp_tab_added {
            self.bottom_dock.update(cx, |dock, cx| {
                dock.remove_tab_by_id("sftp", cx);
            });
            self.sftp_tab_added = false;
        }
    }

    /// The terminal id that currently holds focus, resolved from the focus
    /// manager → project → layout node. `None` if no terminal is focused.
    fn dock_focused_terminal_id(&self, cx: &App) -> Option<String> {
        let focused = self.focus_manager.read(cx).focused_terminal_state()?;
        let project = self.workspace.read(cx).project(&focused.project_id)?;
        let layout = project.layout.as_ref()?;
        match layout.get_at_path(&focused.layout_path) {
            Some(LayoutNode::Terminal {
                terminal_id: Some(id),
                ..
            }) => Some(id.clone()),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Detached bottom-dock windows.
//
// Mirrors the right-sidebar detach flow. Each function opens a fresh OS window,
// creates brand-new panel entities in that window's context (entities are
// window-scoped and cannot cross window boundaries), and wires `on_attach` /
// `on_close` so the panel can be re-docked into the original bottom dock.
// ---------------------------------------------------------------------------

/// Detach a single tab (identified by its business panel id) into its own OS window.
#[allow(clippy::too_many_arguments)]
fn spawn_detached_bottom_tab_window(
    panel_id: String,
    main_dock: Entity<DockPanel>,
    workspace: Entity<Workspace>,
    focus_manager: Entity<FocusManager>,
    terminals: TerminalsRegistry,
    backend: Arc<dyn TerminalBackend>,
    overlay_registry: Entity<OverlayRegistry>,
    cx: &mut Context<WindowView>,
) {
    let main_dock_for_peer = main_dock.clone();
    let is_sftp = panel_id == "sftp";

    let window_title: SharedString = if is_sftp {
        "SFTP - Detached".into()
    } else {
        "Commands - Detached".into()
    };

    let reattach_build = {
        let workspace = workspace.clone();
        let focus_manager = focus_manager.clone();
        let terminals = terminals.clone();
        let backend = backend.clone();
        let main_dock = main_dock.clone();
        let overlay_registry = overlay_registry.clone();
        let panel_id = panel_id.clone();
        let window_view = cx.entity().downgrade();
        Arc::new(move |_pid: &str, _window: &mut Window, cx: &mut App| {
            let workspace_r = workspace.clone();
            let focus_manager_r = focus_manager.clone();
            let terminals_r = terminals.clone();
            let backend_r = backend.clone();
            let main_dock_r = main_dock.clone();
            let overlay_registry_r = overlay_registry.clone();
            let reattach_id = panel_id.clone();

            let _ = main_dock_r.update(cx, |dock, cx| {
                let cmd = cx.new(|cx| {
                    CommandsPanel::new(
                        terminals_r.clone(),
                        workspace_r.clone(),
                        focus_manager_r.clone(),
                        Some(overlay_registry_r.clone()),
                        cx,
                    )
                });
                let sftp = cx.new(|cx| {
                    BottomPanel::new(
                        workspace_r.clone(),
                        focus_manager_r.clone(),
                        backend_r.clone(),
                        cmd.clone(),
                        terminals_r.clone(),
                        cx,
                    )
                });
                sftp.update(cx, |sp, _cx| {
                    sp.set_overlay_registry(overlay_registry.clone())
                });
                if reattach_id == "sftp" {
                    dock.add_tab(AnyPanel::new(sftp), cx);
                } else {
                    dock.add_tab(AnyPanel::new(cmd), cx);
                }
                dock.collapse_state = PanelCollapseState::Normal;
                cx.emit(DockPanelEvent);
                cx.notify();
            });
            // Re-attach: expand the main bottom dock so the re-added tab is visible.
            if let Some(wv) = window_view.upgrade() {
                wv.update(cx, |wv, cx| wv.show_bottom_dock(cx));
            }
        })
    };
    let reattach_close = reattach_build.clone();

    let _ = open_detached_overlay::<DetachedHost<DockPanel>, DetachedHostCloseEvent>(
        window_title,
        move |_window, overlay_registry, cx| {
            let commands_detached = cx.new(|cx| {
                CommandsPanel::new(
                    terminals.clone(),
                    workspace.clone(),
                    focus_manager.clone(),
                    Some(overlay_registry.clone()),
                    cx,
                )
            });
            let sftp_detached = cx.new(|cx| {
                BottomPanel::new(
                    workspace.clone(),
                    focus_manager.clone(),
                    backend.clone(),
                    commands_detached.clone(),
                    terminals.clone(),
                    cx,
                )
            });
            sftp_detached.update(cx, |sp, _cx| {
                sp.set_overlay_registry(overlay_registry.clone())
            });

            let detached_dock = cx.new(|cx| {
                let mut dp = DockPanel::new("detached_bottom_tab_dock", 400.0, cx);
                dp.set_panel_providers(vec![
                    PanelProvider {
                        id: "sftp".to_string(),
                        title: i18n!(cx, "sftp.panel.title"),
                        icon: AppIcon::Folder,
                    },
                    PanelProvider {
                        id: "commands".to_string(),
                        title: i18n!(cx, "sftp.commands.title"),
                        icon: AppIcon::CommandAction,
                    },
                ]);
                let sftp2 = sftp_detached.clone();
                let cmd2 = commands_detached.clone();
                dp.set_on_request_panel(move |id, _window, _cx| match id {
                    "sftp" => AnyPanel::new(sftp2.clone()),
                    "commands" => AnyPanel::new(cmd2.clone()),
                    _ => panic!("unknown panel id: {}", id),
                });
                if is_sftp {
                    dp.add_tab(AnyPanel::new(sftp_detached), cx);
                } else {
                    dp.add_tab(AnyPanel::new(commands_detached), cx);
                }
                dp.fill_window = true;
                dp.set_overlay_registry(overlay_registry.clone());
                dp.set_peer_docks(vec![main_dock_for_peer.downgrade()]);
                dp
            });

            {
                let reattach = reattach_build.clone();
                let _ = detached_dock.update(cx, |dp, _cx| {
                    dp.set_on_attach(move |pid, window, cx| reattach(pid, window, cx));
                });
            }

            cx.new(|_cx| DetachedHost {
                inner: detached_dock,
            })
        },
        DetachedOverlayOptions {
            size: gpui::size(gpui::px(580.0), gpui::px(600.0)),
            min_size: gpui::Size {
                width: gpui::px(300.0),
                height: gpui::px(300.0),
            },
            on_close: Some(Arc::new(move |window, cx| reattach_close("", window, cx))),
            hide_titlebar: true,
        },
        cx,
    );
}

/// Detach the currently visible tabs into a single detached OS window.
#[allow(clippy::too_many_arguments)]
fn spawn_detached_bottom_dock_window(
    tab_ids: Vec<String>,
    active_tab_index: Option<usize>,
    main_dock: Entity<DockPanel>,
    workspace: Entity<Workspace>,
    focus_manager: Entity<FocusManager>,
    terminals: TerminalsRegistry,
    backend: Arc<dyn TerminalBackend>,
    overlay_registry: Entity<OverlayRegistry>,
    cx: &mut Context<WindowView>,
) {
    let main_dock_for_peer = main_dock.clone();

    let reattach_build = {
        let workspace = workspace.clone();
        let focus_manager = focus_manager.clone();
        let terminals = terminals.clone();
        let backend = backend.clone();
        let main_dock = main_dock.clone();
        let overlay_registry = overlay_registry.clone();
        let tab_ids = tab_ids.clone();
        let window_view = cx.entity().downgrade();
        Arc::new(move |_pid: &str, _window: &mut Window, cx: &mut App| {
            let workspace_r = workspace.clone();
            let focus_manager_r = focus_manager.clone();
            let terminals_r = terminals.clone();
            let backend_r = backend.clone();
            let main_dock_r = main_dock.clone();
            let overlay_registry_r = overlay_registry.clone();
            let reattach_ids = tab_ids.clone();

            let _ = main_dock_r.update(cx, |dock, cx| {
                let cmd = cx.new(|cx| {
                    CommandsPanel::new(
                        terminals_r.clone(),
                        workspace_r.clone(),
                        focus_manager_r.clone(),
                        Some(overlay_registry_r.clone()),
                        cx,
                    )
                });
                let sftp = cx.new(|cx| {
                    BottomPanel::new(
                        workspace_r.clone(),
                        focus_manager_r.clone(),
                        backend_r.clone(),
                        cmd.clone(),
                        terminals_r.clone(),
                        cx,
                    )
                });
                sftp.update(cx, |sp, _cx| {
                    sp.set_overlay_registry(overlay_registry.clone())
                });
                for tab_id in &reattach_ids {
                    match tab_id.as_str() {
                        "sftp" => dock.add_tab(AnyPanel::new(sftp.clone()), cx),
                        "commands" => dock.add_tab(AnyPanel::new(cmd.clone()), cx),
                        _ => {}
                    }
                }
                dock.collapse_state = PanelCollapseState::Normal;
                cx.emit(DockPanelEvent);
                cx.notify();
            });
            // Re-attach: expand the main bottom dock so the re-added tabs are visible.
            if let Some(wv) = window_view.upgrade() {
                wv.update(cx, |wv, cx| wv.show_bottom_dock(cx));
            }
        })
    };
    let reattach_close = reattach_build.clone();

    let _ = open_detached_overlay::<DetachedHost<DockPanel>, DetachedHostCloseEvent>(
        "Detached Panel",
        move |_window, overlay_registry, cx| {
            let commands_detached = cx.new(|cx| {
                CommandsPanel::new(
                    terminals.clone(),
                    workspace.clone(),
                    focus_manager.clone(),
                    Some(overlay_registry.clone()),
                    cx,
                )
            });
            let sftp_detached = cx.new(|cx| {
                BottomPanel::new(
                    workspace.clone(),
                    focus_manager.clone(),
                    backend.clone(),
                    commands_detached.clone(),
                    terminals.clone(),
                    cx,
                )
            });
            sftp_detached.update(cx, |sp, _cx| {
                sp.set_overlay_registry(overlay_registry.clone())
            });

            let detached_dock = cx.new(|cx| {
                let mut dp = DockPanel::new("detached_bottom_whole_dock", 400.0, cx);
                dp.set_panel_providers(vec![
                    PanelProvider {
                        id: "sftp".to_string(),
                        title: i18n!(cx, "sftp.panel.title"),
                        icon: AppIcon::Folder,
                    },
                    PanelProvider {
                        id: "commands".to_string(),
                        title: i18n!(cx, "sftp.commands.title"),
                        icon: AppIcon::CommandAction,
                    },
                ]);
                let sftp2 = sftp_detached.clone();
                let cmd2 = commands_detached.clone();
                dp.set_on_request_panel(move |id, _window, _cx| match id {
                    "sftp" => AnyPanel::new(sftp2.clone()),
                    "commands" => AnyPanel::new(cmd2.clone()),
                    _ => panic!("unknown panel id: {}", id),
                });
                for tab_id in &tab_ids {
                    match tab_id.as_str() {
                        "sftp" => dp.add_tab(AnyPanel::new(sftp_detached.clone()), cx),
                        "commands" => dp.add_tab(AnyPanel::new(commands_detached.clone()), cx),
                        _ => {}
                    }
                }
                if let Some(idx) = active_tab_index {
                    if idx < dp.tabs.len() {
                        dp.select_tab(idx, cx);
                    }
                }
                dp.fill_window = true;
                dp.set_overlay_registry(overlay_registry.clone());
                dp.set_peer_docks(vec![main_dock_for_peer.downgrade()]);
                dp
            });

            {
                let reattach = reattach_build.clone();
                let _ = detached_dock.update(cx, |dp, _cx| {
                    dp.set_on_attach(move |pid, window, cx| reattach(pid, window, cx));
                });
            }

            cx.new(|_cx| DetachedHost {
                inner: detached_dock,
            })
        },
        DetachedOverlayOptions {
            size: gpui::size(gpui::px(580.0), gpui::px(600.0)),
            min_size: gpui::Size {
                width: gpui::px(300.0),
                height: gpui::px(300.0),
            },
            on_close: Some(Arc::new(move |window, cx| reattach_close("", window, cx))),
            hide_titlebar: true,
        },
        cx,
    );
}
